use neoflash_core::*;

fn shape(id: &str, x: f64) -> Element {
    Element {
        accessibility: None,
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 10.,
            height: 10.,
            fill: [255, 100, 0, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        },
        scripts: vec![],
        ..Element::default()
    }
}
fn instance(id: &str, target: &str, x: f64) -> Element {
    Element {
        kind: ElementKind::Instance {
            symbol_id: target.into(),
            playback: InstancePlayback::Independent,
        },
        ..shape(id, x)
    }
}
fn key(frame: u32, elements: Vec<Element>, tween: Option<Easing>) -> Keyframe {
    Keyframe {
        frame,
        elements,
        tween,
        label: String::new(),
        ..Default::default()
    }
}
fn symbol(id: &str, frames: u32, keys: Vec<Keyframe>) -> Symbol {
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
                keyframes: keys,
            }],
            scripts: vec![],
        },
    }
}
fn project(symbols: Vec<Symbol>) -> Project {
    Project {
        library_folders: vec![],
        sources: vec![],
        resources: vec![],
        document_class: None,
        scenes: vec![],
        script_language: None,
        also_compile_compatible_scripts: true,
        format_version: 1,
        name: "Test".into(),
        width: 640,
        height: 480,
        fps: 24,
        background: [255, 255, 255, 255],
        root: symbols[0].id.clone(),
        symbols,
    }
}
fn js(source: &str) -> Script {
    Script {
        language: ScriptLanguage::JavaScript,
        source: source.into(),
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn nested_affine_composition_and_opacity() {
    let mut outer = instance("outer", "child", 10.);
    outer.transform.y = 20.;
    outer.transform.rotation = 90.;
    outer.transform.scale_x = 2.;
    outer.transform.scale_y = 3.;
    outer.transform.alpha = 0.5;
    let mut leaf = shape("leaf", 5.);
    leaf.transform.y = 7.;
    leaf.transform.alpha = 0.4;
    let p = project(vec![
        symbol("root", 20, vec![key(0, vec![outer], None)]),
        symbol("child", 10, vec![key(0, vec![leaf], None)]),
    ]);
    p.validate().unwrap();
    let items = evaluate(&p, "root", 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].instance_path, vec!["outer", "leaf"]);
    near(items[0].transform[4], -11.);
    near(items[0].transform[5], 30.);
    near(items[0].alpha, 0.2);

    // Outline colors apply through both instance and folder ancestry, while the
    // runtime snapshot retains the original artwork and colors.
    assert!(!p.to_json().unwrap().contains("outline_color"));
    let mut outlined = p.clone();
    outlined.format_version = 4;
    outlined.symbols[0].timeline.layers[0].properties.outline = true;
    outlined.symbols[0].timeline.layers[0]
        .properties
        .outline_color = Some([30, 150, 220]);
    outlined.symbols[1].timeline.layers[0].properties.outline = true;
    outlined.symbols[1].timeline.layers[0]
        .properties
        .outline_color = Some([230, 80, 40]);
    assert_eq!(
        evaluate_scene(&outlined, "root", 0, true, true).draws[0].outline_color,
        Some([30, 150, 220, 255])
    );
    assert_eq!(
        evaluate_scene(&outlined, "child", 0, true, true).draws[0].outline_color,
        Some([230, 80, 40, 255])
    );
    outlined.symbols[0].timeline.layers[0].properties.parent = Some("outline-folder".into());
    outlined.symbols[0].timeline.layers.push(Layer {
        id: "outline-folder".into(),
        name: "Folder".into(),
        visible: true,
        locked: false,
        keyframes: vec![],
        properties: LayerProperties {
            role: LayerRole::Folder,
            outline: true,
            outline_color: Some([70, 180, 90]),
            ..Default::default()
        },
    });
    assert_eq!(
        evaluate_scene(&outlined, "root", 0, true, true).draws[0].outline_color,
        Some([70, 180, 90, 255])
    );
    assert_eq!(
        Project::from_json(&outlined.to_json().unwrap()).unwrap(),
        outlined
    );
    let player = Player::new(&outlined);
    let published = serde_json::to_value(player.runtime_snapshot(&outlined)).unwrap();
    for layer in outlined
        .symbols
        .iter_mut()
        .flat_map(|symbol| &mut symbol.timeline.layers)
    {
        layer.properties.outline = false;
        layer.properties.outline_color = None;
    }
    assert_eq!(
        serde_json::to_value(player.runtime_snapshot(&outlined)).unwrap(),
        published
    );
    assert_eq!(
        evaluate_scene(&outlined, "root", 0, true, true).draws[0].outline_color,
        None
    );

    // Authored layers share their clip plan between scrubbing and the browser
    // runtime; guides never become live display objects or dispatch scripts.
    let mut authored = project(vec![symbol(
        "root",
        3,
        vec![key(0, vec![shape("content", 0.)], None)],
    )]);
    authored.format_version = 4;
    let timeline = &mut authored.symbols[0].timeline;
    timeline.layers[0].properties.mask = Some("mask-layer".into());
    let mut mask_shape = shape("mask-shape", 4.);
    mask_shape.kind = ElementKind::Ellipse {
        width: 8.,
        height: 8.,
        fill: [0, 0, 0, 255],
        stroke: [0; 4],
        stroke_width: 0.,
    };
    timeline.layers.push(Layer {
        id: "mask-layer".into(),
        name: "Mask".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            role: LayerRole::Mask,
            ..Default::default()
        },
        keyframes: vec![key(0, vec![mask_shape], None), key(2, vec![], None)],
    });
    timeline.layers.push(Layer {
        id: "guide-layer".into(),
        name: "Guide".into(),
        visible: true,
        locked: false,
        properties: LayerProperties {
            role: LayerRole::Guide,
            ..Default::default()
        },
        keyframes: vec![key(0, vec![shape("guide-shape", 100.)], None)],
    });
    authored.validate().unwrap();
    assert_eq!(
        Project::from_json(&authored.to_json().unwrap()).unwrap(),
        authored
    );
    let scene = evaluate_scene(&authored, "root", 0, true, true);
    assert_eq!(scene.draws.len(), 2);
    assert!(scene.contains_point(&["content".into()], [8., 4.]));
    assert!(!scene.contains_point(&["content".into()], [4., 0.]));
    assert!(!scene.contains_point(&["content".into()], [0., 4.]));
    assert_eq!(
        evaluate_scene(&authored, "root", 0, true, false)
            .draws
            .len(),
        3
    );
    let player = Player::new(&authored);
    let runtime = player.runtime_snapshot(&authored);
    assert_eq!(runtime.draws.len(), 1);
    assert_eq!(runtime.draws[0].clip_ids.len(), 1);
    assert_eq!(runtime.clips.len(), 1);
    assert_eq!(runtime.clips[0].components[0].matrix[4], 4.);
    assert!(
        runtime
            .objects
            .iter()
            .all(|node| node.display.name != "guide-shape")
    );
    assert!(
        !evaluate_scene(&authored, "root", 2, false, true)
            .contains_point(&["content".into()], [8., 4.])
    );
    let mut invalid = authored.clone();
    invalid.symbols[0].timeline.layers[0].properties.mask = Some("missing-mask".into());
    assert!(invalid.validate().is_err());
}

#[test]
fn tween_matches_identity_not_array_position() {
    let p = project(vec![symbol(
        "root",
        11,
        vec![
            key(
                0,
                vec![shape("a", 0.), shape("b", 100.)],
                Some(Easing::Linear),
            ),
            key(10, vec![shape("b", 200.), shape("a", 20.)], None),
        ],
    )]);
    let items = evaluate(&p, "root", 5);
    near(items[0].transform[4], 10.);
    near(items[1].transform[4], 150.);
    let before = p.clone();
    evaluate(&p, "root", 3);
    evaluate(&p, "root", 8);
    assert_eq!(before, p);
}

#[test]
fn bezier_solves_horizontal_parameter_and_all_ease_endpoints() {
    let ease = Easing::CubicBezier {
        x1: 0.42,
        y1: 0.,
        x2: 0.58,
        y2: 1.,
    };
    near(easing(0.5, ease), 0.5);
    assert!(easing(0.25, ease) < 0.25);
    for e in [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        ease,
    ] {
        near(easing(0., e), 0.);
        near(easing(1., e), 1.);
    }
}

#[test]
fn independent_instances_and_stopped_parent() {
    let p = project(vec![
        symbol(
            "root",
            20,
            vec![key(
                0,
                vec![instance("one", "clip", 0.), instance("two", "clip", 100.)],
                None,
            )],
        ),
        symbol(
            "clip",
            10,
            vec![
                key(0, vec![shape("leaf", 0.)], Some(Easing::Linear)),
                key(9, vec![shape("leaf", 90.)], None),
            ],
        ),
    ]);
    let mut player = Player::new(&p);
    assert!(player.control(&p, &[], PlaybackCommand::Stop));
    assert!(player.control(&p, &["one".into()], PlaybackCommand::GotoAndStop(4)));
    player.tick(&p);
    player.tick(&p);
    assert_eq!(player.frame(), 0);
    assert_eq!(player.clip_frame(&["one".into()]), Some(4));
    assert_eq!(player.clip_frame(&["two".into()]), Some(2));
    let items = player.evaluate(&p);
    near(items[0].transform[4], 40.);
    near(items[1].transform[4], 120.);
    player.set_playing(false);
    player.tick(&p);
    assert_eq!(player.clip_frame(&["two".into()]), Some(2));
    player.reset(&p);
    assert!(player.is_playing());
    assert_eq!(player.clip_frame(&["one".into()]), Some(0));
}

#[test]
fn removed_and_reinserted_instances_restart() {
    let p = project(vec![
        symbol(
            "root",
            8,
            vec![
                // Serialized keys need not be ordered. Holds are bounded by
                // the next chronological key, including an empty key.
                key(6, vec![instance("one", "clip", 0.)], None),
                key(2, vec![instance("one", "clip", 0.)], None),
                key(4, vec![], None),
            ],
        ),
        symbol("clip", 10, vec![key(0, vec![shape("leaf", 0.)], None)]),
    ]);
    p.validate().unwrap();
    let authored = p.to_json().unwrap();
    let visible = [false, false, true, true, false, false, true, true];
    for (frame, expected) in visible.into_iter().enumerate() {
        assert_eq!(
            !evaluate(&p, "root", frame as u32).is_empty(),
            expected,
            "authoring exposure at frame {frame}"
        );
    }
    assert_eq!(p.to_json().unwrap(), authored);

    let mut player = Player::new(&p);
    let path = vec!["one".into()];
    assert!(player.evaluate(&p).is_empty());
    player.tick(&p);
    assert!(player.evaluate(&p).is_empty());
    player.tick(&p);
    assert_eq!(player.clip_frame(&path), Some(0));
    let first = player.object_ref(&path).unwrap();
    player.tick(&p);
    assert_eq!(player.clip_frame(&path), Some(1));
    assert_eq!(player.object_ref(&path), Some(first.clone()));
    player.tick(&p);
    assert_eq!(player.clip_frame(&path), None);
    assert!(!player.object_alive(&first));
    assert!(player.evaluate(&p).is_empty());
    assert!(player.runtime_snapshot(&p).draws.is_empty());
    player.tick(&p);
    assert!(player.evaluate(&p).is_empty());
    player.tick(&p);
    assert_eq!(player.clip_frame(&path), Some(0));
    assert_ne!(player.object_ref(&path), Some(first));
    player.tick(&p);
    assert_eq!(player.clip_frame(&path), Some(1));
    assert_eq!(player.evaluate(&p).len(), 1);
}

#[test]
fn frame_load_enter_and_click_dispatch_order_is_defined() {
    let mut leaf = shape("leaf", 0.);
    leaf.scripts = vec![
        ObjectScript {
            event: ObjectEvent::Load,
            script: js("leaf-load"),
        },
        ObjectScript {
            event: ObjectEvent::EnterFrame,
            script: js("leaf-enter"),
        },
        ObjectScript {
            event: ObjectEvent::Click,
            script: js("leaf-click"),
        },
    ];
    let mut outer = instance("outer", "clip", 0.);
    outer.scripts = vec![
        ObjectScript {
            event: ObjectEvent::Load,
            script: js("outer-load"),
        },
        ObjectScript {
            event: ObjectEvent::Click,
            script: js("outer-click"),
        },
    ];
    let mut p = project(vec![
        symbol("root", 2, vec![key(0, vec![outer], None)]),
        symbol("clip", 2, vec![key(0, vec![leaf], None)]),
    ]);
    p.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: js("root-frame0"),
        },
        FrameScript {
            layer: None,
            frame: 1,
            script: js("root-frame1"),
        },
    ];
    p.symbols[1].timeline.scripts = vec![
        FrameScript {
            layer: None,
            frame: 0,
            script: js("clip-frame0"),
        },
        FrameScript {
            layer: None,
            frame: 1,
            script: js("clip-frame1"),
        },
    ];
    let mut player = Player::new(&p);
    let source = |events: Vec<ScriptDispatch>| {
        events
            .into_iter()
            .map(|e| e.script.source)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        source(player.drain_scripts()),
        vec!["outer-load", "leaf-load", "root-frame0", "clip-frame0"]
    );
    evaluate(&p, "root", 1);
    assert!(player.drain_scripts().is_empty());
    player.tick(&p);
    assert_eq!(
        source(player.drain_scripts()),
        vec!["root-frame1", "clip-frame1", "leaf-enter"]
    );
    player.dispatch_click(&p, &["outer".into(), "leaf".into()]);
    assert_eq!(
        source(player.drain_scripts()),
        vec!["leaf-click", "outer-click"]
    );
}

#[test]
fn self_goto_dispatches_are_bounded_across_drains() {
    let mut p = project(vec![symbol("root", 1, vec![key(0, vec![], None)])]);
    p.symbols[0].timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: js("gotoAndStop(1)"),
    }];
    let mut player = Player::new(&p);
    let mut count = 0;
    while !player.drain_scripts().is_empty() {
        count += 1;
        player.control(&p, &[], PlaybackCommand::GotoAndStop(0));
        assert!(count <= 4096);
    }
    assert_eq!(count, 4096);
    assert!(player.script_overflowed());
}

#[test]
fn local_script_overrides_compose_with_parent_and_clear_on_removal() {
    let p = project(vec![
        symbol(
            "root",
            4,
            vec![
                key(0, vec![instance("one", "clip", 10.)], None),
                key(2, vec![], None),
            ],
        ),
        symbol("clip", 10, vec![key(0, vec![shape("leaf", 2.)], None)]),
    ]);
    let mut player = Player::new(&p);
    assert!(player.set_property(&["one".into()], "scale_x", 3.));
    assert!(player.set_property(&["one".into(), "leaf".into()], "x", 5.));
    near(player.evaluate(&p)[0].transform[4], 25.);
    assert_eq!(
        player.property(&p, &["one".into(), "leaf".into()], "x"),
        Some(5.)
    );
    assert!(!player.set_property(&["one".into()], "alpha", 2.));
    player.tick(&p);
    player.tick(&p);
    assert!(player.evaluate(&p).is_empty());
    assert!(!player.set_property(&["one".into()], "x", 2.));
}

#[test]
fn persistence_validation_and_import_id_reservation() {
    let p = Project::demo();
    p.validate().unwrap();
    let json = p.to_json().unwrap();
    let round = Project::from_json(&json).unwrap();
    assert_eq!(p, round);
    let mut invalid = p.clone();
    invalid.format_version = u32::MAX;
    assert!(invalid.to_json().is_err());
    invalid = p.clone();
    invalid.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .x = f64::NAN;
    assert!(invalid.validate().is_err());
    invalid = p.clone();
    invalid.symbols[0].timeline.layers[0].keyframes[0].frame = 100000;
    assert!(invalid.validate().is_err());
    let reserved = project(vec![symbol("reserved-1", 1, vec![key(0, vec![], None)])]);
    let _ = Project::from_json(&reserved.to_json().unwrap()).unwrap();
    for _ in 0..20 {
        assert_ne!(new_id("reserved"), "reserved-1");
    }
}

#[test]
fn validation_catches_cycles_missing_references_and_duplicate_identity() {
    let cyclic = project(vec![symbol(
        "root",
        1,
        vec![key(0, vec![instance("self", "root", 0.)], None)],
    )]);
    assert!(
        validate(&cyclic)
            .iter()
            .any(|i| i.message.contains("cyclic"))
    );
    let missing = project(vec![symbol(
        "root",
        1,
        vec![key(0, vec![instance("bad", "missing", 0.)], None)],
    )]);
    assert!(missing.validate().is_err());
    let duplicates = project(vec![symbol(
        "root",
        1,
        vec![key(0, vec![shape("same", 0.), shape("same", 10.)], None)],
    )]);
    assert!(duplicates.validate().is_err());
    let mut shared = Project::demo();
    let id = shared.symbols[0].timeline.layers[0].id.clone();
    shared.symbols[1].timeline.layers[0].id = id;
    assert!(shared.validate().is_err());
}

#[test]
fn history_groups_drag_and_invalidates_redo_on_new_edit() {
    let mut p = Project::blank();
    let original = p.clone();
    let mut h = History::new(2);
    h.begin(&p);
    p.name = "drag intermediate".into();
    h.begin(&p);
    p.name = "drag final".into();
    assert!(h.commit(&p));
    assert_eq!(h.undo_label(), Some("Edit"));
    assert!(h.undo(&mut p));
    assert_eq!(p, original);
    assert!(!h.can_undo());
    assert_eq!(h.redo_label(), Some("Edit"));
    assert!(h.redo(&mut p));
    assert_eq!(p.name, "drag final");
    assert!(h.undo(&mut p));
    h.begin(&p);
    p.name = "other".into();
    h.commit(&p);
    assert!(!h.can_redo());
    h.begin(&p);
    p.name = "cancel".into();
    assert!(h.cancel(&mut p));
    assert_eq!(p.name, "other");
    h.begin(&p);
    assert!(!h.commit(&p));

    // Named steps preserve chronological labels across undo, redo and step_to.
    let mut p = Project::blank();
    let mut h = History::new(10);
    for (label, name) in [
        ("Oval", "step-1"),
        ("Move", "step-2"),
        ("Fill colour", "step-3"),
        ("Transform", "step-4"),
        ("Delete", "step-5"),
    ] {
        h.begin_named(&p, label);
        p.name = name.into();
        assert!(h.commit(&p));
    }
    assert_eq!(
        h.undo_labels().collect::<Vec<_>>(),
        vec!["Oval", "Move", "Fill colour", "Transform", "Delete"]
    );
    assert!(h.step_to(&mut p, 3));
    assert_eq!(p.name, "step-3");
    assert_eq!(h.undo_len(), 3);
    assert_eq!(h.redo_len(), 2);
    assert_eq!(h.undo_label(), Some("Fill colour"));
    assert_eq!(h.redo_label(), Some("Transform"));
    assert_eq!(
        h.undo_labels().collect::<Vec<_>>(),
        vec!["Oval", "Move", "Fill colour"]
    );
    assert_eq!(
        h.redo_labels().collect::<Vec<_>>(),
        vec!["Transform", "Delete"]
    );
    assert!(!h.step_to(&mut p, 3));
    assert!(h.step_to(&mut p, 0));
    assert_eq!(p.name, original.name);
    assert_eq!(h.undo_len(), 0);
    assert_eq!(h.redo_len(), 5);
    assert!(h.step_to(&mut p, 5));
    assert_eq!(p.name, "step-5");
    assert_eq!(h.undo_len(), 5);
    assert_eq!(h.redo_len(), 0);
}

#[test]
fn nested_instance_expansion_is_rejected_before_render_truncation() {
    let mut symbols = vec![];
    for i in 0..18 {
        let elements = if i == 17 {
            vec![shape("leaf", 0.)]
        } else {
            vec![
                instance(&format!("left-{i}"), &format!("s-{}", i + 1), 0.),
                instance(&format!("right-{i}"), &format!("s-{}", i + 1), 20.),
            ]
        };
        symbols.push(symbol(&format!("s-{i}"), 1, vec![key(0, elements, None)]));
    }
    let p = project(symbols);
    assert!(
        validate(&p)
            .iter()
            .any(|i| i.message.contains("expanded scene"))
    );
}

#[test]
fn drained_scripts_become_stale_when_a_prior_handler_navigates() {
    let mut leaf = shape("leaf", 0.);
    leaf.scripts.push(ObjectScript {
        event: ObjectEvent::Load,
        script: js("old handler"),
    });
    let mut p = project(vec![symbol(
        "root",
        2,
        vec![key(0, vec![leaf], None), key(1, vec![], None)],
    )]);
    p.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: js("old frame"),
    });
    let mut player = Player::new(&p);
    let drained = player.drain_scripts();
    assert_eq!(drained.len(), 2);
    assert!(
        drained
            .iter()
            .all(|event| player.is_dispatch_current(&p, event))
    );
    player.control(&p, &[], PlaybackCommand::GotoAndStop(1));
    assert!(
        drained
            .iter()
            .all(|event| !player.is_dispatch_current(&p, event))
    );
}

#[test]
fn recreated_instances_have_fresh_script_lifetimes() {
    let p = project(vec![symbol(
        "root",
        4,
        vec![
            key(0, vec![shape("leaf", 0.)], None),
            key(2, vec![], None),
            key(3, vec![shape("leaf", 0.)], None),
        ],
    )]);
    let path = vec!["leaf".into()];
    let mut player = Player::new(&p);
    let first = player.lifetime_id(&path).unwrap();
    let root = player.lifetime_id(&[]).unwrap();
    player.tick(&p);
    assert_eq!(player.lifetime_id(&path), Some(first));
    player.tick(&p);
    assert_eq!(player.lifetime_id(&path), None);
    player.tick(&p);
    assert_ne!(player.lifetime_id(&path), Some(first));
    player.reset(&p);
    assert_ne!(player.lifetime_id(&[]), Some(root));
}

#[test]
fn ruler_guides_round_trip_and_migrate_older_documents() {
    // Version 5 documents have no `guides` key; they open with an empty guide list and are not
    // rewritten until a guide is added.
    let old = Project::blank().to_json().unwrap();
    assert!(!old.contains("\"guides\""));
    let opened = Project::from_json(&old).unwrap();
    assert!(opened.symbols.iter().all(|s| s.timeline.guides.is_empty()));

    let mut project = Project::blank();
    project.format_version = Guide::FORMAT_VERSION;
    project.symbols[0].timeline.guides = vec![
        Guide {
            axis: GuideAxis::Vertical,
            position: 120.,
        },
        Guide {
            axis: GuideAxis::Horizontal,
            position: -40.5,
        },
    ];
    let json = project.to_json().unwrap();
    assert!(json.contains("\"axis\": \"vertical\"") || json.contains("\"axis\":\"vertical\""));
    assert_eq!(Project::from_json(&json).unwrap(), project);
}

#[test]
fn ruler_guides_are_validated() {
    let mut project = Project::blank();
    project.symbols[0].timeline.guides.push(Guide {
        axis: GuideAxis::Horizontal,
        position: 10.,
    });
    let issues = validate(&project);
    assert!(
        issues.iter().any(|i| i.message.contains("version 6")),
        "{issues:?}"
    );
    project.format_version = Guide::FORMAT_VERSION;
    assert!(validate(&project).is_empty());
    project.symbols[0].timeline.guides[0].position = f64::NAN;
    assert!(
        validate(&project)
            .iter()
            .any(|i| i.path.ends_with("position"))
    );
    project.symbols[0].timeline.guides[0].position = 2.0e6;
    assert!(
        validate(&project)
            .iter()
            .any(|i| i.path.ends_with("position"))
    );
    project.symbols[0].timeline.guides = vec![
        Guide {
            axis: GuideAxis::Vertical,
            position: 0.,
        };
        Guide::LIMIT + 1
    ];
    assert!(
        validate(&project)
            .iter()
            .any(|i| i.message.contains("guide count"))
    );
    project.format_version = 17;
    assert!(
        validate(&project)
            .iter()
            .any(|i| i.message.contains("unsupported version 17"))
    );
}

#[test]
fn format_14_instance_behavior_and_label_types_roundtrip_evaluate_and_validate() {
    let mut mc_sym = symbol(
        "anim-mc",
        3,
        vec![
            key(0, vec![shape("r0", 0.0)], None),
            key(1, vec![shape("r1", 0.0)], None),
            key(2, vec![shape("r2", 0.0)], None),
        ],
    );
    mc_sym.timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: js("stop();"),
    });
    let mut gfx_sym = symbol(
        "anim-gfx",
        3,
        vec![
            key(0, vec![shape("g0", 0.0)], None),
            key(1, vec![shape("g1", 0.0)], None),
            key(2, vec![shape("g2", 0.0)], None),
        ],
    );
    gfx_sym.kind = SymbolKind::Graphic;
    gfx_sym.timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: js("ran_gfx_as_mc();"),
    });

    let mut inst_as_gfx = instance("as-gfx", "anim-mc", 0.0);
    inst_as_gfx.instance_behavior = Some(SymbolKind::Graphic);
    inst_as_gfx.kind = ElementKind::Instance {
        symbol_id: "anim-mc".into(),
        playback: InstancePlayback::Loop,
    };

    let mut inst_as_btn = instance("as-btn", "anim-mc", 20.0);
    inst_as_btn.instance_behavior = Some(SymbolKind::Button);
    inst_as_btn.kind = ElementKind::Instance {
        symbol_id: "anim-mc".into(),
        playback: InstancePlayback::SingleFrame(0),
    };

    let mut inst_as_mc = instance("as-mc", "anim-gfx", 40.0);
    inst_as_mc.instance_behavior = Some(SymbolKind::MovieClip);
    inst_as_mc.kind = ElementKind::Instance {
        symbol_id: "anim-gfx".into(),
        playback: InstancePlayback::Independent,
    };

    let mut comment_kf = key(
        0,
        vec![inst_as_gfx.clone(), inst_as_btn.clone(), inst_as_mc.clone()],
        None,
    );
    comment_kf.label = "todo_note".into();
    comment_kf.label_type = FrameLabelType::Comment;

    let mut anchor_kf = key(
        1,
        vec![inst_as_gfx.clone(), inst_as_btn.clone(), inst_as_mc.clone()],
        None,
    );
    anchor_kf.label = "section_two".into();
    anchor_kf.label_type = FrameLabelType::Anchor;

    let mut p = project(vec![
        symbol("root", 3, vec![comment_kf, anchor_kf]),
        mc_sym,
        gfx_sym,
    ]);
    assert!(p.uses_format_14());
    assert!(p.validate().unwrap_err().to_string().contains("version 14"));
    p.upgrade_format_for_features();
    assert_eq!(p.format_version, 14);
    p.validate().unwrap();

    // Reject instance_behavior on non-Instance elements.
    let mut bad = p.clone();
    let mut bad_rect = shape("bad", 0.0);
    bad_rect.instance_behavior = Some(SymbolKind::Button);
    bad.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(bad_rect);
    assert!(
        bad.validate()
            .unwrap_err()
            .to_string()
            .contains("instance_behavior")
    );

    // JSON roundtrip preserves instance_behavior and label_type.
    let reopened = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(reopened, p);

    // Comment labels are ignored by goto resolution; Anchor labels resolve normally.
    let mut player = Player::new(&p);
    assert_eq!(player.resolve_frame(&p, &[], "todo_note"), None);
    assert_eq!(player.resolve_frame_as2(&p, &[], "todo_note"), None);
    assert_eq!(player.resolve_frame(&p, &[], "section_two"), Some(1));
    assert_eq!(player.resolve_frame_as2(&p, &[], "SECTION_TWO"), Some(1));

    // MovieClip overridden to Graphic does NOT run its `stop();` script, while Graphic overridden to MovieClip DOES!
    let initial_scripts: Vec<_> = player
        .drain_scripts()
        .into_iter()
        .map(|d| (d.target_path, d.script.source))
        .collect();
    assert_eq!(
        initial_scripts,
        vec![(vec!["as-mc".into()], "ran_gfx_as_mc();".into())]
    );

    // DisplayNode owners reflect effective per-instance behavior without mutating Library Symbol.kind.
    assert_eq!(p.symbol("anim-mc").unwrap().kind, SymbolKind::MovieClip);
    assert_eq!(p.symbol("anim-gfx").unwrap().kind, SymbolKind::Graphic);
    let nodes = player.display_nodes(&p);
    let find_node = |id: &str| nodes.iter().find(|n| n.path == [id.to_owned()]).unwrap();
    assert_eq!(find_node("as-gfx").owner, "graphic");
    assert_eq!(find_node("as-btn").owner, "button");
    assert_eq!(find_node("as-mc").owner, "movie_clip");

    // Advancing parent frame 0 -> 1 advances both the parent-synced Graphic override and the independent MovieClip override,
    // while Button override stays on Up (0) until hovered/pressed.
    player.tick(&p);
    let nodes = player.display_nodes(&p);
    let find_node = |id: &str| nodes.iter().find(|n| n.path == [id.to_owned()]).unwrap();
    assert_eq!(find_node("as-gfx").frame, 1);
    assert_eq!(find_node("as-btn").frame, 0);
    assert_eq!(find_node("as-mc").frame, 1);
}

#[test]
fn format_13_frame_script_layer_ownership_roundtrips_and_validates_per_layer() {
    let mut p = project(vec![symbol("root", 1, vec![key(0, vec![], None)])]);
    p.symbols[0].timeline.layers.push(Layer {
        id: "root-layer-2".into(),
        name: "Layer 2".into(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        keyframes: vec![key(0, vec![], None)],
    });
    p.symbols[0].timeline.scripts = vec![
        FrameScript {
            layer: Some("root-layer".into()),
            frame: 0,
            script: js("first();"),
        },
        FrameScript {
            layer: Some("root-layer-2".into()),
            frame: 0,
            script: js("second();"),
        },
    ];

    assert!(p.uses_format_13());
    assert!(p.validate().unwrap_err().to_string().contains("version 13"));
    p.upgrade_format_for_features();
    assert_eq!(p.format_version, 13);
    p.validate().unwrap();
    let executed: Vec<_> = Player::new(&p)
        .drain_scripts()
        .into_iter()
        .map(|dispatch| dispatch.script.source)
        .collect();
    assert_eq!(executed, vec!["first();", "second();"]);
    let reopened = Project::from_json(&p.to_json().unwrap()).unwrap();
    assert_eq!(reopened, p);

    let mut duplicate = p.clone();
    duplicate.symbols[0].timeline.scripts[1].layer = Some("root-layer".into());
    assert!(
        duplicate
            .validate()
            .unwrap_err()
            .to_string()
            .contains("only one frame script")
    );

    let mut legacy = project(vec![symbol("legacy", 1, vec![key(0, vec![], None)])]);
    legacy.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: js("legacy();"),
    });
    let legacy_json = legacy.to_json().unwrap();
    let legacy_script = &serde_json::from_str::<serde_json::Value>(&legacy_json).unwrap()["symbols"]
        [0]["timeline"]["scripts"][0];
    assert!(legacy_script.get("layer").is_none());
    assert_eq!(Project::from_json(&legacy_json).unwrap(), legacy);
}

#[test]
fn format_7_pivot_and_primitive_options_roundtrip_validate_and_evaluate() {
    let old = Project::blank();
    assert!(!old.uses_format_7());
    let old_json = old.to_json().unwrap();
    assert!(!old_json.contains("\"pivot\""));
    assert!(!old_json.contains("\"primitive\""));

    let mut project = Project::blank();
    project.format_version = 7;
    project.symbols[0].timeline.frame_count = 5;
    let rect_el = Element {
        accessibility: None,
        id: new_id("rect"),
        name: "RoundedCard".into(),
        transform: Transform {
            x: 40.,
            y: 50.,
            pivot: Some([10., 20.]),
            primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                corner_radii: [16., 8., 16., 8.],
                corners_locked: false,
            })),
            ..Default::default()
        },
        kind: ElementKind::Rectangle {
            width: 120.,
            height: 80.,
            fill: [40, 120, 220, 255],
            stroke: [10, 20, 30, 255],
            stroke_width: 2.,
        },
        scripts: vec![],
        ..Element::default()
    };
    let oval_el = Element {
        accessibility: None,
        id: new_id("oval"),
        name: "DonutArc".into(),
        transform: Transform {
            x: 200.,
            y: 60.,
            primitive: Some(PrimitiveOptions::Oval(OvalOptions {
                start_angle: 30.,
                end_angle: 300.,
                inner_radius: 45.,
                close_path: false,
            })),
            ..Default::default()
        },
        kind: ElementKind::Ellipse {
            width: 100.,
            height: 100.,
            fill: [240, 140, 40, 255],
            stroke: [20, 20, 20, 255],
            stroke_width: 3.,
        },
        scripts: vec![],
        ..Element::default()
    };
    project.symbols[0].timeline.layers[0].keyframes[0].elements =
        vec![rect_el.clone(), oval_el.clone()];
    assert!(project.uses_format_7());
    project.validate().unwrap();

    let encoded = project.to_json().unwrap();
    let decoded = Project::from_json(&encoded).unwrap();
    assert_eq!(decoded, project);

    let evaluated = evaluate(&project, &project.root, 0);
    assert_eq!(evaluated.len(), 2);
    assert!(matches!(evaluated[0].kind, ElementKind::Path { .. }));
    let ElementKind::Path {
        ref commands,
        fill,
        fill_rule,
        ..
    } = evaluated[1].kind
    else {
        panic!("expected parametric oval to evaluate to Path");
    };
    assert_eq!(fill, [0, 0, 0, 0], "open oval suppresses fill");
    assert_eq!(fill_rule, FillRule::NonZero);
    assert!(
        !commands.iter().any(|c| matches!(c, PathCommand::Close)),
        "open oval must not emit Close"
    );

    // Closed full-circle donut carves its inner hole under NonZero winding.
    let donut = oval_path_commands(
        100.,
        100.,
        OvalOptions {
            start_angle: 0.,
            end_angle: 0.,
            inner_radius: 50.,
            close_path: true,
        },
    );
    assert!(
        !hit_test_path(
            &donut,
            FillRule::NonZero,
            [50., 50.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(
        hit_test_path(
            &donut,
            FillRule::NonZero,
            [85., 50.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );

    // Format 6 must reject format-7 features.
    let mut wrong_version = project.clone();
    wrong_version.format_version = 6;
    assert!(
        validate(&wrong_version)
            .iter()
            .any(|i| i.path == "format_version" && i.message.contains("version 7"))
    );

    // Out-of-range pivot, radii, angles, inner_radius, and mismatched primitive kind are rejected.
    let mut bad_pivot = project.clone();
    bad_pivot.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .pivot = Some([f64::NAN, 0.]);
    assert!(
        validate(&bad_pivot)
            .iter()
            .any(|i| i.path.ends_with("transform.pivot"))
    );

    let mut bad_radius = project.clone();
    bad_radius.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .primitive = Some(PrimitiveOptions::Rectangle(RectangleOptions {
        corner_radii: [-5., 0., 0., 0.],
        corners_locked: true,
    }));
    assert!(
        validate(&bad_radius)
            .iter()
            .any(|i| i.path.ends_with("corner_radii"))
    );

    let mut bad_oval = project.clone();
    bad_oval.symbols[0].timeline.layers[0].keyframes[0].elements[1]
        .transform
        .primitive = Some(PrimitiveOptions::Oval(OvalOptions {
        start_angle: 0.,
        end_angle: 420.,
        inner_radius: 99.5,
        close_path: true,
    }));
    let issues = validate(&bad_oval);
    assert!(
        issues
            .iter()
            .any(|i| i.path.ends_with("transform.primitive"))
    );
    assert!(issues.iter().any(|i| i.path.ends_with("inner_radius")));

    let mut mismatched = project.clone();
    mismatched.symbols[0].timeline.layers[0].keyframes[0].elements[0]
        .transform
        .primitive = Some(PrimitiveOptions::Oval(OvalOptions {
        start_angle: 0.,
        end_angle: 180.,
        inner_radius: 0.,
        close_path: true,
    }));
    assert!(
        validate(&mismatched)
            .iter()
            .any(|i| i.message.contains("apply only to ellipse elements"))
    );
}

#[test]
fn t315_migration_rule_and_foreign_script_count_messages() {
    // 1. Canonical R-102 dropdown order, labels (no "· strict" suffix), and compatibility pairs.
    assert_eq!(
        ScriptLanguage::PROJECT_ORDER.map(ScriptLanguage::display_label),
        [
            "ActionScript 3.0",
            "ActionScript 2.0",
            "ActionScript 1.0",
            "TypeScript",
            "JavaScript",
        ]
    );
    assert_eq!(ScriptLanguage::ActionScript3.compatible_language(), None);
    assert_eq!(
        ScriptLanguage::ActionScript2.compatible_language(),
        Some(ScriptLanguage::ActionScript1)
    );
    assert_eq!(
        ScriptLanguage::ActionScript1.compatible_language(),
        Some(ScriptLanguage::ActionScript2)
    );
    assert_eq!(
        ScriptLanguage::TypeScript.compatible_language(),
        Some(ScriptLanguage::JavaScript)
    );
    assert_eq!(
        ScriptLanguage::JavaScript.compatible_language(),
        Some(ScriptLanguage::TypeScript)
    );

    // 2. Deterministic migration rule for pre-R-102 documents (`script_language: None`).
    let blank = Project::blank();
    assert_eq!(blank.script_language, None);
    assert!(blank.also_compile_compatible_scripts);
    assert_eq!(blank.infer_script_language(), ScriptLanguage::ActionScript3);
    assert_eq!(
        blank.effective_script_language(),
        ScriptLanguage::ActionScript3
    );

    // Majority script language wins across sources, frame scripts, and object scripts (ignoring whitespace-only).
    let mut majority = Project::blank();
    majority.sources.push(SourceUnit {
        id: "s1".into(),
        path: "app.js".into(),
        language: ScriptLanguage::JavaScript,
        source: "console.log(1);".into(),
    });
    majority.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "this.stop();".into(),
        },
    });
    majority.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 1,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: "   \n  ".into(),
        },
    });
    let mut el = shape("btn", 0.);
    el.scripts.push(ObjectScript {
        event: ObjectEvent::Click,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();".into(),
        },
    });
    majority.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(el);
    assert_eq!(majority.infer_script_language(), ScriptLanguage::JavaScript);

    // Tie-breaking follows canonical order: AS3 > AS2 > AS1 > TS > JS.
    let mut tie_ts_js = Project::blank();
    tie_ts_js.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::JavaScript,
            source: "let a = 1;".into(),
        },
    });
    tie_ts_js.symbols[0].timeline.scripts.push(FrameScript {
        layer: None,
        frame: 1,
        script: Script {
            language: ScriptLanguage::TypeScript,
            source: "const b: number = 2;".into(),
        },
    });
    assert_eq!(
        tie_ts_js.infer_script_language(),
        ScriptLanguage::TypeScript
    );

    // Scriptless fallbacks: document_class / class_name -> AS3, linkage_id -> AS2, resource ext -> TS/JS/AS2.
    let mut doc_cls = Project::blank();
    doc_cls.document_class = Some("app.Main".into());
    assert_eq!(
        doc_cls.infer_script_language(),
        ScriptLanguage::ActionScript3
    );

    let mut linkage_only = Project::blank();
    linkage_only.symbols[0].linkage_id = Some("hero_mc".into());
    assert_eq!(
        linkage_only.infer_script_language(),
        ScriptLanguage::ActionScript2
    );

    let mut ts_res = Project::blank();
    ts_res.resources.push(
        ResourceUnit::from_bytes("r1", "scripts/helper.ts", "text/plain", b"export {};").unwrap(),
    );
    assert_eq!(ts_res.infer_script_language(), ScriptLanguage::TypeScript);

    let mut js_res = Project::blank();
    js_res
        .resources
        .push(ResourceUnit::from_bytes("r1", "scripts/helper.js", "text/plain", b"1;").unwrap());
    assert_eq!(js_res.infer_script_language(), ScriptLanguage::JavaScript);

    let mut as_res = Project::blank();
    as_res.resources.push(
        ResourceUnit::from_bytes("r1", "includes/init.as", "text/plain", b"stop();").unwrap(),
    );
    assert_eq!(
        as_res.infer_script_language(),
        ScriptLanguage::ActionScript2
    );

    // 3. Exact R-102 item 3 count messages (both > 0, only skipped, only compiled, singular & plural).
    let mut mixed = Project::blank();
    // 2 JS scripts (compatible with TS) + 3 AS2/AS3 scripts (skipped in TS).
    for (idx, lang) in [
        ScriptLanguage::JavaScript,
        ScriptLanguage::JavaScript,
        ScriptLanguage::ActionScript3,
        ScriptLanguage::ActionScript2,
        ScriptLanguage::ActionScript1,
    ]
    .into_iter()
    .enumerate()
    {
        mixed.symbols[0].timeline.scripts.push(FrameScript {
            layer: None,
            frame: idx as u32,
            script: Script {
                language: lang,
                source: "trace(1);".into(),
            },
        });
    }
    let both = mixed.foreign_script_counts_for(ScriptLanguage::TypeScript, true);
    assert_eq!(both.compiled_with_warning, 2);
    assert_eq!(both.skipped, 3);
    assert_eq!(
        both.count_message(),
        Some((
            true,
            "5 scripts are in other languages and stay in the document: 2 compile with a warning, 3 are skipped when publishing.".into()
        ))
    );

    let skipped_all = mixed.foreign_script_counts_for(ScriptLanguage::TypeScript, false);
    assert_eq!(skipped_all.compiled_with_warning, 0);
    assert_eq!(skipped_all.skipped, 5);
    assert_eq!(
        skipped_all.count_message(),
        Some((
            true,
            "5 scripts are in other languages and stay in the document. They are skipped when publishing.".into()
        ))
    );

    // AS3 ignores `also_compile_compatible_scripts == true` because AS3 has no compatible language.
    let as3_counts = mixed.foreign_script_counts_for(ScriptLanguage::ActionScript3, true);
    assert_eq!(as3_counts.compiled_with_warning, 0);
    assert_eq!(as3_counts.skipped, 4);

    // Singular messages + Info severity when only compatible scripts exist.
    assert_eq!(
        ForeignScriptCounts {
            compiled_with_warning: 0,
            skipped: 0,
            compatible_language: Some(ScriptLanguage::JavaScript),
        }
        .count_message(),
        None
    );
    assert_eq!(
        ForeignScriptCounts {
            compiled_with_warning: 0,
            skipped: 1,
            compatible_language: None,
        }
        .count_message(),
        Some((
            true,
            "1 script is in another language and stays in the document. It is skipped when publishing.".into()
        ))
    );
    assert_eq!(
        ForeignScriptCounts {
            compiled_with_warning: 1,
            skipped: 1,
            compatible_language: Some(ScriptLanguage::JavaScript),
        }
        .count_message(),
        Some((
            true,
            "2 scripts are in other languages and stay in the document: 1 compiles with a warning, 1 is skipped when publishing.".into()
        ))
    );
    assert_eq!(
        ForeignScriptCounts {
            compiled_with_warning: 2,
            skipped: 0,
            compatible_language: Some(ScriptLanguage::JavaScript),
        }
        .count_message(),
        Some((
            false,
            "2 scripts are in JavaScript and compile with a warning.".into()
        ))
    );
    assert_eq!(
        ForeignScriptCounts {
            compiled_with_warning: 1,
            skipped: 0,
            compatible_language: Some(ScriptLanguage::ActionScript1),
        }
        .count_message(),
        Some((
            false,
            "1 script is in ActionScript 1.0 and compiles with a warning.".into()
        ))
    );

    // 4. Serde skip_serializing_if defaults & format-6 validation rule.
    let blank_json = blank.to_json().unwrap();
    assert!(!blank_json.contains("\"script_language\""));
    assert!(!blank_json.contains("\"also_compile_compatible_scripts\""));

    let mut locked = Project::blank();
    locked.script_language = Some(ScriptLanguage::TypeScript);
    assert!(
        validate(&locked)
            .iter()
            .any(|i| i.path == "format_version" && i.message.contains("version 6"))
    );
    locked.format_version = 6;
    locked.also_compile_compatible_scripts = false;
    locked.validate().unwrap();
    let encoded = locked.to_json().unwrap();
    assert!(encoded.contains("\"script_language\""));
    assert!(encoded.contains("\"also_compile_compatible_scripts\""));
    assert_eq!(Project::from_json(&encoded).unwrap(), locked);
}

#[test]
fn as2_legacy_event_wrapper_scripts_enqueue_once_at_load_in_player_and_authored_playback() {
    let mut btn_inst = instance("btn", "clip", 20.0);
    btn_inst.scripts.push(ObjectScript {
        event: ObjectEvent::Click,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "on(release) {\n  trace(\"BTN_RELEASE\");\n}".into(),
        },
    });
    let mut mover_inst = instance("mover", "clip", 60.0);
    mover_inst.scripts.push(ObjectScript {
        event: ObjectEvent::EnterFrame,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "/* legacy clip wrapper */\nonClipEvent(enterFrame) {\n  _x += 5;\n}".into(),
        },
    });
    let mut bare_inst = instance("bare", "clip", 100.0);
    bare_inst.scripts.push(ObjectScript {
        event: ObjectEvent::Click,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "// onClipEvent(press)\ntrace(\"BARE_CLICK\");".into(),
        },
    });

    let mut btn_as1 = instance("btn_as1", "clip", 140.0);
    btn_as1.scripts.push(ObjectScript {
        event: ObjectEvent::Click,
        script: Script {
            language: ScriptLanguage::ActionScript1,
            source: "on(release) {\n  trace(\"AS1_RELEASE\");\n}".into(),
        },
    });

    let proj = project(vec![
        symbol(
            "root",
            2,
            vec![key(0, vec![btn_inst, mover_inst, bare_inst, btn_as1], None)],
        ),
        symbol("clip", 1, vec![key(0, vec![shape("box", 0.0)], None)]),
    ]);

    let mut player = Player::new(&proj);
    let startup = player.drain_scripts();
    assert_eq!(
        startup.len(),
        3,
        "pure AS1/AS2 on(...)/onClipEvent(...) wrappers must enqueue at initial Load while commented-out bare Click stays deferred"
    );
    assert!(startup.iter().all(|d| d.event == ScriptEvent::Load));
    assert!(
        startup.iter().all(|d| player.is_dispatch_current(&proj, d)),
        "Player::is_dispatch_current must accept Load dispatches for AS1/AS2 wrappers stored under Click/EnterFrame"
    );
    let targets: Vec<Vec<String>> = startup.iter().map(|d| d.target_path.clone()).collect();
    assert_eq!(
        targets,
        vec![
            vec!["btn".to_owned()],
            vec!["mover".to_owned()],
            vec!["btn_as1".to_owned()],
        ]
    );

    player.dispatch_click(&proj, &["btn".to_owned()]);
    assert!(
        player.drain_scripts().is_empty(),
        "clicking must not re-enqueue the AS2 on(release) registration wrapper"
    );

    player.dispatch_click(&proj, &["bare".to_owned()]);
    let bare_dispatches = player.drain_scripts();
    assert_eq!(bare_dispatches.len(), 1);
    assert_eq!(bare_dispatches[0].target_path, vec!["bare".to_owned()]);
    assert_eq!(bare_dispatches[0].event, ScriptEvent::Click);

    player.tick(&proj);
    assert!(
        player.drain_scripts().is_empty(),
        "frame advance must not re-enqueue the AS2 onClipEvent(enterFrame) registration wrapper"
    );

    let mut authored = AuthoredPlayback::with_live_scripts(&proj, "root", 0, true);
    assert!(
        authored.drain_traces().is_empty(),
        "AuthoredPlayback startup must register wrappers without firing click handlers"
    );
    authored.click(&proj, &["btn".to_owned()]);
    assert_eq!(
        authored.drain_traces(),
        vec!["BTN_RELEASE".to_owned()],
        "AuthoredPlayback must fire on(release) exactly once per click without double-enqueue"
    );
    authored.click(&proj, &["btn".to_owned()]);
    assert_eq!(
        authored.drain_traces(),
        vec!["BTN_RELEASE".to_owned()],
        "second click in AuthoredPlayback must fire once"
    );
}
