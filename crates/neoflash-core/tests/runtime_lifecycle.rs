use neoflash_core::*;

fn shape(id: &str) -> Element {
    Element {
        accessibility: None,
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [255, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
        ..Element::default()
    }
}

fn instance(id: &str, symbol_id: &str) -> Element {
    Element {
        kind: ElementKind::Instance {
            symbol_id: symbol_id.into(),
            playback: InstancePlayback::Independent,
        },
        ..shape(id)
    }
}

fn as3(source: &str) -> Script {
    Script {
        language: ScriptLanguage::ActionScript3,
        source: source.into(),
    }
}

fn symbol(id: &str, frames: u32, keyframes: Vec<Keyframe>, scripts: Vec<FrameScript>) -> Symbol {
    Symbol {
        folder: String::new(),
        class_name: None,
        linkage_id: None,
        id: id.into(),
        name: id.into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            guides: vec![],
            frame_count: frames,
            layers: vec![Layer {
                properties: Default::default(),
                id: format!("{id}-layer"),
                name: "Layer".into(),
                visible: true,
                locked: false,
                keyframes,
            }],
            scripts,
        },
    }
}

fn make_project(symbols: Vec<Symbol>) -> Project {
    Project {
        library_folders: vec![],
        sources: vec![],
        resources: vec![],
        document_class: None,
        scenes: vec![],
        script_language: None,
        also_compile_compatible_scripts: true,
        format_version: 6,
        name: "LifecycleTest".into(),
        width: 400,
        height: 300,
        fps: 24,
        background: [255, 255, 255, 255],
        root: symbols[0].id.clone(),
        symbols,
    }
}

fn drain_phases(player: &mut Player) -> Vec<LifecycleStep> {
    let mut steps = Vec::new();
    while let Some(step) = player.lifecycle_next_step().expect("lifecycle step") {
        steps.push(step);
    }
    steps
}

#[test]
fn one_frame_timelines_keep_their_visit_while_children_and_enter_frame_keep_ticking() {
    for language in [
        ScriptLanguage::ActionScript1,
        ScriptLanguage::ActionScript2,
        ScriptLanguage::ActionScript3,
    ] {
        let script = |source: &str| Script {
            language,
            source: source.into(),
        };
        let key = |elements| Keyframe {
            elements,
            ..Default::default()
        };
        let mut one_placement = instance("one", "one-frame");
        one_placement.scripts.push(ObjectScript {
            event: ObjectEvent::EnterFrame,
            script: script("one-enter"),
        });
        let root = symbol(
            "root",
            1,
            vec![key(vec![one_placement, instance("sibling", "ticker")])],
            vec![FrameScript {
                layer: None,
                frame: 0,
                script: script("root-frame"),
            }],
        );
        let mut one = symbol(
            "one-frame",
            1,
            vec![key(vec![instance("nested", "ticker")])],
            vec![FrameScript {
                layer: None,
                frame: 0,
                script: script("one-frame"),
            }],
        );
        one.linkage_id = Some("OneFrame".into());
        let ticker = symbol(
            "ticker",
            2,
            vec![key(vec![])],
            (0..2)
                .map(|frame| FrameScript {
                    layer: None,
                    frame,
                    script: script(&format!("ticker-{frame}")),
                })
                .collect(),
        );
        let project = make_project(vec![root, one, ticker]);
        project.validate().unwrap();
        let mut player = Player::new(&project);
        let root_ref = player.object_ref(&[]).unwrap();
        let one_ref = player.object_ref(&["one".into()]).unwrap();
        let linked_ref = player.create_linked(&project, "OneFrame").unwrap();
        player.enable_lifecycle();
        let initial: Vec<_> = drain_phases(&mut player)
            .into_iter()
            .flat_map(|step| step.scripts)
            .collect();
        assert_eq!(initial.len(), 6, "root, authored and linked initial frames");
        let stationary: Vec<_> = initial
            .iter()
            .filter(|item| {
                ["root-frame", "one-frame"].contains(&item.dispatch.script.source.as_str())
            })
            .collect();
        assert_eq!(stationary.len(), 3);

        for tick in 1..=4 {
            // A linked namespace begins detached, then attaches, detaches and
            // reattaches. None of these changes re-enters its single frame.
            if tick == 2 || tick == 4 {
                player
                    .display_mutate(
                        &project,
                        DisplayMutation::Add {
                            parent: root_ref.clone(),
                            child: linked_ref.clone(),
                            index: None,
                        },
                    )
                    .unwrap();
            } else if tick == 3 {
                player
                    .display_mutate(
                        &project,
                        DisplayMutation::Remove {
                            parent: root_ref.clone(),
                            child: linked_ref.clone(),
                        },
                    )
                    .unwrap();
            }
            player.tick(&project);
            let steps = drain_phases(&mut player);
            assert_eq!(steps.last().unwrap().phase, LifecyclePhase::Render);
            let scripts: Vec<_> = steps.into_iter().flat_map(|step| step.scripts).collect();
            assert_eq!(scripts.len(), 4, "{language:?}, tick {tick}");
            assert_eq!(
                scripts
                    .iter()
                    .filter(|item| item.dispatch.script.source == format!("ticker-{}", tick % 2))
                    .count(),
                3,
                "authored, sibling and linked children must advance independently"
            );
            assert_eq!(
                scripts
                    .iter()
                    .filter(|item| item.target_ref == one_ref
                        && item.dispatch.event == ScriptEvent::EnterFrame)
                    .count(),
                1
            );
            assert!(
                stationary
                    .iter()
                    .all(|item| player.is_runtime_dispatch_current(&project, item))
            );
            for reference in [&root_ref, &one_ref, &linked_ref] {
                assert_eq!(player.object_frame(reference), Some(0));
                assert!(player.object_alive(reference));
            }
        }

        // Preserve the explicit-navigation contract: only automatic one-frame
        // looping is suppressed, not a requested fresh frame visit.
        for reference in [&root_ref, &one_ref, &linked_ref] {
            player
                .object_begin_inner_goto(&project, reference, PlaybackCommand::GotoAndStop(0))
                .unwrap();
            let scripts: Vec<_> = drain_phases(&mut player)
                .into_iter()
                .flat_map(|step| step.scripts)
                .collect();
            assert_eq!(scripts.len(), 1);
            assert_eq!(&scripts[0].target_ref, reference);
            assert!(player.is_runtime_dispatch_current(&project, &scripts[0]));
        }
    }
}

#[test]
fn late_frame_work_survives_outer_render_and_empty_inner_exit() {
    for inner in [false, true] {
        let root = symbol(
            "root",
            2,
            vec![Keyframe::default()],
            vec![FrameScript {
                layer: None,
                frame: 1,
                script: as3("late-root-frame"),
            }],
        );
        let mut linked = symbol(
            "linked",
            1,
            vec![Keyframe {
                elements: vec![shape("late-child")],
                ..Default::default()
            }],
            vec![FrameScript {
                layer: None,
                frame: 0,
                script: as3("late-linked-frame"),
            }],
        );
        linked.linkage_id = Some("LateLinked".into());
        let project = make_project(vec![root, linked]);
        let mut player = Player::new(&project);
        player.enable_lifecycle();
        if inner {
            player
                .begin_inner_goto(&project, &[], PlaybackCommand::GotoAndStop(0))
                .unwrap();
        }
        let late_phase = if inner {
            LifecyclePhase::FrameScripts
        } else {
            LifecyclePhase::ExitFrame
        };
        loop {
            let step = player.lifecycle_next_step().unwrap().unwrap();
            if step.phase == late_phase {
                assert!(step.scripts.is_empty());
                break;
            }
        }
        // Work queued by an EXIT_FRAME callback (or after an empty inner
        // FrameScripts phase) must remain available to the host's final drain.
        let linked_ref = player.create_linked(&project, "LateLinked").unwrap();
        assert!(player.control(&project, &[], PlaybackCommand::GotoAndStop(1)));
        let remaining = drain_phases(&mut player);
        assert!(
            remaining.iter().any(|step| step
                .display_events
                .iter()
                .any(|event| event.kind == "added" && event.route.contains(&linked_ref))),
            "late linked child construction must not disappear (inner={inner})"
        );
        let mut scripts: Vec<_> = remaining
            .into_iter()
            .flat_map(|step| step.scripts)
            .collect();
        scripts.extend(player.drain_runtime_scripts());
        assert_eq!(
            scripts.len(),
            2,
            "late script work must survive (inner={inner})"
        );
        assert!(
            scripts
                .iter()
                .all(|item| player.is_runtime_dispatch_current(&project, item))
        );
        let mut sources: Vec<_> = scripts
            .iter()
            .map(|item| item.dispatch.script.source.as_str())
            .collect();
        sources.sort_unstable();
        assert_eq!(sources, ["late-linked-frame", "late-root-frame"]);
    }
}

#[test]
fn startup_and_tick_emit_ordered_lifecycle_phases_and_display_events() {
    let mut child_inst = instance("child", "child-sym");
    child_inst.scripts = vec![ObjectScript {
        event: ObjectEvent::Load,
        script: as3("child-load"),
    }];
    let leaf = shape("leaf");
    let root = symbol(
        "root",
        2,
        vec![
            Keyframe {
                frame: 0,
                elements: vec![child_inst],
                tween: None,
                label: String::new(),
                ..Default::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![],
                tween: None,
                label: String::new(),
                ..Default::default()
            },
        ],
        vec![
            FrameScript {
                layer: None,
                frame: 0,
                script: as3("root-f0"),
            },
            FrameScript {
                layer: None,
                frame: 1,
                script: as3("root-f1"),
            },
        ],
    );
    let child_sym = symbol(
        "child-sym",
        1,
        vec![Keyframe {
            frame: 0,
            elements: vec![leaf],
            tween: None,
            label: String::new(),
            ..Default::default()
        }],
        vec![FrameScript {
            layer: None,
            frame: 0,
            script: as3("child-f0"),
        }],
    );
    let proj = make_project(vec![root, child_sym]);
    let mut player = Player::new(&proj);
    player.enable_lifecycle();

    let startup = drain_phases(&mut player);
    let phases: Vec<_> = startup.iter().map(|s| s.phase).collect();
    assert_eq!(
        phases,
        vec![
            LifecyclePhase::Construct,
            LifecyclePhase::FrameConstructed,
            LifecyclePhase::FrameScripts,
            LifecyclePhase::ExitFrame,
            LifecyclePhase::Render,
        ]
    );
    let construct_events: Vec<_> = startup[0]
        .display_events
        .iter()
        .map(|e| (e.kind.as_str(), e.route.len()))
        .collect();
    assert!(construct_events.iter().any(|(kind, _)| *kind == "added"));
    assert!(
        construct_events
            .iter()
            .any(|(kind, _)| *kind == "added_to_stage")
    );
    let startup_scripts: Vec<_> = startup[2]
        .scripts
        .iter()
        .map(|s| s.dispatch.script.source.as_str())
        .collect();
    assert_eq!(startup_scripts, vec!["child-load", "root-f0", "child-f0"]);

    // Advance to frame 1: child subtree retires (deepest first: leaf then child), frame 1 script runs.
    player.tick(&proj);
    let tick1 = drain_phases(&mut player);
    assert_eq!(
        tick1.iter().map(|s| s.phase).collect::<Vec<_>>(),
        vec![
            LifecyclePhase::Construct,
            LifecyclePhase::FrameConstructed,
            LifecyclePhase::FrameScripts,
            LifecyclePhase::ExitFrame,
            LifecyclePhase::Render,
        ]
    );
    let retired: Vec<_> = tick1[0]
        .display_events
        .iter()
        .filter(|e| e.kind == "removed")
        .collect();
    assert_eq!(
        retired.len(),
        2,
        "leaf and child retire when leaving frame 0"
    );
    assert_eq!(
        retired[0].route.len(),
        3,
        "deepest child (leaf -> child -> root) retires first"
    );
    assert_eq!(
        tick1[2]
            .scripts
            .iter()
            .map(|s| s.dispatch.script.source.as_str())
            .collect::<Vec<_>>(),
        vec!["root-f1"]
    );
}

#[test]
fn reentrant_inner_goto_pushes_bounded_continuation_and_supersedes_old_visit() {
    let child_sym = symbol(
        "child-sym",
        2,
        vec![Keyframe {
            frame: 0,
            elements: vec![shape("mark")],
            tween: None,
            label: String::new(),
            ..Default::default()
        }],
        vec![
            FrameScript {
                layer: None,
                frame: 0,
                script: as3("child-f0"),
            },
            FrameScript {
                layer: None,
                frame: 1,
                script: as3("child-f1"),
            },
        ],
    );
    let root = symbol(
        "root",
        3,
        vec![Keyframe {
            frame: 0,
            elements: vec![instance("child", "child-sym")],
            tween: None,
            label: String::new(),
            ..Default::default()
        }],
        vec![
            FrameScript {
                layer: None,
                frame: 0,
                script: as3("root-f0"),
            },
            FrameScript {
                layer: None,
                frame: 1,
                script: as3("root-f1"),
            },
            FrameScript {
                layer: None,
                frame: 2,
                script: as3("root-f2"),
            },
        ],
    );
    let proj = make_project(vec![root, child_sym]);
    let mut player = Player::new(&proj);
    player.enable_lifecycle();

    // Outer FrameScripts returns [root-f0, child-f0]. While processing root-f0, navigate child to frame 1!
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::Construct
    );
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::FrameConstructed
    );
    let outer_scripts = player.lifecycle_next_step().unwrap().unwrap();
    assert_eq!(outer_scripts.phase, LifecyclePhase::FrameScripts);
    assert_eq!(outer_scripts.scripts.len(), 2);
    let child_old_dispatch = outer_scripts.scripts[1].clone();

    let child_path = vec!["child".to_string()];
    let inner_depth = player
        .begin_inner_goto(&proj, &child_path, PlaybackCommand::GotoAndStop(1))
        .expect("begin_inner_goto");
    assert_eq!(inner_depth, 2);

    // Inner scope runs Construct -> FrameConstructed -> FrameScripts (child-f1) -> ExitFrame (no Render!).
    let mut inner_phases = Vec::new();
    let mut inner_sources = Vec::new();
    while player.lifecycle_depth() >= inner_depth {
        let step = player.lifecycle_next_step().unwrap().expect("inner step");
        inner_phases.push(step.phase);
        assert!(step.inner_goto);
        for s in step.scripts {
            inner_sources.push(s.dispatch.script.source);
        }
    }
    assert_eq!(
        inner_phases,
        vec![
            LifecyclePhase::Construct,
            LifecyclePhase::FrameConstructed,
            LifecyclePhase::FrameScripts,
            LifecyclePhase::ExitFrame,
        ]
    );
    assert_eq!(inner_sources, vec!["child-f1"]);
    assert!(
        !player.is_runtime_dispatch_current(&proj, &child_old_dispatch),
        "old child-f0 dispatch must be invalidated by visit_id after goto"
    );

    // Outer scope resumes cleanly at ExitFrame -> Render.
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::ExitFrame
    );
    assert_eq!(
        player.lifecycle_next_step().unwrap().unwrap().phase,
        LifecyclePhase::Render
    );
    assert!(player.lifecycle_next_step().unwrap().is_none());

    // Bounded nesting depth refuses infinite recursive goto chains.
    for _ in 0..MAX_LIFECYCLE_DEPTH {
        let _ = player.begin_inner_goto(&proj, &[], PlaybackCommand::GotoAndStop(2));
    }
    let overflow = player.begin_inner_goto(&proj, &[], PlaybackCommand::GotoAndStop(2));
    assert!(
        overflow.is_err(),
        "exceeding MAX_LIFECYCLE_DEPTH must error"
    );
}
