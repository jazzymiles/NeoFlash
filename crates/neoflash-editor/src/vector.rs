//! Vector geometry rendering and direct authoring. Draft gestures are not document state.
use crate::app::{Editor, Tool};
use crate::theme::UiPalette;
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
    math::{Point, point},
    path::Path,
};
use neoflash_core::*;
use neoflash_ui::components::canvas as cv;

pub(crate) fn tessellate(
    commands: &[PathCommand],
    rule: FillRule,
    width: Option<f64>,
    tolerance: f32,
) -> Result<VertexBuffers<Point, u32>, String> {
    let mut builder = Path::builder().with_svg();
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                builder.move_to(point(x as f32, y as f32));
            }
            PathCommand::LineTo { x, y } => {
                builder.line_to(point(x as f32, y as f32));
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                builder.quadratic_bezier_to(point(cx as f32, cy as f32), point(x as f32, y as f32));
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                builder.cubic_bezier_to(
                    point(cx1 as f32, cy1 as f32),
                    point(cx2 as f32, cy2 as f32),
                    point(x as f32, y as f32),
                );
            }
            PathCommand::Close => builder.close(),
        }
    }
    let path = builder.build();
    let mut mesh = VertexBuffers::<Point, u32>::new();
    if let Some(width) = width {
        StrokeTessellator::new()
            .tessellate_path(
                &path,
                &StrokeOptions::default()
                    .with_line_width(width as f32)
                    .with_line_cap(lyon_tessellation::LineCap::Round)
                    .with_line_join(lyon_tessellation::LineJoin::Round)
                    .with_tolerance(tolerance),
                &mut BuffersBuilder::new(&mut mesh, |v: StrokeVertex| v.position()),
            )
            .map_err(|e| format!("Stroke tessellation: {e:?}"))?;
    } else {
        FillTessellator::new()
            .tessellate_path(
                &path,
                &FillOptions::default()
                    .with_tolerance(tolerance)
                    .with_fill_rule(match rule {
                        FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
                        FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
                    }),
                &mut BuffersBuilder::new(&mut mesh, |v: FillVertex| v.position()),
            )
            .map_err(|e| format!("Fill tessellation: {e:?}"))?;
    }
    Ok(mesh)
}

pub(crate) struct PathPaint {
    pub rule: FillRule,
    pub fill: Color32,
    pub stroke: Color32,
    pub width: f64,
    pub tolerance: f32,
}

pub(crate) fn paint_path(
    p: &egui::Painter,
    commands: &[PathCommand],
    paint: PathPaint,
    screen: impl Fn(Pos2) -> Pos2,
) {
    let PathPaint {
        rule,
        fill,
        stroke,
        width,
        tolerance,
    } = paint;
    for (c, width) in [(fill, None), (stroke, Some(width))] {
        if c.a() == 0 || width.is_some_and(|w| w <= 0.) {
            continue;
        }
        match tessellate(commands, rule, width, tolerance) {
            Ok(geometry) => {
                let mut mesh = egui::epaint::Mesh::default();
                for vertex in geometry.vertices {
                    mesh.colored_vertex(screen(Pos2::new(vertex.x, vertex.y)), c);
                }
                mesh.indices = geometry.indices;
                p.add(Shape::mesh(mesh));
            }
            Err(message) => {
                p.text(
                    screen(Pos2::ZERO),
                    egui::Align2::LEFT_TOP,
                    message,
                    egui::FontId::monospace(crate::theme::PANEL_FONT),
                    UiPalette::current(p.ctx()).error_text,
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub point: Pos2,
    pub tangent: Vec2,
}
#[derive(Clone, Debug)]
pub(crate) struct PathDraft {
    pub anchors: Vec<Anchor>,
    pub pencil: bool,
    pub dragging: bool,
    pub closing: bool,
    pub limit_reached: bool,
    pub symbol: String,
    pub frame: u32,
    pub layer: usize,
    pub layer_id: String,
    /// View captured for the current press/drag phase. Pen anchors may span
    /// several phases and views; an individual pointer drag may not.
    pub pointer_origin: Pos2,
    pub pointer_zoom: f32,
}
fn smooth_pencil_tangents(anchors: &mut [Anchor]) {
    let points: Vec<_> = anchors.iter().map(|a| a.point).collect();
    for (index, anchor) in anchors.iter_mut().enumerate() {
        let before = points[index.saturating_sub(1)];
        let after = points[(index + 1).min(points.len() - 1)];
        // Catmull–Rom converted to symmetric cubic handles, bounded by adjacent segments.
        let tangent = (after - before) / 6.;
        let limit = if index == 0 {
            anchor.point.distance(after)
        } else if index + 1 == points.len() {
            anchor.point.distance(before)
        } else {
            anchor
                .point
                .distance(before)
                .min(anchor.point.distance(after))
        } / 3.;
        anchor.tangent = tangent.normalized() * tangent.length().min(limit);
    }
}
impl PathDraft {
    pub fn commands(&self, closed: bool) -> Vec<PathCommand> {
        let Some(first) = self.anchors.first() else {
            return vec![];
        };
        let mut commands = vec![PathCommand::MoveTo {
            x: first.point.x as f64,
            y: first.point.y as f64,
        }];
        let mut previous = first;
        for next in self
            .anchors
            .iter()
            .skip(1)
            .chain(if closed { Some(first) } else { None })
        {
            if previous.tangent.length_sq() > 0.01 || next.tangent.length_sq() > 0.01 {
                let c1 = previous.point + previous.tangent;
                let c2 = next.point - next.tangent;
                commands.push(PathCommand::CubicTo {
                    cx1: c1.x as f64,
                    cy1: c1.y as f64,
                    cx2: c2.x as f64,
                    cy2: c2.y as f64,
                    x: next.point.x as f64,
                    y: next.point.y as f64,
                });
            } else {
                commands.push(PathCommand::LineTo {
                    x: next.point.x as f64,
                    y: next.point.y as f64,
                });
            }
            previous = next;
        }
        if closed {
            commands.push(PathCommand::Close);
        }
        commands
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointRole {
    Anchor,
    Control1,
    Control2,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct PathHandle {
    pub command: usize,
    pub role: PointRole,
    pub point: Pos2,
}
#[derive(Clone, Debug)]
pub(crate) struct PointDrag {
    pub handle: PathHandle,
    pub original: Vec<PathCommand>,
    pub inverse: [f64; 6],
    pub symbol: String,
    pub frame: u32,
    pub layer: String,
    pub origin: Pos2,
    pub zoom: f32,
    pub press: Pos2,
    pub began: bool,
    pub changed: bool,
    pub element: String,
}
#[derive(Default)]
pub(crate) struct VectorState {
    pub draft: Option<PathDraft>,
    pub point_drag: Option<PointDrag>,
    /// Selected anchor command indices for an editable path `(element_id, command_indices)`.
    pub selected_anchors: Option<(Id, Vec<usize>)>,
}

pub(crate) fn handles(commands: &[PathCommand]) -> Vec<PathHandle> {
    let mut out = vec![];
    for (i, command) in commands.iter().enumerate() {
        let mut add = |role, x: f64, y: f64| {
            out.push(PathHandle {
                command: i,
                role,
                point: Pos2::new(x as f32, y as f32),
            })
        };
        match *command {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                add(PointRole::Anchor, x, y)
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                add(PointRole::Anchor, x, y);
                add(PointRole::Control1, cx, cy);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                add(PointRole::Anchor, x, y);
                add(PointRole::Control1, cx1, cy1);
                add(PointRole::Control2, cx2, cy2);
            }
            PathCommand::Close => {}
        }
    }
    out
}

/// Move an anchor with its incident controls, including the duplicate endpoint of
/// a curved closing segment. A shared quadratic control is translated only once.
pub(crate) fn move_handle(commands: &mut [PathCommand], handle: PathHandle, to: Pos2) {
    let delta = to - handle.point;
    let dx = delta.x as f64;
    let dy = delta.y as f64;
    let mut anchors = vec![handle.command];
    if handle.role == PointRole::Anchor {
        let start = (0..=handle.command)
            .rev()
            .find(|&i| matches!(commands.get(i), Some(PathCommand::MoveTo { .. })));
        let close = (handle.command..commands.len())
            .take_while(|&i| {
                i == handle.command || !matches!(commands[i], PathCommand::MoveTo { .. })
            })
            .find(|&i| matches!(commands[i], PathCommand::Close));
        if let (Some(start), Some(close)) = (start, close)
            && close > start + 1
        {
            let last = close - 1;
            if handle.command == start || handle.command == last {
                let other = if handle.command == start { last } else { start };
                if handles(commands).iter().any(|h| {
                    h.command == other
                        && h.role == PointRole::Anchor
                        && h.point.distance_sq(handle.point) < 1e-8
                }) {
                    anchors.push(other);
                }
            }
        }
    }
    let mut targets = vec![(handle.command, handle.role)];
    if handle.role == PointRole::Anchor {
        for index in anchors {
            targets.push((index, PointRole::Anchor));
            match commands.get(index) {
                Some(PathCommand::QuadraticTo { .. }) => targets.push((index, PointRole::Control1)),
                Some(PathCommand::CubicTo { .. }) => targets.push((index, PointRole::Control2)),
                _ => {}
            }
            if matches!(
                commands.get(index + 1),
                Some(PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. })
            ) {
                targets.push((index + 1, PointRole::Control1));
            }
        }
    }
    let mut applied = Vec::new();
    for target in targets {
        if applied.contains(&target) {
            continue;
        }
        applied.push(target);
        let (index, role) = target;
        match (commands.get_mut(index), role) {
            (
                Some(
                    PathCommand::MoveTo { x, y }
                    | PathCommand::LineTo { x, y }
                    | PathCommand::QuadraticTo { x, y, .. }
                    | PathCommand::CubicTo { x, y, .. },
                ),
                PointRole::Anchor,
            ) => {
                *x += dx;
                *y += dy;
            }
            (Some(PathCommand::QuadraticTo { cx, cy, .. }), PointRole::Control1) => {
                *cx += dx;
                *cy += dy;
            }
            (Some(PathCommand::CubicTo { cx1, cy1, .. }), PointRole::Control1) => {
                *cx1 += dx;
                *cy1 += dy;
            }
            (Some(PathCommand::CubicTo { cx2, cy2, .. }), PointRole::Control2) => {
                *cx2 += dx;
                *cy2 += dy;
            }
            _ => {}
        }
    }
}

/// Iterative Ramer–Douglas–Peucker keeps pencil geometry compact without recursion.
fn simplify(points: &[Anchor], tolerance: f32) -> Vec<Anchor> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((start, end)) = stack.pop() {
        let a = points[start].point;
        let line = points[end].point - a;
        let mut best = tolerance * tolerance;
        let mut index = None;
        for (i, anchor) in points.iter().enumerate().take(end).skip(start + 1) {
            let t = if line.length_sq() > 0. {
                ((anchor.point - a).dot(line) / line.length_sq()).clamp(0., 1.)
            } else {
                0.
            };
            let distance = anchor.point.distance_sq(a + line * t);
            if distance > best {
                best = distance;
                index = Some(i);
            }
        }
        if let Some(i) = index {
            keep[i] = true;
            stack.push((start, i));
            stack.push((i, end));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(*p))
        .collect()
}

fn polyline_length(anchors: &[Anchor]) -> f32 {
    anchors
        .windows(2)
        .map(|w| w[0].point.distance(w[1].point))
        .sum()
}

/// Preferences ▸ Drawing ▸ Recognise lines: collapse a nearly straight open pencil stroke into a
/// single segment when every sample stays within `mode.recognise_lines_px()` of the start–end chord.
pub(crate) fn recognise_straight_line(
    raw_anchors: &[Anchor],
    zoom: f32,
    mode: crate::workspace::Tolerance,
) -> Option<[Pos2; 2]> {
    let tol_px = mode.recognise_lines_px()?;
    if raw_anchors.len() < 2 {
        return None;
    }
    let zoom = zoom.max(1e-3);
    let tol = tol_px / zoom;
    let a = raw_anchors[0].point;
    let b = raw_anchors[raw_anchors.len() - 1].point;
    let line = b - a;
    let chord_len = line.length();
    if chord_len < 4.0 / zoom {
        return None;
    }
    if polyline_length(raw_anchors) > chord_len * 1.15 + tol {
        return None;
    }
    let line_sq = line.length_sq();
    let max_sq = tol * tol;
    for anchor in raw_anchors {
        let t = ((anchor.point - a).dot(line) / line_sq).clamp(0.0, 1.0);
        if anchor.point.distance_sq(a + line * t) > max_sq {
            return None;
        }
    }
    Some([a, b])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RecognisedShape {
    Rectangle {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    Ellipse {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

fn sample_uniform_loop(points: &[Pos2], count: usize) -> Vec<Pos2> {
    if points.len() < 2 || count == 0 {
        return points.to_vec();
    }
    let mut cumulative = Vec::with_capacity(points.len());
    cumulative.push(0.0_f32);
    let mut total = 0.0_f32;
    for w in points.windows(2) {
        total += w[0].distance(w[1]);
        cumulative.push(total);
    }
    if total <= 1e-5 {
        return vec![points[0]; count];
    }
    let mut out = Vec::with_capacity(count);
    let mut seg = 0_usize;
    for i in 0..count {
        let target = total * (i as f32) / (count as f32);
        while seg + 2 < cumulative.len() && cumulative[seg + 1] < target {
            seg += 1;
        }
        let seg_start = cumulative[seg];
        let seg_len = (cumulative[seg + 1] - seg_start).max(1e-6);
        let t = ((target - seg_start) / seg_len).clamp(0.0, 1.0);
        out.push(points[seg].lerp(points[seg + 1], t));
    }
    out
}

/// Preferences ▸ Drawing ▸ Recognise shapes: classify a closed (or nearly closed) freehand pencil
/// loop as an axis-aligned `Rectangle` or `Ellipse` when the fit is clean and unambiguous.
pub(crate) fn recognise_primitive_shape(
    raw_anchors: &[Anchor],
    closed: bool,
    zoom: f32,
    mode: crate::workspace::Tolerance,
) -> Option<RecognisedShape> {
    use crate::workspace::Tolerance;
    if mode == Tolerance::Off || raw_anchors.len() < 5 {
        return None;
    }
    let zoom = zoom.max(1e-3);
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for anchor in raw_anchors {
        min_x = min_x.min(anchor.point.x);
        max_x = max_x.max(anchor.point.x);
        min_y = min_y.min(anchor.point.y);
        max_y = max_y.max(anchor.point.y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    if !width.is_finite() || !height.is_finite() || width * zoom < 12.0 || height * zoom < 12.0 {
        return None;
    }
    let min_dim = width.min(height);
    let max_dim = width.max(height);
    if min_dim / max_dim < 0.1 {
        return None;
    }

    let first = raw_anchors[0].point;
    let last = raw_anchors[raw_anchors.len() - 1].point;
    let gap = if closed { 0.0 } else { first.distance(last) };
    let max_gap = match mode {
        Tolerance::Off => return None,
        Tolerance::MustBeClose => (8.0 / zoom).max(0.12 * min_dim),
        Tolerance::Normal => (18.0 / zoom).max(0.25 * min_dim),
        Tolerance::CanBeDistant => (32.0 / zoom).max(0.38 * min_dim),
    };
    if gap > max_gap {
        return None;
    }

    let arc_len = polyline_length(raw_anchors);
    let loop_len = arc_len + gap;
    if arc_len <= 0.0 {
        return None;
    }

    let mut loop_pts: Vec<Pos2> = raw_anchors.iter().map(|a| a.point).collect();
    if first.distance(last) > 1e-4 {
        loop_pts.push(first);
    }
    let samples = sample_uniform_loop(&loop_pts, 64);

    // Candidate 1: Axis-aligned Rectangle.
    let rect_perim = 2.0 * (width + height);
    let px_floor = match mode {
        Tolerance::Off => 0.0,
        Tolerance::MustBeClose => 2.0,
        Tolerance::Normal => 4.0,
        Tolerance::CanBeDistant => 7.0,
    } / (zoom * min_dim);
    let (rect_rms_limit, rect_max_limit) = match mode {
        Tolerance::Off => return None,
        Tolerance::MustBeClose => (
            0.035_f32.max(px_floor * 0.6).min(0.06),
            0.085_f32.max(px_floor).min(0.13),
        ),
        Tolerance::Normal => (
            0.065_f32.max(px_floor * 0.6).min(0.08),
            0.145_f32.max(px_floor).min(0.18),
        ),
        Tolerance::CanBeDistant => (
            0.095_f32.max(px_floor * 0.6).min(0.11),
            0.21_f32.max(px_floor).min(0.23),
        ),
    };
    let edge_band = rect_max_limit * min_dim;
    let mut rect_sq_sum = 0.0_f32;
    let mut rect_max = 0.0_f32;
    let mut edge_hits = [0_u32; 4];
    for p in &samples {
        let dl = (p.x - min_x).abs();
        let dr = (max_x - p.x).abs();
        let dt = (p.y - min_y).abs();
        let db = (max_y - p.y).abs();
        if dl <= edge_band {
            edge_hits[0] += 1;
        }
        if dr <= edge_band {
            edge_hits[1] += 1;
        }
        if dt <= edge_band {
            edge_hits[2] += 1;
        }
        if db <= edge_band {
            edge_hits[3] += 1;
        }
        let err = dl.min(dr).min(dt).min(db) / min_dim;
        rect_sq_sum += err * err;
        rect_max = rect_max.max(err);
    }
    let rect_rms = (rect_sq_sum / samples.len() as f32).sqrt();
    let rect_ok = arc_len >= 0.72 * rect_perim
        && loop_len <= 1.35 * rect_perim
        && edge_hits.iter().all(|&c| c >= 4)
        && rect_rms <= rect_rms_limit
        && rect_max <= rect_max_limit;

    // Candidate 2: Axis-aligned Ellipse / Circle.
    let rx = width * 0.5;
    let ry = height * 0.5;
    let cx = min_x + rx;
    let cy = min_y + ry;
    let ell_perim = std::f32::consts::PI
        * (3.0 * (rx + ry) - ((3.0 * rx + ry) * (rx + 3.0 * ry)).max(0.0).sqrt());
    let (ell_rms_limit, ell_max_limit) = match mode {
        Tolerance::Off => return None,
        Tolerance::MustBeClose => (0.025_f32, 0.055_f32),
        Tolerance::Normal => (0.048_f32, 0.105_f32),
        Tolerance::CanBeDistant => (0.072_f32, 0.138_f32),
    };
    let mut ell_sq_sum = 0.0_f32;
    let mut ell_max = 0.0_f32;
    let mut sectors = 0_u8;
    for p in &samples {
        let u = (p.x - cx) / rx;
        let v = (p.y - cy) / ry;
        let angle = v.atan2(u).rem_euclid(std::f32::consts::TAU);
        let bucket = ((angle / std::f32::consts::TAU) * 8.0).floor() as u32;
        sectors |= 1_u8 << bucket.min(7);
        // Scale radial error by 0.5 so both candidates measure error in units of `min_dim`.
        let err = 0.5 * (u.hypot(v) - 1.0).abs();
        ell_sq_sum += err * err;
        ell_max = ell_max.max(err);
    }
    let ell_rms = (ell_sq_sum / samples.len() as f32).sqrt();
    let ell_ok = arc_len >= 0.72 * ell_perim
        && loop_len <= 1.35 * ell_perim
        && sectors.count_ones() >= 7
        && ell_rms <= ell_rms_limit
        && ell_max <= ell_max_limit;

    if rect_ok && rect_rms < 0.75 * ell_rms {
        Some(RecognisedShape::Rectangle {
            x: f64::from(min_x),
            y: f64::from(min_y),
            width: f64::from(width),
            height: f64::from(height),
        })
    } else if ell_ok && ell_rms < 0.75 * rect_rms {
        Some(RecognisedShape::Ellipse {
            x: f64::from(min_x),
            y: f64::from(min_y),
            width: f64::from(width),
            height: f64::from(height),
        })
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointSide {
    Start,
    End,
}

#[derive(Clone, Debug)]
struct OpenEndpointMatch {
    element_id: Id,
    side: EndpointSide,
    world_point: Pos2,
    matrix: [f64; 6],
    same_style: bool,
}

fn open_contour_endpoints(commands: &[PathCommand]) -> Option<((f64, f64), (f64, f64))> {
    if commands.len() < 2 {
        return None;
    }
    let PathCommand::MoveTo { x: x0, y: y0 } = commands[0] else {
        return None;
    };
    if commands[1..]
        .iter()
        .any(|c| matches!(c, PathCommand::MoveTo { .. } | PathCommand::Close))
    {
        return None;
    }
    let (x1, y1) = match *commands.last()? {
        PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. } => (x, y),
        PathCommand::MoveTo { .. } | PathCommand::Close => return None,
    };
    Some(((x0, y0), (x1, y1)))
}

fn inverse_f64(m: [f64; 6], px: f64, py: f64) -> Option<(f64, f64)> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d.abs() < 1e-10 {
        return None;
    }
    let x = px - m[4];
    let y = py - m[5];
    Some(((m[3] * x - m[2] * y) / d, (-m[1] * x + m[0] * y) / d))
}

fn map_commands_inverse(commands: &[PathCommand], m: [f64; 6]) -> Option<Vec<PathCommand>> {
    let mut out = Vec::with_capacity(commands.len());
    for cmd in commands {
        let mapped = match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (x, y) = inverse_f64(m, x, y)?;
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let (x, y) = inverse_f64(m, x, y)?;
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (cx, cy) = inverse_f64(m, cx, cy)?;
                let (x, y) = inverse_f64(m, x, y)?;
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
                let (cx1, cy1) = inverse_f64(m, cx1, cy1)?;
                let (cx2, cy2) = inverse_f64(m, cx2, cy2)?;
                let (x, y) = inverse_f64(m, x, y)?;
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
        };
        out.push(mapped);
    }
    Some(out)
}

fn set_contour_start(commands: &mut [PathCommand], pt: (f64, f64)) {
    if let Some(PathCommand::MoveTo { x, y }) = commands.first_mut() {
        *x = pt.0;
        *y = pt.1;
    }
}

fn set_contour_end(commands: &mut [PathCommand], pt: (f64, f64)) {
    if let Some(
        PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. },
    ) = commands.last_mut()
    {
        *x = pt.0;
        *y = pt.1;
    }
}

fn reverse_open_contour(commands: &[PathCommand]) -> Option<Vec<PathCommand>> {
    let ((x0, y0), (xn, yn)) = open_contour_endpoints(commands)?;
    let mut prev = (x0, y0);
    let mut segments = Vec::with_capacity(commands.len() - 1);
    for cmd in &commands[1..] {
        match *cmd {
            PathCommand::LineTo { x, y } => {
                segments.push((prev, PathCommand::LineTo { x, y }));
                prev = (x, y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                segments.push((prev, PathCommand::QuadraticTo { cx, cy, x, y }));
                prev = (x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                segments.push((
                    prev,
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    },
                ));
                prev = (x, y);
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => return None,
        }
    }
    let mut out = Vec::with_capacity(commands.len());
    out.push(PathCommand::MoveTo { x: xn, y: yn });
    for ((px, py), cmd) in segments.into_iter().rev() {
        out.push(match cmd {
            PathCommand::LineTo { .. } => PathCommand::LineTo { x: px, y: py },
            PathCommand::QuadraticTo { cx, cy, .. } => PathCommand::QuadraticTo {
                cx,
                cy,
                x: px,
                y: py,
            },
            PathCommand::CubicTo {
                cx1, cy1, cx2, cy2, ..
            } => PathCommand::CubicTo {
                cx1: cx2,
                cy1: cy2,
                cx2: cx1,
                cy2: cy1,
                x: px,
                y: py,
            },
            _ => return None,
        });
    }
    Some(out)
}

impl Editor {
    /// End only the active pointer phase before changing the viewport. Pen
    /// keeps its existing anchors/controls for the next press; Pencil commits
    /// samples already collected in the old view. A later move/release cannot
    /// append geometry until a new primary press starts another phase.
    pub(crate) fn end_vector_pointer_phase(&mut self) {
        let finish_pencil = self.vector.draft.as_mut().is_some_and(|draft| {
            let active = draft.dragging || draft.closing;
            draft.dragging = false;
            draft.closing = false;
            draft.pencil && active
        });
        if finish_pencil {
            self.finish_path(false);
        }
    }

    pub(crate) fn finish_point_drag(&mut self) {
        if let Some(drag) = self.vector.point_drag.take()
            && drag.began
        {
            if drag.changed {
                self.commit_history();
            } else {
                self.history.cancel(&mut self.project);
            }
        }
    }
    pub(crate) fn finish_edits(&mut self) {
        self.finish_shape_hint_drag(false);
        self.finish_motion_curve_drag();
        self.finish_motion_key_drag();
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.finish_selection_gesture();
        self.shape_draft = None;
        self.finish_point_drag();
        if self.vector.draft.is_some() {
            self.finish_path(false);
        }
    }
    pub(crate) fn cancel_vector(&mut self) {
        if self
            .vector
            .point_drag
            .as_ref()
            .is_some_and(|drag| drag.began)
        {
            self.history.cancel(&mut self.project);
        }
        self.vector = Default::default();
    }
    pub(crate) fn cancel_edits(&mut self) {
        self.cancel_shape_hint_drag();
        self.cancel_motion_drags();
        self.authoring_tools.cancel();
        self.finish_property_edit();
        self.cancel_selection_gesture();
        self.cancel_vector();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.cancel_guide_drag();
    }
    pub(crate) fn set_playing(&mut self, playing: bool) {
        self.finish_edits();
        if playing && !self.playing {
            self.rewind_if_finished();
        }
        self.timeline_key_drag = None;
        self.playing = playing;
        self.accumulator = 0.;
        self.sync_authored_preview();
        self.last_tick = std::time::Instant::now();
    }
    pub(crate) fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            self.finish_edits();
            self.timeline_key_drag = None;
            self.vector = Default::default();
        }
        self.tool = tool;
    }
    fn find_nearest_open_endpoint(&self, pt: Pos2, tol: f32) -> Option<OpenEndpointMatch> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let sampled = sample_layer(layer, self.frame);
        let no_gradients = self.authoring_modify.drawing_fill_gradient.is_none()
            && self.authoring_modify.drawing_stroke_gradient.is_none();
        let mut best: Option<(f32, OpenEndpointMatch)> = None;
        for el in sampled.iter().rev() {
            let ElementKind::Path {
                commands,
                fill,
                stroke,
                stroke_width,
                ..
            } = &el.kind
            else {
                continue;
            };
            let Some((local_start, local_end)) = open_contour_endpoints(commands) else {
                continue;
            };
            let matrix = transform_matrix(el.transform);
            let same_style = no_gradients
                && fill[3] == 0
                && *stroke == self.stroke
                && (*stroke_width - self.stroke_width).abs() < 1e-6;
            for (side, local_pt) in [
                (EndpointSide::Start, local_start),
                (EndpointSide::End, local_end),
            ] {
                let world_point =
                    crate::stage::affine(matrix, Pos2::new(local_pt.0 as f32, local_pt.1 as f32));
                let dist = world_point.distance(pt);
                if dist <= tol && best.as_ref().is_none_or(|(b, _)| dist < *b) {
                    best = Some((
                        dist,
                        OpenEndpointMatch {
                            element_id: el.id.clone(),
                            side,
                            world_point,
                            matrix,
                            same_style,
                        },
                    ));
                }
            }
        }
        best.map(|(_, m)| m)
    }

    fn build_joined_open_path(
        &self,
        new_world_commands: &[PathCommand],
        start_match: Option<&OpenEndpointMatch>,
        end_match: Option<&OpenEndpointMatch>,
    ) -> Option<(Id, Vec<PathCommand>, bool)> {
        let (primary, is_start_primary) = match (start_match, end_match) {
            (Some(s), _) if s.same_style => (s, true),
            (_, Some(e)) if e.same_style => (e, false),
            _ => return None,
        };
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        let sampled = sample_layer(layer, self.frame);
        let existing = sampled.iter().find(|e| e.id == primary.element_id)?;
        let ElementKind::Path {
            commands: existing_cmds,
            ..
        } = &existing.kind
        else {
            return None;
        };
        let (existing_start, existing_end) = open_contour_endpoints(existing_cmds)?;
        let local_new = map_commands_inverse(new_world_commands, primary.matrix)?;
        if local_new.len() < 2 {
            return None;
        }
        let other_closes_same = if is_start_primary {
            end_match.is_some_and(|e| e.element_id == primary.element_id && e.side != primary.side)
        } else {
            start_match
                .is_some_and(|s| s.element_id == primary.element_id && s.side != primary.side)
        };
        let has_curve = existing_cmds.iter().chain(local_new.iter()).any(|c| {
            matches!(
                c,
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
            )
        });
        let can_close =
            other_closes_same && (has_curve || existing_cmds.len() + local_new.len() >= 5);

        let mut merged = Vec::with_capacity(existing_cmds.len() + local_new.len());
        let mut closed_loop = false;
        match (is_start_primary, primary.side) {
            (true, EndpointSide::End) => {
                // Existing start..end + new start..end
                let mut tail = local_new;
                set_contour_start(&mut tail, existing_end);
                if can_close {
                    set_contour_end(&mut tail, existing_start);
                }
                merged.extend_from_slice(existing_cmds);
                merged.extend_from_slice(&tail[1..]);
                if can_close {
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                }
            }
            (false, EndpointSide::End) => {
                // Existing start..end + reversed(new)
                let mut rev = reverse_open_contour(&local_new)?;
                set_contour_start(&mut rev, existing_end);
                if can_close {
                    set_contour_end(&mut rev, existing_start);
                }
                merged.extend_from_slice(existing_cmds);
                merged.extend_from_slice(&rev[1..]);
                if can_close {
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                }
            }
            (false, EndpointSide::Start) => {
                // New start..end + existing start..end
                let mut head = local_new;
                set_contour_end(&mut head, existing_start);
                if can_close {
                    let head_start = open_contour_endpoints(&head)?.0;
                    let mut rest = existing_cmds.clone();
                    set_contour_end(&mut rest, head_start);
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&rest[1..]);
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                } else {
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&existing_cmds[1..]);
                }
            }
            (true, EndpointSide::Start) => {
                // Reversed(new) + existing start..end
                let mut head = reverse_open_contour(&local_new)?;
                set_contour_end(&mut head, existing_start);
                if can_close {
                    let head_start = open_contour_endpoints(&head)?.0;
                    let mut rest = existing_cmds.clone();
                    set_contour_end(&mut rest, head_start);
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&rest[1..]);
                    merged.push(PathCommand::Close);
                    closed_loop = true;
                } else {
                    merged.extend_from_slice(&head);
                    merged.extend_from_slice(&existing_cmds[1..]);
                }
            }
        }
        validate_path_commands(&merged).ok()?;
        Some((primary.element_id.clone(), merged, closed_loop))
    }

    pub(crate) fn finish_path(&mut self, closed: bool) {
        let Some(mut draft) = self.vector.draft.take() else {
            return;
        };
        if draft.limit_reached {
            self.message("Pencil stroke exceeds 16000 samples. Draw shorter strokes; this draft was discarded.");
            return;
        }
        if draft.anchors.len() < 2 {
            return;
        }
        let target = self.project.symbol(&draft.symbol).and_then(|symbol| {
            (draft.frame < symbol.timeline.frame_count)
                .then(|| {
                    symbol.timeline.layers.iter().position(|layer| {
                        layer.id == draft.layer_id && layer.visible && !layer.locked
                    })
                })
                .flatten()
        });
        let Some(target_layer) = target else {
            self.message("Cannot finish path: its original layer/frame is no longer editable. Draft cancelled.");
            return;
        };
        let zoom = draft.pointer_zoom.max(1e-3);
        let mut drawing = self.workspace.drawing.clone();
        if draft.pencil {
            use crate::{tool_preferences::PencilMode, workspace::Tolerance};
            if self.workspace.tools.pencil_smoothing == 0. {
                drawing.smooth_curves = Tolerance::Off;
            }
            match self.workspace.tools.pencil_mode {
                PencilMode::Straighten => {}
                PencilMode::Smooth => {
                    drawing.recognise_lines = Tolerance::Off;
                    drawing.recognise_shapes = Tolerance::Off;
                }
                PencilMode::Ink => {
                    drawing.recognise_lines = Tolerance::Off;
                    drawing.recognise_shapes = Tolerance::Off;
                    drawing.smooth_curves = Tolerance::Off;
                    drawing.connect_lines = Tolerance::Off;
                }
            }
        }
        let raw_anchors = draft.anchors.clone();

        // 1. Preferences ▸ Drawing ▸ Recognise shapes (Pencil only).
        let recognised_shape = if draft.pencil {
            recognise_primitive_shape(&raw_anchors, closed, zoom, drawing.recognise_shapes)
        } else {
            None
        };

        // 2. Preferences ▸ Drawing ▸ Connect lines (self-close check).
        let connect_tol = drawing.connect_lines.connect_lines_px().map(|px| px / zoom);
        let mut effective_closed = closed;
        let mut self_closed = false;
        if !effective_closed
            && recognised_shape.is_none()
            && let Some(tol) = connect_tol
        {
            let first = raw_anchors[0].point;
            let last = raw_anchors[raw_anchors.len() - 1].point;
            if raw_anchors.len() >= 3
                && polyline_length(&raw_anchors) >= tol * 3.0
                && first.distance(last) <= tol
            {
                effective_closed = true;
                self_closed = true;
            }
        }

        // 3. Preferences ▸ Drawing ▸ Recognise lines & Smooth curves (Pencil only).
        if draft.pencil && recognised_shape.is_none() {
            if !effective_closed
                && let Some([a, b]) =
                    recognise_straight_line(&raw_anchors, zoom, drawing.recognise_lines)
            {
                draft.anchors = vec![
                    Anchor {
                        point: a,
                        tangent: Vec2::ZERO,
                    },
                    Anchor {
                        point: b,
                        tangent: Vec2::ZERO,
                    },
                ];
            } else if let Some(rdp_px) = drawing.smooth_curves.smooth_rdp_px() {
                draft.anchors = simplify(
                    &draft.anchors,
                    rdp_px / zoom * (self.workspace.tools.pencil_smoothing as f32 / 50.),
                );
            }
        }

        if draft.pencil
            && self.workspace.tools.pencil_mode == crate::tool_preferences::PencilMode::Smooth
            && drawing.smooth_curves != crate::workspace::Tolerance::Off
            && recognised_shape.is_none()
        {
            smooth_pencil_tangents(&mut draft.anchors);
        }

        // If self-closing left a trailing point right on top of the start anchor, drop it so
        // `PathDraft::commands(true)` closes cleanly without a tiny hook segment, or cancel
        // self-close if simplification reduced the stroke to a 2-point out-and-back line.
        if self_closed && let Some(tol) = connect_tol {
            if draft.anchors.len() > 3
                && draft.anchors.last().is_some_and(|last| {
                    last.tangent.length_sq() <= 0.01
                        && last.point.distance(draft.anchors[0].point) <= tol
                })
            {
                draft.anchors.pop();
            } else if draft.anchors.len() == 3
                && draft.anchors[2].tangent.length_sq() <= 0.01
                && draft.anchors[2].point.distance(draft.anchors[0].point) <= tol
            {
                effective_closed = false;
            }
        }
        if draft.anchors.len() < 2 && recognised_shape.is_none() {
            return;
        }

        let restore = (draft.symbol != self.symbol_id()
            || draft.frame != self.frame
            || target_layer != self.layer)
            .then(|| {
                (
                    self.editing.clone(),
                    self.frame,
                    self.layer,
                    self.selection.clone(),
                )
            });
        if restore.is_some() {
            self.editing = vec![draft.symbol.clone()];
            self.frame = draft.frame;
            self.layer = target_layer;
        }
        if let Some(shape) = recognised_shape {
            let (kind, x, y, msg) = match shape {
                RecognisedShape::Rectangle {
                    x,
                    y,
                    width,
                    height,
                } => (
                    ElementKind::Rectangle {
                        width,
                        height,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    x,
                    y,
                    "Rectangle recognised.",
                ),
                RecognisedShape::Ellipse {
                    x,
                    y,
                    width,
                    height,
                } => (
                    ElementKind::Ellipse {
                        width,
                        height,
                        fill: self.fill,
                        stroke: self.stroke,
                        stroke_width: self.stroke_width,
                    },
                    x,
                    y,
                    "Oval recognised.",
                ),
            };
            self.add_element(kind, x, y);
            if let Some((editing, frame, layer, selected)) = restore {
                self.editing = editing;
                self.frame = frame;
                self.layer = layer;
                self.selection = selected;
            }
            self.message(msg);
            return;
        }

        // 4. Preferences ▸ Drawing ▸ Connect lines (snap / join open path endpoints on same layer).
        let mut joined = None;
        if !effective_closed && let Some(tol) = connect_tol {
            let start_pt = draft.anchors[0].point;
            let end_pt = draft.anchors[draft.anchors.len() - 1].point;
            let start_match = self.find_nearest_open_endpoint(start_pt, tol);
            let end_match = self.find_nearest_open_endpoint(end_pt, tol).filter(|e| {
                start_match
                    .as_ref()
                    .is_none_or(|s| s.element_id != e.element_id || s.side != e.side)
            });
            if let Some(ref s) = start_match
                && (draft.anchors.len() > 2 || s.world_point.distance(end_pt) > 1e-3)
            {
                draft.anchors[0].point = s.world_point;
            }
            if let Some(ref e) = end_match {
                let current_start = draft.anchors[0].point;
                if (draft.anchors.len() > 2 || e.world_point.distance(current_start) > 1e-3)
                    && let Some(last) = draft.anchors.last_mut()
                {
                    last.point = e.world_point;
                }
            }
            if drawing.object_drawing {
                let world_cmds = draft.commands(false);
                joined = self.build_joined_open_path(
                    &world_cmds,
                    start_match.as_ref(),
                    end_match.as_ref(),
                );
            }
        }

        let final_closed = if let Some((target_id, merged_commands, closed_loop)) = joined {
            let join_fill = self.fill;
            self.transact_named("Path", |s| {
                s.ensure_key(false);
                let layer = s.layer;
                let frame = s.frame;
                if let Some(el) = s.symbol_mut().timeline.layers[layer]
                    .key_at_mut(frame)
                    .and_then(|k| k.elements.iter_mut().find(|e| e.id == target_id))
                    && let ElementKind::Path { commands, fill, .. } = &mut el.kind
                {
                    *commands = merged_commands.clone();
                    if closed_loop {
                        *fill = join_fill;
                    }
                }
                s.select_single_id(target_id.clone());
            });
            closed_loop
        } else {
            let commands = draft.commands(effective_closed);
            self.add_element(
                ElementKind::Path {
                    commands,
                    fill_rule: FillRule::NonZero,
                    fill: if effective_closed {
                        self.fill
                    } else {
                        [0, 0, 0, 0]
                    },
                    stroke: self.stroke,
                    stroke_width: self.stroke_width,
                },
                0.,
                0.,
            );
            effective_closed
        };
        if let Some((editing, frame, layer, selected)) = restore {
            self.editing = editing;
            self.frame = frame;
            self.layer = layer;
            self.selection = selected;
        }
        self.message(if final_closed {
            "Closed vector path created."
        } else {
            "Open vector path created. Use A to edit its points."
        });
    }

    pub(crate) fn vector_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        page: Rect,
        response: &egui::Response,
    ) {
        let zoom = self.zoom;
        let local = |pos: Pos2| Pos2::ZERO + (pos - origin) / zoom;
        let screen = |pos: Pos2| origin + pos.to_vec2() * zoom;
        if self.vector.point_drag.as_ref().is_some_and(|d| {
            !self.point_drag_matches(d) || d.origin != origin || d.zoom != self.zoom
        }) {
            self.cancel_vector();
        }
        if self.vector.draft.as_ref().is_some_and(|draft| {
            (draft.dragging || draft.closing)
                && (draft.pointer_origin != origin || draft.pointer_zoom != zoom)
        }) {
            self.end_vector_pointer_phase();
        }
        if self.vector.draft.as_ref().is_some_and(|d| {
            d.symbol != self.symbol_id()
                || d.frame != self.frame
                || d.layer != self.layer
                || self.symbol().timeline.layers[self.layer].id != d.layer_id
                || (d.pencil && self.tool != Tool::Pencil)
                || (!d.pencil && self.tool != Tool::Pen)
        }) {
            self.finish_path(false);
        }
        let events = ui.input(|i| i.events.clone());
        let canvas_hover = ui.input(|i| i.pointer.hover_pos()).filter(|&pos| {
            response.rect.contains(pos)
                && ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer == response.layer_id)
        });
        if self.tool == Tool::Pen
            && self.vector.draft.is_none()
            && self.layer_editable()
            && let Some(hover_pos) = canvas_hover
            && page.contains(hover_pos)
            && self.pen_subdivide_candidate(origin, hover_pos).is_some()
        {
            self.stage_cursor = Some(neoflash_ui::cursors::PEN_ADD);
        }
        if matches!(self.tool, Tool::Pen | Tool::Pencil) && self.layer_editable() {
            for event in &events {
                match *event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } if page.contains(pos)
                        && response.rect.contains(pos)
                        && ui
                            .ctx()
                            .layer_id_at(pos)
                            .is_none_or(|layer| layer == response.layer_id) =>
                    {
                        self.playing = false;
                        if self.tool == Tool::Pen
                            && self.vector.draft.is_none()
                            && self.try_pen_subdivide(origin, pos)
                        {
                            continue;
                        }
                        // Pen anchors snap to guides and grid (ST-04); pencil strokes are freehand.
                        let position = if self.tool == Tool::Pen {
                            local(self.snap_screen(origin, pos))
                        } else {
                            local(pos)
                        };
                        let symbol = self.symbol_id().to_owned();
                        let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
                        let draft = self.vector.draft.get_or_insert_with(|| PathDraft {
                            anchors: vec![],
                            pencil: self.tool == Tool::Pencil,
                            dragging: false,
                            closing: false,
                            limit_reached: false,
                            symbol,
                            frame: self.frame,
                            layer: self.layer,
                            layer_id,
                            pointer_origin: origin,
                            pointer_zoom: zoom,
                        });
                        draft.pointer_origin = origin;
                        draft.pointer_zoom = zoom;
                        if !draft.pencil
                            && draft.anchors.len() >= 2
                            && screen(draft.anchors[0].point).distance(pos) < 8.
                        {
                            draft.closing = true;
                        } else {
                            draft.anchors.push(Anchor {
                                point: position,
                                tangent: Vec2::ZERO,
                            });
                            draft.dragging = true;
                        }
                    }
                    egui::Event::PointerMoved(pos) => {
                        if let Some(draft) = &mut self.vector.draft
                            && draft.dragging
                        {
                            if draft.pencil {
                                if draft.anchors.len() >= 16_000 {
                                    draft.limit_reached = true;
                                }
                                if draft.anchors.len() < 16_000
                                    && draft
                                        .anchors
                                        .last()
                                        .is_some_and(|a| a.point.distance(local(pos)) * zoom >= 1.5)
                                {
                                    draft.anchors.push(Anchor {
                                        point: local(pos),
                                        tangent: Vec2::ZERO,
                                    });
                                }
                            } else if let Some(anchor) = draft.anchors.last_mut() {
                                anchor.tangent = local(pos) - anchor.point;
                            }
                        }
                    }
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        ..
                    } => {
                        let mut finish = None;
                        if let Some(draft) = &mut self.vector.draft {
                            if draft.closing {
                                finish = Some(true);
                            } else if draft.dragging && draft.pencil {
                                if draft
                                    .anchors
                                    .last()
                                    .is_some_and(|a| a.point.distance(local(pos)) > 0.1)
                                {
                                    draft.anchors.push(Anchor {
                                        point: local(pos),
                                        tangent: Vec2::ZERO,
                                    });
                                }
                                finish = Some(false);
                            } else if draft.dragging
                                && let Some(anchor) = draft.anchors.last_mut()
                            {
                                // Release position is authoritative even if no
                                // separate PointerMoved event was delivered.
                                anchor.tangent = local(pos) - anchor.point;
                            }
                            draft.dragging = false;
                        }
                        if let Some(closed) = finish {
                            self.finish_path(closed);
                        }
                    }
                    _ => {}
                }
            }
        }
        if self.tool == Tool::Subselect {
            self.edit_path_points(ui, p, origin, response);
        }
        if let Some(draft) = &self.vector.draft {
            let blue = UiPalette::current(p.ctx()).overlay;
            paint_path(
                p,
                &draft.commands(false),
                PathPaint {
                    rule: FillRule::NonZero,
                    fill: Color32::TRANSPARENT,
                    stroke: blue,
                    width: 1.5 / zoom as f64,
                    tolerance: 0.2,
                },
                screen,
            );
            // `NF2/Anchor Point` + `NF2/Bezier Handle`; the anchor being placed is the selected one,
            // the first anchor shows Hover while a click there would close the path.
            let hover = response.hover_pos();
            let closes = |at: Pos2| {
                !draft.pencil
                    && draft.anchors.len() >= 2
                    && hover.is_some_and(|pos| at.distance(pos) < 8.)
            };
            if !draft.pencil {
                let last = draft.anchors.len().saturating_sub(1);
                for (i, anchor) in draft.anchors.iter().enumerate() {
                    let at = screen(anchor.point);
                    if anchor.tangent.length_sq() > 0.1 {
                        for handle in [anchor.point + anchor.tangent, anchor.point - anchor.tangent]
                        {
                            cv::paint_bezier_handle(p, p.ctx(), at, screen(handle));
                        }
                    }
                    let state = if i == last {
                        cv::AnchorState::Selected
                    } else if i == 0 && closes(at) {
                        cv::AnchorState::Hover
                    } else {
                        cv::AnchorState::Unselected
                    };
                    cv::paint_anchor(p, p.ctx(), at, state);
                }
                if let Some(first) = draft.anchors.first()
                    && closes(screen(first.point))
                {
                    self.stage_cursor = Some(neoflash_ui::cursors::PEN_CLOSE);
                }
            }
            if !draft.pencil
                && self.workspace.drawing.pen_preview
                && let (Some(last), Some(hover)) = (draft.anchors.last(), response.hover_pos())
                && !draft.dragging
            {
                p.line_segment(
                    [screen(last.point), hover],
                    Stroke::new(1.0_f32, blue.gamma_multiply(0.5)),
                );
            }
        }
    }

    fn edit_path_points(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        response: &egui::Response,
    ) {
        let Some(element) = self.selected_sampled() else {
            return;
        };
        let (ElementKind::Path { ref commands, .. }
        | ElementKind::PaintedPath { ref commands, .. }) = element.kind
        else {
            return;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen = |pos: Pos2| origin + crate::stage::affine(matrix, pos).to_vec2() * zoom;
        let handles = handles(commands);
        // `NF2/Bezier Handle` from each anchor to its control points, then `NF2/Anchor Point`s.
        let mut previous = Pos2::ZERO;
        for command in commands {
            match *command {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    previous = Pos2::new(x as f32, y as f32)
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let control = screen(Pos2::new(cx as f32, cy as f32));
                    let end = Pos2::new(x as f32, y as f32);
                    cv::paint_bezier_handle(p, p.ctx(), screen(previous), control);
                    cv::paint_bezier_handle(p, p.ctx(), screen(end), control);
                    previous = end;
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let end = Pos2::new(x as f32, y as f32);
                    let first = screen(Pos2::new(cx1 as f32, cy1 as f32));
                    let second = screen(Pos2::new(cx2 as f32, cy2 as f32));
                    cv::paint_bezier_handle(p, p.ctx(), screen(previous), first);
                    cv::paint_bezier_handle(p, p.ctx(), screen(end), second);
                    previous = end;
                }
                PathCommand::Close => {}
            }
        }
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|&pos| {
            self.vector.point_drag.is_none()
                && response.rect.contains(pos)
                && ui
                    .ctx()
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer == response.layer_id)
        });
        let dragged = self
            .vector
            .point_drag
            .as_ref()
            .map(|drag| (drag.handle.command, drag.handle.role));
        let near = |at: Pos2| hover.is_some_and(|pos| at.distance(pos) < 8.);
        for handle in &handles {
            if handle.role == PointRole::Anchor {
                let at = screen(handle.point);
                let is_selected = dragged == Some((handle.command, handle.role))
                    || self.is_anchor_selected(&element.id, handle.command);
                let state = if is_selected {
                    cv::AnchorState::Selected
                } else if near(at) {
                    cv::AnchorState::Hover
                } else {
                    cv::AnchorState::Unselected
                };
                cv::paint_anchor(p, p.ctx(), at, state);
            }
        }
        let near_any_handle = handles.iter().any(|h| near(screen(h.point)));
        let alt_hover = ui.input(|i| i.modifiers.alt);
        if dragged.is_some() || near_any_handle {
            self.stage_cursor = Some(neoflash_ui::cursors::SUBSELECT_POINT);
        } else if alt_hover
            && let Some(hover_pos) = hover
            && path_segment_hit_local(commands, matrix, origin, zoom, hover_pos).is_some()
        {
            self.stage_cursor = Some(neoflash_ui::cursors::PEN_ADD);
        }
        let double_clicked = response.double_clicked() && !self.stage_view.guide_pointer;
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } if response.rect.contains(pos)
                    && self.selected_editable()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == response.layer_id) =>
                {
                    if let Some(handle) = handles
                        .iter()
                        .rev()
                        .filter(|h| screen(h.point).distance(pos) < 8.)
                        .min_by(|a, b| {
                            screen(a.point)
                                .distance_sq(pos)
                                .total_cmp(&screen(b.point).distance_sq(pos))
                        })
                        .copied()
                    {
                        self.finish_edits();
                        self.playing = false;
                        if handle.role == PointRole::Anchor {
                            if modifiers.shift {
                                let selected =
                                    self.toggle_selected_anchor(&element.id, handle.command);
                                if !selected {
                                    continue;
                                }
                            } else if !self.is_anchor_selected(&element.id, handle.command) {
                                self.vector.selected_anchors =
                                    Some((element.id.clone(), vec![handle.command]));
                            }
                        }
                        self.vector.point_drag = Some(PointDrag {
                            handle,
                            original: commands.clone(),
                            inverse: matrix,
                            symbol: self.symbol_id().to_owned(),
                            frame: self.frame,
                            layer: self.selection.single().unwrap().layer.clone(),
                            origin,
                            zoom,
                            press: pos,
                            began: false,
                            changed: false,
                            element: element.id.clone(),
                        });
                    } else if (modifiers.alt || double_clicked)
                        && let Some((cmd_idx, t)) =
                            path_segment_hit_local(commands, matrix, origin, zoom, pos)
                        && let Some((new_commands, inserted)) =
                            subdivide_path_at(commands, cmd_idx, t)
                        && validate_path_commands(&new_commands).is_ok()
                    {
                        let element_id = element.id.clone();
                        self.finish_edits();
                        self.playing = false;
                        self.transact(|s| {
                            s.ensure_selected_key();
                            if let Some(Element {
                                kind:
                                    ElementKind::Path {
                                        commands: destination,
                                        ..
                                    }
                                    | ElementKind::PaintedPath {
                                        commands: destination,
                                        ..
                                    },
                                ..
                            }) = s.selected_element_mut()
                            {
                                *destination = new_commands;
                            }
                        });
                        self.vector.selected_anchors = Some((element_id, vec![inserted]));
                        return;
                    }
                }
                egui::Event::PointerMoved(pos) => self.update_path_drag(pos, origin),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.update_path_drag(pos, origin);
                    self.finish_point_drag();
                }
                _ => {}
            }
        }
    }
    fn update_path_drag(&mut self, pos: Pos2, origin: Pos2) {
        let Some(mut drag) = self.vector.point_drag.take() else {
            return;
        };
        if !self.point_drag_matches(&drag) || drag.origin != origin || drag.zoom != self.zoom {
            if drag.began {
                self.history.cancel(&mut self.project);
            }
            return;
        }
        if !drag.began && pos.distance(drag.press) < 3. {
            self.vector.point_drag = Some(drag);
            return;
        }
        // The dragged anchor or control point snaps to guides and grid (ST-04).
        let anchor = crate::stage::affine(drag.inverse, drag.handle.point);
        let pos = self.snap_drag(drag.origin, drag.press, pos, anchor);
        let local = |point| {
            crate::stage::inverse(drag.inverse, Pos2::ZERO + (point - drag.origin) / drag.zoom)
        };
        let (Some(current), Some(press)) = (local(pos), local(drag.press)) else {
            self.vector.point_drag = Some(drag);
            return;
        };
        let mut commands = drag.original.clone();
        move_handle(
            &mut commands,
            drag.handle,
            drag.handle.point + (current - press),
        );
        drag.changed = commands != drag.original;
        if drag.changed && !drag.began {
            self.history.begin(&self.project);
            self.ensure_selected_key();
            drag.began = true;
        }
        if drag.began
            && let Some(Element {
                kind:
                    ElementKind::Path {
                        commands: destination,
                        ..
                    }
                    | ElementKind::PaintedPath {
                        commands: destination,
                        ..
                    },
                ..
            }) = self.selected_element_mut()
        {
            *destination = commands;
        }
        self.vector.point_drag = Some(drag);
    }
    fn point_drag_matches(&self, drag: &PointDrag) -> bool {
        self.tool == Tool::Subselect
            && self.symbol_id() == drag.symbol
            && self.frame == drag.frame
            && self
                .selection
                .single()
                .is_some_and(|target| target.layer == drag.layer && target.element == drag.element)
            && self.selected_editable()
    }

    pub(crate) fn is_anchor_selected(&self, element_id: &str, command: usize) -> bool {
        self.vector
            .selected_anchors
            .as_ref()
            .is_some_and(|(id, indices)| id == element_id && indices.contains(&command))
    }

    pub(crate) fn toggle_selected_anchor(&mut self, element_id: &str, command: usize) -> bool {
        match &mut self.vector.selected_anchors {
            Some((id, indices)) if id == element_id => {
                if let Some(pos) = indices.iter().position(|&i| i == command) {
                    indices.remove(pos);
                    false
                } else {
                    indices.push(command);
                    true
                }
            }
            _ => {
                self.vector.selected_anchors = Some((element_id.to_owned(), vec![command]));
                true
            }
        }
    }

    pub(crate) fn on_selected_path_segment(&self, origin: Pos2, pos: Pos2) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let (ElementKind::Path { ref commands, .. }
        | ElementKind::PaintedPath { ref commands, .. }) = element.kind
        else {
            return false;
        };
        let matrix = transform_matrix(element.transform);
        let zoom = self.zoom;
        let screen = |pt: Pos2| origin + crate::stage::affine(matrix, pt).to_vec2() * zoom;
        if handles(commands)
            .iter()
            .any(|h| screen(h.point).distance(pos) < 8.)
        {
            return false;
        }
        path_segment_hit_local(commands, matrix, origin, zoom, pos).is_some()
    }

    pub(crate) fn pen_subdivide_candidate(
        &self,
        origin: Pos2,
        pos: Pos2,
    ) -> Option<(crate::selection::SelectionTarget, Vec<PathCommand>, usize)> {
        let zoom = self.zoom;
        let try_element =
            |target: crate::selection::SelectionTarget,
             element: Element|
             -> Option<(crate::selection::SelectionTarget, Vec<PathCommand>, usize)> {
                let (ElementKind::Path { ref commands, .. }
                | ElementKind::PaintedPath { ref commands, .. }) = element.kind
                else {
                    return None;
                };
                let matrix = transform_matrix(element.transform);
                let screen = |pt: Pos2| origin + crate::stage::affine(matrix, pt).to_vec2() * zoom;
                if handles(commands)
                    .iter()
                    .any(|h| h.role == PointRole::Anchor && screen(h.point).distance(pos) < 8.)
                {
                    return None;
                }
                let (cmd_idx, t) = path_segment_hit_local(commands, matrix, origin, zoom, pos)?;
                let (new_commands, inserted) = subdivide_path_at(commands, cmd_idx, t)?;
                validate_path_commands(&new_commands).ok()?;
                Some((target, new_commands, inserted))
            };

        if self.selected_editable()
            && let Some(target) = self.selection.single().cloned()
            && let Some(element) = self.selected_sampled()
            && let Some(found) = try_element(target, element)
        {
            return Some(found);
        }

        let timeline = &self.symbol().timeline;
        for layer in timeline.layers.iter().rev() {
            if layer.properties.role == LayerRole::Folder
                || !layer_effectively_visible(timeline, layer)
                || layer_effectively_locked(timeline, layer)
            {
                continue;
            }
            for element in sample_layer(layer, self.frame).into_iter().rev() {
                let target = crate::selection::SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                };
                if let Some(found) = try_element(target, element) {
                    return Some(found);
                }
            }
        }
        None
    }

    pub(crate) fn try_pen_subdivide(&mut self, origin: Pos2, pos: Pos2) -> bool {
        let Some((target, new_commands, inserted)) = self.pen_subdivide_candidate(origin, pos)
        else {
            return false;
        };
        let element_id = target.element.clone();
        self.finish_edits();
        self.playing = false;
        self.transact(|s| {
            s.ensure_key_for(&target.layer, false);
            if let Some(layer_idx) = s.target_layer_index(&target) {
                let frame = s.frame;
                if let Some(key) = s.symbol_mut().timeline.layers[layer_idx].key_at_mut(frame)
                    && let Some(Element {
                        kind:
                            ElementKind::Path {
                                commands: destination,
                                ..
                            }
                            | ElementKind::PaintedPath {
                                commands: destination,
                                ..
                            },
                        ..
                    }) = key.elements.iter_mut().find(|e| e.id == target.element)
                {
                    *destination = new_commands;
                }
            }
            s.set_selection(vec![target]);
        });
        self.vector.selected_anchors = Some((element_id, vec![inserted]));
        true
    }

    pub(crate) fn delete_selected_vector_points(&mut self) -> bool {
        if !self.selected_editable() {
            return false;
        }
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        let Some(element) = self.selected_sampled() else {
            return false;
        };
        let (ElementKind::Path { ref commands, .. }
        | ElementKind::PaintedPath { ref commands, .. }) = element.kind
        else {
            return false;
        };
        let Some((selected_id, indices)) = self.vector.selected_anchors.clone() else {
            return false;
        };
        if selected_id != element.id || indices.is_empty() {
            return false;
        }
        let remaining = delete_anchors_from_path(commands, &indices);
        if remaining == *commands {
            return false;
        }
        self.playing = false;
        if remaining.is_empty() {
            self.transact(|s| {
                s.ensure_key_for(&target.layer, false);
                if let Some(index) = s.target_layer_index(&target) {
                    let frame = s.frame;
                    if let Some(key) = s.symbol_mut().timeline.layers[index].key_at_mut(frame) {
                        key.elements.retain(|el| el.id != target.element);
                    }
                }
                s.prune_authoring_groups();
                s.clear_selection();
            });
            self.vector.selected_anchors = None;
            return true;
        }
        if validate_path_commands(&remaining).is_err() {
            return false;
        }
        self.transact(|s| {
            s.ensure_selected_key();
            if let Some(Element {
                kind:
                    ElementKind::Path {
                        commands: destination,
                        ..
                    }
                    | ElementKind::PaintedPath {
                        commands: destination,
                        ..
                    },
                ..
            }) = s.selected_element_mut()
            {
                *destination = remaining;
            }
        });
        self.vector.selected_anchors = None;
        true
    }
}

fn path_segment_hit_local(
    commands: &[PathCommand],
    matrix: [f64; 6],
    origin: Pos2,
    zoom: f32,
    pos: Pos2,
) -> Option<(usize, f64)> {
    let stage_pt = Pos2::ZERO + (pos - origin) / zoom.max(1e-3);
    let local = crate::stage::inverse(matrix, stage_pt)?;
    let scale = matrix[0]
        .hypot(matrix[1])
        .max(matrix[2].hypot(matrix[3]))
        .max(1e-3);
    let tolerance = 7.0 / (scale * f64::from(zoom.max(1e-3)));
    let (cmd_idx, t, _closest, dist) =
        nearest_path_segment(commands, [f64::from(local.x), f64::from(local.y)])?;
    (dist <= tolerance).then_some((cmd_idx, t))
}

fn anchor_endpoint(cmd: &PathCommand) -> Option<[f64; 2]> {
    match *cmd {
        PathCommand::MoveTo { x, y }
        | PathCommand::LineTo { x, y }
        | PathCommand::QuadraticTo { x, y, .. }
        | PathCommand::CubicTo { x, y, .. } => Some([x, y]),
        PathCommand::Close => None,
    }
}

pub(crate) fn delete_anchors_from_path(
    commands: &[PathCommand],
    remove_indices: &[usize],
) -> Vec<PathCommand> {
    if remove_indices.is_empty() {
        return commands.to_vec();
    }
    let mut contours = Vec::new();
    let mut current_start = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if matches!(cmd, PathCommand::MoveTo { .. }) {
            if let Some(start) = current_start {
                contours.push(start..idx);
            }
            current_start = Some(idx);
        }
    }
    if let Some(start) = current_start {
        contours.push(start..commands.len());
    }

    let mut out = Vec::new();
    for range in contours {
        let start = range.start;
        let end = range.end;
        if end <= start {
            continue;
        }
        let closed = matches!(commands.get(end - 1), Some(PathCommand::Close));
        let body_end = if closed { end - 1 } else { end };
        if body_end <= start {
            continue;
        }
        let coincident_seam = closed
            && body_end > start + 1
            && anchor_endpoint(&commands[start])
                .zip(anchor_endpoint(&commands[body_end - 1]))
                .is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-4);

        if coincident_seam {
            let last = body_end - 1;
            let remove_seam = remove_indices.contains(&start) || remove_indices.contains(&last);
            let kept_interior: Vec<PathCommand> = ((start + 1)..last)
                .filter(|idx| !remove_indices.contains(idx))
                .map(|idx| commands[idx])
                .collect();
            if remove_seam {
                if kept_interior.len() < 2 {
                    continue;
                }
                if let Some([x, y]) = anchor_endpoint(&kept_interior[0]) {
                    out.push(PathCommand::MoveTo { x, y });
                    out.extend(kept_interior.into_iter().skip(1));
                    out.push(PathCommand::Close);
                }
            } else {
                if kept_interior.is_empty() {
                    continue;
                }
                out.push(commands[start]);
                out.extend(kept_interior);
                out.push(commands[last]);
                out.push(PathCommand::Close);
            }
        } else {
            let kept: Vec<PathCommand> = (start..body_end)
                .filter(|idx| !remove_indices.contains(idx))
                .map(|idx| commands[idx])
                .collect();
            if kept.len() < 2 {
                continue;
            }
            if let Some([x, y]) = anchor_endpoint(&kept[0]) {
                out.push(PathCommand::MoveTo { x, y });
                out.extend(kept.into_iter().skip(1));
                if closed {
                    out.push(PathCommand::Close);
                }
            }
        }
    }
    out
}

/// Gradient geometry stays authored; native preview samples the shared brush on
/// a bounded adaptive mesh. HTML5 uses Canvas gradients on the original path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_gradient_path(
    painter: &egui::Painter,
    commands: &[PathCommand],
    rule: FillRule,
    width: f64,
    fill: (&Color, Option<&GradientPaint>),
    stroke: (&Color, Option<&GradientPaint>),
    screen: impl Fn(Pos2) -> Pos2,
    color: impl Fn(Color) -> Color32,
) {
    let mut limited = false;
    for ((solid, gradient), width) in [(fill, None), (stroke, Some(width))] {
        if width.is_some_and(|w| w <= 0.)
            || gradient.map_or(solid[3] == 0, |g| g.stops.iter().all(|s| s.color[3] == 0))
        {
            continue;
        }
        let geometry = match tessellate(commands, rule, width, 0.1) {
            Ok(value) => value,
            Err(error) => {
                painter.text(
                    screen(Pos2::ZERO),
                    egui::Align2::LEFT_TOP,
                    error,
                    egui::FontId::monospace(crate::theme::PANEL_FONT),
                    UiPalette::current(painter.ctx()).error_text,
                );
                continue;
            }
        };
        let mut mesh = egui::Mesh::default();
        let sample = |p: Pos2| color(gradient.map_or(*solid, |g| g.sample(p.x as f64, p.y as f64)));
        for indices in geometry.indices.as_chunks::<3>().0 {
            let points = [indices[0], indices[1], indices[2]].map(|i| {
                let p = geometry.vertices[i as usize];
                Pos2::new(p.x, p.y)
            });
            let mut pending = vec![(points, 0u8)];
            while let Some(([a, b, c], depth)) = pending.pop() {
                let center = (a.to_vec2() + b.to_vec2() + c.to_vec2()) / 3.;
                let colors = [sample(a), sample(b), sample(c)];
                let mid = sample(center.to_pos2());
                let error = (0..4)
                    .map(|i| {
                        ((u16::from(colors[0][i])
                            + u16::from(colors[1][i])
                            + u16::from(colors[2][i])) as f32
                            / 3.
                            - f32::from(mid[i]))
                        .abs()
                    })
                    .fold(0f32, f32::max);
                let extent = (screen(a) - screen(b))
                    .length()
                    .max((screen(a) - screen(c)).length())
                    .max((screen(b) - screen(c)).length());
                let refine = gradient.is_some() && (error > 2. || extent > 24.);
                if refine && depth < 7 && mesh.vertices.len() + pending.len() * 3 < 150_000 {
                    let ab = a + (b - a) * 0.5;
                    let ac = a + (c - a) * 0.5;
                    let bc = b + (c - b) * 0.5;
                    pending.extend([
                        ([a, ab, ac], depth + 1),
                        ([ab, b, bc], depth + 1),
                        ([ac, bc, c], depth + 1),
                        ([ab, bc, ac], depth + 1),
                    ]);
                    continue;
                }
                limited |= refine && depth < 7;
                let index = mesh.vertices.len() as u32;
                for (p, c) in [a, b, c].into_iter().zip(colors) {
                    mesh.colored_vertex(screen(p), c);
                }
                mesh.indices.extend([index, index + 1, index + 2]);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    if limited {
        painter.text(
            screen(Pos2::ZERO),
            egui::Align2::LEFT_TOP,
            "Gradient preview mesh limit",
            egui::FontId::monospace(crate::theme::PANEL_FONT),
            UiPalette::current(painter.ctx()).error_text,
        );
    }
}
