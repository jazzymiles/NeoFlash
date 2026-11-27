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

use crate::authoring_geometry::{erase_path_stroke_with_shape, square_sweep_polygon};
use crate::evaluate::{affine_multiply, transform_matrix};
use crate::geometry::{
    FlattenOptions, GeometryError, MAX_PATH_COMMANDS, flatten_path, invert_affine_matrix,
    validate_path_commands,
};
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
const MAX_SPLIT_INTERSECTION_CHECKS: usize = 8_000_000;
const MAX_MERGE_ERASER_POINTS: usize = 4_096;
// Paint Bucket is invoked synchronously from a pointer gesture. Bound every potentially
// quadratic arrangement stage before it runs, and bound the number of generated planar edges.
// This keeps hostile or accidentally dense layers from monopolising the editor thread.
const MAX_BUCKET_QUADRATIC_CHECKS: usize = 8_000_000;
const MAX_BUCKET_RAW_SEGMENTS: usize = 4_096;
const MAX_BUCKET_PLANAR_EDGES: usize = 16_384;

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
        instance_behavior: None,
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
        instance_behavior: None,
    }
}

/// Split fragments are new pieces of geometry, not duplicate authored objects. Only the first
/// surviving outside piece keeps the source object's identity-bearing metadata; appearance and
/// compositing metadata remain shared by every piece.
fn clear_fragment_identity(element: &mut Element) {
    element.name.clear();
    element.scripts.clear();
    element.accessibility = None;
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

fn checked_flatten_closed_rings(
    commands: &[PathCommand],
) -> Result<Vec<Vec<[f64; 2]>>, GeometryError> {
    let contours = flatten_path(
        commands,
        FlattenOptions {
            tolerance: 0.35,
            ..FlattenOptions::default()
        },
    )?;
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
    Ok(rings)
}

/// Flatten closed contours from `commands` into vertex rings (without duplicate closing vertex).
/// This compatibility helper keeps its historical empty fallback; transactional authoring paths
/// use `checked_flatten_closed_rings` so a rejected boundary is never silently omitted.
pub fn flatten_closed_rings(commands: &[PathCommand]) -> Vec<Vec<[f64; 2]>> {
    checked_flatten_closed_rings(commands).unwrap_or_default()
}

fn checked_flatten_stroke_polylines(
    commands: &[PathCommand],
) -> Result<Vec<Vec<[f64; 2]>>, GeometryError> {
    let contours = flatten_path(
        commands,
        FlattenOptions {
            tolerance: 0.35,
            ..FlattenOptions::default()
        },
    )?;
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
    Ok(polylines)
}

/// Flatten stroke contours from `commands` into open or closed polylines (closed polylines repeat
/// their start vertex at the end). Transactional authoring paths use the fallible helper above.
pub fn flatten_stroke_polylines(commands: &[PathCommand]) -> Vec<Vec<[f64; 2]>> {
    checked_flatten_stroke_polylines(commands).unwrap_or_default()
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

mod boolean_ops;
mod faces;
mod keyframe_remerge;
mod lasso_erase;
mod planar_graph;
#[cfg(test)]
mod tests;

pub use boolean_ops::*;
use boolean_ops::{boolean_rings_with_interaction, trim_polyline_inside_prepared_fill};
pub use faces::*;
use faces::{
    assemble_shells_and_holes, polyline_to_commands, ring_to_commands, simplify_ring_collinear,
    trace_half_edge_cycles, trace_half_edge_cycles_with_chains,
};
pub use keyframe_remerge::*;
pub use lasso_erase::*;
use planar_graph::*;
