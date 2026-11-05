//! Planar Merge Drawing geometry engine and 2D polygon boolean / slicing operations.
//!
//! Classic Flash offers two authoring drawing models:
//! - **Object Drawing (`merge == false`)**: each drawn shape is a self-contained element with
//!   unified fill and stroke.
//! - **Merge Drawing (`merge == true`)**: drawn shapes decompose into independent fill and stroke
//!   elements on the active keyframe. Overlapping fills of different colours punch (`Difference`)
//!   underlying merge fills, overlapping fills of the same colour without a separating stroke
//!   coalesce (`Union`), strokes drawn across a fill slice it into separate selectable fills, and
//!   intersecting strokes split at their intersection points.

use crate::evaluate::{affine_multiply, transform_matrix};
use crate::geometry::{FlattenOptions, flatten_path};
use crate::model::{Color, Element, ElementKind, FillRule, Id, PathCommand, Transform, new_id};
use crate::runtime_graphics::GradientPaint;

const SNAP_EPS: f64 = 1e-2;
const PARAM_EPS: f64 = 1e-6;
const AREA_EPS: f64 = 0.25;
const ADJACENT_TOL: f64 = 1.5;

/// Planar boolean operation between closed vector shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Intersection,
    Difference,
    Xor,
}

impl BooleanOp {
    fn eval(self, in_a: bool, in_b: bool) -> bool {
        match self {
            Self::Union => in_a || in_b,
            Self::Intersection => in_a && in_b,
            Self::Difference => in_a && !in_b,
            Self::Xor => in_a ^ in_b,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum FillPaint {
    Solid(Color),
    Gradient(Box<GradientPaint>),
}

#[derive(Clone, Debug, PartialEq)]
struct StrokePaint {
    color: Color,
    gradient: Option<GradientPaint>,
    width: f64,
}

fn extract_fill_paint(kind: &ElementKind) -> Option<FillPaint> {
    match kind {
        ElementKind::Rectangle { fill, .. }
        | ElementKind::Ellipse { fill, .. }
        | ElementKind::Path { fill, .. } => (*fill != [0; 4]).then_some(FillPaint::Solid(*fill)),
        ElementKind::PaintedPath {
            fill,
            fill_gradient,
            ..
        } => {
            if let Some(grad) = fill_gradient {
                Some(FillPaint::Gradient(Box::new(grad.clone())))
            } else if *fill != [0; 4] {
                Some(FillPaint::Solid(*fill))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn extract_stroke_paint(kind: &ElementKind) -> Option<StrokePaint> {
    match kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width,
            ..
        } => (*stroke_width > 0.0 && stroke[3] > 0).then_some(StrokePaint {
            color: *stroke,
            gradient: None,
            width: *stroke_width,
        }),
        ElementKind::PaintedPath {
            stroke,
            stroke_gradient,
            stroke_width,
            ..
        } => (*stroke_width > 0.0 && (stroke[3] > 0 || stroke_gradient.is_some())).then_some(
            StrokePaint {
                color: *stroke,
                gradient: stroke_gradient.clone(),
                width: *stroke_width,
            },
        ),
        _ => None,
    }
}

/// True when `element` is a merge-mode fill-only shape.
pub fn is_merge_fill(element: &Element) -> bool {
    element.merge && extract_fill_paint(&element.kind).is_some()
}

/// True when `element` is a merge-mode stroke-only shape.
pub fn is_merge_stroke(element: &Element) -> bool {
    element.merge
        && extract_fill_paint(&element.kind).is_none()
        && extract_stroke_paint(&element.kind).is_some()
}

fn transform_point(m: [f64; 6], x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// Convert a vector element (`Rectangle`, `Ellipse`, `Path`, `PaintedPath`) into world-space
/// path commands by baking its geometric `Transform`.
pub fn element_world_commands(element: &Element) -> Option<(Vec<PathCommand>, FillRule)> {
    let (local, rule) = element.to_path_commands()?;
    let m = transform_matrix(element.transform);
    let is_identity = (m[0] - 1.0).abs() <= 1e-9
        && m[1].abs() <= 1e-9
        && m[2].abs() <= 1e-9
        && (m[3] - 1.0).abs() <= 1e-9
        && m[4].abs() <= 1e-9
        && m[5].abs() <= 1e-9;
    if is_identity {
        return Some((local, rule));
    }
    let commands = local
        .into_iter()
        .map(|cmd| match cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = transform_point(m, x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = transform_point(m, x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = transform_point(m, cx, cy);
                let [nx, ny] = transform_point(m, x, y);
                PathCommand::QuadraticTo {
                    cx: ncx,
                    cy: ncy,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let [ncx1, ncy1] = transform_point(m, cx1, cy1);
                let [ncx2, ncy2] = transform_point(m, cx2, cy2);
                let [nx, ny] = transform_point(m, x, y);
                PathCommand::CubicTo {
                    cx1: ncx1,
                    cy1: ncy1,
                    cx2: ncx2,
                    cy2: ncy2,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect();
    Some((commands, rule))
}

fn transform_scale_factor(transform: &Transform) -> f64 {
    let sx = transform.scale_x.abs();
    let sy = transform.scale_y.abs();
    ((sx + sy) * 0.5).max(1e-3)
}

/// Merge paths bake the placement matrix into their coordinates. Paint coordinates must follow
/// the same matrix before the placement is reset, including rotation and skew.
fn world_gradient(template: &Element, gradient: &GradientPaint) -> GradientPaint {
    let mut gradient = gradient.clone();
    gradient.matrix = affine_multiply(transform_matrix(template.transform), gradient.matrix);
    gradient
}

fn build_fill_element(
    template: &Element,
    id: Id,
    commands: Vec<PathCommand>,
    fill_rule: FillRule,
    paint: &FillPaint,
    merge: bool,
) -> Element {
    let kind = match paint {
        FillPaint::Solid(color) => ElementKind::Path {
            commands,
            fill_rule,
            fill: *color,
            stroke: [0; 4],
            stroke_width: 0.0,
        },
        FillPaint::Gradient(grad) => ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill: [0, 0, 0, 255],
            stroke: [0; 4],
            fill_gradient: Some(world_gradient(template, grad)),
            stroke_gradient: None,
            stroke_width: 0.0,
        },
    };
    Element {
        id,
        name: template.name.clone(),
        transform: Transform {
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
        merge,
    }
}

fn build_stroke_element(
    template: &Element,
    id: Id,
    commands: Vec<PathCommand>,
    paint: &StrokePaint,
    merge: bool,
) -> Element {
    let kind = if let Some(grad) = &paint.gradient {
        ElementKind::PaintedPath {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: paint.color,
            fill_gradient: None,
            stroke_gradient: Some(world_gradient(template, grad)),
            stroke_width: paint.width,
        }
    } else {
        ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [0; 4],
            stroke: paint.color,
            stroke_width: paint.width,
        }
    };
    Element {
        id,
        name: template.name.clone(),
        transform: Transform {
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
        merge,
    }
}

/// Decompose a drawable vector shape into an optional world-space Merge fill element and zero or
/// more world-space Merge stroke elements.
pub fn decompose_element_to_merge_parts(
    element: &Element,
) -> Option<(Option<Element>, Vec<Element>)> {
    let (commands, fill_rule) = element_world_commands(element)?;
    if commands.is_empty() {
        return None;
    }
    let fill_paint = extract_fill_paint(&element.kind);
    let mut stroke_paint = extract_stroke_paint(&element.kind);
    if let Some(sp) = &mut stroke_paint {
        sp.width *= transform_scale_factor(&element.transform);
    }

    let fill_el = fill_paint.as_ref().and_then(|fp| {
        let rings = flatten_closed_rings(&commands);
        if rings.is_empty() {
            return None;
        }
        let id = if stroke_paint.is_none() && !element.id.is_empty() {
            element.id.clone()
        } else {
            new_id("element")
        };
        Some(build_fill_element(
            element,
            id,
            commands.clone(),
            fill_rule,
            fp,
            true,
        ))
    });

    let mut stroke_els = Vec::new();
    if let Some(sp) = &stroke_paint {
        let stroke_cmds = split_commands_into_contours(&commands);
        for (idx, contour_cmds) in stroke_cmds.into_iter().enumerate() {
            let id = if fill_el.is_none() && idx == 0 && !element.id.is_empty() {
                element.id.clone()
            } else {
                new_id("element")
            };
            stroke_els.push(build_stroke_element(element, id, contour_cmds, sp, true));
        }
    }

    Some((fill_el, stroke_els))
}

fn split_commands_into_contours(commands: &[PathCommand]) -> Vec<Vec<PathCommand>> {
    let mut contours = Vec::new();
    let mut current = Vec::new();
    for cmd in commands {
        if matches!(cmd, PathCommand::MoveTo { .. }) && !current.is_empty() {
            if current.len() >= 2 {
                contours.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
        current.push(*cmd);
    }
    if current.len() >= 2 {
        contours.push(current);
    }
    contours
}

fn dist_sq(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    dist_sq(a, b).sqrt()
}

fn signed_ring_area(ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut area2 = 0.0;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        area2 += a[0] * b[1] - b[0] * a[1];
    }
    area2 * 0.5
}

/// Flatten closed contours from `commands` into vertex rings (without duplicate closing vertex).
pub fn flatten_closed_rings(commands: &[PathCommand]) -> Vec<Vec<[f64; 2]>> {
    let Ok(contours) = flatten_path(
        commands,
        FlattenOptions {
            tolerance: 0.35,
            ..FlattenOptions::default()
        },
    ) else {
        return Vec::new();
    };
    let mut rings = Vec::new();
    for c in contours {
        let mut pts: Vec<[f64; 2]> = Vec::with_capacity(c.points.len());
        for p in c.points {
            if pts
                .last()
                .is_none_or(|prev| dist_sq(*prev, p) > SNAP_EPS * SNAP_EPS)
            {
                pts.push(p);
            }
        }
        if pts.len() >= 2 && dist_sq(pts[0], *pts.last().unwrap()) <= SNAP_EPS * SNAP_EPS {
            pts.pop();
        }
        if pts.len() >= 3 && signed_ring_area(&pts).abs() > AREA_EPS {
            rings.push(pts);
        }
    }
    rings
}

/// Flatten stroke contours from `commands` into open or closed polylines (closed polylines repeat
/// their start vertex at the end).
pub fn flatten_stroke_polylines(commands: &[PathCommand]) -> Vec<Vec<[f64; 2]>> {
    let Ok(contours) = flatten_path(
        commands,
        FlattenOptions {
            tolerance: 0.35,
            ..FlattenOptions::default()
        },
    ) else {
        return Vec::new();
    };
    let mut polylines = Vec::new();
    for c in contours {
        let mut pts: Vec<[f64; 2]> = Vec::with_capacity(c.points.len() + 1);
        for p in c.points {
            if pts
                .last()
                .is_none_or(|prev| dist_sq(*prev, p) > SNAP_EPS * SNAP_EPS)
            {
                pts.push(p);
            }
        }
        if c.closed && pts.len() >= 3 && dist_sq(pts[0], *pts.last().unwrap()) > SNAP_EPS * SNAP_EPS
        {
            let first = pts[0];
            pts.push(first);
        }
        if pts.len() >= 2 {
            polylines.push(pts);
        }
    }
    polylines
}

fn ring_winding_number(pt: [f64; 2], ring: &[[f64; 2]]) -> i32 {
    let n = ring.len();
    if n < 3 {
        return 0;
    }
    let mut winding = 0i32;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if a[1] <= pt[1] {
            if b[1] > pt[1] {
                let cross = (b[0] - a[0]) * (pt[1] - a[1]) - (pt[0] - a[0]) * (b[1] - a[1]);
                if cross > 0.0 {
                    winding += 1;
                }
            }
        } else if b[1] <= pt[1] {
            let cross = (b[0] - a[0]) * (pt[1] - a[1]) - (pt[0] - a[0]) * (b[1] - a[1]);
            if cross < 0.0 {
                winding -= 1;
            }
        }
    }
    winding
}

/// Test whether `pt` lies inside the multi-contour polygon `rings` under `fill_rule`.
pub fn point_in_rings(pt: [f64; 2], rings: &[Vec<[f64; 2]>], fill_rule: FillRule) -> bool {
    let mut total_winding = 0i32;
    for ring in rings {
        total_winding += ring_winding_number(pt, ring);
    }
    match fill_rule {
        FillRule::NonZero => total_winding != 0,
        FillRule::EvenOdd => total_winding.rem_euclid(2) == 1,
    }
}

fn rings_bounds(rings: &[Vec<[f64; 2]>]) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY, f64::INFINITY];
    let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut any = false;
    for ring in rings {
        for &p in ring {
            min[0] = min[0].min(p[0]);
            min[1] = min[1].min(p[1]);
            max[0] = max[0].max(p[0]);
            max[1] = max[1].max(p[1]);
            any = true;
        }
    }
    any.then_some((min, max))
}

fn bounds_overlap(a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2]), pad: f64) -> bool {
    a.0[0] - pad <= b.1[0]
        && a.1[0] + pad >= b.0[0]
        && a.0[1] - pad <= b.1[1]
        && a.1[1] + pad >= b.0[1]
}

fn point_to_segment_projection(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len_sq = dx * dx + dy * dy;
    if len_sq <= 1e-18 {
        return (0.0, dist(p, a));
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len_sq).clamp(0.0, 1.0);
    let proj = [a[0] + t * dx, a[1] + t * dy];
    (t, dist(p, proj))
}

fn segment_intersections(
    a0: [f64; 2],
    a1: [f64; 2],
    b0: [f64; 2],
    b1: [f64; 2],
) -> Vec<(f64, f64)> {
    let mut hits = Vec::new();
    let r = [a1[0] - a0[0], a1[1] - a0[1]];
    let s = [b1[0] - b0[0], b1[1] - b0[1]];
    let rxs = r[0] * s[1] - r[1] * s[0];
    let qp = [b0[0] - a0[0], b0[1] - a0[1]];
    let qpxr = qp[0] * r[1] - qp[1] * r[0];

    if rxs.abs() > 1e-9 {
        let t = (qp[0] * s[1] - qp[1] * s[0]) / rxs;
        let u = qpxr / rxs;
        if (-1e-5..=1.0 + 1e-5).contains(&t) && (-1e-5..=1.0 + 1e-5).contains(&u) {
            hits.push((t.clamp(0.0, 1.0), u.clamp(0.0, 1.0)));
        }
    }

    // Also snap near-touching endpoints or collinear overlap endpoints.
    for (u_end, pt) in [(0.0, b0), (1.0, b1)] {
        let (t, d) = point_to_segment_projection(pt, a0, a1);
        if d <= SNAP_EPS {
            hits.push((t, u_end));
        }
    }
    for (t_end, pt) in [(0.0, a0), (1.0, a1)] {
        let (u, d) = point_to_segment_projection(pt, b0, b1);
        if d <= SNAP_EPS {
            hits.push((t_end, u));
        }
    }
    hits
}

struct PlanarGraph {
    vertices: Vec<[f64; 2]>,
    edges: Vec<(usize, usize)>,
}

impl PlanarGraph {
    fn from_segments(raw_segments: &[([f64; 2], [f64; 2])]) -> Self {
        let n = raw_segments.len();
        let mut params: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];

        for i in 0..n {
            let (a0, a1) = raw_segments[i];
            let a_min = [a0[0].min(a1[0]), a0[1].min(a1[1])];
            let a_max = [a0[0].max(a1[0]), a0[1].max(a1[1])];
            for j in (i + 1)..n {
                let (b0, b1) = raw_segments[j];
                let b_min = [b0[0].min(b1[0]), b0[1].min(b1[1])];
                let b_max = [b0[0].max(b1[0]), b0[1].max(b1[1])];
                if !bounds_overlap((a_min, a_max), (b_min, b_max), SNAP_EPS * 2.0) {
                    continue;
                }
                for (t, u) in segment_intersections(a0, a1, b0, b1) {
                    params[i].push(t);
                    params[j].push(u);
                }
            }
        }

        let mut vertices: Vec<[f64; 2]> = Vec::new();
        let mut intern_vertex = |pt: [f64; 2]| -> usize {
            if let Some(idx) = vertices
                .iter()
                .position(|&v| dist_sq(v, pt) <= SNAP_EPS * SNAP_EPS)
            {
                idx
            } else {
                let idx = vertices.len();
                vertices.push(pt);
                idx
            }
        };

        let mut initial_edges: Vec<(usize, usize)> = Vec::new();
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
                    initial_edges.push((u, v));
                }
            }
        }

        // Split any edge that passes within SNAP_EPS of an existing canonical vertex.
        let mut edges: Vec<(usize, usize)> = Vec::with_capacity(initial_edges.len());
        for (u, v) in initial_edges {
            let a = vertices[u];
            let b = vertices[v];
            let seg_len = dist(a, b);
            if seg_len <= SNAP_EPS {
                continue;
            }
            let mut splits: Vec<(f64, usize)> = vec![(0.0, u), (1.0, v)];
            for (w_idx, &w_pt) in vertices.iter().enumerate() {
                if w_idx == u || w_idx == v {
                    continue;
                }
                let (t, d) = point_to_segment_projection(w_pt, a, b);
                if d <= SNAP_EPS && t > PARAM_EPS && t < 1.0 - PARAM_EPS {
                    splits.push((t, w_idx));
                }
            }
            splits.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
            for pair in splits.windows(2) {
                let s0 = pair[0].1;
                let s1 = pair[1].1;
                if s0 != s1 {
                    let key = if s0 < s1 { (s0, s1) } else { (s1, s0) };
                    edges.push(key);
                }
            }
        }

        edges.sort_unstable();
        edges.dedup();

        Self { vertices, edges }
    }

    fn probe_points(&self, edge_idx: usize) -> Option<([f64; 2], [f64; 2])> {
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
        for (other_idx, &(ou, ov)) in self.edges.iter().enumerate() {
            if other_idx == edge_idx {
                continue;
            }
            let (_, d) = point_to_segment_projection(mid, self.vertices[ou], self.vertices[ov]);
            if d > 1e-6 && d < min_dist {
                min_dist = d;
            }
        }
        let delta = (min_dist * 0.25).clamp(1e-4, 0.05);
        let p_left = [mid[0] + nx * delta, mid[1] + ny * delta];
        let p_right = [mid[0] - nx * delta, mid[1] - ny * delta];
        Some((p_left, p_right))
    }
}

fn rings_to_segments(rings: &[Vec<[f64; 2]>]) -> Vec<([f64; 2], [f64; 2])> {
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

/// Only edges separating filled and unfilled space are fill boundaries. A compound path can
/// contain adjacent/overlapping contours (including the triangles of an authored Brush stroke);
/// their internal edges must not cut artwork or split another stroke. Winding-based occupancy
/// also retains genuine holes for both fill rules.
fn fill_boundary_segments(
    rings: &[Vec<[f64; 2]>],
    fill_rule: FillRule,
) -> Vec<([f64; 2], [f64; 2])> {
    let graph = PlanarGraph::from_segments(&rings_to_segments(rings));
    let boundary_half_edges: Vec<_> = graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(edge_idx, &(u, v))| {
            let (left, right) = graph.probe_points(edge_idx)?;
            let left_in = point_in_rings(left, rings, fill_rule);
            let right_in = point_in_rings(right, rings, fill_rule);
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
fn segment_lies_on_polyline(segment: ([f64; 2], [f64; 2]), poly: &[[f64; 2]]) -> bool {
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

fn polylines_to_segments(polylines: &[Vec<[f64; 2]>]) -> Vec<([f64; 2], [f64; 2])> {
    let mut segs = Vec::new();
    for poly in polylines {
        for pair in poly.windows(2) {
            segs.push((pair[0], pair[1]));
        }
    }
    segs
}

/// Trace closed cycles from a set of active directed half-edges `(u, v)`.
///
/// At each vertex `v` arrived via `u -> v`, we look in the direction of `v -> u` and rotate
/// clockwise (decreasing angle) to pick the first outgoing active half-edge `v -> w`. This keeps
/// the minimal interior face to the left of `u -> v -> w`.
fn trace_half_edge_cycles(vertices: &[[f64; 2]], half_edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    if half_edges.is_empty() {
        return Vec::new();
    }
    let mut outgoing: Vec<Vec<(usize, f64)>> = vec![Vec::new(); vertices.len()];
    for (idx, &(u, v)) in half_edges.iter().enumerate() {
        let a = vertices[u];
        let b = vertices[v];
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

    let mut visited = vec![false; half_edges.len()];
    let mut cycles = Vec::new();

    for start_idx in 0..half_edges.len() {
        if visited[start_idx] {
            continue;
        }
        let mut cycle_edges = Vec::new();
        let mut curr = start_idx;
        let mut closed = false;
        while !visited[curr] {
            visited[curr] = true;
            cycle_edges.push(curr);
            let (u, v) = half_edges[curr];
            let list = &outgoing[v];
            if list.is_empty() {
                break;
            }
            let a = vertices[v];
            let b = vertices[u];
            let dy = if (b[1] - a[1]).abs() == 0.0 {
                0.0
            } else {
                b[1] - a[1]
            };
            let rev_angle = dy.atan2(b[0] - a[0]);
            let idx_first_ge = list.partition_point(|&(_, ang)| ang < rev_angle - 1e-9);
            let next_pos = (idx_first_ge + list.len() - 1) % list.len();
            let next_he = list[next_pos].0;
            if next_he == start_idx {
                closed = true;
                break;
            }
            curr = next_he;
        }
        if closed && cycle_edges.len() >= 3 {
            cycles.push(cycle_edges);
        }
    }
    cycles
}

fn simplify_ring_collinear(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
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

fn ring_to_commands(ring: &[[f64; 2]]) -> Vec<PathCommand> {
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

fn polyline_to_commands(poly: &[[f64; 2]]) -> Vec<PathCommand> {
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

fn assemble_shells_and_holes(rings: Vec<Vec<[f64; 2]>>) -> Vec<Vec<PathCommand>> {
    type Shell = (Vec<[f64; 2]>, f64, Vec<Vec<[f64; 2]>>);
    let mut shells: Vec<Shell> = Vec::new();
    let mut holes: Vec<Vec<[f64; 2]>> = Vec::new();

    for raw_ring in rings {
        let ring = simplify_ring_collinear(&raw_ring);
        if ring.len() < 3 {
            continue;
        }
        let area = signed_ring_area(&ring);
        if area > AREA_EPS {
            shells.push((ring, area, Vec::new()));
        } else if area < -AREA_EPS {
            holes.push(ring);
        }
    }

    // Sort shells from smallest positive area to largest so each hole attaches to its tightest
    // enclosing outer shell.
    shells.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    for hole in holes {
        let hole_area = signed_ring_area(&hole).abs();
        let sample = hole
            .iter()
            .copied()
            .find(|&pt| {
                !shells.iter().any(|(shell, _, _)| {
                    (0..shell.len()).any(|i| {
                        point_to_segment_projection(pt, shell[i], shell[(i + 1) % shell.len()]).1
                            <= SNAP_EPS
                    })
                })
            })
            .unwrap_or(hole[0]);
        if let Some((_, _, shell_holes)) = shells.iter_mut().find(|(shell, shell_area, _)| {
            *shell_area > hole_area && ring_winding_number(sample, shell) != 0
        }) {
            shell_holes.push(hole);
        }
    }

    let mut result = Vec::with_capacity(shells.len());
    for (shell, _, shell_holes) in shells {
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
    if rings_a.is_empty() {
        return match op {
            BooleanOp::Union | BooleanOp::Xor => assemble_shells_and_holes(
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
            BooleanOp::Intersection | BooleanOp::Difference => Vec::new(),
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
                    Vec::new()
                } else {
                    vec![cmds]
                }
            }
            BooleanOp::Intersection => Vec::new(),
        };
    }

    let mut segments = rings_to_segments(rings_a);
    segments.extend(rings_to_segments(rings_b));
    let graph = PlanarGraph::from_segments(&segments);

    let mut boundary_half_edges = Vec::new();
    for edge_idx in 0..graph.edges.len() {
        let Some((p_left, p_right)) = graph.probe_points(edge_idx) else {
            continue;
        };
        let keep_left = op.eval(
            point_in_rings(p_left, rings_a, rule_a),
            point_in_rings(p_left, rings_b, rule_b),
        );
        let keep_right = op.eval(
            point_in_rings(p_right, rings_a, rule_a),
            point_in_rings(p_right, rings_b, rule_b),
        );
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
    assemble_shells_and_holes(rings)
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

    // Quick bounding-box check before building planar graph.
    let mut any_overlap = false;
    for &(a, b) in &stroke_segs {
        let s_min = [a[0].min(b[0]), a[1].min(b[1])];
        let s_max = [a[0].max(b[0]), a[1].max(b[1])];
        if bounds_overlap(fill_bbox, (s_min, s_max), SNAP_EPS * 2.0) {
            any_overlap = true;
            break;
        }
    }
    if !any_overlap {
        let mut cmds = Vec::new();
        for r in fill_rings {
            cmds.extend(ring_to_commands(r));
        }
        return vec![cmds];
    }

    let mut all_segs = rings_to_segments(fill_rings);
    all_segs.extend_from_slice(&stroke_segs);
    let graph = PlanarGraph::from_segments(&all_segs);

    let mut active_half_edges: Vec<(usize, usize)> = Vec::new();
    for edge_idx in 0..graph.edges.len() {
        let Some((p_left, p_right)) = graph.probe_points(edge_idx) else {
            continue;
        };
        let left_in = point_in_rings(p_left, fill_rings, fill_rule);
        let right_in = point_in_rings(p_right, fill_rings, fill_rule);
        let (u, v) = graph.edges[edge_idx];
        // An internal contour edge is not a cut. Only an actual stroke may divide two filled
        // faces. The graph has already split edges at every intersection, so its midpoint
        // identifies whether this complete subedge belongs to a cutter.
        let is_cut = if left_in && right_in {
            let a = graph.vertices[u];
            let b = graph.vertices[v];
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            stroke_segs
                .iter()
                .any(|&(s0, s1)| point_to_segment_projection(mid, s0, s1).1 <= SNAP_EPS)
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
    let cutters: Vec<_> = fill_segs
        .iter()
        .copied()
        .filter(|&segment| !segment_lies_on_polyline(segment, poly))
        .collect();
    let sub_segments = split_polyline_into_subsegments(poly, &cutters, false);

    let mut result: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();

    for (a, b, cut_at_start) in sub_segments {
        if cut_at_start && current.len() >= 2 {
            result.push(std::mem::take(&mut current));
        }
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let on_boundary = fill_segs.iter().any(|&(s0, s1)| {
            let (_, d) = point_to_segment_projection(mid, s0, s1);
            d <= SNAP_EPS * 2.0
        });
        let strictly_inside = !on_boundary && point_in_rings(mid, fill_rings, fill_rule);
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

fn split_polyline_into_subsegments(
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

fn ensure_existing_merge_normalized(elements: &mut Vec<Element>) {
    let mut normalized = Vec::with_capacity(elements.len());
    for el in elements.drain(..) {
        if !el.merge {
            normalized.push(el);
            continue;
        }
        if let Some((fill_opt, stroke_els)) = decompose_element_to_merge_parts(&el) {
            if let Some(f) = fill_opt {
                normalized.push(f);
            }
            normalized.extend(stroke_els);
        } else {
            normalized.push(el);
        }
    }
    *elements = normalized;
}

/// Apply a newly drawn shape (`new_element`) in Merge Drawing mode to the keyframe's `elements`,
/// performing planar fill union/difference, stroke slicing of fills, and stroke intersection
/// splitting against existing `merge == true` elements on the keyframe.
///
/// Returns the IDs of the newly created Merge element(s).
pub fn apply_merge_shape_to_keyframe(elements: &mut Vec<Element>, new_element: Element) -> Vec<Id> {
    ensure_existing_merge_normalized(elements);
    let Some((mut new_fill_opt, new_strokes)) = decompose_element_to_merge_parts(&new_element)
    else {
        let id = new_element.id.clone();
        elements.push(new_element);
        return vec![id];
    };

    let has_own_stroke = !new_strokes.is_empty();
    let mut created_ids = Vec::new();

    // 1. Fill interaction against existing Merge elements.
    if let Some(mut new_fill) = new_fill_opt.take() {
        let Some((mut new_cmds, mut new_rule)) = element_world_commands(&new_fill) else {
            return Vec::new();
        };
        let mut new_rings = flatten_closed_rings(&new_cmds);
        let new_paint = extract_fill_paint(&new_fill.kind);

        if !new_rings.is_empty()
            && let Some(new_fp) = &new_paint
        {
            let mut next_elements = Vec::with_capacity(elements.len() + 2);
            for existing in elements.drain(..) {
                if !existing.merge {
                    next_elements.push(existing);
                    continue;
                }
                if is_merge_fill(&existing) {
                    let Some((ex_cmds, ex_rule)) = element_world_commands(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let ex_rings = flatten_closed_rings(&ex_cmds);
                    let (Some(ex_bbox), Some(new_bbox)) =
                        (rings_bounds(&ex_rings), rings_bounds(&new_rings))
                    else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let same_paint = extract_fill_paint(&existing.kind).as_ref() == Some(new_fp);
                    if !has_own_stroke && same_paint {
                        let unioned = boolean_rings(
                            &ex_rings,
                            ex_rule,
                            &new_rings,
                            new_rule,
                            BooleanOp::Union,
                        );
                        if unioned.len() == 1 {
                            new_cmds = unioned.into_iter().next().unwrap();
                            new_rule = FillRule::NonZero;
                            new_rings = flatten_closed_rings(&new_cmds);
                            continue;
                        }
                    }
                    // Different colour (or separated by new shape's outline stroke): subtract
                    // `new_fill` from `existing`.
                    let diff = boolean_rings(
                        &ex_rings,
                        ex_rule,
                        &new_rings,
                        new_rule,
                        BooleanOp::Difference,
                    );
                    if let Some(ex_fp) = extract_fill_paint(&existing.kind) {
                        for (idx, piece_cmds) in diff.into_iter().enumerate() {
                            let piece_id = if idx == 0 {
                                existing.id.clone()
                            } else {
                                new_id("element")
                            };
                            next_elements.push(build_fill_element(
                                &existing,
                                piece_id,
                                piece_cmds,
                                FillRule::NonZero,
                                &ex_fp,
                                true,
                            ));
                        }
                    }
                } else if is_merge_stroke(&existing) {
                    let Some((ex_cmds, _)) = element_world_commands(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let polylines = flatten_stroke_polylines(&ex_cmds);
                    let Some(sp) = extract_stroke_paint(&existing.kind) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let mut piece_idx = 0usize;
                    for poly in polylines {
                        for trimmed in trim_polyline_inside_fill(&poly, &new_rings, new_rule) {
                            let cmds = polyline_to_commands(&trimmed);
                            if cmds.len() >= 2 {
                                let piece_id = if piece_idx == 0 {
                                    existing.id.clone()
                                } else {
                                    new_id("element")
                                };
                                piece_idx += 1;
                                next_elements.push(build_stroke_element(
                                    &existing, piece_id, cmds, &sp, true,
                                ));
                            }
                        }
                    }
                } else {
                    next_elements.push(existing);
                }
            }
            *elements = next_elements;
            new_fill = build_fill_element(
                &new_fill,
                new_fill.id.clone(),
                new_cmds,
                new_rule,
                new_fp,
                true,
            );
            created_ids.push(new_fill.id.clone());
            elements.push(new_fill);
        }
    }

    // 2. Stroke interaction against existing Merge fills and strokes.
    if !new_strokes.is_empty() {
        let own_fill_id = created_ids.first().cloned();
        let mut stroke_polylines_all = Vec::new();
        for ns in &new_strokes {
            if let Some((cmds, _)) = element_world_commands(ns) {
                stroke_polylines_all.extend(flatten_stroke_polylines(&cmds));
            }
        }

        // 2a. Slice any existing Merge fills crossed by the new stroke(s).
        if !stroke_polylines_all.is_empty() {
            let mut next_elements = Vec::with_capacity(elements.len() + 2);
            for existing in elements.drain(..) {
                if !existing.merge
                    || !is_merge_fill(&existing)
                    || own_fill_id.as_deref() == Some(existing.id.as_str())
                {
                    next_elements.push(existing);
                    continue;
                }
                let Some((ex_cmds, ex_rule)) = element_world_commands(&existing) else {
                    next_elements.push(existing);
                    continue;
                };
                let ex_rings = flatten_closed_rings(&ex_cmds);
                let sliced =
                    slice_fill_rings_with_polylines(&ex_rings, ex_rule, &stroke_polylines_all);
                if sliced.len() <= 1 {
                    next_elements.push(existing);
                } else if let Some(ex_fp) = extract_fill_paint(&existing.kind) {
                    for (idx, piece_cmds) in sliced.into_iter().enumerate() {
                        let piece_id = if idx == 0 {
                            existing.id.clone()
                        } else {
                            new_id("element")
                        };
                        next_elements.push(build_fill_element(
                            &existing,
                            piece_id,
                            piece_cmds,
                            FillRule::NonZero,
                            &ex_fp,
                            true,
                        ));
                    }
                }
            }
            *elements = next_elements;
        }

        // 2b. Split existing Merge strokes at intersections with the new stroke(s), and split
        // the new stroke(s) at intersections with existing Merge strokes & fill boundaries.
        let new_stroke_segs = polylines_to_segments(&stroke_polylines_all);
        let mut existing_stroke_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
        let mut existing_fill_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();

        let mut next_elements = Vec::with_capacity(elements.len() + new_strokes.len());
        for existing in elements.drain(..) {
            if !existing.merge {
                next_elements.push(existing);
                continue;
            }
            if is_merge_fill(&existing) {
                if own_fill_id.as_deref() != Some(existing.id.as_str())
                    && let Some((ex_cmds, ex_rule)) = element_world_commands(&existing)
                {
                    let rings = flatten_closed_rings(&ex_cmds);
                    existing_fill_segs.extend(fill_boundary_segments(&rings, ex_rule));
                }
                next_elements.push(existing);
            } else if is_merge_stroke(&existing) {
                let Some((ex_cmds, _)) = element_world_commands(&existing) else {
                    next_elements.push(existing);
                    continue;
                };
                let polys = flatten_stroke_polylines(&ex_cmds);
                existing_stroke_segs.extend(polylines_to_segments(&polys));
                let Some(sp) = extract_stroke_paint(&existing.kind) else {
                    next_elements.push(existing);
                    continue;
                };
                let mut piece_idx = 0usize;
                for poly in polys {
                    for piece in split_polyline_with_segments(&poly, &new_stroke_segs, false) {
                        let cmds = polyline_to_commands(&piece);
                        if cmds.len() >= 2 {
                            let piece_id = if piece_idx == 0 {
                                existing.id.clone()
                            } else {
                                new_id("element")
                            };
                            piece_idx += 1;
                            next_elements
                                .push(build_stroke_element(&existing, piece_id, cmds, &sp, true));
                        }
                    }
                }
            } else {
                next_elements.push(existing);
            }
        }
        *elements = next_elements;

        for ns in new_strokes {
            let Some((ns_cmds, _)) = element_world_commands(&ns) else {
                continue;
            };
            let Some(sp) = extract_stroke_paint(&ns.kind) else {
                continue;
            };
            let polys = flatten_stroke_polylines(&ns_cmds);
            let mut piece_idx = 0usize;
            for poly in polys {
                let mut cutters = existing_stroke_segs.clone();
                cutters.extend(
                    existing_fill_segs
                        .iter()
                        .copied()
                        .filter(|&segment| !segment_lies_on_polyline(segment, &poly)),
                );
                for piece in split_polyline_with_segments(&poly, &cutters, true) {
                    let cmds = polyline_to_commands(&piece);
                    if cmds.len() >= 2 {
                        let piece_id = if piece_idx == 0 {
                            ns.id.clone()
                        } else {
                            new_id("element")
                        };
                        piece_idx += 1;
                        created_ids.push(piece_id.clone());
                        elements.push(build_stroke_element(&ns, piece_id, cmds, &sp, true));
                    }
                }
            }
        }
    }

    created_ids
}

/// Re-apply Merge Drawing planar interactions after existing Merge-mode elements (`moved_ids`)
/// have been moved or transformed on the stage.
pub fn remerge_moved_elements_in_keyframe(
    elements: &mut Vec<Element>,
    moved_ids: &[Id],
) -> Vec<Id> {
    let mut moved = Vec::new();
    let mut remaining = Vec::with_capacity(elements.len());
    for el in elements.drain(..) {
        if el.merge && moved_ids.iter().any(|id| id == &el.id) {
            moved.push(el);
        } else {
            remaining.push(el);
        }
    }
    *elements = remaining;
    if moved.is_empty() {
        return Vec::new();
    }
    let mut result_ids = Vec::new();
    for el in moved {
        result_ids.extend(apply_merge_shape_to_keyframe(elements, el));
    }
    result_ids
}

/// Find all Merge-mode stroke IDs on `elements` that lie along or touch the boundary of the
/// Merge-mode fill `fill_id` (used when double-clicking a Merge fill on stage).
pub fn merge_fill_adjacent_stroke_ids(elements: &[Element], fill_id: &str) -> Vec<Id> {
    let Some(fill_el) = elements
        .iter()
        .find(|e| e.id == fill_id && is_merge_fill(e))
    else {
        return Vec::new();
    };
    let Some((fill_cmds, fill_rule)) = element_world_commands(fill_el) else {
        return Vec::new();
    };
    let fill_rings = flatten_closed_rings(&fill_cmds);
    let fill_segs = fill_boundary_segments(&fill_rings, fill_rule);
    if fill_segs.is_empty() {
        return Vec::new();
    }

    let mut result = vec![fill_id.to_string()];
    for el in elements {
        if !is_merge_stroke(el) {
            continue;
        }
        let Some((s_cmds, _)) = element_world_commands(el) else {
            continue;
        };
        let polys = flatten_stroke_polylines(&s_cmds);
        let touches = polys.iter().any(|poly| {
            poly.iter().all(|&pt| {
                fill_segs
                    .iter()
                    .any(|&(a, b)| point_to_segment_projection(pt, a, b).1 <= ADJACENT_TOL)
            })
        });
        if touches {
            result.push(el.id.clone());
        }
    }
    result
}

/// Find all Merge-mode stroke IDs on `elements` connected to `stroke_id` by shared endpoints
/// (used when double-clicking a Merge stroke on stage).
pub fn merge_connected_stroke_ids(elements: &[Element], stroke_id: &str) -> Vec<Id> {
    let mut strokes: Vec<(Id, Vec<[f64; 2]>)> = Vec::new();
    for el in elements {
        if !is_merge_stroke(el) {
            continue;
        }
        let Some((cmds, _)) = element_world_commands(el) else {
            continue;
        };
        let mut endpoints = Vec::new();
        for poly in flatten_stroke_polylines(&cmds) {
            if let (Some(&first), Some(&last)) = (poly.first(), poly.last()) {
                endpoints.push(first);
                endpoints.push(last);
            }
        }
        strokes.push((el.id.clone(), endpoints));
    }

    let Some(start_idx) = strokes.iter().position(|(id, _)| id == stroke_id) else {
        return Vec::new();
    };
    let mut visited = vec![false; strokes.len()];
    visited[start_idx] = true;
    let mut queue = std::collections::VecDeque::from([start_idx]);
    let mut result = Vec::new();

    while let Some(curr) = queue.pop_front() {
        result.push(strokes[curr].0.clone());
        for other in 0..strokes.len() {
            if visited[other] {
                continue;
            }
            let connected = strokes[curr].1.iter().any(|&a| {
                strokes[other]
                    .1
                    .iter()
                    .any(|&b| dist_sq(a, b) <= ADJACENT_TOL * ADJACENT_TOL)
            });
            if connected {
                visited[other] = true;
                queue.push_back(other);
            }
        }
    }
    result
}

fn translate_path_commands(commands: &[PathCommand], dx: f64, dy: f64) -> Vec<PathCommand> {
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

fn shift_gradient(mut gradient: GradientPaint, dx: f64, dy: f64) -> GradientPaint {
    gradient.matrix = affine_multiply([1.0, 0.0, 0.0, 1.0, dx, dy], gradient.matrix);
    gradient
}

fn build_combined_shape_element(
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

#[cfg(test)]
mod tests {
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
                    is_merge_stroke(e)
                        && extract_stroke_paint(&e.kind).unwrap().color == [0, 0, 0, 255]
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
                        dist_sq(poly[0], point) < 1e-9
                            || dist_sq(*poly.last().unwrap(), point) < 1e-9
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
                commands: ring_to_commands(&[
                    [0.0, 0.0],
                    [100.0, 0.0],
                    [100.0, 100.0],
                    [0.0, 100.0],
                ]),
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
                    placement_matrix =
                        affine_multiply(transform_matrix(movement), placement_matrix);
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
            combine_elements_boolean(&[base.clone(), cutter.clone()], BooleanOp::Union, false)
                .unwrap();
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
}
