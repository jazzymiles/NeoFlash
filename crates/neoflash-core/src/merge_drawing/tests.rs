use super::*;

#[allow(clippy::too_many_arguments)] // Test fixture keeps geometry and both paints explicit.
fn rect_element(
    id: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: Color,
    stroke: Color,
    stroke_width: f64,
) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform {
            x,
            y,
            ..Transform::default()
        },
        kind: ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        },
        merge: true,
        ..Element::default()
    }
}

fn line_element(id: &str, x0: f64, y0: f64, x1: f64, y1: f64, stroke: Color) -> Element {
    Element {
        id: id.into(),
        name: id.into(),
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x: x0, y: y0 },
                PathCommand::LineTo { x: x1, y: y1 },
            ],
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke,
            stroke_width: 2.0,
        },
        merge: true,
        ..Element::default()
    }
}

fn rectangle_polygon(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo { x: x0, y: y0 },
        PathCommand::LineTo { x: x1, y: y0 },
        PathCommand::LineTo { x: x1, y: y1 },
        PathCommand::LineTo { x: x0, y: y1 },
        PathCommand::Close,
    ]
}

fn add_identity_metadata(mut element: Element, label: &str) -> Element {
    element.name = label.into();
    element.scripts = vec![crate::model::ObjectScript {
        event: crate::model::ObjectEvent::Click,
        script: crate::model::Script {
            language: crate::model::ScriptLanguage::ActionScript3,
            source: format!("trace('{label}')"),
        },
    }];
    element.accessibility = Some(crate::model::Accessibility {
        name: label.into(),
        description: "fragment identity".into(),
        role: crate::model::AccessibleRole::Button,
        tab_order: 1,
        disabled: false,
    });
    element
}

fn fill_contains(element: &Element, point: [f64; 2]) -> bool {
    let Some((commands, rule)) = element_world_commands(element) else {
        return false;
    };
    point_in_rings(point, &flatten_closed_rings(&commands), rule)
}

#[test]
fn planar_bucket_returns_an_enclosed_face_and_rejects_hostile_arrangements_early() {
    let lines = vec![
        line_element("top", 0.0, 0.0, 50.0, 0.0, [0, 0, 0, 255]),
        line_element("right", 50.0, 0.0, 50.0, 50.0, [0, 0, 0, 255]),
        line_element("bottom", 50.0, 50.0, 0.0, 50.0, [0, 0, 0, 255]),
        line_element("left", 0.0, 50.0, 0.0, 0.0, [0, 0, 0, 255]),
    ];
    let face = paint_bucket_planar_face(&lines, [25.0, 25.0], 0.0)
        .expect("ordinary arrangement is within the budget")
        .expect("closed strokes enclose a face");
    assert!(matches!(face.first(), Some(PathCommand::MoveTo { .. })));
    assert_eq!(face.last(), Some(&PathCommand::Close));

    let mut commands = Vec::with_capacity(4_003);
    commands.push(PathCommand::MoveTo { x: 0.0, y: 0.0 });
    for index in 1..=4_002 {
        commands.push(PathCommand::LineTo {
            x: index as f64,
            y: (index % 2) as f64,
        });
    }
    let dense = Element {
        id: "hostile-bucket".into(),
        kind: ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.0,
        },
        merge: true,
        ..Element::default()
    };
    assert_eq!(
        paint_bucket_planar_face(&[dense], [1.0, 0.5], 0.0),
        Err(GeometryError::ComplexityLimit)
    );

    // A valid boundary that exceeds the shared flattening point budget must reject the whole
    // arrangement. Silently dropping it and returning the nearby small face can flood across
    // artwork the user expected to be a boundary.
    let mut over_flatten_commands = Vec::with_capacity(65_537);
    over_flatten_commands.push(PathCommand::MoveTo { x: 100.0, y: 0.0 });
    for index in 1..=65_536 {
        over_flatten_commands.push(PathCommand::LineTo {
            x: 100.0 + index as f64,
            y: (index % 2) as f64,
        });
    }
    let over_flatten = Element {
        id: "over-flatten-boundary".into(),
        kind: ElementKind::Path {
            commands: over_flatten_commands,
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            stroke_width: 1.0,
        },
        merge: true,
        ..Element::default()
    };
    let mut mixed = lines.clone();
    mixed.push(over_flatten);
    assert_eq!(
        paint_bucket_planar_face(&mixed, [25.0, 25.0], 0.0),
        Err(GeometryError::ComplexityLimit)
    );
    assert!(matches!(
        paint_bucket_planar_face(&lines, [25.0, 25.0], f64::NAN),
        Err(GeometryError::InvalidPath(_))
    ));
}

#[test]
fn planar_bucket_measures_gap_close_in_the_supplied_affine_metric() {
    let almost_closed = vec![
        line_element("top", 0.0, 0.0, 48.0, 0.0, [0, 0, 0, 255]),
        line_element("right", 50.0, 0.0, 50.0, 50.0, [0, 0, 0, 255]),
        line_element("bottom", 50.0, 50.0, 0.0, 50.0, [0, 0, 0, 255]),
        line_element("left", 0.0, 50.0, 0.0, 0.0, [0, 0, 0, 255]),
    ];
    assert!(
        paint_bucket_planar_face_in_metric(
            &almost_closed,
            [25.0, 25.0],
            [10.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            6.0,
        )
        .unwrap()
        .is_none(),
        "a 20-pixel visual gap must not close"
    );
    assert!(
        paint_bucket_planar_face_in_metric(
            &almost_closed,
            [25.0, 25.0],
            [0.1, 0.0, 0.0, 1.0, 0.0, 0.0],
            6.0,
        )
        .unwrap()
        .is_some(),
        "a compressed 0.2-pixel visual gap should close"
    );

    let visually_open = vec![
        line_element("top", 0.0, 0.0, 49.98, 0.0, [0, 0, 0, 255]),
        line_element("right", 50.0, 0.0, 50.0, 50.0, [0, 0, 0, 255]),
        line_element("bottom", 50.0, 50.0, 0.0, 50.0, [0, 0, 0, 255]),
        line_element("left", 0.0, 50.0, 0.0, 0.0, [0, 0, 0, 255]),
    ];
    assert!(
        paint_bucket_planar_face_in_metric(
            &visually_open,
            [25.0, 25.0],
            [1_000.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            6.0,
        )
        .unwrap()
        .is_none(),
        "the planar vertex snap must not close a 20-pixel visual gap"
    );
}

#[test]
fn polygon_split_slices_merge_fills_and_preserves_bezier_stroke_fragments() {
    let fill = add_identity_metadata(
        rect_element(
            "fill-source",
            0.0,
            0.0,
            100.0,
            100.0,
            [20, 80, 160, 220],
            [0; 4],
            0.0,
        ),
        "Fill identity",
    );
    let stroke = add_identity_metadata(
        Element {
            id: "stroke-source".into(),
            kind: ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 0.0, y: 50.0 },
                    PathCommand::CubicTo {
                        cx1: 25.0,
                        cy1: 10.0,
                        cx2: 75.0,
                        cy2: 90.0,
                        x: 100.0,
                        y: 50.0,
                    },
                ],
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [210, 40, 30, 230],
                stroke_width: 3.5,
            },
            merge: true,
            ..Element::default()
        },
        "Stroke identity",
    );
    let mut elements = vec![fill, stroke];
    let selected =
        split_merge_geometry_by_polygon(&mut elements, &rectangle_polygon(25.0, 0.0, 75.0, 100.0))
            .expect("valid polygon split");

    assert_eq!(
        selected
            .iter()
            .filter(|id| elements
                .iter()
                .any(|element| &element.id == *id && is_merge_fill(element)))
            .count(),
        1
    );
    assert_eq!(
        selected
            .iter()
            .filter(|id| elements
                .iter()
                .any(|element| &element.id == *id && is_merge_stroke(element)))
            .count(),
        1
    );
    let selected_stroke = elements
        .iter()
        .find(|element| selected.contains(&element.id) && is_merge_stroke(element))
        .expect("inside stroke fragment");
    let (inside_stroke_commands, _) = element_world_commands(selected_stroke).unwrap();
    assert!(
        inside_stroke_commands
            .iter()
            .any(|command| matches!(command, PathCommand::CubicTo { .. })),
        "polygon splitting must retain an exact cubic sub-arc"
    );

    let selected_fill = elements
        .iter()
        .find(|element| selected.contains(&element.id) && is_merge_fill(element))
        .expect("inside fill fragment");
    assert!(fill_contains(selected_fill, [50.0, 20.0]));
    assert!(elements.iter().any(|element| {
        is_merge_fill(element)
            && !selected.contains(&element.id)
            && fill_contains(element, [10.0, 20.0])
    }));
    assert!(elements.iter().any(|element| {
        is_merge_fill(element)
            && !selected.contains(&element.id)
            && fill_contains(element, [90.0, 20.0])
    }));

    let retained_fill = elements
        .iter()
        .find(|element| element.id == "fill-source")
        .expect("first outside fill keeps source ID");
    assert_eq!(retained_fill.name, "Fill identity");
    assert_eq!(retained_fill.scripts.len(), 1);
    assert!(retained_fill.accessibility.is_some());
    let retained_stroke = elements
        .iter()
        .find(|element| element.id == "stroke-source")
        .expect("first outside stroke keeps source ID");
    assert_eq!(retained_stroke.name, "Stroke identity");
    assert_eq!(retained_stroke.scripts.len(), 1);
    assert!(retained_stroke.accessibility.is_some());
    for fragment in elements
        .iter()
        .filter(|element| element.id != "fill-source" && element.id != "stroke-source")
    {
        assert!(fragment.name.is_empty());
        assert!(fragment.scripts.is_empty());
        assert!(fragment.accessibility.is_none());
    }
}

#[test]
fn polygon_split_is_atomic_for_invalid_and_over_budget_inputs() {
    let mut elements = vec![line_element(
        "atomic-stroke",
        -10.0,
        0.0,
        10.0,
        0.0,
        [0, 0, 0, 255],
    )];
    let before = elements.clone();
    let open_polygon = vec![
        PathCommand::MoveTo { x: -5.0, y: -5.0 },
        PathCommand::LineTo { x: 5.0, y: -5.0 },
        PathCommand::LineTo { x: 5.0, y: 5.0 },
    ];
    assert!(matches!(
        split_merge_geometry_by_polygon(&mut elements, &open_polygon),
        Err(GeometryError::InvalidPath(_))
    ));
    assert_eq!(elements, before);

    let edge_count = 520usize;
    let mut polygon = Vec::with_capacity(edge_count + 2);
    for index in 0..edge_count {
        let angle = index as f64 * std::f64::consts::TAU / edge_count as f64;
        let command = if index == 0 {
            PathCommand::MoveTo {
                x: angle.cos() * 100.0,
                y: angle.sin() * 100.0,
            }
        } else {
            PathCommand::LineTo {
                x: angle.cos() * 100.0,
                y: angle.sin() * 100.0,
            }
        };
        polygon.push(command);
    }
    polygon.push(PathCommand::Close);
    let mut commands = vec![PathCommand::MoveTo { x: -90.0, y: 0.0 }];
    for index in 1..=edge_count {
        commands.push(PathCommand::LineTo {
            x: -90.0 + index as f64 * 180.0 / edge_count as f64,
            y: (index as f64 * 0.13).sin(),
        });
    }
    elements[0].kind = ElementKind::Path {
        commands,
        fill_rule: FillRule::NonZero,
        fill: [0; 4],
        stroke: [0, 0, 0, 255],
        stroke_width: 2.0,
    };
    let before_budget = elements.clone();
    assert_eq!(
        split_merge_geometry_by_polygon(&mut elements, &polygon),
        Err(GeometryError::ComplexityLimit)
    );
    assert_eq!(elements, before_budget);
}

#[test]
fn partial_merge_edits_refuse_compositing_effects_atomically() {
    let polygon = rectangle_polygon(25.0, 25.0, 75.0, 75.0);
    for effect in ["cache", "blend", "filter"] {
        let mut element = rect_element(
            "protected-fill",
            0.0,
            0.0,
            100.0,
            100.0,
            [20, 80, 160, 255],
            [0; 4],
            0.0,
        );
        match effect {
            "cache" => element.cache_as_bitmap = true,
            "blend" => element.blend_mode = crate::BlendMode::Multiply,
            "filter" => {
                element.filters = vec![crate::FilterSpec::Blur {
                    blur_x: 4.0,
                    blur_y: 4.0,
                    quality: 1,
                }]
            }
            _ => unreachable!(),
        }

        let mut split_elements = vec![element.clone()];
        let split_before = split_elements.clone();
        let split_error =
            split_merge_geometry_by_polygon(&mut split_elements, &polygon).unwrap_err();
        assert!(
            matches!(&split_error, GeometryError::InvalidPath(message) if message.contains("whole-object editing")),
            "{effect}: {split_error}"
        );
        assert_eq!(split_elements, split_before, "{effect} split is atomic");

        let mut erase_elements = vec![element];
        let erase_before = erase_elements.clone();
        let erase_error = erase_merge_geometry_with_sweep(
            &mut erase_elements,
            &[[50.0, -10.0], [50.0, 110.0]],
            8.0,
            0.2,
        )
        .unwrap_err();
        assert!(
            matches!(&erase_error, GeometryError::InvalidPath(message) if message.contains("whole-object editing")),
            "{effect}: {erase_error}"
        );
        assert_eq!(erase_elements, erase_before, "{effect} erase is atomic");
    }
}

#[test]
fn swept_merge_eraser_cuts_fill_and_stroke_and_preserves_one_identity() {
    let fill = add_identity_metadata(
        rect_element(
            "eraser-fill",
            0.0,
            0.0,
            100.0,
            100.0,
            [70, 120, 200, 190],
            [0; 4],
            0.0,
        ),
        "Fill script owner",
    );
    let stroke = add_identity_metadata(
        line_element("eraser-stroke", -20.0, 50.0, 120.0, 50.0, [15, 25, 35, 240]),
        "Stroke script owner",
    );
    let mut elements = vec![fill, stroke];
    assert!(
        erase_merge_geometry_with_sweep(&mut elements, &[[50.0, -20.0], [50.0, 120.0]], 10.0, 0.2,)
            .expect("valid swept erase")
    );

    let fills: Vec<_> = elements
        .iter()
        .filter(|element| is_merge_fill(element))
        .collect();
    let strokes: Vec<_> = elements
        .iter()
        .filter(|element| is_merge_stroke(element))
        .collect();
    assert_eq!(fills.len(), 2, "vertical sweep splits the fill in two");
    assert_eq!(strokes.len(), 2, "vertical sweep splits the stroke in two");
    assert!(
        fills
            .iter()
            .any(|element| fill_contains(element, [20.0, 50.0]))
    );
    assert!(
        fills
            .iter()
            .any(|element| fill_contains(element, [80.0, 50.0]))
    );
    assert!(
        !fills
            .iter()
            .any(|element| fill_contains(element, [50.0, 50.0]))
    );

    for (id, name) in [
        ("eraser-fill", "Fill script owner"),
        ("eraser-stroke", "Stroke script owner"),
    ] {
        let retained = elements
            .iter()
            .find(|element| element.id == id)
            .expect("first surviving piece keeps source ID");
        assert_eq!(retained.name, name);
        assert_eq!(retained.scripts.len(), 1);
        assert!(retained.accessibility.is_some());
    }
    for fragment in elements
        .iter()
        .filter(|element| element.id != "eraser-fill" && element.id != "eraser-stroke")
    {
        assert!(fragment.name.is_empty());
        assert!(fragment.scripts.is_empty());
        assert!(fragment.accessibility.is_none());
    }
    assert!(fills.iter().all(|element| {
        matches!(
            element.kind,
            ElementKind::Path {
                fill: [70, 120, 200, 190],
                ..
            }
        )
    }));
    assert!(strokes.iter().all(|element| {
        matches!(
            element.kind,
            ElementKind::Path {
                stroke: [15, 25, 35, 240],
                stroke_width: 2.0,
                ..
            }
        )
    }));
}

#[test]
fn swept_merge_eraser_keeps_a_round_radius_in_an_affine_metric() {
    let mut elements = vec![rect_element(
        "metric-fill",
        0.0,
        0.0,
        100.0,
        100.0,
        [70, 120, 200, 255],
        [0; 4],
        0.0,
    )];
    assert!(
        erase_merge_geometry_with_metric(
            &mut elements,
            &[[50.0, 50.0]],
            [100.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            10.0,
            0.2,
        )
        .expect("valid affine-metric erase")
    );
    assert!(
        elements
            .iter()
            .any(|element| fill_contains(element, [50.5, 50.0])),
        "50 screen pixels along stretched X must remain"
    );
    assert!(
        !elements
            .iter()
            .any(|element| fill_contains(element, [50.05, 50.0])),
        "five screen pixels along stretched X must erase"
    );
    assert!(
        !elements
            .iter()
            .any(|element| fill_contains(element, [50.0, 59.0])),
        "nine screen pixels along Y must erase"
    );
}

#[test]
fn swept_merge_eraser_is_atomic_for_invalid_or_over_budget_input() {
    let mut elements = vec![rect_element(
        "atomic-fill",
        0.0,
        0.0,
        20.0,
        20.0,
        [20, 40, 60, 255],
        [0; 4],
        0.0,
    )];
    let before = elements.clone();
    assert!(matches!(
        erase_merge_geometry_with_sweep(&mut elements, &[[10.0, 10.0]], f64::NAN, 0.2),
        Err(GeometryError::InvalidPath(_))
    ));
    assert_eq!(elements, before);

    let too_many = (0..=MAX_MERGE_ERASER_POINTS)
        .map(|index| [index as f64 * 0.01, 10.0])
        .collect::<Vec<_>>();
    assert_eq!(
        erase_merge_geometry_with_sweep(&mut elements, &too_many, 2.0, 0.2),
        Err(GeometryError::ComplexityLimit)
    );
    assert_eq!(elements, before);
    assert!(!erase_merge_geometry_with_sweep(&mut elements, &[], 2.0, 0.2).unwrap());
    assert_eq!(elements, before);
}

#[test]
fn prepared_merge_ring_index_limits_boolean_input_to_the_local_islands() {
    let rings: Vec<Vec<[f64; 2]>> = (0..10_000)
        .map(|index| {
            let x = index as f64 * 8.0;
            vec![[x, 0.0], [x + 2.0, 0.0], [x + 2.0, 2.0], [x, 2.0]]
        })
        .collect();
    let prepared = PreparedMergeRings::new(&rings, FillRule::NonZero);
    let target = 7_321usize;
    let x = target as f64 * 8.0;
    let local = prepared.rings_overlapping(&rings, ([x - 0.5, -0.5], [x + 2.5, 2.5]), SNAP_EPS);
    assert_eq!(local, vec![rings[target].clone()]);
}

/// Adjacent triangles are how existing Brush artwork stores a continuous filled shape.
fn triangulated_cells(cells: &[(u32, u32)], fill_rule: FillRule) -> Element {
    let mut commands = Vec::new();
    for &(x, y) in cells {
        let x = f64::from(x) * 20.0;
        let y = f64::from(y) * 20.0;
        for triangle in [
            [[x, y], [x + 20.0, y], [x + 20.0, y + 20.0]],
            [[x, y], [x + 20.0, y + 20.0], [x, y + 20.0]],
        ] {
            commands.extend(ring_to_commands(&triangle));
        }
    }
    Element {
        id: "brush".into(),
        kind: ElementKind::Path {
            commands,
            fill_rule,
            fill: [0, 100, 255, 255],
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        merge: true,
        ..Element::default()
    }
}

fn fills_contain(elements: &[Element], point: [f64; 2]) -> bool {
    elements.iter().filter(|e| is_merge_fill(e)).any(|e| {
        let (commands, rule) = element_world_commands(e).unwrap();
        point_in_rings(point, &flatten_closed_rings(&commands), rule)
    })
}

#[test]
fn zero_width_merge_fills_keep_dormant_stroke_paint_and_gradients() {
    let gradient = GradientPaint {
        stops: vec![
            crate::runtime_graphics::GradientStop {
                ratio: 0,
                color: [10, 20, 30, 180],
            },
            crate::runtime_graphics::GradientStop {
                ratio: 255,
                color: [200, 100, 50, 240],
            },
        ],
        kind: crate::runtime_graphics::GradientKind::Linear,
        matrix: [0.02, 0., 0., 0.02, 0., 0.],
        focal_point_ratio: 0.,
        ..Default::default()
    };
    let source = Element {
        id: "zero-width".into(),
        transform: Transform {
            x: 20.,
            y: 30.,
            ..Default::default()
        },
        kind: ElementKind::PaintedPath {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 40., y: 0. },
                PathCommand::LineTo { x: 40., y: 40. },
                PathCommand::LineTo { x: 0., y: 40. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill: [80, 100, 200, 255],
            stroke: [120, 40, 210, 173],
            fill_gradient: None,
            stroke_gradient: Some(gradient.clone()),
            stroke_width: 0.,
        },
        ..Default::default()
    };
    let (fill, strokes) = decompose_element_to_merge_parts(&source).unwrap();
    assert!(strokes.is_empty(), "width zero creates no visible stroke");
    let fill = fill.unwrap();
    assert!(is_merge_fill(&fill));
    let ElementKind::PaintedPath {
        stroke,
        stroke_width,
        stroke_gradient: Some(saved),
        ..
    } = fill.kind
    else {
        panic!("paint must be retained")
    };
    assert_eq!(stroke, [120, 40, 210, 173]);
    assert_eq!(stroke_width, 0.);
    assert_eq!(saved, world_gradient(&source, &gradient));
}

#[test]
fn merge_triangle_soups_split_only_at_visible_boundaries() {
    let mut elements = Vec::new();
    // L-shaped brush: the upper-right corner of its bounding box contains no artwork.
    apply_merge_shape_to_keyframe(
        &mut elements,
        triangulated_cells(&[(0, 0), (1, 0), (0, 1)], FillRule::NonZero),
    );
    let original = elements[0].clone();
    let mut edge = vec![original.clone()];
    apply_merge_shape_to_keyframe(
        &mut edge,
        line_element("edge", -10.0, 0.0, 50.0, 0.0, [255, 0, 0, 255]),
    );
    assert_eq!(edge.iter().filter(|e| is_merge_fill(e)).count(), 1);
    assert_eq!(
        edge.iter().filter(|e| is_merge_stroke(e)).count(),
        3,
        "a line on a straight boundary must not split at triangle seams"
    );
    for y in [25.0, 30.0] {
        apply_merge_shape_to_keyframe(
            &mut elements,
            line_element("outside", 25.0, y, 35.0, y, [255, 0, 0, 255]),
        );
        let fills: Vec<_> = elements.iter().filter(|e| is_merge_fill(e)).collect();
        assert_eq!(
            fills,
            vec![&original],
            "an unrelated line must leave the fill intact"
        );
    }

    apply_merge_shape_to_keyframe(
        &mut elements,
        line_element("cut", -10.0, 10.0, 50.0, 10.0, [255, 0, 0, 255]),
    );
    assert_eq!(elements.iter().filter(|e| is_merge_fill(e)).count(), 2);
    let cut_strokes: Vec<_> = elements
        .iter()
        .filter(|e| is_merge_stroke(e))
        .filter(|e| {
            let (commands, _) = element_world_commands(e).unwrap();
            flatten_stroke_polylines(&commands)
                .iter()
                .flatten()
                .all(|p| (p[1] - 10.0).abs() < 1e-9)
        })
        .collect();
    assert_eq!(
        cut_strokes.len(),
        3,
        "only the two exterior boundaries cut the line"
    );
    for p in [[5.0, 5.0], [35.0, 15.0], [5.0, 35.0]] {
        assert!(fills_contain(&elements, p));
    }
    assert!(!fills_contain(&elements, [30.0, 30.0]));

    // A new fill trims a pre-existing line only at its exterior, never at triangle seams.
    let mut underneath = vec![line_element(
        "under",
        -10.0,
        30.0,
        50.0,
        30.0,
        [0, 0, 0, 255],
    )];
    apply_merge_shape_to_keyframe(
        &mut underneath,
        triangulated_cells(&[(0, 0), (1, 0), (0, 1)], FillRule::NonZero),
    );
    assert_eq!(underneath.iter().filter(|e| is_merge_stroke(e)).count(), 2);
}

#[test]
fn merge_slicing_preserves_compound_and_triangulated_holes() {
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let cells: Vec<_> = (0..3)
            .flat_map(|x| (0..3).map(move |y| (x, y)))
            .filter(|cell| *cell != (1, 1))
            .collect();
        let triangulated = triangulated_cells(&cells, rule);
        let outer = [[0.0, 0.0], [60.0, 0.0], [60.0, 60.0], [0.0, 60.0]];
        let mut hole = vec![[20.0, 20.0], [40.0, 20.0], [40.0, 40.0], [20.0, 40.0]];
        if rule == FillRule::NonZero {
            hole.reverse();
        }
        let mut compound = triangulated.clone();
        if let ElementKind::Path { commands, .. } = &mut compound.kind {
            *commands = ring_to_commands(&outer);
            commands.extend(ring_to_commands(&hole));
        }
        for shape in [triangulated, compound] {
            let mut elements = Vec::new();
            apply_merge_shape_to_keyframe(&mut elements, shape);
            apply_merge_shape_to_keyframe(
                &mut elements,
                line_element("in-hole", 25.0, 30.0, 35.0, 30.0, [0, 0, 0, 255]),
            );
            assert_eq!(elements.iter().filter(|e| is_merge_fill(e)).count(), 1);
            assert!(!fills_contain(&elements, [30.0, 30.0]));

            apply_merge_shape_to_keyframe(
                &mut elements,
                line_element("cut", -10.0, 10.0, 70.0, 10.0, [0, 0, 0, 255]),
            );
            assert_eq!(elements.iter().filter(|e| is_merge_fill(e)).count(), 2);
            assert_eq!(elements.iter().filter(|e| is_merge_stroke(e)).count(), 4);
            let area: f64 = elements
                .iter()
                .filter(|e| is_merge_fill(e))
                .flat_map(|e| flatten_closed_rings(&element_world_commands(e).unwrap().0))
                .map(|ring| signed_ring_area(&ring))
                .sum();
            assert!((area - 3200.0).abs() < 1e-6, "hole area changed: {area}");
            assert!(!fills_contain(&elements, [30.0, 30.0]));
            for p in [[5.0, 5.0], [5.0, 30.0], [55.0, 30.0], [30.0, 55.0]] {
                assert!(fills_contain(&elements, p));
            }
        }
    }
}

#[test]
fn merge_repeated_outline_moves_preserve_contours_and_real_junctions() {
    let rectangle = rect_element(
        "outlined",
        0.0,
        0.0,
        100.0,
        80.0,
        [20, 100, 220, 255],
        [0, 0, 0, 255],
        2.0,
    );
    let ellipse = Element {
        kind: ElementKind::Ellipse {
            width: 100.0,
            height: 80.0,
            fill: [20, 100, 220, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.0,
        },
        ..rectangle.clone()
    };
    for shape in [rectangle.clone(), ellipse] {
        for reverse in [false, true] {
            let mut elements = Vec::new();
            apply_merge_shape_to_keyframe(&mut elements, shape.clone());
            assert_eq!(elements.len(), 2);
            if reverse {
                elements.reverse();
            }
            for _ in 0..3 {
                for element in &mut elements {
                    element.transform.x = 11.25;
                    element.transform.y = -7.75;
                    element.transform.rotation = 15.0;
                }
                let ids: Vec<_> = elements.iter().map(|e| e.id.clone()).collect();
                remerge_moved_elements_in_keyframe(&mut elements, &ids);
                assert_eq!(elements.iter().filter(|e| is_merge_fill(e)).count(), 1);
                assert_eq!(
                    elements.iter().filter(|e| is_merge_stroke(e)).count(),
                    1,
                    "a coincident outline must not split at its own vertices"
                );
                let outline = elements.iter().find(|e| is_merge_stroke(e)).unwrap();
                let (commands, _) = element_world_commands(outline).unwrap();
                assert!(matches!(commands.last(), Some(PathCommand::Close)));
            }
        }
    }

    // A crossing and an endpoint junction with a DIFFERENT stroke still split the outline.
    for end_y in [0.0, 100.0] {
        let mut elements = Vec::new();
        apply_merge_shape_to_keyframe(&mut elements, rectangle.clone());
        apply_merge_shape_to_keyframe(
            &mut elements,
            line_element("junction", 50.0, -20.0, 50.0, end_y, [255, 0, 0, 255]),
        );
        let outlines: Vec<_> = elements
            .iter()
            .filter(|e| {
                is_merge_stroke(e) && extract_stroke_paint(&e.kind).unwrap().color == [0, 0, 0, 255]
            })
            .collect();
        assert!(
            outlines.len() >= 2,
            "real stroke junction must remain selectable"
        );
        for point in if end_y == 0.0 {
            vec![[50.0, 0.0]]
        } else {
            vec![[50.0, 0.0], [50.0, 80.0]]
        } {
            let ends = outlines
                .iter()
                .flat_map(|e| flatten_stroke_polylines(&element_world_commands(e).unwrap().0))
                .filter(|poly| {
                    dist_sq(poly[0], point) < 1e-9 || dist_sq(*poly.last().unwrap(), point) < 1e-9
                })
                .count();
            assert_eq!(
                ends, 2,
                "both outline pieces must meet at the actual junction"
            );
        }
    }
}

#[test]
fn merge_bakes_fill_and_stroke_gradient_coordinates_once() {
    use crate::runtime_graphics::{GradientKind, GradientStop};

    let fill_gradient = GradientPaint {
        kind: GradientKind::Linear,
        matrix: [0.06103515625, 0.0, 0.0, 0.06103515625, 50.0, 50.0],
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [0, 80, 220, 255],
            },
            GradientStop {
                ratio: 255,
                color: [240, 40, 0, 255],
            },
        ],
        focal_point_ratio: 0.0,
        ..Default::default()
    };
    let stroke_gradient = GradientPaint {
        kind: GradientKind::Radial,
        ..fill_gradient.clone()
    };
    let transform = Transform {
        x: 200.0,
        y: 80.0,
        scale_x: 1.5,
        scale_y: 0.75,
        rotation: 25.0,
        skew: 12.0,
        alpha: 0.6,
        ..Transform::default()
    };
    let source = Element {
        id: "gradient".into(),
        transform,
        kind: ElementKind::PaintedPath {
            commands: ring_to_commands(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]]),
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 255],
            fill_gradient: Some(fill_gradient.clone()),
            stroke_gradient: Some(stroke_gradient.clone()),
            stroke_width: 2.0,
        },
        merge: true,
        ..Element::default()
    };
    let (fill, strokes) = decompose_element_to_merge_parts(&source).unwrap();
    assert!(fill.is_some());
    assert_eq!(strokes.len(), 1);
    // Check individual paints and the mixed fill/outline pair. Normalizing an already baked
    // path must not bake paint twice or split the coincident outline on repeated moves.
    let mut cases: Vec<_> = fill
        .into_iter()
        .chain(strokes)
        .map(|part| (part, 1))
        .collect();
    cases.push((source.clone(), 2));
    for (part, expected_count) in cases {
        let mut elements = Vec::new();
        apply_merge_shape_to_keyframe(&mut elements, part);
        let mut placement_matrix = transform_matrix(transform);
        for round in 0..3 {
            assert_eq!(elements.len(), expected_count);
            for element in &elements {
                assert_eq!(element.transform.x, 0.0);
                assert_eq!(element.transform.y, 0.0);
                assert_eq!(element.transform.alpha, transform.alpha);
                let ElementKind::PaintedPath {
                    fill_gradient: fill,
                    stroke_gradient: stroke,
                    ..
                } = &element.kind
                else {
                    panic!("gradient paint was lost");
                };
                let (actual, original) = if let Some(gradient) = fill {
                    (gradient, &fill_gradient)
                } else {
                    (stroke.as_ref().unwrap(), &stroke_gradient)
                };
                let expected = affine_multiply(placement_matrix, original.matrix);
                for (actual, expected) in actual.matrix.iter().zip(expected) {
                    assert!((actual - expected).abs() < 1e-9);
                }
                for local in [[20.0, 30.0], [50.0, 50.0], [80.0, 60.0]] {
                    let world = transform_point(placement_matrix, local[0], local[1]);
                    for (a, b) in actual
                        .sample(world[0], world[1])
                        .into_iter()
                        .zip(original.sample(local[0], local[1]))
                    {
                        assert!(a.abs_diff(b) <= 1, "paint moved relative to its geometry");
                    }
                }
            }
            if round < 2 {
                let movement = Transform {
                    x: 40.0,
                    y: -25.0,
                    rotation: -15.0,
                    ..Transform::default()
                };
                for element in &mut elements {
                    element.transform = Transform {
                        alpha: transform.alpha,
                        ..movement
                    };
                }
                let ids: Vec<_> = elements.iter().map(|e| e.id.clone()).collect();
                remerge_moved_elements_in_keyframe(&mut elements, &ids);
                placement_matrix = affine_multiply(transform_matrix(movement), placement_matrix);
            }
        }
    }

    // Boolean commands bake transformed object paths through the same paint builders.
    let clip = rect_element(
        "clip",
        100.0,
        0.0,
        400.0,
        400.0,
        [0, 0, 0, 255],
        [0; 4],
        0.0,
    );
    let combined =
        combine_elements_boolean(&[source, clip], BooleanOp::Intersection, false).unwrap();
    assert_eq!(combined.len(), 1);
    let ElementKind::PaintedPath {
        fill_gradient: Some(actual),
        ..
    } = &combined[0].kind
    else {
        panic!("boolean operation lost the gradient");
    };
    assert_eq!(
        actual.matrix,
        affine_multiply(transform_matrix(transform), fill_gradient.matrix)
    );
}

#[test]
fn merge_shape_separates_fill_and_stroke() {
    let mut elements = Vec::new();
    let rect = rect_element(
        "r1",
        10.0,
        20.0,
        100.0,
        80.0,
        [255, 0, 0, 255],
        [0, 0, 0, 255],
        2.0,
    );
    let ids = apply_merge_shape_to_keyframe(&mut elements, rect);
    assert_eq!(ids.len(), 2);
    assert_eq!(elements.len(), 2);
    assert!(is_merge_fill(&elements[0]));
    assert!(is_merge_stroke(&elements[1]));
}

#[test]
fn overlapping_different_color_fills_subtract_underlying() {
    let mut elements = Vec::new();
    let blue = rect_element(
        "blue",
        0.0,
        0.0,
        100.0,
        100.0,
        [0, 100, 255, 255],
        [0; 4],
        0.0,
    );
    apply_merge_shape_to_keyframe(&mut elements, blue);
    assert_eq!(elements.len(), 1);

    // Red vertical strip cutting right through the middle of `blue` (x = 40..60).
    let red = rect_element(
        "red",
        40.0,
        -10.0,
        20.0,
        120.0,
        [255, 40, 40, 255],
        [0; 4],
        0.0,
    );
    apply_merge_shape_to_keyframe(&mut elements, red);

    // `blue` is split into two separate islands (`[0..40]` and `[60..100]`) plus `red` = 3 fills.
    assert_eq!(elements.len(), 3);
    let blue_pieces: Vec<&Element> = elements
        .iter()
        .filter(|e| extract_fill_paint(&e.kind) == Some(FillPaint::Solid([0, 100, 255, 255])))
        .collect();
    assert_eq!(blue_pieces.len(), 2);
}

#[test]
fn overlapping_same_color_fills_without_stroke_union() {
    let mut elements = Vec::new();
    let a = rect_element("a", 0.0, 0.0, 60.0, 60.0, [0, 200, 100, 255], [0; 4], 0.0);
    let b = rect_element("b", 30.0, 0.0, 60.0, 60.0, [0, 200, 100, 255], [0; 4], 0.0);
    apply_merge_shape_to_keyframe(&mut elements, a);
    apply_merge_shape_to_keyframe(&mut elements, b);
    assert_eq!(elements.len(), 1);
    let (cmds, rule) = element_world_commands(&elements[0]).unwrap();
    let rings = flatten_closed_rings(&cmds);
    assert!(point_in_rings([10.0, 30.0], &rings, rule));
    assert!(point_in_rings([80.0, 30.0], &rings, rule));
}

#[test]
fn line_drawn_across_fill_slices_fill_and_splits_line() {
    let mut elements = Vec::new();
    let fill = rect_element(
        "box",
        0.0,
        0.0,
        100.0,
        100.0,
        [240, 180, 0, 255],
        [0; 4],
        0.0,
    );
    apply_merge_shape_to_keyframe(&mut elements, fill);

    let cutter = line_element("cut", -20.0, 50.0, 120.0, 50.0, [0, 0, 0, 255]);
    apply_merge_shape_to_keyframe(&mut elements, cutter);

    let fills: Vec<&Element> = elements.iter().filter(|e| is_merge_fill(e)).collect();
    let strokes: Vec<&Element> = elements.iter().filter(|e| is_merge_stroke(e)).collect();
    // Fill sliced into top and bottom halves:
    assert_eq!(fills.len(), 2);
    // Line split at x=0 and x=100 into left overhang, middle cut, right overhang:
    assert_eq!(strokes.len(), 3);
}

#[test]
fn intersecting_lines_split_into_four_segments() {
    let mut elements = Vec::new();
    let h = line_element("h", 0.0, 50.0, 100.0, 50.0, [0, 0, 0, 255]);
    let v = line_element("v", 50.0, 0.0, 50.0, 100.0, [0, 0, 0, 255]);
    apply_merge_shape_to_keyframe(&mut elements, h);
    apply_merge_shape_to_keyframe(&mut elements, v);

    let strokes: Vec<&Element> = elements.iter().filter(|e| is_merge_stroke(e)).collect();
    assert_eq!(strokes.len(), 4);
    let connected = merge_connected_stroke_ids(&elements, &strokes[0].id);
    assert_eq!(connected.len(), 4);

    // Intersecting lines of different colors only select connected segments of the same color:
    let mut diff_elements = Vec::new();
    let h_black = line_element("hb", 0.0, 50.0, 100.0, 50.0, [0, 0, 0, 255]);
    let v_red = line_element("vr", 50.0, 0.0, 50.0, 100.0, [220, 40, 40, 255]);
    apply_merge_shape_to_keyframe(&mut diff_elements, h_black);
    apply_merge_shape_to_keyframe(&mut diff_elements, v_red);
    let diff_strokes: Vec<&Element> = diff_elements
        .iter()
        .filter(|e| is_merge_stroke(e))
        .collect();
    assert_eq!(diff_strokes.len(), 4);
    let same_color_connected = merge_connected_stroke_ids(&diff_elements, &diff_strokes[0].id);
    assert_eq!(same_color_connected.len(), 2);
}

#[test]
fn combine_elements_boolean_covers_all_ops_local_origin_gradients_and_merge_strokes() {
    use crate::runtime_graphics::{GradientKind, GradientStop};

    let mut base = rect_element(
        "base",
        200.0,
        150.0,
        50.0,
        40.0,
        [30, 120, 220, 255],
        [10, 20, 30, 255],
        3.0,
    );
    base.merge = false;
    base.transform.scale_x = 2.0;
    base.transform.scale_y = 2.0; // world size 100x80 at (200..300, 150..230), scaled stroke = 6.0

    let mut cutter = rect_element(
        "cutter",
        250.0,
        170.0,
        100.0,
        80.0,
        [220, 60, 40, 255],
        [0; 4],
        0.0,
    );
    cutter.merge = false;

    // 1. Union of Object Drawing shapes normalizes local bounds to (0, 0) and scales stroke width.
    let union =
        combine_elements_boolean(&[base.clone(), cutter.clone()], BooleanOp::Union, false).unwrap();
    assert_eq!(union.len(), 1);
    assert!(!union[0].merge);
    assert!((union[0].transform.x - 200.0).abs() < 1e-6);
    assert!((union[0].transform.y - 150.0).abs() < 1e-6);
    let ElementKind::Path {
        commands,
        fill,
        stroke,
        stroke_width,
        ..
    } = &union[0].kind
    else {
        panic!("expected Path");
    };
    assert_eq!(*fill, [30, 120, 220, 255]);
    assert_eq!(*stroke, [10, 20, 30, 255]);
    assert!((*stroke_width - 6.0).abs() < 1e-6);
    let local_bounds = crate::path_bounds(commands).unwrap().unwrap();
    assert!(local_bounds.x_min.abs() < 1e-6 && local_bounds.y_min.abs() < 1e-6);
    assert!((local_bounds.width() - 150.0).abs() < 1e-6);
    assert!((local_bounds.height() - 100.0).abs() < 1e-6);

    // 2. Intersect and Crop produce the (250..300, 170..230) overlap with local (0, 0) origin.
    for crop_by_top in [false, true] {
        let isect = combine_elements_boolean(
            &[base.clone(), cutter.clone()],
            BooleanOp::Intersection,
            crop_by_top,
        )
        .unwrap();
        assert_eq!(isect.len(), 1);
        assert!((isect[0].transform.x - 250.0).abs() < 1e-6);
        assert!((isect[0].transform.y - 170.0).abs() < 1e-6);
        let (world_cmds, rule) = element_world_commands(&isect[0]).unwrap();
        let rings = flatten_closed_rings(&world_cmds);
        assert!(point_in_rings([275.0, 200.0], &rings, rule));
        assert!(!point_in_rings([220.0, 160.0], &rings, rule));
    }

    // 3. Punch (Difference) with an interior cutter creates a donut with a hole.
    let mut hole_cutter = rect_element(
        "hole",
        230.0,
        170.0,
        40.0,
        40.0,
        [255, 255, 255, 255],
        [0; 4],
        0.0,
    );
    hole_cutter.merge = false;
    let punched =
        combine_elements_boolean(&[base.clone(), hole_cutter], BooleanOp::Difference, false)
            .unwrap();
    assert_eq!(punched.len(), 1);
    let (punched_world, punched_rule) = element_world_commands(&punched[0]).unwrap();
    let punched_rings = flatten_closed_rings(&punched_world);
    assert_eq!(punched_rings.len(), 2, "outer shell + inner hole");
    assert!(point_in_rings([210.0, 160.0], &punched_rings, punched_rule));
    assert!(!point_in_rings(
        [250.0, 190.0],
        &punched_rings,
        punched_rule
    ));

    // 4. Object Drawing PaintedPath with fill AND stroke gradients preserves both in local coords.
    let grad = GradientPaint {
        kind: GradientKind::Linear,
        matrix: [0.06103515625, 0.0, 0.0, 0.06103515625, 50.0, 40.0],
        stops: vec![
            GradientStop {
                ratio: 0,
                color: [10, 80, 240, 255],
            },
            GradientStop {
                ratio: 255,
                color: [240, 60, 20, 255],
            },
        ],
        focal_point_ratio: 0.0,
        ..Default::default()
    };
    let stroke_grad = GradientPaint {
        kind: GradientKind::Radial,
        ..grad.clone()
    };
    let painted_obj = Element {
        id: "painted-obj".into(),
        transform: Transform {
            x: 200.0,
            y: 150.0,
            ..Transform::default()
        },
        kind: ElementKind::PaintedPath {
            commands: ring_to_commands(&[[0.0, 0.0], [100.0, 0.0], [100.0, 80.0], [0.0, 80.0]]),
            fill_rule: FillRule::NonZero,
            fill: [10, 80, 240, 255],
            stroke: [20, 30, 40, 255],
            fill_gradient: Some(grad.clone()),
            stroke_gradient: Some(stroke_grad.clone()),
            stroke_width: 4.0,
        },
        merge: false,
        ..Element::default()
    };
    let combined_painted =
        combine_elements_boolean(&[painted_obj, cutter], BooleanOp::Union, false).unwrap();
    assert_eq!(combined_painted.len(), 1);
    let ElementKind::PaintedPath {
        fill_gradient: Some(fg),
        stroke_gradient: Some(sg),
        stroke_width,
        stroke,
        ..
    } = &combined_painted[0].kind
    else {
        panic!("expected PaintedPath with both fill and stroke gradients");
    };
    assert_eq!(*stroke, [20, 30, 40, 255]);
    assert!((*stroke_width - 4.0).abs() < 1e-6);
    assert_eq!(fg.sample(25.0, 20.0), grad.sample(25.0, 20.0));
    assert_eq!(sg.sample(25.0, 20.0), stroke_grad.sample(25.0, 20.0));

    // 5. Merge Drawing selection containing fills + separate stroke outlines + open lines combines cleanly.
    let mut merge_elements = Vec::new();
    apply_merge_shape_to_keyframe(
        &mut merge_elements,
        rect_element(
            "m1",
            0.0,
            0.0,
            80.0,
            80.0,
            [40, 160, 90, 255],
            [0, 0, 0, 255],
            2.0,
        ),
    );
    apply_merge_shape_to_keyframe(
        &mut merge_elements,
        rect_element(
            "m2",
            40.0,
            0.0,
            80.0,
            80.0,
            [200, 80, 40, 255],
            [0, 0, 0, 255],
            2.0,
        ),
    );
    let merge_combined =
        combine_elements_boolean(&merge_elements, BooleanOp::Union, false).unwrap();
    assert_eq!(
        merge_combined.iter().filter(|e| is_merge_fill(e)).count(),
        1
    );
    assert_eq!(
        merge_combined.iter().filter(|e| is_merge_stroke(e)).count(),
        1
    );
}

/// Hatching blob of overlapping capsules (exterior plus many holes), built by repeated unions.
fn hatch_rings(count: usize, seed: f64) -> Vec<Vec<[f64; 2]>> {
    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
    for i in 0..count {
        let t = i as f64 + seed;
        let horizontal = i % 2 == 0;
        let k = (i / 2) as f64;
        let (cx, cy) = if horizontal {
            (60.0 + (t * 1.7).sin() * 4.0, 8.0 + k * 7.0)
        } else {
            (8.0 + k * 7.0, 60.0 + (t * 1.3).cos() * 4.0)
        };
        let (hw, hh) = if horizontal { (55.0, 1.6) } else { (1.6, 55.0) };
        let capsule: Vec<[f64; 2]> = (0..24)
            .map(|n| {
                let ang = n as f64 * std::f64::consts::TAU / 24.0;
                [cx + ang.cos() * hw, cy + ang.sin() * hh]
            })
            .collect();
        let next = boolean_rings(
            &rings,
            FillRule::NonZero,
            &[capsule],
            FillRule::NonZero,
            BooleanOp::Union,
        );
        rings = next.iter().flat_map(|c| flatten_closed_rings(c)).collect();
    }
    rings
}

#[test]
fn windowed_boolean_matches_point_coverage() {
    let a = hatch_rings(30, 0.0);
    let total: usize = a.iter().map(Vec::len).sum();
    assert!(
        a.len() > 20 && total > 300,
        "{} rings / {total} edges",
        a.len()
    );
    let blob = |cx: f64, cy: f64, r: f64| -> Vec<Vec<[f64; 2]>> {
        vec![
            (0..20)
                .map(|n| {
                    let ang = n as f64 * std::f64::consts::TAU / 20.0;
                    [cx + ang.cos() * r * 1.7, cy + ang.sin() * r]
                })
                .collect(),
        ]
    };
    for (case, b) in [
        blob(40.0, 40.0, 6.0),
        blob(12.0, 90.0, 9.0),
        blob(95.0, 15.0, 3.0),
        blob(60.0, 60.0, 30.0),
        blob(150.0, 150.0, 5.0),
    ]
    .into_iter()
    .enumerate()
    {
        for op in [BooleanOp::Union, BooleanOp::Difference] {
            let (out, _) = boolean_rings_windowed(&a, &b, FillRule::NonZero, op, 0)
                .unwrap_or_else(|| panic!("case {case} {op:?}: windowed path declined"));
            let out_rings: Vec<Vec<[f64; 2]>> =
                out.iter().flat_map(|c| flatten_closed_rings(c)).collect();
            let near_boundary = |p: [f64; 2]| {
                a.iter().chain(b.iter()).any(|r| {
                    (0..r.len()).any(|i| {
                        point_to_segment_projection(p, r[i], r[(i + 1) % r.len()]).1 < 0.05
                    })
                })
            };
            let mut checked = 0;
            for gx in 0..130 {
                for gy in 0..130 {
                    let p = [gx as f64 + 0.37, gy as f64 + 0.61];
                    if near_boundary(p) {
                        continue;
                    }
                    let want = op.eval(
                        point_in_rings(p, &a, FillRule::NonZero),
                        point_in_rings(p, &b, FillRule::NonZero),
                    );
                    let got = point_in_rings(p, &out_rings, FillRule::NonZero);
                    assert_eq!(got, want, "case {case} {op:?} at {p:?}");
                    checked += 1;
                }
            }
            assert!(checked > 10_000);
        }
    }
}

#[test]
fn dense_brush_stroke_and_move_complete_without_quadratic_stall() {
    // Build a 568-dab variable-width Brush stroke (1,136 overlapping disk + quad contours =
    // 9,651 commands, matching the T-090 tablet repro `object-458`) on a dense keyframe of
    // 275 Merge elements.
    let mut elements = Vec::with_capacity(280);
    for idx in 0..274 {
        let col = (idx % 20) as f64;
        let row = (idx / 20) as f64;
        let color = if idx % 3 == 0 {
            [212, 162, 249, 255]
        } else if idx % 3 == 1 {
            [80, 170, 230, 255]
        } else {
            [240, 130, 90, 255]
        };
        elements.push(Element {
            id: format!("existing-{idx}"),
            name: format!("existing-{idx}"),
            transform: Transform::default(),
            kind: ElementKind::Path {
                commands: ring_to_commands(&[
                    [col * 25.0, row * 25.0],
                    [col * 25.0 + 18.0, row * 25.0],
                    [col * 25.0 + 18.0, row * 25.0 + 18.0],
                    [col * 25.0, row * 25.0 + 18.0],
                ]),
                fill_rule: FillRule::NonZero,
                fill: color,
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            merge: true,
            ..Element::default()
        });
    }

    let make_heavy_brush_element = |id: &str, offset_x: f64, offset_y: f64| -> Element {
        let n_points = 568usize;
        let mut commands = Vec::with_capacity(n_points * 23);
        let mut prev = [0.0f64, 0.0f64];
        let mut prev_r = 6.0f64;
        for i in 0..n_points {
            let t = i as f64 * 0.06;
            let pt = [
                offset_x + i as f64 * 0.65 + (t * 0.7).sin() * 12.0,
                offset_y + (t * 0.35).cos() * 45.0 + 120.0,
            ];
            let r = 4.0 + (t * 0.5).sin().abs() * 5.0;
            let disk: Vec<[f64; 2]> = (0..16)
                .map(|k| {
                    let ang = k as f64 * std::f64::consts::TAU / 16.0;
                    [pt[0] + ang.cos() * r, pt[1] + ang.sin() * r]
                })
                .collect();
            commands.extend(ring_to_commands(&disk));
            if i > 0 {
                let dx = pt[0] - prev[0];
                let dy = pt[1] - prev[1];
                let len = (dx * dx + dy * dy).sqrt();
                if len > 1e-6 {
                    let nx = -dy / len;
                    let ny = dx / len;
                    commands.extend(ring_to_commands(&[
                        [prev[0] - nx * prev_r, prev[1] - ny * prev_r],
                        [pt[0] - nx * r, pt[1] - ny * r],
                        [pt[0] + nx * r, pt[1] + ny * r],
                        [prev[0] + nx * prev_r, prev[1] + ny * prev_r],
                    ]));
                }
            }
            prev = pt;
            prev_r = r;
        }
        Element {
            id: id.into(),
            name: id.into(),
            transform: Transform::default(),
            kind: ElementKind::Path {
                commands,
                fill_rule: FillRule::NonZero,
                fill: [212, 162, 249, 255],
                stroke: [212, 162, 249, 255],
                stroke_width: 0.0,
            },
            merge: true,
            ..Element::default()
        }
    };

    // Time ceilings apply to optimized builds; unoptimized test runs check behaviour only.
    let brush_el = make_heavy_brush_element("heavy-brush-1", 20.0, 20.0);
    let raw_contours =
        split_commands_into_contours(&element_world_commands(&brush_el).unwrap().0).len();
    assert_eq!(raw_contours, 1135);

    let t0 = std::time::Instant::now();
    let created = apply_merge_shape_to_keyframe(&mut elements, brush_el);
    let draw_elapsed = t0.elapsed();
    assert_eq!(created.len(), 1);
    let merged_el = elements.iter().find(|e| e.id == created[0]).unwrap();
    let merged_contours =
        split_commands_into_contours(&element_world_commands(merged_el).unwrap().0).len();
    assert!(
        merged_contours < 30,
        "overlapping brush dabs must dissolve into outer boundary + holes, got {merged_contours} contours"
    );

    // Move the resulting merged brush artwork across the dense keyframe and re-merge.
    for el in &mut elements {
        if el.id == created[0] {
            el.transform.x = 18.0;
            el.transform.y = -14.0;
        }
    }
    let t1 = std::time::Instant::now();
    let moved_ids = remerge_moved_elements_in_keyframe(&mut elements, &created);
    let move_elapsed = t1.elapsed();
    assert!(!moved_ids.is_empty());
    assert!(
        cfg!(debug_assertions)
            || (draw_elapsed.as_millis() < 1500 && move_elapsed.as_millis() < 1500),
        "expected fast draw ({draw_elapsed:?}) and move ({move_elapsed:?})"
    );

    let repro_json_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/t090-rapid-brush-hang/rapid-brush-after-stall.nflash");
    if let Ok(raw_json) = std::fs::read_to_string(&repro_json_path) {
        let project: crate::model::Project = serde_json::from_str(&raw_json).unwrap();
        let kf_elements = &project.symbols[0].timeline.layers[0]
            .keyframes
            .iter()
            .find(|kf| kf.frame == 55)
            .unwrap()
            .elements;
        assert_eq!(kf_elements.len(), 275);

        // 1. Test moving `object-458` (the 1,136-contour rapid brush stroke) on keyframe 55.
        let mut moved_kf = kf_elements.clone();
        for el in &mut moved_kf {
            if el.id == "object-458" {
                el.transform.x = 24.0;
                el.transform.y = -16.0;
            }
        }
        let t_repro_move = std::time::Instant::now();
        let repro_moved_ids =
            remerge_moved_elements_in_keyframe(&mut moved_kf, &["object-458".to_string()]);
        let repro_move_elapsed = t_repro_move.elapsed();
        assert!(!repro_moved_ids.is_empty());
        assert!(
            cfg!(debug_assertions) || (repro_move_elapsed.as_millis() < 1500),
            "expected fast repro move, got {repro_move_elapsed:?}"
        );

        // 2. Subsequent move of the now-dissolved stroke is even faster.
        for el in &mut moved_kf {
            if repro_moved_ids.contains(&el.id) {
                el.transform.x = -12.0;
                el.transform.y = 10.0;
            }
        }
        let t_repro_move2 = std::time::Instant::now();
        let repro_moved_ids2 = remerge_moved_elements_in_keyframe(&mut moved_kf, &repro_moved_ids);
        let repro_move2_elapsed = t_repro_move2.elapsed();
        assert!(!repro_moved_ids2.is_empty());
        assert!(
            cfg!(debug_assertions) || (repro_move2_elapsed.as_millis() < 500),
            "expected sub-500ms second repro move, got {repro_move2_elapsed:?}"
        );
    }
}

#[test]
fn pinch_vertex_and_kissing_hole_never_drop_hole_or_flood_fill() {
    // Outer shell [0..200, 0..200] with an interior triangular hole [20,100] -> [100,180] -> [100,20]
    // whose vertices kiss a second hole and a figure-8 pinch point at (100, 100).
    let vertices = vec![
        [0.0, 0.0],     // 0
        [200.0, 0.0],   // 1
        [200.0, 200.0], // 2
        [0.0, 200.0],   // 3
        [100.0, 100.0], // 4: shared pinch vertex
        [40.0, 160.0],  // 5
        [40.0, 40.0],   // 6
        [160.0, 40.0],  // 7
        [160.0, 160.0], // 8
    ];
    // Outer shell (0->1->2->3->0, area > 0) + two negative-area holes kissing at vertex 4
    // (4->6->5->4 and 4->8->7->4, area < 0).
    let half_edges = vec![
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 6),
        (6, 5),
        (5, 4),
        (4, 8),
        (8, 7),
        (7, 4),
    ];
    let cycles = trace_half_edge_cycles(&vertices, &half_edges);
    assert_eq!(
        cycles.len(),
        3,
        "both kissing holes and the outer shell must close without dropping any cycle"
    );
    let rings: Vec<Vec<[f64; 2]>> = cycles
        .into_iter()
        .map(|c| c.into_iter().map(|he| vertices[half_edges[he].0]).collect())
        .collect();
    let islands = assemble_shells_and_holes(rings);
    assert_eq!(islands.len(), 1);
    let move_count = islands[0]
        .iter()
        .filter(|cmd| matches!(cmd, PathCommand::MoveTo { .. }))
        .count();
    assert_eq!(
        move_count, 3,
        "assembled compound path must retain outer shell and both interior holes"
    );
}
