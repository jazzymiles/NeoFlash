use super::*;
use crate::ObjectEvent;

#[test]
fn non_looping_preview_stops_at_last_frame_and_can_resume_looping() {
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 3;
    let root = project.root.clone();
    let mut preview = AuthoredPlayback::new(&project, &root, 0);

    preview.set_looping(false);
    preview.tick(&project);
    preview.tick(&project);
    preview.tick(&project);
    assert_eq!(preview.frame(), 2);

    preview.set_looping(true);
    preview.tick(&project);
    assert_eq!(preview.frame(), 0);
}

#[test]
fn demo_playback_emits_traces_and_supports_live_script_mode() {
    let project = Project::demo();
    let root = project.root.clone();
    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert_eq!(
        preview.drain_traces(),
        vec!["NeoFlash: the stage is ready".to_string()]
    );

    let orbit_id = project
        .symbol(&root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .find(|e| e.name == "mainOrbit")
        .unwrap()
        .id
        .clone();

    preview.click(&project, std::slice::from_ref(&orbit_id));
    assert_eq!(preview.drain_traces(), vec!["Orbit rotated".to_string()]);
}

#[test]
fn classic_preview_ignores_initial_tick_and_click_scripts() {
    let mut project = Project::demo();
    let root = project.root.clone();
    project
        .symbol_mut(&root)
        .unwrap()
        .timeline
        .scripts
        .push(crate::FrameScript {
            layer: None,
            frame: 1,
            script: crate::Script {
                language: crate::ScriptLanguage::JavaScript,
                source: "trace(\"tick\"); this.x = 99;".into(),
            },
        });
    let orbit_id = project
        .symbol(&root)
        .unwrap()
        .timeline
        .layers
        .iter()
        .flat_map(|layer| &layer.keyframes)
        .flat_map(|keyframe| &keyframe.elements)
        .find(|element| element.name == "mainOrbit")
        .unwrap()
        .id
        .clone();

    let mut preview = AuthoredPlayback::new(&project, &root, 0);
    assert!(preview.drain_traces().is_empty());
    assert!(preview.vars.is_empty());
    assert!(preview.player.overrides.is_empty());

    preview.click(&project, std::slice::from_ref(&orbit_id));
    assert!(preview.drain_traces().is_empty());
    assert!(preview.vars.is_empty());
    assert!(preview.player.overrides.is_empty());

    preview.tick(&project);
    assert_eq!(preview.frame(), 1);
    assert!(preview.drain_traces().is_empty());
    assert!(preview.vars.is_empty());
    assert!(preview.player.overrides.is_empty());
}

#[test]
fn disabling_live_scripts_clears_runtime_state_and_resumes_root() {
    let mut project = Project::demo();
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.scripts = vec![
        crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::JavaScript,
                source: "var count = 1; this.x = 12; stop(); trace(\"live\");".into(),
            },
        },
        crate::FrameScript {
            layer: None,
            frame: 1,
            script: crate::Script {
                language: crate::ScriptLanguage::JavaScript,
                source: "trace(\"ignored after toggle\"); this.x = 99;".into(),
            },
        },
    ];

    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert!(preview.is_root_halted_by_script());
    assert!(!preview.vars.is_empty());
    assert!(!preview.player.overrides.is_empty());

    preview.set_live_scripts(&project, false);
    assert!(!preview.is_root_halted_by_script());
    assert!(preview.vars.is_empty());
    assert!(preview.player.overrides.is_empty());
    assert_eq!(preview.drain_traces(), vec!["live".to_string()]);

    preview.tick(&project);
    assert_eq!(preview.frame(), 1);
    assert!(preview.drain_traces().is_empty());
    assert!(preview.vars.is_empty());
    assert!(preview.player.overrides.is_empty());
}

#[test]
fn switching_live_scripts_rebuilds_a_stopped_child_clock() {
    let mut project = Project::blank();
    let root = project.root.clone();
    let mut child = project.symbol(&root).unwrap().clone();
    child.id = "live-toggle-child".into();
    child.name = "Live toggle child".into();
    child.timeline.frame_count = 3;
    child.timeline.layers[0].id = "live-toggle-child-layer".into();
    child.timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: crate::ScriptLanguage::JavaScript,
            source: "stop();".into(),
        },
    }];
    project.symbols.push(child);
    project.symbol_mut(&root).unwrap().timeline.frame_count = 1;
    project.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0].elements =
        vec![crate::Element {
            id: "live-toggle-instance".into(),
            name: "liveToggleClip".into(),
            kind: crate::ElementKind::Instance {
                symbol_id: "live-toggle-child".into(),
                playback: crate::InstancePlayback::Independent,
            },
            ..crate::Element::default()
        }];
    project.validate().unwrap();

    let path = vec!["live-toggle-instance".into()];
    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    preview.tick(&project);
    assert_eq!(preview.clip_frame(&path), Some(0));

    preview.set_live_scripts(&project, false);
    assert_eq!(preview.frame(), 0, "the authored root playhead is retained");
    preview.tick(&project);
    assert_eq!(
        preview.clip_frame(&path),
        Some(1),
        "classic preview gets a fresh child clock and ignores stop()"
    );
}

#[test]
fn instance_wrappers_assigned_callbacks_conditionals_and_diagnostics_work_in_live_preview() {
    let mut project = Project::blank();
    let root = project.root.clone();
    let mut btn_sym = project.symbol(&root).unwrap().clone();
    btn_sym.id = "play-btn-sym".into();
    btn_sym.name = "PlayButton".into();
    btn_sym.kind = SymbolKind::Button;
    btn_sym.timeline.frame_count = 4;
    btn_sym.timeline.layers[0].id = "btn-layer".into();

    let mut mc_sym = project.symbol(&root).unwrap().clone();
    mc_sym.id = "hero-mc-sym".into();
    mc_sym.name = "HeroClip".into();
    mc_sym.kind = SymbolKind::MovieClip;
    mc_sym.timeline.frame_count = 2;
    mc_sym.timeline.layers[0].id = "mc-layer".into();
    project.symbols.push(btn_sym);
    project.symbols.push(mc_sym);

    let root_sym = project.symbol_mut(&root).unwrap();
    root_sym.timeline.frame_count = 5;
    root_sym.timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript2,
                source: "if (false) {\n    stop();\n    trace(\"wrong branch\");\n}\nif (1 + 1 == 2) {\n    stop();\n}\nhero_mc.onRelease = function() {\n    this._x += 25;\n    trace(\"hero clicked at \" + this._x);\n};\nhero_mc.addEventListener(\"enterFrame\", function(e) {\n    this._y += 2;\n});"
                    .into(),
            },
        }];
    root_sym.timeline.layers[0].keyframes[0].elements = vec![
            crate::Element {
                id: "btn-inst".into(),
                name: "go_btn".into(),
                kind: crate::ElementKind::Instance {
                    symbol_id: "play-btn-sym".into(),
                    playback: crate::InstancePlayback::Independent,
                },
                // Stored inside ObjectEvent::Load with classic `on (release)` wrapper, matching FLA/XFL imports!
                scripts: vec![crate::ObjectScript {
                    event: ObjectEvent::Load,
                    script: crate::Script {
                        language: crate::ScriptLanguage::ActionScript2,
                        source: "on (release) {\n    gotoAndStop(4);\n    trace(\"btn jumped root\");\n}\nnew Sound();"
                            .into(),
                    },
                }],
                ..crate::Element::default()
            },
            crate::Element {
                id: "hero-inst".into(),
                name: "hero_mc".into(),
                kind: crate::ElementKind::Instance {
                    symbol_id: "hero-mc-sym".into(),
                    playback: crate::InstancePlayback::Independent,
                },
                scripts: vec![crate::ObjectScript {
                    event: ObjectEvent::Load,
                    script: crate::Script {
                        language: crate::ScriptLanguage::ActionScript2,
                        source: "onClipEvent (load) {\n    this._x = 10;\n}\nonClipEvent (enterFrame) {\n    this._x += 5;\n}"
                            .into(),
                    },
                }],
                ..crate::Element::default()
            },
        ];
    // Extend keyframe span across all 5 frames so instances stay alive on frame 3 (1-based 4)
    project.validate().unwrap();

    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    // `if (false)` did NOT trace "wrong branch", `if (1 + 1 == 2)` halted root, and `hero_mc.onRelease` body did NOT run on assignment!
    assert!(preview.is_root_halted_by_script());
    assert!(preview.drain_traces().is_empty());

    // Unsupported `new Sound()` emitted a located diagnostic once
    let diags = preview.drain_diagnostics();
    assert_eq!(diags.len(), 1);
    assert!(diags[0].contains("go_btn"));
    assert!(diags[0].contains("new Sound()"));

    // Tick runs both `onClipEvent(enterFrame)` (+5 -> 15) and `addEventListener("enterFrame")` (+2 -> y=2)
    preview.tick(&project);
    let hero_path = vec!["hero-inst".to_owned()];
    assert_eq!(
        preview.player.property(&project, &hero_path, "x"),
        Some(15.0)
    );
    assert_eq!(
        preview.player.property(&project, &hero_path, "y"),
        Some(2.0)
    );

    // Clicking `hero_mc` fires assigned `onRelease` with `this` bound to `hero_mc`!
    preview.click(&project, &hero_path);
    assert_eq!(
        preview.drain_traces(),
        vec!["hero clicked at 40".to_string()]
    );

    // Clicking `go_btn` fires `on (release)` which jumps parent/root timeline to frame 4 (0-based 3)!
    let btn_path = vec!["btn-inst".to_owned()];
    assert!(preview.is_interactive_target(&project, &btn_path));
    preview.click(&project, &btn_path);
    assert_eq!(preview.frame(), 3);
    assert_eq!(preview.drain_traces(), vec!["btn jumped root".to_string()]);
}

#[test]
fn on_clip_event_load_persists_into_enter_frame_and_compound_ops_mutate_stage_preview() {
    let mut project = Project::blank();
    let root = project.root.clone();
    let mut mc_sym = project.symbol(&root).unwrap().clone();
    mc_sym.id = "spinner-mc".into();
    mc_sym.name = "SpinnerClip".into();
    mc_sym.kind = SymbolKind::MovieClip;
    mc_sym.timeline.frame_count = 1;
    mc_sym.timeline.layers[0].id = "spinner-layer".into();
    project.symbols.push(mc_sym);

    let root_sym = project.symbol_mut(&root).unwrap();
    root_sym.timeline.frame_count = 4;
    root_sym.timeline.layers[0].keyframes[0].elements = vec![crate::Element {
            id: "spinner-inst".into(),
            name: "spinner".into(),
            kind: crate::ElementKind::Instance {
                symbol_id: "spinner-mc".into(),
                playback: crate::InstancePlayback::Independent,
            },
            scripts: vec![crate::ObjectScript {
                event: ObjectEvent::EnterFrame,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript2,
                    source: "onClipEvent (load) {\n    var vx = 20, bonus;\n    omega = 15;\n    _x = 100;\n    _y = 50;\n    _rotation = 10;\n    _xscale = 120;\n    _yscale = 80;\n    _alpha = 90;\n}\nonClipEvent (enterFrame) {\n    bonus += 3;\n    vx /= 2;\n    omega %= 10;\n    _x += vx + bonus;\n    ++_x;\n    _y -= 5;\n    --_y;\n    _rotation += omega;\n    _rotation++;\n    _xscale *= 1.5;\n    _yscale /= 2;\n    _alpha -= 15;\n}\non (release) {\n    vx += 4;\n    trace(\"spinner vx=\" + vx + \" x=\" + _x + \" rot=\" + _rotation);\n}"
                        .into(),
                },
            }],
            ..crate::Element::default()
        }];
    project.validate().unwrap();

    let spinner_path = vec!["spinner-inst".to_owned()];
    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert!(preview.drain_diagnostics().is_empty());
    assert_eq!(
        preview.player.property(&project, &spinner_path, "x"),
        Some(100.0)
    );
    assert_eq!(
        preview.player.property(&project, &spinner_path, "rotation"),
        Some(10.0)
    );

    // Frame 1 tick:
    // bonus: undefined -> 0 + 3 = 3
    // vx: 20 /= 2 -> 10
    // omega: 15 %= 10 -> 5
    // _x: 100 + (10 + 3) = 113, ++_x -> 114
    // _y: 50 - 5 = 45, --_y -> 44
    // _rotation: 10 + 5 = 15, _rotation++ -> 16
    // _xscale: 120 *= 1.5 -> 180 (scale_x = 1.8)
    // _yscale: 80 /= 2 -> 40 (scale_y = 0.4)
    // _alpha: 90 -= 15 -> 75 (alpha = 0.75)
    preview.tick(&project);
    assert!(preview.drain_diagnostics().is_empty());
    assert_eq!(
        preview.player.property(&project, &spinner_path, "x"),
        Some(114.0)
    );
    assert_eq!(
        preview.player.property(&project, &spinner_path, "y"),
        Some(44.0)
    );
    assert_eq!(
        preview.player.property(&project, &spinner_path, "rotation"),
        Some(16.0)
    );
    assert!(
        (preview
            .player
            .property(&project, &spinner_path, "scale_x")
            .unwrap()
            - 1.8)
            .abs()
            < 1e-6
    );
    assert!(
        (preview
            .player
            .property(&project, &spinner_path, "scale_y")
            .unwrap()
            - 0.4)
            .abs()
            < 1e-6
    );
    assert!(
        (preview
            .player
            .property(&project, &spinner_path, "alpha")
            .unwrap()
            - 0.75)
            .abs()
            < 1e-6
    );

    // Clicking `spinner` mutates the same instance scope (`vx`: 10 += 4 -> 14)
    preview.click(&project, &spinner_path);
    assert_eq!(
        preview.drain_traces(),
        vec!["spinner vx=14 x=114 rot=16".to_owned()]
    );

    // Frame 2 tick uses updated `vx = 14` (`14 /= 2 -> 7`) and `bonus` (`3 += 3 -> 6`):
    // `_x`: 114 + (7 + 6) + 1 = 128
    preview.tick(&project);
    assert_eq!(
        preview.player.property(&project, &spinner_path, "x"),
        Some(128.0)
    );

    // Verify `evaluate_scene_with_symbol_overrides` evaluates single-symbol solo overrides
    // without cloning `Project`.
    let mut solo_sym = project.symbol(&root).unwrap().clone();
    solo_sym.timeline.layers[0].visible = false;
    let solo_overrides = std::collections::HashMap::from([(root.clone(), solo_sym)]);
    let hidden_scene = crate::evaluate_scene_with_symbol_overrides(
        &project,
        &solo_overrides,
        &root,
        0,
        true,
        true,
    );
    assert!(hidden_scene.draws.is_empty());
}

#[test]
fn t266_typed_preview_value_strict_vs_loose_equality_string_plus_and_null_undefined() {
    let mut project = Project::blank();
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: crate::ScriptLanguage::ActionScript3,
            source: r#"
                    trace("5" === 5, "5" == 5, "5" !== 5, "5" != 5);
                    trace(null == undefined, null === undefined, null == 0, undefined == 0);
                    trace("null" == null, "undefined" == undefined);
                    var sn = "null", su = "undefined", n_val = null, u_val;
                    trace(sn == n_val, su == u_val, n_val == u_val, n_val === u_val);
                    var a = "5";
                    var b = a + 1;
                    var s = "hello ";
                    var w = s + "world";
                    var n = 2;
                    var m = n + 3;
                    trace(b, b === "51", b === 51, w, m, m === 5);
                    a += 2;
                    trace(a, a === "52");
                    if ("5" === 5) {
                        trace("BUG_STRICT_EQ");
                    }
                    if ("5" == 5 && "5" !== 5 && null == undefined && !(null === undefined)) {
                        trace("COND_OK");
                    }
                "#
            .into(),
        },
    }];
    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert!(preview.drain_diagnostics().is_empty());
    assert_eq!(
        preview.drain_traces(),
        vec![
            "false true true false".to_owned(),
            "true false false false".to_owned(),
            "false false".to_owned(),
            "false false true false".to_owned(),
            "51 true false hello world 5 true".to_owned(),
            "52 true".to_owned(),
            "COND_OK".to_owned(),
        ]
    );
}

#[test]
fn t266_null_and_undefined_arithmetic_and_relational_across_as1_as2_and_as3() {
    let run_for_lang = |language: crate::ScriptLanguage| -> Vec<String> {
        let mut project = Project::blank();
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language,
                source: r#"
                        var u;
                        var nl = null;
                        trace(nl + 5, nl - 2, nl * 10, nl < 5, nl >= 0, nl > 0);
                        trace(u + 5, u - 2, u < 5, u > 5, u <= 0, u >= 0);
                        trace("ab" < "ac", "b" < "a");
                    "#
                .into(),
            },
        }];
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert!(preview.drain_diagnostics().is_empty());
        preview.drain_traces()
    };

    let as1_traces = run_for_lang(crate::ScriptLanguage::ActionScript1);
    assert_eq!(
        as1_traces,
        vec![
            "5 -2 0 true true false".to_owned(),
            "5 -2 true false true true".to_owned(),
            "true false".to_owned(),
        ]
    );

    for lang in [
        crate::ScriptLanguage::ActionScript2,
        crate::ScriptLanguage::ActionScript3,
    ] {
        let traces = run_for_lang(lang);
        assert_eq!(
            traces,
            vec![
                "5 -2 0 true true false".to_owned(),
                "NaN NaN false false false false".to_owned(),
                "true false".to_owned(),
            ]
        );
    }
}

#[test]
fn r102_skipped_foreign_scripts_do_not_execute_or_make_placements_interactive() {
    let mut project = Project::blank();
    project.format_version = project.format_version.max(6);
    project.script_language = Some(crate::ScriptLanguage::ActionScript3);
    project.also_compile_compatible_scripts = false;
    let root = project.root.clone();

    let clip_id: Id = "mc_foreign".into();
    let mut mc_sym = project.symbol(&root).unwrap().clone();
    mc_sym.id = clip_id.clone();
    mc_sym.name = "ForeignClip".into();
    mc_sym.kind = crate::SymbolKind::MovieClip;
    mc_sym.timeline.frame_count = 1;
    mc_sym.timeline.layers[0].id = "mc_layer".into();
    project.symbols.push(mc_sym);

    let foreign_elem_id: Id = "inst_foreign".into();
    let primary_elem_id: Id = "inst_primary".into();
    {
        let root_sym = project.symbol_mut(&root).unwrap();
        root_sym.timeline.frame_count = 2;
        root_sym.timeline.scripts = vec![
            crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript3,
                    source: r#"trace("AS3_FRAME_RAN");"#.into(),
                },
            },
            crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript2,
                    source: r#"trace("SKIPPED_AS2_FRAME");"#.into(),
                },
            },
            crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::JavaScript,
                    source: r#"trace("SKIPPED_JS_FRAME");"#.into(),
                },
            },
        ];
        root_sym.timeline.layers[0].keyframes[0].elements = vec![
                crate::Element {
                    id: foreign_elem_id.clone(),
                    name: "foreignMc".into(),
                    kind: crate::ElementKind::Instance {
                        symbol_id: clip_id.clone(),
                        playback: crate::InstancePlayback::Independent,
                    },
                    scripts: vec![
                        crate::ObjectScript {
                            event: ObjectEvent::Load,
                            script: crate::Script {
                                language: crate::ScriptLanguage::ActionScript2,
                                source: r#"trace("SKIPPED_AS2_LOAD");"#.into(),
                            },
                        },
                        crate::ObjectScript {
                            event: ObjectEvent::EnterFrame,
                            script: crate::Script {
                                language: crate::ScriptLanguage::ActionScript2,
                                source: r#"onClipEvent(load) { trace("SKIPPED_WRAPPER_LOAD"); } on(release) { trace("SKIPPED_WRAPPER_CLICK"); } trace("SKIPPED_AS2_ENTER");"#.into(),
                            },
                        },
                        crate::ObjectScript {
                            event: ObjectEvent::Click,
                            script: crate::Script {
                                language: crate::ScriptLanguage::JavaScript,
                                source: r#"trace("SKIPPED_JS_CLICK");"#.into(),
                            },
                        },
                    ],
                    ..crate::Element::default()
                },
                crate::Element {
                    id: primary_elem_id.clone(),
                    name: "primaryMc".into(),
                    kind: crate::ElementKind::Instance {
                        symbol_id: clip_id,
                        playback: crate::InstancePlayback::Independent,
                    },
                    scripts: vec![crate::ObjectScript {
                        event: ObjectEvent::Click,
                        script: crate::Script {
                            language: crate::ScriptLanguage::ActionScript3,
                            source: r#"trace("PRIMARY_AS3_CLICK");"#.into(),
                        },
                    }],
                    ..crate::Element::default()
                },
            ];
    }
    project.validate().unwrap();

    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert_eq!(preview.drain_traces(), vec!["AS3_FRAME_RAN".to_owned()]);
    assert!(
        !preview.is_interactive_target(&project, std::slice::from_ref(&foreign_elem_id)),
        "placement with only SkippedForeign scripts must not be clickable"
    );
    assert_eq!(
        preview.interactive_ancestor_path(&project, std::slice::from_ref(&primary_elem_id)),
        Some(vec![primary_elem_id.clone()]),
    );

    preview.tick(&project);
    preview.click(&project, std::slice::from_ref(&foreign_elem_id));
    assert!(
        preview.drain_traces().is_empty(),
        "SkippedForeign EnterFrame/Click handlers must never execute"
    );

    preview.click(&project, std::slice::from_ref(&primary_elem_id));
    assert_eq!(preview.drain_traces(), vec!["PRIMARY_AS3_CLICK".to_owned()]);

    // Enabling `also_compile_compatible_scripts` on a JS primary project compiles TS
    // (`CompiledCompatibleWithWarning`) while still skipping AS2 (`SkippedForeign`).
    project.script_language = Some(crate::ScriptLanguage::JavaScript);
    project.also_compile_compatible_scripts = true;
    project.symbol_mut(&root).unwrap().timeline.scripts = vec![
        crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::TypeScript,
                source: r#"trace("COMPAT_TS_FRAME");"#.into(),
            },
        },
        crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript2,
                source: r#"trace("STILL_SKIPPED_AS2");"#.into(),
            },
        },
    ];
    let mut compat_preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert_eq!(
        compat_preview.drain_traces(),
        vec!["COMPAT_TS_FRAME".to_owned()]
    );
}

#[test]
fn t320_multi_scene_auto_advance_scene_step_and_dialect_aware_goto() {
    let mut project = Project::blank();
    project.format_version = 6;
    project.script_language = Some(crate::ScriptLanguage::ActionScript3);
    let s1_id = project.root.clone();
    let s2_id: Id = "scene-2".into();

    {
        let s1 = project.symbol_mut(&s1_id).unwrap();
        s1.name = "Scene 1".into();
        s1.timeline.frame_count = 2;
        s1.timeline.layers[0].keyframes[0].label = "s1_start".into();
    }
    let mut s2 = project.symbol(&s1_id).unwrap().clone();
    s2.id = s2_id.clone();
    s2.name = "Scene 2".into();
    s2.timeline.frame_count = 2;
    s2.timeline.layers[0].id = "s2-layer".into();
    s2.timeline.layers[0].keyframes[0].label = "s2_entry".into();
    s2.timeline.layers[0].keyframes.push(crate::Keyframe {
        frame: 1,
        label: "s2_final".into(),
        ..crate::Keyframe::default()
    });
    s2.timeline.scripts = vec![
        crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript3,
                source: r#"trace("S2_F1");"#.into(),
            },
        },
        crate::FrameScript {
            layer: None,
            frame: 1,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript3,
                source: r#"trace("S2_F2");"#.into(),
            },
        },
    ];
    project.symbols.push(s2);
    project.scenes = vec![s1_id.clone(), s2_id.clone()];
    project.upgrade_format_for_features();
    project.validate().unwrap();

    // 0. Flattened `Player` (used by HTML5/WASM) auto-advances unstopped Scene 1 -> Scene 2.
    let (flat_project, scene_ranges) = crate::flatten_scenes_with_ranges(&project);
    let mut runtime_player = crate::Player::new(&flat_project);
    runtime_player
        .set_scenes(&flat_project, scene_ranges)
        .unwrap();
    assert_eq!(runtime_player.frame(), 0);
    runtime_player.tick(&flat_project);
    assert_eq!(runtime_player.frame(), 1);
    runtime_player.tick(&flat_project);
    assert_eq!(runtime_player.frame(), 2); // Scene 2 frame 0 in flattened timeline

    // 1. `AuthoredPlayback` with `set_play_all_scenes(true)` auto-advances Scene 1 (frames 0..1) -> Scene 2 frame 0 -> Scene 2 frame 1 -> wraps to Scene 1 frame 0.
    let mut pb = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
    assert!(!pb.is_play_all_scenes());
    pb.set_play_all_scenes(true);
    assert!(pb.is_play_all_scenes());
    assert_eq!(pb.symbol(), s1_id);
    assert_eq!(pb.frame(), 0);

    pb.tick(&project);
    assert_eq!(pb.symbol(), s1_id);
    assert_eq!(pb.frame(), 1);
    pb.vars.insert(
        (Vec::new(), "root_state".into()),
        PreviewValue::String("keep".into()),
    );
    pb.vars.insert(
        (vec!["departed-child".into()], "child_state".into()),
        PreviewValue::String("discard".into()),
    );

    pb.tick(&project);
    assert_eq!(pb.symbol(), s2_id);
    assert_eq!(pb.frame(), 0);
    assert!(pb.vars.contains_key(&(Vec::new(), "root_state".into())));
    assert!(
        !pb.vars
            .contains_key(&(vec!["departed-child".into()], "child_state".into()))
    );
    assert_eq!(pb.drain_traces(), vec!["S2_F1".to_owned()]);

    pb.tick(&project);
    assert_eq!(pb.symbol(), s2_id);
    assert_eq!(pb.frame(), 1);
    assert_eq!(pb.drain_traces(), vec!["S2_F2".to_owned()]);

    pb.tick(&project);
    assert_eq!(pb.symbol(), s1_id);
    assert_eq!(pb.frame(), 0);

    // 2. Single-scene mode (`set_play_all_scenes(false)`) loops inside Scene 1 without crossing.
    pb.set_play_all_scenes(false);
    pb.tick(&project);
    assert_eq!((pb.symbol(), pb.frame()), (s1_id.as_str(), 1));
    pb.tick(&project);
    assert_eq!((pb.symbol(), pb.frame()), (s1_id.as_str(), 0));
    pb.set_play_all_scenes(true);

    // 3. AS3 `nextScene()` / `prevScene()`, 2-arg `(frame, scene)`, and cross-scene 1-arg label.
    project.symbol_mut(&s1_id).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: crate::ScriptLanguage::ActionScript3,
            source: r#"gotoAndStop("s2_final");"#.into(),
        },
    }];
    let mut pb_label = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
    assert_eq!((pb_label.symbol(), pb_label.frame()), (s2_id.as_str(), 1));
    assert!(pb_label.is_root_halted_by_script());
    assert_eq!(pb_label.drain_traces(), vec!["S2_F2".to_owned()]);

    project.symbol_mut(&s1_id).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: crate::ScriptLanguage::ActionScript3,
            source: r#"nextScene();"#.into(),
        },
    }];
    let mut pb_next = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
    assert_eq!((pb_next.symbol(), pb_next.frame()), (s2_id.as_str(), 0));
    assert_eq!(pb_next.drain_traces(), vec!["S2_F1".to_owned()]);

    project.symbol_mut(&s2_id).unwrap().timeline.scripts[0] = crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: crate::ScriptLanguage::ActionScript3,
            source: r#"gotoAndPlay(2, "Scene 1");"#.into(),
        },
    };
    let pb_as3_2arg = AuthoredPlayback::with_live_scripts(&project, &s2_id, 0, true);
    assert_eq!(
        (pb_as3_2arg.symbol(), pb_as3_2arg.frame()),
        (s1_id.as_str(), 1)
    );
    assert!(!pb_as3_2arg.is_root_halted_by_script());

    // 4. AS2 2-arg `gotoAndStop("Scene 2", 2)` uses `(scene, frame)` argument order!
    project.script_language = Some(crate::ScriptLanguage::ActionScript2);
    project.symbol_mut(&s1_id).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: crate::ScriptLanguage::ActionScript2,
            source: r#"gotoAndStop("Scene 2", 2);"#.into(),
        },
    }];
    project.symbol_mut(&s2_id).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 1,
        script: crate::Script {
            language: crate::ScriptLanguage::ActionScript2,
            source: r#"trace("AS2_S2_F2");"#.into(),
        },
    }];
    let mut pb_as2 = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
    assert_eq!((pb_as2.symbol(), pb_as2.frame()), (s2_id.as_str(), 1));
    assert!(pb_as2.is_root_halted_by_script());
    assert_eq!(pb_as2.drain_traces(), vec!["AS2_S2_F2".to_owned()]);
}

#[test]
fn t320_runtime_display_graph_applies_camera_pose_parallax_tint_and_pinned_hud() {
    let mut project = Project::blank();
    project.width = 400;
    project.height = 300;
    project.format_version = 10;
    let root_id = project.root.clone();

    let box_sym_id: Id = "box-mc".into();
    let mut box_sym = project.symbol(&root_id).unwrap().clone();
    box_sym.id = box_sym_id.clone();
    box_sym.name = "BoxMc".into();
    box_sym.kind = crate::SymbolKind::MovieClip;
    box_sym.timeline.frame_count = 1;
    box_sym.timeline.layers[0].id = "box-layer".into();
    box_sym.timeline.layers[0].keyframes[0].elements = vec![crate::Element {
        id: "box-rect".into(),
        name: String::new(),
        kind: crate::ElementKind::Rectangle {
            width: 20.0,
            height: 20.0,
            fill: [200, 100, 50, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.0,
        },
        ..crate::Element::default()
    }];
    project.symbols.push(box_sym);

    {
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 1;
        // Layer 0: active Camera layer panning to (+50, 0) with 2x zoom and 0.5 red tint.
        let mut cam_layer = root.timeline.layers[0].clone();
        cam_layer.id = "cam-layer".into();
        cam_layer.name = "Camera".into();
        cam_layer.keyframes[0].elements.clear();
        cam_layer.keyframes[0].camera = Some(crate::CameraPose {
            x: 50.0,
            y: 0.0,
            zoom: 2.0,
            rotation: 0.0,
            color_transform: crate::ColorTransform {
                multipliers: [0.5, 1.0, 1.0, 1.0],
                offsets: [0.0; 4],
            },
            ..crate::CameraPose::default()
        });

        // Layer 1: normal world layer (z_depth = 0) with clip at stage center (200, 150).
        let mut world_layer = root.timeline.layers[0].clone();
        world_layer.id = "world-layer".into();
        world_layer.name = "World".into();
        world_layer.keyframes[0].elements = vec![crate::Element {
            id: "world-inst".into(),
            name: "worldMc".into(),
            transform: crate::Transform {
                x: 200.0,
                y: 150.0,
                ..crate::Transform::default()
            },
            kind: crate::ElementKind::Instance {
                symbol_id: box_sym_id.clone(),
                playback: crate::InstancePlayback::Independent,
            },
            ..crate::Element::default()
        }];

        // Layer 2: parallax layer (z_depth = 200 -> focal_length/(focal_length+z_depth)) at (200, 150).
        let mut parallax_layer = world_layer.clone();
        parallax_layer.id = "parallax-layer".into();
        parallax_layer.name = "Parallax".into();
        parallax_layer.properties.z_depth = 200.0;
        parallax_layer.keyframes[0].elements[0].id = "parallax-inst".into();
        parallax_layer.keyframes[0].elements[0].name = "parallaxMc".into();

        // Layer 3: HUD layer pinned to camera (`attach_to_camera = true`) at (25, 15).
        let mut hud_layer = world_layer.clone();
        hud_layer.id = "hud-layer".into();
        hud_layer.name = "HUD".into();
        hud_layer.properties.attach_to_camera = true;
        hud_layer.keyframes[0].elements[0].id = "hud-inst".into();
        hud_layer.keyframes[0].elements[0].name = "hudMc".into();
        hud_layer.keyframes[0].elements[0].transform = crate::Transform {
            x: 25.0,
            y: 15.0,
            ..crate::Transform::default()
        };

        root.timeline.layers = vec![cam_layer, world_layer, parallax_layer, hud_layer];
    }
    project.upgrade_format_for_features();
    project.validate().unwrap();

    let player = crate::Player::new(&project);
    let snap = player.runtime_snapshot(&project);

    let world_node = snap
        .objects
        .iter()
        .find(|n| n.display.name == "worldMc")
        .expect("worldMc node");
    let parallax_node = snap
        .objects
        .iter()
        .find(|n| n.display.name == "parallaxMc")
        .expect("parallaxMc node");
    let hud_node = snap
        .objects
        .iter()
        .find(|n| n.display.name == "hudMc")
        .expect("hudMc node");

    // Authored local matrices stay pure (unmutated by camera).
    assert!((world_node.local_matrix[0] - 1.0).abs() < 1e-6);
    assert!((world_node.local_matrix[4] - 200.0).abs() < 1e-6);
    assert!((world_node.local_matrix[5] - 150.0).abs() < 1e-6);

    // World clip at (200, 150) with camera (+50, 0) zoom 2 around (200, 150) -> scaled 2x and shifted by -50*2 = -100 -> x = 100, y = 150.
    assert!((world_node.display.world[0] - 2.0).abs() < 1e-6);
    assert!((world_node.display.world[3] - 2.0).abs() < 1e-6);
    assert!((world_node.display.world[4] - 100.0).abs() < 1e-6);
    assert!((world_node.display.world[5] - 150.0).abs() < 1e-6);

    // Parallax clip (`z_depth = 200.0`) has distinct perspective scale (< 2.0) and pan (> 100.0).
    assert!(parallax_node.display.world[0] < 1.9 && parallax_node.display.world[0] > 0.5);
    assert!(parallax_node.display.world[4] > 105.0 && parallax_node.display.world[4] < 200.0);

    // HUD clip (`attach_to_camera = true`) ignores camera transform completely!
    assert!((hud_node.display.world[0] - 1.0).abs() < 1e-6);
    assert!((hud_node.display.world[4] - 25.0).abs() < 1e-6);
    assert!((hud_node.display.world[5] - 15.0).abs() < 1e-6);

    let world_child = snap
        .objects
        .iter()
        .find(|n| n.parent_ref.as_ref() == Some(&world_node.object_ref))
        .expect("worldMc child shape");
    let hud_child = snap
        .objects
        .iter()
        .find(|n| n.parent_ref.as_ref() == Some(&hud_node.object_ref))
        .expect("hudMc child shape");
    let world_draw = snap
        .draws
        .iter()
        .find(|d| d.owner_ref == world_child.object_ref)
        .expect("worldMc draw");
    let hud_draw = snap
        .draws
        .iter()
        .find(|d| d.owner_ref == hud_child.object_ref)
        .expect("hudMc draw");
    let crate::runtime_graph::RuntimeDrawKind::Element(crate::ElementKind::Rectangle {
        fill: world_fill,
        ..
    }) = &world_draw.item.kind
    else {
        panic!("expected rectangle draw for worldMc");
    };
    let crate::runtime_graph::RuntimeDrawKind::Element(crate::ElementKind::Rectangle {
        fill: hud_fill,
        ..
    }) = &hud_draw.item.kind
    else {
        panic!("expected rectangle draw for hudMc");
    };
    assert_eq!(
        world_fill[0], 100,
        "camera color_transform (0.5 red) must tint non-attached layers (200 -> 100)"
    );
    assert_eq!(
        hud_fill[0], 200,
        "attach_to_camera HUD layer must not receive camera color_transform"
    );
}

#[test]
fn t368_authored_playback_traces_multidimensional_arrays_objects_and_container_mutations() {
    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let root = project.root.clone();
    project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: ScriptLanguage::ActionScript2,
            source: [
                "stop();",
                "var row = [0, 1, 0];",
                "trace(row);",
                "var TFigure = [[0, 1, 0], [1, 1, 1], [0, 0, 0]];",
                "trace(TFigure);",
                "var AllFigures = new Array(TFigure, [[1, 1, 1, 1]]);",
                "trace(AllFigures);",
                "var sized = new Array(3);",
                "var wrapped = new Array(row);",
                "trace(sized.length, wrapped);",
                "TFigure[0][2] = 9;",
                "TFigure.push([2, 2, 2]);",
                "trace(TFigure);",
                "var state = { title: \"Tetris\", score: 100, board: [[1, 0], [0, 1]] };",
                "state.score += 50;",
                "state.board[1][0] = 7;",
                "trace(state);",
                "trace(\"csv=\" + row + \"|obj=\" + state);",
                "new Sound();",
            ]
            .join("\n"),
        },
    }];

    let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    let traces = preview.drain_traces();
    assert_eq!(
        traces,
        vec![
            "[0, 1, 0]",
            "[\n  [0, 1, 0],\n  [1, 1, 1],\n  [0, 0, 0]\n]",
            "[\n  [\n    [0, 1, 0],\n    [1, 1, 1],\n    [0, 0, 0]\n  ],\n  [\n    [1, 1, 1, 1]\n  ]\n]",
            "3 [\n  [0, 1, 0]\n]",
            "[\n  [0, 1, 9],\n  [1, 1, 1],\n  [0, 0, 0],\n  [2, 2, 2]\n]",
            "{\n  title: \"Tetris\",\n  score: 150,\n  board: [\n    [1, 0],\n    [7, 1]\n  ]\n}",
            "csv=0,1,0|obj=[object Object]",
        ]
    );
    let diags = preview.drain_diagnostics();
    assert!(
        diags.iter().any(|d| d.contains("new Sound()")
            && d.contains("outside the bounded native stage preview subset")),
        "unsupported constructor statements must continue emitting diagnostics: {diags:?}"
    );
}

#[test]
fn t369_authored_playback_array_join_nested_matrices_chained_string_methods_and_unsupported_call_diagnostics()
 {
    let run = |lang: ScriptLanguage, src: &str| -> (Vec<String>, Vec<String>) {
        let mut project = Project::blank();
        project.script_language = Some(lang);
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: lang,
                source: src.to_owned(),
            },
        }];
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        (preview.drain_traces(), preview.drain_diagnostics())
    };

    // Exact user snippet + chained flatten + builtin Math/String/Array value methods + unsupported call diagnostic
    let (as2_traces, as2_diags) = run(
        ScriptLanguage::ActionScript2,
        &[
            "stop();",
            "lFigure = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);",
            "f=lFigure.join(\"\");",
            "trace ( f );",
            "trace ( lFigure );",
            "flat = lFigure.join(\"\").split(\",\").join(\"\");",
            "trace ( flat, flat.length, flat.slice(1, 5), Math.floor(3.9), parseInt(\"0x10\") );",
            "avm1Join = [\"zero\", undefined, null, \"five\"].join(\"/\");",
            "avm1Hole = new Array(2).toString();",
            "trace ( avm1Join, avm1Hole );",
            "bad = lFigure.length();",
            "trace ( bad );",
        ]
        .join("\n"),
    );
    assert_eq!(
        as2_traces,
        vec![
            "0,1,00,1,00,1,00,1,0",
            "[\n  [0, 1, 0],\n  [0, 1, 0],\n  [0, 1, 0],\n  [0, 1, 0]\n]",
            "010010010010 12 1001 3 16",
            "zero/undefined/null/five undefined,undefined",
            "undefined",
        ]
    );
    assert!(
        as2_diags.iter().any(|d| d.contains("lFigure.length()")
            && d.contains("outside the bounded native stage preview subset")),
        "unsupported call `.length()` must emit a preview diagnostic instead of storing raw source text: {as2_diags:?}"
    );

    let (as1_traces, as1_diags) = run(
        ScriptLanguage::ActionScript1,
        "stop();\ntrace([\"zero\", undefined, null, \"five\"].join(\"/\"), new Array(2).toString());",
    );
    assert!(as1_diags.is_empty());
    assert_eq!(as1_traces, vec!["zero//null/five ,"]);

    let (as3_traces, as3_diags) = run(
        ScriptLanguage::ActionScript3,
        "stop();\nvar lFigure:Array = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);\nvar f:String = lFigure.join(\"\");\ntrace(f, [\"zero\", undefined, null, \"five\"].join(\"/\"), new Array(2).toString());",
    );
    assert!(as3_diags.is_empty());
    assert_eq!(as3_traces, vec!["0,1,00,1,00,1,00,1,0 zero///five ,"]);
}

#[test]
fn t370_authored_playback_string_array_eval_set_and_bitwise_operators_conformance() {
    let run = |lang: ScriptLanguage, src: &str| -> (Vec<String>, Vec<String>) {
        let mut project = Project::blank();
        project.script_language = Some(lang);
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: lang,
                source: src.to_owned(),
            },
        }];
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        (preview.drain_traces(), preview.drain_diagnostics())
    };

    // 1. AS2: String ops (+, +=, add/eq/ne/lt/gt/<>, Flash 4 chr/ord/length/substring),
    // Array ops (sparse literals, compound bitwise/arith on elements, delete hole, in, instanceof),
    // eval/set (named & slash/colon & container paths, object identity, safe malformed/expr paths),
    // and Bitwise ops (&, |, ^, ~, <<, >>, >>>, &=, |=, ^=, <<=, >>=, >>>=, >32-bit wrapping).
    let (as2_traces, as2_diags) = run(
            ScriptLanguage::ActionScript2,
            &[
                "stop();",
                // String operators & dialect coercions:
                "var s = \"\" + [undefined, null, 1];",
                "var pairConcat = [1, 2] + [3, 4];",
                "var legAdd = (undefined add \"X\") + \"|\" + (\"10\" lt \"2\") + \"|\" + (undefined eq \"undefined\") + \"|\" + (3 <> 4);",
                "var legFns = chr(65) + \"|\" + ord(\"B\") + \"|\" + length(\"neo\") + \"|\" + substring(\"abcdef\", 2, 3);",
                "var leftAssoc = String((\"a\" + 5) - 2) + \"|\" + (10 - 3 + 2) + \"|\" + (12 / 3 * 2);",
                "trace(s, pairConcat, legAdd, legFns, leftAssoc);",
                // Array operators:
                "var sp = [, 10, , 30,];",
                "var hasBefore = (0 in sp) + \",\" + (1 in sp) + \",\" + (\"length\" in sp) + \",\" + (sp instanceof Array);",
                "var arr = [1, 20, 30];",
                "arr[0] <<= 3; arr[0] |= 3; arr[0] &= 10; arr[0] ^= 1; arr[0] >>= 1; arr[0] >>>= 1;",
                "var del1 = delete arr[1]; var delAgain = delete arr[1]; var delLen = delete arr.length;",
                "trace(sp.length, hasBefore, arr[0], del1, delAgain, delLen, 1 in arr, arr.length);",
                // eval & set:
                "set(\"dynScore\", 42);",
                "var lFigure = new Array([0, 1, 0], [0, 1, 0]);",
                "set(\"lFigure.0.2\", 9);",
                "var idx = 0;",
                "trace(eval(\"dyn\" + \"Score\"), eval(\":dynScore\"), eval(\"lFigure.\" + idx), eval(lFigure).join(\"\"), eval(\"1 + 2\"), eval(\"a..b\"), eval(\"\"));",
                // Bitwise operators & precedence:
                "var bw1 = ((undefined | 1) == 1) && ((null ^ 1) == 1) && ((4294967297 | 0) == 1) && ((~0) == -1) && ((1 << 33) == 2);",
                "var bw2 = (-8 >> 1) + \"|\" + (-1 >>> 0) + \"|\" + (-3 >>> 1) + \"|\" + (1 + 2 << 3) + \"|\" + (4 | 2 & 1);",
                "var grid = [[0, 1], [2, 0]];",
                "grid[0][0] |= (1 << 2);",
                "this._x = 4; this._x |= 2;",
                "trace(bw1, bw2, grid[0][0], this._x, typeof null, (grid[0][0] & 4) ? \"HIT\" : \"MISS\");",
            ]
            .join("\n"),
        );
    assert!(
        as2_diags.is_empty(),
        "unexpected AS2 diagnostics: {as2_diags:?}"
    );
    assert_eq!(
        as2_traces,
        vec![
            "undefined,null,1 1,23,4 undefinedX|true|true|true A|66|3|bcd NaN|9|8",
            "4 false,true,true,true 2 true false false false 3",
            "42 42 [0, 1, 9] 0,1,90,1,0 undefined undefined undefined",
            "true -4|4294967295|2147483646|24|4 4 6 null HIT",
        ]
    );

    // 2. AS1: SWF6 `undefined` -> `""` in `+`/`add`/`eq`/`String()`, `undefined < 1 == true`, and case-insensitive `eval`/`set`.
    let (as1_traces, as1_diags) = run(
            ScriptLanguage::ActionScript1,
            &[
                "stop();",
                "set(\"/:RootScore\", 77);",
                "var s1 = \"\" + [undefined, null, 1];",
                "var leg1 = (undefined add \"Z\") + \"|\" + (undefined eq \"\") + \"|\" + (undefined < 1) + \"|\" + String(undefined);",
                "trace(eval(\"/:ROOTSCORE\"), s1, leg1, (1 and 1), (0 or 1), (not 0));",
            ]
            .join("\n"),
        );
    assert!(
        as1_diags.is_empty(),
        "unexpected AS1 diagnostics: {as1_diags:?}"
    );
    assert_eq!(as1_traces, vec!["77 ,null,1 Z|true|true| true true true"]);

    // 3. AS3: `"" + [undefined, null, 1] == ",,1"`, `is`/`as`, `typeof null == "object"`, `delete arr[99] == true`,
    // and explicit diagnostics when removed `eval()` / `set()` are called.
    let (as3_traces, as3_diags) = run(
        ScriptLanguage::ActionScript3,
        &[
            "stop();",
            "var arr:Array = [undefined, null, 1];",
            "var u:uint = uint(-1 >>> 0);",
            "var delMissing:Boolean = delete arr[99];",
            "trace(\"\" + arr, arr is Array, (arr as Array).length, typeof null, delMissing, u);",
            "var badEval:* = eval(\"arr\");",
            "set(\"x\", 1);",
        ]
        .join("\n"),
    );
    assert_eq!(as3_traces, vec![",,1 true 3 object true 4294967295"]);
    assert!(
        as3_diags.iter().any(|d| d.contains("eval(\"arr\")"))
            && as3_diags.iter().any(|d| d.contains("removed set()")),
        "AS3 must diagnose removed eval() and set(): {as3_diags:?}"
    );

    // 4. JavaScript: bitwise ops, array ops, short-circuit `||`, and pure expression `eval("6 * 7")`.
    let (js_traces, js_diags) = run(
        ScriptLanguage::JavaScript,
        "stop();\nlet flags = 1; flags <<= 3; flags |= 3;\ntrace(flags, \"\" || \"fallback\", eval(\"6 * 7\"), (-1 >>> 1));",
    );
    assert!(
        js_diags.is_empty(),
        "unexpected JS diagnostics: {js_diags:?}"
    );
    assert_eq!(js_traces, vec!["11 fallback 42 2147483647"]);
}

#[test]
fn t412_authored_playback_expands_as1_as2_as3_includes_and_reports_diagnostics() {
    let mut project = Project::blank();
    project.script_language = Some(ScriptLanguage::ActionScript2);
    let root = project.root.clone();

    project.resources.push(
        crate::ResourceUnit::from_bytes(
            "res-defs",
            "common/defs.as",
            "text/plain",
            "\u{feff}var PIECE_COUNT = 7;\r\nvar FIGURES = [[1, 1, 1, 1], [1, 1, 0, 0]];\n"
                .as_bytes(),
        )
        .unwrap(),
    );
    project.resources.push(
        crate::ResourceUnit::from_bytes(
            "res-rotate",
            "sub/rotate.as",
            "text/plain",
            "/* nested */ #include '../common/defs.as'\nvar ROTATIONS = 4;\n".as_bytes(),
        )
        .unwrap(),
    );
    project.sources.push(crate::SourceUnit {
            id: "src-figures".into(),
            path: "figures.as".into(),
            language: ScriptLanguage::ActionScript2,
            source: "#include \"sub/rotate.as\"\nvar TOTAL_STATES = 0;\nfunction computeStates() { TOTAL_STATES = PIECE_COUNT * ROTATIONS; }\n"
                .into(),
        });

    project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: ScriptLanguage::ActionScript2,
                source: "#initclip\n#include \"figures.as\"\n#endinitclip\ncomputeStates();\nstop();\ntrace(TOTAL_STATES, FIGURES.length);\n"
                    .into(),
            },
        }];

    let mut pb = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
    assert_eq!(pb.drain_traces(), vec!["28 2"]);
    assert!(pb.drain_diagnostics().is_empty());

    // Builtin NetServices.as / NetDebug.as expand cleanly through expand_script_includes
    let (net_expanded, _, _) = AuthoredPlayback::expand_script_includes(
        &crate::Script {
            language: ScriptLanguage::ActionScript2,
            source: "#include \"NetServices.as\"\n#include \"NetDebug.as\"\n".into(),
        },
        "",
        &std::collections::BTreeMap::new(),
    )
    .expect("builtin NetServices/NetDebug includes should expand");
    assert!(net_expanded.source.contains("createGatewayConnection"));
    assert!(net_expanded.source.contains("traceNetServices"));

    // AS3 `include "path.as";` in AuthoredPlayback
    let mut as3_proj = Project::blank();
    as3_proj.script_language = Some(ScriptLanguage::ActionScript3);
    let as3_root = as3_proj.root.clone();
    as3_proj.resources.push(
        crate::ResourceUnit::from_bytes(
            "res-as3",
            "logic/step.as",
            "text/plain",
            b"var speed:int = 15;\ntrace('as3-inc:' + (speed * 2));\n",
        )
        .unwrap(),
    );
    as3_proj.symbol_mut(&as3_root).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: ScriptLanguage::ActionScript3,
            source: "include \"logic/step.as\";\nstop();\n".into(),
        },
    }];
    let mut pb_as3 = AuthoredPlayback::with_live_scripts(&as3_proj, &as3_root, 0, true);
    assert_eq!(pb_as3.drain_traces(), vec!["as3-inc:30"]);
    assert!(pb_as3.drain_diagnostics().is_empty());

    // Missing include surfaces an actionable preview diagnostic instead of panicking or dropping silently
    let mut bad_proj = Project::blank();
    bad_proj.script_language = Some(ScriptLanguage::ActionScript2);
    let bad_root = bad_proj.root.clone();
    bad_proj.symbol_mut(&bad_root).unwrap().timeline.scripts = vec![crate::FrameScript {
        layer: None,
        frame: 0,
        script: crate::Script {
            language: ScriptLanguage::ActionScript2,
            source: "#include \"does_not_exist.as\"\ntrace('never');\n".into(),
        },
    }];
    let mut pb_bad = AuthoredPlayback::with_live_scripts(&bad_proj, &bad_root, 0, true);
    assert!(pb_bad.drain_traces().is_empty());
    let diags = pb_bad.drain_diagnostics();
    assert!(
        diags
            .iter()
            .any(|d| d.contains("Missing #include resource 'does_not_exist.as'")),
        "expected missing include diagnostic, got: {diags:?}"
    );
}
