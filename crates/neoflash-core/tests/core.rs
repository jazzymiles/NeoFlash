use neoflash_core::*;

fn shape(id: &str, x: f64) -> Element {
    Element {
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
    }
}
fn symbol(id: &str, frames: u32, keys: Vec<Keyframe>) -> Symbol {
    Symbol {
        id: id.into(),
        name: id.into(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: frames,
            layers: vec![Layer {
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
            5,
            vec![
                key(0, vec![instance("one", "clip", 0.)], None),
                key(2, vec![], None),
                key(3, vec![instance("one", "clip", 0.)], None),
            ],
        ),
        symbol("clip", 10, vec![key(0, vec![shape("leaf", 0.)], None)]),
    ]);
    let mut player = Player::new(&p);
    player.tick(&p);
    assert_eq!(player.clip_frame(&["one".into()]), Some(1));
    player.tick(&p);
    assert_eq!(player.clip_frame(&["one".into()]), None);
    player.tick(&p);
    assert_eq!(player.clip_frame(&["one".into()]), Some(0));
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
            frame: 0,
            script: js("root-frame0"),
        },
        FrameScript {
            frame: 1,
            script: js("root-frame1"),
        },
    ];
    p.symbols[1].timeline.scripts = vec![
        FrameScript {
            frame: 0,
            script: js("clip-frame0"),
        },
        FrameScript {
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
    invalid.format_version = 2;
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
    assert!(h.undo(&mut p));
    assert_eq!(p, original);
    assert!(!h.can_undo());
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
