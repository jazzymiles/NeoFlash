//! Original synthetic geometry; no corpus artwork, strings or filenames.
use neoflash_core::*;
use neoflash_import::*;
type Point = [i32; 2];
#[derive(Clone, Copy)]
struct Edge {
    a: Point,
    c: Option<Point>,
    b: Point,
    side0: u16,
    side1: u16,
}
fn p(x: i32, y: i32) -> Point {
    [x * 5120, y * 5120]
}
fn line(a: Point, b: Point) -> Edge {
    Edge {
        a,
        c: None,
        b,
        side0: 0,
        side1: 1,
    }
}
fn quad(a: Point, c: Point, b: Point) -> Edge {
    Edge {
        c: Some(c),
        ..line(a, b)
    }
}
fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<Edge> {
    let v = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
    (0..4).map(|i| line(v[i], v[(i + 1) % 4])).collect()
}
fn reverse(edges: &mut [Edge]) {
    for e in edges {
        std::mem::swap(&mut e.a, &mut e.b);
    }
}
fn encoded(edges: &[Edge], matrix: [i32; 6]) -> Vec<u8> {
    let mut b = vec![2];
    for word in matrix {
        b.extend(word.to_le_bytes());
    }
    b.push(4);
    b.extend(0u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend([31, 97, 211, 255, 0, 0]);
    b.extend(0u16.to_le_bytes());
    let mut previous = [0; 2];
    for e in edges {
        b.push(if e.c.is_some() { 0x6a } else { 0x62 });
        for index in [0, e.side0, e.side1] {
            b.extend(index.to_le_bytes());
        }
        for v in [e.a[0] - previous[0], e.a[1] - previous[1]] {
            b.extend(v.to_le_bytes());
        }
        if let Some(c) = e.c {
            for v in [c[0] - e.a[0], c[1] - e.a[1]] {
                b.extend(v.to_le_bytes());
            }
        }
        for v in [e.b[0] - e.a[0], e.b[1] - e.a[1]] {
            b.extend(v.to_le_bytes());
        }
        previous = e.b;
    }
    b.push(0);
    b
}
const IDENTITY: [i32; 6] = [65536, 0, 0, 65536, 0, 0];
fn shape(edges: &[Edge]) -> BinaryShapeRecord {
    decode_binary_shape(&encoded(edges, IDENTITY)).unwrap()
}
fn adapt(shape: &BinaryShapeRecord) -> Result<BinarySolidPath, BinaryContourError> {
    adapt_binary_solid_shape(
        shape,
        BinaryContourContext {
            contents_revision: 0x14,
        },
    )
}
fn commands(result: &BinarySolidPath) -> &[PathCommand] {
    let ElementKind::Path { commands, .. } = &result.path else {
        panic!("not path")
    };
    commands
}
fn hit(result: &BinarySolidPath, point: [f64; 2]) -> bool {
    let ElementKind::Path {
        commands,
        fill_rule,
        ..
    } = &result.path
    else {
        panic!()
    };
    hit_test_path(
        commands,
        *fill_rule,
        point,
        true,
        0.,
        FlattenOptions {
            tolerance: 0.0001,
            ..Default::default()
        },
    )
    .unwrap()
}
fn code(shape: &BinaryShapeRecord) -> String {
    adapt(shape).unwrap_err().code
}

#[test]
fn rectangle_has_exact_units_color_closed_path_and_deterministic_edge_order() {
    let mut edges = rect(-3, -2, 11, 7);
    let first = adapt(&shape(&edges)).unwrap();
    assert_eq!(
        commands(&first),
        &[
            PathCommand::MoveTo { x: -3., y: -2. },
            PathCommand::LineTo { x: 11., y: -2. },
            PathCommand::LineTo { x: 11., y: 7. },
            PathCommand::LineTo { x: -3., y: 7. },
            PathCommand::LineTo { x: -3., y: -2. },
            PathCommand::Close,
        ]
    );
    let ElementKind::Path {
        fill,
        stroke,
        stroke_width,
        fill_rule,
        ..
    } = first.path
    else {
        panic!()
    };
    assert_eq!(fill, [31, 97, 211, 255]);
    assert_eq!(stroke, [0; 4]);
    assert_eq!(stroke_width, 0.);
    assert_eq!(fill_rule, FillRule::EvenOdd);
    edges.swap(0, 2);
    edges.swap(1, 3);
    let shuffled = adapt(&shape(&edges)).unwrap();
    assert_eq!(first.path, shuffled.path);
    assert!(hit(&shuffled, [0., 0.]));
    assert!(!hit(&shuffled, [20., 0.]));
    assert_eq!(shuffled.contours[0].edges.len(), 4);
}

#[test]
fn quadratic_reversal_preserves_control_and_combines_both_fill_sides() {
    let mut edges = vec![
        quad(p(0, 0), p(5, -6), p(10, 0)),
        quad(p(10, 0), p(5, 6), p(0, 0)),
    ];
    let first = adapt(&shape(&edges)).unwrap();
    reverse(&mut edges[..1]);
    edges[0].side0 = 0x8001;
    edges[0].side1 = 0;
    let flipped = adapt(&shape(&edges)).unwrap();
    assert_eq!(first.path, flipped.path);
    assert_eq!(flipped.selected_edges, vec![0]);
    assert!(flipped.contours[0].edges[0].reversed);
    assert!(hit(&flipped, [5., 0.]));
    assert!(!hit(&flipped, [5., 4.]));
    let bounds = path_bounds(commands(&flipped)).unwrap().unwrap();
    assert_eq!((bounds.y_min, bounds.y_max), (-3., 3.));
}

#[test]
fn nested_opposite_orientation_holes_and_islands_are_certified_but_same_orientation_is_blocked() {
    let mut all = rect(0, 0, 20, 20);
    let mut inner = rect(4, 4, 16, 16);
    reverse(&mut inner);
    all.extend(inner);
    all.extend(rect(8, 8, 12, 12));
    let converted = adapt(&shape(&all)).unwrap();
    assert_eq!(converted.contours.len(), 3);
    assert!(hit(&converted, [2., 2.]));
    assert!(!hit(&converted, [6., 6.]));
    assert!(hit(&converted, [10., 10.]));
    let mut same = rect(0, 0, 20, 20);
    same.extend(rect(4, 4, 16, 16));
    assert_eq!(code(&shape(&same)), "fla_contour_winding_unsupported");
    // A disjoint component may have the other orientation; this is no hole.
    let mut disjoint = rect(0, 0, 4, 4);
    let mut second = rect(8, 0, 12, 4);
    reverse(&mut second);
    disjoint.extend(second);
    let converted = adapt(&shape(&disjoint)).unwrap();
    assert!(hit(&converted, [2., 2.]));
    assert!(hit(&converted, [10., 2.]));
}

#[test]
fn curved_nesting_touching_and_near_coincident_boundaries_have_explicit_certificates() {
    let mut outer = vec![
        quad(p(0, 0), p(20, -20), p(40, 0)),
        quad(p(40, 0), p(20, 20), p(0, 0)),
    ];
    let mut hole = rect(16, -2, 24, 2);
    reverse(&mut hole);
    outer.extend(hole);
    let result = adapt(&shape(&outer)).unwrap();
    assert!(hit(&result, [10., 0.]));
    assert!(!hit(&result, [20., 0.]));

    let mut tangent = vec![
        quad(p(0, 0), p(5, -6), p(10, 0)),
        quad(p(10, 0), p(5, 6), p(0, 0)),
    ];
    tangent.extend(rect(4, 3, 6, 5)); // Touches the quadratic at its interior extremum.
    assert_eq!(code(&shape(&tangent)), "fla_contour_intersection");
    let mut crossing = vec![
        quad(p(0, 0), p(5, -6), p(10, 0)),
        quad(p(10, 0), p(5, 6), p(0, 0)),
    ];
    crossing.extend(rect(4, 2, 6, 5));
    assert!(code(&shape(&crossing)).starts_with("fla_contour_intersection"));

    let near = [
        quad(p(0, 0), [25600, 30000], p(10, 0)),
        quad(p(10, 0), [25600, 30001], p(0, 0)),
    ];
    assert_eq!(code(&shape(&near)), "fla_contour_intersection_uncertain");
}

#[test]
fn same_fill_interior_edges_are_removed_with_provenance_and_boundary_duplicates_are_not() {
    let mut edges = vec![
        line(p(0, 0), p(5, 0)),
        line(p(5, 0), p(10, 0)),
        line(p(10, 0), p(10, 10)),
        line(p(10, 10), p(5, 10)),
        line(p(5, 10), p(0, 10)),
        line(p(0, 10), p(0, 0)),
    ];
    edges.push(Edge {
        side0: 1,
        side1: 1,
        ..line(p(5, 0), p(5, 10))
    });
    edges.push(Edge {
        side0: 1,
        side1: 1,
        ..line(p(5, 10), p(5, 0))
    });
    let result = adapt(&shape(&edges)).unwrap();
    assert_eq!(result.internal_edges, vec![6, 7]);
    assert_eq!(result.contours.len(), 1);
    assert!(hit(&result, [5., 5.]));
    edges.push(edges[0]);
    assert_eq!(code(&shape(&edges)), "fla_contour_branch");
}

#[test]
fn affine_translation_reflection_shear_rotation_and_fractional_edges_are_baked_without_trs_approximation()
 {
    let edges = rect(0, 0, 2, 3);
    for (matrix, expected) in [
        ([65536, 0, 0, 65536, 40, -20], [2., -1., 4., 2.]),
        ([-65536, 0, 0, 65536, 200, 0], [8., 0., 10., 3.]),
        ([65536, 0, 32768, 65536, 0, 0], [0., 0., 3.5, 3.]),
        ([0, 65536, -65536, 0, 0, 0], [-3., 0., 0., 2.]),
    ] {
        let record = decode_binary_shape(&encoded(&edges, matrix)).unwrap();
        let result = adapt(&record).unwrap();
        let bounds = path_bounds(commands(&result)).unwrap().unwrap();
        assert_eq!(
            [bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max],
            expected
        );
    }
    let v = [[-128, -256], [256, -256], [256, 512], [-128, 512]];
    let edges = (0..4)
        .map(|i| line(v[i], v[(i + 1) % 4]))
        .collect::<Vec<_>>();
    let result = adapt(&shape(&edges)).unwrap();
    let PathCommand::MoveTo { x, y } = commands(&result)[0] else {
        panic!()
    };
    assert_eq!([x, y], [-0.025, -0.05]);
    let mut record = shape(&rect(0, 0, 2, 2));
    record.matrix_words = [65536, 0, 65536, 0, 0, 0];
    assert_eq!(code(&record), "fla_contour_transform_unsupported");
    let tiny = [
        line([0, 0], [1, 0]),
        line([1, 0], [1, 1]),
        line([1, 1], [0, 1]),
        line([0, 1], [0, 0]),
    ];
    let record = decode_binary_shape(&encoded(&tiny, [1, 0, 0, 1, i32::MAX, i32::MAX])).unwrap();
    assert_eq!(code(&record), "fla_contour_precision_unsupported");
}

#[test]
fn open_branch_intersecting_touching_retracing_and_degenerate_graphs_are_rejected() {
    assert_eq!(code(&shape(&rect(0, 0, 5, 5)[..3])), "fla_contour_open");
    let mut branch = rect(0, 0, 5, 5);
    branch.push(line(p(0, 0), p(3, 3)));
    assert_eq!(code(&shape(&branch)), "fla_contour_branch");
    let bowtie = [
        line(p(0, 0), p(10, 10)),
        line(p(10, 10), p(0, 10)),
        line(p(0, 10), p(10, 0)),
        line(p(10, 0), p(0, 0)),
    ];
    assert_eq!(code(&shape(&bowtie)), "fla_contour_intersection");
    let mut touch = rect(0, 0, 10, 10);
    touch.extend(rect(10, 2, 14, 6));
    assert_eq!(code(&shape(&touch)), "fla_contour_intersection");
    assert_eq!(
        code(&shape(&[line(p(0, 0), p(0, 0))])),
        "fla_contour_degenerate"
    );
    let retrace = [quad(p(0, 0), p(20, 0), p(10, 0)), line(p(10, 0), p(0, 0))];
    assert_eq!(code(&shape(&retrace)), "fla_contour_degenerate");
    let overlapping = [line(p(0, 0), p(10, 0)), line(p(10, 0), p(0, 0))];
    assert_eq!(code(&shape(&overlapping)), "fla_contour_intersection");
}

#[test]
fn unsupported_paints_metadata_and_inconsistent_deserialized_ir_are_never_silently_accepted() {
    let base = shape(&rect(0, 0, 4, 4));
    let mut changed = base.clone();
    changed.strokes.push(BinaryStroke {
        color_word: 0xff000000,
        width_word: 20,
        style_words: [0; 2],
        extension: None,
    });
    assert_eq!(code(&changed), "fla_contour_feature_unsupported");
    let mut changed = base.clone();
    changed.auxiliary.push(BinaryAuxiliaryCurve {
        point_words: [0; 8],
        entries: None,
        boundary_flags: None,
        boundary_start: None,
        boundary_end: None,
    });
    assert_eq!(code(&changed), "fla_contour_feature_unsupported");
    let mut changed = base.clone();
    changed.fills.push(changed.fills[0].clone());
    assert_eq!(code(&changed), "fla_contour_paint_unsupported");
    for alpha in [0, 127, 254] {
        let mut changed = base.clone();
        changed.fills[0].packed_value = (changed.fills[0].packed_value & 0xffffff) | (alpha << 24);
        assert_eq!(code(&changed), "fla_contour_paint_unsupported");
    }
    let mut changed = base.clone();
    changed.fills[0].encoding = BinaryFillEncoding::Extended {
        flags: 0x10,
        secondary_flags: 0,
        payload: BinaryFillPayload::Gradient {
            matrix_words: IDENTITY,
            extension: None,
            stops: vec![],
        },
    };
    assert_eq!(code(&changed), "fla_contour_paint_unsupported");
    let mut changed = base.clone();
    changed.edges[1].start[0] += 1;
    assert_eq!(code(&changed), "fla_contour_inconsistent_ir");
    let mut changed = base.clone();
    changed.edges[0].encoding_flags = 0x40; // Declares absent start/end deltas.
    assert_eq!(code(&changed), "fla_contour_inconsistent_ir");
    let mut changed = base.clone();
    changed.edges[0].line_flags = Some(99);
    assert_eq!(code(&changed), "fla_contour_inconsistent_ir");
    let mut changed = base.clone();
    changed.data_revision = 5;
    assert_eq!(code(&changed), "fla_contour_revision_unsupported");
    assert_eq!(
        adapt_binary_solid_shape(
            &base,
            BinaryContourContext {
                contents_revision: 0x20
            }
        )
        .unwrap_err()
        .code,
        "fla_contour_revision_unsupported"
    );
    let mut changed = base.clone();
    changed.edges[0].end_delta = [i32::MAX, 0];
    changed.edges[0].start = [1, 0];
    changed.edges[0].start_delta = [1, 0];
    changed.edges[0].end = [i32::MIN, 0];
    assert_eq!(code(&changed), "fla_contour_wrapped_coordinate");
}

#[test]
fn public_adapter_size_and_work_budgets_fail_explicitly() {
    let mut too_many = shape(&rect(0, 0, 4, 4));
    too_many.edges = vec![too_many.edges[0].clone(); MAX_BINARY_CONTOUR_EDGES + 1];
    assert_eq!(code(&too_many), "fla_contour_size_limit");
    // Many separate narrow boxes share the same X interval. Pair tests are
    // bounded even though their Y ranges quickly prove them disjoint.
    let mut edges = vec![];
    for i in 0..400 {
        edges.extend(rect(0, i * 3, 1, i * 3 + 1));
    }
    assert_eq!(code(&shape(&edges)), "fla_contour_work_limit");
}

#[test]
fn core_project_roundtrip_and_hit_testing_accept_the_original_converted_fixture() {
    let path = adapt(&shape(&rect(0, 0, 10, 10))).unwrap();
    let mut project = Project::blank();
    project.symbols[0].timeline.layers[0].keyframes[0]
        .elements
        .push(Element {
            id: new_id("contour-fixture"),
            name: "Original solid fixture".into(),
            transform: Transform::default(),
            kind: path.path,
            scripts: vec![],
            accessibility: None,
        });
    assert!(validate(&project).is_empty());
    let json = serde_json::to_string(&project).unwrap();
    let reopened = Project::from_json(&json).unwrap();
    let scene = evaluate(&reopened, &reopened.root, 0);
    assert_eq!(scene.len(), 1);
    assert_eq!(
        scene[0].kind,
        project.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind
    );
}
