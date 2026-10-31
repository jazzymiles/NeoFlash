use neoflash_core::*;

fn element(id: &str, kind: ElementKind) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        kind,
        transform: Transform::default(),
        scripts: vec![],
        accessibility: None,
    }
}
fn rectangle() -> ElementKind {
    ElementKind::Rectangle {
        width: 20.,
        height: 30.,
        fill: [100, 150, 200, 255],
        stroke: [0; 4],
        stroke_width: 0.,
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
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}
fn gradient(kind: GradientKind) -> GradientPaint {
    GradientPaint {
        kind,
        matrix: [1., 0., 0., 1., 0., 0.],
        focal_point_ratio: 0.,
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [255, 0, 0, 255],
            },
            GradientStop {
                ratio: 255,
                color: [0, 0, 255, 255],
            },
        ],
    }
}

#[test]
fn imported_affines_colors_and_classic_tweens_stay_editable_and_roundtrip() {
    let candidates = vec!["hourhand".into(), "Ärger".into()];
    assert_eq!(
        neoflash_core::as2_strings::legacy_case_name("HourHand", &candidates).unwrap(),
        "hourhand"
    );
    assert_eq!(
        neoflash_core::as2_strings::legacy_case_name("ärger", &candidates).unwrap(),
        "Ärger"
    );
    for matrix in [
        [0.7, 0.2, 0.3, -1.2, 33., -45.],
        [0., 0., 0.7, 0.5, 0., 0.],
        [-1., 0., 0., 1., 0., 0.],
    ] {
        let transform = Transform::from_matrix(matrix).unwrap();
        for (actual, expected) in transform_matrix(transform).into_iter().zip(matrix) {
            near(actual, expected);
        }
    }
    let mut project = Project::blank();
    project.format_version = 5;
    let mut shape = element("shape", rectangle());
    shape.transform = Transform::from_matrix([1., 0.2, 0.4, 1., 0., 0.]).unwrap();
    shape.transform.color_transform = ColorTransform {
        multipliers: [0.5, 1., 0., 0.5],
        offsets: [10., 0., 50., 0.],
    };
    let mut end = shape.clone();
    end.transform.x = 100.;
    end.transform.skew += 40.;
    end.transform.color_transform.offsets[0] = 30.;
    let timeline = &mut project.symbols[0].timeline;
    timeline.frame_count = 11;
    timeline.layers[0].keyframes = vec![
        key(0, vec![shape], Some(Easing::Flash { acceleration: 50. })),
        key(10, vec![end], None),
    ];
    project.validate().unwrap();
    let json = serde_json::to_string(&project).unwrap();
    let restored: Project = serde_json::from_str(&json).unwrap();
    let evaluated = evaluate(&restored, &restored.root, 5);
    near(evaluated[0].transform[4], 62.5);
    near(evaluated[0].color_transform.offsets[0], 22.5);
    let player = Player::new(&restored);
    let snapshot = player.runtime_snapshot(&restored);
    assert!(matches!(
        snapshot.draws[0].item.kind,
        RuntimeDrawKind::Element(ElementKind::Rectangle {
            fill: [60, 150, 50, 128],
            ..
        })
    ));
    let old: Transform =
        serde_json::from_str(r#"{"x":0,"y":0,"scale_x":1,"scale_y":1,"rotation":0,"alpha":1}"#)
            .unwrap();
    assert_eq!(old, Transform::default());
    near(
        easing(
            0.5,
            Easing::Flash {
                acceleration: -100.,
            },
        ),
        0.25,
    );
    near(easing(0.5, Easing::Flash { acceleration: 100. }), 0.75);
    let custom = Easing::CubicBezier {
        x1: 0.3621,
        y1: 0.9,
        x2: 0.1171,
        y2: 1.,
    };
    assert!(easing(0.5, custom) > 0.9);
    project.format_version = 4;
    assert!(project.validate().is_err());
}

#[test]
fn authored_gradient_stops_survive_inherited_color_transforms() {
    let paint = gradient(GradientKind::Linear);
    paint.validate().unwrap();
    assert_eq!(paint.sample(-819.2, 0.), [255, 0, 0, 255]);
    assert_eq!(paint.sample(819.2, 0.), [0, 0, 255, 255]);
    let radial = gradient(GradientKind::Radial);
    assert_eq!(radial.sample(0., 0.), [255, 0, 0, 255]);
    assert_eq!(radial.sample(0., 819.2), [0, 0, 255, 255]);
    let mut project = Project::blank();
    project.format_version = 5;
    let mut child = project.symbols[0].clone();
    child.id = "gradient-symbol".into();
    child.timeline.layers[0].id = "gradient-layer".into();
    let mut path = element(
        "gradient-shape",
        ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 20., y: 0. },
                PathCommand::LineTo { x: 20., y: 20. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [0; 4],
            fill_gradient: Some(paint),
            stroke_gradient: None,
            stroke_width: 0.,
        },
    );
    path.transform.color_transform = ColorTransform {
        multipliers: [0.5, 1., 1., 1.],
        offsets: [10., 0., 0., 0.],
    };
    child.timeline.layers[0].keyframes[0].elements = vec![path];
    project.symbols.push(child);
    let mut placement = element(
        "holder",
        ElementKind::Instance {
            symbol_id: "gradient-symbol".into(),
            playback: InstancePlayback::Independent,
        },
    );
    placement.transform.color_transform = ColorTransform {
        multipliers: [1., 1., 1., 0.5],
        offsets: [5., 0., 0., 0.],
    };
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![placement];
    project.validate().unwrap();
    let authored = evaluate(&project, &project.root, 0);
    assert_eq!(
        authored[0].color_transform.apply([255, 0, 0, 255]),
        [143, 0, 0, 128]
    );
    let player = Player::new(&project);
    let snapshot = player.runtime_snapshot(&project);
    let RuntimeDrawKind::Element(ElementKind::PaintedPath {
        fill_gradient: Some(paint),
        ..
    }) = &snapshot.draws[0].item.kind
    else {
        panic!("lost authored gradient")
    };
    assert_eq!(paint.stops[0].color, [143, 0, 0, 128]);
    assert_eq!(
        serde_json::to_value(&snapshot.draws[0]).unwrap()["kind"]["type"],
        "gradient_path"
    );
}

#[test]
fn button_hit_frame_is_input_only_and_visible_states_keep_placement_identity() {
    let mut project = Project::blank();
    project.format_version = 5;
    let mut button = project.symbols[0].clone();
    button.id = "button".into();
    button.kind = SymbolKind::Button;
    button.linkage_id = Some("StatesButton".into());
    button.timeline.frame_count = 4;
    button.timeline.layers[0].id = "button-layer".into();
    button.timeline.layers[0].keyframes = vec![
        key(0, vec![], None),
        key(1, vec![element("over", rectangle())], None),
        key(2, vec![element("down", rectangle())], None),
        key(3, vec![element("hit", rectangle())], None),
    ];
    project.symbols.push(button);
    project.symbols[0].timeline.layers[0].keyframes[0].elements = vec![element(
        "placement",
        ElementKind::Instance {
            symbol_id: "button".into(),
            playback: InstancePlayback::Independent,
        },
    )];
    project.validate().unwrap();
    let mut player = Player::new(&project);
    let reference = player.object_ref(&["placement".into()]).unwrap();
    let snapshot = player.runtime_snapshot(&project);
    assert!(snapshot.draws.is_empty());
    let object = snapshot
        .objects
        .iter()
        .find(|o| o.object_ref == reference)
        .unwrap();
    assert_eq!(object.button_hits.len(), 1);
    assert_eq!(object.button_hits[0].element_id, "hit");
    for _ in 0..10 {
        player.tick(&project);
    }
    assert!(player.runtime_snapshot(&project).draws.is_empty());
    assert!(player.object_button_state(&project, &reference, 1));
    assert_eq!(
        player.runtime_snapshot(&project).draws[0].item.element_id,
        "over"
    );
    assert!(player.object_button_state(&project, &reference, 2));
    assert_eq!(
        player.runtime_snapshot(&project).draws[0].item.element_id,
        "down"
    );
    assert!(!player.object_button_state(&project, &reference, 3));
    assert_eq!(
        player.object_ref(&["placement".into()]),
        Some(reference.clone())
    );
    assert!(player.object_button_state(&project, &reference, 0));
    assert!(player.runtime_snapshot(&project).draws.is_empty());

    // Exported Button symbols keep the same state machine when instantiated
    // through library linkage; Hit remains input-only and ticks do not animate it.
    let linked = player.create_linked(&project, "StatesButton").unwrap();
    let root = player.object_ref(&[]).unwrap();
    player
        .display_mutate(
            &project,
            DisplayMutation::Add {
                parent: root.clone(),
                child: linked.clone(),
                index: None,
            },
        )
        .unwrap();
    let snapshot = player.runtime_snapshot(&project);
    let linked_object = snapshot
        .objects
        .iter()
        .find(|object| object.object_ref == linked)
        .unwrap();
    assert_eq!(linked_object.display.owner, "button");
    assert_eq!(linked_object.parent_ref, Some(root));
    assert_eq!(linked_object.button_hits.len(), 1);
    assert_eq!(linked_object.button_hits[0].element_id, "hit");
    assert!(snapshot.draws.is_empty());
    assert!(player.object_button_state(&project, &linked, 1));
    for _ in 0..10 {
        player.tick(&project);
    }
    let snapshot = player.runtime_snapshot(&project);
    assert_eq!(snapshot.draws.len(), 1);
    assert_eq!(snapshot.draws[0].item.element_id, "over");
    assert!(player.object_button_state(&project, &linked, 2));
    assert_eq!(
        player.runtime_snapshot(&project).draws[0].item.element_id,
        "down"
    );
    assert!(!player.object_button_state(&project, &linked, 3));
    assert!(player.object_button_state(&project, &linked, 0));
    assert!(player.runtime_snapshot(&project).draws.is_empty());
}

#[test]
fn authored_playback_advances_independent_clips_without_scripts_or_editor_outlines() {
    fn symbol(id: &str, count: u32, keys: Vec<Keyframe>) -> Symbol {
        Symbol {
            id: id.into(),
            name: id.into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                guides: vec![],
                frame_count: count,
                layers: vec![Layer {
                    id: format!("{id}-layer"),
                    name: id.into(),
                    visible: true,
                    locked: false,
                    keyframes: keys,
                    properties: LayerProperties::default(),
                }],
                scripts: vec![],
            },
            class_name: None,
            linkage_id: None,
        }
    }
    fn instance(id: &str, symbol: &str, playback: InstancePlayback) -> Element {
        element(
            id,
            ElementKind::Instance {
                symbol_id: symbol.into(),
                playback,
            },
        )
    }
    fn draw<'a>(scene: &'a AuthoredScene, id: &str) -> &'a AuthoredDraw {
        scene
            .draws
            .iter()
            .find(|d| d.item.element_id == id)
            .unwrap()
    }

    let mut project = Project::blank();
    project.format_version = 5;
    let root_id = project.root.clone();
    let mut first = element(
        "animated-paint",
        ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 20., y: 0. },
                PathCommand::LineTo { x: 20., y: 20. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            fill_gradient: Some(gradient(GradientKind::Linear)),
            stroke_gradient: None,
            stroke_width: 0.,
        },
    );
    first.scripts.push(ObjectScript {
        event: ObjectEvent::EnterFrame,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "this._x = 10000; getURL('https://example.invalid/');".into(),
        },
    });
    let mut last = first.clone();
    last.transform.x = 380.;
    last.transform.color_transform = ColorTransform {
        multipliers: [0.2, 0.6, 1., 0.25],
        offsets: [80., 0., 0., 0.],
    };
    let mut movie = symbol(
        "long-movie",
        381,
        vec![
            key(0, vec![first], Some(Easing::Linear)),
            key(380, vec![last], None),
        ],
    );
    movie.timeline.layers[0].properties.outline = true;
    movie.timeline.layers[0].properties.outline_color = Some([255, 0, 255]);
    movie.timeline.scripts.push(FrameScript {
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript2,
            source: "stop();".into(),
        },
    });
    let mut graphic_first = element("graphic-paint", rectangle());
    graphic_first.transform.y = 40.;
    let mut graphic_last = graphic_first.clone();
    graphic_last.transform.x = 100.;
    let mut graphic = symbol(
        "graphic",
        11,
        vec![
            key(0, vec![graphic_first], Some(Easing::Linear)),
            key(10, vec![graphic_last], None),
        ],
    );
    graphic.kind = SymbolKind::Graphic;
    movie.timeline.layers.push(Layer {
        id: "movie-graphic-layer".into(),
        name: "Graphic follows movie".into(),
        visible: true,
        locked: false,
        keyframes: vec![key(
            0,
            vec![instance("movie-graphic", "graphic", InstancePlayback::Loop)],
            None,
        )],
        properties: LayerProperties::default(),
    });
    project.symbols.extend([movie, graphic]);
    let placement = instance("movie", "long-movie", InstancePlayback::Independent);
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 1;
    root.timeline.layers[0].keyframes = vec![key(0, vec![placement.clone()], None)];
    root.timeline.layers[0].properties.mask = Some("preview-mask".into());
    root.timeline.layers.push(Layer {
        id: "root-graphic-layer".into(),
        name: "Graphic follows one-frame root".into(),
        visible: true,
        locked: false,
        keyframes: vec![key(
            0,
            vec![instance("root-graphic", "graphic", InstancePlayback::Loop)],
            None,
        )],
        properties: LayerProperties::default(),
    });
    root.timeline.layers.push(Layer {
        id: "preview-mask".into(),
        name: "Mask".into(),
        visible: true,
        locked: false,
        keyframes: vec![key(
            0,
            vec![element(
                "mask-paint",
                ElementKind::Rectangle {
                    width: 400.,
                    height: 100.,
                    fill: [255; 4],
                    stroke: [0; 4],
                    stroke_width: 0.,
                },
            )],
            None,
        )],
        properties: LayerProperties {
            role: LayerRole::Mask,
            ..Default::default()
        },
    });
    project.validate().unwrap();
    let original = project.to_json().unwrap();
    let static_scene = evaluate_scene(&project, &root_id, 0, true, true);
    assert!(
        draw(&static_scene, "animated-paint")
            .outline_color
            .is_none()
    );
    let edited_movie = evaluate_scene(&project, "long-movie", 0, true, true);
    assert_eq!(
        draw(&edited_movie, "animated-paint").outline_color,
        Some([255, 0, 255, 255])
    );
    let mut outlined_root = project.clone();
    let root = outlined_root.symbol_mut(&root_id).unwrap();
    root.timeline.layers[0].properties.parent = Some("outline-folder".into());
    root.timeline.layers.push(Layer {
        id: "outline-folder".into(),
        name: "Outline folder".into(),
        visible: true,
        locked: false,
        keyframes: vec![key(0, vec![], None)],
        properties: LayerProperties {
            role: LayerRole::Folder,
            outline: true,
            outline_color: Some([0, 255, 0]),
            ..Default::default()
        },
    });
    outlined_root.validate().unwrap();
    let static_outline = evaluate_scene(&outlined_root, &root_id, 0, true, true);
    assert_eq!(
        draw(&static_outline, "animated-paint").outline_color,
        Some([0, 255, 0, 255])
    );
    let mut preview = AuthoredPlayback::new(&project, &root_id, 0);
    for _ in 0..190 {
        preview.tick(&project);
    }
    assert_eq!(preview.frame(), 0, "root playhead stays on its only frame");
    let middle = preview.scene(&project, true, true);
    let animated = draw(&middle, "animated-paint");
    near(animated.item.transform[4], 190.);
    assert_eq!(
        animated.item.color_transform.apply([100, 150, 200, 255]),
        [100, 120, 200, 159]
    );
    assert!(animated.outline_color.is_none());
    assert!(matches!(
        animated.item.kind,
        ElementKind::PaintedPath {
            fill_gradient: Some(_),
            ..
        }
    ));
    assert_eq!(animated.clip_ids.len(), 1);
    assert_eq!(middle.clips.len(), 1);
    assert!(
        !middle
            .draws
            .iter()
            .any(|draw| draw.item.element_id == "mask-paint")
    );
    assert!(middle.contains_point(&animated.item.instance_path, [195., 5.]));
    assert!(!middle.contains_point(&animated.item.instance_path, [405., 5.]));
    let graphic_x = |parent: &str| {
        middle
            .draws
            .iter()
            .find(|draw| {
                draw.item.element_id == "graphic-paint"
                    && draw.item.instance_path.iter().any(|part| part == parent)
            })
            .unwrap()
            .item
            .transform[4]
    };
    near(graphic_x("movie-graphic"), 30.);
    near(graphic_x("root-graphic"), 0.);
    assert!(
        preview
            .scene(&project, true, false)
            .draws
            .iter()
            .any(|draw| draw.item.element_id == "mask-paint")
    );
    // No tick means a paused transport preserves the independent frame.
    near(
        draw(&preview.scene(&project, true, true), "animated-paint")
            .item
            .transform[4],
        190.,
    );
    for _ in 190..381 {
        preview.tick(&project);
    }
    near(
        draw(&preview.scene(&project, true, true), "animated-paint")
            .item
            .transform[4],
        0.,
    );
    assert_eq!(project.to_json().unwrap(), original);
    near(
        draw(
            &evaluate_scene(&project, &root_id, 0, true, true),
            "animated-paint",
        )
        .item
        .transform[4],
        0.,
    );

    // A blank interval ends the placement lifetime even when the author reuses
    // the same element ID. Re-entry starts a fresh independent clock.
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 4;
    root.timeline.layers[0].keyframes = vec![
        key(0, vec![placement.clone()], None),
        key(2, vec![], None),
        key(3, vec![placement], None),
    ];
    project.validate().unwrap();
    let mut preview = AuthoredPlayback::new(&project, &root_id, 0);
    preview.tick(&project);
    near(
        draw(&preview.scene(&project, true, true), "animated-paint")
            .item
            .transform[4],
        1.,
    );
    preview.tick(&project);
    assert!(
        !preview
            .scene(&project, true, true)
            .draws
            .iter()
            .any(|draw| draw.item.element_id == "animated-paint")
    );
    preview.tick(&project);
    near(
        draw(&preview.scene(&project, true, true), "animated-paint")
            .item
            .transform[4],
        0.,
    );
    let mut selected_symbol = AuthoredPlayback::new(&project, "long-movie", 190);
    assert_eq!(selected_symbol.frame(), 190);
    selected_symbol.tick(&project);
    near(
        draw(
            &selected_symbol.scene(&project, true, true),
            "animated-paint",
        )
        .item
        .transform[4],
        191.,
    );
}
