//! Flash-style shape tweens between the shapes of two keyframes on one layer.
//!
//! Shapes (`Rectangle`, `Ellipse`, `Path`, `PaintedPath`) are converted to
//! commands in their parent space and morphed into a `Path` (or `PaintedPath`
//! when a gradient is involved) with a geometrically identity transform.
//! Command lists with equal topology keep the plain per-command interpolation;
//! everything else is morphed contour by contour after arc-length resampling to
//! a common cubic segment count and a start-point/direction alignment.
//!
//! The resampling and alignment are written for this project: lines and
//! quadratics are elevated to cubics, every source segment keeps at least one
//! output segment (so corners survive), extra segments are distributed by arc
//! length and produced by exact de Casteljau subdivision, so t=0 and t=1
//! reproduce the keyframe geometry.
use crate::evaluate::{affine_multiply, transform_matrix};
use crate::geometry::{interpolate_commands, paths_have_same_topology, validate_path_commands};
use crate::runtime_graph::clipping::vector_path;
use crate::{
    Color, ColorTransform, Element, ElementKind, FillRule, GradientPaint, GradientStop,
    PathCommand, Transform,
};

/// Upper bound of cubic segments in one tweened shape. A pair needing more is
/// held at its `from` element instead of producing a partial morph.
pub const MAX_SHAPE_TWEEN_SEGMENTS: usize = 4096;
const MIN_CONTOUR_SEGMENTS: usize = 8;
const MAX_CONTOUR_SEGMENTS: usize = 256;
/// Polyline samples per cubic used to estimate arc length.
const ARC_SAMPLES: usize = 32;
/// Maximum start rotations tried while aligning one contour pair.
const MAX_ROTATION_CANDIDATES: usize = 256;
/// Endpoint welding tolerance for contiguous Merge Drawing stroke segments (stage px).
const MERGE_STROKE_WELD_EPS: f64 = 1e-2;
/// Default magnetic corner/anchor snap radius when dragging shape hints on Stage (stage px).
pub const DEFAULT_HINT_VERTEX_SNAP_RADIUS: f64 = 6.0;

type P = [f64; 2];
/// Cubic segment: first control, second control, end. The start is the
/// previous segment's end (or the contour start).
type Seg = [P; 3];

#[derive(Clone, Debug)]
struct Contour {
    start: P,
    segs: Vec<Seg>,
    closed: bool,
}

#[derive(Clone, Debug)]
struct Paint {
    fill: Color,
    stroke: Color,
    fill_gradient: Option<GradientPaint>,
    stroke_gradient: Option<GradientPaint>,
    stroke_width: f64,
    fill_rule: FillRule,
}

struct Shape {
    contours: Vec<Contour>,
    commands: Vec<PathCommand>,
    paint: Paint,
}

/// Sampled contour polyline and on-curve anchor vertices in parent space for Shape Hint authoring.
#[derive(Clone, Debug)]
pub struct ShapeHintContour {
    pub points: Vec<[f64; 2]>,
    pub anchors: Vec<[f64; 2]>,
    pub closed: bool,
}

/// Keyframe-level parent-space contour set and unified UV bounding box used by both
/// the Shape Tween runtime morph engine and Stage Shape Hint authoring.
#[derive(Clone, Debug)]
pub struct ShapeHintGeometry {
    pub contours: Vec<ShapeHintContour>,
    pub low: [f64; 2],
    pub high: [f64; 2],
}

/// Deterministic initial UV assigned to a newly added shape hint `'a'..='z'` before placement.
pub fn default_shape_hint_uv(label: char) -> [f64; 2] {
    let index = if label.is_ascii_lowercase() {
        f64::from(label as u8 - b'a' + 1)
    } else {
        1.0
    };
    [index / 27.0, 0.5]
}

/// True when `uv` is still at the untouched default `[k / 27.0, 0.5]` coordinate assigned by
/// `add_shape_hint` (`k in 1..=26`).
pub fn is_default_unplaced_uv(uv: [f64; 2]) -> bool {
    (uv[1] - 0.5).abs() <= 1e-9
        && (1..=ShapeHint::MAX_HINTS).any(|k| (uv[0] - (k as f64 / 27.0)).abs() <= 1e-9)
}

use crate::ShapeHint;

fn contour_sampled_points(c: &Contour) -> Vec<P> {
    let mut pts = Vec::with_capacity(1 + c.segs.len() * ARC_SAMPLES);
    pts.push(c.start);
    let mut p0 = c.start;
    for s in &c.segs {
        for k in 1..=ARC_SAMPLES {
            pts.push(cubic_point(p0, s, k as f64 / ARC_SAMPLES as f64));
        }
        p0 = s[2];
    }
    if c.cyclic()
        && pts.first() != pts.last()
        && let Some(&first) = pts.first()
    {
        pts.push(first);
    }
    pts
}

impl ShapeHintGeometry {
    pub fn from_key(key: &crate::Keyframe) -> Option<Self> {
        Self::from_elements(&key.elements)
    }

    pub fn from_elements(elements: &[Element]) -> Option<Self> {
        let welded = weld_merge_strokes_for_tween(elements);
        let mut contours = Vec::new();
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for element in &welded {
            let Some(shape) = parent_shape(element) else {
                continue;
            };
            for contour in &shape.contours {
                let points = contour_sampled_points(contour);
                if points.len() < 2 {
                    continue;
                }
                for p in &points {
                    low = [low[0].min(p[0]), low[1].min(p[1])];
                    high = [high[0].max(p[0]), high[1].max(p[1])];
                }
                let mut anchors = contour.vertices();
                if contour.cyclic() && anchors.len() > 1 && anchors.first() == anchors.last() {
                    anchors.pop();
                }
                contours.push(ShapeHintContour {
                    points,
                    anchors,
                    closed: contour.cyclic(),
                });
            }
        }
        if contours.is_empty() || low[0] > high[0] {
            return None;
        }
        Some(Self {
            contours,
            low,
            high,
        })
    }

    pub fn position(&self, uv: [f64; 2]) -> [f64; 2] {
        uv_to_point((self.low, self.high), uv)
    }

    /// Closest point on the flattened contour segments without magnetic corner jump.
    pub fn nearest_on_contour(&self, p: [f64; 2]) -> [f64; 2] {
        self.contours
            .iter()
            .flat_map(|c| c.points.windows(2))
            .map(|pair| project_on_segment(p, pair[0], pair[1]).0)
            .min_by(|a, b| dist2(*a, p).total_cmp(&dist2(*b, p)))
            .unwrap_or(p)
    }

    /// Closest point on the shape contours, snapping magnetically to on-curve corner/anchor
    /// vertices within `DEFAULT_HINT_VERTEX_SNAP_RADIUS`.
    pub fn nearest(&self, p: [f64; 2]) -> [f64; 2] {
        self.nearest_with_snap(p, DEFAULT_HINT_VERTEX_SNAP_RADIUS)
    }

    /// Closest point on the shape contours, snapping magnetically to on-curve corner/anchor
    /// vertices within `vertex_snap_radius` stage pixels before falling back to edge projection.
    pub fn nearest_with_snap(&self, p: [f64; 2], vertex_snap_radius: f64) -> [f64; 2] {
        let on_edge = self.nearest_on_contour(p);
        if vertex_snap_radius > 0.0 {
            let mut best_anchor: Option<(f64, [f64; 2])> = None;
            for contour in &self.contours {
                let min_chord = if contour.anchors.len() >= 2 {
                    let mut mc = f64::INFINITY;
                    for i in 0..contour.anchors.len() {
                        let next = (i + 1) % contour.anchors.len();
                        if next == 0 && !contour.closed {
                            break;
                        }
                        let d = dist2(contour.anchors[i], contour.anchors[next]).sqrt();
                        if d > 1e-6 {
                            mc = mc.min(d);
                        }
                    }
                    mc
                } else {
                    f64::INFINITY
                };
                let eff_radius = vertex_snap_radius.min(min_chord * 0.35);
                if eff_radius <= 0.0 {
                    continue;
                }
                for &anchor in &contour.anchors {
                    let d = dist2(p, anchor).sqrt();
                    if d <= eff_radius && best_anchor.as_ref().is_none_or(|(best_d, _)| d < *best_d)
                    {
                        best_anchor = Some((d, anchor));
                    }
                }
            }
            if let Some((_, anchor)) = best_anchor {
                return anchor;
            }
        }
        on_edge
    }

    pub fn placed(&self, uv: [f64; 2]) -> bool {
        if !uv_valid_for_bounds(uv, (self.low, self.high)) {
            return false;
        }
        let p = self.position(uv);
        let tol = bounds_snap_tolerance((self.low, self.high));
        dist2(p, self.nearest_on_contour(p)) <= tol * tol
    }

    pub fn snapped_uv(&self, pointer: [f64; 2]) -> [f64; 2] {
        self.snapped_uv_with_snap(pointer, DEFAULT_HINT_VERTEX_SNAP_RADIUS)
    }

    pub fn snapped_uv_with_snap(&self, pointer: [f64; 2], vertex_snap_radius: f64) -> [f64; 2] {
        let p = self.nearest_with_snap(pointer, vertex_snap_radius);
        point_to_snapped_uv((self.low, self.high), p)
    }
}

fn uv_to_point(bounds: (P, P), uv: [f64; 2]) -> P {
    let (low, high) = bounds;
    [
        low[0] + (high[0] - low[0]) * uv[0].clamp(0.0, 1.0),
        low[1] + (high[1] - low[1]) * uv[1].clamp(0.0, 1.0),
    ]
}

fn point_to_snapped_uv(bounds: (P, P), p: P) -> [f64; 2] {
    let (low, high) = bounds;
    let mut uv: [f64; 2] = std::array::from_fn(|axis| {
        let size = high[axis] - low[axis];
        if size > 1e-9 {
            ((p[axis] - low[axis]) / size).clamp(0.0, 1.0)
        } else {
            0.0
        }
    });
    if is_default_unplaced_uv(uv) {
        uv[1] = (uv[1] + 1e-6).clamp(0.0, 1.0);
    }
    uv
}

fn uv_valid_for_bounds(uv: [f64; 2], bounds: (P, P)) -> bool {
    if !uv[0].is_finite() || !uv[1].is_finite() || is_default_unplaced_uv(uv) {
        return false;
    }
    let (low, high) = bounds;
    for axis in 0..2 {
        if high[axis] - low[axis] <= 1e-9 && uv[axis].abs() > 1e-3 && (uv[axis] - 1.0).abs() > 1e-3
        {
            return false;
        }
    }
    true
}

fn bounds_snap_tolerance(bounds: (P, P)) -> f64 {
    let (low, high) = bounds;
    let diag = (high[0] - low[0]).hypot(high[1] - low[1]);
    (diag * 0.02).clamp(1e-3, 0.5)
}

fn project_on_segment(p: P, a: P, b: P) -> (P, f64) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ([a[0] + d[0] * t, a[1] + d[1] * t], t)
}

fn is_weldable_merge_stroke(element: &Element, shape: &Shape) -> bool {
    element.merge
        && shape.paint.fill[3] == 0
        && shape.paint.fill_gradient.is_none()
        && shape.paint.stroke_width > 0.0
        && (shape.paint.stroke[3] > 0 || shape.paint.stroke_gradient.is_some())
        && shape.contours.len() == 1
        && !shape.contours[0].closed
}

fn merge_strokes_compatible(a: &Element, sa: &Shape, b: &Element, sb: &Shape) -> bool {
    sa.paint.stroke == sb.paint.stroke
        && sa.paint.stroke_gradient == sb.paint.stroke_gradient
        && (sa.paint.stroke_width - sb.paint.stroke_width).abs() <= 1e-4
        && a.visible == b.visible
        && a.blend_mode == b.blend_mode
        && a.filters == b.filters
        && (a.transform.alpha - b.transform.alpha).abs() <= 1e-6
        && a.transform.color_transform == b.transform.color_transform
}

/// Weld contiguous Merge Drawing stroke-only `Element`s with identical stroke paint into unified
/// continuous (and closed, when start meets end) paths before Shape Tween pairing or Hint snapping.
pub fn weld_merge_strokes_for_tween(elements: &[Element]) -> Vec<Element> {
    let shapes: Vec<Option<Shape>> = elements.iter().map(parent_shape).collect();
    let mut consumed = vec![false; elements.len()];
    let mut out = Vec::with_capacity(elements.len());
    let eps2 = MERGE_STROKE_WELD_EPS * MERGE_STROKE_WELD_EPS;

    for i in 0..elements.len() {
        if consumed[i] {
            continue;
        }
        let Some(ref shape_i) = shapes[i] else {
            out.push(elements[i].clone());
            continue;
        };
        if !is_weldable_merge_stroke(&elements[i], shape_i) {
            out.push(elements[i].clone());
            continue;
        }
        consumed[i] = true;
        let mut contour = shape_i.contours[0].clone();
        let mut welded_any = false;

        loop {
            if contour.segs.len() >= 2 && dist2(contour.start, contour.end()) <= eps2 {
                if let Some(last) = contour.segs.last_mut() {
                    last[2] = contour.start;
                }
                contour.closed = true;
                welded_any = true;
                break;
            }
            let mut found = false;
            for j in (i + 1)..elements.len() {
                if consumed[j] {
                    continue;
                }
                let Some(ref shape_j) = shapes[j] else {
                    continue;
                };
                if !is_weldable_merge_stroke(&elements[j], shape_j)
                    || !merge_strokes_compatible(&elements[i], shape_i, &elements[j], shape_j)
                {
                    continue;
                }
                let cj = &shape_j.contours[0];
                let c_end = contour.end();
                let c_start = contour.start;
                if dist2(cj.start, c_end) <= eps2 {
                    contour.segs.extend_from_slice(&cj.segs);
                    consumed[j] = true;
                    welded_any = true;
                    found = true;
                    break;
                } else if dist2(cj.end(), c_end) <= eps2 {
                    let rev = cj.reversed();
                    contour.segs.extend_from_slice(&rev.segs);
                    consumed[j] = true;
                    welded_any = true;
                    found = true;
                    break;
                } else if dist2(cj.end(), c_start) <= eps2 {
                    let mut new_segs = cj.segs.clone();
                    if let Some(last) = new_segs.last_mut() {
                        last[2] = contour.start;
                    }
                    new_segs.extend(contour.segs);
                    contour.start = cj.start;
                    contour.segs = new_segs;
                    consumed[j] = true;
                    welded_any = true;
                    found = true;
                    break;
                } else if dist2(cj.start, c_start) <= eps2 {
                    let rev = cj.reversed();
                    let mut new_segs = rev.segs;
                    if let Some(last) = new_segs.last_mut() {
                        last[2] = contour.start;
                    }
                    new_segs.extend(contour.segs);
                    contour.start = rev.start;
                    contour.segs = new_segs;
                    consumed[j] = true;
                    welded_any = true;
                    found = true;
                    break;
                }
            }
            if !found {
                if !contour.closed
                    && !contour.segs.is_empty()
                    && dist2(contour.start, contour.end()) <= eps2
                {
                    if let Some(last) = contour.segs.last_mut() {
                        last[2] = contour.start;
                    }
                    contour.closed = true;
                    welded_any = true;
                }
                break;
            }
        }

        if welded_any {
            let mut commands = Vec::with_capacity(contour.segs.len() + 2);
            emit(
                &mut commands,
                contour.start,
                contour.segs.into_iter(),
                contour.closed,
            );
            out.push(build(
                &elements[i],
                commands,
                shape_i.paint.clone(),
                placement(&elements[i].transform, &elements[i].transform, 0.0),
            ));
        } else {
            out.push(elements[i].clone());
        }
    }
    out
}

fn element_has_fill(element: &Element) -> bool {
    match &element.kind {
        ElementKind::Rectangle { fill, .. }
        | ElementKind::Ellipse { fill, .. }
        | ElementKind::Path { fill, .. } => fill[3] > 0,
        ElementKind::PaintedPath {
            fill,
            fill_gradient,
            ..
        } => fill[3] > 0 || fill_gradient.is_some(),
        _ => false,
    }
}

fn pair_shape_elements(from: &[Element], to: &[Element]) -> (Vec<Option<usize>>, Vec<bool>) {
    let from_shapes: Vec<usize> = (0..from.len())
        .filter(|&i| is_shape(&from[i].kind))
        .collect();
    let to_shapes: Vec<usize> = (0..to.len()).filter(|&i| is_shape(&to[i].kind)).collect();
    let mut partner: Vec<Option<usize>> = vec![None; from.len()];
    let mut to_used = vec![false; to.len()];
    // 1. Pair by identical element ID.
    for &fi in &from_shapes {
        if let Some(&ti) = to_shapes
            .iter()
            .find(|&&ti| !to_used[ti] && to[ti].id == from[fi].id)
        {
            partner[fi] = Some(ti);
            to_used[ti] = true;
        }
    }
    // 2. Pair fill-bearing with fill-bearing and stroke-only with stroke-only.
    for &fi in &from_shapes {
        if partner[fi].is_none() {
            let from_fill = element_has_fill(&from[fi]);
            if let Some(&ti) = to_shapes
                .iter()
                .find(|&&ti| !to_used[ti] && element_has_fill(&to[ti]) == from_fill)
            {
                partner[fi] = Some(ti);
                to_used[ti] = true;
            }
        }
    }
    // 3. Pair any remaining shape elements in order.
    let free_to: Vec<usize> = to_shapes
        .iter()
        .copied()
        .filter(|&ti| !to_used[ti])
        .collect();
    let mut free_to = free_to.into_iter();
    for &fi in &from_shapes {
        if partner[fi].is_none()
            && let Some(ti) = free_to.next()
        {
            partner[fi] = Some(ti);
            to_used[ti] = true;
        }
    }
    (partner, to_used)
}

/// Return the labels of all `ShapeHint`s that are actively snapped onto corresponding contours of
/// `from` and `to` in valid cyclic/monotonic contour order.
pub fn active_shape_hint_labels(
    from: &[Element],
    to: &[Element],
    hints: &[ShapeHint],
) -> Vec<char> {
    if hints.is_empty() {
        return Vec::new();
    }
    let from = weld_merge_strokes_for_tween(from);
    let to = weld_merge_strokes_for_tween(to);
    let (Some(from_geom), Some(to_geom)) = (
        ShapeHintGeometry::from_elements(&from),
        ShapeHintGeometry::from_elements(&to),
    ) else {
        return Vec::new();
    };
    let a_bounds = (from_geom.low, from_geom.high);
    let b_bounds = (to_geom.low, to_geom.high);
    let (partner, _) = pair_shape_elements(&from, &to);
    let mut active = Vec::new();
    for (fi, element) in from.iter().enumerate() {
        let Some(ti) = partner[fi] else {
            continue;
        };
        let (Some(sa), Some(sb)) = (parent_shape(element), parent_shape(&to[ti])) else {
            continue;
        };
        let pairs = sa.contours.len().min(sb.contours.len());
        let allow_reverse = sb.contours.len() == 1 || sb.paint.fill_rule == FillRule::EvenOdd;
        for i in 0..pairs {
            for resolved in
                resolve_contour_hints(&sa, &sb, i, hints, a_bounds, b_bounds, allow_reverse)
            {
                if !active.contains(&resolved.label) {
                    active.push(resolved.label);
                }
            }
        }
    }
    active
}

/// Shape tween between the shapes of two keyframes on one layer, at eased progress `t` (0..=1).
/// Returns the elements to draw at that frame.
///
/// Shapes pair by equal `id` first, then by order. Non-shape elements of `from`
/// are held unchanged. Unpaired `from` shapes shrink to the centre of their
/// bounds (omitted at t >= 1); unpaired `to` shapes grow from theirs (omitted
/// at t <= 0) and are appended after the `from` elements.
pub fn shape_tween_elements(from: &[Element], to: &[Element], t: f64) -> Vec<Element> {
    shape_tween_elements_with_hints(from, to, &[], t)
}

/// Shape tween between the shapes of two keyframes on one layer with optional correspondence
/// hints (`ShapeHint`), at eased progress `t` (`0.0..=1.0`).
pub fn shape_tween_elements_with_hints(
    from: &[Element],
    to: &[Element],
    hints: &[crate::ShapeHint],
    t: f64,
) -> Vec<Element> {
    let t = if t.is_finite() { t } else { 0. };
    let from = weld_merge_strokes_for_tween(from);
    let to = weld_merge_strokes_for_tween(to);
    let from_bounds = ShapeHintGeometry::from_elements(&from).map(|g| (g.low, g.high));
    let to_bounds = ShapeHintGeometry::from_elements(&to).map(|g| (g.low, g.high));
    let (partner, to_used) = pair_shape_elements(&from, &to);
    let to_shapes: Vec<usize> = (0..to.len()).filter(|&i| is_shape(&to[i].kind)).collect();

    let mut out = Vec::with_capacity(from.len() + to.len());
    for (fi, element) in from.iter().enumerate() {
        if !is_shape(&element.kind) {
            out.push(element.clone());
            continue;
        }
        match partner[fi] {
            Some(ti) => out.push(morph_pair_with_hints(
                element,
                &to[ti],
                hints,
                from_bounds,
                to_bounds,
                t,
            )),
            // Over the segment cap the shape is held unchanged.
            None if t < 1. => {
                out.push(collapse_shape(element, t).unwrap_or_else(|| element.clone()))
            }
            None => {}
        }
    }
    if t > 0. {
        for &ti in &to_shapes {
            if !to_used[ti] {
                // Over the segment cap a growing shape stays hidden until t = 1.
                out.extend(collapse_shape(&to[ti], 1. - t));
            }
        }
    }
    out
}

fn is_shape(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. }
    )
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
fn lerp_p(a: P, b: P, t: f64) -> P {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]
}
fn color_lerp(a: Color, b: Color, t: f64) -> Color {
    std::array::from_fn(|i| lerp(a[i] as f64, b[i] as f64, t).clamp(0., 255.).round() as u8)
}
fn dist2(a: P, b: P) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}
fn map_point(m: [f64; 6], x: f64, y: f64) -> P {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// Commands, contours and paint of a shape element in its parent space.
fn parent_shape(element: &Element) -> Option<Shape> {
    let local = match &element.kind {
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. } => {
            if element.has_parametric_options() {
                element.to_path_commands().map_or_else(
                    || degenerate_box(width.max(0.), height.max(0.)),
                    |(commands, _)| commands,
                )
            } else {
                vector_path(&element.kind).map_or_else(
                    // Zero-sized boxes stay drawable as a degenerate outline so they
                    // can grow into (or shrink from) their partner.
                    || degenerate_box(width.max(0.), height.max(0.)),
                    |(commands, _)| commands,
                )
            }
        }
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            commands.clone()
        }
        _ => return None,
    };
    let m = transform_matrix(element.transform);
    let commands: Vec<PathCommand> = local
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo { x, y } => {
                let [x, y] = map_point(m, x, y);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let [x, y] = map_point(m, x, y);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [cx, cy] = map_point(m, cx, cy);
                let [x, y] = map_point(m, x, y);
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let [cx1, cy1] = map_point(m, cx1, cy1);
                let [cx2, cy2] = map_point(m, cx2, cy2);
                let [x, y] = map_point(m, x, y);
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect();
    validate_path_commands(&commands).ok()?;
    // Strokes scale with the placement; non-uniform scale is approximated by
    // the geometric mean of the axis scales.
    let stroke_scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt();
    let to_parent = |g: &Option<GradientPaint>| {
        g.as_ref().map(|g| GradientPaint {
            matrix: affine_multiply(m, g.matrix),
            ..g.clone()
        })
    };
    let paint = match &element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        } => Paint {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: FillRule::NonZero,
        },
        ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        } => Paint {
            fill: if element.oval_options().close_path {
                *fill
            } else {
                [0, 0, 0, 0]
            },
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: FillRule::NonZero,
        },
        ElementKind::Path {
            fill_rule,
            fill,
            stroke,
            stroke_width,
            ..
        } => Paint {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: None,
            stroke_gradient: None,
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: *fill_rule,
        },
        ElementKind::PaintedPath {
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => Paint {
            fill: *fill,
            stroke: *stroke,
            fill_gradient: to_parent(fill_gradient),
            stroke_gradient: to_parent(stroke_gradient),
            stroke_width: *stroke_width * stroke_scale,
            fill_rule: *fill_rule,
        },
        _ => return None,
    };
    let contours = contours(&commands);
    Some(Shape {
        contours,
        commands,
        paint,
    })
}

fn degenerate_box(width: f64, height: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: width, y: 0. },
        PathCommand::LineTo {
            x: width,
            y: height,
        },
        PathCommand::LineTo { x: 0., y: height },
        PathCommand::Close,
    ]
}

fn line(a: P, b: P) -> Seg {
    [lerp_p(a, b, 1. / 3.), lerp_p(a, b, 2. / 3.), b]
}

/// Split validated commands into cubic contours. A closed contour whose pen is
/// not at its start gets the explicit closing line. Move-only contours are dropped.
fn contours(commands: &[PathCommand]) -> Vec<Contour> {
    let mut out = Vec::new();
    let mut current: Option<Contour> = None;
    let mut pen = [0., 0.];
    let finish = |c: Option<Contour>, out: &mut Vec<Contour>| {
        if let Some(c) = c
            && !c.segs.is_empty()
        {
            out.push(c);
        }
    };
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                finish(current.take(), &mut out);
                current = Some(Contour {
                    start: [x, y],
                    segs: Vec::new(),
                    closed: false,
                });
                pen = [x, y];
            }
            PathCommand::LineTo { x, y } => {
                if let Some(c) = current.as_mut() {
                    c.segs.push(line(pen, [x, y]));
                }
                pen = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(c) = current.as_mut() {
                    let (q, e) = ([cx, cy], [x, y]);
                    c.segs
                        .push([lerp_p(pen, q, 2. / 3.), lerp_p(e, q, 2. / 3.), e]);
                }
                pen = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if let Some(c) = current.as_mut() {
                    c.segs.push([[cx1, cy1], [cx2, cy2], [x, y]]);
                }
                pen = [x, y];
            }
            PathCommand::Close => {
                if let Some(mut c) = current.take() {
                    if pen != c.start {
                        c.segs.push(line(pen, c.start));
                    }
                    c.closed = true;
                    pen = c.start;
                    finish(Some(c), &mut out);
                }
            }
        }
    }
    finish(current, &mut out);
    out
}

impl Contour {
    fn end(&self) -> P {
        self.segs.last().map_or(self.start, |s| s[2])
    }
    /// Ends where it starts, so any segment may begin the outline.
    fn cyclic(&self) -> bool {
        self.closed || self.end() == self.start
    }
    fn make_cyclic(&mut self) {
        if self.end() != self.start {
            let s = line(self.end(), self.start);
            self.segs.push(s);
        }
    }
    /// On-curve points: the start plus every segment end.
    fn vertices(&self) -> Vec<P> {
        std::iter::once(self.start)
            .chain(self.segs.iter().map(|s| s[2]))
            .collect()
    }
    fn reversed(&self) -> Contour {
        let mut segs = Vec::with_capacity(self.segs.len());
        for i in (0..self.segs.len()).rev() {
            let start = if i == 0 {
                self.start
            } else {
                self.segs[i - 1][2]
            };
            segs.push([self.segs[i][1], self.segs[i][0], start]);
        }
        Contour {
            start: self.end(),
            segs,
            closed: self.closed,
        }
    }
    /// Start at vertex `r` of a cyclic contour.
    fn rotated(&self, r: usize) -> Contour {
        if r == 0 {
            return self.clone();
        }
        let mut segs = self.segs.clone();
        segs.rotate_left(r);
        Contour {
            start: self.segs[r - 1][2],
            segs,
            closed: self.closed,
        }
    }
    fn bounds_centre(&self) -> P {
        bounds_centre(self.vertices().into_iter())
    }
}

fn bounds_centre(points: impl Iterator<Item = P>) -> P {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in points {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    if lo[0] > hi[0] {
        return [0., 0.];
    }
    [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2.]
}

fn cubic_point(p0: P, s: &Seg, u: f64) -> P {
    let v = 1. - u;
    let (a, b, c, d) = (v * v * v, 3. * v * v * u, 3. * v * u * u, u * u * u);
    [
        a * p0[0] + b * s[0][0] + c * s[1][0] + d * s[2][0],
        a * p0[1] + b * s[0][1] + c * s[1][1] + d * s[2][1],
    ]
}

/// Cumulative polyline lengths at u = i / ARC_SAMPLES.
fn arc_table(p0: P, s: &Seg) -> [f64; ARC_SAMPLES + 1] {
    let mut table = [0.; ARC_SAMPLES + 1];
    let (mut prev, mut length) = (p0, 0.);
    for (i, slot) in table.iter_mut().enumerate().skip(1) {
        let p = cubic_point(p0, s, i as f64 / ARC_SAMPLES as f64);
        length += dist2(prev, p).sqrt();
        *slot = length;
        prev = p;
    }
    table
}

/// de Casteljau split at `u`; returns (left, right) with the right part's
/// start being the left part's end.
fn split(p0: P, s: &Seg, u: f64) -> (Seg, Seg) {
    let p01 = lerp_p(p0, s[0], u);
    let p12 = lerp_p(s[0], s[1], u);
    let p23 = lerp_p(s[1], s[2], u);
    let p012 = lerp_p(p01, p12, u);
    let p123 = lerp_p(p12, p23, u);
    let mid = lerp_p(p012, p123, u);
    ([p01, p012, mid], [p123, p23, s[2]])
}

/// Split one cubic into `k` pieces of (approximately) equal arc length.
fn split_equal(p0: P, s: &Seg, k: usize, out: &mut Vec<Seg>) {
    if k <= 1 {
        out.push(*s);
        return;
    }
    let table = arc_table(p0, s);
    let total = table[ARC_SAMPLES];
    let mut rest = *s;
    let mut rest_start = p0;
    let mut prev_u = 0.;
    for j in 1..k {
        let u = if total > 0. {
            let target = total * j as f64 / k as f64;
            let i = table.partition_point(|&l| l < target).clamp(1, ARC_SAMPLES);
            let span = table[i] - table[i - 1];
            let f = if span > 0. {
                (target - table[i - 1]) / span
            } else {
                0.
            };
            (i as f64 - 1. + f) / ARC_SAMPLES as f64
        } else {
            j as f64 / k as f64
        };
        let remaining = 1. - prev_u;
        let local = if remaining > 0. {
            ((u - prev_u) / remaining).clamp(0., 1.)
        } else {
            0.
        };
        let (left, right) = split(rest_start, &rest, local);
        out.push(left);
        rest_start = left[2];
        rest = right;
        prev_u = u;
    }
    out.push(rest);
}

/// Distribute `n >= lengths.len()` segments: one per source segment, extras by
/// arc length with the largest-remainder rule (ties to the earlier segment).
fn distribute(lengths: &[f64], n: usize) -> Vec<usize> {
    let m = lengths.len();
    let extra = n - m;
    let total: f64 = lengths.iter().sum();
    if !(total.is_finite() && total > 0.) {
        return (0..m)
            .map(|i| 1 + extra / m + usize::from(i < extra % m))
            .collect();
    }
    let ideal: Vec<f64> = lengths.iter().map(|l| extra as f64 * l / total).collect();
    let mut counts: Vec<usize> = ideal.iter().map(|v| 1 + v.floor() as usize).collect();
    let assigned: usize = counts.iter().sum::<usize>() - m;
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| {
        (ideal[b] - ideal[b].floor())
            .total_cmp(&(ideal[a] - ideal[a].floor()))
            .then(a.cmp(&b))
    });
    for &i in order.iter().take(extra.saturating_sub(assigned)) {
        counts[i] += 1;
    }
    counts
}

fn resample(c: &Contour, n: usize) -> Contour {
    let mut lengths = Vec::with_capacity(c.segs.len());
    let mut p0 = c.start;
    for s in &c.segs {
        lengths.push(arc_table(p0, s)[ARC_SAMPLES]);
        p0 = s[2];
    }
    let counts = distribute(&lengths, n);
    let mut segs = Vec::with_capacity(n);
    let mut p0 = c.start;
    for (s, &k) in c.segs.iter().zip(&counts) {
        split_equal(p0, s, k, &mut segs);
        p0 = s[2];
    }
    Contour {
        start: c.start,
        segs,
        closed: c.closed,
    }
}

fn contour_segments(a: &Contour, b: &Contour) -> usize {
    let most = a.segs.len().max(b.segs.len());
    (2 * most)
        .clamp(MIN_CONTOUR_SEGMENTS, MAX_CONTOUR_SEGMENTS)
        .max(most)
}

/// Choose the start vertex (cyclic only) and direction of `dst` that minimise
/// the summed squared distance to the vertices of `src` (same segment count).
fn align_unhinted(src: &Contour, dst: &Contour, cyclic: bool, allow_reverse: bool) -> Contour {
    let n = src.segs.len();
    let sv = src.vertices();
    let mut best: Option<(f64, Contour)> = None;
    let mut candidates = vec![dst.clone()];
    if allow_reverse {
        candidates.push(dst.reversed());
    }
    let stride = if cyclic {
        n.div_ceil(MAX_ROTATION_CANDIDATES).max(1)
    } else {
        n + 1
    };
    for candidate in candidates {
        let dv = candidate.vertices();
        let rotations = if cyclic { n } else { 1 };
        let mut r = 0;
        while r < rotations {
            let cost: f64 = if cyclic {
                (0..n).map(|i| dist2(sv[i], dv[(i + r) % n])).sum()
            } else {
                sv.iter().zip(&dv).map(|(a, b)| dist2(*a, *b)).sum()
            };
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                best = Some((cost, candidate.rotated(r)));
            }
            r += stride;
        }
    }
    best.map_or_else(|| dst.clone(), |(_, c)| c)
}

fn contour_point_at(c: &Contour, pos: f64) -> P {
    if c.segs.is_empty() {
        return c.start;
    }
    let m = c.segs.len();
    let clamped = pos.clamp(0.0, m as f64);
    let idx = (clamped.floor() as usize).min(m - 1);
    let u = (clamped - idx as f64).clamp(0.0, 1.0);
    let p0 = if idx == 0 {
        c.start
    } else {
        c.segs[idx - 1][2]
    };
    cubic_point(p0, &c.segs[idx], u)
}

fn project_onto_contour(c: &Contour, target: P) -> (f64, f64) {
    if c.segs.is_empty() {
        return (0.0, dist2(c.start, target).sqrt());
    }
    let m = c.segs.len();
    let mut best_d2 = f64::INFINITY;
    let mut best_pos = 0.0;
    let mut p0 = c.start;
    for (seg_idx, s) in c.segs.iter().enumerate() {
        let mut prev = p0;
        for k in 1..=ARC_SAMPLES {
            let u1 = k as f64 / ARC_SAMPLES as f64;
            let cur = cubic_point(p0, s, u1);
            let (proj, chord_t) = project_on_segment(target, prev, cur);
            let d2 = dist2(target, proj);
            if d2 < best_d2 {
                best_d2 = d2;
                let local_u = (k as f64 - 1.0 + chord_t) / ARC_SAMPLES as f64;
                best_pos = seg_idx as f64 + local_u;
            }
            prev = cur;
        }
        p0 = s[2];
    }
    // Snap near-integer parameters to exact segment boundaries to avoid microscopic slivers.
    let nearest_int = best_pos.round();
    if (best_pos - nearest_int).abs() <= 1e-4 {
        best_pos = nearest_int.clamp(0.0, m as f64);
    }
    if c.cyclic() && best_pos >= m as f64 - 1e-4 {
        best_pos = 0.0;
    }
    (best_pos, best_d2.sqrt())
}

fn contour_signed_area(c: &Contour) -> f64 {
    let pts = contour_sampled_points(c);
    if pts.len() < 3 {
        return 0.0;
    }
    let mut area2 = 0.0;
    for pair in pts.windows(2) {
        area2 += pair[0][0] * pair[1][1] - pair[1][0] * pair[0][1];
    }
    if let (Some(last), Some(first)) = (pts.last(), pts.first())
        && last != first
    {
        area2 += last[0] * first[1] - first[0] * last[1];
    }
    0.5 * area2
}

fn contour_arc_length(c: &Contour) -> f64 {
    let mut total = 0.0;
    let mut p0 = c.start;
    for s in &c.segs {
        total += arc_table(p0, s)[ARC_SAMPLES];
        p0 = s[2];
    }
    total
}

fn shape_sampled_bounds(shape: &Shape) -> (P, P) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for c in &shape.contours {
        for p in contour_sampled_points(c) {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
    }
    if lo[0] > hi[0] {
        ([0.0, 0.0], [0.0, 0.0])
    } else {
        (lo, hi)
    }
}

#[derive(Clone, Copy, Debug)]
struct ResolvedHint {
    label: char,
    pos_a: f64,
    pos_b: f64,
    reverse_cb: bool,
}

fn resolve_contour_hints(
    sa: &Shape,
    sb: &Shape,
    contour_idx: usize,
    hints: &[ShapeHint],
    a_bounds: (P, P),
    b_bounds: (P, P),
    allow_reverse: bool,
) -> Vec<ResolvedHint> {
    if hints.is_empty() {
        return Vec::new();
    }
    let ca = &sa.contours[contour_idx];
    let mut cb = sb.contours[contour_idx].clone();
    let cyclic = ca.cyclic();
    if cyclic {
        cb.make_cyclic();
    }
    if ca.segs.is_empty() || cb.segs.is_empty() {
        return Vec::new();
    }
    let tol_a = bounds_snap_tolerance(a_bounds);
    let tol_b = bounds_snap_tolerance(b_bounds);

    let mut candidates: Vec<(char, P, P, f64)> = Vec::new();
    for h in hints {
        if !uv_valid_for_bounds(h.start_uv, a_bounds) || !uv_valid_for_bounds(h.end_uv, b_bounds) {
            continue;
        }
        let pt_a = uv_to_point(a_bounds, h.start_uv);
        let pt_b = uv_to_point(b_bounds, h.end_uv);
        let (pos_a, dist_a) = project_onto_contour(ca, pt_a);
        let (_, dist_b) = project_onto_contour(&cb, pt_b);
        if dist_a > tol_a || dist_b > tol_b {
            continue;
        }
        if sa.contours.len() > 1
            && sa.contours.iter().enumerate().any(|(idx, other)| {
                idx != contour_idx && project_onto_contour(other, pt_a).1 + 1e-6 < dist_a
            })
        {
            continue;
        }
        if sb.contours.len() > 1
            && sb.contours.iter().enumerate().any(|(idx, other)| {
                idx != contour_idx && project_onto_contour(other, pt_b).1 + 1e-6 < dist_b
            })
        {
            continue;
        }
        candidates.push((h.label, pt_a, pt_b, pos_a));
    }
    if candidates.is_empty() {
        return Vec::new();
    }
    candidates.sort_by_key(|c| c.0);

    if cyclic {
        let area_a = contour_signed_area(ca);
        let area_b = contour_signed_area(&cb);
        let reverse_cb = allow_reverse
            && area_a.abs() > 1e-6
            && area_b.abs() > 1e-6
            && area_a.signum() != area_b.signum();
        let cb_oriented = if reverse_cb { cb.reversed() } else { cb };
        let m_a = ca.segs.len() as f64;
        let m_b = cb_oriented.segs.len() as f64;

        struct AcceptedCyclic {
            resolved: ResolvedHint,
            da: f64,
            db: f64,
        }
        let mut accepted: Vec<AcceptedCyclic> = Vec::new();
        let mut origin_ua = 0.0;
        let mut origin_ub = 0.0;

        for (label, _pt_a, pt_b, pos_a) in candidates {
            let (pos_b, _) = project_onto_contour(&cb_oriented, pt_b);
            let ua = (pos_a / m_a).rem_euclid(1.0);
            let ub = (pos_b / m_b).rem_euclid(1.0);
            if accepted.is_empty() {
                origin_ua = ua;
                origin_ub = ub;
                accepted.push(AcceptedCyclic {
                    resolved: ResolvedHint {
                        label,
                        pos_a,
                        pos_b,
                        reverse_cb,
                    },
                    da: 0.0,
                    db: 0.0,
                });
            } else {
                let da = (ua - origin_ua).rem_euclid(1.0);
                let db = (ub - origin_ub).rem_euclid(1.0);
                if da <= 1e-4 || da >= 1.0 - 1e-4 || db <= 1e-4 || db >= 1.0 - 1e-4 {
                    continue;
                }
                let order_ok = accepted.iter().skip(1).all(|prev| {
                    (da - prev.da).abs() > 1e-4
                        && (db - prev.db).abs() > 1e-4
                        && ((da < prev.da) == (db < prev.db))
                });
                if order_ok {
                    accepted.push(AcceptedCyclic {
                        resolved: ResolvedHint {
                            label,
                            pos_a,
                            pos_b,
                            reverse_cb,
                        },
                        da,
                        db,
                    });
                }
            }
        }
        accepted.into_iter().map(|a| a.resolved).collect()
    } else {
        let m_a = ca.segs.len() as f64;
        let m_b_raw = cb.segs.len() as f64;
        let reverse_cb = if candidates.len() >= 2 && allow_reverse {
            let (_, _, pt_b0, pos_a0) = candidates[0];
            let (_, _, pt_b1, pos_a1) = candidates[1];
            let pos_b0 = project_onto_contour(&cb, pt_b0).0;
            let pos_b1 = project_onto_contour(&cb, pt_b1).0;
            (pos_a1 > pos_a0 && pos_b1 < pos_b0) || (pos_a1 < pos_a0 && pos_b1 > pos_b0)
        } else {
            false
        };
        let cb_oriented = if reverse_cb { cb.reversed() } else { cb };
        let m_b = cb_oriented.segs.len().max(1) as f64;
        let _ = m_b_raw;

        struct AcceptedOpen {
            resolved: ResolvedHint,
            ua: f64,
            ub: f64,
        }
        let mut accepted: Vec<AcceptedOpen> = Vec::new();
        for (label, _pt_a, pt_b, pos_a) in candidates {
            let (pos_b, _) = project_onto_contour(&cb_oriented, pt_b);
            let ua = (pos_a / m_a).clamp(0.0, 1.0);
            let ub = (pos_b / m_b).clamp(0.0, 1.0);
            let order_ok = accepted.iter().all(|prev| {
                (ua - prev.ua).abs() > 1e-4
                    && (ub - prev.ub).abs() > 1e-4
                    && ((ua < prev.ua) == (ub < prev.ub))
            });
            if order_ok {
                accepted.push(AcceptedOpen {
                    resolved: ResolvedHint {
                        label,
                        pos_a,
                        pos_b,
                        reverse_cb,
                    },
                    ua,
                    ub,
                });
            }
        }
        accepted.into_iter().map(|a| a.resolved).collect()
    }
}

fn subcontour_open(c: &Contour, pos0: f64, pos1: f64) -> Contour {
    let m = c.segs.len();
    let p0 = pos0.clamp(0.0, m as f64);
    let p1 = pos1.clamp(0.0, m as f64);
    if p1 <= p0 + 1e-6 || m == 0 {
        let pt = contour_point_at(c, p0);
        return Contour {
            start: pt,
            segs: Vec::new(),
            closed: false,
        };
    }
    let mut segs = Vec::new();
    let mut first_start = None;
    for idx in 0..m {
        let u0 = (p0 - idx as f64).clamp(0.0, 1.0);
        let u1 = (p1 - idx as f64).clamp(0.0, 1.0);
        if u1 - u0 <= 1e-6 {
            continue;
        }
        let seg_start = if idx == 0 {
            c.start
        } else {
            c.segs[idx - 1][2]
        };
        let s = c.segs[idx];
        let (piece_start, piece_seg) = if u0 > 1e-6 {
            let (left, right) = split(seg_start, &s, u0);
            let r_start = left[2];
            let rem = ((u1 - u0) / (1.0 - u0)).clamp(0.0, 1.0);
            if rem < 1.0 - 1e-6 {
                let (mid, _) = split(r_start, &right, rem);
                (r_start, mid)
            } else {
                (r_start, right)
            }
        } else if u1 < 1.0 - 1e-6 {
            let (left, _) = split(seg_start, &s, u1);
            (seg_start, left)
        } else {
            (seg_start, s)
        };
        if first_start.is_none() {
            first_start = Some(piece_start);
        }
        segs.push(piece_seg);
    }
    let start = first_start.unwrap_or_else(|| contour_point_at(c, p0));
    Contour {
        start,
        segs,
        closed: false,
    }
}

fn concat_open(a: Contour, b: Contour) -> Contour {
    if a.segs.is_empty() {
        return b;
    }
    if b.segs.is_empty() {
        return a;
    }
    let mut segs = a.segs;
    if let Some(last) = segs.last_mut() {
        last[2] = b.start;
    }
    segs.extend(b.segs);
    Contour {
        start: a.start,
        segs,
        closed: false,
    }
}

fn ensure_non_empty_open(mut c: Contour) -> Contour {
    if c.segs.is_empty() {
        c.segs.push(line(c.start, c.start));
    }
    c
}

fn cyclic_slice_full(c: &Contour, pos: f64) -> Contour {
    let m = c.segs.len() as f64;
    let p = pos.rem_euclid(m);
    if p <= 1e-5 || (m - p) <= 1e-5 {
        return Contour {
            start: c.start,
            segs: c.segs.clone(),
            closed: c.closed,
        };
    }
    let part1 = subcontour_open(c, p, m);
    let part2 = subcontour_open(c, 0.0, p);
    let mut combined = ensure_non_empty_open(concat_open(part1, part2));
    if let Some(last) = combined.segs.last_mut() {
        last[2] = combined.start;
    }
    combined.closed = c.closed;
    combined
}

fn cyclic_slice_arc(c: &Contour, pos0: f64, pos1: f64) -> Contour {
    let m = c.segs.len() as f64;
    let p0 = pos0.rem_euclid(m);
    let p1 = pos1.rem_euclid(m);
    let raw = if p0 + 1e-5 < p1 {
        subcontour_open(c, p0, p1)
    } else {
        let part1 = subcontour_open(c, p0, m);
        let part2 = subcontour_open(c, 0.0, p1);
        concat_open(part1, part2)
    };
    ensure_non_empty_open(raw)
}

fn resample_paired_pieces(
    pieces: &[(Contour, Contour)],
    target_n: usize,
    closed: bool,
) -> (Contour, Contour) {
    let mins: Vec<usize> = pieces
        .iter()
        .map(|(a, b)| a.segs.len().max(b.segs.len()).max(1))
        .collect();
    let total_min: usize = mins.iter().sum();
    let total_n = target_n.max(total_min);
    let extra = total_n - total_min;
    let weights: Vec<f64> = pieces
        .iter()
        .map(|(a, b)| contour_arc_length(a) + contour_arc_length(b))
        .collect();
    let weight_sum: f64 = weights.iter().sum();
    let mut counts = mins.clone();
    if extra > 0 {
        if weight_sum.is_finite() && weight_sum > 0.0 {
            let ideal: Vec<f64> = weights
                .iter()
                .map(|w| extra as f64 * w / weight_sum)
                .collect();
            let mut add: Vec<usize> = ideal.iter().map(|v| v.floor() as usize).collect();
            let assigned: usize = add.iter().sum();
            let mut order: Vec<usize> = (0..pieces.len()).collect();
            order.sort_by(|&i, &j| {
                (ideal[j] - ideal[j].floor())
                    .total_cmp(&(ideal[i] - ideal[i].floor()))
                    .then(i.cmp(&j))
            });
            for &idx in order.iter().take(extra.saturating_sub(assigned)) {
                add[idx] += 1;
            }
            for (c, a) in counts.iter_mut().zip(add) {
                *c += a;
            }
        } else {
            for (i, count) in counts.iter_mut().enumerate().take(pieces.len()) {
                *count += extra / pieces.len() + usize::from(i < extra % pieces.len());
            }
        }
    }

    let mut segs_a: Vec<Seg> = Vec::with_capacity(total_n);
    let mut segs_b: Vec<Seg> = Vec::with_capacity(total_n);
    let start_a = pieces[0].0.start;
    let start_b = pieces[0].1.start;
    for ((pa, pb), &nj) in pieces.iter().zip(&counts) {
        let ra = resample(pa, nj);
        let rb = resample(pb, nj);
        if let Some(prev_last) = segs_a.last_mut() {
            prev_last[2] = ra.start;
        }
        if let Some(prev_last) = segs_b.last_mut() {
            prev_last[2] = rb.start;
        }
        segs_a.extend(ra.segs);
        segs_b.extend(rb.segs);
    }
    if closed {
        if let Some(last) = segs_a.last_mut() {
            last[2] = start_a;
        }
        if let Some(last) = segs_b.last_mut() {
            last[2] = start_b;
        }
    }
    (
        Contour {
            start: start_a,
            segs: segs_a,
            closed,
        },
        Contour {
            start: start_b,
            segs: segs_b,
            closed,
        },
    )
}

fn morph_contour_pair(
    ca: &Contour,
    cb_raw: &Contour,
    cyclic: bool,
    allow_reverse: bool,
    resolved: &[ResolvedHint],
    n: usize,
) -> (Contour, Contour) {
    if resolved.is_empty() {
        let ra = resample(ca, n);
        let rb = align_unhinted(&ra, &resample(cb_raw, n), cyclic, allow_reverse);
        return (ra, rb);
    }
    let reverse_cb = resolved[0].reverse_cb;
    let cb = if reverse_cb {
        cb_raw.reversed()
    } else {
        cb_raw.clone()
    };
    let m_a = ca.segs.len() as f64;
    let m_b = cb.segs.len() as f64;

    if cyclic {
        if resolved.len() == 1 {
            let h = resolved[0];
            let ca_rot = cyclic_slice_full(ca, h.pos_a);
            let cb_rot = cyclic_slice_full(&cb, h.pos_b);
            let target_n = n.max(ca_rot.segs.len()).max(cb_rot.segs.len());
            let ra = resample(&ca_rot, target_n);
            let rb = resample(&cb_rot, target_n);
            return (ra, rb);
        }
        let origin_ua = (resolved[0].pos_a / m_a).rem_euclid(1.0);
        let mut ordered = resolved.to_vec();
        ordered.sort_by(|x, y| {
            let dx = ((x.pos_a / m_a) - origin_ua).rem_euclid(1.0);
            let dy = ((y.pos_a / m_a) - origin_ua).rem_euclid(1.0);
            dx.total_cmp(&dy)
        });
        let k = ordered.len();
        let mut pieces = Vec::with_capacity(k);
        for j in 0..k {
            let next = (j + 1) % k;
            let pa = cyclic_slice_arc(ca, ordered[j].pos_a, ordered[next].pos_a);
            let pb = cyclic_slice_arc(&cb, ordered[j].pos_b, ordered[next].pos_b);
            pieces.push((pa, pb));
        }
        resample_paired_pieces(&pieces, n, ca.closed)
    } else {
        let mut ordered = resolved.to_vec();
        ordered.sort_by(|x, y| x.pos_a.total_cmp(&y.pos_a));
        let mut breaks_a = Vec::with_capacity(ordered.len() + 2);
        let mut breaks_b = Vec::with_capacity(ordered.len() + 2);
        breaks_a.push(0.0);
        breaks_b.push(0.0);
        for h in &ordered {
            breaks_a.push(h.pos_a.clamp(0.0, m_a));
            breaks_b.push(h.pos_b.clamp(0.0, m_b));
        }
        breaks_a.push(m_a);
        breaks_b.push(m_b);

        let mut pieces = Vec::new();
        for j in 0..(breaks_a.len() - 1) {
            let (a0, a1) = (breaks_a[j], breaks_a[j + 1]);
            let (b0, b1) = (breaks_b[j], breaks_b[j + 1]);
            if a1 - a0 <= 1e-4 && b1 - b0 <= 1e-4 {
                continue;
            }
            let pa = ensure_non_empty_open(subcontour_open(ca, a0, a1));
            let pb = ensure_non_empty_open(subcontour_open(&cb, b0, b1));
            pieces.push((pa, pb));
        }
        if pieces.is_empty() {
            let ra = resample(ca, n);
            let rb = resample(&cb, n);
            return (ra, rb);
        }
        resample_paired_pieces(&pieces, n, false)
    }
}

fn emit(commands: &mut Vec<PathCommand>, start: P, segs: impl Iterator<Item = Seg>, close: bool) {
    commands.push(PathCommand::MoveTo {
        x: start[0],
        y: start[1],
    });
    for s in segs {
        commands.push(PathCommand::CubicTo {
            cx1: s[0][0],
            cy1: s[0][1],
            cx2: s[1][0],
            cy2: s[1][1],
            x: s[2][0],
            y: s[2][1],
        });
    }
    if close {
        commands.push(PathCommand::Close);
    }
}

/// Emit a contour moved toward `centre` by `amount` (0 = unchanged, 1 = point).
fn emit_toward(commands: &mut Vec<PathCommand>, c: &Contour, centre: P, amount: f64) {
    let m = |p: P| lerp_p(p, centre, amount);
    emit(
        commands,
        m(c.start),
        c.segs.iter().map(|s| [m(s[0]), m(s[1]), m(s[2])]),
        c.closed,
    );
}

fn shape_pair_has_active_hints(
    a: &Shape,
    b: &Shape,
    hints: &[crate::ShapeHint],
    a_bounds: (P, P),
    b_bounds: (P, P),
) -> bool {
    if hints.is_empty() {
        return false;
    }
    let pairs = a.contours.len().min(b.contours.len());
    let allow_reverse = b.contours.len() == 1 || b.paint.fill_rule == FillRule::EvenOdd;
    (0..pairs).any(|i| {
        !resolve_contour_hints(a, b, i, hints, a_bounds, b_bounds, allow_reverse).is_empty()
    })
}

/// Contour-by-contour morph; `None` when the segment budget is exceeded.
fn morph_contours(
    a: &Shape,
    b: &Shape,
    hints: &[crate::ShapeHint],
    a_bounds: (P, P),
    b_bounds: (P, P),
    t: f64,
) -> Option<Vec<PathCommand>> {
    let pairs = a.contours.len().min(b.contours.len());
    let mut budget = 0usize;
    for i in 0..pairs {
        budget += contour_segments(&a.contours[i], &b.contours[i]);
    }
    budget += a.contours[pairs..]
        .iter()
        .chain(&b.contours[pairs..])
        .map(|c| c.segs.len())
        .sum::<usize>();
    if budget > MAX_SHAPE_TWEEN_SEGMENTS {
        return None;
    }
    // Direction only changes the fill of a multi-contour nonzero target.
    let allow_reverse = b.contours.len() == 1 || b.paint.fill_rule == FillRule::EvenOdd;
    let mut commands = Vec::with_capacity(budget + 2 * (a.contours.len() + b.contours.len()));
    for i in 0..pairs {
        // The source decides: a cyclic source makes the target cyclic too.
        // Closed contours already end at their start (see `contours`).
        let ca = &a.contours[i];
        let mut cb = b.contours[i].clone();
        let cyclic = ca.cyclic();
        if cyclic {
            cb.make_cyclic();
        }
        let n = contour_segments(ca, &cb);
        let resolved = resolve_contour_hints(a, b, i, hints, a_bounds, b_bounds, allow_reverse);
        let (ra, rb) = morph_contour_pair(ca, &cb, cyclic, allow_reverse, &resolved, n);
        let close = if t < 0.5 { ca.closed } else { cb.closed };
        emit(
            &mut commands,
            lerp_p(ra.start, rb.start, t),
            ra.segs.iter().zip(&rb.segs).map(|(x, y)| {
                [
                    lerp_p(x[0], y[0], t),
                    lerp_p(x[1], y[1], t),
                    lerp_p(x[2], y[2], t),
                ]
            }),
            close,
        );
    }
    if t < 1. {
        for c in &a.contours[pairs..] {
            emit_toward(&mut commands, c, c.bounds_centre(), t);
        }
    }
    if t > 0. {
        for c in &b.contours[pairs..] {
            emit_toward(&mut commands, c, c.bounds_centre(), 1. - t);
        }
    }
    Some(commands)
}

fn compatible(a: &GradientPaint, b: &GradientPaint) -> bool {
    a.kind == b.kind && a.stops.len() == b.stops.len()
}

/// The gradient `like` repainted in one solid colour.
fn solid_like(like: &GradientPaint, color: Color) -> GradientPaint {
    GradientPaint {
        stops: like
            .stops
            .iter()
            .map(|s| GradientStop {
                ratio: s.ratio,
                color,
            })
            .collect(),
        ..like.clone()
    }
}

fn lerp_gradient(a: &GradientPaint, b: &GradientPaint, t: f64) -> GradientPaint {
    GradientPaint {
        kind: a.kind,
        matrix: std::array::from_fn(|i| lerp(a.matrix[i], b.matrix[i], t)),
        stops: a
            .stops
            .iter()
            .zip(&b.stops)
            .map(|(x, y)| GradientStop {
                ratio: lerp(x.ratio as f64, y.ratio as f64, t)
                    .clamp(0., 255.)
                    .round() as u8,
                color: color_lerp(x.color, y.color, t),
            })
            .collect(),
        focal_point_ratio: lerp(a.focal_point_ratio, b.focal_point_ratio, t).clamp(-1.0, 1.0),
        spread: if t < 0.5 { a.spread } else { b.spread },
        linear_rgb: if t < 0.5 { a.linear_rgb } else { b.linear_rgb },
    }
}

/// Solid ↔ gradient morphs through the gradient repainted in the solid colour;
/// structurally different gradients (kind or stop count) switch at t = 0.5.
fn paint_gradient(
    a: Option<&GradientPaint>,
    a_color: Color,
    b: Option<&GradientPaint>,
    b_color: Color,
    t: f64,
) -> Option<GradientPaint> {
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(lerp_gradient(a, &solid_like(a, b_color), t)),
        (None, Some(b)) => Some(lerp_gradient(&solid_like(b, a_color), b, t)),
        (Some(a), Some(b)) if compatible(a, b) => Some(lerp_gradient(a, b, t)),
        (Some(a), Some(b)) => Some(if t < 0.5 { a.clone() } else { b.clone() }),
    }
}

fn lerp_paint(a: &Paint, b: &Paint, t: f64) -> Paint {
    Paint {
        fill: color_lerp(a.fill, b.fill, t),
        stroke: color_lerp(a.stroke, b.stroke, t),
        fill_gradient: paint_gradient(
            a.fill_gradient.as_ref(),
            a.fill,
            b.fill_gradient.as_ref(),
            b.fill,
            t,
        ),
        stroke_gradient: paint_gradient(
            a.stroke_gradient.as_ref(),
            a.stroke,
            b.stroke_gradient.as_ref(),
            b.stroke,
            t,
        ),
        stroke_width: lerp(a.stroke_width, b.stroke_width, t).max(0.),
        fill_rule: if t < 0.5 { a.fill_rule } else { b.fill_rule },
    }
}

/// Geometrically identity placement that keeps the non-geometric channels.
fn placement(a: &Transform, b: &Transform, t: f64) -> Transform {
    Transform {
        alpha: lerp(a.alpha, b.alpha, t).clamp(0.0, 1.0),
        color_transform: ColorTransform {
            multipliers: std::array::from_fn(|i| {
                lerp(
                    a.color_transform.multipliers[i],
                    b.color_transform.multipliers[i],
                    t,
                )
            }),
            offsets: std::array::from_fn(|i| {
                lerp(
                    a.color_transform.offsets[i],
                    b.color_transform.offsets[i],
                    t,
                )
            }),
        },
        ..Transform::default()
    }
}

fn build(
    template: &Element,
    commands: Vec<PathCommand>,
    paint: Paint,
    transform: Transform,
) -> Element {
    let mut element = template.clone();
    element.transform = transform;
    element.kind = if paint.fill_gradient.is_some() || paint.stroke_gradient.is_some() {
        ElementKind::PaintedPath {
            commands,
            fill_rule: paint.fill_rule,
            fill: paint.fill,
            stroke: paint.stroke,
            fill_gradient: paint.fill_gradient,
            stroke_gradient: paint.stroke_gradient,
            stroke_width: paint.stroke_width,
        }
    } else {
        ElementKind::Path {
            commands,
            fill_rule: paint.fill_rule,
            fill: paint.fill,
            stroke: paint.stroke,
            stroke_width: paint.stroke_width,
        }
    };
    element
}

fn morph_pair_with_hints(
    a: &Element,
    b: &Element,
    hints: &[crate::ShapeHint],
    from_bounds: Option<(P, P)>,
    to_bounds: Option<(P, P)>,
    t: f64,
) -> Element {
    let (Some(sa), Some(sb)) = (parent_shape(a), parent_shape(b)) else {
        return a.clone();
    };
    let a_bounds = from_bounds.unwrap_or_else(|| shape_sampled_bounds(&sa));
    let b_bounds = to_bounds.unwrap_or_else(|| shape_sampled_bounds(&sb));
    let has_active_hints = shape_pair_has_active_hints(&sa, &sb, hints, a_bounds, b_bounds);
    let commands = if !has_active_hints && paths_have_same_topology(&sa.commands, &sb.commands) {
        interpolate_commands(&sa.commands, &sb.commands, t)
    } else {
        match morph_contours(&sa, &sb, hints, a_bounds, b_bounds, t) {
            Some(commands) => commands,
            None => return a.clone(),
        }
    };
    build(
        a,
        commands,
        lerp_paint(&sa.paint, &sb.paint, t),
        placement(&a.transform, &b.transform, t),
    )
}

/// An unpaired shape moved toward the centre of its bounds by `amount`.
fn collapse_shape(element: &Element, amount: f64) -> Option<Element> {
    let shape = parent_shape(element)?;
    if shape.contours.iter().map(|c| c.segs.len()).sum::<usize>() > MAX_SHAPE_TWEEN_SEGMENTS {
        return None;
    }
    let centre = bounds_centre(shape.contours.iter().flat_map(|c| c.vertices()));
    let mut commands = Vec::new();
    for c in &shape.contours {
        emit_toward(&mut commands, c, centre, amount);
    }
    Some(build(
        element,
        commands,
        shape.paint,
        placement(&element.transform, &element.transform, 0.),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{FlattenOptions, hit_test_path, path_bounds};

    fn element(id: &str, kind: ElementKind, transform: Transform) -> Element {
        Element {
            id: id.into(),
            name: format!("{id} name"),
            transform,
            kind,
            scripts: Vec::new(),
            accessibility: None,
            ..Element::default()
        }
    }
    fn at(x: f64, y: f64) -> Transform {
        Transform {
            x,
            y,
            ..Transform::default()
        }
    }
    fn rect(width: f64, height: f64) -> ElementKind {
        ElementKind::Rectangle {
            width,
            height,
            fill: [200, 0, 0, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 2.,
        }
    }
    fn ellipse(width: f64, height: f64) -> ElementKind {
        ElementKind::Ellipse {
            width,
            height,
            fill: [0, 0, 200, 255],
            stroke: [0, 0, 0, 255],
            stroke_width: 4.,
        }
    }
    fn polygon(contours: &[&[P]]) -> ElementKind {
        let mut commands = Vec::new();
        for points in contours {
            commands.push(PathCommand::MoveTo {
                x: points[0][0],
                y: points[0][1],
            });
            for p in &points[1..] {
                commands.push(PathCommand::LineTo { x: p[0], y: p[1] });
            }
            commands.push(PathCommand::Close);
        }
        ElementKind::Path {
            commands,
            fill_rule: FillRule::NonZero,
            fill: [10, 20, 30, 255],
            stroke: [0, 0, 0, 0],
            stroke_width: 0.,
        }
    }
    fn commands(e: &Element) -> &[PathCommand] {
        match &e.kind {
            ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
                commands
            }
            other => panic!("expected a path, got {other:?}"),
        }
    }
    fn bounds(e: &Element) -> [f64; 4] {
        let b = path_bounds(commands(e)).unwrap().unwrap();
        [b.x_min, b.y_min, b.x_max, b.y_max]
    }
    fn near(a: [f64; 4], b: [f64; 4], tolerance: f64) {
        assert!(
            a.iter().zip(&b).all(|(x, y)| (x - y).abs() < tolerance),
            "{a:?} != {b:?}"
        );
    }
    fn inside(e: &Element, p: P) -> bool {
        hit_test_path(
            commands(e),
            FillRule::NonZero,
            p,
            true,
            0.,
            FlattenOptions::default(),
        )
        .unwrap()
    }
    fn on_curve(e: &Element) -> Vec<P> {
        commands(e)
            .iter()
            .filter_map(|c| match *c {
                PathCommand::MoveTo { x, y }
                | PathCommand::LineTo { x, y }
                | PathCommand::QuadraticTo { x, y, .. }
                | PathCommand::CubicTo { x, y, .. } => Some([x, y]),
                PathCommand::Close => None,
            })
            .collect()
    }
    fn contour_count(e: &Element) -> usize {
        commands(e)
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
            .count()
    }

    #[test]
    fn rectangle_to_ellipse_reproduces_both_keys_and_morphs_linearly() {
        let from = [element("a", rect(100., 60.), at(10., 20.))];
        let to = [element("b", ellipse(100., 60.), at(10., 20.))];
        let start = &shape_tween_elements(&from, &to, 0.)[0];
        let mid = &shape_tween_elements(&from, &to, 0.5)[0];
        let end = &shape_tween_elements(&from, &to, 1.)[0];
        near(bounds(start), [10., 20., 110., 80.], 1e-9);
        near(bounds(end), [10., 20., 110., 80.], 1e-6);
        for [x, y] in on_curve(start) {
            let on_edge = (x - 10.).abs() < 1e-9
                || (x - 110.).abs() < 1e-9
                || (y - 20.).abs() < 1e-9
                || (y - 80.).abs() < 1e-9;
            assert!(on_edge, "({x}, {y}) is not on the rectangle");
        }
        for [x, y] in on_curve(end) {
            let r = ((x - 60.) / 50.).powi(2) + ((y - 50.) / 30.).powi(2);
            assert!((r - 1.).abs() < 1e-3, "({x}, {y}) is not on the ellipse");
        }
        assert!(inside(start, [11., 21.]));
        assert!(!inside(end, [11., 21.]));
        assert!(inside(end, [60., 50.]) && inside(mid, [60., 50.]));
        // One resampling for every t: the midpoint is the average of both keys.
        for ((a, b), m) in on_curve(start).iter().zip(on_curve(end)).zip(on_curve(mid)) {
            assert!((lerp(a[0], b[0], 0.5) - m[0]).abs() < 1e-9);
            assert!((lerp(a[1], b[1], 0.5) - m[1]).abs() < 1e-9);
        }
        let ElementKind::Path {
            fill, stroke_width, ..
        } = &mid.kind
        else {
            panic!("expected a path")
        };
        assert_eq!(*fill, [100, 0, 100, 255]);
        assert_eq!(*stroke_width, 3.);
        assert_eq!(mid.id, "a");
        assert_eq!(
            shape_tween_elements(&from, &to, 0.37),
            shape_tween_elements(&from, &to, 0.37)
        );
    }

    #[test]
    fn same_topology_paths_keep_plain_command_interpolation() {
        let a = polygon(&[&[[0., 0.], [10., 0.], [10., 10.], [0., 10.]]]);
        let b = polygon(&[&[[5., 5.], [30., 0.], [40., 25.], [0., 30.]]]);
        let from = [element("p", a.clone(), Transform::default())];
        let to = [element("p", b.clone(), Transform::default())];
        let out = shape_tween_elements(&from, &to, 0.3);
        let (ElementKind::Path { commands: ca, .. }, ElementKind::Path { commands: cb, .. }) =
            (&a, &b)
        else {
            unreachable!()
        };
        let mut expected = a.clone();
        if let ElementKind::Path { commands, .. } = &mut expected {
            *commands = interpolate_commands(ca, cb, 0.3);
        }
        assert_eq!(out[0].kind, expected);
    }

    #[test]
    fn triangle_morphs_into_square_with_different_command_counts() {
        let from = [element(
            "t",
            polygon(&[&[[0., 100.], [50., 0.], [100., 100.]]]),
            Transform::default(),
        )];
        let to = [element(
            "s",
            polygon(&[&[[0., 0.], [100., 0.], [100., 100.], [0., 100.]]]),
            Transform::default(),
        )];
        let start = &shape_tween_elements(&from, &to, 0.)[0];
        let end = &shape_tween_elements(&from, &to, 1.)[0];
        near(bounds(start), [0., 0., 100., 100.], 1e-9);
        near(bounds(end), [0., 0., 100., 100.], 1e-9);
        assert!(!inside(start, [5., 5.]) && inside(end, [5., 5.]));
        assert!(inside(start, [50., 60.]) && inside(end, [50., 60.]));
        assert_eq!(commands(start).len(), commands(end).len());
        assert_eq!(commands(start).len(), 1 + MIN_CONTOUR_SEGMENTS + 1);
        for corner in [[0., 0.], [100., 0.], [100., 100.], [0., 100.]] {
            assert!(on_curve(end).iter().any(|p| dist2(*p, corner) < 1e-12));
        }
    }

    #[test]
    fn extra_contours_collapse_to_their_centre() {
        let square = |x: f64| [[x, 0.], [x + 20., 0.], [x + 20., 20.], [x, 20.]];
        let from = [element(
            "two",
            polygon(&[&square(0.), &square(100.)]),
            Transform::default(),
        )];
        let to = [element(
            "one",
            polygon(&[&square(40.)]),
            Transform::default(),
        )];
        let start = &shape_tween_elements(&from, &to, 0.)[0];
        assert!(inside(start, [10., 10.]) && inside(start, [110., 10.]));
        let mid = &shape_tween_elements(&from, &to, 0.5)[0];
        assert_eq!(contour_count(mid), 2);
        assert!(inside(mid, [110., 10.]) && !inside(mid, [104., 4.]));
        let end = &shape_tween_elements(&from, &to, 1.)[0];
        assert_eq!(contour_count(end), 1);
        near(bounds(end), [40., 0., 60., 20.], 1e-9);
        // The reverse direction grows the missing contour from its centre.
        let grow = &shape_tween_elements(&to, &from, 0.5)[0];
        assert!(inside(grow, [110., 10.]) && !inside(grow, [104., 4.]));
        assert_eq!(contour_count(&shape_tween_elements(&to, &from, 0.)[0]), 1);
    }

    #[test]
    fn unpaired_shapes_shrink_or_grow_and_other_elements_hold() {
        let video = element(
            "video",
            ElementKind::Video {
                width: 40.,
                height: 30.,
            },
            at(5., 5.),
        );
        let from = [
            element("a", rect(10., 10.), Transform::default()),
            video.clone(),
            element("b", rect(20., 20.), at(100., 100.)),
        ];
        let to = [element("a", rect(30., 30.), Transform::default())];
        let mid = shape_tween_elements(&from, &to, 0.5);
        assert_eq!(mid.len(), 3);
        assert_eq!(mid[1], video);
        near(bounds(&mid[0]), [0., 0., 20., 20.], 1e-9);
        near(bounds(&mid[2]), [105., 105., 115., 115.], 1e-9);
        assert_eq!(shape_tween_elements(&from, &to, 1.).len(), 2);
        // Swapped keys: the extra target shape is appended and grows.
        let grow = shape_tween_elements(&to, &from, 0.25);
        assert_eq!(grow.len(), 2);
        assert_eq!(grow[1].id, "b");
        near(bounds(&grow[1]), [107.5, 107.5, 112.5, 112.5], 1e-9);
        assert_eq!(shape_tween_elements(&to, &from, 0.).len(), 1);
    }

    #[test]
    fn shapes_pair_by_id_before_order() {
        let from = [
            element("a", rect(10., 10.), Transform::default()),
            element("b", rect(10., 10.), at(100., 0.)),
        ];
        let to = [
            element("b", ellipse(10., 10.), at(200., 0.)),
            element("a", ellipse(10., 10.), at(0., 50.)),
        ];
        let end = shape_tween_elements(&from, &to, 1.);
        assert_eq!(end[0].id, "a");
        near(bounds(&end[0]), [0., 50., 10., 60.], 1e-6);
        near(bounds(&end[1]), [200., 0., 210., 10.], 1e-6);
    }

    #[test]
    fn output_is_a_path_with_identity_geometry_transform() {
        let mut from_transform = at(50., 50.);
        from_transform.rotation = 90.;
        from_transform.scale_x = 2.;
        from_transform.alpha = 0.5;
        let from = element("r", rect(10., 20.), from_transform);
        let to = element("e", ellipse(10., 10.), at(0., 0.));
        let start = &shape_tween_elements(std::slice::from_ref(&from), &[to], 0.)[0];
        let t = start.transform;
        assert_eq!(
            (t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.skew),
            (0., 0., 1., 1., 0., 0.)
        );
        assert_eq!(t.alpha, 0.5);
        assert_eq!((&start.id, &start.name), (&from.id, &from.name));
        // 90 degrees maps local x to parent y and local y to parent -x.
        near(bounds(start), [30., 50., 50., 70.], 1e-9);
        let ElementKind::Path { stroke_width, .. } = start.kind else {
            panic!("expected a path")
        };
        assert!((stroke_width - 2. * 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn solid_fill_morphs_into_a_gradient() {
        let gradient = GradientPaint {
            kind: crate::GradientKind::Linear,
            matrix: [0.01, 0., 0., 0.01, 5., 5.],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 255, 255, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 0, 255],
                },
            ],
            focal_point_ratio: 0.,
            ..Default::default()
        };
        let painted = ElementKind::PaintedPath {
            commands: degenerate_box(10., 10.),
            fill_rule: FillRule::NonZero,
            fill: [0, 0, 0, 255],
            stroke: [0, 0, 0, 0],
            fill_gradient: Some(gradient.clone()),
            stroke_gradient: None,
            stroke_width: 0.,
        };
        let from = [element("a", rect(10., 10.), Transform::default())];
        let to = [element("a", painted, at(10., 0.))];
        let gradient_at = |t: f64| match shape_tween_elements(&from, &to, t).remove(0).kind {
            ElementKind::PaintedPath {
                fill_gradient: Some(g),
                ..
            } => g,
            other => panic!("expected a painted path, got {other:?}"),
        };
        assert!(
            gradient_at(0.)
                .stops
                .iter()
                .all(|s| s.color == [200, 0, 0, 255])
        );
        let end = gradient_at(1.);
        assert_eq!(end.stops, gradient.stops);
        assert_eq!(end.matrix, [0.01, 0., 0., 0.01, 15., 5.]);
    }

    #[test]
    fn segment_cap_holds_the_from_shape() {
        let points: Vec<P> = (0..=MAX_SHAPE_TWEEN_SEGMENTS)
            .map(|i| {
                let a = i as f64 / MAX_SHAPE_TWEEN_SEGMENTS as f64 * std::f64::consts::TAU;
                [100. * a.cos(), 100. * a.sin()]
            })
            .collect();
        let from = [element("big", polygon(&[&points]), at(3., 4.))];
        let to = [element("small", rect(10., 10.), Transform::default())];
        assert_eq!(shape_tween_elements(&from, &to, 0.5), from.to_vec());
    }

    #[test]
    fn shape_hints_steer_contour_vertex_alignment() {
        let from = [element(
            "poly",
            polygon(&[&[[0., 0.], [100., 0.], [100., 100.], [0., 100.]]]),
            Transform::default(),
        )];
        let to = [element(
            "poly",
            polygon(&[&[[0., 0.], [100., 0.], [100., 100.], [0., 100.]]]),
            Transform::default(),
        )];
        let unhinted = &shape_tween_elements(&from, &to, 0.5)[0];
        // Hint top-left (0,0) of start to bottom-right (1,1) of end, forcing a rotated alignment.
        let hints = [crate::ShapeHint {
            label: 'a',
            start_uv: [0.0, 0.0],
            end_uv: [1.0, 1.0],
        }];
        let hinted = &shape_tween_elements_with_hints(&from, &to, &hints, 0.5)[0];
        assert_ne!(commands(unhinted), commands(hinted));
        // At t = 0.5, the vertex starting at (0,0) is pulled toward (100,100), so its midpoint is (50,50).
        let pts = on_curve(hinted);
        assert!(
            pts.iter().any(|p| dist2(*p, [50.0, 50.0]) < 1e-6),
            "expected hinted vertex midpoint at (50, 50), got {pts:?}"
        );
    }
}
