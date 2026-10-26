use neoflash_core::*;

fn square(x: f64, y: f64, size: f64, reverse: bool) -> Vec<PathCommand> {
    let mut points = [[x, y], [x + size, y], [x + size, y + size], [x, y + size]];
    if reverse {
        points.reverse();
    }
    let mut commands = vec![PathCommand::MoveTo {
        x: points[0][0],
        y: points[0][1],
    }];
    commands.extend(
        points[1..]
            .iter()
            .map(|p| PathCommand::LineTo { x: p[0], y: p[1] }),
    );
    commands.push(PathCommand::Close);
    commands
}
fn path(commands: Vec<PathCommand>) -> ElementKind {
    ElementKind::Path {
        commands,
        fill_rule: FillRule::EvenOdd,
        fill: [230, 100, 20, 255],
        stroke: [20, 30, 40, 255],
        stroke_width: 2.,
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn path_roundtrips_every_command_and_defaults_fill_rule_for_compatibility() {
    let commands = vec![
        PathCommand::MoveTo { x: 0., y: 1. },
        PathCommand::LineTo { x: 2., y: 3. },
        PathCommand::QuadraticTo {
            cx: 4.,
            cy: 5.,
            x: 6.,
            y: 7.,
        },
        PathCommand::CubicTo {
            cx1: 8.,
            cy1: 9.,
            cx2: 10.,
            cy2: 11.,
            x: 12.,
            y: 13.,
        },
        PathCommand::Close,
    ];
    let original = path(commands);
    let mut json = serde_json::to_value(&original).unwrap();
    assert_eq!(json["type"], "path");
    assert_eq!(json["commands"][2]["op"], "quadratic_to");
    assert_eq!(json["commands"][3]["cx2"], 10.);
    assert_eq!(json["fill_rule"], "evenodd");
    assert_eq!(
        serde_json::from_value::<ElementKind>(json.clone()).unwrap(),
        original
    );
    json.as_object_mut().unwrap().remove("fill_rule");
    assert!(matches!(
        serde_json::from_value::<ElementKind>(json).unwrap(),
        ElementKind::Path {
            fill_rule: FillRule::NonZero,
            ..
        }
    ));
    let old = Project::demo();
    assert_eq!(Project::from_json(&old.to_json().unwrap()).unwrap(), old);
}

#[test]
fn curve_bounds_use_extrema_instead_of_the_control_hull() {
    let quadratic = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::QuadraticTo {
            cx: 50.,
            cy: 100.,
            x: 100.,
            y: 0.,
        },
    ];
    let bounds = path_bounds(&quadratic).unwrap().unwrap();
    near(bounds.x_min, 0.);
    near(bounds.x_max, 100.);
    near(bounds.y_max, 50.);
    let cubic = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::CubicTo {
            cx1: 0.,
            cy1: 100.,
            cx2: 100.,
            cy2: 100.,
            x: 100.,
            y: 0.,
        },
    ];
    let bounds = path_bounds(&cubic).unwrap().unwrap();
    near(bounds.y_max, 75.);
    near(bounds.width(), 100.);
    near(bounds.height(), 75.);
    assert!(bounds.expanded(2.).contains([-1., 20.]));
    assert_eq!(path_bounds(&[]).unwrap(), None);
    assert_eq!(
        path_bounds(&[PathCommand::MoveTo { x: 9., y: 8. }]).unwrap(),
        None
    );
    assert_eq!(
        path_bounds(&[PathCommand::MoveTo { x: 9., y: 8. }, PathCommand::Close]).unwrap(),
        None
    );
}

#[test]
fn flattening_preserves_compound_contours_and_endpoints() {
    let mut commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::QuadraticTo {
            cx: 50.,
            cy: 100.,
            x: 100.,
            y: 0.,
        },
        PathCommand::CubicTo {
            cx1: 120.,
            cy1: -20.,
            cx2: 140.,
            cy2: 40.,
            x: 160.,
            y: 0.,
        },
        PathCommand::Close,
    ];
    commands.extend(square(20., 10., 10., false));
    let coarse = flatten_path(
        &commands,
        FlattenOptions {
            tolerance: 2.,
            ..Default::default()
        },
    )
    .unwrap();
    let fine = flatten_path(
        &commands,
        FlattenOptions {
            tolerance: 0.1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(fine.len(), 2);
    assert!(fine.iter().all(|c| c.closed));
    assert_eq!(fine[0].points.first(), Some(&[0., 0.]));
    assert_eq!(fine[0].points.last(), Some(&[160., 0.]));
    assert!(fine[0].points.len() > coarse[0].points.len());
    assert_eq!(
        fine,
        flatten_path(
            &commands,
            FlattenOptions {
                tolerance: 0.1,
                ..Default::default()
            }
        )
        .unwrap()
    );
}

#[test]
fn fill_rules_respect_holes_and_contour_orientation() {
    let mut same = square(0., 0., 20., false);
    same.extend(square(5., 5., 10., false));
    let hit = |commands: &[PathCommand], rule, point| {
        hit_test_path(commands, rule, point, true, 0., FlattenOptions::default()).unwrap()
    };
    assert!(hit(&same, FillRule::NonZero, [10., 10.]));
    assert!(!hit(&same, FillRule::EvenOdd, [10., 10.]));
    assert!(hit(&same, FillRule::EvenOdd, [2., 2.]));
    assert!(!hit(&same, FillRule::EvenOdd, [30., 10.]));
    let mut reversed = square(0., 0., 20., false);
    reversed.extend(square(5., 5., 10., true));
    assert!(!hit(&reversed, FillRule::NonZero, [10., 10.]));
    assert!(hit(&reversed, FillRule::NonZero, [2., 2.]));
}

#[test]
fn open_paths_fill_implicitly_but_do_not_stroke_an_imaginary_closing_edge() {
    let mut commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: 10., y: 0. },
        PathCommand::LineTo { x: 10., y: 10. },
    ];
    let hit = |commands: &[PathCommand], point, fill, stroke| {
        hit_test_path(
            commands,
            FillRule::NonZero,
            point,
            fill,
            stroke,
            FlattenOptions::default(),
        )
        .unwrap()
    };
    assert!(hit(&commands, [8., 2.], true, 0.));
    assert!(!hit(&commands, [5., 5.], false, 1.));
    assert!(hit(&commands, [-0.5, 0.], false, 2.));
    assert!(!hit(&commands, [-1.1, 0.], false, 2.));
    commands.push(PathCommand::Close);
    assert!(hit(&commands, [5., 5.], false, 1.));
    assert!(!hit(&commands, [8., 2.], false, 0.));
}

#[test]
fn picking_curves_follows_the_curve_instead_of_its_bounding_box() {
    let commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::QuadraticTo {
            cx: 50.,
            cy: 100.,
            x: 100.,
            y: 0.,
        },
    ];
    let hit = |point| {
        hit_test_path(
            &commands,
            FillRule::NonZero,
            point,
            false,
            2.,
            FlattenOptions {
                tolerance: 0.01,
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert!(hit([50., 50.]));
    assert!(!hit([50., 10.]));
    assert!(!hit([50., 90.]));
}

#[test]
fn limits_fail_explicitly_without_returning_partial_geometry() {
    let commands = vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::QuadraticTo {
            cx: 50.,
            cy: 100.,
            x: 100.,
            y: 0.,
        },
    ];
    assert_eq!(
        flatten_path(
            &commands,
            FlattenOptions {
                max_depth: 0,
                ..Default::default()
            }
        ),
        Err(GeometryError::ComplexityLimit)
    );
    assert_eq!(
        flatten_path(
            &commands,
            FlattenOptions {
                max_points: 1,
                ..Default::default()
            }
        ),
        Err(GeometryError::ComplexityLimit)
    );
    assert_eq!(
        flatten_path(
            &commands,
            FlattenOptions {
                tolerance: f64::NAN,
                ..Default::default()
            }
        ),
        Err(GeometryError::InvalidOptions)
    );
    assert_eq!(
        flatten_path(
            &commands,
            FlattenOptions {
                max_depth: 25,
                ..Default::default()
            }
        ),
        Err(GeometryError::InvalidOptions)
    );
    assert!(validate_path_commands(&[PathCommand::LineTo { x: 0., y: 0. }]).is_err());
    assert!(
        validate_path_commands(&[
            PathCommand::MoveTo { x: 0., y: 0. },
            PathCommand::Close,
            PathCommand::LineTo { x: 1., y: 2. }
        ])
        .is_err()
    );
    assert!(
        validate_path_commands(&[PathCommand::MoveTo {
            x: f64::INFINITY,
            y: 0.
        }])
        .is_err()
    );
    assert!(
        validate_path_commands(&vec![
            PathCommand::MoveTo { x: 0., y: 0. };
            MAX_PATH_COMMANDS + 1
        ])
        .is_err()
    );
}

#[test]
fn compatible_path_topology_morphs_and_incompatible_topology_holds() {
    let mut project = Project::blank();
    let symbol = &mut project.symbols[0];
    symbol.timeline.frame_count = 11;
    let from = Element {
        id: new_id("path"),
        name: "Contour".into(),
        transform: Transform::default(),
        kind: path(square(0., 0., 10., false)),
        scripts: vec![],
    };
    let mut to = from.clone();
    to.kind = path(square(20., 0., 30., false));
    if let ElementKind::Path {
        stroke_width, fill, ..
    } = &mut to.kind
    {
        *stroke_width = 6.;
        *fill = [0, 0, 0, 255];
    }
    symbol.timeline.layers[0].keyframes = vec![
        Keyframe {
            frame: 0,
            elements: vec![from.clone()],
            tween: Some(Easing::Linear),
            label: String::new(),
        },
        Keyframe {
            frame: 10,
            elements: vec![to.clone()],
            tween: None,
            label: String::new(),
        },
    ];
    project.validate().unwrap();
    let middle = evaluate(&project, &project.root, 5);
    let ElementKind::Path {
        commands,
        stroke_width,
        fill,
        ..
    } = &middle[0].kind
    else {
        panic!()
    };
    assert_eq!(commands[0], PathCommand::MoveTo { x: 10., y: 0. });
    assert_eq!(*stroke_width, 4.);
    assert_eq!(*fill, [115, 50, 10, 255]);
    if let ElementKind::Path { commands, .. } = &mut to.kind {
        commands.pop();
    }
    project.symbols[0].timeline.layers[0].keyframes[1].elements[0] = to;
    assert_eq!(evaluate(&project, &project.root, 5)[0].kind, from.kind);
}

#[test]
fn invalid_path_geometry_is_rejected_on_document_load() {
    let mut project = Project::blank();
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: new_id("path"),
            name: "Bad".into(),
            transform: Transform::default(),
            kind: path(vec![PathCommand::LineTo { x: 3., y: 4. }]),
            scripts: vec![],
        });
    assert!(project.validate().is_err());
    let json = serde_json::to_string(&project).unwrap();
    assert!(Project::from_json(&json).is_err());
}
