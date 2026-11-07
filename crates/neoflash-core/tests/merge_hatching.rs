//! Merge Drawing under dense same-colour hatching (T-090).
//!
//! Opt-in timing runs:
//! `MERGE_HATCH_STROKES=3000 cargo test --release -p neoflash-core --test merge_hatching dense_hatching_merge_timing -- --ignored --nocapture`
//! `MERGE_MOVE_SHAPES=60000 cargo test --release -p neoflash-core --test merge_hatching transformed_merge_move_benchmark -- --ignored --nocapture`

use neoflash_core::merge_drawing::{
    apply_merge_shape_to_keyframe, element_world_commands, flatten_closed_rings, point_in_rings,
    remerge_moved_elements_in_keyframe,
};
use neoflash_core::model::{Element, ElementKind, FillRule, PathCommand, Project, Transform};

const MOVED_FILL: [u8; 4] = [34, 116, 165, 255];
const SENTINEL_FILL: [u8; 4] = [247, 92, 3, 255];

fn rectangle_element(
    id: impl Into<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    fill: [u8; 4],
    transform: Transform,
) -> Element {
    let id = id.into();
    Element {
        name: id.clone(),
        id,
        transform,
        kind: ElementKind::Path {
            commands: vec![
                PathCommand::MoveTo { x, y },
                PathCommand::LineTo { x: x + width, y },
                PathCommand::LineTo {
                    x: x + width,
                    y: y + height,
                },
                PathCommand::LineTo { x, y: y + height },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            fill,
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        merge: true,
        ..Element::default()
    }
}

struct CompoundRectangles {
    first_index: usize,
    count: usize,
    columns: usize,
    spacing: [f64; 2],
    size: [f64; 2],
}

fn compound_rectangle_element(
    id: impl Into<String>,
    rectangles: CompoundRectangles,
    fill: [u8; 4],
    transform: Transform,
) -> Element {
    let CompoundRectangles {
        first_index,
        count,
        columns,
        spacing,
        size,
    } = rectangles;
    assert!(count > 0);
    let first_x = (first_index % columns) as f64 * spacing[0];
    let first_y = (first_index / columns) as f64 * spacing[1];
    let mut element = rectangle_element(id, first_x, first_y, size[0], size[1], fill, transform);
    let ElementKind::Path { commands, .. } = &mut element.kind else {
        unreachable!()
    };
    commands.clear();
    commands.reserve(count * 5);
    for index in first_index..first_index + count {
        let x = (index % columns) as f64 * spacing[0];
        let y = (index / columns) as f64 * spacing[1];
        commands.extend([
            PathCommand::MoveTo { x, y },
            PathCommand::LineTo { x: x + size[0], y },
            PathCommand::LineTo {
                x: x + size[0],
                y: y + size[1],
            },
            PathCommand::LineTo { x, y: y + size[1] },
            PathCommand::Close,
        ]);
    }
    element
}

fn element_fill(element: &Element) -> Option<[u8; 4]> {
    match &element.kind {
        ElementKind::Path { fill, .. } | ElementKind::PaintedPath { fill, .. } => Some(*fill),
        _ => None,
    }
}

fn fill_rings_with_color(elements: &[Element], color: [u8; 4]) -> Vec<Vec<[f64; 2]>> {
    elements
        .iter()
        .filter(|element| element_fill(element) == Some(color))
        .filter_map(element_world_commands)
        .flat_map(|(commands, _)| flatten_closed_rings(&commands))
        .collect()
}

fn fills_with_color_contain(elements: &[Element], color: [u8; 4], point: [f64; 2]) -> bool {
    elements
        .iter()
        .filter(|element| element_fill(element) == Some(color))
        .filter_map(element_world_commands)
        .any(|(commands, rule)| point_in_rings(point, &flatten_closed_rings(&commands), rule))
}

fn ring_area(ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    (0..ring.len())
        .map(|index| {
            let a = ring[index];
            let b = ring[(index + 1) % ring.len()];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}

fn assert_valid_keyframe(elements: &[Element]) {
    let mut project = Project::blank();
    project.symbols[0].timeline.layers[0].keyframes[0].elements = elements.to_vec();
    project.upgrade_format_for_features();
    project.validate().expect("remerged keyframe must validate");
}

/// Round-capped variable-width stroke outline (one contour, like the Brush envelope).
fn stroke_outline(i: usize) -> Vec<[f64; 2]> {
    let patch = i / 60;
    let k = i % 60;
    let (px, py) = ((patch % 12) as f64 * 120.0, (patch / 12) as f64 * 110.0);
    let jitter = ((i * 7919) % 13) as f64 * 0.7;
    let (x0, y0, dx, dy, len): (f64, f64, f64, f64, f64) = if k < 30 {
        (
            px + k as f64 * 5.0 + jitter,
            py,
            0.3,
            1.0,
            120.0 + jitter * 3.0,
        )
    } else {
        (
            px,
            py + (k - 30) as f64 * 5.0 + jitter,
            1.0,
            0.2,
            130.0 + jitter * 3.0,
        )
    };
    let norm: f64 = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / norm, dy / norm);
    let steps = (len / 6.0) as usize;
    let mut spine = Vec::with_capacity(steps + 1);
    for s in 0..=steps {
        let d = s as f64 * len / steps as f64;
        let wob = (d * 0.05 + i as f64).sin() * 0.9;
        let r = 1.0 + 2.0 * (std::f64::consts::PI * s as f64 / steps as f64).sin();
        spine.push(([x0 + ux * d - uy * wob, y0 + uy * d + ux * wob], r));
    }
    let (nx, ny) = (-uy, ux);
    let mut ring: Vec<[f64; 2]> = spine
        .iter()
        .map(|&(p, r)| [p[0] + nx * r, p[1] + ny * r])
        .collect();
    let cap = |ring: &mut Vec<[f64; 2]>, (c, r): ([f64; 2], f64), from: f64| {
        for n in 1..8 {
            let ang = from + std::f64::consts::PI * n as f64 / 8.0;
            let (ca, sa) = (ang.cos(), ang.sin());
            // Rotate the cap's unit vector from the stroke frame into world space.
            ring.push([
                c[0] + (nx * ca + ux * sa) * r,
                c[1] + (ny * ca + uy * sa) * r,
            ]);
        }
    };
    cap(&mut ring, *spine.last().unwrap(), 0.0);
    ring.extend(
        spine
            .iter()
            .rev()
            .map(|&(p, r)| [p[0] - nx * r, p[1] - ny * r]),
    );
    cap(&mut ring, spine[0], std::f64::consts::PI);
    ring
}

fn stroke_element(i: usize, ring: &[[f64; 2]]) -> Element {
    let mut commands = vec![PathCommand::MoveTo {
        x: ring[0][0],
        y: ring[0][1],
    }];
    commands.extend(
        ring[1..]
            .iter()
            .map(|p| PathCommand::LineTo { x: p[0], y: p[1] }),
    );
    commands.push(PathCommand::Close);
    Element {
        id: format!("stroke-{i}"),
        name: format!("stroke-{i}"),
        transform: Transform::default(),
        kind: ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [20, 20, 20, 255],
            stroke: [20, 20, 20, 255],
            stroke_width: 0.0,
        },
        merge: true,
        ..Element::default()
    }
}

fn fill_rings(elements: &[Element]) -> Vec<Vec<[f64; 2]>> {
    elements
        .iter()
        .filter_map(element_world_commands)
        .flat_map(|(commands, _)| flatten_closed_rings(&commands))
        .collect()
}

/// Cross-hatched patches merge into large same-colour blobs with many holes. Whatever path the
/// Merge takes (local window or full rebuild), the painted coverage must equal the union of
/// every stroke drawn.
#[test]
fn dense_hatching_merge_keeps_exact_coverage() {
    let count = 150;
    let mut elements = Vec::new();
    let mut strokes = Vec::new();
    for i in 0..count {
        let ring = stroke_outline(i);
        apply_merge_shape_to_keyframe(&mut elements, stroke_element(i, &ring));
        strokes.push(ring);
    }
    let merged = fill_rings(&elements);
    let total_edges: usize = merged.iter().map(Vec::len).sum();
    assert!(
        merged.len() > 50,
        "hatching should leave many holes, got {} rings",
        merged.len()
    );
    assert!(total_edges > 1000, "{total_edges} edges");

    // Gaps narrower than the snapping tolerance close by design (both Merge paths).
    let near_edge = |p: [f64; 2], rings: &[Vec<[f64; 2]>]| {
        rings.iter().any(|r| {
            (0..r.len()).any(|k| {
                let (a, b) = (r[k], r[(k + 1) % r.len()]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len_sq = dx * dx + dy * dy;
                let t = if len_sq > 0.0 {
                    (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (qx, qy) = (a[0] + dx * t - p[0], a[1] + dy * t - p[1]);
                qx * qx + qy * qy < 0.15 * 0.15
            })
        })
    };
    let mut checked = 0;
    for gx in 0..150 {
        for gy in 0..90 {
            let p = [gx as f64 * 1.37 + 0.11, gy as f64 * 1.73 + 0.29];
            if near_edge(p, &strokes) {
                continue;
            }
            let want = strokes
                .iter()
                .any(|s| point_in_rings(p, std::slice::from_ref(s), FillRule::NonZero));
            let got = point_in_rings(p, &merged, FillRule::NonZero);
            assert_eq!(got, want, "coverage differs at {p:?}");
            checked += 1;
        }
    }
    assert!(checked > 10_000);
}

/// A real Stage move leaves normalized Merge parts carrying a non-identity transform until
/// `remerge_moved_elements_in_keyframe` bakes them back into world-space paths. More than sixteen
/// same-paint parts must keep every disconnected island even when untouched artwork is interleaved
/// in the element order. The implementation may emit one compound element or several bounded
/// batches, so correctness is checked across all returned geometry rather than by output count.
#[test]
fn transformed_multi_fill_remerge_preserves_coverage_and_untouched_artwork() {
    const MOVED_COUNT: usize = 24;
    const WIDTH: f64 = 6.0;
    const HEIGHT: f64 = 4.0;
    let movement = Transform {
        x: 137.25,
        y: -53.5,
        ..Transform::default()
    };

    let mut elements = Vec::new();
    let mut moved_ids = Vec::new();
    let mut moved_centres = Vec::new();
    let mut sentinels = Vec::new();
    for index in 0..MOVED_COUNT {
        let x = (index % 8) as f64 * 12.0;
        let y = (index / 8) as f64 * 10.0;
        let id = format!("moved-{index}");
        moved_ids.push(id.clone());
        moved_centres.push([x + movement.x + WIDTH * 0.5, y + movement.y + HEIGHT * 0.5]);
        elements.push(rectangle_element(
            id, x, y, WIDTH, HEIGHT, MOVED_FILL, movement,
        ));

        if index % 4 == 1 {
            let sentinel_index = sentinels.len();
            let sentinel = rectangle_element(
                format!("sentinel-{sentinel_index}"),
                -800.0 - sentinel_index as f64 * 20.0,
                -600.0,
                8.0,
                5.0,
                SENTINEL_FILL,
                Transform::default(),
            );
            elements.push(sentinel.clone());
            sentinels.push(sentinel);
        }
    }

    let result_ids = remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    assert_eq!(
        result_ids, moved_ids,
        "every disconnected moved fill must retain its selection identity"
    );

    let unique_ids: std::collections::HashSet<&str> =
        elements.iter().map(|element| element.id.as_str()).collect();
    assert_eq!(
        unique_ids.len(),
        elements.len(),
        "element IDs must stay unique"
    );
    for id in &result_ids {
        let element = elements
            .iter()
            .find(|element| &element.id == id)
            .unwrap_or_else(|| panic!("returned moved ID {id} must exist"));
        assert_eq!(element.name, *id, "moved name changed for {id}");
    }
    for expected in &sentinels {
        let actual = elements
            .iter()
            .find(|element| element.id == expected.id)
            .unwrap_or_else(|| panic!("untouched sentinel {} was removed", expected.id));
        assert_eq!(actual, expected, "untouched sentinel changed");
        assert_eq!(element_fill(actual), Some(SENTINEL_FILL));
    }

    let moved_rings = fill_rings_with_color(&elements, MOVED_FILL);
    assert_eq!(
        moved_rings.len(),
        MOVED_COUNT,
        "every disconnected moved rectangle must remain a contour"
    );
    let moved_area: f64 = moved_rings.iter().map(|ring| ring_area(ring).abs()).sum();
    let expected_area = MOVED_COUNT as f64 * WIDTH * HEIGHT;
    assert!(
        (moved_area - expected_area).abs() < 1e-8,
        "moved area changed: expected {expected_area}, got {moved_area}"
    );
    for centre in moved_centres {
        assert!(
            point_in_rings(centre, &moved_rings, FillRule::NonZero),
            "moved fill missing at {centre:?}"
        );
    }
    for row in 0..3 {
        let gap = [
            movement.x + 9.0,
            movement.y + row as f64 * 10.0 + HEIGHT * 0.5,
        ];
        assert!(
            !point_in_rings(gap, &moved_rings, FillRule::NonZero),
            "remerge filled the gap at {gap:?}"
        );
    }
    assert_valid_keyframe(&elements);
}

/// Geometry may be batched only when the element-level compositing state is also compatible.
/// Otherwise disjoint same-paint fills would silently inherit the first element's alpha.
#[test]
fn transformed_same_paint_batch_preserves_distinct_alpha() {
    const COUNT: usize = 17;
    const SPECIAL: usize = 8;
    let mut elements = Vec::with_capacity(COUNT);
    let mut moved_ids = Vec::with_capacity(COUNT);
    let mut special_centre = [0.0; 2];
    for index in 0..COUNT {
        let x = index as f64 * 12.0;
        let id = format!("alpha-moved-{index}");
        moved_ids.push(id.clone());
        let mut element = rectangle_element(
            id,
            x,
            0.0,
            6.0,
            4.0,
            MOVED_FILL,
            Transform {
                x: 80.0,
                y: -25.0,
                ..Transform::default()
            },
        );
        if index == SPECIAL {
            element.transform.alpha = 0.35;
            special_centre = [x + 83.0, -23.0];
        }
        elements.push(element);
    }

    remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    let special = elements
        .iter()
        .find(|element| (element.transform.alpha - 0.35).abs() < 1e-12)
        .expect("the distinct alpha fill must not be swallowed by a default-alpha batch");
    let (commands, rule) = element_world_commands(special).expect("batched fill geometry");
    assert!(point_in_rings(
        special_centre,
        &flatten_closed_rings(&commands),
        rule
    ));
    assert!(
        elements
            .iter()
            .any(|element| (element.transform.alpha - 1.0).abs() < 1e-12),
        "default-alpha neighbours must remain independently composited"
    );
    assert_valid_keyframe(&elements);
}

/// Clockwise and counter-clockwise outer rings are both valid under NonZero. Concatenating them
/// into one compound batch would cancel coincident coverage even though sequential same-paint
/// Merge semantics are a union.
#[test]
fn transformed_nonzero_batch_does_not_cancel_opposite_outer_winding() {
    const COUNT: usize = 17;
    let movement = Transform {
        x: 50.0,
        y: 25.0,
        ..Transform::default()
    };
    let mut elements = Vec::with_capacity(COUNT);
    let mut moved_ids = Vec::with_capacity(COUNT);
    for index in 0..COUNT {
        let x = if index < 2 {
            0.0
        } else {
            100.0 + index as f64 * 12.0
        };
        let id = format!("winding-moved-{index}");
        moved_ids.push(id.clone());
        let mut element = rectangle_element(id, x, 0.0, 8.0, 6.0, MOVED_FILL, movement);
        if index == 1 {
            let ElementKind::Path { commands, .. } = &mut element.kind else {
                unreachable!()
            };
            *commands = vec![
                PathCommand::MoveTo { x, y: 0.0 },
                PathCommand::LineTo { x, y: 6.0 },
                PathCommand::LineTo { x: x + 8.0, y: 6.0 },
                PathCommand::LineTo { x: x + 8.0, y: 0.0 },
                PathCommand::Close,
            ];
        }
        elements.push(element);
    }

    remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    let moved_rings = fill_rings_with_color(&elements, MOVED_FILL);
    assert!(
        point_in_rings([54.0, 28.0], &moved_rings, FillRule::NonZero),
        "opposite outer winding must not cancel same-paint union coverage"
    );
    assert_valid_keyframe(&elements);
}

/// Containment does not make arbitrary negative rings safe to concatenate. Two coincident
/// negative inner contours leave a filled NonZero region (winding -1); adding a positive island
/// there must be a union, not cancellation to zero.
#[test]
fn transformed_nonzero_batch_does_not_cancel_double_negative_inner_winding() {
    const COUNT: usize = 17;
    let movement = Transform {
        x: 50.0,
        y: -25.0,
        ..Transform::default()
    };
    let mut compound = rectangle_element(
        "negative-stack",
        0.0,
        0.0,
        100.0,
        100.0,
        MOVED_FILL,
        movement,
    );
    let ElementKind::Path { commands, .. } = &mut compound.kind else {
        unreachable!()
    };
    let negative_inner = [
        PathCommand::MoveTo { x: 20.0, y: 20.0 },
        PathCommand::LineTo { x: 20.0, y: 80.0 },
        PathCommand::LineTo { x: 80.0, y: 80.0 },
        PathCommand::LineTo { x: 80.0, y: 20.0 },
        PathCommand::Close,
    ];
    commands.extend(negative_inner);
    commands.extend(negative_inner);

    let mut elements = vec![compound];
    let mut moved_ids = vec!["negative-stack".to_string()];
    let island_id = "positive-inner-island".to_string();
    moved_ids.push(island_id.clone());
    elements.push(rectangle_element(
        island_id, 35.0, 35.0, 30.0, 30.0, MOVED_FILL, movement,
    ));
    for index in 2..COUNT {
        let id = format!("remote-positive-{index}");
        moved_ids.push(id.clone());
        elements.push(rectangle_element(
            id,
            200.0 + index as f64 * 12.0,
            0.0,
            6.0,
            4.0,
            MOVED_FILL,
            movement,
        ));
    }

    remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    assert!(
        fills_with_color_contain(&elements, MOVED_FILL, [100.0, 25.0]),
        "a positive island must not cancel the filled negative-winding region"
    );
    assert!(
        fills_with_color_contain(&elements, MOVED_FILL, [60.0, -15.0]),
        "the positive outer shell must remain filled"
    );
    assert_valid_keyframe(&elements);
}

/// Crossing the batching threshold must not turn independently authored fills into one selection
/// target or discard their names. Sixteen and seventeen otherwise identical fills have the same
/// baked identity, order and coverage.
#[test]
fn transformed_batch_threshold_preserves_disjoint_ids_names_and_order() {
    fn run(count: usize) -> Vec<Element> {
        let movement = Transform {
            x: 73.0,
            y: -41.0,
            ..Transform::default()
        };
        let sentinel_a = rectangle_element(
            "identity-sentinel-a",
            -1_000.0,
            -1_000.0,
            5.0,
            5.0,
            SENTINEL_FILL,
            Transform::default(),
        );
        let sentinel_b = rectangle_element(
            "identity-sentinel-b",
            -1_020.0,
            -1_000.0,
            5.0,
            5.0,
            SENTINEL_FILL,
            Transform::default(),
        );
        let mut elements = vec![sentinel_a];
        let mut moved_ids = Vec::with_capacity(count);
        let mut moved_names = Vec::with_capacity(count);
        let mut centres = Vec::with_capacity(count);
        for index in 0..count {
            if index == count / 2 {
                elements.push(sentinel_b.clone());
            }
            let id = format!("threshold-moved-{index}");
            let name = format!("Authored fill {index}");
            let x = index as f64 * 12.0;
            let mut element = rectangle_element(id.clone(), x, 0.0, 6.0, 4.0, MOVED_FILL, movement);
            element.name = name.clone();
            moved_ids.push(id);
            moved_names.push(name);
            centres.push([movement.x + x + 3.0, movement.y + 2.0]);
            elements.push(element);
        }

        let result_ids = remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
        assert_eq!(result_ids, moved_ids, "count {count}: moved IDs changed");
        let expected_ids: Vec<&str> = ["identity-sentinel-a", "identity-sentinel-b"]
            .into_iter()
            .chain(moved_ids.iter().map(String::as_str))
            .collect();
        let actual_ids: Vec<&str> = elements.iter().map(|element| element.id.as_str()).collect();
        assert_eq!(
            actual_ids, expected_ids,
            "count {count}: element order changed"
        );
        let actual_names: Vec<&str> = elements
            .iter()
            .skip(2)
            .map(|element| element.name.as_str())
            .collect();
        let expected_names: Vec<&str> = moved_names.iter().map(String::as_str).collect();
        assert_eq!(actual_names, expected_names, "count {count}: names changed");
        assert!(
            elements
                .iter()
                .skip(2)
                .all(|element| element.transform == Transform::default()),
            "count {count}: transforms were not baked"
        );
        for centre in centres {
            assert!(
                fills_with_color_contain(&elements, MOVED_FILL, centre),
                "count {count}: missing coverage at {centre:?}"
            );
        }
        assert_valid_keyframe(&elements);
        elements
    }

    let sixteen = run(16);
    let seventeen = run(17);
    assert_eq!(
        &sixteen[2..],
        &seventeen[2..18],
        "the fast-path threshold changed the first sixteen authored fills"
    );
}

/// A temporary compound must never consume an existing same-paint region and then be discarded.
/// Any such interaction takes the exact sequential path, whose surviving moved element owns the
/// complete union.
#[test]
fn transformed_batch_uses_exact_path_for_interacting_same_paint_fill() {
    const COUNT: usize = 17;
    let movement = Transform {
        x: 100.0,
        y: 20.0,
        ..Transform::default()
    };
    let stationary = rectangle_element(
        "stationary-same-paint",
        98.0,
        20.0,
        4.0,
        4.0,
        MOVED_FILL,
        Transform::default(),
    );
    let mut elements = vec![stationary];
    let mut moved_ids = Vec::with_capacity(COUNT);
    for index in 0..COUNT {
        let id = format!("same-paint-moved-{index}");
        moved_ids.push(id.clone());
        elements.push(rectangle_element(
            id,
            index as f64 * 12.0,
            0.0,
            6.0,
            4.0,
            MOVED_FILL,
            movement,
        ));
    }

    let result_ids = remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    assert_eq!(result_ids, moved_ids);
    assert!(
        fills_with_color_contain(&elements, MOVED_FILL, [99.0, 22.0]),
        "the stationary extension was lost when the temporary compound was removed"
    );
    assert!(
        !elements
            .iter()
            .any(|element| element.id == "stationary-same-paint"),
        "the exact same-paint union must retain the incoming moved identity"
    );
    assert_valid_keyframe(&elements);
}

/// Local-ring subtraction must retain the complete containment stack of the existing fill while
/// ignoring remote incoming islands. Exercise both authored fill rules with a shell, hole and
/// nested island, then punch only the nested island with a different colour.
#[test]
fn local_ring_subtraction_preserves_nested_nonzero_and_evenodd_holes() {
    const BLUE: [u8; 4] = [0, 100, 255, 255];
    const RED: [u8; 4] = [255, 40, 40, 255];
    let outer = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
    let positive_hole = [[20.0, 20.0], [80.0, 20.0], [80.0, 80.0], [20.0, 80.0]];
    let negative_hole = [[20.0, 20.0], [20.0, 80.0], [80.0, 80.0], [80.0, 20.0]];
    let island = [[40.0, 40.0], [60.0, 40.0], [60.0, 60.0], [40.0, 60.0]];

    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let mut blue_commands = Vec::new();
        for ring in [
            outer.as_slice(),
            if rule == FillRule::NonZero {
                negative_hole.as_slice()
            } else {
                positive_hole.as_slice()
            },
            island.as_slice(),
        ] {
            blue_commands.push(PathCommand::MoveTo {
                x: ring[0][0],
                y: ring[0][1],
            });
            blue_commands.extend(ring[1..].iter().map(|point| PathCommand::LineTo {
                x: point[0],
                y: point[1],
            }));
            blue_commands.push(PathCommand::Close);
        }
        let blue = Element {
            id: format!("blue-{rule:?}"),
            name: "blue".into(),
            kind: ElementKind::Path {
                commands: blue_commands,
                fill_rule: rule,
                fill: BLUE,
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            merge: true,
            ..Element::default()
        };

        let mut red = rectangle_element(
            format!("red-{rule:?}"),
            45.0,
            45.0,
            10.0,
            10.0,
            RED,
            Transform::default(),
        );
        let ElementKind::Path { commands, .. } = &mut red.kind else {
            unreachable!()
        };
        commands.extend([
            PathCommand::MoveTo { x: 300.0, y: 300.0 },
            PathCommand::LineTo { x: 310.0, y: 300.0 },
            PathCommand::LineTo { x: 310.0, y: 310.0 },
            PathCommand::LineTo { x: 300.0, y: 310.0 },
            PathCommand::Close,
        ]);

        let mut elements = vec![blue];
        apply_merge_shape_to_keyframe(&mut elements, red);
        let blue_rings = fill_rings_with_color(&elements, BLUE);
        let red_rings = fill_rings_with_color(&elements, RED);
        for point in [[10.0, 10.0], [42.0, 42.0], [58.0, 58.0]] {
            assert!(point_in_rings(point, &blue_rings, FillRule::NonZero));
        }
        for point in [[30.0, 30.0], [50.0, 50.0]] {
            assert!(!point_in_rings(point, &blue_rings, FillRule::NonZero));
        }
        assert!(point_in_rings([50.0, 50.0], &red_rings, FillRule::NonZero));
        assert!(point_in_rings(
            [305.0, 305.0],
            &red_rings,
            FillRule::NonZero
        ));
        let blue_area: f64 = blue_rings.iter().map(|ring| ring_area(ring)).sum();
        assert!((blue_area - 6_700.0).abs() < 1e-8, "{rule:?}: {blue_area}");
        assert_valid_keyframe(&elements);
    }
}

/// Batch limits are applied across compound fills, not just across element IDs. Seventeen moved
/// elements containing 1,000 rectangles each contribute 68,000 flattened ring points, so the
/// operation must use multiple bounded compounds while restoring every authored element.
#[test]
fn transformed_compound_fills_split_at_flattened_point_budget_without_loss() {
    const ELEMENT_COUNT: usize = 17;
    const RECTANGLES_PER_ELEMENT: usize = 1_000;
    const TOTAL_RECTANGLES: usize = ELEMENT_COUNT * RECTANGLES_PER_ELEMENT;
    const COLUMNS: usize = 200;
    const SIZE: [f64; 2] = [1.0, 1.0];
    const SPACING: [f64; 2] = [3.0, 3.0];
    let movement = Transform {
        x: 0.5,
        y: -0.75,
        ..Transform::default()
    };

    let mut elements = Vec::with_capacity(ELEMENT_COUNT);
    let mut moved_ids = Vec::with_capacity(ELEMENT_COUNT);
    for element_index in 0..ELEMENT_COUNT {
        let id = format!("compound-moved-{element_index}");
        moved_ids.push(id.clone());
        elements.push(compound_rectangle_element(
            id,
            CompoundRectangles {
                first_index: element_index * RECTANGLES_PER_ELEMENT,
                count: RECTANGLES_PER_ELEMENT,
                columns: COLUMNS,
                spacing: SPACING,
                size: SIZE,
            },
            MOVED_FILL,
            movement,
        ));
    }

    let result_ids = remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    assert_eq!(
        result_ids, moved_ids,
        "bounded compounds must restore every authored moved ID"
    );
    let actual_ids: Vec<&str> = elements.iter().map(|element| element.id.as_str()).collect();
    let expected_ids: Vec<&str> = moved_ids.iter().map(String::as_str).collect();
    assert_eq!(actual_ids, expected_ids, "bounded compounds changed order");
    assert!(
        elements
            .iter()
            .zip(&moved_ids)
            .all(|(element, id)| element.name == *id),
        "bounded compounds changed authored names"
    );
    assert!(
        elements
            .iter()
            .filter(|element| element_fill(element) == Some(MOVED_FILL))
            .all(|element| element.transform == Transform::default()),
        "successful remerge must bake Stage transforms into world-space paths"
    );

    let moved_rings = fill_rings_with_color(&elements, MOVED_FILL);
    assert_eq!(
        moved_rings.len(),
        TOTAL_RECTANGLES,
        "bounded batches lost disconnected contours"
    );
    let actual_area: f64 = moved_rings.iter().map(|ring| ring_area(ring).abs()).sum();
    let expected_area = TOTAL_RECTANGLES as f64 * SIZE[0] * SIZE[1];
    assert_eq!(actual_area, expected_area, "bounded batches changed area");
    for index in [
        0,
        RECTANGLES_PER_ELEMENT - 1,
        RECTANGLES_PER_ELEMENT,
        14_999,
        15_000,
        TOTAL_RECTANGLES - 1,
    ] {
        let centre = [
            (index % COLUMNS) as f64 * SPACING[0] + movement.x + SIZE[0] * 0.5,
            (index / COLUMNS) as f64 * SPACING[1] + movement.y + SIZE[1] * 0.5,
        ];
        assert!(
            point_in_rings(centre, &moved_rings, FillRule::NonZero),
            "bounded batches lost coverage at rectangle {index}"
        );
    }
    assert_valid_keyframe(&elements);
}

/// A failure after earlier batches have already mutated the keyframe must restore the complete
/// pre-remerge element vector and the original moved selection IDs.
#[test]
fn transformed_remerge_rolls_back_atomically_when_late_compound_exceeds_path_limit() {
    const VALID_COUNT: usize = 16;
    const OVERSIZED_RECTANGLES: usize = neoflash_core::MAX_PATH_COMMANDS / 5 + 1;
    let movement = Transform {
        x: 24.0,
        y: -16.0,
        ..Transform::default()
    };

    let mut elements = Vec::with_capacity(VALID_COUNT + 1);
    let mut moved_ids = Vec::with_capacity(VALID_COUNT + 1);
    for index in 0..VALID_COUNT {
        let id = format!("valid-before-failure-{index}");
        moved_ids.push(id.clone());
        elements.push(rectangle_element(
            id,
            index as f64 * 4.0,
            0.0,
            2.0,
            2.0,
            MOVED_FILL,
            movement,
        ));
    }

    let oversized_id = "oversized-late-fill".to_string();
    moved_ids.push(oversized_id.clone());
    let oversized = compound_rectangle_element(
        oversized_id,
        CompoundRectangles {
            first_index: 0,
            count: OVERSIZED_RECTANGLES,
            columns: 256,
            spacing: [3.0, 3.0],
            size: [1.0, 1.0],
        },
        MOVED_FILL,
        movement,
    );
    let ElementKind::Path { commands, .. } = &oversized.kind else {
        unreachable!()
    };
    assert_eq!(commands.len(), neoflash_core::MAX_PATH_COMMANDS + 5);
    assert!(
        flatten_closed_rings(commands).is_empty(),
        "oversized compound must be deliberately unflattenable"
    );
    elements.push(oversized);

    let before = elements.clone();
    let original_moved_ids = moved_ids.clone();
    let result_ids = remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    assert_eq!(
        elements, before,
        "a late batch failure must restore the exact element vector"
    );
    assert_eq!(
        result_ids, original_moved_ids,
        "rollback must preserve the original moved selection IDs and order"
    );
}

#[test]
#[ignore]
fn dense_hatching_merge_timing() {
    let count: usize = std::env::var("MERGE_HATCH_STROKES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let mut elements = Vec::new();
    let mut worst = std::time::Duration::ZERO;
    let mut window = std::time::Duration::ZERO;
    let started = std::time::Instant::now();
    for i in 0..count {
        let ring = stroke_outline(i);
        let t = std::time::Instant::now();
        apply_merge_shape_to_keyframe(&mut elements, stroke_element(i, &ring));
        let dt = t.elapsed();
        worst = worst.max(dt);
        window = window.max(dt);
        if (i + 1) % 250 == 0 {
            let edges: usize = fill_rings(&elements).iter().map(Vec::len).sum();
            eprintln!(
                "strokes {:>5}: worst of last 250 {:>9.2?}; {} elements, {} edges",
                i + 1,
                window,
                elements.len(),
                edges
            );
            window = std::time::Duration::ZERO;
        }
    }
    eprintln!(
        "{count} strokes in {:.2?}, worst stroke {worst:.2?}",
        started.elapsed()
    );
}

/// Opt-in release benchmark for the Stage multi-selection path. It deliberately has no timing
/// ceiling: performance varies by machine, while contour count and area are deterministic.
#[test]
#[ignore]
fn transformed_merge_move_benchmark() {
    if cfg!(debug_assertions) {
        panic!("run this opt-in benchmark with cargo test --release");
    }
    let count: usize = std::env::var("MERGE_MOVE_SHAPES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60_000);
    assert!(count > 16, "benchmark must exercise the multi-fill path");

    const WIDTH: f64 = 1.25;
    const HEIGHT: f64 = 0.75;
    const SENTINEL_LIMIT: usize = 32;
    const COLUMNS: usize = 1_000;
    let movement = Transform {
        x: 0.75,
        y: -3.5,
        ..Transform::default()
    };
    let sentinel_step = (count / SENTINEL_LIMIT).max(1);
    let mut elements = Vec::with_capacity(count + SENTINEL_LIMIT);
    let mut moved_ids = Vec::with_capacity(count);
    let mut sentinels = Vec::with_capacity(SENTINEL_LIMIT);
    for index in 0..count {
        if sentinels.len() < SENTINEL_LIMIT && index == sentinels.len() * sentinel_step {
            let sentinel_index = sentinels.len();
            let sentinel = rectangle_element(
                format!("bench-sentinel-{sentinel_index}"),
                -10_000.0 - sentinel_index as f64 * 4.0,
                -10_000.0,
                2.0,
                2.0,
                SENTINEL_FILL,
                Transform::default(),
            );
            elements.push(sentinel.clone());
            sentinels.push(sentinel);
        }

        let x = (index % COLUMNS) as f64 * 3.0;
        let y = (index / COLUMNS) as f64 * 2.5;
        let id = format!("bench-moved-{index}");
        moved_ids.push(id.clone());
        elements.push(rectangle_element(
            id, x, y, WIDTH, HEIGHT, MOVED_FILL, movement,
        ));
    }

    let started = std::time::Instant::now();
    let result_ids = remerge_moved_elements_in_keyframe(&mut elements, &moved_ids);
    let elapsed = started.elapsed();
    assert_eq!(
        result_ids, moved_ids,
        "benchmark must preserve every moved ID in authored order"
    );
    let element_ids: std::collections::HashSet<&str> =
        elements.iter().map(|element| element.id.as_str()).collect();
    assert_eq!(
        element_ids.len(),
        elements.len(),
        "benchmark output IDs must remain unique"
    );
    assert!(
        result_ids
            .iter()
            .all(|id| element_ids.contains(id.as_str())),
        "every returned moved ID must exist"
    );
    assert!(
        elements
            .iter()
            .filter(|element| element_fill(element) == Some(MOVED_FILL))
            .all(|element| element.name == element.id),
        "benchmark must preserve every authored moved name"
    );
    for expected in &sentinels {
        assert_eq!(
            elements.iter().find(|element| element.id == expected.id),
            Some(expected),
            "untouched benchmark sentinel changed"
        );
    }

    let moved_rings = fill_rings_with_color(&elements, MOVED_FILL);
    assert_eq!(
        moved_rings.len(),
        count,
        "benchmark remerge changed moved contour count"
    );
    let actual_area: f64 = moved_rings.iter().map(|ring| ring_area(ring).abs()).sum();
    let expected_area = count as f64 * WIDTH * HEIGHT;
    assert!(
        (actual_area - expected_area).abs() <= expected_area * 1e-9,
        "benchmark remerge changed area: expected {expected_area}, got {actual_area}"
    );
    for index in [0, count / 2, count - 1] {
        let centre = [
            (index % COLUMNS) as f64 * 3.0 + movement.x + WIDTH * 0.5,
            (index / COLUMNS) as f64 * 2.5 + movement.y + HEIGHT * 0.5,
        ];
        assert!(point_in_rings(centre, &moved_rings, FillRule::NonZero));
    }
    assert_valid_keyframe(&elements);
    eprintln!(
        "remerged {count} transformed rectangles into {} output IDs in {elapsed:.2?}; {} contours, exact area {actual_area}",
        result_ids.len(),
        moved_rings.len()
    );
}
