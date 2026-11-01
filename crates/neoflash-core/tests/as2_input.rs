use neoflash_core::*;
use serde_json::{Value, json};
fn command(player: &mut Player, project: &Project, target: &ObjectRef, value: Value) -> Value {
    serde_json::from_str(&player.as2_input_command(project, target, value).unwrap()).unwrap()
}
#[test]
fn as2_focus_flags_and_custom_tab_order_respect_graph_ancestry() {
    let project = Project::blank();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let a = player.create_display(DynamicKind::MovieClip).unwrap();
    let b = player.create_display(DynamicKind::MovieClip).unwrap();
    player.as2_place_depth(&project, &root, &a, 1).unwrap();
    player.as2_place_depth(&project, &root, &b, 2).unwrap();
    for target in [&a, &b] {
        command(
            &mut player,
            &project,
            target,
            json!({"op":"set","name":"trackAsMenu","value":true}),
        );
    }
    assert_eq!(
        command(
            &mut player,
            &project,
            &a,
            json!({"op":"menu_transfer","target":b})
        ),
        true
    );
    command(
        &mut player,
        &project,
        &b,
        json!({"op":"set","name":"trackAsMenu","value":false}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &a,
            json!({"op":"menu_transfer","target":b})
        ),
        false
    );
    command(
        &mut player,
        &project,
        &a,
        json!({"op":"set","name":"_focusrect","value":true}),
    );
    assert_eq!(
        command(&mut player, &project, &a, json!({"op":"get"}))["focus_rect"],
        true
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &a,
            json!({"op":"focusable","has_button_handler":false})
        ),
        false
    );
    command(
        &mut player,
        &project,
        &a,
        json!({"op":"set","name":"focusEnabled","value":true}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &a,
            json!({"op":"focusable","has_button_handler":false})
        ),
        true
    );
    for (target, index) in [(&a, 2), (&b, 1)] {
        command(
            &mut player,
            &project,
            target,
            json!({"op":"set","name":"tabEnabled","value":true}),
        );
        command(
            &mut player,
            &project,
            target,
            json!({"op":"set","name":"tabIndex","value":index}),
        );
    }
    assert_eq!(
        command(
            &mut player,
            &project,
            &root,
            json!({"op":"tab_order","button_targets":[]})
        ),
        json!([b, a])
    );
    command(
        &mut player,
        &project,
        &b,
        json!({"op":"set","name":"enabled","value":false}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &root,
            json!({"op":"tab_order","button_targets":[]})
        ),
        json!([a])
    );
    command(
        &mut player,
        &project,
        &root,
        json!({"op":"set","name":"tabChildren","value":false}),
    );
    assert_eq!(
        command(
            &mut player,
            &project,
            &root,
            json!({"op":"tab_order","button_targets":[]})
        ),
        json!([])
    );
    command(
        &mut player,
        &project,
        &root,
        json!({"op":"set","name":"tabChildren","value":true}),
    );
    assert!(
        player
            .as2_input_command(
                &project,
                &a,
                json!({"op":"set","name":"tabIndex","value":-1})
            )
            .is_err()
    );
    assert_eq!(
        command(&mut player, &project, &a, json!({"op":"get"}))["tab_index"],
        2
    );
    player.reset(&project);
    assert!(
        player
            .as2_input_command(&project, &a, json!({"op":"get"}))
            .is_err()
    );
}

#[test]
fn as2_publication_settings_lockroot_and_scale9_grid_roundtrip_and_reset() {
    let mut project = Project::blank();
    project.symbols.push(
        serde_json::from_value(json!({
            "id": "btn_sym",
            "name": "ButtonSymbol",
            "kind": "button",
            "timeline": {
                "frame_count": 4,
                "layers": [{
                    "id": "btn_layer",
                    "name": "button",
                    "visible": true,
                    "locked": false,
                    "keyframes": [{
                        "frame": 0,
                        "elements": [{
                            "id": "btn_rect",
                            "name": "",
                            "transform": {"x": 0.0, "y": 0.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0},
                            "kind": {"type": "rectangle", "width": 80.0, "height": 30.0, "fill": [40, 120, 220, 255], "stroke": [0, 0, 0, 0], "stroke_width": 0.0},
                            "scripts": []
                        }],
                        "tween": null,
                        "label": ""
                    }]
                }],
                "scripts": []
            }
        }))
        .unwrap(),
    );
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(
            serde_json::from_value(json!({
                "id": "btn_inst",
                "name": "actionBtn",
                "transform": {"x": 0.0, "y": 0.0, "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0, "alpha": 1.0},
                "kind": {"type": "instance", "symbol_id": "btn_sym", "playback": "independent"},
                "scripts": []
            }))
            .unwrap(),
        );

    let mut player = Player::new(&project);
    let settings = |player: &mut Player, req: Value| -> Value {
        serde_json::from_str(&player.as2_settings_command(req).unwrap()).unwrap()
    };

    let initial = settings(&mut player, json!({"op":"get"}));
    assert_eq!(initial["quality"], "HIGH");
    assert_eq!(initial["high_quality"], 1);
    assert_eq!(initial["sound_buf_time"], 5);
    assert_eq!(initial["focus_rect"], true);
    assert_eq!(initial["redraw_regions"], false);

    let best = settings(&mut player, json!({"op":"set_quality","value":"best"}));
    assert_eq!(best["quality"], "BEST");
    assert_eq!(best["high_quality"], 2);
    let ignored = settings(&mut player, json!({"op":"set_quality","value":"bogus"}));
    assert_eq!(ignored["quality"], "BEST");

    let toggled = settings(&mut player, json!({"op":"toggle_high_quality"}));
    assert_eq!(toggled["quality"], "LOW");
    assert_eq!(toggled["high_quality"], 0);
    let toggled_back = settings(&mut player, json!({"op":"toggle_high_quality"}));
    assert_eq!(toggled_back["quality"], "HIGH");
    assert_eq!(toggled_back["high_quality"], 1);

    let hq = settings(&mut player, json!({"op":"set_high_quality","value":2.0}));
    assert_eq!(hq["quality"], "BEST");
    let buf = settings(&mut player, json!({"op":"set_sound_buf_time","value":12.0}));
    assert_eq!(buf["sound_buf_time"], 12);
    let focus = settings(&mut player, json!({"op":"set_focus_rect","value":false}));
    assert_eq!(focus["focus_rect"], false);
    let redraw = settings(
        &mut player,
        json!({"op":"show_redraw_regions","enable":true,"color":0x00ff88}),
    );
    assert_eq!(redraw["redraw_regions"], true);
    assert_eq!(redraw["redraw_region_color"], 0x00ff88);
    assert!(
        player
            .as2_settings_command(
                json!({"op":"show_redraw_regions","enable":true,"color":0x1000000})
            )
            .is_err()
    );

    let root = player.object_ref(&[]).unwrap();
    let container = player.create_display(DynamicKind::MovieClip).unwrap();
    let inner = player.create_display(DynamicKind::MovieClip).unwrap();
    assert!(player.object_set_name(&container, "container"));
    assert!(player.object_set_name(&inner, "inner"));
    player
        .as2_place_depth(&project, &root, &container, 1)
        .unwrap();
    player
        .as2_place_depth(&project, &container, &inner, 1)
        .unwrap();

    assert_eq!(player.as2_root(&inner).unwrap(), root);
    command(
        &mut player,
        &project,
        &container,
        json!({"op":"set","name":"_lockroot","value":true}),
    );
    assert_eq!(
        command(&mut player, &project, &container, json!({"op":"get"}))["lock_root"],
        true
    );
    assert_eq!(player.as2_root(&container).unwrap(), container);
    assert_eq!(player.as2_root(&inner).unwrap(), container);
    assert_eq!(
        player
            .as2_resolve_target(&project, &inner, "_root")
            .unwrap(),
        Some(container.clone())
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &inner, "_root.inner")
            .unwrap(),
        Some(inner.clone())
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &inner, "/inner")
            .unwrap(),
        Some(inner.clone())
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &inner, "_level0")
            .unwrap(),
        Some(root.clone())
    );

    let button = player.object_ref(&["btn_inst".to_string()]).unwrap();
    for target in [&container, &button] {
        let updated = player
            .as2_render_command(
                &project,
                target,
                json!({"op":"set","name":"scale9Grid","value":{"x":10.0,"y":8.0,"width":40.0,"height":14.0}}),
            )
            .unwrap();
        assert_eq!(updated["scale9Grid"], json!([10.0, 8.0, 40.0, 14.0]));
        assert!(
            player
                .as2_render_command(
                    &project,
                    target,
                    json!({"op":"set","name":"scale9Grid","value":{"x":0.0,"y":0.0,"width":0.0,"height":10.0}}),
                )
                .is_err()
        );
        let cleared = player
            .as2_render_command(
                &project,
                target,
                json!({"op":"set","name":"scale9Grid","value":null}),
            )
            .unwrap();
        assert!(cleared["scale9Grid"].is_null());
    }

    player.reset(&project);
    let after_reset = settings(&mut player, json!({"op":"get"}));
    assert_eq!(after_reset["quality"], "HIGH");
    assert_eq!(after_reset["sound_buf_time"], 5);
    assert_eq!(after_reset["focus_rect"], true);
    assert_eq!(after_reset["redraw_regions"], false);
}

#[test]
fn as2_level_bitmap_fill_call_frame_loaded_and_accessibility_semantics() {
    let mut project = Project::blank();
    project.symbols[0].timeline.frame_count = 3;
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![
        Element {
            id: "hero_shape".into(),
            name: "hero".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 40.0,
                height: 20.0,
                fill: [30, 120, 220, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            scripts: vec![],
            accessibility: None,
        },
        Element {
            id: "child_inst".into(),
            name: "child".into(),
            transform: Transform::default(),
            kind: ElementKind::Instance {
                symbol_id: "sub_mc".into(),
                playback: InstancePlayback::Independent,
            },
            scripts: vec![],
            accessibility: None,
        },
    ];
    let frame0_elements = project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .clone();
    project.symbols[0].timeline.layers[0]
        .keyframes
        .push(Keyframe {
            frame: 1,
            elements: frame0_elements,
            tween: None,
            tween_kind: TweenKind::default(),
            keyed_properties: None,
            orient_to_path: false,
            sound: None,
            label: "ready".into(),
            ..Default::default()
        });
    project.symbols[0].timeline.scripts = vec![
        FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace('f1');".into(),
            },
        },
        FrameScript {
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript2,
                source: "trace('f2');".into(),
            },
        },
    ];
    project.symbols.push(Symbol {
        id: "sub_mc".into(),
        name: "SubMc".into(),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        linkage_id: Some("SubMc".into()),
        class_name: None,
        timeline: Timeline {
            frame_count: 2,
            layers: vec![Layer {
                id: "sub_layer".into(),
                name: "sub".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![
                    Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "sub_rect".into(),
                            name: "".into(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 24.0,
                                height: 24.0,
                                fill: [200, 60, 40, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            scripts: vec![],
                            accessibility: None,
                        }],
                        tween: None,
                        tween_kind: TweenKind::default(),
                        keyed_properties: None,
                        orient_to_path: false,
                        sound: None,
                        label: "".into(),
                        ..Default::default()
                    },
                    Keyframe {
                        frame: 1,
                        elements: vec![],
                        tween: None,
                        tween_kind: TweenKind::default(),
                        keyed_properties: None,
                        orient_to_path: false,
                        sound: None,
                        label: "helper".into(),
                        ..Default::default()
                    },
                ],
            }],
            scripts: vec![FrameScript {
                frame: 1,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: "calledSub = 1;".into(),
                },
            }],
            guides: vec![],
        },
    });

    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let sub = player.object_ref(&["child_inst".to_string()]).unwrap();

    // _level0 resolves to root; _levelN (N >= 1) resolves cleanly to None.
    assert_eq!(
        player
            .as2_resolve_target(&project, &sub, "_level0")
            .unwrap(),
        Some(root.clone())
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &sub, "_level1")
            .unwrap(),
        None
    );
    assert_eq!(
        player
            .as2_resolve_target(&project, &sub, "_level2.nested")
            .unwrap(),
        None
    );

    // ifFrameLoaded checks authored frames and labels without side effects.
    assert!(
        player
            .as2_is_frame_loaded(
                &project,
                &root,
                &GotoRequest {
                    frame: GotoFrame::Number(1.0),
                    scene: None,
                    as2: true,
                },
            )
            .unwrap()
    );
    assert!(
        player
            .as2_is_frame_loaded(
                &project,
                &root,
                &GotoRequest {
                    frame: GotoFrame::Number(3.0),
                    scene: None,
                    as2: true,
                },
            )
            .unwrap()
    );
    assert!(
        !player
            .as2_is_frame_loaded(
                &project,
                &root,
                &GotoRequest {
                    frame: GotoFrame::Number(4.0),
                    scene: None,
                    as2: true,
                },
            )
            .unwrap()
    );
    assert!(
        player
            .as2_is_frame_loaded(
                &project,
                &root,
                &GotoRequest {
                    frame: GotoFrame::Label("ready".into()),
                    scene: None,
                    as2: true,
                },
            )
            .unwrap()
    );
    assert!(
        !player
            .as2_is_frame_loaded(
                &project,
                &root,
                &GotoRequest {
                    frame: GotoFrame::Label("missing".into()),
                    scene: None,
                    as2: true,
                },
            )
            .unwrap()
    );

    // call() resolves frame scripts by number, label, and target:frame without advancing playhead.
    let (called_target, scripts) = player
        .as2_call_frame_scripts(&project, &root, &GotoFrame::Label("ready".into()))
        .unwrap()
        .unwrap();
    assert_eq!(called_target, root);
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].source, "trace('f2');");
    assert_eq!(player.object_frame(&root), Some(0));

    let (sub_target, sub_scripts) = player
        .as2_call_frame_scripts(&project, &root, &GotoFrame::Label("child:helper".into()))
        .unwrap()
        .unwrap();
    assert_eq!(sub_target, sub);
    assert_eq!(sub_scripts.len(), 1);
    assert_eq!(sub_scripts[0].source, "calledSub = 1;");
    assert_eq!(player.object_frame(&sub), Some(0));
    assert!(
        player
            .as2_call_frame_scripts(&project, &root, &GotoFrame::Number(99.0))
            .unwrap()
            .is_none()
    );

    // BeginBitmapFill produces a GradientPath with fill_bitmap and drops disposed PixelRefs.
    let pixel = player.bitmap_create(4, 4, true, 0xff22aa88).unwrap();
    let canvas = player.create_display(DynamicKind::MovieClip).unwrap();
    player
        .as2_place_depth(&project, &root, &canvas, 10)
        .unwrap();
    player
        .graphics_apply(
            &canvas,
            &[
                GraphicsOp::BeginBitmapFill {
                    bitmap: BitmapFillSpec {
                        pixel_ref: pixel.clone(),
                        matrix: [1.0, 0.0, 0.0, 1.0, 4.0, 6.0],
                        repeat: false,
                        smoothing: true,
                    },
                },
                GraphicsOp::DrawRect {
                    x: 0.0,
                    y: 0.0,
                    width: 16.0,
                    height: 12.0,
                },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    let snap = player.runtime_snapshot(&project);
    let draw = snap
        .draws
        .iter()
        .find(|item| item.owner_ref == canvas)
        .expect("bitmap fill draw");
    match &draw.item.kind {
        RuntimeDrawKind::GradientPath {
            fill_bitmap: Some(paint),
            ..
        } => {
            assert_eq!(paint.pixel_ref, pixel);
            assert_eq!(paint.width, 4);
            assert_eq!(paint.height, 4);
            assert!(!paint.repeat);
            assert!(paint.smoothing);
            assert_eq!(paint.matrix, [1.0, 0.0, 0.0, 1.0, 4.0, 6.0]);
        }
        other => panic!("expected GradientPath with fill_bitmap, got {other:?}"),
    }
    player.bitmap_dispose(&pixel).unwrap();
    assert!(
        !player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .any(|item| item.owner_ref == canvas),
        "disposing bitmap fill pixels omits the fill draw cleanly"
    );
    assert!(
        player
            .graphics_apply(
                &canvas,
                &[GraphicsOp::BeginBitmapFill {
                    bitmap: BitmapFillSpec {
                        pixel_ref: pixel,
                        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                        repeat: true,
                        smoothing: false,
                    },
                }],
            )
            .is_err(),
        "BeginBitmapFill rejects disposed PixelRefs"
    );

    // AS2 _accProps updates runtime_snapshot().semantics, respects silent and enabled=false.
    command(
        &mut player,
        &project,
        &sub,
        json!({"op":"set","name":"enabled","value":false}),
    );
    player
        .as2_accessibility_update(vec![As2AccUpdateEntry {
            object_ref: sub.clone(),
            props: As2AccProps {
                silent: false,
                force_simple: true,
                name: Some("Play Clip".into()),
                description: Some("Starts clip".into()),
                interactive: true,
            },
        }])
        .unwrap();
    let sem_snap = player.runtime_snapshot(&project);
    let node = sem_snap
        .semantics
        .iter()
        .find(|n| n.owner_ref == sub)
        .expect("semantic node for AS2 _accProps");
    assert_eq!(node.node.accessibility.name, "Play Clip");
    assert_eq!(node.node.accessibility.description, "Starts clip");
    assert_eq!(node.node.accessibility.role, AccessibleRole::Button);
    assert!(node.node.accessibility.disabled);

    player
        .as2_accessibility_update(vec![As2AccUpdateEntry {
            object_ref: sub.clone(),
            props: As2AccProps {
                silent: true,
                force_simple: false,
                name: Some("Hidden".into()),
                description: None,
                interactive: true,
            },
        }])
        .unwrap();
    assert!(
        !player
            .runtime_snapshot(&project)
            .semantics
            .iter()
            .any(|n| n.owner_ref == sub),
        "silent _accProps suppresses semantic output"
    );
}
