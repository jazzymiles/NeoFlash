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
use crate::geometry::{FlattenOptions, MAX_PATH_COMMANDS, flatten_path};
use crate::model::{Color, Element, ElementKind, FillRule, Id, PathCommand, Transform, new_id};
use crate::runtime_graphics::GradientPaint;

const SNAP_EPS: f64 = 1e-2;
const PARAM_EPS: f64 = 1e-6;
const AREA_EPS: f64 = 0.25;
const ADJACENT_TOL: f64 = 1.5;
// Keep batch inputs below both public path limits. The point headroom matters for curves and for
// the closing point accounted for by the shared flattener; a failed batch must never consume art.
const REMERGE_BATCH_MAX_COMMANDS: usize = MAX_PATH_COMMANDS * 9 / 10;
const REMERGE_BATCH_MAX_POINTS: usize = 60_000;

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
    // Width zero disables rendering; it does not mean the stored paint was cleared.
    // Retain dormant stroke settings when normalizing or rebuilding a Merge fill.
    let (stroke, stroke_gradient) = match &template.kind {
        ElementKind::Rectangle {
            stroke,
            stroke_width: 0.,
            ..
        }
        | ElementKind::Ellipse {
            stroke,
            stroke_width: 0.,
            ..
        }
        | ElementKind::Path {
            stroke,
            stroke_width: 0.,
            ..
        } => (*stroke, None),
        ElementKind::PaintedPath {
            stroke,
            stroke_gradient,
            stroke_width: 0.,
            ..
        } => (
            *stroke,
            stroke_gradient
                .as_ref()
                .map(|g| world_gradient(template, g)),
        ),
        _ => ([0; 4], None),
    };
    let kind = match (paint, stroke_gradient) {
        (FillPaint::Solid(color), None) => ElementKind::Path {
            commands,
            fill_rule,
            fill: *color,
            stroke,
            stroke_width: 0.0,
        },
        (FillPaint::Solid(color), Some(stroke_gradient)) => ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill: *color,
            stroke,
            stroke_width: 0.,
            fill_gradient: None,
            stroke_gradient: Some(stroke_gradient),
        },
        (FillPaint::Gradient(grad), stroke_gradient) => ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill: [0, 0, 0, 255],
            stroke,
            fill_gradient: Some(world_gradient(template, grad)),
            stroke_gradient,
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

/// Pre-indexed horizontal Y-slab accelerator for repeated `point_in_rings` queries.
///
/// Because winding number contributions (`+1`, `-1`, `0`) are summed as exact `i32` integers and
/// an edge `(a, b)` only contributes when its Y-span covers `pt[1]`, bucketing non-horizontal
/// edges into Y-slabs produces the exact same `total_winding` as scanning all rings while testing
/// only edges overlapping `pt[1]`'s slab.
struct PreparedRings {
    fill_rule: FillRule,
    bounds: Option<([f64; 2], [f64; 2])>,
    y_min: f64,
    inv_slab_height: f64,
    slabs: Vec<Vec<([f64; 2], [f64; 2])>>,
}

impl PreparedRings {
    fn new(rings: &[Vec<[f64; 2]>], fill_rule: FillRule) -> Self {
        let bounds = rings_bounds(rings);
        let Some((min, max)) = bounds else {
            return Self {
                fill_rule,
                bounds: None,
                y_min: 0.0,
                inv_slab_height: 1.0,
                slabs: Vec::new(),
            };
        };
        let total_edges: usize = rings.iter().filter(|r| r.len() >= 3).map(|r| r.len()).sum();
        let num_slabs = (total_edges / 4).clamp(16, 8192);
        let y_min = min[1];
        let height = (max[1] - min[1]).max(1e-6);
        let inv_slab_height = (num_slabs as f64) / height;
        let mut slabs = vec![Vec::new(); num_slabs];
        let max_slab = num_slabs as isize - 1;

        for ring in rings {
            let n = ring.len();
            if n < 3 {
                continue;
            }
            for i in 0..n {
                let a = ring[i];
                let b = ring[(i + 1) % n];
                if a[1] == b[1] {
                    continue;
                }
                let ey_min = a[1].min(b[1]);
                let ey_max = a[1].max(b[1]);
                let s0 = (((ey_min - y_min) * inv_slab_height).floor() as isize).clamp(0, max_slab)
                    as usize;
                let s1 = (((ey_max - y_min) * inv_slab_height).floor() as isize).clamp(0, max_slab)
                    as usize;
                for slab in &mut slabs[s0..=s1] {
                    slab.push((a, b));
                }
            }
        }

        Self {
            fill_rule,
            bounds,
            y_min,
            inv_slab_height,
            slabs,
        }
    }

    #[inline]
    fn contains(&self, pt: [f64; 2]) -> bool {
        let Some((min, max)) = self.bounds else {
            return false;
        };
        if pt[0] < min[0] || pt[0] > max[0] || pt[1] < min[1] || pt[1] > max[1] {
            return false;
        }
        let max_slab = self.slabs.len() as isize - 1;
        let slab_idx = (((pt[1] - self.y_min) * self.inv_slab_height).floor() as isize)
            .clamp(0, max_slab) as usize;
        let mut total_winding = 0i32;
        for &(a, b) in &self.slabs[slab_idx] {
            if a[1] <= pt[1] {
                if b[1] > pt[1] {
                    let cross = (b[0] - a[0]) * (pt[1] - a[1]) - (pt[0] - a[0]) * (b[1] - a[1]);
                    if cross > 0.0 {
                        total_winding += 1;
                    }
                }
            } else if b[1] <= pt[1] {
                let cross = (b[0] - a[0]) * (pt[1] - a[1]) - (pt[0] - a[0]) * (b[1] - a[1]);
                if cross < 0.0 {
                    total_winding -= 1;
                }
            }
        }
        match self.fill_rule {
            FillRule::NonZero => total_winding != 0,
            FillRule::EvenOdd => total_winding.rem_euclid(2) == 1,
        }
    }
}

fn ring_bounds(ring: &[[f64; 2]]) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY, f64::INFINITY];
    let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    for &p in ring {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    (!ring.is_empty()).then_some((min, max))
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

fn segments_bounds(segs: &[([f64; 2], [f64; 2])]) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY, f64::INFINITY];
    let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    for &(a, b) in segs {
        min[0] = min[0].min(a[0]).min(b[0]);
        min[1] = min[1].min(a[1]).min(b[1]);
        max[0] = max[0].max(a[0]).max(b[0]);
        max[1] = max[1].max(a[1]).max(b[1]);
    }
    (!segs.is_empty()).then_some((min, max))
}

fn commands_control_bounds(commands: &[PathCommand]) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY, f64::INFINITY];
    let mut max = [f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut any = false;
    let mut include = |x: f64, y: f64| {
        min[0] = min[0].min(x);
        min[1] = min[1].min(y);
        max[0] = max[0].max(x);
        max[1] = max[1].max(y);
        any = true;
    };
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => include(x, y),
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                include(cx, cy);
                include(x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                include(cx1, cy1);
                include(cx2, cy2);
                include(x, y);
            }
            PathCommand::Close => {}
        }
    }
    any.then_some((min, max))
}

fn is_identity_transform(transform: &Transform) -> bool {
    transform.x.abs() <= 1e-9
        && transform.y.abs() <= 1e-9
        && (transform.scale_x - 1.0).abs() <= 1e-9
        && (transform.scale_y - 1.0).abs() <= 1e-9
        && transform.rotation.abs() <= 1e-9
        && transform.skew.abs() <= 1e-9
}

fn element_fast_world_bounds(element: &Element) -> Option<([f64; 2], [f64; 2])> {
    if is_identity_transform(&element.transform)
        && let ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } =
            &element.kind
    {
        return commands_control_bounds(commands);
    }
    let (cmds, _) = element_world_commands(element)?;
    commands_control_bounds(&cmds)
}

#[inline]
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

/// Flat 2D bounding-volume hierarchy over line segments for sub-millisecond proximity and AABB queries.
struct SegmentBvh {
    indices: Vec<usize>,
    nodes: Vec<BvhNode>,
}

#[derive(Clone, Copy)]
struct BvhNode {
    min: [f64; 2],
    max: [f64; 2],
    start: u32,
    count: u32,
    left: u32,
    right: u32,
}

impl SegmentBvh {
    fn from_segments(segments: &[([f64; 2], [f64; 2])]) -> Self {
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

    fn build_node(
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
    fn any_bounds_overlap(
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
    fn any_endpoint_matching(
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

    fn bounds_overlap_indices(
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

    fn node_within_dist(node: &BvhNode, pt: [f64; 2], radius: f64) -> bool {
        pt[0] >= node.min[0] - radius
            && pt[0] <= node.max[0] + radius
            && pt[1] >= node.min[1] - radius
            && pt[1] <= node.max[1] + radius
    }

    fn any_within_distance(
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

    fn any_segment_intersections(
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
struct PreparedMergeRings {
    segments: Vec<([f64; 2], [f64; 2])>,
    segment_bvh: SegmentBvh,
    fill: PreparedRings,
    ring_boxes: Vec<([f64; 2], [f64; 2])>,
    ring_indices: Vec<usize>,
    ring_bvh: SegmentBvh,
}

impl PreparedMergeRings {
    fn new(rings: &[Vec<[f64; 2]>], rule: FillRule) -> Self {
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

    fn rings_overlapping(
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
type ProbeContext<'a> = (&'a [([f64; 2], [f64; 2])], &'a SegmentBvh);

struct PlanarGraph {
    vertices: Vec<[f64; 2]>,
    edges: Vec<(usize, usize)>,
    edge_bvh: SegmentBvh,
}

impl PlanarGraph {
    fn from_segments_with_chains(raw_segments: &[([f64; 2], [f64; 2])]) -> (Self, Vec<Vec<usize>>) {
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

    fn probe_points(&self, edge_idx: usize) -> Option<([f64; 2], [f64; 2])> {
        self.probe_points_with_context(edge_idx, None)
    }

    /// Probe points beside an edge, closer to it than any other graph edge or any `context`
    /// segment (contour edges kept outside the graph by the windowed boolean).
    fn probe_points_with_context(
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

type UnionSegments = Vec<([f64; 2], [f64; 2])>;
type UnionSegmentMap = Vec<Vec<Option<usize>>>;

/// When a `NonZero` compound path consists of many same-orientation contours (such as the
/// overlapping circles, quads, or triangles of a Brush stroke), any sub-segment that lies deep
/// inside another contour can never be part of the union boundary. Clipping each segment against
/// overlapping contours (visiting immediate index neighbors first, while keeping a `4 * SNAP_EPS`
/// crossing stub whenever an edge enters/exits a contour so `PlanarGraph` still computes exact
/// boundary intersections) eliminates the 20-to-40-deep internal dab pileup before building
/// `PlanarGraph`.
fn cull_and_clip_union_segments_with_map(
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

fn build_snapped_rings(
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
fn dissolve_rings_to_islands(
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
fn fill_boundary_segments(
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
    trace_half_edge_cycles_with_chains(vertices, half_edges, &[])
}

/// Trace closed boundary cycles. `chains[idx]`, when present and non-empty, lists the interior
/// points of half-edge `idx` (an untouched run of an existing contour kept verbatim by the
/// windowed boolean); turning angles at its ends use its first/last interior segment.
fn trace_half_edge_cycles_with_chains(
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

fn rings_interact_with_prepared(
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

fn boolean_rings_with_interaction(
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

fn boolean_rings_full(
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
const WINDOW_MIN_EDGES: usize = 256;

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
fn boolean_rings_windowed(
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

fn trim_polyline_inside_prepared_fill(
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

fn has_normalized_merge_part_kind(el: &Element) -> bool {
    if !el.merge {
        return false;
    }
    let (ElementKind::Path {
        commands,
        stroke_width,
        ..
    }
    | ElementKind::PaintedPath {
        commands,
        stroke_width,
        ..
    }) = &el.kind
    else {
        return false;
    };
    if commands.is_empty() {
        return false;
    }
    let has_fill = extract_fill_paint(&el.kind).is_some();
    let has_stroke = extract_stroke_paint(&el.kind).is_some();
    if has_fill && !has_stroke && *stroke_width == 0.0 {
        return true;
    }
    if !has_fill && has_stroke {
        let move_tos = commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
            .count();
        return move_tos <= 1;
    }
    false
}

fn is_already_normalized_merge_part(el: &Element) -> bool {
    is_identity_transform(&el.transform) && has_normalized_merge_part_kind(el)
}

fn ensure_existing_merge_normalized(elements: &mut Vec<Element>) {
    if elements
        .iter()
        .all(|el| !el.merge || is_already_normalized_merge_part(el))
    {
        return;
    }
    let mut normalized = Vec::with_capacity(elements.len());
    for el in elements.drain(..) {
        if !el.merge || is_already_normalized_merge_part(&el) {
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
    apply_merge_shape_to_keyframe_inner(elements, new_element, false)
}

fn apply_merge_shape_to_keyframe_inner(
    elements: &mut Vec<Element>,
    new_element: Element,
    already_dissolved: bool,
) -> Vec<Id> {
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
            // Dissolve internal overlaps of compound / Brush strokes up front so the stroke
            // stores and tests only its clean exterior boundary and holes.
            if new_rings.len() > 1 && !already_dissolved {
                let dissolved = dissolve_rings_to_islands(&new_rings, new_rule);
                if !dissolved.is_empty() {
                    new_cmds = dissolved.into_iter().flatten().collect();
                    new_rule = FillRule::NonZero;
                    new_rings = flatten_closed_rings(&new_cmds);
                }
            }
            let new_bbox_opt = rings_bounds(&new_rings);
            let mut cached_trim_fill: Option<PreparedMergeRings> = None;
            let mut pending_union_rings: Vec<Vec<[f64; 2]>> = Vec::new();

            let mut next_elements = Vec::with_capacity(elements.len() + 2);
            for existing in elements.drain(..) {
                if !existing.merge {
                    next_elements.push(existing);
                    continue;
                }
                let Some(new_bbox) = new_bbox_opt else {
                    next_elements.push(existing);
                    continue;
                };
                if is_merge_fill(&existing) {
                    let Some(ex_fast_bbox) = element_fast_world_bounds(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_fast_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let Some((ex_cmds, ex_rule)) = element_world_commands(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let ex_rings = flatten_closed_rings(&ex_cmds);
                    let Some(ex_bbox) = rings_bounds(&ex_rings) else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let prepared_fill = cached_trim_fill
                        .get_or_insert_with(|| PreparedMergeRings::new(&new_rings, new_rule));
                    if !rings_interact_with_prepared(
                        &ex_rings,
                        ex_rule,
                        ex_bbox,
                        &prepared_fill.segments,
                        &prepared_fill.segment_bvh,
                        &prepared_fill.fill,
                    ) {
                        next_elements.push(existing);
                        continue;
                    }
                    let same_paint = extract_fill_paint(&existing.kind).as_ref() == Some(new_fp);
                    if !has_own_stroke && same_paint {
                        if ex_rule == FillRule::NonZero {
                            pending_union_rings.extend(ex_rings);
                        } else {
                            for island in dissolve_rings_to_islands(&ex_rings, ex_rule) {
                                pending_union_rings.extend(flatten_closed_rings(&island));
                            }
                        }
                        continue;
                    }
                    // Different colour (or separated by new shape's outline stroke): subtract
                    // `new_fill` from `existing`.
                    let local_new_rings =
                        prepared_fill.rings_overlapping(&new_rings, ex_bbox, SNAP_EPS * 4.0);
                    let Some(local_new_bbox) = rings_bounds(&local_new_rings) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let (mut diff, interacted) = boolean_rings_with_interaction(
                        &ex_rings,
                        ex_rule,
                        &local_new_rings,
                        new_rule,
                        BooleanOp::Difference,
                    );
                    if !interacted {
                        next_elements.push(existing);
                        continue;
                    }
                    let extends_outside_new = ex_bbox.0[0] < local_new_bbox.0[0] - SNAP_EPS * 8.0
                        || ex_bbox.0[1] < local_new_bbox.0[1] - SNAP_EPS * 8.0
                        || ex_bbox.1[0] > local_new_bbox.1[0] + SNAP_EPS * 8.0
                        || ex_bbox.1[1] > local_new_bbox.1[1] + SNAP_EPS * 8.0;
                    if diff.is_empty() && extends_outside_new {
                        diff = boolean_rings_full(
                            &ex_rings,
                            ex_rule,
                            &local_new_rings,
                            new_rule,
                            BooleanOp::Difference,
                        )
                        .0;
                        if diff.is_empty() {
                            next_elements.push(existing);
                            continue;
                        }
                    }
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
                    let Some(ex_fast_bbox) = element_fast_world_bounds(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    if !bounds_overlap(ex_fast_bbox, new_bbox, SNAP_EPS * 2.0) {
                        next_elements.push(existing);
                        continue;
                    }
                    let Some((ex_cmds, _)) = element_world_commands(&existing) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let polylines = flatten_stroke_polylines(&ex_cmds);
                    let Some(sp) = extract_stroke_paint(&existing.kind) else {
                        next_elements.push(existing);
                        continue;
                    };
                    let prepared_fill = cached_trim_fill
                        .get_or_insert_with(|| PreparedMergeRings::new(&new_rings, new_rule));
                    let mut piece_idx = 0usize;
                    for poly in polylines {
                        for trimmed in trim_polyline_inside_prepared_fill(
                            &poly,
                            &prepared_fill.segments,
                            &prepared_fill.segment_bvh,
                            &prepared_fill.fill,
                        ) {
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
            if !pending_union_rings.is_empty() {
                let (unioned, _) = boolean_rings_with_interaction(
                    &pending_union_rings,
                    FillRule::NonZero,
                    &new_rings,
                    new_rule,
                    BooleanOp::Union,
                );
                if !unioned.is_empty() {
                    new_cmds = unioned.into_iter().flatten().collect();
                    new_rule = FillRule::NonZero;
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
        let new_stroke_segs = polylines_to_segments(&stroke_polylines_all);
        let new_strokes_bbox = segments_bounds(&new_stroke_segs);

        // 2a. Slice any existing Merge fills crossed by the new stroke(s).
        if !stroke_polylines_all.is_empty()
            && let Some(strokes_bbox) = new_strokes_bbox
        {
            let mut next_elements = Vec::with_capacity(elements.len() + 2);
            for existing in elements.drain(..) {
                if !existing.merge
                    || !is_merge_fill(&existing)
                    || own_fill_id.as_deref() == Some(existing.id.as_str())
                {
                    next_elements.push(existing);
                    continue;
                }
                let Some(ex_fast_bbox) = element_fast_world_bounds(&existing) else {
                    next_elements.push(existing);
                    continue;
                };
                if !bounds_overlap(ex_fast_bbox, strokes_bbox, SNAP_EPS * 2.0) {
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
        let mut existing_stroke_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
        let mut existing_fill_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();

        let mut next_elements = Vec::with_capacity(elements.len() + new_strokes.len());
        for existing in elements.drain(..) {
            if !existing.merge {
                next_elements.push(existing);
                continue;
            }
            let overlaps_new_strokes = new_strokes_bbox
                .zip(element_fast_world_bounds(&existing))
                .is_some_and(|(sb, eb)| bounds_overlap(eb, sb, SNAP_EPS * 2.0));
            if !overlaps_new_strokes {
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
                let poly_bbox = ring_bounds(&poly);
                let mut cutters: Vec<([f64; 2], [f64; 2])> = existing_stroke_segs
                    .iter()
                    .copied()
                    .filter(|&(a, b)| {
                        poly_bbox.is_some_and(|pb| {
                            bounds_overlap(
                                pb,
                                (
                                    [a[0].min(b[0]), a[1].min(b[1])],
                                    [a[0].max(b[0]), a[1].max(b[1])],
                                ),
                                SNAP_EPS * 2.0,
                            )
                        })
                    })
                    .collect();
                cutters.extend(existing_fill_segs.iter().copied().filter(|&(a, b)| {
                    poly_bbox.is_some_and(|pb| {
                        bounds_overlap(
                            pb,
                            (
                                [a[0].min(b[0]), a[1].min(b[1])],
                                [a[0].max(b[0]), a[1].max(b[1])],
                            ),
                            SNAP_EPS * 2.0,
                        )
                    }) && !segment_lies_on_polyline((a, b), &poly)
                }));
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
    if moved_ids.is_empty() {
        return Vec::new();
    }
    let moved_set: std::collections::HashSet<&str> = moved_ids.iter().map(String::as_str).collect();
    let original_moved_ids: Vec<Id> = elements
        .iter()
        .filter(|element| element.merge && moved_set.contains(element.id.as_str()))
        .map(|element| element.id.clone())
        .collect();
    if original_moved_ids.is_empty() {
        return Vec::new();
    }

    // Merge mutates several existing shapes while it works. Keep the operation atomic: a geometry
    // limit or malformed path must leave the already-moved artwork intact instead of deleting the
    // selected elements (the former unbounded batch did exactly that above the flattening budget).
    let before = elements.clone();
    if let Some(ids) = remerge_moved_elements_inner(elements, &moved_set) {
        ids
    } else {
        *elements = before;
        original_moved_ids
    }
}

fn batch_flattened_point_count(commands: &[PathCommand]) -> Option<usize> {
    let rings = flatten_closed_rings(commands);
    if rings.is_empty() || rings.iter().any(|ring| signed_ring_area(ring) <= 0.0) {
        return None;
    }
    // Concatenating NonZero paths is a union only when every candidate contributes non-negative
    // winding everywhere. Strictly positive rings prove that cheaply. Any hole, clockwise shell,
    // or degenerate contour takes the exact sequential path; containment alone is insufficient
    // because two overlapping negative rings can create negative winding that a later positive
    // candidate would cancel.
    rings
        .iter()
        .try_fold(0usize, |total, ring| total.checked_add(ring.len()))
}

/// Bake a Stage transform once so the existing normalized-fill batch can actually accept real
/// drag results. Merge fills are already planar parts; an affine transform preserves that fact.
fn bake_batchable_moved_fill(element: Element) -> Element {
    if is_already_normalized_merge_part(&element)
        || !has_normalized_merge_part_kind(&element)
        || !is_merge_fill(&element)
    {
        return element;
    }
    if let Some((Some(fill), strokes)) = decompose_element_to_merge_parts(&element)
        && strokes.is_empty()
    {
        return fill;
    }
    element
}

/// Compatibility for a temporary compound cutter. The compound output is removed after it updates
/// other paint, so authored IDs and names may differ; presentation and dormant paint state must
/// still agree because they affect that one-pass interaction.
fn merge_fill_batch_compatible(template: &Element, candidate: &Element) -> bool {
    // Filters, object scripts and accessibility operate at element boundaries; even identical
    // values on two disconnected shapes are not equivalent to one combined display object.
    let has_default_presentation = |element: &Element| {
        element.transform == Transform::default()
            && element.scripts.is_empty()
            && element.accessibility.is_none()
            && element.visible
            && !element.cache_as_bitmap
            && element.blend_mode.is_normal()
            && element.filters.is_empty()
    };
    if !has_default_presentation(template) || !has_default_presentation(candidate) {
        return false;
    }
    match (&template.kind, &candidate.kind) {
        (
            ElementKind::Path {
                fill_rule: template_rule,
                fill: template_fill,
                stroke: template_stroke,
                stroke_width: template_stroke_width,
                ..
            },
            ElementKind::Path {
                fill_rule: candidate_rule,
                fill: candidate_fill,
                stroke: candidate_stroke,
                stroke_width: candidate_stroke_width,
                ..
            },
        ) => {
            template_rule == candidate_rule
                && template_fill == candidate_fill
                && template_stroke == candidate_stroke
                && template_stroke_width == candidate_stroke_width
        }
        (
            ElementKind::PaintedPath {
                fill_rule: template_rule,
                fill: template_fill,
                stroke: template_stroke,
                fill_gradient: template_fill_gradient,
                stroke_gradient: template_stroke_gradient,
                stroke_width: template_stroke_width,
                ..
            },
            ElementKind::PaintedPath {
                fill_rule: candidate_rule,
                fill: candidate_fill,
                stroke: candidate_stroke,
                fill_gradient: candidate_fill_gradient,
                stroke_gradient: candidate_stroke_gradient,
                stroke_width: candidate_stroke_width,
                ..
            },
        ) => {
            template_rule == candidate_rule
                && template_fill == candidate_fill
                && template_stroke == candidate_stroke
                && template_fill_gradient == candidate_fill_gradient
                && template_stroke_gradient == candidate_stroke_gradient
                && template_stroke_width == candidate_stroke_width
        }
        _ => false,
    }
}

fn batch_interacts_with_same_paint_fill(
    elements: &[Element],
    new_rings: &[Vec<[f64; 2]>],
    new_rule: FillRule,
    new_paint: &FillPaint,
) -> bool {
    let Some(new_bbox) = rings_bounds(new_rings) else {
        return false;
    };
    let is_bbox_candidate = |existing: &Element| {
        is_merge_fill(existing)
            && extract_fill_paint(&existing.kind).as_ref() == Some(new_paint)
            && element_fast_world_bounds(existing)
                .is_some_and(|bounds| bounds_overlap(bounds, new_bbox, SNAP_EPS * 2.0))
    };
    if !elements.iter().any(is_bbox_candidate) {
        return false;
    }
    let prepared = PreparedMergeRings::new(new_rings, new_rule);
    elements.iter().any(|existing| {
        if !is_bbox_candidate(existing) {
            return false;
        }
        let Some((ex_commands, ex_rule)) = element_world_commands(existing) else {
            return false;
        };
        let ex_rings = flatten_closed_rings(&ex_commands);
        let Some(ex_bbox) = rings_bounds(&ex_rings) else {
            return false;
        };
        bounds_overlap(ex_bbox, new_bbox, SNAP_EPS * 2.0)
            && rings_interact_with_prepared(
                &ex_rings,
                ex_rule,
                ex_bbox,
                &prepared.segments,
                &prepared.segment_bvh,
                &prepared.fill,
            )
    })
}

fn apply_remerged_element(
    elements: &mut Vec<Element>,
    result_ids: &mut Vec<Id>,
    element: Element,
) -> bool {
    let already_dissolved = is_already_normalized_merge_part(&element)
        && matches!(
            element_world_commands(&element),
            Some((_, FillRule::NonZero))
        );
    let ids = apply_merge_shape_to_keyframe_inner(elements, element, already_dissolved);
    if ids.is_empty() {
        return false;
    }
    result_ids.extend(ids);
    true
}

fn remerge_moved_elements_inner(
    elements: &mut Vec<Element>,
    moved_set: &std::collections::HashSet<&str>,
) -> Option<Vec<Id>> {
    let mut moved = Vec::new();
    let mut remaining = Vec::with_capacity(elements.len());
    for el in elements.drain(..) {
        if el.merge && moved_set.contains(el.id.as_str()) {
            moved.push(bake_batchable_moved_fill(el));
        } else {
            remaining.push(el);
        }
    }
    *elements = remaining;
    if moved.is_empty() {
        return Some(Vec::new());
    }
    // The first exact application would normalize these parts too. Doing it once up front makes
    // the same-paint interaction preflight match the subsequent compound pass without changing
    // the atomic outer transaction.
    ensure_existing_merge_normalized(elements);
    let batch_same_paint = moved.len() > 16;
    let mut result_ids = Vec::new();
    let mut idx = 0;
    while idx < moved.len() {
        if batch_same_paint
            && is_already_normalized_merge_part(&moved[idx])
            && is_merge_fill(&moved[idx])
            && let Some(fp) = extract_fill_paint(&moved[idx].kind)
            && let Some((mut combined_cmds, FillRule::NonZero)) =
                element_world_commands(&moved[idx])
            && combined_cmds.len() <= REMERGE_BATCH_MAX_COMMANDS
            && let Some(mut combined_points) = batch_flattened_point_count(&combined_cmds)
            && combined_points <= REMERGE_BATCH_MAX_POINTS
        {
            let mut end = idx + 1;
            while end < moved.len() {
                if !is_already_normalized_merge_part(&moved[end])
                    || !is_merge_fill(&moved[end])
                    || extract_fill_paint(&moved[end].kind).as_ref() != Some(&fp)
                    || !merge_fill_batch_compatible(&moved[idx], &moved[end])
                {
                    break;
                }
                let Some((cmds, FillRule::NonZero)) = element_world_commands(&moved[end]) else {
                    break;
                };
                let Some(points) = batch_flattened_point_count(&cmds) else {
                    break;
                };
                if combined_cmds.len().saturating_add(cmds.len()) > REMERGE_BATCH_MAX_COMMANDS
                    || combined_points.saturating_add(points) > REMERGE_BATCH_MAX_POINTS
                {
                    break;
                }
                combined_cmds.extend(cmds);
                combined_points += points;
                end += 1;
            }
            if end > idx + 1 {
                let batched = build_fill_element(
                    &moved[idx],
                    moved[idx].id.clone(),
                    combined_cmds,
                    FillRule::NonZero,
                    &fp,
                    true,
                );
                let (batch_commands, batch_rule) = element_world_commands(&batched)?;
                let batch_rings = flatten_closed_rings(&batch_commands);
                if batch_interacts_with_same_paint_fill(elements, &batch_rings, batch_rule, &fp) {
                    // A real same-paint union must choose identities through the exact existing
                    // order. Do not let the acceleration compound decide that policy.
                    for element in &mut moved[idx..end] {
                        if !apply_remerged_element(
                            elements,
                            &mut result_ids,
                            std::mem::take(element),
                        ) {
                            return None;
                        }
                    }
                    idx = end;
                    continue;
                }

                // Use the compound only as a single cutter against other paints and strokes. The
                // moved fills themselves remain separate authored elements with their original
                // IDs, names and order.
                let compound_id = batched.id.clone();
                let ids = apply_merge_shape_to_keyframe_inner(elements, batched, true);
                if ids.len() != 1 || ids[0] != compound_id {
                    return None;
                }
                let compound_position = elements
                    .iter()
                    .rposition(|element| element.id == compound_id)?;
                elements.remove(compound_position);
                for element in &mut moved[idx..end] {
                    let element = std::mem::take(element);
                    result_ids.push(element.id.clone());
                    elements.push(element);
                }
                idx = end;
                continue;
            }
        }
        let element = std::mem::take(&mut moved[idx]);
        if !apply_remerged_element(elements, &mut result_ids, element) {
            return None;
        }
        idx += 1;
    }
    Some(result_ids)
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
    let Some(fill_bbox) = segments_bounds(&fill_segs) else {
        return Vec::new();
    };
    let fill_bvh = SegmentBvh::from_segments(&fill_segs);

    let mut result = vec![fill_id.to_string()];
    for el in elements {
        if !is_merge_stroke(el) {
            continue;
        }
        let Some(el_bbox) = element_fast_world_bounds(el) else {
            continue;
        };
        if !bounds_overlap(el_bbox, fill_bbox, ADJACENT_TOL) {
            continue;
        }
        let Some((s_cmds, _)) = element_world_commands(el) else {
            continue;
        };
        let polys = flatten_stroke_polylines(&s_cmds);
        let touches = polys.iter().any(|poly| {
            poly.iter()
                .all(|&pt| fill_bvh.any_within_distance(&fill_segs, pt, ADJACENT_TOL))
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

fn eval_command_at(p0: [f64; 2], cmd: PathCommand, t: f64) -> [f64; 2] {
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

/// Multi-edge planar-face Paint Bucket (`T-104`): builds a planar half-edge arrangement from all
/// visible strokes and fill boundaries in `elements` (bridging endpoint gaps up to `gap_tolerance`)
/// and extracts the minimal enclosed polygon face containing `seed`.
pub fn paint_bucket_planar_face(
    elements: &[Element],
    seed: [f64; 2],
    gap_tolerance: f64,
) -> Option<Vec<PathCommand>> {
    let mut raw_segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for el in elements {
        if !el.visible {
            continue;
        }
        let Some((cmds, rule)) = element_world_commands(el) else {
            continue;
        };
        if is_merge_stroke(el) || extract_stroke_paint(&el.kind).is_some() {
            for poly in flatten_stroke_polylines(&cmds) {
                for pair in poly.windows(2) {
                    if dist_sq(pair[0], pair[1]) > 1e-8 {
                        raw_segs.push((pair[0], pair[1]));
                    }
                }
            }
        }
        if is_merge_fill(el) || extract_fill_paint(&el.kind).is_some() {
            let rings = flatten_closed_rings(&cmds);
            raw_segs.extend(fill_boundary_segments(&rings, rule));
        }
    }
    if raw_segs.len() < 3 {
        return None;
    }

    // Bridge small open endpoint gaps up to `gap_tolerance`.
    let tol = gap_tolerance.max(0.0);
    if tol > 1e-6 {
        let mut endpoints: Vec<[f64; 2]> = Vec::with_capacity(raw_segs.len() * 2);
        for &(a, b) in &raw_segs {
            endpoints.push(a);
            endpoints.push(b);
        }
        let mut bridges = Vec::new();
        for i in 0..endpoints.len() {
            let p = endpoints[i];
            let deg = endpoints.iter().filter(|&&q| dist_sq(p, q) <= 1e-4).count();
            if deg == 1 {
                let mut best: Option<([f64; 2], f64)> = None;
                for (j, &q) in endpoints.iter().enumerate() {
                    if i == j || (i / 2) == (j / 2) {
                        continue;
                    }
                    let d = (p[0] - q[0]).hypot(p[1] - q[1]);
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
    }

    // Subdivide segments at all pairwise intersections.
    let n = raw_segs.len();
    let mut split_params: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let (a0, a1) = raw_segs[i];
            let (b0, b1) = raw_segs[j];
            for (t, u) in segment_intersections(a0, a1, b0, b1) {
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                    split_params[i].push(t);
                    split_params[j].push(u);
                }
            }
        }
    }

    // Snap vertices within 0.05 px and build directed half-edges.
    const SNAP: f64 = 0.05;
    let mut vertices: Vec<[f64; 2]> = Vec::new();
    let mut find_or_insert = |pt: [f64; 2]| -> usize {
        if let Some(idx) = vertices
            .iter()
            .position(|&v| (v[0] - pt[0]).hypot(v[1] - pt[1]) <= SNAP)
        {
            idx
        } else {
            let idx = vertices.len();
            vertices.push(pt);
            idx
        }
    };

    let mut adj: Vec<Vec<usize>> = Vec::new();
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
            if point_in_rings(seed, &[ring.clone()], FillRule::EvenOdd) {
                if best_face.as_ref().is_none_or(|(best_a, _)| area < *best_a) {
                    best_face = Some((area, ring));
                }
            }
        }
    }

    let (_, ring) = best_face?;
    let mut cmds = Vec::with_capacity(ring.len() + 1);
    cmds.push(PathCommand::MoveTo {
        x: ring[0][0],
        y: ring[0][1],
    });
    for &[x, y] in &ring[1..ring.len().saturating_sub(1)] {
        cmds.push(PathCommand::LineTo { x, y });
    }
    cmds.push(PathCommand::Close);
    Some(cmds)
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
            let repro_moved_ids2 =
                remerge_moved_elements_in_keyframe(&mut moved_kf, &repro_moved_ids);
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
}
