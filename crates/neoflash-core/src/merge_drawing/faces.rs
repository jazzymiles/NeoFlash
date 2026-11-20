use super::*;

/// Trace closed cycles from a set of active directed half-edges `(u, v)`.
///
/// At each vertex `v` arrived via `u -> v`, we look in the direction of `v -> u` and rotate
/// clockwise (decreasing angle) to pick the first outgoing active half-edge `v -> w`. This keeps
/// the minimal interior face to the left of `u -> v -> w`.
pub(super) fn trace_half_edge_cycles(
    vertices: &[[f64; 2]],
    half_edges: &[(usize, usize)],
) -> Vec<Vec<usize>> {
    trace_half_edge_cycles_with_chains(vertices, half_edges, &[])
}

/// Trace closed boundary cycles. `chains[idx]`, when present and non-empty, lists the interior
/// points of half-edge `idx` (an untouched run of an existing contour kept verbatim by the
/// windowed boolean); turning angles at its ends use its first/last interior segment.
pub(super) fn trace_half_edge_cycles_with_chains(
    vertices: &[[f64; 2]],
    half_edges: &[(usize, usize)],
    chains: &[Vec<[f64; 2]>],
) -> Vec<Vec<usize>> {
    if half_edges.is_empty() {
        return Vec::new();
    }
    let chain_of = |idx: usize| chains.get(idx).filter(|c| !c.is_empty());
    let n_verts = vertices.len();
    let n_he = half_edges.len();

    // Iteratively prune dead-end half-edges (where `u` has no incoming active half-edge or `v`
    // has no outgoing active half-edge) so dangling spurs can never divert a cycle walk into a
    // dead end.
    let mut in_deg = vec![0u32; n_verts];
    let mut out_deg = vec![0u32; n_verts];
    let mut in_edges: Vec<Vec<usize>> = vec![Vec::new(); n_verts];
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n_verts];
    for (idx, &(u, v)) in half_edges.iter().enumerate() {
        out_deg[u] += 1;
        in_deg[v] += 1;
        out_edges[u].push(idx);
        in_edges[v].push(idx);
    }
    let mut active = vec![true; n_he];
    let mut queue: Vec<usize> = (0..n_he)
        .filter(|&idx| {
            let (u, v) = half_edges[idx];
            in_deg[u] == 0 || out_deg[v] == 0
        })
        .collect();
    for &idx in &queue {
        active[idx] = false;
    }
    let mut q_head = 0usize;
    while q_head < queue.len() {
        let idx = queue[q_head];
        q_head += 1;
        let (u, v) = half_edges[idx];
        out_deg[u] = out_deg[u].saturating_sub(1);
        in_deg[v] = in_deg[v].saturating_sub(1);
        if out_deg[u] == 0 {
            for &in_he in &in_edges[u] {
                if active[in_he] {
                    active[in_he] = false;
                    queue.push(in_he);
                }
            }
        }
        if in_deg[v] == 0 {
            for &out_he in &out_edges[v] {
                if active[out_he] {
                    active[out_he] = false;
                    queue.push(out_he);
                }
            }
        }
    }

    let mut outgoing: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n_verts];
    for (idx, &(u, v)) in half_edges.iter().enumerate() {
        if !active[idx] {
            continue;
        }
        let a = vertices[u];
        let b = chain_of(idx).map_or(vertices[v], |c| c[0]);
        let dy = if (b[1] - a[1]).abs() == 0.0 {
            0.0
        } else {
            b[1] - a[1]
        };
        let angle = dy.atan2(b[0] - a[0]);
        outgoing[u].push((idx, angle));
    }
    for list in &mut outgoing {
        list.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    let mut visited = vec![false; n_he];
    // Position of each source vertex inside the active walk stack so closing a cycle (including
    // figure-8 / kissing cycles at a shared pinch vertex) is O(1) and decomposes into simple rings.
    let mut vert_walk = vec![usize::MAX; n_verts];
    let mut vert_pos = vec![0usize; n_verts];
    let mut cycles = Vec::new();

    for start_idx in 0..n_he {
        if !active[start_idx] || visited[start_idx] {
            continue;
        }
        let mut cycle_edges = Vec::new();
        let mut next_opt = Some(start_idx);
        while let Some(curr) = next_opt.take() {
            if !visited[curr] {
                let (u_curr, v_curr) = half_edges[curr];
                visited[curr] = true;
                vert_walk[u_curr] = start_idx;
                vert_pos[u_curr] = cycle_edges.len();
                cycle_edges.push(curr);

                if vert_walk[v_curr] == start_idx {
                    let pos = vert_pos[v_curr];
                    let sub_cycle: Vec<usize> = cycle_edges.drain(pos..).collect();
                    for &he in &sub_cycle {
                        vert_walk[half_edges[he].0] = usize::MAX;
                    }
                    let points: usize = sub_cycle
                        .iter()
                        .map(|&he| 1 + chain_of(he).map_or(0, Vec::len))
                        .sum();
                    if points >= 3 {
                        cycles.push(sub_cycle);
                    }
                }
            }
            let Some(&last_he) = cycle_edges.last() else {
                break;
            };
            let (u, v) = half_edges[last_he];
            let list = &outgoing[v];
            if list.is_empty() {
                break;
            }
            let a = vertices[v];
            let b = chain_of(last_he).map_or(vertices[u], |c| c[c.len() - 1]);
            let dy = if (b[1] - a[1]).abs() == 0.0 {
                0.0
            } else {
                b[1] - a[1]
            };
            let rev_angle = dy.atan2(b[0] - a[0]);
            let idx_first_ge = list.partition_point(|&(_, ang)| ang < rev_angle - 1e-9);
            let chosen = (0..list.len())
                .map(|step| list[(idx_first_ge + list.len() - 1 - step) % list.len()].0)
                .find(|&he| !visited[he]);
            if let Some(next_he) = chosen {
                next_opt = Some(next_he);
            } else {
                break;
            }
        }
        for &he in &cycle_edges {
            vert_walk[half_edges[he].0] = usize::MAX;
        }
    }
    cycles
}

pub(super) fn simplify_ring_collinear(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut pts = ring.to_vec();
    // Strip zero-area backtracks `a -> b -> a` and collinear midpoints.
    loop {
        let n = pts.len();
        if n < 3 {
            return Vec::new();
        }
        let mut cleaned = Vec::with_capacity(n);
        let mut changed = false;
        for i in 0..n {
            let prev = pts[(i + n - 1) % n];
            let curr = pts[i];
            let next = pts[(i + 1) % n];
            if dist_sq(prev, next) <= SNAP_EPS * SNAP_EPS {
                changed = true;
                continue;
            }
            let (_, d) = point_to_segment_projection(curr, prev, next);
            if d <= 1e-3
                && (curr[0] - prev[0]) * (next[0] - curr[0])
                    + (curr[1] - prev[1]) * (next[1] - curr[1])
                    > 0.0
            {
                changed = true;
                continue;
            }
            cleaned.push(curr);
        }
        pts = cleaned;
        if !changed {
            break;
        }
    }
    pts
}

pub(super) fn ring_to_commands(ring: &[[f64; 2]]) -> Vec<PathCommand> {
    if ring.len() < 3 {
        return Vec::new();
    }
    let mut cmds = Vec::with_capacity(ring.len() + 1);
    cmds.push(PathCommand::MoveTo {
        x: ring[0][0],
        y: ring[0][1],
    });
    for pt in &ring[1..] {
        cmds.push(PathCommand::LineTo { x: pt[0], y: pt[1] });
    }
    cmds.push(PathCommand::Close);
    cmds
}

pub(super) fn polyline_to_commands(poly: &[[f64; 2]]) -> Vec<PathCommand> {
    if poly.len() < 2 {
        return Vec::new();
    }
    let closed = poly.len() >= 4 && dist_sq(poly[0], *poly.last().unwrap()) <= SNAP_EPS * SNAP_EPS;
    let slice = if closed {
        &poly[..poly.len() - 1]
    } else {
        poly
    };
    let mut cmds = Vec::with_capacity(slice.len() + usize::from(closed));
    cmds.push(PathCommand::MoveTo {
        x: slice[0][0],
        y: slice[0][1],
    });
    for pt in &slice[1..] {
        cmds.push(PathCommand::LineTo { x: pt[0], y: pt[1] });
    }
    if closed {
        cmds.push(PathCommand::Close);
    }
    cmds
}

pub(super) fn assemble_shells_and_holes(rings: Vec<Vec<[f64; 2]>>) -> Vec<Vec<PathCommand>> {
    type Shell = (Vec<[f64; 2]>, f64, ([f64; 2], [f64; 2]), Vec<Vec<[f64; 2]>>);
    let mut shells: Vec<Shell> = Vec::new();
    let mut holes: Vec<Vec<[f64; 2]>> = Vec::new();

    for raw_ring in rings {
        let ring = simplify_ring_collinear(&raw_ring);
        if ring.len() < 3 {
            continue;
        }
        let area = signed_ring_area(&ring);
        if area > AREA_EPS {
            if let Some(bbox) = ring_bounds(&ring) {
                shells.push((ring, area, bbox, Vec::new()));
            }
        } else if area < -AREA_EPS {
            holes.push(ring);
        }
    }

    // Sort shells from smallest positive area to largest so each hole attaches to its tightest
    // enclosing outer shell.
    shells.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Indexed so a blob with hundreds of holes and a long outer contour stays near-linear.
    let shell_segs: Vec<([f64; 2], [f64; 2])> = if holes.is_empty() {
        Vec::new()
    } else {
        shells
            .iter()
            .flat_map(|(shell, ..)| {
                (0..shell.len()).map(|i| (shell[i], shell[(i + 1) % shell.len()]))
            })
            .collect()
    };
    let shell_bvh = SegmentBvh::from_segments(&shell_segs);
    let mut prepared_shells: Vec<Option<PreparedRings>> = (0..shells.len()).map(|_| None).collect();
    for hole in holes {
        let hole_area = signed_ring_area(&hole).abs();
        let n = hole.len();
        let mut candidates = Vec::with_capacity(6);
        if let Some(mid_pt) = (0..n)
            .map(|i| {
                let a = hole[i];
                let b = hole[(i + 1) % n];
                [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
            })
            .find(|&pt| !shell_bvh.any_within_distance(&shell_segs, pt, SNAP_EPS * 2.0))
        {
            candidates.push(mid_pt);
        }
        if let Some(v_pt) = hole
            .iter()
            .copied()
            .find(|&pt| !shell_bvh.any_within_distance(&shell_segs, pt, SNAP_EPS * 2.0))
        {
            candidates.push(v_pt);
        }
        candidates.push(hole[0]);
        if n > 1 {
            candidates.push(hole[n / 2]);
        }
        let found = candidates
            .iter()
            .find_map(|&sample| {
                shells.iter().enumerate().position(
                    |(idx, (shell, shell_area, (s_min, s_max), _))| {
                        if !(*shell_area > hole_area
                            && sample[0] >= s_min[0] - SNAP_EPS
                            && sample[0] <= s_max[0] + SNAP_EPS
                            && sample[1] >= s_min[1] - SNAP_EPS
                            && sample[1] <= s_max[1] + SNAP_EPS)
                        {
                            return false;
                        }
                        if shell.len() < 64 {
                            return ring_winding_number(sample, shell) != 0;
                        }
                        prepared_shells[idx]
                            .get_or_insert_with(|| {
                                PreparedRings::new(std::slice::from_ref(shell), FillRule::NonZero)
                            })
                            .contains(sample)
                    },
                )
            })
            .or_else(|| {
                let (h_min, h_max) = ring_bounds(&hole)?;
                shells
                    .iter()
                    .position(|(_, shell_area, (s_min, s_max), _)| {
                        *shell_area > hole_area
                            && h_min[0] >= s_min[0] - SNAP_EPS * 4.0
                            && h_max[0] <= s_max[0] + SNAP_EPS * 4.0
                            && h_min[1] >= s_min[1] - SNAP_EPS * 4.0
                            && h_max[1] <= s_max[1] + SNAP_EPS * 4.0
                    })
            });
        if let Some(idx) = found {
            shells[idx].3.push(hole);
        }
    }

    let mut result = Vec::with_capacity(shells.len());
    for (shell, _, _, shell_holes) in shells {
        let mut cmds = ring_to_commands(&shell);
        for hole in shell_holes {
            cmds.extend(ring_to_commands(&hole));
        }
        if !cmds.is_empty() {
            result.push(cmds);
        }
    }
    result
}

/// Multi-edge planar-face Paint Bucket (`T-104`): builds a planar half-edge arrangement from all
/// visible strokes and fill boundaries in `elements` (bridging endpoint gaps up to `gap_tolerance`)
/// and extracts the minimal enclosed polygon face containing `seed`.
pub fn paint_bucket_planar_face(
    elements: &[Element],
    seed: [f64; 2],
    gap_tolerance: f64,
) -> Result<Option<Vec<PathCommand>>, GeometryError> {
    paint_bucket_planar_face_in_metric(elements, seed, [1., 0., 0., 1., 0., 0.], gap_tolerance)
}

/// Extract the planar face containing `seed`, measuring endpoint gaps after `local_to_metric`.
/// This keeps a pixel-sized gap setting exact through rotated, skewed and non-uniformly scaled
/// edit-in-place contexts while returning geometry in authored coordinates.
pub fn paint_bucket_planar_face_in_metric(
    elements: &[Element],
    seed: [f64; 2],
    local_to_metric: [f64; 6],
    gap_tolerance: f64,
) -> Result<Option<Vec<PathCommand>>, GeometryError> {
    if seed.iter().any(|value| !value.is_finite())
        || local_to_metric.iter().any(|value| !value.is_finite())
        || invert_affine_matrix(local_to_metric).is_none()
        || !gap_tolerance.is_finite()
        || gap_tolerance < 0.0
    {
        return Err(invalid_merge_geometry(
            "paint bucket seed and metric must be finite and invertible, and the gap tolerance nonnegative",
        ));
    }
    let mut raw_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for el in elements {
        if !el.visible {
            continue;
        }
        let Some((cmds, rule)) = element_world_commands(el) else {
            continue;
        };
        if is_merge_stroke(el) || extract_stroke_paint(&el.kind).is_some() {
            for poly in checked_flatten_stroke_polylines(&cmds)? {
                for pair in poly.windows(2) {
                    if dist_sq(pair[0], pair[1]) > 1e-8 {
                        raw_segs.push((pair[0], pair[1]));
                        if raw_segs.len() > MAX_BUCKET_RAW_SEGMENTS {
                            return Err(GeometryError::ComplexityLimit);
                        }
                    }
                }
            }
        }
        if is_merge_fill(el) || extract_fill_paint(&el.kind).is_some() {
            let rings = checked_flatten_closed_rings(&cmds)?;
            raw_segs.extend(fill_boundary_segments(&rings, rule));
            if raw_segs.len() > MAX_BUCKET_RAW_SEGMENTS {
                return Err(GeometryError::ComplexityLimit);
            }
        }
    }
    if raw_segs.len() < 3 {
        return Ok(None);
    }

    // Bridge small open endpoint gaps up to `gap_tolerance`.
    let tol = gap_tolerance.max(0.0);
    if tol > 1e-6 {
        let mut endpoints: Vec<[f64; 2]> = Vec::with_capacity(raw_segs.len() * 2);
        for &(a, b) in &raw_segs {
            endpoints.push(a);
            endpoints.push(b);
        }
        let metric_endpoints: Vec<_> = endpoints
            .iter()
            .map(|point| transform_point(local_to_metric, point[0], point[1]))
            .collect();
        let endpoint_checks = endpoints
            .len()
            .checked_mul(endpoints.len())
            .and_then(|checks| checks.checked_mul(2))
            .ok_or(GeometryError::ComplexityLimit)?;
        if endpoint_checks > MAX_BUCKET_QUADRATIC_CHECKS {
            return Err(GeometryError::ComplexityLimit);
        }
        let mut bridges = Vec::new();
        for i in 0..endpoints.len() {
            let p = endpoints[i];
            let metric_p = metric_endpoints[i];
            let deg = metric_endpoints
                .iter()
                .filter(|&&q| dist_sq(metric_p, q) <= 1e-4)
                .count();
            if deg == 1 {
                let mut best: Option<([f64; 2], f64)> = None;
                for (j, &q) in endpoints.iter().enumerate() {
                    if i == j || (i / 2) == (j / 2) {
                        continue;
                    }
                    let metric_q = metric_endpoints[j];
                    let d = (metric_p[0] - metric_q[0]).hypot(metric_p[1] - metric_q[1]);
                    if d > 1e-4 && d <= tol && best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((q, d));
                    }
                }
                if let Some((q, _)) = best {
                    bridges.push((p, q));
                }
            }
        }
        raw_segs.extend(bridges);
        if raw_segs.len() > MAX_BUCKET_RAW_SEGMENTS {
            return Err(GeometryError::ComplexityLimit);
        }
    }

    // Subdivide segments at all pairwise intersections.
    let n = raw_segs.len();
    let pair_checks = n
        .checked_mul(n.saturating_sub(1))
        .map(|checks| checks / 2)
        .ok_or(GeometryError::ComplexityLimit)?;
    if pair_checks > MAX_BUCKET_QUADRATIC_CHECKS {
        return Err(GeometryError::ComplexityLimit);
    }
    let mut split_params: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
    let mut split_param_count = n.saturating_mul(2);
    for i in 0..n {
        for j in (i + 1)..n {
            let (a0, a1) = raw_segs[i];
            let (b0, b1) = raw_segs[j];
            for (t, u) in segment_intersections(a0, a1, b0, b1) {
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                    split_params[i].push(t);
                    split_params[j].push(u);
                    split_param_count = split_param_count
                        .checked_add(2)
                        .ok_or(GeometryError::ComplexityLimit)?;
                    if split_param_count > MAX_BUCKET_PLANAR_EDGES * 2 {
                        return Err(GeometryError::ComplexityLimit);
                    }
                }
            }
        }
    }

    // Snap vertices within 0.05 metric px and build directed half-edges. The planar graph remains
    // in authored coordinates, but its topology must not close a visibly large gap merely because
    // an in-place parent stretches the authored coordinate system.
    const SNAP: f64 = 0.05;
    let mut vertices: Vec<[f64; 2]> = Vec::new();
    let mut metric_vertices: Vec<[f64; 2]> = Vec::new();
    let inv_cell = 1.0 / (SNAP * 2.0);
    let mut vertex_grid: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::with_capacity(n.saturating_mul(2));
    let mut find_or_insert = |pt: [f64; 2]| -> usize {
        let metric_pt = transform_point(local_to_metric, pt[0], pt[1]);
        let gx = (metric_pt[0] * inv_cell).floor() as i64;
        let gy = (metric_pt[1] * inv_cell).floor() as i64;
        let mut best = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if let Some(bucket) = vertex_grid.get(&(gx + dx, gy + dy)) {
                    for &idx in bucket {
                        if best.is_none_or(|previous| idx < previous)
                            && dist_sq(metric_vertices[idx], metric_pt) <= SNAP * SNAP
                        {
                            best = Some(idx);
                        }
                    }
                }
            }
        }
        if let Some(idx) = best {
            idx
        } else {
            let idx = vertices.len();
            vertices.push(pt);
            metric_vertices.push(metric_pt);
            vertex_grid.entry((gx, gy)).or_default().push(idx);
            idx
        }
    };

    let mut adj: Vec<Vec<usize>> = Vec::new();
    let mut planar_edges = 0usize;
    for (i, &(a, b)) in raw_segs.iter().enumerate() {
        let params = &mut split_params[i];
        params.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        params.dedup_by(|x, y| (*x - *y).abs() < 1e-4);
        for pair in params.windows(2) {
            let p0 = [
                a[0] + (b[0] - a[0]) * pair[0],
                a[1] + (b[1] - a[1]) * pair[0],
            ];
            let p1 = [
                a[0] + (b[0] - a[0]) * pair[1],
                a[1] + (b[1] - a[1]) * pair[1],
            ];
            let u = find_or_insert(p0);
            let v = find_or_insert(p1);
            if u == v {
                continue;
            }
            planar_edges = planar_edges
                .checked_add(1)
                .ok_or(GeometryError::ComplexityLimit)?;
            if planar_edges > MAX_BUCKET_PLANAR_EDGES {
                return Err(GeometryError::ComplexityLimit);
            }
            if adj.len() <= u.max(v) {
                adj.resize(u.max(v) + 1, Vec::new());
            }
            if !adj[u].contains(&v) {
                adj[u].push(v);
            }
            if !adj[v].contains(&u) {
                adj[v].push(u);
            }
        }
    }

    // Iteratively prune degree-1 dangling filaments.
    loop {
        let mut pruned = false;
        for u in 0..adj.len() {
            if adj[u].len() == 1 {
                let v = adj[u].pop().unwrap();
                adj[v].retain(|&x| x != u);
                pruned = true;
            }
        }
        if !pruned {
            break;
        }
    }

    // Sort outgoing neighbors at each vertex by angle.
    for (u, nbrs) in adj.iter_mut().enumerate() {
        let pu = vertices[u];
        nbrs.sort_by(|&a, &b| {
            let ang_a = (vertices[a][1] - pu[1]).atan2(vertices[a][0] - pu[0]);
            let ang_b = (vertices[b][1] - pu[1]).atan2(vertices[b][0] - pu[0]);
            ang_a
                .partial_cmp(&ang_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Trace all minimal planar faces and pick the smallest positive-area face containing `seed`.
    let mut visited_half: std::collections::HashSet<(usize, usize)> =
        std::collections::HashSet::new();
    let mut best_face: Option<(f64, Vec<[f64; 2]>)> = None;

    for u in 0..adj.len() {
        for &v in &adj[u] {
            if visited_half.contains(&(u, v)) {
                continue;
            }
            let mut cycle = Vec::new();
            let (mut curr_u, mut curr_v) = (u, v);
            let mut valid = true;
            for _ in 0..=vertices.len() * 2 {
                if !visited_half.insert((curr_u, curr_v)) {
                    valid = false;
                    break;
                }
                cycle.push(vertices[curr_u]);
                let nbrs = &adj[curr_v];
                if nbrs.len() < 2 {
                    valid = false;
                    break;
                }
                let Some(pos) = nbrs.iter().position(|&w| w == curr_u) else {
                    valid = false;
                    break;
                };
                // Next clockwise half-edge around `curr_v` encloses the interior face to the left.
                let next_w = nbrs[(pos + nbrs.len() - 1) % nbrs.len()];
                curr_u = curr_v;
                curr_v = next_w;
                if (curr_u, curr_v) == (u, v) {
                    break;
                }
            }
            if !valid || cycle.len() < 3 || (curr_u, curr_v) != (u, v) {
                continue;
            }
            let mut signed_area2 = 0.0;
            for i in 0..cycle.len() {
                let p = cycle[i];
                let q = cycle[(i + 1) % cycle.len()];
                signed_area2 += p[0] * q[1] - q[0] * p[1];
            }
            let area = 0.5 * signed_area2.abs();
            if area <= 1e-2 {
                continue;
            }
            let mut ring = cycle.clone();
            ring.push(cycle[0]);
            if point_in_rings(seed, &[ring.clone()], FillRule::EvenOdd)
                && best_face.as_ref().is_none_or(|(best_a, _)| area < *best_a)
            {
                best_face = Some((area, ring));
            }
        }
    }

    let Some((_, ring)) = best_face else {
        return Ok(None);
    };
    let mut cmds = Vec::with_capacity(ring.len() + 1);
    cmds.push(PathCommand::MoveTo {
        x: ring[0][0],
        y: ring[0][1],
    });
    for &[x, y] in &ring[1..ring.len().saturating_sub(1)] {
        cmds.push(PathCommand::LineTo { x, y });
    }
    cmds.push(PathCommand::Close);
    Ok(Some(cmds))
}

/// Classic Flash Brush paint modes (`T-105`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushPaintMode {
    /// Paints over lines and fills on the same layer.
    #[default]
    PaintNormal,
    /// Paints fills and empty areas, leaving existing lines/strokes unaffected.
    PaintFills,
    /// Paints in blank areas of the Stage behind existing fills and strokes on the layer.
    PaintBehind,
    /// Paints only inside the currently selected fill shapes.
    PaintSelection,
    /// Paints only inside the fill region where the brush stroke started (`inside_seed`).
    PaintInside,
}

impl BrushPaintMode {
    pub const ALL: [Self; 5] = [
        Self::PaintNormal,
        Self::PaintFills,
        Self::PaintBehind,
        Self::PaintSelection,
        Self::PaintInside,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PaintNormal => "Paint Normal",
            Self::PaintFills => "Paint Fills",
            Self::PaintBehind => "Paint Behind",
            Self::PaintSelection => "Paint Selection",
            Self::PaintInside => "Paint Inside",
        }
    }
}

/// Apply a Merge-mode brush fill `brush` to `elements` according to `mode` (`T-105`).
pub fn apply_brush_stroke_with_mode(
    elements: &mut Vec<Element>,
    brush: Element,
    mode: BrushPaintMode,
    selected_ids: &[Id],
    inside_seed: Option<[f64; 2]>,
) -> Vec<Id> {
    match mode {
        BrushPaintMode::PaintNormal => apply_merge_shape_to_keyframe(elements, brush),
        BrushPaintMode::PaintFills => {
            // Preserve all existing strokes intact and merge the brush only with fills.
            let mut saved_strokes = Vec::new();
            let mut fill_only = Vec::new();
            for el in elements.drain(..) {
                if is_merge_stroke(&el) {
                    saved_strokes.push(el);
                } else {
                    fill_only.push(el);
                }
            }
            let ids = apply_merge_shape_to_keyframe(&mut fill_only, brush);
            fill_only.extend(saved_strokes);
            *elements = fill_only;
            ids
        }
        BrushPaintMode::PaintBehind => {
            // Subtract all existing fills from the brush contour and place behind existing art.
            let Some((mut brush_cmds, mut brush_rule)) = element_world_commands(&brush) else {
                return Vec::new();
            };
            for el in elements.iter() {
                if !is_merge_fill(el) {
                    continue;
                }
                let Some((el_cmds, el_rule)) = element_world_commands(el) else {
                    continue;
                };
                let pieces = boolean_paths(
                    &brush_cmds,
                    brush_rule,
                    &el_cmds,
                    el_rule,
                    BooleanOp::Difference,
                );
                brush_cmds.clear();
                for p in pieces {
                    brush_cmds.extend(p);
                }
                brush_rule = FillRule::NonZero;
                if brush_cmds.is_empty() {
                    return Vec::new();
                }
            }
            let fp = extract_fill_paint(&brush.kind).unwrap_or(FillPaint::Solid([0; 4]));
            let piece =
                build_fill_element(&brush, brush.id.clone(), brush_cmds, brush_rule, &fp, true);
            let id = piece.id.clone();
            elements.insert(0, piece);
            vec![id]
        }
        BrushPaintMode::PaintSelection => {
            if selected_ids.is_empty() {
                return Vec::new();
            }
            let Some((brush_cmds, brush_rule)) = element_world_commands(&brush) else {
                return Vec::new();
            };
            let mut mask_cmds = Vec::new();
            for el in elements.iter() {
                if selected_ids.contains(&el.id)
                    && is_merge_fill(el)
                    && let Some((c, _)) = element_world_commands(el)
                {
                    mask_cmds.extend(c);
                }
            }
            if mask_cmds.is_empty() {
                return Vec::new();
            }
            let clipped = boolean_paths(
                &brush_cmds,
                brush_rule,
                &mask_cmds,
                FillRule::NonZero,
                BooleanOp::Intersection,
            );
            let mut combined = Vec::new();
            for p in clipped {
                combined.extend(p);
            }
            if combined.is_empty() {
                return Vec::new();
            }
            let fp = extract_fill_paint(&brush.kind).unwrap_or(FillPaint::Solid([0; 4]));
            let clipped_brush = build_fill_element(
                &brush,
                brush.id.clone(),
                combined,
                FillRule::NonZero,
                &fp,
                true,
            );
            apply_merge_shape_to_keyframe(elements, clipped_brush)
        }
        BrushPaintMode::PaintInside => {
            let Some(seed) = inside_seed else {
                return apply_merge_shape_to_keyframe(elements, brush);
            };
            let Some((brush_cmds, brush_rule)) = element_world_commands(&brush) else {
                return Vec::new();
            };
            // Find the topmost fill containing `seed`.
            let target_fill = elements.iter().rev().find_map(|el| {
                if !is_merge_fill(el) {
                    return None;
                }
                let (cmds, rule) = element_world_commands(el)?;
                let rings = flatten_closed_rings(&cmds);
                point_in_rings(seed, &rings, rule).then_some((cmds, rule))
            });
            if let Some((target_cmds, target_rule)) = target_fill {
                let clipped = boolean_paths(
                    &brush_cmds,
                    brush_rule,
                    &target_cmds,
                    target_rule,
                    BooleanOp::Intersection,
                );
                let mut combined = Vec::new();
                for p in clipped {
                    combined.extend(p);
                }
                if combined.is_empty() {
                    return Vec::new();
                }
                let fp = extract_fill_paint(&brush.kind).unwrap_or(FillPaint::Solid([0; 4]));
                let clipped_brush = build_fill_element(
                    &brush,
                    brush.id.clone(),
                    combined,
                    FillRule::NonZero,
                    &fp,
                    true,
                );
                apply_merge_shape_to_keyframe(elements, clipped_brush)
            } else {
                // Started on empty stage: paint only in areas outside existing fills.
                apply_brush_stroke_with_mode(
                    elements,
                    brush,
                    BrushPaintMode::PaintBehind,
                    selected_ids,
                    None,
                )
            }
        }
    }
}
