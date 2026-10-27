use neoflash_core::*;

fn add(
    player: &mut Player,
    project: &Project,
    parent: &ObjectRef,
    child: &ObjectRef,
) -> MutationResult {
    player
        .display_mutate(
            project,
            DisplayMutation::Add {
                parent: parent.clone(),
                child: child.clone(),
                index: None,
            },
        )
        .unwrap()
}
fn object<'a>(scene: &'a RuntimeSnapshot, id: &ObjectRef) -> &'a RuntimeObject {
    scene
        .objects
        .iter()
        .find(|node| &node.object_ref == id)
        .unwrap()
}

#[test]
fn runtime_component_keeps_identity_geometry_and_order_through_its_lifecycle() {
    let mut project = Project::blank();
    let root_id = project.root.clone();
    let mut symbol = project.symbols[0].clone();
    symbol.id = "component".into();
    symbol.name = "Component".into();
    symbol.timeline.layers[0].id = "component-layer".into();
    project.symbols.push(symbol);
    let placement = Element {
        id: "authored-container".into(),
        name: "holder".into(),
        transform: Transform {
            x: 100.,
            ..Transform::default()
        },
        scripts: vec![],
        accessibility: None,
        kind: ElementKind::Instance {
            symbol_id: "component".into(),
            playback: InstancePlayback::Independent,
        },
    };
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.frame_count = 3;
    root.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![placement.clone()],
            tween: None,
            label: String::new(),
        },
        Keyframe {
            frame: 1,
            elements: vec![],
            tween: None,
            label: String::new(),
        },
        Keyframe {
            frame: 2,
            elements: vec![placement],
            tween: None,
            label: String::new(),
        },
    ];
    assert!(validate(&project).is_empty());
    let original = project.to_json().unwrap();
    let authored = evaluate(&project, &project.root, 0);
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    let left = player.create_display(DynamicKind::Sprite).unwrap();
    let right = player.create_display(DynamicKind::Sprite).unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    let movie = player.create_display(DynamicKind::MovieClip).unwrap();
    assert!(player.object_set_property(&left, "x", 10.));
    assert!(player.object_set_property(&right, "x", 300.));
    assert!(player.object_set_property(&shape, "x", 5.));
    assert!(player.object_set_name(&shape, "drawing"));
    assert!(player.object_control(&project, &movie, PlaybackCommand::Stop));
    assert_eq!(player.object_frame_count(&project, &movie), Some(1));
    assert!(!player.object_control(&project, &movie, PlaybackCommand::GotoAndPlay(1)));

    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginFill {
                    color: 0x336699,
                    alpha: 1.,
                },
                GraphicsOp::DrawRect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 100.,
                },
                GraphicsOp::DrawRect {
                    x: 20.,
                    y: 20.,
                    width: 40.,
                    height: 40.,
                },
                GraphicsOp::EndFill,
                GraphicsOp::BeginFill {
                    color: 0xff8844,
                    alpha: 1.,
                },
                GraphicsOp::MoveTo { x: 120., y: 0. },
                GraphicsOp::LineTo { x: 160., y: 0. },
                GraphicsOp::LineStyle {
                    width: Some(2.),
                    color: 0x112233,
                    alpha: 1.,
                },
                GraphicsOp::CurveTo {
                    cx: 180.,
                    cy: 40.,
                    x: 120.,
                    y: 80.,
                },
                GraphicsOp::EndFill,
                GraphicsOp::BeginFill {
                    color: 0x11aa66,
                    alpha: 0.5,
                },
                GraphicsOp::DrawEllipse {
                    x: 200.,
                    y: 10.,
                    width: 60.,
                    height: 40.,
                },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    let detached = player.runtime_snapshot(&project);
    assert!(!object(&detached, &shape).on_stage);
    assert!(!object(&detached, &shape).graphics.is_empty());
    assert!(detached.draws.iter().all(|draw| draw.owner_ref != shape));
    for kind in &object(&detached, &shape).graphics {
        let RuntimeDrawKind::Element(ElementKind::Path { commands, .. }) = kind else {
            panic!("shared path paint")
        };
        validate_path_commands(commands).unwrap();
    }
    let RuntimeDrawKind::Element(ElementKind::Path {
        commands,
        fill_rule,
        ..
    }) = &object(&detached, &shape).graphics[0]
    else {
        panic!("fill")
    };
    assert!(
        hit_test_path(
            commands,
            *fill_rule,
            [5., 5.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(
        !hit_test_path(
            commands,
            *fill_rule,
            [30., 30.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    // Changing stroke style kept the fill's preceding line, rather than splitting it.
    let RuntimeDrawKind::Element(ElementKind::Path { commands, .. }) =
        &object(&detached, &shape).graphics[1]
    else {
        panic!("fill")
    };
    assert!(
        commands
            .iter()
            .any(|op| matches!(op, PathCommand::LineTo { x: 160., .. }))
    );
    assert!(
        commands
            .iter()
            .any(|op| matches!(op, PathCommand::QuadraticTo { .. }))
    );

    add(&mut player, &project, &holder, &left);
    add(&mut player, &project, &root, &right);
    add(&mut player, &project, &left, &shape);
    add(&mut player, &project, &left, &movie);
    let scene = player.runtime_snapshot(&project);
    assert_eq!(object(&scene, &shape).display.world[4], 115.);
    assert!(
        scene
            .draws
            .iter()
            .filter(|draw| draw.owner_ref == shape)
            .all(|draw| draw.item.transform[4] == 115.)
    );
    let before = serde_json::to_value(&scene).unwrap();
    for invalid in [
        DisplayMutation::Add {
            parent: root.clone(),
            child: right.clone(),
            index: Some(0),
        },
        DisplayMutation::Add {
            parent: shape.clone(),
            child: left.clone(),
            index: None,
        },
        DisplayMutation::Add {
            parent: movie.clone(),
            child: left.clone(),
            index: None,
        },
        DisplayMutation::Remove {
            parent: root.clone(),
            child: holder.clone(),
        },
    ] {
        assert!(player.display_mutate(&project, invalid).is_err());
    }
    assert_eq!(
        serde_json::to_value(player.runtime_snapshot(&project)).unwrap(),
        before
    );
    let events = add(&mut player, &project, &right, &shape).events;
    assert_eq!(
        events
            .iter()
            .map(|event| event.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["removed", "added"]
    );
    assert_eq!(events[0].route[1], left);
    assert_eq!(events[1].route[1], right);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &shape)
            .display
            .world[4],
        305.
    );
    add(&mut player, &project, &right, &movie);
    player
        .display_mutate(
            &project,
            DisplayMutation::Swap {
                parent: right.clone(),
                first: shape.clone(),
                second: movie.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        object(&player.runtime_snapshot(&project), &right).children,
        vec![movie.clone(), shape.clone()]
    );
    player
        .display_mutate(
            &project,
            DisplayMutation::SetIndex {
                parent: right.clone(),
                child: shape.clone(),
                index: 0,
            },
        )
        .unwrap();
    let removed = player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: right.clone(),
                child: shape.clone(),
            },
        )
        .unwrap();
    assert_eq!(removed.events[0].kind, "removed");
    assert_eq!(removed.events[1].kind, "removed_from_stage");
    assert!(player.object_alive(&shape));
    let scene = player.runtime_snapshot(&project);
    assert_eq!(object(&scene, &shape).parent_ref, None);
    assert_eq!(object(&scene, &shape).display.world[4], 5.);
    assert!(scene.draws.iter().all(|draw| draw.owner_ref != shape));
    add(&mut player, &project, &left, &shape);
    player.object_set_visible(&left, false);
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .all(|draw| draw.owner_ref != shape)
    );
    player.object_set_visible(&left, true);

    // Batch rejection preserves all previous graphics; clear is not partially applied.
    let paints = object(&player.runtime_snapshot(&project), &shape)
        .graphics
        .clone();
    assert!(
        player
            .graphics_apply(
                &shape,
                &[
                    GraphicsOp::Clear,
                    GraphicsOp::LineTo {
                        x: f64::INFINITY,
                        y: 0.
                    }
                ]
            )
            .is_err()
    );
    assert!(
        player
            .graphics_apply(&shape, &vec![GraphicsOp::Clear; MAX_GRAPHICS_OPS + 1])
            .is_err()
    );
    assert_eq!(
        object(&player.runtime_snapshot(&project), &shape).graphics,
        paints
    );
    assert!(!player.object_set_property(&shape, "scale_x", 1e200));
    assert!(!player.object_set_text(&project, &shape, "unsupported"));

    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    let scene = player.runtime_snapshot(&project);
    assert!(!player.object_alive(&holder));
    assert!(player.object_alive(&left));
    assert_eq!(object(&scene, &left).parent_ref, None);
    assert_eq!(object(&scene, &shape).parent_ref, Some(left.clone()));
    assert_eq!(object(&scene, &shape).display.world[4], 15.);
    assert!(!object(&scene, &shape).on_stage);
    assert!(
        player
            .drain_display_events()
            .unwrap()
            .events
            .iter()
            .any(|event| event.kind == "removed_from_stage" && event.target_ref == shape)
    );
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(replacement, holder);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &left).parent_ref,
        None
    );
    add(&mut player, &project, &replacement, &left);
    assert!(object(&player.runtime_snapshot(&project), &shape).on_stage);
    assert_eq!(evaluate(&project, &project.root, 0), authored);
    assert_eq!(project.to_json().unwrap(), original);

    let another = Player::new(&project);
    assert!(!another.object_alive(&root));
    player.reset(&project);
    assert!(!player.object_alive(&shape));
    assert!(!player.object_alive(&root));
    assert!(player.graphics_apply(&shape, &[GraphicsOp::Clear]).is_err());
    assert_eq!(player.evaluate(&project), authored);
    // Detached allocation is bounded, including objects never attached to Stage.
    for _ in 0..MAX_DYNAMIC_OBJECTS {
        player.create_display(DynamicKind::Shape).unwrap();
    }
    assert!(player.create_display(DynamicKind::Sprite).is_err());
    gradient_geometry_remains_shared_and_atomic(project.clone());
    affine_color_views_follow_the_display_lifecycle(project.clone());
    linked_timelines_are_independent_namespaces(project);
}

fn gradient_geometry_remains_shared_and_atomic(project: Project) {
    let authored = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    let copy = player.create_display(DynamicKind::Sprite).unwrap();
    add(&mut player, &project, &root, &shape);
    let linear = GradientSpec {
        kind: GradientKind::Linear,
        colors: vec![0x204060, 0x80a0c0, 0xffffff],
        alphas: vec![0., 0.5, 1.],
        ratios: vec![0., 128., 255.],
        matrix: [0.1, 0., 0., 0.1, 60., 40.],
        spread_method: "pad".into(),
        interpolation_method: "rgb".into(),
        focal_point_ratio: 0.,
    };
    let radial = GradientSpec {
        kind: GradientKind::Radial,
        focal_point_ratio: 1.,
        ..linear.clone()
    };
    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginGradientFill {
                    gradient: linear.clone(),
                },
                GraphicsOp::LineStyle {
                    width: Some(6.),
                    color: 0x123456,
                    alpha: 1.,
                },
                GraphicsOp::DrawRoundRect {
                    x: 0.,
                    y: 0.,
                    width: 120.,
                    height: 80.,
                    ellipse_width: 24.,
                    ellipse_height: 32.,
                },
                GraphicsOp::LineGradientStyle {
                    gradient: radial.clone(),
                },
                GraphicsOp::MoveTo { x: 0., y: 100. },
                GraphicsOp::CubicCurveTo {
                    cx1: 30.,
                    cy1: 75.,
                    cx2: 90.,
                    cy2: 125.,
                    x: 120.,
                    y: 100.,
                },
                GraphicsOp::DrawRoundRectComplex {
                    x: 140.,
                    y: 0.,
                    width: 100.,
                    height: 80.,
                    top_left_radius: 10.,
                    top_right_radius: 20.,
                    bottom_left_radius: 15.,
                    bottom_right_radius: 5.,
                },
                GraphicsOp::DrawPath {
                    commands: vec![
                        PathCommand::MoveTo { x: 0., y: 140. },
                        PathCommand::LineTo { x: 100., y: 140. },
                        PathCommand::LineTo { x: 100., y: 200. },
                        PathCommand::Close,
                    ],
                    fill_rule: FillRule::NonZero,
                },
                GraphicsOp::LineTo { x: 20., y: 150. },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    let snapshot = player.runtime_snapshot(&project);
    let raw = object(&snapshot, &shape).graphics.clone();
    for paint in &raw {
        let commands = match paint {
            RuntimeDrawKind::Element(ElementKind::Path { commands, .. })
            | RuntimeDrawKind::GradientPath { commands, .. } => commands,
            _ => panic!("shared path geometry"),
        };
        validate_path_commands(commands).unwrap();
    }
    let RuntimeDrawKind::GradientPath {
        commands,
        fill_rule,
        fill_gradient: Some(gradient),
        stroke_gradient,
        stroke_width,
        ..
    } = &raw[0]
    else {
        panic!("gradient fill")
    };
    assert_eq!(gradient.kind, GradientKind::Linear);
    assert_eq!(gradient.stops[1].color, [128, 160, 192, 128]);
    assert!(stroke_gradient.is_none());
    assert_eq!(*stroke_width, 0.);
    assert!(
        hit_test_path(
            commands,
            *fill_rule,
            [60., 40.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(
        !hit_test_path(
            commands,
            *fill_rule,
            [0., 0.],
            true,
            0.,
            FlattenOptions::default()
        )
        .unwrap()
    );
    assert!(raw.iter().any(|p| matches!(
        p,
        RuntimeDrawKind::GradientPath {
            fill_rule: FillRule::NonZero,
            ..
        }
    )));
    assert!(raw.iter().any(|p|matches!(p,RuntimeDrawKind::GradientPath{stroke_gradient:Some(g),fill_gradient:None,fill,..} if g.kind==GradientKind::Radial && *fill==[0;4])));
    let transform = ColorTransform {
        multipliers: [0.5, 1., 1., 0.5],
        offsets: [10., 0., 0., 10.],
    };
    player
        .object_set_color_transform(&shape, transform)
        .unwrap();
    let transformed = player.runtime_snapshot(&project);
    assert_eq!(object(&transformed, &shape).graphics, raw);
    let first = transformed
        .draws
        .iter()
        .find(|d| d.owner_ref == shape)
        .unwrap();
    let RuntimeDrawKind::GradientPath {
        fill_gradient: Some(gradient),
        ..
    } = &first.item.kind
    else {
        panic!("transformed gradient")
    };
    assert_eq!(gradient.stops[0].color, transform.apply([32, 64, 96, 0]));
    assert_eq!(first.item.alpha, 1.);
    player.graphics_copy_from(&copy, &shape).unwrap();
    player.graphics_apply(&shape, &[GraphicsOp::Clear]).unwrap();
    assert_eq!(
        object(&player.runtime_snapshot(&project), &copy).graphics,
        raw
    );
    let mut invalid = linear;
    invalid.ratios = vec![255., 128., 0.];
    for operations in [
        vec![
            GraphicsOp::Clear,
            GraphicsOp::BeginGradientFill { gradient: invalid },
        ],
        vec![
            GraphicsOp::Clear,
            GraphicsOp::LineGradientStyle { gradient: radial },
        ],
        vec![
            GraphicsOp::Clear,
            GraphicsOp::DrawPath {
                commands: vec![PathCommand::LineTo { x: f64::NAN, y: 0. }],
                fill_rule: FillRule::EvenOdd,
            },
        ],
    ] {
        assert!(player.graphics_apply(&copy, &operations).is_err());
        assert_eq!(
            object(&player.runtime_snapshot(&project), &copy).graphics,
            raw
        );
    }
    let wire = serde_json::to_value(&raw[0]).unwrap();
    assert_eq!(wire["type"], "gradient_path");
    assert_eq!(wire["fill_rule"], "evenodd");
    assert_eq!(wire["fill_gradient"]["stops"][1]["ratio"], 128);
    assert_eq!(project.to_json().unwrap(), authored);
}

fn affine_color_views_follow_the_display_lifecycle(mut project: Project) {
    let marker = Element {
        id: "transform-marker".into(),
        name: "marker".into(),
        transform: Transform::default(),
        scripts: vec![],
        accessibility: Some(Accessibility {
            role: AccessibleRole::Button,
            name: "Marker".into(),
            description: String::new(),
            tab_order: 0,
            disabled: false,
        }),
        kind: ElementKind::Rectangle {
            width: 20.,
            height: 20.,
            fill: [32, 64, 96, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
    };
    project.symbol_mut("component").unwrap().timeline.layers[0].keyframes[0]
        .elements
        .push(marker);
    let untouched = project.to_json().unwrap();
    let mut player = Player::new(&project);
    let root = player.object_ref(&[]).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    let marker = player
        .object_ref(&["authored-container".into(), "transform-marker".into()])
        .unwrap();
    let shape = player.create_display(DynamicKind::Shape).unwrap();
    add(&mut player, &project, &holder, &shape);
    player
        .graphics_apply(
            &shape,
            &[
                GraphicsOp::BeginFill {
                    color: 0x204060,
                    alpha: 0.,
                },
                GraphicsOp::LineStyle {
                    width: Some(2.),
                    color: 0xffffff,
                    alpha: 1.,
                },
                GraphicsOp::DrawRect {
                    x: 0.,
                    y: 0.,
                    width: 30.,
                    height: 20.,
                },
                GraphicsOp::EndFill,
            ],
        )
        .unwrap();
    player
        .object_set_matrix(&holder, [1., 0., 0.5, 1., 20., 30.])
        .unwrap();
    assert!(player.set_property(&["authored-container".into()], "x", 25.));
    assert!(player.object_set_property(&holder, "rotation", 90.));
    assert!(player.object_set_property(&holder, "scale_x", 0.));
    assert!(player.object_set_property(&holder, "scale_x", -2.));
    let expected = [0., -2., -1., 0.5, 25., 30.];
    let matrix = player.object_matrix(&project, &holder).unwrap();
    for (actual, expected) in matrix.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert_eq!(
        player.property(&project, &["authored-container".into()], "scale_x"),
        Some(-2.)
    );
    let color = ColorTransform {
        multipliers: [1., 1., 1., 0.],
        offsets: [10., 20., 30., 96.],
    };
    player.object_set_color_transform(&holder, color).unwrap();
    player
        .object_set_color_transform(
            &shape,
            ColorTransform {
                multipliers: [0.5, 1., 2., 1.],
                offsets: [0.; 4],
            },
        )
        .unwrap();
    assert_eq!(player.object_property(&project, &holder, "alpha"), Some(0.));
    let scene = player.runtime_snapshot(&project);
    assert!(scene.semantics.iter().any(|node| node.owner_ref == marker));
    assert_eq!(object(&scene, &holder).local_matrix, matrix);
    let drawings: Vec<_> = scene
        .draws
        .iter()
        .filter(|draw| draw.owner_ref == shape)
        .collect();
    assert_eq!(drawings.len(), 2);
    assert_eq!(drawings[0].item.alpha, 1.);
    let RuntimeDrawKind::Element(ElementKind::Path { fill, .. }) = &drawings[0].item.kind else {
        panic!("fill");
    };
    assert_eq!(*fill, [26, 84, 222, 96]); // Intentionally transparent fill becomes visible.
    let RuntimeDrawKind::Element(ElementKind::Path { fill, stroke, .. }) = &drawings[1].item.kind
    else {
        panic!("stroke");
    };
    assert_eq!(*fill, [0; 4]); // A stroke run must not invent an interior fill.
    assert_eq!(stroke[3], 96);

    let text = player.create_display(DynamicKind::TextField).unwrap();
    add(&mut player, &project, &holder, &text);
    assert!(player.object_set_text(&project, &text, "Tinted text"));
    let pixels = player.bitmap_create(2, 1, true, 0xff204060).unwrap();
    let bitmap = player.create_display(DynamicKind::Bitmap).unwrap();
    let sibling = player.create_display(DynamicKind::Bitmap).unwrap();
    for node in [&bitmap, &sibling] {
        player
            .object_set_bitmap(&project, node, Some(pixels.clone()))
            .unwrap();
    }
    add(&mut player, &project, &holder, &bitmap);
    add(&mut player, &project, &root, &sibling);
    let scene = player.runtime_snapshot(&project);
    let text_draw = scene
        .draws
        .iter()
        .find(|draw| draw.owner_ref == text)
        .unwrap();
    let RuntimeDrawKind::TextField { field } = &text_draw.item.kind else {
        panic!("text");
    };
    assert_eq!((field.format.color, field.alpha), (0x0a141e, 96));
    assert_eq!(object(&scene, &text).text.as_ref().unwrap().format.color, 0);
    let bitmap_draw = scene
        .draws
        .iter()
        .find(|draw| draw.owner_ref == bitmap)
        .unwrap();
    let RuntimeDrawKind::BitmapPixels {
        color_transform, ..
    } = bitmap_draw.item.kind
    else {
        panic!("bitmap");
    };
    let original_pixels = player.bitmap_pixels(&pixels).unwrap();
    let original_revision = player.bitmap_info(&pixels).unwrap().revision;
    assert_eq!(
        player
            .bitmap_pixels_transformed(&pixels, color_transform)
            .unwrap(),
        [42, 84, 126, 96, 42, 84, 126, 96]
    );
    assert_eq!(
        player
            .bitmap_pixels_transformed(&pixels, ColorTransform::default())
            .unwrap(),
        original_pixels
    );
    assert_eq!(player.bitmap_pixels(&pixels).unwrap(), original_pixels);
    assert_eq!(
        player.bitmap_info(&pixels).unwrap().revision,
        original_revision
    );
    assert_eq!(
        object(&scene, &sibling).concatenated_color_transform,
        ColorTransform::default()
    );

    assert!(player.object_set_matrix(&holder, [f64::NAN; 6]).is_err());
    assert!(
        player
            .object_set_color_transform(
                &holder,
                ColorTransform {
                    offsets: [256.; 4],
                    ..color
                }
            )
            .is_err()
    );
    assert_eq!(player.object_matrix(&project, &holder).unwrap(), matrix);
    assert_eq!(
        player.object_color_transform(&project, &holder).unwrap(),
        color
    );
    player
        .object_set_color_transform(
            &holder,
            ColorTransform {
                offsets: [0.; 4],
                ..color
            },
        )
        .unwrap();
    assert!(
        !player
            .runtime_snapshot(&project)
            .semantics
            .iter()
            .any(|node| node.owner_ref == marker)
    );
    assert!(player.set_property(&["authored-container".into()], "alpha", 0.5));
    assert_eq!(
        player
            .object_color_transform(&project, &holder)
            .unwrap()
            .multipliers[3],
        0.5
    );
    player.object_set_color_transform(&holder, color).unwrap();
    player
        .object_set_matrix(&shape, [1., 0.25, 0., 1., 5., 6.])
        .unwrap();
    player.control(&project, &[], PlaybackCommand::GotoAndStop(1));
    let detached = player.runtime_snapshot(&project);
    assert!(!player.object_alive(&holder));
    assert!(player.object_matrix(&project, &holder).is_err());
    assert!(!object(&detached, &shape).on_stage);
    assert_eq!(
        object(&detached, &shape).local_matrix,
        [1., 0.25, 0., 1., 5., 6.]
    );
    assert_eq!(
        object(&detached, &shape)
            .concatenated_color_transform
            .multipliers,
        [0.5, 1., 2., 1.]
    );
    player.control(&project, &[], PlaybackCommand::GotoAndStop(2));
    let replacement = player.object_ref(&["authored-container".into()]).unwrap();
    assert_ne!(holder, replacement);
    assert_eq!(
        player
            .object_color_transform(&project, &replacement)
            .unwrap(),
        ColorTransform::default()
    );
    assert_eq!(
        player.object_matrix(&project, &replacement).unwrap(),
        [1., 0., 0., 1., 100., 0.]
    );
    add(&mut player, &project, &replacement, &shape);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &shape)
            .display
            .world[4],
        105.
    );
    player.reset(&project);
    assert!(
        player
            .object_set_matrix(&shape, [1., 0., 0., 1., 0., 0.])
            .is_err()
    );
    assert_eq!(project.to_json().unwrap(), untouched);
}

fn linked_timelines_are_independent_namespaces(mut project: Project) {
    let script = |source: &str| Script {
        language: ScriptLanguage::JavaScript,
        source: source.into(),
    };
    let leaf = Element {
        id: "linked-leaf".into(),
        name: "action".into(),
        transform: Transform {
            x: 5.,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width: 20.,
            height: 20.,
            fill: [10, 20, 30, 255],
            stroke: [0; 4],
            stroke_width: 0.,
        },
        accessibility: Some(Accessibility {
            role: AccessibleRole::Button,
            name: "Action".into(),
            description: String::new(),
            tab_order: 1,
            disabled: false,
        }),
        scripts: vec![
            ObjectScript {
                event: ObjectEvent::Load,
                script: script("load();"),
            },
            ObjectScript {
                event: ObjectEvent::Click,
                script: script("click();"),
            },
        ],
    };
    let text = Element {
        id: "linked-text".into(),
        name: "caption".into(),
        transform: Transform::default(),
        accessibility: None,
        scripts: vec![],
        kind: ElementKind::Text {
            text: "Original".into(),
            font_family: "Arial".into(),
            font_size: 16.,
            color: [0, 0, 0, 255],
        },
    };
    let nested = Element {
        id: "linked-nested".into(),
        name: "nested".into(),
        transform: Transform {
            x: 10.,
            ..Transform::default()
        },
        kind: ElementKind::Instance {
            symbol_id: "pulse".into(),
            playback: InstancePlayback::Independent,
        },
        accessibility: None,
        scripts: vec![],
    };
    let mut pulse = project.symbol("component").unwrap().clone();
    pulse.id = "pulse".into();
    pulse.name = "Pulse".into();
    pulse.timeline.frame_count = 4;
    pulse.timeline.layers[0].id = "pulse-layer".into();
    pulse.timeline.layers[0].keyframes[0].elements = vec![leaf.clone()];
    pulse.timeline.scripts = vec![FrameScript {
        frame: 1,
        script: script("pulse();"),
    }];
    project.symbols.push(pulse);
    let component = project.symbol_mut("component").unwrap();
    component.linkage_id = Some("ExportedClip".into());
    component.timeline.frame_count = 3;
    component.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![nested.clone(), text.clone()],
            tween: None,
            label: "Start".into(),
        },
        Keyframe {
            frame: 1,
            elements: vec![nested, text],
            tween: None,
            label: "Turn".into(),
        },
        Keyframe {
            frame: 2,
            elements: vec![],
            tween: None,
            label: "Empty".into(),
        },
    ];
    component.timeline.scripts = vec![
        FrameScript {
            frame: 0,
            script: script("initial();"),
        },
        FrameScript {
            frame: 1,
            script: script("turn();"),
        },
    ];
    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).unwrap();
    root.timeline.layers[0].keyframes[0].elements[0].accessibility = Some(Accessibility {
        role: AccessibleRole::Button,
        name: "Disabled holder".into(),
        description: String::new(),
        tab_order: 0,
        disabled: true,
    });
    project.enable_program();
    assert!(validate(&project).is_empty());
    let mut player = Player::new(&project);
    player.control(&project, &[], PlaybackCommand::Stop);
    let root = player.object_ref(&[]).unwrap();
    let holder = player.object_ref(&["authored-container".into()]).unwrap();
    player.drain_runtime_scripts();
    assert!(player.create_linked(&project, "Component").is_err()); // Never guess by symbol name.
    let first = player.create_linked(&project, "ExportedClip").unwrap();
    let second = player.create_linked(&project, "ExportedClip").unwrap();
    let initial = player.drain_runtime_scripts();
    assert!(
        initial
            .iter()
            .any(|item| item.target_ref == first && item.dispatch.script.source == "initial();")
    );
    assert!(
        initial
            .iter()
            .any(|item| item.target_ref == second && item.dispatch.script.source == "initial();")
    );
    assert!(
        initial
            .iter()
            .all(|item| player.is_runtime_dispatch_current(&project, item))
    );
    let find = |player: &Player, namespace: &ObjectRef, path: &[&str]| {
        player
            .runtime_snapshot(&project)
            .objects
            .iter()
            .find(|node| {
                node.namespace_ref.as_ref() == Some(namespace)
                    && node.source_path.as_deref()
                        == Some(&path.iter().map(|part| part.to_string()).collect::<Vec<_>>())
            })
            .unwrap()
            .object_ref
            .clone()
    };
    let first_nested = find(&player, &first, &["linked-nested"]);
    let first_leaf = find(&player, &first, &["linked-nested", "linked-leaf"]);
    let second_leaf = find(&player, &second, &["linked-nested", "linked-leaf"]);
    let first_text = find(&player, &first, &["linked-text"]);
    let second_text = find(&player, &second, &["linked-text"]);
    assert_ne!(first_leaf, second_leaf);
    assert!(player.object_set_text(&project, &first_text, "First only"));
    assert!(
        matches!(object(&player.runtime_snapshot(&project),&second_text).display.kind,Some(ElementKind::Text{ref text,..}) if text=="Original")
    );
    player.object_set_property(&first, "x", 100.);
    add(&mut player, &project, &holder, &first);
    add(&mut player, &project, &root, &second);
    assert!(!player.object_activate_accessible(&project, &first_leaf));
    assert!(
        player
            .runtime_snapshot(&project)
            .semantics
            .iter()
            .any(|node| node.owner_ref == first_leaf && node.node.accessibility.disabled)
    );
    add(&mut player, &project, &root, &first);
    assert!(player.object_activate_accessible(&project, &first_leaf));
    let clicked = player.drain_runtime_scripts();
    assert_eq!(
        clicked
            .iter()
            .filter(|item| item.dispatch.event == ScriptEvent::Click)
            .count(),
        1
    );
    assert_eq!(clicked[0].target_ref, first_leaf);
    assert_eq!(
        object(&player.runtime_snapshot(&project), &first_leaf)
            .display
            .world[4],
        115.
    );
    for property in ["alpha", "scale_x"] {
        player.object_set_property(&first, property, 0.);
        assert!(
            !player
                .runtime_snapshot(&project)
                .semantics
                .iter()
                .any(|node| node.owner_ref == first_leaf)
        );
        player.object_set_property(&first, property, 1.);
    }
    player.object_set_visible(&first, false);
    assert!(!player.object_click(&project, &first_leaf));
    player.object_set_visible(&first, true);
    assert_eq!(
        player.object_resolve_frame(&project, &first, "turn", true),
        Some(1)
    );
    assert_eq!(
        player.object_resolve_frame(&project, &first, "turn", false),
        None
    );
    player.object_control(&project, &first, PlaybackCommand::GotoAndStop(1));
    player.object_control(&project, &second, PlaybackCommand::Stop);
    let entered = player.drain_runtime_scripts();
    player.tick(&project);
    assert_eq!(player.object_frame(&first), Some(1));
    assert_eq!(player.object_frame(&second), Some(0));
    assert_eq!(player.object_frame(&first_nested), Some(1));
    assert!(
        player.drain_runtime_scripts().iter().any(
            |item| item.target_ref == first_nested && item.dispatch.script.source == "pulse();"
        )
    );
    let decoration = player.create_display(DynamicKind::Sprite).unwrap();
    add(&mut player, &project, &first_nested, &decoration);
    player.object_control(&project, &first, PlaybackCommand::GotoAndStop(2));
    assert!(!player.object_alive(&first_leaf));
    assert!(player.object_alive(&second_leaf));
    assert!(
        entered
            .iter()
            .all(|item| !player.is_runtime_dispatch_current(&project, item))
    );
    assert_eq!(
        object(&player.runtime_snapshot(&project), &decoration).parent_ref,
        None
    );
    player.object_control(&project, &first, PlaybackCommand::GotoAndStop(0));
    let recreated = find(&player, &first, &["linked-nested", "linked-leaf"]);
    assert_ne!(recreated, first_leaf);
    player.drain_runtime_scripts();
    player
        .display_mutate(
            &project,
            DisplayMutation::Remove {
                parent: root.clone(),
                child: first.clone(),
            },
        )
        .unwrap();
    player.object_control(&project, &first, PlaybackCommand::Play);
    player.tick(&project);
    assert_eq!(player.object_frame(&first), Some(1));
    assert!(
        player
            .runtime_snapshot(&project)
            .draws
            .iter()
            .all(|draw| draw.owner_ref != recreated)
    );
    assert!(
        player
            .drain_runtime_scripts()
            .iter()
            .any(|item| item.target_ref == first && item.dispatch.script.source == "turn();")
    );
    add(&mut player, &project, &root, &first);
    assert!(object(&player.runtime_snapshot(&project), &recreated).on_stage);
    player.reset(&project);
    assert!(!player.object_alive(&first));
    assert!(!player.object_alive(&recreated));
}
