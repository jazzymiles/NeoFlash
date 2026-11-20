use super::*;

/// Flat 2D bounding-volume hierarchy over line segments for sub-millisecond proximity and AABB queries.
pub(super) struct SegmentBvh {
    pub(super) indices: Vec<usize>,
    pub(super) nodes: Vec<BvhNode>,
}

#[derive(Clone, Copy)]
pub(super) struct BvhNode {
    pub(super) min: [f64; 2],
    pub(super) max: [f64; 2],
    pub(super) start: u32,
    pub(super) count: u32,
    pub(super) left: u32,
    pub(super) right: u32,
}

impl SegmentBvh {
    pub(super) fn from_segments(segments: &[([f64; 2], [f64; 2])]) -> Self {
        let n = segments.len();
        if n == 0 {
            return Self {
                indices: Vec::new(),
                nodes: Vec::new(),
            };
        }
        let item_bounds: Vec<([f64; 2], [f64; 2], [f64; 2])> = segments
            .iter()
            .map(|&(a, b)| {
                let min = [a[0].min(b[0]), a[1].min(b[1])];
                let max = [a[0].max(b[0]), a[1].max(b[1])];
                let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
                (min, max, center)
            })
            .collect();
        let mut indices: Vec<usize> = (0..n).collect();
        let mut nodes = Vec::with_capacity(n.saturating_mul(2) / 4 + 2);
        Self::build_node(&item_bounds, &mut indices, 0, n, &mut nodes);
        Self { indices, nodes }
    }

    pub(super) fn build_node(
        item_bounds: &[([f64; 2], [f64; 2], [f64; 2])],
        indices: &mut [usize],
        start: usize,
        end: usize,
        nodes: &mut Vec<BvhNode>,
    ) -> u32 {
        let node_idx = nodes.len() as u32;
        let mut min = [f64::INFINITY, f64::INFINITY];
        let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
        for &idx in &indices[start..end] {
            let (b_min, b_max, _) = item_bounds[idx];
            min[0] = min[0].min(b_min[0]);
            min[1] = min[1].min(b_min[1]);
            max[0] = max[0].max(b_max[0]);
            max[1] = max[1].max(b_max[1]);
        }
        let count = end - start;
        nodes.push(BvhNode {
            min,
            max,
            start: start as u32,
            count: count as u32,
            left: 0,
            right: 0,
        });
        if count <= 8 {
            return node_idx;
        }
        let axis = usize::from(max[1] - min[1] > max[0] - min[0]);
        let mid = start + count / 2;
        indices[start..end].select_nth_unstable_by(count / 2, |&i, &j| {
            item_bounds[i].2[axis].total_cmp(&item_bounds[j].2[axis])
        });
        let left = Self::build_node(item_bounds, indices, start, mid, nodes);
        let right = Self::build_node(item_bounds, indices, mid, end, nodes);
        nodes[node_idx as usize].count = 0;
        nodes[node_idx as usize].left = left;
        nodes[node_idx as usize].right = right;
        node_idx
    }

    #[inline]
    /// Whether any indexed segment's bounds overlap `bounds` expanded by `pad`.
    pub(super) fn any_bounds_overlap(
        &self,
        segments: &[([f64; 2], [f64; 2])],
        bounds: ([f64; 2], [f64; 2]),
        pad: f64,
    ) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !bounds_overlap((node.min, node.max), bounds, pad) {
                continue;
            }
            if node.count > 0 {
                let start = node.start as usize;
                let end = start + node.count as usize;
                if self.indices[start..end].iter().any(|&idx| {
                    let (a, b) = segments[idx];
                    bounds_overlap(
                        (
                            [a[0].min(b[0]), a[1].min(b[1])],
                            [a[0].max(b[0]), a[1].max(b[1])],
                        ),
                        bounds,
                        pad,
                    )
                }) {
                    return true;
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.right;
                stack[sp + 1] = node.left;
                sp += 2;
            }
        }
        false
    }

    /// Whether an endpoint in this index lies in `bounds` and satisfies `predicate`.
    ///
    /// This is the reverse-containment query used by Merge: after edge intersections and
    /// "A inside B" have been rejected, A and B interact only when a B boundary endpoint is
    /// inside A. Traversing the existing 2D BVH avoids scanning every ring in a large moved batch
    /// for every element on the keyframe.
    pub(super) fn any_endpoint_matching(
        &self,
        segments: &[([f64; 2], [f64; 2])],
        bounds: ([f64; 2], [f64; 2]),
        pad: f64,
        mut predicate: impl FnMut([f64; 2]) -> bool,
    ) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let contains = |point: [f64; 2]| {
            point[0] >= bounds.0[0] - pad
                && point[0] <= bounds.1[0] + pad
                && point[1] >= bounds.0[1] - pad
                && point[1] <= bounds.1[1] + pad
        };
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !bounds_overlap((node.min, node.max), bounds, pad) {
                continue;
            }
            if node.count > 0 {
                let start = node.start as usize;
                let end = start + node.count as usize;
                for &idx in &self.indices[start..end] {
                    let (a, b) = segments[idx];
                    if (contains(a) && predicate(a)) || (contains(b) && predicate(b)) {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.right;
                stack[sp + 1] = node.left;
                sp += 2;
            }
        }
        false
    }

    pub(super) fn bounds_overlap_indices(
        &self,
        segments: &[([f64; 2], [f64; 2])],
        bounds: ([f64; 2], [f64; 2]),
        pad: f64,
    ) -> Vec<usize> {
        if self.nodes.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !bounds_overlap((node.min, node.max), bounds, pad) {
                continue;
            }
            if node.count > 0 {
                let start = node.start as usize;
                let end = start + node.count as usize;
                result.extend(self.indices[start..end].iter().copied().filter(|&index| {
                    let (a, b) = segments[index];
                    bounds_overlap(
                        (
                            [a[0].min(b[0]), a[1].min(b[1])],
                            [a[0].max(b[0]), a[1].max(b[1])],
                        ),
                        bounds,
                        pad,
                    )
                }));
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.right;
                stack[sp + 1] = node.left;
                sp += 2;
            }
        }
        result.sort_unstable();
        result.dedup();
        result
    }

    pub(super) fn node_within_dist(node: &BvhNode, pt: [f64; 2], radius: f64) -> bool {
        pt[0] >= node.min[0] - radius
            && pt[0] <= node.max[0] + radius
            && pt[1] >= node.min[1] - radius
            && pt[1] <= node.max[1] + radius
    }

    pub(super) fn any_within_distance(
        &self,
        segments: &[([f64; 2], [f64; 2])],
        pt: [f64; 2],
        tol: f64,
    ) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !Self::node_within_dist(node, pt, tol) {
                continue;
            }
            if node.count > 0 {
                let start = node.start as usize;
                let end = start + node.count as usize;
                for &seg_idx in &self.indices[start..end] {
                    let (a, b) = segments[seg_idx];
                    if point_to_segment_projection(pt, a, b).1 <= tol {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.right;
                stack[sp + 1] = node.left;
                sp += 2;
            }
        }
        false
    }

    pub(super) fn any_segment_intersections(
        &self,
        segments: &[([f64; 2], [f64; 2])],
        a0: [f64; 2],
        a1: [f64; 2],
    ) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let s_min = [a0[0].min(a1[0]), a0[1].min(a1[1])];
        let s_max = [a0[0].max(a1[0]), a0[1].max(a1[1])];
        let pad = SNAP_EPS * 2.0;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !bounds_overlap((node.min, node.max), (s_min, s_max), pad) {
                continue;
            }
            if node.count > 0 {
                let start = node.start as usize;
                let end = start + node.count as usize;
                for &seg_idx in &self.indices[start..end] {
                    let (b0, b1) = segments[seg_idx];
                    let b_min = [b0[0].min(b1[0]), b0[1].min(b1[1])];
                    let b_max = [b0[0].max(b1[0]), b0[1].max(b1[1])];
                    if bounds_overlap((s_min, s_max), (b_min, b_max), pad)
                        && !segment_intersections(a0, a1, b0, b1).is_empty()
                    {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.right;
                stack[sp + 1] = node.left;
                sp += 2;
            }
        }
        false
    }
}

/// Geometry prepared once for an incoming Merge fill. Besides segment/winding acceleration, the
/// ring-level BVH lets each small existing island subtract only the moved contours whose 2D bounds
/// can affect it. This is the same locality principle as VectorCraft's 2D overlap preflight, adapted
/// to NeoFlash's different-colour subtraction semantics.
pub(super) struct PreparedMergeRings {
    pub(super) segments: Vec<([f64; 2], [f64; 2])>,
    pub(super) segment_bvh: SegmentBvh,
    pub(super) fill: PreparedRings,
    pub(super) ring_boxes: Vec<([f64; 2], [f64; 2])>,
    pub(super) ring_indices: Vec<usize>,
    pub(super) ring_bvh: SegmentBvh,
}

impl PreparedMergeRings {
    pub(super) fn new(rings: &[Vec<[f64; 2]>], rule: FillRule) -> Self {
        let segments = if rule == FillRule::NonZero {
            rings_to_segments(rings)
        } else {
            fill_boundary_segments(rings, rule)
        };
        let segment_bvh = SegmentBvh::from_segments(&segments);
        let fill = PreparedRings::new(rings, rule);
        let mut ring_boxes = Vec::with_capacity(rings.len());
        let mut ring_indices = Vec::with_capacity(rings.len());
        for (index, ring) in rings.iter().enumerate() {
            if let Some((min, max)) = ring_bounds(ring) {
                // A diagonal segment has exactly the ring's AABB in the generic BVH.
                ring_boxes.push((min, max));
                ring_indices.push(index);
            }
        }
        let ring_bvh = SegmentBvh::from_segments(&ring_boxes);
        Self {
            segments,
            segment_bvh,
            fill,
            ring_boxes,
            ring_indices,
            ring_bvh,
        }
    }

    pub(super) fn rings_overlapping(
        &self,
        rings: &[Vec<[f64; 2]>],
        bounds: ([f64; 2], [f64; 2]),
        pad: f64,
    ) -> Vec<Vec<[f64; 2]>> {
        let mut source_indices: Vec<usize> = self
            .ring_bvh
            .bounds_overlap_indices(&self.ring_boxes, bounds, pad)
            .into_iter()
            .filter_map(|box_index| self.ring_indices.get(box_index).copied())
            .collect();
        source_indices.sort_unstable();
        source_indices.dedup();
        source_indices
            .into_iter()
            .filter_map(|index| rings.get(index).cloned())
            .collect()
    }
}

/// Segments outside a planar graph that its probe points must not cross, with their index.
pub(super) type ProbeContext<'a> = (&'a [([f64; 2], [f64; 2])], &'a SegmentBvh);

pub(super) struct PlanarGraph {
    pub(super) vertices: Vec<[f64; 2]>,
    pub(super) edges: Vec<(usize, usize)>,
    pub(super) edge_bvh: SegmentBvh,
}

impl PlanarGraph {
    pub(super) fn from_segments_with_chains(
        raw_segments: &[([f64; 2], [f64; 2])],
    ) -> (Self, Vec<Vec<usize>>) {
        let n = raw_segments.len();
        let mut params: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];

        // 1. Sweep-line sorted by segment min_x so we only test pairs whose X-intervals overlap.
        let seg_bounds: Vec<([f64; 2], [f64; 2])> = raw_segments
            .iter()
            .map(|&(a0, a1)| {
                (
                    [a0[0].min(a1[0]), a0[1].min(a1[1])],
                    [a0[0].max(a1[0]), a0[1].max(a1[1])],
                )
            })
            .collect();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_unstable_by(|&i, &j| {
            seg_bounds[i].0[0]
                .total_cmp(&seg_bounds[j].0[0])
                .then(i.cmp(&j))
        });

        let pad = SNAP_EPS * 2.0;
        for (idx_a, &i) in order.iter().enumerate() {
            let (a_min, a_max) = seg_bounds[i];
            let x_limit = a_max[0] + pad;
            for &j in &order[(idx_a + 1)..] {
                let (b_min, b_max) = seg_bounds[j];
                if b_min[0] > x_limit {
                    break;
                }
                if a_min[1] - pad > b_max[1] || a_max[1] + pad < b_min[1] {
                    continue;
                }
                let (first, second) = if i < j { (i, j) } else { (j, i) };
                let (a0, a1) = raw_segments[first];
                let (b0, b1) = raw_segments[second];
                for (t, u) in segment_intersections(a0, a1, b0, b1) {
                    params[first].push(t);
                    params[second].push(u);
                }
            }
        }

        // 2. Spatial hash grid over canonical vertices (cell size = 2 * SNAP_EPS) so vertex
        // interning is O(1) while returning the exact same lowest matching index.
        let mut vertices: Vec<[f64; 2]> = Vec::new();
        let inv_cell = 1.0 / (SNAP_EPS * 2.0);
        let mut vertex_grid: std::collections::HashMap<(i64, i64), Vec<usize>> =
            std::collections::HashMap::with_capacity(n.saturating_mul(2));
        let mut intern_vertex = |pt: [f64; 2]| -> usize {
            let gx = (pt[0] * inv_cell).floor() as i64;
            let gy = (pt[1] * inv_cell).floor() as i64;
            let mut best: Option<usize> = None;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(bucket) = vertex_grid.get(&(gx + dx, gy + dy)) {
                        for &idx in bucket {
                            if best.is_none_or(|b| idx < b)
                                && dist_sq(vertices[idx], pt) <= SNAP_EPS * SNAP_EPS
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
                vertex_grid.entry((gx, gy)).or_default().push(idx);
                idx
            }
        };

        let mut seg_initial: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
        let mut unique_initial: Vec<(usize, usize)> = Vec::new();
        for (i, (a0, a1)) in raw_segments.iter().enumerate() {
            let ps = &mut params[i];
            ps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            ps.dedup_by(|a, b| (*a - *b).abs() <= PARAM_EPS);
            for w in ps.windows(2) {
                let p0 = [
                    a0[0] + (a1[0] - a0[0]) * w[0],
                    a0[1] + (a1[1] - a0[1]) * w[0],
                ];
                let p1 = [
                    a0[0] + (a1[0] - a0[0]) * w[1],
                    a0[1] + (a1[1] - a0[1]) * w[1],
                ];
                let u = intern_vertex(p0);
                let v = intern_vertex(p1);
                if u != v {
                    seg_initial[i].push((u, v));
                    let key = if u < v { (u, v) } else { (v, u) };
                    unique_initial.push(key);
                }
            }
        }
        unique_initial.sort_unstable();
        unique_initial.dedup();

        // 3. Split any edge that passes within SNAP_EPS of an existing canonical vertex using
        // X-sorted vertices to avoid scanning vertices outside the edge's expanded AABB.
        let mut verts_by_x: Vec<(f64, usize)> = vertices
            .iter()
            .enumerate()
            .map(|(idx, pt)| (pt[0], idx))
            .collect();
        verts_by_x.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut edges: Vec<(usize, usize)> = Vec::with_capacity(unique_initial.len());
        let mut initial_split_chains: Vec<Vec<usize>> = Vec::with_capacity(unique_initial.len());
        for &(u, v) in &unique_initial {
            let a = vertices[u];
            let b = vertices[v];
            let seg_len = dist(a, b);
            if seg_len <= SNAP_EPS {
                initial_split_chains.push(vec![u, v]);
                continue;
            }
            let min_x = a[0].min(b[0]) - SNAP_EPS;
            let max_x = a[0].max(b[0]) + SNAP_EPS;
            let min_y = a[1].min(b[1]) - SNAP_EPS;
            let max_y = a[1].max(b[1]) + SNAP_EPS;
            let start = verts_by_x.partition_point(|&(x, _)| x < min_x);
            let mut splits: Vec<(f64, usize)> = vec![(0.0, u), (1.0, v)];
            for &(x, w_idx) in &verts_by_x[start..] {
                if x > max_x {
                    break;
                }
                if w_idx == u || w_idx == v {
                    continue;
                }
                let w_pt = vertices[w_idx];
                if w_pt[1] < min_y || w_pt[1] > max_y {
                    continue;
                }
                let (t, d) = point_to_segment_projection(w_pt, a, b);
                if d <= SNAP_EPS && t > PARAM_EPS && t < 1.0 - PARAM_EPS {
                    splits.push((t, w_idx));
                }
            }
            splits.sort_by(|x, y| {
                x.0.partial_cmp(&y.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(x.1.cmp(&y.1))
            });
            let mut chain = Vec::with_capacity(splits.len());
            for &(_, idx) in &splits {
                if chain.last().copied() != Some(idx) {
                    chain.push(idx);
                }
            }
            for pair in chain.windows(2) {
                let s0 = pair[0];
                let s1 = pair[1];
                if s0 != s1 {
                    let key = if s0 < s1 { (s0, s1) } else { (s1, s0) };
                    edges.push(key);
                }
            }
            initial_split_chains.push(chain);
        }

        let mut seg_chains: Vec<Vec<usize>> = Vec::with_capacity(n);
        for init_list in &seg_initial {
            let mut full_chain: Vec<usize> = Vec::new();
            for &(u, v) in init_list {
                let key = if u < v { (u, v) } else { (v, u) };
                if let Ok(pos) = unique_initial.binary_search(&key) {
                    let sub = &initial_split_chains[pos];
                    if u < v {
                        for &vx in sub {
                            if full_chain.last().copied() != Some(vx) {
                                full_chain.push(vx);
                            }
                        }
                    } else {
                        for &vx in sub.iter().rev() {
                            if full_chain.last().copied() != Some(vx) {
                                full_chain.push(vx);
                            }
                        }
                    }
                }
            }
            seg_chains.push(full_chain);
        }

        edges.sort_unstable();
        edges.dedup();

        let edge_segs: Vec<([f64; 2], [f64; 2])> = edges
            .iter()
            .map(|&(u, v)| (vertices[u], vertices[v]))
            .collect();
        let edge_bvh = SegmentBvh::from_segments(&edge_segs);

        (
            Self {
                vertices,
                edges,
                edge_bvh,
            },
            seg_chains,
        )
    }

    pub(super) fn probe_points(&self, edge_idx: usize) -> Option<([f64; 2], [f64; 2])> {
        self.probe_points_with_context(edge_idx, None)
    }

    /// Probe points beside an edge, closer to it than any other graph edge or any `context`
    /// segment (contour edges kept outside the graph by the windowed boolean).
    pub(super) fn probe_points_with_context(
        &self,
        edge_idx: usize,
        context: Option<ProbeContext<'_>>,
    ) -> Option<([f64; 2], [f64; 2])> {
        let (u, v) = self.edges[edge_idx];
        let a = self.vertices[u];
        let b = self.vertices[v];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 1e-9 {
            return None;
        }
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let nx = -dy / len;
        let ny = dx / len;

        let mut min_dist: f64 = 0.2;
        if !self.edge_bvh.nodes.is_empty() {
            let mut stack = [0u32; 64];
            let mut sp = 1usize;
            while sp > 0 {
                sp -= 1;
                let node = &self.edge_bvh.nodes[stack[sp] as usize];
                if !SegmentBvh::node_within_dist(node, mid, min_dist) {
                    continue;
                }
                if node.count > 0 {
                    let start = node.start as usize;
                    let end = start + node.count as usize;
                    for &other_idx in &self.edge_bvh.indices[start..end] {
                        if other_idx == edge_idx {
                            continue;
                        }
                        let (ou, ov) = self.edges[other_idx];
                        let (_, d) =
                            point_to_segment_projection(mid, self.vertices[ou], self.vertices[ov]);
                        if d > 1e-9 && d < min_dist {
                            min_dist = d;
                        }
                    }
                } else if sp + 2 <= stack.len() {
                    stack[sp] = node.right;
                    stack[sp + 1] = node.left;
                    sp += 2;
                }
            }
        }
        if let Some((segs, bvh)) = context
            && !bvh.nodes.is_empty()
        {
            let mut stack = [0u32; 64];
            let mut sp = 1usize;
            while sp > 0 {
                sp -= 1;
                let node = &bvh.nodes[stack[sp] as usize];
                if !SegmentBvh::node_within_dist(node, mid, min_dist) {
                    continue;
                }
                if node.count > 0 {
                    let start = node.start as usize;
                    let end = start + node.count as usize;
                    for &other_idx in &bvh.indices[start..end] {
                        let (oa, ob) = segs[other_idx];
                        let (_, d) = point_to_segment_projection(mid, oa, ob);
                        if d > 1e-9 && d < min_dist {
                            min_dist = d;
                        }
                    }
                } else if sp + 2 <= stack.len() {
                    stack[sp] = node.right;
                    stack[sp + 1] = node.left;
                    sp += 2;
                }
            }
        }
        let delta = (min_dist * 0.25).clamp(1e-7, 0.05);
        let p_left = [mid[0] + nx * delta, mid[1] + ny * delta];
        let p_right = [mid[0] - nx * delta, mid[1] - ny * delta];
        Some((p_left, p_right))
    }
}

pub(super) fn rings_to_segments(rings: &[Vec<[f64; 2]>]) -> Vec<([f64; 2], [f64; 2])> {
    let mut segs = Vec::new();
    for ring in rings {
        if ring.len() < 3 {
            continue;
        }
        for i in 0..ring.len() {
            segs.push((ring[i], ring[(i + 1) % ring.len()]));
        }
    }
    segs
}

pub(super) type UnionSegments = Vec<([f64; 2], [f64; 2])>;
pub(super) type UnionSegmentMap = Vec<Vec<Option<usize>>>;

/// When a `NonZero` compound path consists of many same-orientation contours (such as the
/// overlapping circles, quads, or triangles of a Brush stroke), any sub-segment that lies deep
/// inside another contour can never be part of the union boundary. Clipping each segment against
/// overlapping contours (visiting immediate index neighbors first, while keeping a `4 * SNAP_EPS`
/// crossing stub whenever an edge enters/exits a contour so `PlanarGraph` still computes exact
/// boundary intersections) eliminates the 20-to-40-deep internal dab pileup before building
/// `PlanarGraph`.
pub(super) fn cull_and_clip_union_segments_with_map(
    rings: &[Vec<[f64; 2]>],
    fill_rule: FillRule,
) -> (UnionSegments, UnionSegmentMap) {
    let all_without_cull = |rings: &[Vec<[f64; 2]>]| {
        let mut segs = Vec::new();
        let mut map = Vec::with_capacity(rings.len());
        for ring in rings {
            if ring.len() < 3 {
                map.push(Vec::new());
                continue;
            }
            let mut r_map = Vec::with_capacity(ring.len());
            for i in 0..ring.len() {
                let a0 = ring[i];
                let a1 = ring[(i + 1) % ring.len()];
                if dist_sq(a0, a1) <= SNAP_EPS * SNAP_EPS {
                    r_map.push(None);
                } else {
                    let idx = segs.len();
                    segs.push((a0, a1));
                    r_map.push(Some(idx));
                }
            }
            map.push(r_map);
        }
        (segs, map)
    };

    if fill_rule != FillRule::NonZero || rings.len() <= 4 {
        return all_without_cull(rings);
    }
    let mut pos_count = 0usize;
    let mut neg_count = 0usize;
    let mut ring_bboxes: Vec<Option<([f64; 2], [f64; 2])>> = Vec::with_capacity(rings.len());
    for ring in rings {
        if ring.len() < 3 {
            ring_bboxes.push(None);
            continue;
        }
        let area = signed_ring_area(ring);
        if area > AREA_EPS {
            pos_count += 1;
        } else if area < -AREA_EPS {
            neg_count += 1;
        }
        ring_bboxes.push(ring_bounds(ring));
    }
    // Only pre-cull pure unions (all rings have the same winding sign; no subtractive holes).
    if pos_count > 0 && neg_count > 0 {
        return all_without_cull(rings);
    }

    let n_rings = rings.len();
    // Sort ring indices by min_x for fast non-local overlap lookup.
    let mut rings_by_x: Vec<(f64, usize)> = ring_bboxes
        .iter()
        .enumerate()
        .filter_map(|(idx, b)| b.map(|(min, _)| (min[0], idx)))
        .collect();
    rings_by_x.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let max_ring_width = ring_bboxes
        .iter()
        .flatten()
        .map(|(min, max)| max[0] - min[0])
        .fold(0.0_f64, f64::max);

    let point_deep_inside_ring =
        |pt: [f64; 2], other_ring: &[[f64; 2]], other_bbox: ([f64; 2], [f64; 2])| -> bool {
            let margin = SNAP_EPS * 2.0;
            if pt[0] <= other_bbox.0[0] + margin
                || pt[0] >= other_bbox.1[0] - margin
                || pt[1] <= other_bbox.0[1] + margin
                || pt[1] >= other_bbox.1[1] - margin
            {
                return false;
            }
            if ring_winding_number(pt, other_ring) == 0 {
                return false;
            }
            let m = other_ring.len();
            !(0..m).any(|k| {
                point_to_segment_projection(pt, other_ring[k], other_ring[(k + 1) % m]).1 <= margin
            })
        };

    let segment_strictly_inside_ring = |a0: [f64; 2],
                                        a1: [f64; 2],
                                        a_min: [f64; 2],
                                        a_max: [f64; 2],
                                        other_ring: &[[f64; 2]],
                                        other_bbox: ([f64; 2], [f64; 2])|
     -> bool {
        let margin = SNAP_EPS * 2.0;
        if a_min[0] <= other_bbox.0[0] + margin
            || a_max[0] >= other_bbox.1[0] - margin
            || a_min[1] <= other_bbox.0[1] + margin
            || a_max[1] >= other_bbox.1[1] - margin
        {
            return false;
        }
        let mid = [(a0[0] + a1[0]) * 0.5, (a0[1] + a1[1]) * 0.5];
        if !point_deep_inside_ring(a0, other_ring, other_bbox)
            || !point_deep_inside_ring(a1, other_ring, other_bbox)
            || !point_deep_inside_ring(mid, other_ring, other_bbox)
        {
            return false;
        }
        let m = other_ring.len();
        for k in 0..m {
            let b0 = other_ring[k];
            let b1 = other_ring[(k + 1) % m];
            let b_min = [b0[0].min(b1[0]), b0[1].min(b1[1])];
            let b_max = [b0[0].max(b1[0]), b0[1].max(b1[1])];
            if !bounds_overlap((a_min, a_max), (b_min, b_max), margin) {
                continue;
            }
            if !segment_intersections(a0, a1, b0, b1).is_empty()
                || point_to_segment_projection(b0, a0, a1).1 <= margin
                || point_to_segment_projection(b1, a0, a1).1 <= margin
            {
                return false;
            }
        }
        true
    };

    let local_window = 24usize;
    let mut out_segments = Vec::new();
    let mut ring_map = Vec::with_capacity(rings.len());
    for (r_idx, ring) in rings.iter().enumerate() {
        let len = ring.len();
        if len < 3 {
            ring_map.push(Vec::new());
            continue;
        }
        let mut r_edges = Vec::with_capacity(len);
        'edge_loop: for edge_i in 0..len {
            let a0 = ring[edge_i];
            let a1 = ring[(edge_i + 1) % len];
            let seg_len = dist(a0, a1);
            if seg_len <= SNAP_EPS {
                r_edges.push(None);
                continue;
            }
            let a_min = [a0[0].min(a1[0]), a0[1].min(a1[1])];
            let a_max = [a0[0].max(a1[0]), a0[1].max(a1[1])];

            // 1. Check immediate index neighbors first (consecutive brush dabs/quads).
            for delta in 1..=local_window {
                for cand in [r_idx.checked_sub(delta), r_idx.checked_add(delta)]
                    .into_iter()
                    .flatten()
                {
                    if cand < n_rings
                        && let Some(other_bbox) = ring_bboxes[cand]
                        && segment_strictly_inside_ring(
                            a0,
                            a1,
                            a_min,
                            a_max,
                            &rings[cand],
                            other_bbox,
                        )
                    {
                        r_edges.push(None);
                        continue 'edge_loop;
                    }
                }
            }

            // 2. Check any non-local rings whose X-interval can overlap `[a_min[0], a_max[0]]`.
            let search_min_x = a_min[0] - max_ring_width - SNAP_EPS * 2.0;
            let search_max_x = a_max[0] + SNAP_EPS * 2.0;
            let start = rings_by_x.partition_point(|&(x, _)| x < search_min_x);
            for &(x, other_idx) in &rings_by_x[start..] {
                if x > search_max_x {
                    break;
                }
                if other_idx.abs_diff(r_idx) <= local_window {
                    continue;
                }
                if let Some(other_bbox) = ring_bboxes[other_idx]
                    && segment_strictly_inside_ring(
                        a0,
                        a1,
                        a_min,
                        a_max,
                        &rings[other_idx],
                        other_bbox,
                    )
                {
                    r_edges.push(None);
                    continue 'edge_loop;
                }
            }

            let idx = out_segments.len();
            out_segments.push((a0, a1));
            r_edges.push(Some(idx));
        }
        ring_map.push(r_edges);
    }
    (out_segments, ring_map)
}

pub(super) fn build_snapped_rings(
    rings: &[Vec<[f64; 2]>],
    ring_map: &[Vec<Option<usize>>],
    seg_offset: usize,
    vertices: &[[f64; 2]],
    seg_chains: &[Vec<usize>],
) -> Vec<Vec<[f64; 2]>> {
    let mut snapped = Vec::with_capacity(rings.len());
    for (r_idx, ring) in rings.iter().enumerate() {
        let len = ring.len();
        if len < 3 || r_idx >= ring_map.len() || ring_map[r_idx].len() != len {
            continue;
        }
        let r_edges = &ring_map[r_idx];
        let mut pts: Vec<[f64; 2]> = Vec::with_capacity(len * 2);
        for edge_i in 0..len {
            if let Some(local_idx) = r_edges[edge_i]
                && let Some(chain) = seg_chains.get(seg_offset + local_idx)
                && !chain.is_empty()
            {
                for &vx in chain {
                    let p = vertices[vx];
                    if pts.last().is_none_or(|&last| dist_sq(last, p) > 1e-18) {
                        pts.push(p);
                    }
                }
            } else {
                let prev_i = (edge_i + len - 1) % len;
                let prev_had_chain = r_edges[prev_i]
                    .and_then(|idx| seg_chains.get(seg_offset + idx))
                    .is_some_and(|c| !c.is_empty());
                if !prev_had_chain {
                    let p = ring[edge_i];
                    if pts.last().is_none_or(|&last| dist_sq(last, p) > 1e-18) {
                        pts.push(p);
                    }
                }
            }
        }
        if pts.len() >= 2 && dist_sq(pts[0], *pts.last().unwrap()) <= 1e-18 {
            pts.pop();
        }
        if pts.len() >= 3 {
            snapped.push(pts);
        }
    }
    snapped
}

/// Dissolve a multi-contour fill (such as a Brush stroke composed of overlapping circles/quads/triangles)
/// into its exterior boundary shells and interior holes.
pub(super) fn dissolve_rings_to_islands(
    rings: &[Vec<[f64; 2]>],
    fill_rule: FillRule,
) -> Vec<Vec<PathCommand>> {
    if rings.is_empty() {
        return Vec::new();
    }
    let (culled_segments, ring_map) = cull_and_clip_union_segments_with_map(rings, fill_rule);
    let (graph, seg_chains) = PlanarGraph::from_segments_with_chains(&culled_segments);
    let snapped_rings = build_snapped_rings(rings, &ring_map, 0, &graph.vertices, &seg_chains);
    let prepared = PreparedRings::new(&snapped_rings, fill_rule);
    let mut boundary_half_edges = Vec::new();
    for edge_idx in 0..graph.edges.len() {
        let Some((p_left, p_right)) = graph.probe_points(edge_idx) else {
            continue;
        };
        let left_in = prepared.contains(p_left);
        let right_in = prepared.contains(p_right);
        let (u, v) = graph.edges[edge_idx];
        if left_in && !right_in {
            boundary_half_edges.push((u, v));
        } else if !left_in && right_in {
            boundary_half_edges.push((v, u));
        }
    }
    let cycles = trace_half_edge_cycles(&graph.vertices, &boundary_half_edges);
    let traced_rings: Vec<Vec<[f64; 2]>> = cycles
        .into_iter()
        .map(|cycle| {
            cycle
                .into_iter()
                .map(|he_idx| graph.vertices[boundary_half_edges[he_idx].0])
                .collect()
        })
        .collect();
    assemble_shells_and_holes(traced_rings)
}

/// Only edges separating filled and unfilled space are fill boundaries. A compound path can
/// contain adjacent/overlapping contours (including the triangles of an authored Brush stroke);
/// their internal edges must not cut artwork or split another stroke. Winding-based occupancy
/// also retains genuine holes for both fill rules.
pub(super) fn fill_boundary_segments(
    rings: &[Vec<[f64; 2]>],
    fill_rule: FillRule,
) -> Vec<([f64; 2], [f64; 2])> {
    let (culled_segments, ring_map) = cull_and_clip_union_segments_with_map(rings, fill_rule);
    let (graph, seg_chains) = PlanarGraph::from_segments_with_chains(&culled_segments);
    let snapped_rings = build_snapped_rings(rings, &ring_map, 0, &graph.vertices, &seg_chains);
    let prepared = PreparedRings::new(&snapped_rings, fill_rule);
    let boundary_half_edges: Vec<_> = graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(edge_idx, &(u, v))| {
            let (left, right) = graph.probe_points(edge_idx)?;
            let left_in = prepared.contains(left);
            let right_in = prepared.contains(right);
            match (left_in, right_in) {
                (true, false) => Some((u, v)),
                (false, true) => Some((v, u)),
                _ => None,
            }
        })
        .collect();
    let rings: Vec<_> = trace_half_edge_cycles(&graph.vertices, &boundary_half_edges)
        .into_iter()
        .map(|cycle| {
            let ring: Vec<_> = cycle
                .into_iter()
                .map(|index| graph.vertices[boundary_half_edges[index].0])
                .collect();
            simplify_ring_collinear(&ring)
        })
        .collect();
    rings_to_segments(&rings)
}

/// A stroke following a fill's boundary does not cross it at every contour vertex. Require full
/// collinear coverage, rather than just matching endpoints: a path can leave and rejoin an edge.
pub(super) fn segment_lies_on_polyline(segment: ([f64; 2], [f64; 2]), poly: &[[f64; 2]]) -> bool {
    let (a, b) = segment;
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len_sq = dx * dx + dy * dy;
    let len = len_sq.sqrt();
    if len <= SNAP_EPS {
        return true;
    }
    let mut intervals = Vec::new();
    for pair in poly.windows(2) {
        let [p, q] = [pair[0], pair[1]];
        if [p, q]
            .iter()
            .any(|v| ((v[0] - a[0]) * dy - (v[1] - a[1]) * dx).abs() > SNAP_EPS * len)
        {
            continue;
        }
        let t0 = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len_sq;
        let t1 = ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / len_sq;
        let start = t0.min(t1).max(0.0);
        let end = t0.max(t1).min(1.0);
        if start <= end {
            intervals.push((start, end));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut covered: f64 = 0.0;
    let tolerance = SNAP_EPS / len;
    for (start, end) in intervals {
        if start > covered + tolerance {
            return false;
        }
        covered = covered.max(end);
        if covered >= 1.0 - tolerance {
            return true;
        }
    }
    false
}

pub(super) fn polylines_to_segments(polylines: &[Vec<[f64; 2]>]) -> Vec<([f64; 2], [f64; 2])> {
    let mut segs = Vec::new();
    for poly in polylines {
        for pair in poly.windows(2) {
            segs.push((pair[0], pair[1]));
        }
    }
    segs
}
