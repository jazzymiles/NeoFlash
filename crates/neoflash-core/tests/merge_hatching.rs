//! Merge Drawing under dense same-colour hatching (T-090).
//!
//! Opt-in timing run (thousands of strokes, per-stroke Merge time):
//! `MERGE_HATCH_STROKES=3000 cargo test --release -p neoflash-core --test merge_hatching -- --ignored --nocapture`

use neoflash_core::merge_drawing::{
    apply_merge_shape_to_keyframe, element_world_commands, flatten_closed_rings, point_in_rings,
};
use neoflash_core::model::{Element, ElementKind, FillRule, PathCommand, Transform};

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
