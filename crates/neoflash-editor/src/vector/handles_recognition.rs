use super::smooth_reshape::{anchor_endpoint, contour_ranges};
use super::{Anchor, PathHandle, PointRole};
use eframe::egui::{Pos2, Vec2};
use neoflash_core::*;

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
    if handle.role == PointRole::Anchor {
        move_selected_anchors(commands, &[handle.command], to - handle.point);
        return;
    }
    let delta = to - handle.point;
    let dx = delta.x as f64;
    let dy = delta.y as f64;
    match (commands.get_mut(handle.command), handle.role) {
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

pub(crate) fn move_selected_anchors(
    commands: &mut [PathCommand],
    anchor_indices: &[usize],
    delta: Vec2,
) {
    let dx = delta.x as f64;
    let dy = delta.y as f64;
    let all_handles = handles(commands);
    let mut anchors: Vec<usize> = anchor_indices.to_vec();
    for &cmd_idx in anchor_indices {
        let Some(h) = all_handles
            .iter()
            .find(|h| h.command == cmd_idx && h.role == PointRole::Anchor)
        else {
            continue;
        };
        let start = (0..=cmd_idx)
            .rev()
            .find(|&i| matches!(commands.get(i), Some(PathCommand::MoveTo { .. })));
        let close = (cmd_idx..commands.len())
            .take_while(|&i| i == cmd_idx || !matches!(commands[i], PathCommand::MoveTo { .. }))
            .find(|&i| matches!(commands[i], PathCommand::Close));
        if let (Some(start), Some(close)) = (start, close)
            && close > start + 1
        {
            let last = close - 1;
            if cmd_idx == start || cmd_idx == last {
                let other = if cmd_idx == start { last } else { start };
                if all_handles.iter().any(|oh| {
                    oh.command == other
                        && oh.role == PointRole::Anchor
                        && oh.point.distance_sq(h.point) < 1e-8
                }) {
                    anchors.push(other);
                }
            }
        }
    }
    let mut targets = Vec::new();
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

type AnchorIncidentSegments = ([f64; 2], Option<(usize, [f64; 2])>, Option<usize>);

fn anchor_incident_segments(
    commands: &[PathCommand],
    anchor_cmd: usize,
) -> Option<AnchorIncidentSegments> {
    let anchor = anchor_endpoint(commands.get(anchor_cmd)?)?;
    for (start, body_end, closed) in contour_ranges(commands) {
        if anchor_cmd < start || anchor_cmd >= body_end {
            continue;
        }
        let coincident_seam = closed
            && body_end > start + 1
            && anchor_endpoint(&commands[start])
                .zip(anchor_endpoint(&commands[body_end - 1]))
                .is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-4);
        let incoming = if anchor_cmd > start && anchor_cmd < body_end {
            anchor_endpoint(&commands[anchor_cmd - 1]).map(|prev| (anchor_cmd, prev))
        } else if anchor_cmd == start && coincident_seam && body_end >= start + 2 {
            anchor_endpoint(&commands[body_end - 2]).map(|prev| (body_end - 1, prev))
        } else {
            None
        };
        let outgoing = if anchor_cmd == body_end - 1 && coincident_seam {
            (start + 1 < body_end).then_some(start + 1)
        } else if anchor_cmd + 1 < body_end {
            Some(anchor_cmd + 1)
        } else {
            None
        };
        return Some((anchor, incoming, outgoing));
    }
    None
}

pub(crate) fn collapse_anchor_handles(
    commands: &[PathCommand],
    anchor_cmd: usize,
) -> Vec<PathCommand> {
    let mut out = commands.to_vec();
    let Some(([ax, ay], incoming, outgoing)) = anchor_incident_segments(commands, anchor_cmd)
    else {
        return out;
    };
    if let Some((in_idx, prev_pt)) = incoming {
        match out[in_idx] {
            PathCommand::QuadraticTo { x, y, .. } => {
                out[in_idx] = PathCommand::LineTo { x, y };
            }
            PathCommand::CubicTo { cx1, cy1, x, y, .. } => {
                if (cx1 - prev_pt[0]).hypot(cy1 - prev_pt[1]) <= 1e-4 {
                    out[in_idx] = PathCommand::LineTo { x, y };
                } else {
                    out[in_idx] = PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2: x,
                        cy2: y,
                        x,
                        y,
                    };
                }
            }
            _ => {}
        }
    }
    if let Some(out_idx) = outgoing {
        match out[out_idx] {
            PathCommand::QuadraticTo { x, y, .. } => {
                out[out_idx] = PathCommand::LineTo { x, y };
            }
            PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
                if (cx2 - x).hypot(cy2 - y) <= 1e-4 {
                    out[out_idx] = PathCommand::LineTo { x, y };
                } else {
                    out[out_idx] = PathCommand::CubicTo {
                        cx1: ax,
                        cy1: ay,
                        cx2,
                        cy2,
                        x,
                        y,
                    };
                }
            }
            _ => {}
        }
    }
    out
}

pub(crate) fn pull_anchor_tangent(commands: &mut [PathCommand], anchor_cmd: usize, tangent: Vec2) {
    let Some(([ax, ay], incoming, outgoing)) = anchor_incident_segments(commands, anchor_cmd)
    else {
        return;
    };
    let tx = f64::from(tangent.x);
    let ty = f64::from(tangent.y);
    if let Some((in_idx, prev_pt)) = incoming {
        match commands[in_idx] {
            PathCommand::LineTo { x, y } => {
                commands[in_idx] = PathCommand::CubicTo {
                    cx1: prev_pt[0],
                    cy1: prev_pt[1],
                    cx2: ax - tx,
                    cy2: ay - ty,
                    x,
                    y,
                };
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                commands[in_idx] = PathCommand::CubicTo {
                    cx1: cx,
                    cy1: cy,
                    cx2: ax - tx,
                    cy2: ay - ty,
                    x,
                    y,
                };
            }
            PathCommand::CubicTo { cx1, cy1, x, y, .. } => {
                commands[in_idx] = PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2: ax - tx,
                    cy2: ay - ty,
                    x,
                    y,
                };
            }
            _ => {}
        }
    }
    if let Some(out_idx) = outgoing {
        match commands[out_idx] {
            PathCommand::LineTo { x, y } => {
                commands[out_idx] = PathCommand::CubicTo {
                    cx1: ax + tx,
                    cy1: ay + ty,
                    cx2: x,
                    cy2: y,
                    x,
                    y,
                };
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                commands[out_idx] = PathCommand::CubicTo {
                    cx1: ax + tx,
                    cy1: ay + ty,
                    cx2: cx,
                    cy2: cy,
                    x,
                    y,
                };
            }
            PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
                commands[out_idx] = PathCommand::CubicTo {
                    cx1: ax + tx,
                    cy1: ay + ty,
                    cx2,
                    cy2,
                    x,
                    y,
                };
            }
            _ => {}
        }
    }
}

/// Iterative Ramer–Douglas–Peucker keeps pencil geometry compact without recursion.
pub(super) fn simplify(points: &[Anchor], tolerance: f32) -> Vec<Anchor> {
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

pub(super) fn polyline_length(anchors: &[Anchor]) -> f32 {
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
pub(super) enum EndpointSide {
    Start,
    End,
}

#[derive(Clone, Debug)]
pub(super) struct OpenEndpointMatch {
    pub(super) element_id: Id,
    pub(super) side: EndpointSide,
    pub(super) world_point: Pos2,
    pub(super) matrix: [f64; 6],
    pub(super) same_style: bool,
}

pub(super) fn open_contour_endpoints(commands: &[PathCommand]) -> Option<((f64, f64), (f64, f64))> {
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

pub(super) fn inverse_f64(m: [f64; 6], px: f64, py: f64) -> Option<(f64, f64)> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d.abs() < 1e-10 {
        return None;
    }
    let x = px - m[4];
    let y = py - m[5];
    Some(((m[3] * x - m[2] * y) / d, (-m[1] * x + m[0] * y) / d))
}

pub(super) fn map_commands_inverse(
    commands: &[PathCommand],
    m: [f64; 6],
) -> Option<Vec<PathCommand>> {
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

pub(super) fn map_commands_forward(commands: &[PathCommand], matrix: [f64; 6]) -> Vec<PathCommand> {
    let map = |x: f64, y: f64| {
        (
            matrix[0] * x + matrix[2] * y + matrix[4],
            matrix[1] * x + matrix[3] * y + matrix[5],
        )
    };
    commands
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo { x, y } => {
                let (x, y) = map(x, y);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let (x, y) = map(x, y);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (cx, cy) = map(cx, cy);
                let (x, y) = map(x, y);
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
                let (cx1, cy1) = map(cx1, cy1);
                let (cx2, cy2) = map(cx2, cy2);
                let (x, y) = map(x, y);
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
        .collect()
}

pub(super) fn set_contour_start(commands: &mut [PathCommand], pt: (f64, f64)) {
    if let Some(PathCommand::MoveTo { x, y }) = commands.first_mut() {
        *x = pt.0;
        *y = pt.1;
    }
}

pub(super) fn set_contour_end(commands: &mut [PathCommand], pt: (f64, f64)) {
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

pub(super) fn reverse_open_contour(commands: &[PathCommand]) -> Option<Vec<PathCommand>> {
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
