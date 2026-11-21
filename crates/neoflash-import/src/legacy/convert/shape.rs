//! Convert directed binary fill edges into compound paths without flattening
//! curves. Archive layout/units are referenced in docs/IMPORTS.md. This graph
//! assembly is independently implemented: all directed edges are consumed once,
//! including separate contours meeting at a vertex; winding is preserved.
use super::*;
use crate::legacy::geometry::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Edge {
    start: [i32; 2],
    end: [i32; 2],
    control: Option<[i32; 2]>,
}
impl Edge {
    fn new(e: &BinaryEdge, reverse: bool) -> Self {
        Self {
            start: if reverse { e.end } else { e.start },
            end: if reverse { e.start } else { e.end },
            control: e.control,
        }
    }
    fn append(&self, commands: &mut Vec<PathCommand>) {
        let [x, y] = point(self.end);
        commands.push(if let Some(c) = self.control {
            let [cx, cy] = point(c);
            PathCommand::QuadraticTo { cx, cy, x, y }
        } else {
            PathCommand::LineTo { x, y }
        });
    }
}
fn point(p: [i32; 2]) -> [f64; 2] {
    [p[0] as f64 / 5120., p[1] as f64 / 5120.]
}
pub(super) fn matrix(w: [i32; 6]) -> [f64; 6] {
    [
        w[0] as f64 / 65536.,
        w[1] as f64 / 65536.,
        w[2] as f64 / 65536.,
        w[3] as f64 / 65536.,
        w[4] as f64 / 20.,
        w[5] as f64 / 20.,
    ]
}
pub(super) fn color(w: u32) -> Color {
    w.to_le_bytes()
}

const MAX_FILL_EDGES: usize = 100_000;
const MAX_STROKE_EDGES: usize = 250_000;

fn edges_bounding_rect(
    edges: &[Edge],
    raw_matrix: [f64; 6],
    base: Transform,
) -> (Transform, f64, f64) {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for e in edges {
        for axis in 0..2 {
            let p = f64::from(e.start[axis]) / 5120.0;
            let q = f64::from(e.end[axis]) / 5120.0;
            min[axis] = min[axis].min(p.min(q));
            max[axis] = max[axis].max(p.max(q));
            if let Some(ctrl) = e.control {
                let c = f64::from(ctrl[axis]) / 5120.0;
                let denom = p - 2.0 * c + q;
                if denom.abs() > 1e-12 {
                    let t = (p - c) / denom;
                    if t > 0.0 && t < 1.0 {
                        let u = 1.0 - t;
                        let v = u * u * p + 2.0 * u * t * c + t * t * q;
                        min[axis] = min[axis].min(v);
                        max[axis] = max[axis].max(v);
                    }
                }
            }
        }
    }
    if min[0].is_finite() && min[1].is_finite() && min[0] <= max[0] && min[1] <= max[1] {
        let mut placed = base;
        let tx = placed.x + raw_matrix[0] * min[0] + raw_matrix[2] * min[1];
        let ty = placed.y + raw_matrix[1] * min[0] + raw_matrix[3] * min[1];
        if tx.is_finite() && ty.is_finite() && tx.abs() <= 1e12 && ty.abs() <= 1e12 {
            placed.x = tx;
            placed.y = ty;
        }
        (
            placed,
            (max[0] - min[0]).clamp(0.0, 1e9),
            (max[1] - min[1]).clamp(0.0, 1e9),
        )
    } else {
        (base, 0.0, 0.0)
    }
}

fn split_stroke_commands(commands: Vec<PathCommand>, max_commands: usize) -> Vec<Vec<PathCommand>> {
    if commands.len() <= max_commands {
        return vec![commands];
    }
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut last_point = [0.0, 0.0];
    let mut subpath_start = [0.0, 0.0];
    for cmd in commands {
        if current.len() >= max_commands {
            chunks.push(std::mem::take(&mut current));
            if !matches!(cmd, PathCommand::MoveTo { .. }) {
                current.push(PathCommand::MoveTo {
                    x: last_point[0],
                    y: last_point[1],
                });
            }
        }
        match cmd {
            PathCommand::MoveTo { x, y } => {
                if current.len() + 1 >= max_commands && !current.is_empty() {
                    chunks.push(std::mem::take(&mut current));
                }
                last_point = [x, y];
                subpath_start = [x, y];
                current.push(cmd);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                last_point = [x, y];
                current.push(cmd);
            }
            PathCommand::Close => {
                if current.is_empty() {
                    current.push(PathCommand::MoveTo {
                        x: last_point[0],
                        y: last_point[1],
                    });
                }
                current.push(PathCommand::LineTo {
                    x: subpath_start[0],
                    y: subpath_start[1],
                });
                last_point = subpath_start;
            }
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn paths(edges: Vec<Edge>, closed: bool) -> Result<Vec<PathCommand>, String> {
    let limit = if closed {
        MAX_FILL_EDGES
    } else {
        MAX_STROKE_EDGES
    };
    if edges.len() > limit {
        return Err("Shape exceeds edge budget".into());
    }
    let mut outgoing: BTreeMap<[i32; 2], Vec<usize>> = BTreeMap::new();
    let mut balance: BTreeMap<[i32; 2], i32> = BTreeMap::new();
    for (i, e) in edges.iter().enumerate() {
        outgoing.entry(e.start).or_default().push(i);
        *balance.entry(e.start).or_default() += 1;
        *balance.entry(e.end).or_default() -= 1;
    }
    if closed && balance.values().any(|v| *v != 0) {
        return Err(
            "Fill boundary has unmatched endpoints; original edges retained in import source"
                .into(),
        );
    }
    let mut unused: BTreeSet<usize> = (0..edges.len()).collect();
    let mut commands = Vec::new();
    while let Some(&first) = unused.first() {
        let start = edges[first].start;
        let mut stack = vec![(start, None)];
        let mut circuit = Vec::new();
        while let Some((vertex, _)) = stack.last() {
            let next = outgoing.get_mut(vertex).and_then(|ids| {
                while let Some(id) = ids.pop() {
                    if unused.remove(&id) {
                        return Some(id);
                    }
                }
                None
            });
            if let Some(id) = next {
                stack.push((edges[id].end, Some(id)))
            } else {
                if let Some((_, Some(id))) = stack.pop() {
                    circuit.push(id)
                }
            }
        }
        circuit.reverse();
        let mut current = None;
        let mut contour_start = None;
        for id in circuit {
            let e = &edges[id];
            if current != Some(e.start) {
                if closed && current.is_some() && current != contour_start {
                    return Err("Cannot assemble closed fill graph".into());
                }
                let [x, y] = point(e.start);
                commands.push(PathCommand::MoveTo { x, y });
                contour_start = Some(e.start);
            }
            e.append(&mut commands);
            current = Some(e.end);
        }
        if closed {
            if current != contour_start {
                return Err("Unclosed fill boundary".into());
            }
            commands.push(PathCommand::Close);
        }
    }
    Ok(commands)
}

/// Returns `(has_open_caps, has_corner_joins)` for assembled stroke path commands.
/// Closed loops have no open endpoints (so cap style is visually invariant), and
/// single-segment open lines have no interior vertices (so join style is invariant).
fn stroke_topology_features(commands: &[PathCommand]) -> (bool, bool) {
    let mut degrees: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let quantize = |x: f64, y: f64| ((x * 20.0).round() as i64, (y * 20.0).round() as i64);
    let mut cur: Option<(i64, i64)> = None;
    let mut subpath_start: Option<(i64, i64)> = None;
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                let p = quantize(x, y);
                cur = Some(p);
                subpath_start = Some(p);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                let next = quantize(x, y);
                if let Some(prev) = cur
                    && prev != next
                {
                    *degrees.entry(prev).or_default() += 1;
                    *degrees.entry(next).or_default() += 1;
                }
                cur = Some(next);
            }
            PathCommand::Close => {
                if let (Some(prev), Some(start)) = (cur, subpath_start)
                    && prev != start
                {
                    *degrees.entry(prev).or_default() += 1;
                    *degrees.entry(start).or_default() += 1;
                }
                cur = subpath_start;
            }
        }
    }
    let has_open_caps = degrees.values().any(|&deg| deg == 1);
    let has_corner_joins = degrees.values().any(|&deg| deg >= 2);
    (has_open_caps, has_corner_joins)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum StrokeCap {
    Round,
    Butt,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum StrokeJoin {
    Round,
    Bevel,
    Miter { limit: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StrokeScaleMode {
    Normal,
    Horizontal,
    Vertical,
    None,
}

pub(super) fn decode_stroke_style(
    stroke: &BinaryStroke,
) -> (StrokeCap, StrokeJoin, StrokeScaleMode) {
    let cap = match stroke.extension.as_ref().map_or(1, |e| e.cap_style) {
        0 => StrokeCap::Butt,
        2 => StrokeCap::Square,
        _ => StrokeCap::Round,
    };
    let sharp_flag = (stroke.style_words[1] & 0x8000) != 0;
    let join = if let Some(ext) = &stroke.extension {
        match ext.join_style {
            0 => StrokeJoin::Miter {
                limit: (f64::from(ext.miter_limit_word) / 256.0).clamp(1.0, 255.0),
            },
            2 => StrokeJoin::Bevel,
            _ if sharp_flag => StrokeJoin::Miter { limit: 3.0 },
            _ => StrokeJoin::Round,
        }
    } else if sharp_flag {
        StrokeJoin::Miter { limit: 3.0 }
    } else {
        StrokeJoin::Round
    };
    let scale_mode = if stroke.width_word == 0 {
        StrokeScaleMode::None
    } else {
        match stroke.extension.as_ref().map_or(0, |e| e.scale_mode) {
            1 => StrokeScaleMode::Horizontal,
            2 => StrokeScaleMode::Vertical,
            3 => StrokeScaleMode::None,
            _ => StrokeScaleMode::Normal,
        }
    };
    (cap, join, scale_mode)
}

fn matrix_has_non_unit_linear_part(m: [f64; 6]) -> bool {
    (m[0] - 1.0).abs() > 1e-5 || m[1].abs() > 1e-5 || m[2].abs() > 1e-5 || (m[3] - 1.0).abs() > 1e-5
}

fn transform_commands_by_matrix(commands: &[PathCommand], m: [f64; 6]) -> Vec<PathCommand> {
    let map_pt =
        |x: f64, y: f64| -> [f64; 2] { [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]] };
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => {
                let [nx, ny] = map_pt(x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let [nx, ny] = map_pt(x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let [ncx, ncy] = map_pt(cx, cy);
                let [nx, ny] = map_pt(x, y);
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
                let [nc1x, nc1y] = map_pt(cx1, cy1);
                let [nc2x, nc2y] = map_pt(cx2, cy2);
                let [nx, ny] = map_pt(x, y);
                PathCommand::CubicTo {
                    cx1: nc1x,
                    cy1: nc1y,
                    cx2: nc2x,
                    cy2: nc2y,
                    x: nx,
                    y: ny,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn scaled_stroke_width_for_mode(
    base_width: f64,
    mode: StrokeScaleMode,
    raw_matrix: [f64; 6],
) -> f64 {
    let sx = raw_matrix[0].hypot(raw_matrix[1]);
    let sy = raw_matrix[2].hypot(raw_matrix[3]);
    let w = match mode {
        StrokeScaleMode::Normal => base_width * 0.5 * (sx + sy),
        StrokeScaleMode::Horizontal => base_width * sx,
        StrokeScaleMode::Vertical => base_width * sy,
        StrokeScaleMode::None => base_width,
    };
    w.clamp(0.05, 1e6)
}

fn flatten_quad_into(p0: [f64; 2], c: [f64; 2], p2: [f64; 2], depth: u8, out: &mut Vec<[f64; 2]>) {
    let sag_x = p0[0] - 2.0 * c[0] + p2[0];
    let sag_y = p0[1] - 2.0 * c[1] + p2[1];
    if depth >= 5 || sag_x.hypot(sag_y) * 0.25 <= 0.12 {
        if out
            .last()
            .is_none_or(|prev| (prev[0] - p2[0]).hypot(prev[1] - p2[1]) > 1e-5)
        {
            out.push(p2);
        }
        return;
    }
    let q0 = [0.5 * (p0[0] + c[0]), 0.5 * (p0[1] + c[1])];
    let q1 = [0.5 * (c[0] + p2[0]), 0.5 * (c[1] + p2[1])];
    let mid = [0.5 * (q0[0] + q1[0]), 0.5 * (q0[1] + q1[1])];
    flatten_quad_into(p0, q0, mid, depth + 1, out);
    flatten_quad_into(mid, q1, p2, depth + 1, out);
}

fn extract_stroke_polylines(commands: &[PathCommand]) -> Option<Vec<(Vec<[f64; 2]>, bool)>> {
    const MAX_POLYLINE_POINTS: usize = MAX_PATH_COMMANDS / 2;
    let mut result = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    let mut closed = false;
    let mut total_pts = 0usize;

    let flush = |pts: &mut Vec<[f64; 2]>,
                 is_closed: &mut bool,
                 total: &mut usize,
                 dst: &mut Vec<(Vec<[f64; 2]>, bool)>| {
        if pts.len() >= 2 {
            let mut c = *is_closed;
            if pts.len() >= 3
                && (pts[0][0] - pts.last().unwrap()[0]).hypot(pts[0][1] - pts.last().unwrap()[1])
                    <= 1e-3
            {
                c = true;
                pts.pop();
            } else if c
                && (pts[0][0] - pts.last().unwrap()[0]).hypot(pts[0][1] - pts.last().unwrap()[1])
                    <= 1e-3
            {
                pts.pop();
            }
            if pts.len() >= 2 && (!c || pts.len() >= 3) {
                *total = total.saturating_add(pts.len());
                dst.push((std::mem::take(pts), c));
            } else {
                pts.clear();
            }
        } else {
            pts.clear();
        }
        *is_closed = false;
    };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                flush(&mut cur, &mut closed, &mut total_pts, &mut result);
                cur.push([x, y]);
            }
            PathCommand::LineTo { x, y } => {
                if cur
                    .last()
                    .is_none_or(|last| (last[0] - x).hypot(last[1] - y) > 1e-5)
                {
                    cur.push([x, y]);
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(&p0) = cur.last() {
                    flatten_quad_into(p0, [cx, cy], [x, y], 0, &mut cur);
                } else {
                    cur.push([x, y]);
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
                if let Some(&p0) = cur.last() {
                    for step in 1..=8 {
                        let t = f64::from(step) / 8.0;
                        let u = 1.0 - t;
                        let px = u * u * u * p0[0]
                            + 3.0 * u * u * t * cx1
                            + 3.0 * u * t * t * cx2
                            + t * t * t * x;
                        let py = u * u * u * p0[1]
                            + 3.0 * u * u * t * cy1
                            + 3.0 * u * t * t * cy2
                            + t * t * t * y;
                        if (cur.last().unwrap()[0] - px).hypot(cur.last().unwrap()[1] - py) > 1e-5 {
                            cur.push([px, py]);
                        }
                    }
                } else {
                    cur.push([x, y]);
                }
            }
            PathCommand::Close => {
                closed = true;
                flush(&mut cur, &mut closed, &mut total_pts, &mut result);
            }
        }
        if total_pts.saturating_add(cur.len()) > MAX_POLYLINE_POINTS {
            return None;
        }
    }
    flush(&mut cur, &mut closed, &mut total_pts, &mut result);
    (total_pts <= MAX_POLYLINE_POINTS).then_some(result)
}

fn append_round_arc(
    center: [f64; 2],
    radius: f64,
    start_dir: [f64; 2],
    end_dir: [f64; 2],
    sweep_sign: f64,
    out: &mut Vec<PathCommand>,
) {
    let a0 = start_dir[1].atan2(start_dir[0]);
    let a1 = end_dir[1].atan2(end_dir[0]);
    let mut delta = a1 - a0;
    if sweep_sign < 0.0 {
        while delta > 1e-6 {
            delta -= std::f64::consts::TAU;
        }
        if delta < -std::f64::consts::TAU {
            delta += std::f64::consts::TAU;
        }
    } else {
        while delta < -1e-6 {
            delta += std::f64::consts::TAU;
        }
        if delta > std::f64::consts::TAU {
            delta -= std::f64::consts::TAU;
        }
    }
    let steps = ((delta.abs() / (std::f64::consts::FRAC_PI_4)).ceil() as usize).clamp(1, 8);
    let step_angle = delta / (steps as f64);
    let cos_half = (step_angle * 0.5).cos().max(1e-4);
    for k in 0..steps {
        let mid_a = a0 + (k as f64 + 0.5) * step_angle;
        let end_a = a0 + ((k + 1) as f64) * step_angle;
        let ctrl_r = radius / cos_half;
        out.push(PathCommand::QuadraticTo {
            cx: center[0] + ctrl_r * mid_a.cos(),
            cy: center[1] + ctrl_r * mid_a.sin(),
            x: center[0] + radius * end_a.cos(),
            y: center[1] + radius * end_a.sin(),
        });
    }
}

fn emit_corner_join(
    p: [f64; 2],
    (t_in, len_in): ([f64; 2], f64),
    (t_out, len_out): ([f64; 2], f64),
    half_w: f64,
    join: StrokeJoin,
    out: &mut Vec<PathCommand>,
) {
    let n_in = [-t_in[1], t_in[0]];
    let n_out = [-t_out[1], t_out[0]];
    let p_in = [p[0] + half_w * n_in[0], p[1] + half_w * n_in[1]];
    let p_out = [p[0] + half_w * n_out[0], p[1] + half_w * n_out[1]];
    let cross = t_in[0] * t_out[1] - t_in[1] * t_out[0];

    if cross > 1e-6 {
        // Inner turn on the `+N` side: intersect offset lines when bounded within segment lengths.
        let b_raw = [n_in[0] + n_out[0], n_in[1] + n_out[1]];
        let b_len = b_raw[0].hypot(b_raw[1]);
        if b_len > 1e-6 {
            let b = [b_raw[0] / b_len, b_raw[1] / b_len];
            let cos_half = (b[0] * n_in[0] + b[1] * n_in[1]).max(1e-6);
            let d = half_w / cos_half;
            let trim = (d * d - half_w * half_w).max(0.0).sqrt();
            if trim <= len_in.min(len_out) {
                out.push(PathCommand::LineTo {
                    x: p[0] + d * b[0],
                    y: p[1] + d * b[1],
                });
                return;
            }
        }
        out.push(PathCommand::LineTo {
            x: p_in[0],
            y: p_in[1],
        });
        out.push(PathCommand::LineTo { x: p[0], y: p[1] });
        out.push(PathCommand::LineTo {
            x: p_out[0],
            y: p_out[1],
        });
        return;
    }

    out.push(PathCommand::LineTo {
        x: p_in[0],
        y: p_in[1],
    });
    if cross >= -1e-6 {
        return;
    }

    // Outer turn on the `+N` side (`cross < -1e-6`).
    match join {
        StrokeJoin::Bevel => {
            out.push(PathCommand::LineTo {
                x: p_out[0],
                y: p_out[1],
            });
        }
        StrokeJoin::Miter { limit } => {
            let b_raw = [n_in[0] + n_out[0], n_in[1] + n_out[1]];
            let b_len = b_raw[0].hypot(b_raw[1]);
            if b_len > 1e-6 {
                let b = [b_raw[0] / b_len, b_raw[1] / b_len];
                let cos_half = (b[0] * n_in[0] + b[1] * n_in[1]).max(1e-6);
                let miter_ratio = 1.0 / cos_half;
                if miter_ratio <= limit {
                    let d = half_w * miter_ratio;
                    out.push(PathCommand::LineTo {
                        x: p[0] + d * b[0],
                        y: p[1] + d * b[1],
                    });
                }
            }
            out.push(PathCommand::LineTo {
                x: p_out[0],
                y: p_out[1],
            });
        }
        StrokeJoin::Round => {
            append_round_arc(p, half_w, n_in, n_out, -1.0, out);
        }
    }
}

fn emit_end_cap(
    endpoint: [f64; 2],
    tangent: [f64; 2],
    half_w: f64,
    cap: StrokeCap,
    out: &mut Vec<PathCommand>,
) {
    let n = [-tangent[1], tangent[0]];
    let p_plus = [endpoint[0] + half_w * n[0], endpoint[1] + half_w * n[1]];
    let p_minus = [endpoint[0] - half_w * n[0], endpoint[1] - half_w * n[1]];
    out.push(PathCommand::LineTo {
        x: p_plus[0],
        y: p_plus[1],
    });
    match cap {
        StrokeCap::Butt => {
            out.push(PathCommand::LineTo {
                x: p_minus[0],
                y: p_minus[1],
            });
        }
        StrokeCap::Square => {
            out.push(PathCommand::LineTo {
                x: p_plus[0] + half_w * tangent[0],
                y: p_plus[1] + half_w * tangent[1],
            });
            out.push(PathCommand::LineTo {
                x: p_minus[0] + half_w * tangent[0],
                y: p_minus[1] + half_w * tangent[1],
            });
            out.push(PathCommand::LineTo {
                x: p_minus[0],
                y: p_minus[1],
            });
        }
        StrokeCap::Round => {
            append_round_arc(endpoint, half_w, n, [-n[0], -n[1]], -1.0, out);
        }
    }
}

fn expand_stroke_to_outline(
    commands: &[PathCommand],
    width: f64,
    cap: StrokeCap,
    join: StrokeJoin,
) -> Option<Vec<PathCommand>> {
    let half_w = (width * 0.5).max(0.025);
    let polylines = extract_stroke_polylines(commands)?;
    if polylines.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for (pts, closed) in polylines {
        if closed {
            for pass in 0..2 {
                let chain: Vec<[f64; 2]> = if pass == 0 {
                    pts.clone()
                } else {
                    pts.iter().copied().rev().collect()
                };
                let m = chain.len();
                let mut segs = Vec::with_capacity(m);
                for i in 0..m {
                    let a = chain[i];
                    let b = chain[(i + 1) % m];
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let len = dx.hypot(dy);
                    if len <= 1e-6 {
                        return None;
                    }
                    segs.push(([dx / len, dy / len], len));
                }
                let start_idx = out.len();
                out.push(PathCommand::MoveTo { x: 0.0, y: 0.0 });
                for i in 0..m {
                    let prev = (i + m - 1) % m;
                    emit_corner_join(chain[i], segs[prev], segs[i], half_w, join, &mut out);
                    if out.len() > MAX_PATH_COMMANDS {
                        return None;
                    }
                }
                if let Some(PathCommand::LineTo { x, y }) = out.get(start_idx + 1).copied() {
                    out[start_idx] = PathCommand::MoveTo { x, y };
                } else {
                    return None;
                }
                out.push(PathCommand::Close);
                if out.len() > MAX_PATH_COMMANDS {
                    return None;
                }
            }
        } else {
            let m = pts.len();
            let mut segs = Vec::with_capacity(m - 1);
            for i in 0..(m - 1) {
                let dx = pts[i + 1][0] - pts[i][0];
                let dy = pts[i + 1][1] - pts[i][1];
                let len = dx.hypot(dy);
                if len <= 1e-6 {
                    return None;
                }
                segs.push(([dx / len, dy / len], len));
            }
            let n0 = [-segs[0].0[1], segs[0].0[0]];
            let start_pt = match cap {
                StrokeCap::Square => [
                    pts[0][0] + half_w * n0[0] - half_w * segs[0].0[0],
                    pts[0][1] + half_w * n0[1] - half_w * segs[0].0[1],
                ],
                _ => [pts[0][0] + half_w * n0[0], pts[0][1] + half_w * n0[1]],
            };
            out.push(PathCommand::MoveTo {
                x: start_pt[0],
                y: start_pt[1],
            });
            for i in 1..(m - 1) {
                emit_corner_join(pts[i], segs[i - 1], segs[i], half_w, join, &mut out);
                if out.len() > MAX_PATH_COMMANDS {
                    return None;
                }
            }
            let last_t = segs[m - 2].0;
            if cap == StrokeCap::Square {
                let nl = [-last_t[1], last_t[0]];
                out.push(PathCommand::LineTo {
                    x: pts[m - 1][0] + half_w * nl[0] + half_w * last_t[0],
                    y: pts[m - 1][1] + half_w * nl[1] + half_w * last_t[1],
                });
                out.push(PathCommand::LineTo {
                    x: pts[m - 1][0] - half_w * nl[0] + half_w * last_t[0],
                    y: pts[m - 1][1] - half_w * nl[1] + half_w * last_t[1],
                });
            } else {
                emit_end_cap(pts[m - 1], last_t, half_w, cap, &mut out);
            }
            // Return along the opposite side (`pts` reversed).
            for i in (1..(m - 1)).rev() {
                let rev_in = ([-segs[i].0[0], -segs[i].0[1]], segs[i].1);
                let rev_out = ([-segs[i - 1].0[0], -segs[i - 1].0[1]], segs[i - 1].1);
                emit_corner_join(pts[i], rev_in, rev_out, half_w, join, &mut out);
                if out.len() > MAX_PATH_COMMANDS {
                    return None;
                }
            }
            let first_rev_t = [-segs[0].0[0], -segs[0].0[1]];
            if cap == StrokeCap::Square {
                let nf = [-first_rev_t[1], first_rev_t[0]];
                out.push(PathCommand::LineTo {
                    x: pts[0][0] + half_w * nf[0] + half_w * first_rev_t[0],
                    y: pts[0][1] + half_w * nf[1] + half_w * first_rev_t[1],
                });
            } else {
                emit_end_cap(pts[0], first_rev_t, half_w, cap, &mut out);
            }
            out.push(PathCommand::Close);
        }
        if out.len() > MAX_PATH_COMMANDS {
            return None;
        }
    }
    (!out.is_empty() && out.len() <= MAX_PATH_COMMANDS).then_some(out)
}

/// A paint and, when it is only approximated, the reason for the report.
type Paint = (
    Color,
    Option<GradientPaint>,
    Option<(u16, [f64; 6], bool, bool)>,
    Option<&'static str>,
);

#[derive(Default)]
pub(super) struct ConvertedShape {
    pub elements: Vec<Element>,
    pub bitmap_fills: Vec<AuthoredBitmapFill>,
    pub appearances: Vec<ElementAppearance>,
    pub vector_effects: Vec<ElementVectorEffects>,
}

fn paint(fill: &BinaryFill, bitmaps: &BTreeMap<u16, super::assets::BitmapAsset>) -> Paint {
    const STANDIN: Color = [128, 128, 128, 255];
    match &fill.encoding {
        BinaryFillEncoding::Legacy { word: _ } => (color(fill.packed_value), None, None, None),
        BinaryFillEncoding::Extended {
            flags,
            secondary_flags: _,
            payload: BinaryFillPayload::Plain,
        } if flags & 0x70 == 0 => (color(fill.packed_value), None, None, None),
        BinaryFillEncoding::Extended {
            flags,
            payload:
                BinaryFillPayload::Gradient {
                    matrix_words,
                    extension,
                    stops,
                },
            ..
        } => {
            let Some(kind) = (match *flags {
                0x10 => Some(GradientKind::Linear),
                0x12 | 0x13 => Some(GradientKind::Radial),
                _ => None,
            }) else {
                let first = stops.first().map_or(STANDIN, |s| color(s.color_word));
                return (
                    first,
                    None,
                    None,
                    Some("an unknown gradient kind is shown in its first color"),
                );
            };
            let ext_flags = extension.as_ref().map_or(0, |e| e.flags);
            let spread = match (ext_flags >> 2) & 3 {
                1 => GradientSpread::Reflect,
                2 => GradientSpread::Repeat,
                _ => GradientSpread::Extend,
            };
            let linear_rgb = ext_flags & 1 != 0;
            let note = (ext_flags & !13 != 0)
                .then_some("gradient spread/interpolation modes are drawn as pad/RGB");
            let gradient = GradientPaint {
                kind,
                matrix: matrix(*matrix_words),
                stops: stops
                    .iter()
                    .map(|s| GradientStop {
                        ratio: s.ratio,
                        color: color(s.color_word),
                    })
                    .collect(),
                focal_point_ratio: match kind {
                    GradientKind::Radial => extension.as_ref().map_or(0., |e| {
                        ((e.focal_point as i16) as f64 / 255.).clamp(-1., 1.)
                    }),
                    GradientKind::Linear => 0.,
                },
                spread,
                linear_rgb,
            };
            ([0, 0, 0, 255], Some(gradient), None, note)
        }
        BinaryFillEncoding::Extended {
            flags,
            payload:
                BinaryFillPayload::Bitmap {
                    matrix_words,
                    media_id,
                },
            ..
        } if (*flags & 0x70) == 0x40 => {
            let raw = matrix(*matrix_words);
            if let Some(asset) = bitmaps.get(media_id) {
                let fill_matrix = [
                    (raw[0] / 20.0) * asset.scale_x,
                    (raw[1] / 20.0) * asset.scale_x,
                    (raw[2] / 20.0) * asset.scale_y,
                    (raw[3] / 20.0) * asset.scale_y,
                    raw[4],
                    raw[5],
                ];
                if fill_matrix.iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
                    let repeat = (*flags & 1) == 0;
                    let smoothing = (*flags & 2) == 0;
                    (
                        asset.average_color,
                        None,
                        Some((*media_id, fill_matrix, repeat, smoothing)),
                        None,
                    )
                } else {
                    (
                        STANDIN,
                        None,
                        None,
                        Some("unresolved bitmap fill is shown as neutral gray"),
                    )
                }
            } else {
                (
                    STANDIN,
                    None,
                    None,
                    Some("unresolved bitmap fill is shown as neutral gray"),
                )
            }
        }
        _ => (
            STANDIN,
            None,
            None,
            Some("unresolved bitmap fill is shown as neutral gray"),
        ),
    }
}

#[cfg(test)]
pub(super) fn convert_shape(
    shape: &BinaryShapeRecord,
    id: &str,
    report: &mut ImportReport,
    location: &str,
) -> Result<Vec<Element>, String> {
    convert_shape_with_effects(shape, id, &BTreeMap::new(), report, location)
        .map(|converted| converted.elements)
}

pub(super) fn convert_shape_with_effects(
    shape: &BinaryShapeRecord,
    id: &str,
    bitmaps: &BTreeMap<u16, super::assets::BitmapAsset>,
    report: &mut ImportReport,
    location: &str,
) -> Result<ConvertedShape, String> {
    convert_shape_with_effects_opts(shape, id, bitmaps, report, location, true, true)
}

pub(super) fn convert_shape_with_effects_opts(
    shape: &BinaryShapeRecord,
    id: &str,
    bitmaps: &BTreeMap<u16, super::assets::BitmapAsset>,
    report: &mut ImportReport,
    location: &str,
    allow_outline_expansion: bool,
    dynamic_scale_possible: bool,
) -> Result<ConvertedShape, String> {
    let raw_matrix = matrix(shape.matrix_words);
    let transform = Transform::from_matrix(raw_matrix).ok_or("Invalid shape matrix")?;
    let mut output = ConvertedShape::default();
    // Each paint converts independently; approximations are reported once per shape.
    let mut notes = Vec::new();
    for (i, fill) in shape.fills.iter().enumerate() {
        let index = (i + 1) as u16;
        let edges: Vec<_> = shape
            .edges
            .iter()
            .filter_map(|e| {
                let a = e.styles.fill_0 & 0x7fff;
                let b = e.styles.fill_1 & 0x7fff;
                if a == b {
                    None
                } else if b == index {
                    Some(Edge::new(e, false))
                } else if a == index {
                    Some(Edge::new(e, true))
                } else {
                    None
                }
            })
            .collect();
        if edges.is_empty() {
            continue;
        }
        let (fill_color, fill_gradient, bitmap_spec, note) = paint(fill, bitmaps);
        let elem_id = format!("{id}-fill-{i}");
        if edges.len() > MAX_FILL_EDGES {
            notes.extend(note.map(str::to_owned));
            notes.push(format!(
                "fill {} exceeds edge budget and is represented by its bounding box",
                i + 1
            ));
            let (placed, rect_w, rect_h) = edges_bounding_rect(&edges, raw_matrix, transform);
            output.elements.push(Element {
                id: elem_id,
                name: format!("Fill {} (budget placeholder)", i + 1),
                transform: placed,
                kind: ElementKind::Rectangle {
                    width: rect_w,
                    height: rect_h,
                    fill: fill_color,
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                scripts: vec![],
                accessibility: None,
                ..Element::default()
            });
            continue;
        }
        let bounds_fallback = edges_bounding_rect(&edges, raw_matrix, transform);
        let commands = match paths(edges, true) {
            Ok(commands) => commands,
            Err(e) => {
                notes.push(format!("fill {} skipped: {e}", i + 1));
                continue;
            }
        };
        notes.extend(note.map(str::to_owned));
        if commands.len() > MAX_PATH_COMMANDS {
            notes.push(format!(
                "fill {} exceeds the {}-command path limit and is represented by its bounding box",
                i + 1,
                MAX_PATH_COMMANDS
            ));
            output.elements.push(Element {
                id: elem_id,
                name: format!("Fill {} (budget placeholder)", i + 1),
                transform: bounds_fallback.0,
                kind: ElementKind::Rectangle {
                    width: bounds_fallback.1,
                    height: bounds_fallback.2,
                    fill: fill_color,
                    stroke: [0; 4],
                    stroke_width: 0.0,
                },
                scripts: vec![],
                accessibility: None,
                ..Element::default()
            });
            continue;
        }
        if let Some((media_id, fill_matrix, repeat, smoothing)) = bitmap_spec
            && let Some(asset) = bitmaps.get(&media_id)
        {
            output.bitmap_fills.push(AuthoredBitmapFill {
                element_id: elem_id.clone(),
                resource_id: asset.resource_id.clone(),
                matrix: fill_matrix,
                repeat,
                smoothing,
            });
        }
        output.elements.push(Element {
            id: elem_id,
            name: format!("Fill {}", i + 1),
            transform,
            kind: ElementKind::PaintedPath {
                commands,
                fill_rule: FillRule::NonZero,
                fill: fill_color,
                stroke: [0; 4],
                fill_gradient,
                stroke_gradient: None,
                stroke_width: 0.,
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        });
    }
    for (i, stroke) in shape.strokes.iter().enumerate() {
        let index = (i + 1) as u16;
        let edges: Vec<_> = shape
            .edges
            .iter()
            .filter(|e| e.styles.stroke & 0x7fff == index)
            .map(|e| Edge::new(e, false))
            .collect();
        if edges.is_empty() {
            continue;
        }
        let first_style = stroke.style_words[0];
        let second_style = stroke.style_words[1];
        let stroke_type = if first_style != 0 {
            1
        } else {
            second_style & 7
        };
        if stroke_type >= 6 {
            notes.push("dashed/dotted/hatched strokes are drawn solid".into());
        }
        let (stroke_color, stroke_gradient, stroke_bitmap) = if let Some(e) = &stroke.extension {
            let (c, g, bm, note) = paint(&e.paint, bitmaps);
            notes.extend(note.map(str::to_owned));
            (c, g, bm)
        } else {
            (color(stroke.color_word), None, None)
        };
        let width = stroke.width_word as f64 / 20.;
        if width == 0. {
            report.warning("fla_hairline_rendering",location,"A device hairline is represented as a one-pixel stroke; the original width remains in source provenance.")
        }
        let stroke_width = if width == 0. { 1. } else { width };
        if edges.len() > MAX_STROKE_EDGES {
            notes.push(format!(
                "stroke {} exceeds edge budget and is represented by its bounding box",
                i + 1
            ));
            let (placed, rect_w, rect_h) = edges_bounding_rect(&edges, raw_matrix, transform);
            output.elements.push(Element {
                id: format!("{id}-stroke-{i}"),
                name: format!("Stroke {} (budget placeholder)", i + 1),
                transform: placed,
                kind: ElementKind::Rectangle {
                    width: rect_w,
                    height: rect_h,
                    fill: [0; 4],
                    stroke: stroke_color,
                    stroke_width,
                },
                scripts: vec![],
                accessibility: None,
                ..Element::default()
            });
            continue;
        }
        let commands = match paths(edges, false) {
            Ok(commands) => commands,
            Err(e) => {
                notes.push(format!("stroke {} skipped: {e}", i + 1));
                continue;
            }
        };
        let (has_open_caps, has_corner_joins) = stroke_topology_features(&commands);
        let (cap, join, scale_mode) = decode_stroke_style(stroke);
        let non_round_cap_or_join = (cap != StrokeCap::Round && has_open_caps)
            || (join != StrokeJoin::Round && has_corner_joins);
        let bake_shape_matrix_for_scale = stroke.width_word > 0
            && scale_mode != StrokeScaleMode::Normal
            && matrix_has_non_unit_linear_part(raw_matrix)
            && allow_outline_expansion;

        let (
            effective_commands,
            effective_transform,
            effective_width,
            effective_grad,
            effective_bitmap,
        ) = if bake_shape_matrix_for_scale
            || (width == 0.0
                && matrix_has_non_unit_linear_part(raw_matrix)
                && allow_outline_expansion)
        {
            let baked_cmds = transform_commands_by_matrix(&commands, raw_matrix);
            let baked_w = if width == 0.0 {
                1.0
            } else {
                scaled_stroke_width_for_mode(stroke_width, scale_mode, raw_matrix)
            };
            let baked_grad = stroke_gradient.clone().map(|mut g| {
                g.matrix = neoflash_core::affine_multiply(raw_matrix, g.matrix);
                g
            });
            let baked_bm = stroke_bitmap.map(|(mid, fm, rep, sm)| {
                (mid, neoflash_core::affine_multiply(raw_matrix, fm), rep, sm)
            });
            (
                baked_cmds,
                Transform::default(),
                baked_w,
                baked_grad,
                baked_bm,
            )
        } else {
            (
                commands,
                transform,
                stroke_width,
                stroke_gradient.clone(),
                stroke_bitmap,
            )
        };

        let unpreserved_dynamic_scale = stroke.width_word > 0
            && scale_mode != StrokeScaleMode::Normal
            && (dynamic_scale_possible || !allow_outline_expansion);

        if allow_outline_expansion
            && stroke_type == 0
            && non_round_cap_or_join
            && let Some(outline_commands) =
                expand_stroke_to_outline(&effective_commands, effective_width, cap, join)
                    .filter(|cmds| cmds.len() <= MAX_PATH_COMMANDS)
        {
            if unpreserved_dynamic_scale {
                report.warning("fla_stroke_rendering",location,"Stroke cap/join/scale editor settings remain in the embedded source; this renderer uses scaling round strokes.");
            }
            let elem_id = format!("{id}-stroke-{i}");
            if let Some((media_id, fill_matrix, repeat, smoothing)) = effective_bitmap
                && let Some(asset) = bitmaps.get(&media_id)
            {
                output.bitmap_fills.push(AuthoredBitmapFill {
                    element_id: elem_id.clone(),
                    resource_id: asset.resource_id.clone(),
                    matrix: fill_matrix,
                    repeat,
                    smoothing,
                });
            }
            output.elements.push(Element {
                id: elem_id,
                name: format!("Stroke {}", i + 1),
                transform: effective_transform,
                kind: ElementKind::PaintedPath {
                    commands: outline_commands,
                    fill_rule: FillRule::NonZero,
                    fill: stroke_color,
                    stroke: [0; 4],
                    fill_gradient: effective_grad,
                    stroke_gradient: None,
                    stroke_width: 0.0,
                },
                scripts: vec![],
                accessibility: None,
                ..Element::default()
            });
            continue;
        }

        if non_round_cap_or_join || unpreserved_dynamic_scale {
            report.warning("fla_stroke_rendering",location,"Stroke cap/join/scale editor settings remain in the embedded source; this renderer uses scaling round strokes.");
        }
        let chunks = split_stroke_commands(effective_commands, MAX_PATH_COMMANDS);
        let chunk_count = chunks.len();
        for (chunk_idx, chunk_commands) in chunks.into_iter().enumerate() {
            let elem_id = if chunk_idx == 0 {
                format!("{id}-stroke-{i}")
            } else {
                format!("{id}-stroke-{i}-part-{chunk_idx}")
            };
            let elem_name = if chunk_count == 1 {
                format!("Stroke {}", i + 1)
            } else {
                format!("Stroke {} (part {})", i + 1, chunk_idx + 1)
            };
            if let Some((media_id, fill_matrix, repeat, smoothing)) = effective_bitmap
                && let Some(asset) = bitmaps.get(&media_id)
            {
                output.bitmap_fills.push(AuthoredBitmapFill {
                    element_id: elem_id.clone(),
                    resource_id: asset.resource_id.clone(),
                    matrix: fill_matrix,
                    repeat,
                    smoothing,
                });
            }
            match stroke_type {
                1 => {
                    let dash = (f64::from(first_style) / 20.0).clamp(0.5, 1000.0);
                    let gap = (f64::from(second_style & 0x7fff) / 20.0).clamp(0.5, 1000.0);
                    output.appearances.push(ElementAppearance {
                        element_id: elem_id.clone(),
                        entries: vec![AppearancePaintEntry::Stroke {
                            color: stroke_color,
                            gradient: effective_grad.clone(),
                            width: effective_width,
                            swatch_id: None,
                            offset_distance: 0.0,
                            pattern_brush: Some(PatternBrushKind::DashPattern { dash, gap }),
                            opacity: 1.0,
                        }],
                        ..ElementAppearance::default()
                    });
                }
                2 => {
                    let dash = effective_width.clamp(0.5, 1000.0);
                    let gap = (f64::from(second_style >> 3) / 20.0).clamp(0.5, 1000.0);
                    output.appearances.push(ElementAppearance {
                        element_id: elem_id.clone(),
                        entries: vec![AppearancePaintEntry::Stroke {
                            color: stroke_color,
                            gradient: effective_grad.clone(),
                            width: effective_width,
                            swatch_id: None,
                            offset_distance: 0.0,
                            pattern_brush: Some(PatternBrushKind::DashPattern { dash, gap }),
                            opacity: 1.0,
                        }],
                        ..ElementAppearance::default()
                    });
                }
                3 => {
                    let pattern = (second_style >> 3) & 7;
                    let wave_height = (second_style >> 6) & 3;
                    let wave_length = (second_style >> 8) & 3;
                    output.vector_effects.push(ElementVectorEffects {
                        element_id: elem_id.clone(),
                        procedural_effects: vec![ProceduralVectorEffect::ZigZag {
                            size: match wave_height {
                                0 => 0.5,
                                1 => 2.0,
                                2 => 4.0,
                                _ => 7.0,
                            },
                            ridges_per_segment: match wave_length {
                                0 => 8,
                                1 => 5,
                                2 => 3,
                                _ => 2,
                            },
                            smooth: (pattern & 1) != 0,
                        }],
                        ..ElementVectorEffects::default()
                    });
                }
                4 => {
                    let dot_size = (second_style >> 3) & 3;
                    let variation = (second_style >> 5) & 3;
                    let density = (second_style >> 7) & 3;
                    output.vector_effects.push(ElementVectorEffects {
                        element_id: elem_id.clone(),
                        procedural_effects: vec![ProceduralVectorEffect::Roughen {
                            size: match dot_size {
                                0 => 0.75,
                                1 => 1.5,
                                2 => 3.0,
                                _ => 5.0,
                            },
                            detail: match density {
                                0 => 8,
                                1 => 6,
                                2 => 4,
                                _ => 2,
                            },
                            seed: u32::from(variation) + 1,
                        }],
                        ..ElementVectorEffects::default()
                    });
                }
                5 => {
                    let hatch_thickness = (second_style >> 3) & 3;
                    let space = (second_style >> 5) & 3;
                    let rotate = (second_style >> 7) & 3;
                    let jiggle = (second_style >> 9) & 3;
                    let curve = (second_style >> 11) & 3;
                    let length = (second_style >> 13) & 3;
                    output.vector_effects.push(ElementVectorEffects {
                        element_id: elem_id.clone(),
                        procedural_effects: vec![ProceduralVectorEffect::Roughen {
                            size: match length {
                                0 => 1.5,
                                1 => 2.5,
                                2 => 4.0,
                                _ => 6.0,
                            },
                            detail: match space {
                                0 => 8,
                                1 => 6,
                                2 => 4,
                                _ => 2,
                            },
                            seed: u32::from(hatch_thickness)
                                | (u32::from(rotate) << 2)
                                | (u32::from(jiggle) << 4)
                                | (u32::from(curve) << 6),
                        }],
                        ..ElementVectorEffects::default()
                    });
                }
                _ => {}
            }
            output.elements.push(Element {
                id: elem_id,
                name: elem_name,
                transform: effective_transform,
                kind: ElementKind::PaintedPath {
                    commands: chunk_commands,
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: stroke_color,
                    fill_gradient: None,
                    stroke_gradient: effective_grad.clone(),
                    stroke_width: effective_width,
                },
                scripts: vec![],
                accessibility: None,
                ..Element::default()
            });
        }
    }
    if !notes.is_empty() {
        notes.dedup();
        report.warning(
            "fla_shape_approximated",
            location,
            format!("Shape imported with approximations: {}. The original shape remains in the embedded source.", notes.join("; ")),
        );
    }
    if !shape.auxiliary.is_empty() {
        report.warning("fla_cubic_editing_metadata",location,"Rendered quadratic edges are preserved; auxiliary cubic editing handles remain in source provenance.")
    }
    Ok(output)
}

pub(super) fn try_convert_primitive(
    class: TimelineClass,
    prim: &TimelinePrimitive,
    shape: &BinaryShapeRecord,
) -> Option<(Transform, [f64; 2], ElementKind)> {
    if !shape.auxiliary.is_empty() {
        return None;
    }
    let mut fill_indices = BTreeSet::new();
    let mut stroke_indices = BTreeSet::new();
    for e in &shape.edges {
        let f0 = e.styles.fill_0 & 0x7fff;
        let f1 = e.styles.fill_1 & 0x7fff;
        let st = e.styles.stroke & 0x7fff;
        if f0 != 0 {
            fill_indices.insert(f0);
        }
        if f1 != 0 {
            fill_indices.insert(f1);
        }
        if st != 0 {
            stroke_indices.insert(st);
        }
    }
    if fill_indices.len() > 1 || stroke_indices.len() > 1 {
        return None;
    }
    let fill = if let Some(&idx) = fill_indices.first() {
        let f = shape.fills.get((idx - 1) as usize)?;
        match &f.encoding {
            BinaryFillEncoding::Legacy { word: 0 } => color(f.packed_value),
            BinaryFillEncoding::Extended {
                flags,
                payload: BinaryFillPayload::Plain,
                ..
            } if flags & 0x70 == 0 => color(f.packed_value),
            _ => return None,
        }
    } else {
        [0, 0, 0, 0]
    };
    let (stroke, stroke_width) = if let Some(&idx) = stroke_indices.first() {
        let st = shape.strokes.get((idx - 1) as usize)?;
        if st.style_words[0] != 0 || (st.style_words[1] & 7) != 0 {
            return None;
        }
        let scolor = if let Some(ext) = &st.extension {
            match &ext.paint.encoding {
                BinaryFillEncoding::Legacy { word: 0 } => color(ext.paint.packed_value),
                BinaryFillEncoding::Extended {
                    flags,
                    payload: BinaryFillPayload::Plain,
                    ..
                } if flags & 0x70 == 0 => color(ext.paint.packed_value),
                _ => return None,
            }
        } else {
            color(st.color_word)
        };
        let w = f64::from(st.width_word) / 20.0;
        (scolor, if w == 0.0 { 1.0 } else { w.clamp(0.0, 1e9) })
    } else {
        ([0, 0, 0, 0], 0.0)
    };
    let [xmin, ymin, xmax, ymax] = if class == TimelineClass::Oval {
        let [xmin_tw, xmax_tw, ymin_tw, ymax_tw] = prim.bounds_words?;
        if xmin_tw == i32::MIN {
            return None;
        }
        [
            f64::from(xmin_tw) / 20.0,
            f64::from(ymin_tw) / 20.0,
            f64::from(xmax_tw) / 20.0,
            f64::from(ymax_tw) / 20.0,
        ]
    } else {
        if shape.edges.is_empty() {
            return None;
        }
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for e in &shape.edges {
            for axis in 0..2 {
                let p = f64::from(e.start[axis]) / 5120.0;
                let q = f64::from(e.end[axis]) / 5120.0;
                min[axis] = min[axis].min(p.min(q));
                max[axis] = max[axis].max(p.max(q));
                if let Some(ctrl) = e.control {
                    let c = f64::from(ctrl[axis]) / 5120.0;
                    let denom = p - 2.0 * c + q;
                    if denom.abs() > 1e-12 {
                        let t = (p - c) / denom;
                        if t > 0.0 && t < 1.0 {
                            let u = 1.0 - t;
                            let v = u * u * p + 2.0 * u * t * c + t * t * q;
                            min[axis] = min[axis].min(v);
                            max[axis] = max[axis].max(v);
                        }
                    }
                }
            }
        }
        [min[0], min[1], max[0], max[1]]
    };
    if ![xmin, ymin, xmax, ymax].iter().all(|v| v.is_finite()) || xmax < xmin || ymax < ymin {
        return None;
    }
    let m = matrix(shape.matrix_words);
    let mut transform = Transform::from_matrix(m)?;
    transform.x += m[0] * xmin + m[2] * ymin;
    transform.y += m[1] * xmin + m[3] * ymin;
    if ![transform.x, transform.y]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1e12)
    {
        return None;
    }
    let width = (xmax - xmin).clamp(0.0, 1e9);
    let height = (ymax - ymin).clamp(0.0, 1e9);
    let kind = if class == TimelineClass::Oval {
        let start_raw = f64::from(prim.start_angle_degrees?);
        let end_raw = f64::from(prim.end_angle_degrees?);
        let inner_raw = f64::from(prim.inner_radius_percent?);
        if ![start_raw, end_raw, inner_raw]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        let norm_angle = |a: f64| {
            if (a - 360.0).abs() <= 1e-6 {
                360.0
            } else {
                a.rem_euclid(360.0)
            }
        };
        let opts = OvalOptions {
            start_angle: norm_angle(start_raw),
            end_angle: norm_angle(end_raw),
            inner_radius: inner_raw.clamp(0.0, 99.0),
            close_path: (prim.flags.unwrap_or(1) & 1) != 0,
        };
        if !opts.is_default() {
            transform.primitive = Some(PrimitiveOptions::Oval(opts));
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        }
    } else {
        let radii = prim.corner_radii?;
        let corner_radii = radii.map(|r| (f64::from(r) / 20.0).clamp(0.0, 1e9));
        let opts = RectangleOptions {
            corner_radii,
            corners_locked: (prim.flags.unwrap_or(1) & 1) != 0,
        };
        if !opts.is_default() {
            transform.primitive = Some(PrimitiveOptions::Rectangle(opts));
        }
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        }
    };
    Some((transform, [xmin, ymin], kind))
}
