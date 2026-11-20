use super::*;

/// Perform a 2D boolean operation on two multi-contour paths `a` and `b`, returning one
/// `Vec<PathCommand>` per disconnected outer island (with any inner holes included in that
/// island's compound path).
pub fn boolean_paths(
    a_cmds: &[PathCommand],
    a_rule: FillRule,
    b_cmds: &[PathCommand],
    b_rule: FillRule,
    op: BooleanOp,
) -> Vec<Vec<PathCommand>> {
    let rings_a = flatten_closed_rings(a_cmds);
    let rings_b = flatten_closed_rings(b_cmds);
    boolean_rings(&rings_a, a_rule, &rings_b, b_rule, op)
}

pub fn boolean_rings(
    rings_a: &[Vec<[f64; 2]>],
    rule_a: FillRule,
    rings_b: &[Vec<[f64; 2]>],
    rule_b: FillRule,
    op: BooleanOp,
) -> Vec<Vec<PathCommand>> {
    boolean_rings_with_interaction(rings_a, rule_a, rings_b, rule_b, op).0
}

pub(super) fn rings_interact_with_prepared(
    rings_a: &[Vec<[f64; 2]>],
    rule_a: FillRule,
    bbox_a: ([f64; 2], [f64; 2]),
    segs_b: &[([f64; 2], [f64; 2])],
    bvh_b: &SegmentBvh,
    prepared_b: &PreparedRings,
) -> bool {
    for ring in rings_a {
        let len = ring.len();
        if len < 3 {
            continue;
        }
        if prepared_b.contains(ring[0]) {
            return true;
        }
        for i in 0..len {
            let a0 = ring[i];
            let a1 = ring[(i + 1) % len];
            if bvh_b.any_segment_intersections(segs_b, a0, a1) {
                return true;
            }
        }
    }
    let pad = SNAP_EPS * 2.0;
    bvh_b.any_endpoint_matching(segs_b, bbox_a, pad, |point| {
        point_in_rings(point, rings_a, rule_a)
    })
}

pub(super) fn boolean_rings_with_interaction(
    rings_a: &[Vec<[f64; 2]>],
    rule_a: FillRule,
    rings_b: &[Vec<[f64; 2]>],
    rule_b: FillRule,
    op: BooleanOp,
) -> (Vec<Vec<PathCommand>>, bool) {
    if rings_a.is_empty() {
        return match op {
            BooleanOp::Union | BooleanOp::Xor => (
                assemble_shells_and_holes(
                    rings_b
                        .iter()
                        .cloned()
                        .map(|mut r| {
                            if signed_ring_area(&r) < 0.0 {
                                r.reverse();
                            }
                            r
                        })
                        .collect(),
                ),
                !rings_b.is_empty(),
            ),
            BooleanOp::Intersection | BooleanOp::Difference => (Vec::new(), false),
        };
    }
    if rings_b.is_empty() {
        return match op {
            BooleanOp::Union | BooleanOp::Difference | BooleanOp::Xor => {
                let mut cmds = Vec::new();
                for r in rings_a {
                    cmds.extend(ring_to_commands(r));
                }
                if cmds.is_empty() {
                    (Vec::new(), false)
                } else {
                    (vec![cmds], false)
                }
            }
            BooleanOp::Intersection => (Vec::new(), false),
        };
    }

    if matches!(op, BooleanOp::Union | BooleanOp::Difference)
        && rule_a == FillRule::NonZero
        && let Some(result) = boolean_rings_windowed(rings_a, rings_b, rule_b, op, WINDOW_MIN_EDGES)
    {
        return result;
    }
    boolean_rings_full(rings_a, rule_a, rings_b, rule_b, op)
}

pub(super) fn boolean_rings_full(
    rings_a: &[Vec<[f64; 2]>],
    rule_a: FillRule,
    rings_b: &[Vec<[f64; 2]>],
    rule_b: FillRule,
    op: BooleanOp,
) -> (Vec<Vec<PathCommand>>, bool) {
    let (mut segments, map_a) = cull_and_clip_union_segments_with_map(rings_a, rule_a);
    let offset_b = segments.len();
    let (segs_b, mut map_b) = cull_and_clip_union_segments_with_map(rings_b, rule_b);
    if matches!(op, BooleanOp::Difference | BooleanOp::Intersection)
        && let Some(bbox_a) = rings_bounds(rings_a)
    {
        let pad = SNAP_EPS * 4.0;
        let mut filtered_segs_b = Vec::new();
        let mut remap = vec![None; segs_b.len()];
        for (old_idx, &(b0, b1)) in segs_b.iter().enumerate() {
            let b_min = [b0[0].min(b1[0]), b0[1].min(b1[1])];
            let b_max = [b0[0].max(b1[0]), b0[1].max(b1[1])];
            if bounds_overlap(bbox_a, (b_min, b_max), pad) {
                let new_idx = filtered_segs_b.len();
                filtered_segs_b.push((b0, b1));
                remap[old_idx] = Some(new_idx);
            }
        }
        for r_edges in &mut map_b {
            for edge_opt in r_edges {
                if let Some(old_idx) = *edge_opt {
                    *edge_opt = remap[old_idx];
                }
            }
        }
        segments.extend(filtered_segs_b);
    } else {
        segments.extend(segs_b);
    }
    let (graph, seg_chains) = PlanarGraph::from_segments_with_chains(&segments);
    let snapped_a = build_snapped_rings(rings_a, &map_a, 0, &graph.vertices, &seg_chains);
    let snapped_b = build_snapped_rings(rings_b, &map_b, offset_b, &graph.vertices, &seg_chains);
    let prepared_a = PreparedRings::new(&snapped_a, rule_a);
    let prepared_b = PreparedRings::new(&snapped_b, rule_b);

    let mut boundary_half_edges = Vec::new();
    let mut interacted = false;
    for edge_idx in 0..graph.edges.len() {
        let Some((p_left, p_right)) = graph.probe_points(edge_idx) else {
            continue;
        };
        let in_a_left = prepared_a.contains(p_left);
        let in_b_left = prepared_b.contains(p_left);
        let in_a_right = prepared_a.contains(p_right);
        let in_b_right = prepared_b.contains(p_right);
        match op {
            BooleanOp::Difference | BooleanOp::Intersection => {
                interacted |= (in_a_left && in_b_left) || (in_a_right && in_b_right);
            }
            BooleanOp::Union | BooleanOp::Xor => {
                interacted |= (in_a_left || in_a_right) && (in_b_left || in_b_right);
            }
        }
        let keep_left = op.eval(in_a_left, in_b_left);
        let keep_right = op.eval(in_a_right, in_b_right);
        let (u, v) = graph.edges[edge_idx];
        if keep_left && !keep_right {
            boundary_half_edges.push((u, v));
        } else if !keep_left && keep_right {
            boundary_half_edges.push((v, u));
        }
    }

    let cycles = trace_half_edge_cycles(&graph.vertices, &boundary_half_edges);
    let rings: Vec<Vec<[f64; 2]>> = cycles
        .into_iter()
        .map(|cycle| {
            cycle
                .into_iter()
                .map(|he_idx| graph.vertices[boundary_half_edges[he_idx].0])
                .collect()
        })
        .collect();
    (assemble_shells_and_holes(rings), interacted)
}

/// Existing artwork smaller than this always takes the full planar rebuild.
pub(super) const WINDOW_MIN_EDGES: usize = 256;

/// Local Merge boolean (T-090): `A ∪ B` or `A − B` where only the edges of `A` near `B` enter
/// the planar graph.
///
/// Under Union and Difference an edge of `A` whose bounds stay clear of `B`'s bounds keeps its
/// status: it bounds the result exactly where it bounds `A`. Such runs are kept verbatim and
/// stitched to the recomputed local edges at their shared endpoints, so drawing a stroke over a
/// large same-colour blob costs roughly the size of the stroke's neighbourhood instead of the
/// whole blob (the way classic Flash edits only the edges a new stroke touches).
///
/// Returns `None` (caller falls back to the full rebuild) when the window covers most of `A`, or
/// when a kept run turns out not to be a clean boundary of `A` (self-overlapping input).
pub(super) fn boolean_rings_windowed(
    rings_a: &[Vec<[f64; 2]>],
    rings_b: &[Vec<[f64; 2]>],
    rule_b: FillRule,
    op: BooleanOp,
    min_edges: usize,
) -> Option<(Vec<Vec<PathCommand>>, bool)> {
    let total_edges: usize = rings_a.iter().filter(|r| r.len() >= 3).map(Vec::len).sum();
    if total_edges < min_edges {
        return None;
    }
    let bbox_b = rings_bounds(rings_b)?;
    let pad = SNAP_EPS * 8.0;
    let window = (
        [bbox_b.0[0] - pad, bbox_b.0[1] - pad],
        [bbox_b.1[0] + pad, bbox_b.1[1] + pad],
    );

    // An edge of `A` keeps its status unless it comes near `B`'s boundary or lies inside `B`.
    // Without approaching the boundary an edge is entirely inside or outside `B`, so its
    // midpoint decides.
    let raw_b_segs = rings_to_segments(rings_b);
    let b_bvh = SegmentBvh::from_segments(&raw_b_segs);
    let raw_b = PreparedRings::new(rings_b, rule_b);
    let mut segments: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let mut map_a: Vec<Vec<Option<usize>>> = Vec::with_capacity(rings_a.len());
    for ring in rings_a {
        let n = ring.len();
        let mut map = vec![None; n];
        if n >= 3 {
            for (i, slot) in map.iter_mut().enumerate() {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                let seg_box = (
                    [a[0].min(b[0]), a[1].min(b[1])],
                    [a[0].max(b[0]), a[1].max(b[1])],
                );
                if bounds_overlap(seg_box, window, 0.0)
                    && (b_bvh.any_bounds_overlap(&raw_b_segs, seg_box, pad)
                        || raw_b.contains([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]))
                {
                    *slot = Some(segments.len());
                    segments.push((a, b));
                }
            }
        }
        map_a.push(map);
    }
    // Mostly local: the full rebuild is as cheap and keeps its contour culling.
    if segments.len() * 2 > total_edges {
        return None;
    }

    // Kept edges that pass within probe distance of a recomputed edge: probes must not cross them.
    let near_bvh = SegmentBvh::from_segments(&segments);
    let mut context: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for (ring, map) in rings_a.iter().zip(&map_a) {
        let n = ring.len();
        for (i, slot) in map.iter().enumerate() {
            if slot.is_some() {
                continue;
            }
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let seg_box = (
                [a[0].min(b[0]), a[1].min(b[1])],
                [a[0].max(b[0]), a[1].max(b[1])],
            );
            if near_bvh.any_bounds_overlap(&segments, seg_box, 0.1) {
                context.push((a, b));
            }
        }
    }
    let context_bvh = SegmentBvh::from_segments(&context);

    let offset_b = segments.len();
    let (segs_b, map_b) = cull_and_clip_union_segments_with_map(rings_b, rule_b);
    segments.extend(segs_b);
    let (graph, seg_chains) = PlanarGraph::from_segments_with_chains(&segments);
    let snapped_a = build_snapped_rings(rings_a, &map_a, 0, &graph.vertices, &seg_chains);
    let snapped_b = build_snapped_rings(rings_b, &map_b, offset_b, &graph.vertices, &seg_chains);
    let prepared_a = PreparedRings::new(&snapped_a, FillRule::NonZero);
    let prepared_b = PreparedRings::new(&snapped_b, rule_b);

    let mut half_edges: Vec<(usize, usize)> = Vec::new();
    let mut interacted = false;
    for edge_idx in 0..graph.edges.len() {
        let Some((p_left, p_right)) =
            graph.probe_points_with_context(edge_idx, Some((&context, &context_bvh)))
        else {
            continue;
        };
        let in_a_left = prepared_a.contains(p_left);
        let in_b_left = prepared_b.contains(p_left);
        let in_a_right = prepared_a.contains(p_right);
        let in_b_right = prepared_b.contains(p_right);
        interacted |= match op {
            BooleanOp::Difference => (in_a_left && in_b_left) || (in_a_right && in_b_right),
            _ => (in_a_left || in_a_right) && (in_b_left || in_b_right),
        };
        let keep_left = op.eval(in_a_left, in_b_left);
        let keep_right = op.eval(in_a_right, in_b_right);
        let (u, v) = graph.edges[edge_idx];
        if keep_left && !keep_right {
            half_edges.push((u, v));
        } else if !keep_left && keep_right {
            half_edges.push((v, u));
        }
    }

    // Which side of a far edge of `A` is filled: Some(true) = left, None = not a clean boundary.
    // Far from `B`, both Union and Difference reduce to `A` itself.
    let filled_on_left = |a: [f64; 2], b: [f64; 2]| -> Option<bool> {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 1e-9 {
            return None;
        }
        let delta = (len * 0.25).min(1e-4);
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let (nx, ny) = (-dy / len * delta, dx / len * delta);
        let left = prepared_a.contains([mid[0] + nx, mid[1] + ny]);
        let right = prepared_a.contains([mid[0] - nx, mid[1] - ny]);
        (left != right).then_some(left)
    };

    let mut chains: Vec<Vec<[f64; 2]>> = vec![Vec::new(); half_edges.len()];
    let mut untouched_rings: Vec<Vec<[f64; 2]>> = Vec::new();
    for (ring, map) in rings_a.iter().zip(&map_a) {
        let n = ring.len();
        if n < 3 {
            continue;
        }
        let near = map.iter().filter(|m| m.is_some()).count();
        if near == n {
            continue;
        }
        if near == 0 {
            let edge = (0..n).max_by(|&i, &j| {
                dist_sq(ring[i], ring[(i + 1) % n]).total_cmp(&dist_sq(ring[j], ring[(j + 1) % n]))
            })?;
            let mut kept = ring.clone();
            if !filled_on_left(ring[edge], ring[(edge + 1) % n])? {
                kept.reverse();
            }
            untouched_rings.push(kept);
            continue;
        }
        for first in 0..n {
            let prev = (first + n - 1) % n;
            if map[first].is_some() || map[prev].is_none() {
                continue;
            }
            let mut last = first;
            while map[(last + 1) % n].is_none() {
                last = (last + 1) % n;
            }
            let next = (last + 1) % n;
            let start_v = *seg_chains.get(map[prev]?)?.last()?;
            let end_v = *seg_chains.get(map[next]?)?.first()?;
            let run_len = (last + n - first) % n + 1;
            let probe = (first + run_len / 2) % n;
            let mut interior: Vec<[f64; 2]> = (1..run_len).map(|k| ring[(first + k) % n]).collect();
            if filled_on_left(ring[probe], ring[(probe + 1) % n])? {
                half_edges.push((start_v, end_v));
            } else {
                interior.reverse();
                half_edges.push((end_v, start_v));
            }
            chains.push(interior);
        }
    }

    let cycles = trace_half_edge_cycles_with_chains(&graph.vertices, &half_edges, &chains);
    // Every boundary half-edge must close into a cycle; a dangling run means the local result
    // could not be stitched to the kept contours, so take the exact full rebuild instead.
    if cycles.iter().map(Vec::len).sum::<usize>() != half_edges.len() {
        return None;
    }
    let mut rings: Vec<Vec<[f64; 2]>> = cycles
        .into_iter()
        .map(|cycle| {
            let mut pts = Vec::new();
            for he in cycle {
                pts.push(graph.vertices[half_edges[he].0]);
                pts.extend_from_slice(&chains[he]);
            }
            pts
        })
        .collect();
    rings.extend(untouched_rings);
    Some((assemble_shells_and_holes(rings), interacted))
}

/// Slice a filled multi-contour path with one or more stroke polylines.
///
/// If the stroke cuts across the fill (or forms a closed loop inside it), returns the resulting
/// separated fill islands; if the stroke does not divide the fill, returns a single path matching
/// the original fill.
pub fn slice_fill_with_stroke(
    fill_cmds: &[PathCommand],
    fill_rule: FillRule,
    stroke_cmds: &[PathCommand],
) -> Vec<Vec<PathCommand>> {
    let fill_rings = flatten_closed_rings(fill_cmds);
    let polylines = flatten_stroke_polylines(stroke_cmds);
    slice_fill_rings_with_polylines(&fill_rings, fill_rule, &polylines)
}

pub fn slice_fill_rings_with_polylines(
    fill_rings: &[Vec<[f64; 2]>],
    fill_rule: FillRule,
    polylines: &[Vec<[f64; 2]>],
) -> Vec<Vec<PathCommand>> {
    if fill_rings.is_empty() {
        return Vec::new();
    }
    let Some(fill_bbox) = rings_bounds(fill_rings) else {
        return Vec::new();
    };
    let stroke_segs = polylines_to_segments(polylines);
    if stroke_segs.is_empty() {
        let mut cmds = Vec::new();
        for r in fill_rings {
            cmds.extend(ring_to_commands(r));
        }
        return vec![cmds];
    }

    // Filter stroke segments to those overlapping the fill's bounding box.
    let overlapping_stroke_segs: Vec<([f64; 2], [f64; 2])> = stroke_segs
        .into_iter()
        .filter(|&(a, b)| {
            let s_min = [a[0].min(b[0]), a[1].min(b[1])];
            let s_max = [a[0].max(b[0]), a[1].max(b[1])];
            bounds_overlap(fill_bbox, (s_min, s_max), SNAP_EPS * 2.0)
        })
        .collect();
    if overlapping_stroke_segs.is_empty() {
        let mut cmds = Vec::new();
        for r in fill_rings {
            cmds.extend(ring_to_commands(r));
        }
        return vec![cmds];
    }

    let (mut all_segs, fill_map) = cull_and_clip_union_segments_with_map(fill_rings, fill_rule);
    all_segs.extend_from_slice(&overlapping_stroke_segs);
    let (graph, seg_chains) = PlanarGraph::from_segments_with_chains(&all_segs);
    let snapped_fill = build_snapped_rings(fill_rings, &fill_map, 0, &graph.vertices, &seg_chains);
    let prepared_fill = PreparedRings::new(&snapped_fill, fill_rule);
    let stroke_bvh = SegmentBvh::from_segments(&overlapping_stroke_segs);

    let mut active_half_edges: Vec<(usize, usize)> = Vec::new();
    for edge_idx in 0..graph.edges.len() {
        let Some((p_left, p_right)) = graph.probe_points(edge_idx) else {
            continue;
        };
        let left_in = prepared_fill.contains(p_left);
        let right_in = prepared_fill.contains(p_right);
        let (u, v) = graph.edges[edge_idx];
        // An internal contour edge is not a cut. Only an actual stroke may divide two filled
        // faces. The graph has already split edges at every intersection, so its midpoint
        // identifies whether this complete subedge belongs to a cutter.
        let is_cut = if left_in && right_in {
            let a = graph.vertices[u];
            let b = graph.vertices[v];
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            stroke_bvh.any_within_distance(&overlapping_stroke_segs, mid, SNAP_EPS)
        } else {
            false
        };
        if left_in && (!right_in || is_cut) {
            active_half_edges.push((u, v));
        }
        if right_in && (!left_in || is_cut) {
            active_half_edges.push((v, u));
        }
    }

    // Iteratively prune any bridge/slit edge where both `(u, v)` and `(v, u)` belong to the same
    // traced cycle (meaning the stroke segment did not divide the face into two separate regions).
    loop {
        let cycles = trace_half_edge_cycles(&graph.vertices, &active_half_edges);
        let mut he_to_cycle = vec![usize::MAX; active_half_edges.len()];
        for (c_idx, cycle) in cycles.iter().enumerate() {
            for &he_idx in cycle {
                he_to_cycle[he_idx] = c_idx;
            }
        }
        let mut drop_undirected: Vec<(usize, usize)> = Vec::new();
        for (he_idx, &(u, v)) in active_half_edges.iter().enumerate() {
            if he_to_cycle[he_idx] == usize::MAX {
                let key = if u < v { (u, v) } else { (v, u) };
                drop_undirected.push(key);
                continue;
            }
            if u < v
                && let Some(rev_idx) = active_half_edges.iter().position(|&e| e == (v, u))
                && he_to_cycle[he_idx] == he_to_cycle[rev_idx]
            {
                drop_undirected.push((u, v));
            }
        }
        if drop_undirected.is_empty() {
            let rings: Vec<Vec<[f64; 2]>> = cycles
                .into_iter()
                .map(|cycle| {
                    cycle
                        .into_iter()
                        .map(|he_idx| graph.vertices[active_half_edges[he_idx].0])
                        .collect()
                })
                .collect();
            return assemble_shells_and_holes(rings);
        }
        drop_undirected.sort_unstable();
        drop_undirected.dedup();
        active_half_edges.retain(|&(u, v)| {
            let key = if u < v { (u, v) } else { (v, u) };
            !drop_undirected.binary_search(&key).is_ok()
        });
        if active_half_edges.is_empty() {
            return Vec::new();
        }
    }
}

/// Trim a stroke polyline against a filled region, removing any portion strictly inside the fill's
/// interior while keeping portions outside (or on the boundary).
pub fn trim_polyline_inside_fill(
    poly: &[[f64; 2]],
    fill_rings: &[Vec<[f64; 2]>],
    fill_rule: FillRule,
) -> Vec<Vec<[f64; 2]>> {
    if poly.len() < 2 || fill_rings.is_empty() {
        return vec![poly.to_vec()];
    }
    let fill_segs = fill_boundary_segments(fill_rings, fill_rule);
    let fill_bvh = SegmentBvh::from_segments(&fill_segs);
    let prepared_fill = PreparedRings::new(fill_rings, fill_rule);
    trim_polyline_inside_prepared_fill(poly, &fill_segs, &fill_bvh, &prepared_fill)
}

pub(super) fn trim_polyline_inside_prepared_fill(
    poly: &[[f64; 2]],
    fill_segs: &[([f64; 2], [f64; 2])],
    fill_bvh: &SegmentBvh,
    prepared_fill: &PreparedRings,
) -> Vec<Vec<[f64; 2]>> {
    let Some(poly_bbox) = ring_bounds(poly) else {
        return Vec::new();
    };
    let Some(fill_bbox) = prepared_fill.bounds else {
        return vec![poly.to_vec()];
    };
    if !bounds_overlap(poly_bbox, fill_bbox, SNAP_EPS * 2.0) {
        return vec![poly.to_vec()];
    }
    let cutters: Vec<_> = fill_segs
        .iter()
        .copied()
        .filter(|&(a, b)| {
            let s_min = [a[0].min(b[0]), a[1].min(b[1])];
            let s_max = [a[0].max(b[0]), a[1].max(b[1])];
            bounds_overlap(poly_bbox, (s_min, s_max), SNAP_EPS * 2.0)
                && !segment_lies_on_polyline((a, b), poly)
        })
        .collect();
    let sub_segments = split_polyline_into_subsegments(poly, &cutters, false);

    let mut result: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();

    for (a, b, cut_at_start) in sub_segments {
        if cut_at_start && current.len() >= 2 {
            result.push(std::mem::take(&mut current));
        }
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let on_boundary = fill_bvh.any_within_distance(fill_segs, mid, SNAP_EPS * 2.0);
        let strictly_inside = !on_boundary && prepared_fill.contains(mid);
        if strictly_inside {
            if current.len() >= 2 {
                result.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        } else {
            if current.is_empty() {
                current.push(a);
            }
            current.push(b);
        }
    }
    if current.len() >= 2 {
        result.push(current);
    }
    result
}

/// Split a stroke polyline at every intersection with `cutter_segments` (and optionally at
/// self-intersections).
pub fn split_polyline_with_segments(
    poly: &[[f64; 2]],
    cutter_segments: &[([f64; 2], [f64; 2])],
    self_split: bool,
) -> Vec<Vec<[f64; 2]>> {
    if poly.len() < 2 {
        return Vec::new();
    }
    let sub_segments = split_polyline_into_subsegments(poly, cutter_segments, self_split);
    let mut pieces: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();

    for (a, b, cut_at_start) in sub_segments {
        if cut_at_start && current.len() >= 2 {
            pieces.push(std::mem::take(&mut current));
        }
        if current.is_empty() {
            current.push(a);
        }
        current.push(b);
    }
    if current.len() >= 2 {
        pieces.push(current);
    }
    pieces
}

pub(super) fn split_polyline_into_subsegments(
    poly: &[[f64; 2]],
    cutter_segments: &[([f64; 2], [f64; 2])],
    self_split: bool,
) -> Vec<([f64; 2], [f64; 2], bool)> {
    let n_segs = poly.len().saturating_sub(1);
    if n_segs == 0 {
        return Vec::new();
    }
    let mut cut_params: Vec<Vec<f64>> = vec![Vec::new(); n_segs];

    for i in 0..n_segs {
        let a0 = poly[i];
        let a1 = poly[i + 1];
        let a_min = [a0[0].min(a1[0]), a0[1].min(a1[1])];
        let a_max = [a0[0].max(a1[0]), a0[1].max(a1[1])];

        for &(b0, b1) in cutter_segments {
            let b_min = [b0[0].min(b1[0]), b0[1].min(b1[1])];
            let b_max = [b0[0].max(b1[0]), b0[1].max(b1[1])];
            if !bounds_overlap((a_min, a_max), (b_min, b_max), SNAP_EPS * 2.0) {
                continue;
            }
            for (t, _) in segment_intersections(a0, a1, b0, b1) {
                cut_params[i].push(t);
            }
        }

        if self_split {
            for j in (i + 2)..n_segs {
                // Skip closing corner of a closed loop.
                if i == 0
                    && j + 1 == n_segs
                    && dist_sq(poly[0], poly[n_segs]) <= SNAP_EPS * SNAP_EPS
                {
                    continue;
                }
                let b0 = poly[j];
                let b1 = poly[j + 1];
                let b_min = [b0[0].min(b1[0]), b0[1].min(b1[1])];
                let b_max = [b0[0].max(b1[0]), b0[1].max(b1[1])];
                if !bounds_overlap((a_min, a_max), (b_min, b_max), SNAP_EPS * 2.0) {
                    continue;
                }
                for (t, u) in segment_intersections(a0, a1, b0, b1) {
                    cut_params[i].push(t);
                    cut_params[j].push(u);
                }
            }
        }
    }

    let mut out = Vec::new();
    for i in 0..n_segs {
        let a0 = poly[i];
        let a1 = poly[i + 1];
        let mut cuts = cut_params[i].clone();
        cuts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        cuts.dedup_by(|x, y| (*x - *y).abs() <= 1e-4);

        let cut_at_0 = cuts.iter().any(|&t| t <= 1e-4);
        let mut pts_t = vec![0.0];
        for &t in &cuts {
            if t > 1e-4 && t < 1.0 - 1e-4 {
                pts_t.push(t);
            }
        }
        pts_t.push(1.0);

        for (w_idx, w) in pts_t.windows(2).enumerate() {
            let p0 = [
                a0[0] + (a1[0] - a0[0]) * w[0],
                a0[1] + (a1[1] - a0[1]) * w[0],
            ];
            let p1 = [
                a0[0] + (a1[0] - a0[0]) * w[1],
                a0[1] + (a1[1] - a0[1]) * w[1],
            ];
            if dist_sq(p0, p1) <= SNAP_EPS * SNAP_EPS {
                continue;
            }
            let is_cut = if w_idx == 0 { cut_at_0 } else { true };
            out.push((p0, p1, is_cut));
        }
    }
    out
}

pub(super) fn translate_path_commands(
    commands: &[PathCommand],
    dx: f64,
    dy: f64,
) -> Vec<PathCommand> {
    if dx.abs() <= 1e-12 && dy.abs() <= 1e-12 {
        return commands.to_vec();
    }
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
                x: x + dx,
                y: y + dy,
            },
            PathCommand::LineTo { x, y } => PathCommand::LineTo {
                x: x + dx,
                y: y + dy,
            },
            PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
                cx: cx + dx,
                cy: cy + dy,
                x: x + dx,
                y: y + dy,
            },
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => PathCommand::CubicTo {
                cx1: cx1 + dx,
                cy1: cy1 + dy,
                cx2: cx2 + dx,
                cy2: cy2 + dy,
                x: x + dx,
                y: y + dy,
            },
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

pub(super) fn shift_gradient(mut gradient: GradientPaint, dx: f64, dy: f64) -> GradientPaint {
    gradient.matrix = affine_multiply([1.0, 0.0, 0.0, 1.0, dx, dy], gradient.matrix);
    gradient
}

pub(super) fn build_combined_shape_element(
    template: &Element,
    id: Id,
    world_cmds: Vec<PathCommand>,
    fill_rule: FillRule,
    fp: &FillPaint,
    sp: Option<&StrokePaint>,
) -> Element {
    if template.merge {
        return build_fill_element(template, id, world_cmds, fill_rule, fp, true);
    }
    let (ox, oy, local_cmds) = if let Ok(Some(bounds)) = crate::path_bounds(&world_cmds)
        && bounds.x_min.is_finite()
        && bounds.y_min.is_finite()
    {
        let ox = bounds.x_min;
        let oy = bounds.y_min;
        (ox, oy, translate_path_commands(&world_cmds, -ox, -oy))
    } else {
        (0.0, 0.0, world_cmds)
    };
    let fill_color = match fp {
        FillPaint::Solid(color) => *color,
        FillPaint::Gradient(grad) => match &template.kind {
            ElementKind::PaintedPath { fill, .. } => *fill,
            _ => grad.stops.first().map_or([0, 0, 0, 255], |s| s.color),
        },
    };
    let fill_gradient = match fp {
        FillPaint::Solid(_) => None,
        FillPaint::Gradient(grad) => Some(shift_gradient(world_gradient(template, grad), -ox, -oy)),
    };
    let (stroke_color, stroke_width, stroke_gradient) = match sp {
        Some(s) => (
            s.color,
            s.width,
            s.gradient
                .as_ref()
                .map(|g| shift_gradient(world_gradient(template, g), -ox, -oy)),
        ),
        None => ([0; 4], 0.0, None),
    };
    let kind = if fill_gradient.is_some() || stroke_gradient.is_some() {
        ElementKind::PaintedPath {
            commands: local_cmds,
            fill_rule,
            fill: fill_color,
            stroke: stroke_color,
            fill_gradient,
            stroke_gradient,
            stroke_width,
        }
    } else {
        ElementKind::Path {
            commands: local_cmds,
            fill_rule,
            fill: fill_color,
            stroke: stroke_color,
            stroke_width,
        }
    };
    Element {
        id,
        name: template.name.clone(),
        transform: Transform {
            x: ox,
            y: oy,
            alpha: template.transform.alpha,
            color_transform: template.transform.color_transform,
            ..Transform::default()
        },
        kind,
        scripts: template.scripts.clone(),
        accessibility: template.accessibility.clone(),
        visible: template.visible,
        cache_as_bitmap: template.cache_as_bitmap,
        blend_mode: template.blend_mode,
        filters: template.filters.clone(),
        merge: false,
        instance_behavior: None,
    }
}

/// Combine two or more vector shapes using `BooleanOp` (`Modify ▸ Combine objects`).
///
/// - `Union`: merges all selected shapes into unified shape(s) using the bottom-most shape's
///   style.
/// - `Intersection`: keeps the region common to all selected shapes.
/// - `Difference` (`Punch`): subtracts the top-most selected shape from each underlying selected
///   shape.
/// - `Xor` (`Crop`): clips each underlying selected shape to the top-most selected shape's
///   boundary (`Intersection` of each underlying shape with the top-most shape).
pub fn combine_elements_boolean(
    selected: &[Element],
    op: BooleanOp,
    crop_by_top: bool,
) -> Option<Vec<Element>> {
    if selected.len() < 2 {
        return None;
    }
    let fallback_merge_stroke: Option<(Element, StrokePaint)> = selected
        .iter()
        .filter(|el| is_merge_stroke(el))
        .find_map(|el| {
            let mut sp = extract_stroke_paint(&el.kind)?;
            sp.width *= transform_scale_factor(&el.transform);
            Some(((*el).clone(), sp))
        });
    let closed_non_merge_stroke_count = selected
        .iter()
        .filter(|el| {
            !is_merge_stroke(el)
                && element_world_commands(el)
                    .is_some_and(|(cmds, _)| !flatten_closed_rings(&cmds).is_empty())
        })
        .count();

    let mut shapes = Vec::with_capacity(selected.len());
    for el in selected {
        if closed_non_merge_stroke_count >= 2 && is_merge_stroke(el) {
            continue;
        }
        let (cmds, rule) = element_world_commands(el)?;
        let rings = flatten_closed_rings(&cmds);
        if rings.is_empty() {
            if closed_non_merge_stroke_count >= 2 {
                continue;
            }
            return None;
        }
        shapes.push((el, rings, rule));
    }
    if shapes.len() < 2 {
        return None;
    }

    if crop_by_top || op == BooleanOp::Difference {
        let (top_el, top_rings, top_rule) = shapes.pop()?;
        let _ = top_el;
        let clip_op = if crop_by_top {
            BooleanOp::Intersection
        } else {
            BooleanOp::Difference
        };
        let mut out = Vec::new();
        let mut out_strokes = Vec::new();
        for (base_el, base_rings, base_rule) in shapes {
            let pieces = boolean_rings(&base_rings, base_rule, &top_rings, top_rule, clip_op);
            let fp = extract_fill_paint(&base_el.kind).unwrap_or(FillPaint::Solid([0; 4]));
            let mut sp = extract_stroke_paint(&base_el.kind);
            if let Some(sp) = &mut sp {
                sp.width *= transform_scale_factor(&base_el.transform);
            }
            for (idx, cmds) in pieces.into_iter().enumerate() {
                let id = if idx == 0 {
                    base_el.id.clone()
                } else {
                    new_id("element")
                };
                if base_el.merge
                    && let Some((ref stroke_tpl, ref msp)) = fallback_merge_stroke
                {
                    for contour_cmds in split_commands_into_contours(&cmds) {
                        out_strokes.push(build_stroke_element(
                            stroke_tpl,
                            new_id("element"),
                            contour_cmds,
                            msp,
                            true,
                        ));
                    }
                }
                let el = build_combined_shape_element(
                    base_el,
                    id,
                    cmds,
                    FillRule::NonZero,
                    &fp,
                    sp.as_ref(),
                );
                out.push(el);
            }
        }
        out.extend(out_strokes);
        return Some(out);
    }

    // Union or Intersection across all selected shapes sequentially.
    let (first_el, mut acc_rings, mut acc_rule) = shapes[0].clone();
    let mut acc_cmds = Vec::new();
    for (el, rings, rule) in shapes.into_iter().skip(1) {
        let _ = el;
        let pieces = boolean_rings(&acc_rings, acc_rule, &rings, rule, op);
        acc_cmds.clear();
        for p in pieces {
            acc_cmds.extend(p);
        }
        acc_rings = flatten_closed_rings(&acc_cmds);
        acc_rule = FillRule::NonZero;
        if acc_rings.is_empty() {
            return Some(Vec::new());
        }
    }

    let final_pieces = assemble_shells_and_holes(acc_rings);
    let fp = extract_fill_paint(&first_el.kind).unwrap_or(FillPaint::Solid([0; 4]));
    let mut sp = extract_stroke_paint(&first_el.kind);
    if let Some(sp) = &mut sp {
        sp.width *= transform_scale_factor(&first_el.transform);
    }
    let mut out = Vec::with_capacity(final_pieces.len());
    let mut out_strokes = Vec::new();
    for (idx, cmds) in final_pieces.into_iter().enumerate() {
        let id = if idx == 0 {
            first_el.id.clone()
        } else {
            new_id("element")
        };
        if first_el.merge
            && let Some((ref stroke_tpl, ref msp)) = fallback_merge_stroke
        {
            for contour_cmds in split_commands_into_contours(&cmds) {
                out_strokes.push(build_stroke_element(
                    stroke_tpl,
                    new_id("element"),
                    contour_cmds,
                    msp,
                    true,
                ));
            }
        }
        let el =
            build_combined_shape_element(first_el, id, cmds, FillRule::NonZero, &fp, sp.as_ref());
        out.push(el);
    }
    out.extend(out_strokes);
    Some(out)
}

/// Extract the exact sub-arc `[t0, t1]` (`0.0 <= t0 < t1 <= 1.0`) of a single `LineTo`,
/// `QuadraticTo`, or `CubicTo` segment starting at `p0` using De Casteljau subdivision (`T-104`).
pub fn sub_arc_bezier_command(
    p0: [f64; 2],
    cmd: PathCommand,
    t0: f64,
    t1: f64,
) -> ([f64; 2], PathCommand) {
    let lerp2 = |a: [f64; 2], b: [f64; 2], t: f64| -> [f64; 2] {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    };
    let t0 = t0.clamp(0.0, 1.0);
    let t1 = t1.clamp(t0, 1.0);
    let u = if (1.0 - t0).abs() > 1e-12 {
        ((t1 - t0) / (1.0 - t0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    match cmd {
        PathCommand::LineTo { x, y } | PathCommand::MoveTo { x, y } => {
            let p1 = [x, y];
            let start = lerp2(p0, p1, t0);
            let end = lerp2(p0, p1, t1);
            (
                start,
                PathCommand::LineTo {
                    x: end[0],
                    y: end[1],
                },
            )
        }
        PathCommand::QuadraticTo { cx, cy, x, y } => {
            let c = [cx, cy];
            let p2 = [x, y];
            // First split at t0 -> keep right piece [r0, r1, p2]
            let a0 = lerp2(p0, c, t0);
            let a1 = lerp2(c, p2, t0);
            let r0 = lerp2(a0, a1, t0);
            let r1 = a1;
            // Second split of [r0, r1, p2] at u -> keep left piece [r0, b0, end]
            let b0 = lerp2(r0, r1, u);
            let b1 = lerp2(r1, p2, u);
            let end = lerp2(b0, b1, u);
            (
                r0,
                PathCommand::QuadraticTo {
                    cx: b0[0],
                    cy: b0[1],
                    x: end[0],
                    y: end[1],
                },
            )
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            let c1 = [cx1, cy1];
            let c2 = [cx2, cy2];
            let p3 = [x, y];
            // First split at t0 -> keep right piece [r0, r1, r2, p3]
            let a0 = lerp2(p0, c1, t0);
            let a1 = lerp2(c1, c2, t0);
            let a2 = lerp2(c2, p3, t0);
            let b0 = lerp2(a0, a1, t0);
            let b1 = lerp2(a1, a2, t0);
            let r0 = lerp2(b0, b1, t0);
            let r1 = b1;
            let r2 = a2;
            // Second split of [r0, r1, r2, p3] at u -> keep left piece [r0, d0, e0, end]
            let d0 = lerp2(r0, r1, u);
            let d1 = lerp2(r1, r2, u);
            let d2 = lerp2(r2, p3, u);
            let e0 = lerp2(d0, d1, u);
            let e1 = lerp2(d1, d2, u);
            let end = lerp2(e0, e1, u);
            (
                r0,
                PathCommand::CubicTo {
                    cx1: d0[0],
                    cy1: d0[1],
                    cx2: e0[0],
                    cy2: e0[1],
                    x: end[0],
                    y: end[1],
                },
            )
        }
        PathCommand::Close => (p0, PathCommand::Close),
    }
}

pub(super) fn eval_command_at(p0: [f64; 2], cmd: PathCommand, t: f64) -> [f64; 2] {
    let (_, sub) = sub_arc_bezier_command(p0, cmd, 0.0, t);
    match sub {
        PathCommand::MoveTo { x, y }
        | PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. } => [x, y],
        PathCommand::Close => p0,
    }
}

/// Split a path containing `LineTo`, `QuadraticTo`, and `CubicTo` segments against cutting
/// segments while preserving exact `QuadraticTo` / `CubicTo` Bézier sub-arcs (`T-104`).
pub fn split_bezier_path_with_segments(
    commands: &[PathCommand],
    cutters: &[([f64; 2], [f64; 2])],
) -> Vec<Vec<PathCommand>> {
    if commands.is_empty() {
        return Vec::new();
    }
    const SAMPLES: usize = 32;
    let mut pieces: Vec<Vec<PathCommand>> = Vec::new();
    let mut current: Vec<PathCommand> = Vec::new();
    let mut cursor = [0.0, 0.0];
    let mut subpath_start = [0.0, 0.0];

    for &cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                if current.len() >= 2 {
                    pieces.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                cursor = [x, y];
                subpath_start = [x, y];
                current.push(PathCommand::MoveTo { x, y });
            }
            PathCommand::LineTo { .. }
            | PathCommand::QuadraticTo { .. }
            | PathCommand::CubicTo { .. } => {
                let p0 = cursor;
                let end_pt = eval_command_at(p0, cmd, 1.0);
                let mut splits = vec![0.0_f64, 1.0_f64];
                let steps = if matches!(cmd, PathCommand::LineTo { .. }) {
                    1
                } else {
                    SAMPLES
                };
                for s in 0..steps {
                    let ta = s as f64 / steps as f64;
                    let tb = (s + 1) as f64 / steps as f64;
                    let pa = eval_command_at(p0, cmd, ta);
                    let pb = eval_command_at(p0, cmd, tb);
                    for &(ca, cb) in cutters {
                        for (u, _) in segment_intersections(pa, pb, ca, cb) {
                            let t_hit = ta + (tb - ta) * u;
                            if t_hit > 1e-4 && t_hit < 1.0 - 1e-4 {
                                splits.push(t_hit);
                            }
                        }
                    }
                }
                splits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                splits.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

                for pair in splits.windows(2) {
                    let (t0, t1) = (pair[0], pair[1]);
                    if t1 - t0 <= 1e-5 {
                        continue;
                    }
                    let (seg_start, sub_cmd) = sub_arc_bezier_command(p0, cmd, t0, t1);
                    if current.is_empty() {
                        current.push(PathCommand::MoveTo {
                            x: seg_start[0],
                            y: seg_start[1],
                        });
                    }
                    current.push(sub_cmd);
                    if t1 < 1.0 - 1e-4 && current.len() >= 2 {
                        pieces.push(std::mem::take(&mut current));
                    }
                }
                cursor = end_pt;
            }
            PathCommand::Close => {
                if (cursor[0] - subpath_start[0]).hypot(cursor[1] - subpath_start[1]) > 1e-6 {
                    current.push(PathCommand::LineTo {
                        x: subpath_start[0],
                        y: subpath_start[1],
                    });
                }
                cursor = subpath_start;
            }
        }
    }
    if current.len() >= 2 {
        pieces.push(current);
    }
    pieces
}
