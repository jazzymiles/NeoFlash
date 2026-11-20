use super::Anchor;
use super::handles_recognition::{
    RecognisedShape, map_commands_forward, recognise_primitive_shape, recognise_straight_line,
    simplify,
};
use crate::stage_mapping::StageMapping;
use eframe::egui::{Pos2, Vec2};
use neoflash_core::*;

fn contour_anchor_points(
    commands: &[PathCommand],
    start: usize,
    body_end: usize,
    closed: bool,
) -> Vec<Pos2> {
    let mut pts = Vec::new();
    let Some(PathCommand::MoveTo { x: x0, y: y0 }) = commands.get(start).copied() else {
        return pts;
    };
    pts.push(Pos2::new(x0 as f32, y0 as f32));
    for cmd in &commands[(start + 1)..body_end] {
        match *cmd {
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                pts.push(Pos2::new(x as f32, y as f32));
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => {}
        }
    }
    if closed && pts.len() >= 2 && pts.first().unwrap().distance(*pts.last().unwrap()) <= 1e-3 {
        pts.pop();
    }
    pts
}

fn contour_dense_samples(commands: &[PathCommand], start: usize, body_end: usize) -> Vec<Pos2> {
    let mut pts = Vec::new();
    let Some(PathCommand::MoveTo { x: x0, y: y0 }) = commands.get(start).copied() else {
        return pts;
    };
    let mut cur = [x0, y0];
    pts.push(Pos2::new(x0 as f32, y0 as f32));
    for cmd in &commands[(start + 1)..body_end] {
        match *cmd {
            PathCommand::LineTo { x, y } => {
                pts.push(Pos2::new(x as f32, y as f32));
                cur = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                for step in 1..=4 {
                    let t = f64::from(step) / 4.0;
                    let mt = 1.0 - t;
                    let px = mt * mt * cur[0] + 2.0 * mt * t * cx + t * t * x;
                    let py = mt * mt * cur[1] + 2.0 * mt * t * cy + t * t * y;
                    pts.push(Pos2::new(px as f32, py as f32));
                }
                cur = [x, y];
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                for step in 1..=4 {
                    let t = f64::from(step) / 4.0;
                    let mt = 1.0 - t;
                    let px = mt * mt * mt * cur[0]
                        + 3.0 * mt * mt * t * cx1
                        + 3.0 * mt * t * t * cx2
                        + t * t * t * x;
                    let py = mt * mt * mt * cur[1]
                        + 3.0 * mt * mt * t * cy1
                        + 3.0 * mt * t * t * cy2
                        + t * t * t * y;
                    pts.push(Pos2::new(px as f32, py as f32));
                }
                cur = [x, y];
            }
            PathCommand::MoveTo { .. } | PathCommand::Close => {}
        }
    }
    pts
}

fn rdp_simplify_points(points: &[Pos2], tolerance: f32) -> Vec<Pos2> {
    let anchors: Vec<Anchor> = points
        .iter()
        .map(|&point| Anchor {
            point,
            tangent: Vec2::ZERO,
        })
        .collect();
    simplify(&anchors, tolerance)
        .into_iter()
        .map(|a| a.point)
        .collect()
}

fn open_catmull_rom_to_cubic(pts: &[Pos2], tension: f32) -> Vec<PathCommand> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let k = tension / 6.0;
    let mut out = Vec::with_capacity(n);
    out.push(PathCommand::MoveTo {
        x: f64::from(pts[0].x),
        y: f64::from(pts[0].y),
    });
    for i in 0..(n - 1) {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < n { pts[i + 2] } else { pts[n - 1] };
        let c1 = p1 + (p2 - p0) * k;
        let c2 = p2 - (p3 - p1) * k;
        out.push(PathCommand::CubicTo {
            cx1: f64::from(c1.x),
            cy1: f64::from(c1.y),
            cx2: f64::from(c2.x),
            cy2: f64::from(c2.y),
            x: f64::from(p2.x),
            y: f64::from(p2.y),
        });
    }
    out
}

fn closed_catmull_rom_to_cubic(pts: &[Pos2], tension: f32) -> Vec<PathCommand> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    let k = tension / 6.0;
    let mut out = Vec::with_capacity(n + 2);
    out.push(PathCommand::MoveTo {
        x: f64::from(pts[0].x),
        y: f64::from(pts[0].y),
    });
    for i in 0..n {
        let p0 = pts[(i + n - 1) % n];
        let p1 = pts[i];
        let p2 = pts[(i + 1) % n];
        let p3 = pts[(i + 2) % n];
        let c1 = p1 + (p2 - p0) * k;
        let c2 = p2 - (p3 - p1) * k;
        out.push(PathCommand::CubicTo {
            cx1: f64::from(c1.x),
            cy1: f64::from(c1.y),
            cx2: f64::from(c2.x),
            cy2: f64::from(c2.y),
            x: f64::from(p2.x),
            y: f64::from(p2.y),
        });
    }
    out.push(PathCommand::Close);
    out
}

pub(crate) fn smooth_path_commands(commands: &[PathCommand], zoom: f32) -> Vec<PathCommand> {
    let ranges = contour_ranges(commands);
    if ranges.is_empty() {
        return commands.to_vec();
    }
    let rdp_eps = (1.2 / zoom.max(0.25)).clamp(0.35, 2.5);
    let mut out = Vec::new();
    for (start, body_end, closed) in ranges {
        let mut pts = contour_anchor_points(commands, start, body_end, closed);
        if closed {
            if pts.len() >= 3 {
                if pts.len() > 8 {
                    let mut loop_pts = pts.clone();
                    let mid = loop_pts.len() / 2;
                    let first_half = rdp_simplify_points(&loop_pts[..=mid], rdp_eps);
                    loop_pts.push(pts[0]);
                    let second_half = rdp_simplify_points(&loop_pts[mid..], rdp_eps);
                    let mut combined = first_half;
                    if second_half.len() > 2 {
                        combined.extend_from_slice(&second_half[1..(second_half.len() - 1)]);
                    }
                    if combined.len() >= 3 {
                        pts = combined;
                    }
                }
                let n = pts.len();
                let smoothed: Vec<Pos2> = (0..n)
                    .map(|i| {
                        let prev = pts[(i + n - 1) % n];
                        let cur = pts[i];
                        let next = pts[(i + 1) % n];
                        Pos2::new(
                            0.2 * prev.x + 0.6 * cur.x + 0.2 * next.x,
                            0.2 * prev.y + 0.6 * cur.y + 0.2 * next.y,
                        )
                    })
                    .collect();
                out.extend(closed_catmull_rom_to_cubic(&smoothed, 1.0));
            } else {
                out.extend_from_slice(&commands[start..body_end]);
                if closed {
                    out.push(PathCommand::Close);
                }
            }
        } else if pts.len() >= 3 {
            if pts.len() > 8 {
                let simplified = rdp_simplify_points(&pts, rdp_eps);
                if simplified.len() >= 3 {
                    pts = simplified;
                }
            }
            let n = pts.len();
            let mut smoothed = pts.clone();
            for i in 1..(n - 1) {
                let prev = pts[i - 1];
                let cur = pts[i];
                let next = pts[i + 1];
                smoothed[i] = Pos2::new(
                    0.22 * prev.x + 0.56 * cur.x + 0.22 * next.x,
                    0.22 * prev.y + 0.56 * cur.y + 0.22 * next.y,
                );
            }
            out.extend(open_catmull_rom_to_cubic(&smoothed, 1.0));
        } else if body_end == start + 2 {
            // Single 2-point curve: relax control points gently toward the chord.
            let p0 = [f64::from(pts[0].x), f64::from(pts[0].y)];
            out.push(commands[start]);
            match commands[start + 1] {
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let mid_x = 0.5 * (p0[0] + x);
                    let mid_y = 0.5 * (p0[1] + y);
                    out.push(PathCommand::QuadraticTo {
                        cx: 0.75 * cx + 0.25 * mid_x,
                        cy: 0.75 * cy + 0.25 * mid_y,
                        x,
                        y,
                    });
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let l1x = p0[0] + (x - p0[0]) / 3.0;
                    let l1y = p0[1] + (y - p0[1]) / 3.0;
                    let l2x = p0[0] + 2.0 * (x - p0[0]) / 3.0;
                    let l2y = p0[1] + 2.0 * (y - p0[1]) / 3.0;
                    out.push(PathCommand::CubicTo {
                        cx1: 0.75 * cx1 + 0.25 * l1x,
                        cy1: 0.75 * cy1 + 0.25 * l1y,
                        cx2: 0.75 * cx2 + 0.25 * l2x,
                        cy2: 0.75 * cy2 + 0.25 * l2y,
                        x,
                        y,
                    });
                }
                other => out.push(other),
            }
        } else {
            out.extend_from_slice(&commands[start..body_end]);
        }
    }
    out
}

fn snap_near_axis_endpoint(from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let len = dx.hypot(dy);
    if len <= 1e-5 {
        return to;
    }
    // Snap segments within ~4 degrees of horizontal or vertical.
    let tan_limit = 0.07;
    if (dy.abs() / len) <= tan_limit {
        [to[0], from[1]]
    } else if (dx.abs() / len) <= tan_limit {
        [from[0], to[1]]
    } else {
        to
    }
}

pub(crate) fn straighten_path_commands(commands: &[PathCommand], zoom: f32) -> Vec<PathCommand> {
    use crate::workspace::Tolerance;
    let ranges = contour_ranges(commands);
    if ranges.is_empty() {
        return commands.to_vec();
    }
    let rdp_eps = (2.4 / zoom.max(0.25)).clamp(0.8, 6.0);
    let curve_flat_tol = f64::from((5.0 / zoom.max(0.25)).clamp(1.5, 12.0));
    let mut out = Vec::new();
    for (start, body_end, closed) in ranges {
        let samples = contour_dense_samples(commands, start, body_end);
        let sample_anchors: Vec<Anchor> = samples
            .iter()
            .map(|&point| Anchor {
                point,
                tangent: Vec2::ZERO,
            })
            .collect();
        if closed {
            if let Some(recognised) =
                recognise_primitive_shape(&sample_anchors, true, zoom, Tolerance::Normal)
            {
                match recognised {
                    RecognisedShape::Rectangle {
                        x,
                        y,
                        width,
                        height,
                    } => {
                        out.push(PathCommand::MoveTo { x, y });
                        out.push(PathCommand::LineTo { x: x + width, y });
                        out.push(PathCommand::LineTo {
                            x: x + width,
                            y: y + height,
                        });
                        out.push(PathCommand::LineTo { x, y: y + height });
                        out.push(PathCommand::Close);
                        continue;
                    }
                    RecognisedShape::Ellipse {
                        x,
                        y,
                        width,
                        height,
                    } => {
                        let rx = width * 0.5;
                        let ry = height * 0.5;
                        let cx = x + rx;
                        let cy = y + ry;
                        let k = 0.552_284_749_830_793_6;
                        let kx = rx * k;
                        let ky = ry * k;
                        out.push(PathCommand::MoveTo { x: cx + rx, y: cy });
                        out.push(PathCommand::CubicTo {
                            cx1: cx + rx,
                            cy1: cy + ky,
                            cx2: cx + kx,
                            cy2: cy + ry,
                            x: cx,
                            y: cy + ry,
                        });
                        out.push(PathCommand::CubicTo {
                            cx1: cx - kx,
                            cy1: cy + ry,
                            cx2: cx - rx,
                            cy2: cy + ky,
                            x: cx - rx,
                            y: cy,
                        });
                        out.push(PathCommand::CubicTo {
                            cx1: cx - rx,
                            cy1: cy - ky,
                            cx2: cx - kx,
                            cy2: cy - ry,
                            x: cx,
                            y: cy - ry,
                        });
                        out.push(PathCommand::CubicTo {
                            cx1: cx + kx,
                            cy1: cy - ry,
                            cx2: cx + rx,
                            cy2: cy - ky,
                            x: cx + rx,
                            y: cy,
                        });
                        out.push(PathCommand::Close);
                        continue;
                    }
                }
            }
        } else if let Some([s_pt, e_pt]) =
            recognise_straight_line(&sample_anchors, zoom, Tolerance::Normal)
        {
            let snapped_end = snap_near_axis_endpoint(
                [f64::from(s_pt.x), f64::from(s_pt.y)],
                [f64::from(e_pt.x), f64::from(e_pt.y)],
            );
            out.push(PathCommand::MoveTo {
                x: f64::from(s_pt.x),
                y: f64::from(s_pt.y),
            });
            out.push(PathCommand::LineTo {
                x: snapped_end[0],
                y: snapped_end[1],
            });
            continue;
        }

        // Stepwise straightening: flatten shallow curves / pull deeper curve control points
        // toward their chord, then simplify nearly-collinear LineTo runs.
        let has_curves = commands[(start + 1)..body_end].iter().any(|c| {
            matches!(
                c,
                PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. }
            )
        });
        if has_curves {
            let Some(PathCommand::MoveTo { x: x0, y: y0 }) = commands.get(start).copied() else {
                continue;
            };
            let mut cur = [x0, y0];
            out.push(PathCommand::MoveTo { x: x0, y: y0 });
            for cmd in &commands[(start + 1)..body_end] {
                match *cmd {
                    PathCommand::LineTo { x, y } => {
                        let p = snap_near_axis_endpoint(cur, [x, y]);
                        out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                        cur = p;
                    }
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        let mid = [0.5 * (cur[0] + x), 0.5 * (cur[1] + y)];
                        let dev = (cx - mid[0]).hypot(cy - mid[1]);
                        if dev <= curve_flat_tol {
                            let p = snap_near_axis_endpoint(cur, [x, y]);
                            out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                            cur = p;
                        } else {
                            out.push(PathCommand::QuadraticTo {
                                cx: 0.5 * cx + 0.5 * mid[0],
                                cy: 0.5 * cy + 0.5 * mid[1],
                                x,
                                y,
                            });
                            cur = [x, y];
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
                        let l1 = [cur[0] + (x - cur[0]) / 3.0, cur[1] + (y - cur[1]) / 3.0];
                        let l2 = [
                            cur[0] + 2.0 * (x - cur[0]) / 3.0,
                            cur[1] + 2.0 * (y - cur[1]) / 3.0,
                        ];
                        let dev = (cx1 - l1[0])
                            .hypot(cy1 - l1[1])
                            .max((cx2 - l2[0]).hypot(cy2 - l2[1]));
                        if dev <= curve_flat_tol {
                            let p = snap_near_axis_endpoint(cur, [x, y]);
                            out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                            cur = p;
                        } else {
                            out.push(PathCommand::CubicTo {
                                cx1: 0.5 * cx1 + 0.5 * l1[0],
                                cy1: 0.5 * cy1 + 0.5 * l1[1],
                                cx2: 0.5 * cx2 + 0.5 * l2[0],
                                cy2: 0.5 * cy2 + 0.5 * l2[1],
                                x,
                                y,
                            });
                            cur = [x, y];
                        }
                    }
                    PathCommand::MoveTo { .. } | PathCommand::Close => {}
                }
            }
            if closed {
                out.push(PathCommand::Close);
            }
        } else {
            let anchors = contour_anchor_points(commands, start, body_end, closed);
            if closed && anchors.len() >= 3 {
                let mut loop_pts = anchors.clone();
                loop_pts.push(anchors[0]);
                let simplified = rdp_simplify_points(&loop_pts, rdp_eps);
                let mut clean: Vec<Pos2> = if simplified.len() >= 4 {
                    simplified[..(simplified.len() - 1)].to_vec()
                } else {
                    anchors
                };
                out.push(PathCommand::MoveTo {
                    x: f64::from(clean[0].x),
                    y: f64::from(clean[0].y),
                });
                let mut cur = [f64::from(clean[0].x), f64::from(clean[0].y)];
                for pt in clean.drain(1..) {
                    let p = snap_near_axis_endpoint(cur, [f64::from(pt.x), f64::from(pt.y)]);
                    out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                    cur = p;
                }
                out.push(PathCommand::Close);
            } else if !closed && anchors.len() >= 2 {
                let simplified = rdp_simplify_points(&anchors, rdp_eps);
                let clean = if simplified.len() >= 2 {
                    simplified
                } else {
                    anchors
                };
                out.push(PathCommand::MoveTo {
                    x: f64::from(clean[0].x),
                    y: f64::from(clean[0].y),
                });
                let mut cur = [f64::from(clean[0].x), f64::from(clean[0].y)];
                for pt in &clean[1..] {
                    let p = snap_near_axis_endpoint(cur, [f64::from(pt.x), f64::from(pt.y)]);
                    out.push(PathCommand::LineTo { x: p[0], y: p[1] });
                    cur = p;
                }
            } else {
                out.extend_from_slice(&commands[start..body_end]);
                if closed {
                    out.push(PathCommand::Close);
                }
            }
        }
    }
    out
}

pub(crate) fn editable_element_commands(element: &Element) -> Option<Vec<PathCommand>> {
    element.to_path_commands().map(|(commands, _)| commands)
}

pub(crate) fn write_editable_element_commands(element: &mut Element, commands: Vec<PathCommand>) {
    match &mut element.kind {
        ElementKind::Path {
            commands: destination,
            ..
        }
        | ElementKind::PaintedPath {
            commands: destination,
            ..
        } => {
            *destination = commands;
        }
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            element.transform.primitive = None;
            element.kind = ElementKind::Path {
                commands,
                fill_rule: FillRule::NonZero,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            };
        }
        _ => {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReshapeVertexKind {
    Corner,
    SmoothCurve,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ReshapeAction {
    Corner {
        local_vertex: [f64; 2],
    },
    SmoothVertex {
        local_vertex: [f64; 2],
        break_corner: bool,
    },
    Segment {
        cmd_idx: usize,
        t: f64,
        local_point: [f64; 2],
        create_corner: bool,
    },
}

impl ReshapeAction {
    pub(crate) fn with_corner_modifier(self) -> Self {
        match self {
            Self::SmoothVertex { local_vertex, .. } => Self::SmoothVertex {
                local_vertex,
                break_corner: true,
            },
            Self::Segment {
                cmd_idx,
                t,
                local_point,
                ..
            } => Self::Segment {
                cmd_idx,
                t,
                local_point,
                create_corner: true,
            },
            other => other,
        }
    }
}

#[derive(Clone, Copy)]
struct ContourSegInfo {
    p0: [f64; 2],
    p1: [f64; 2],
    is_curve: bool,
    out_dir: [f64; 2],
    in_dir: [f64; 2],
}

pub(super) fn contour_ranges(commands: &[PathCommand]) -> Vec<(usize, usize, bool)> {
    let mut ranges = Vec::new();
    let mut current_start = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if matches!(cmd, PathCommand::MoveTo { .. }) {
            if let Some(start) = current_start {
                let closed = matches!(commands.get(idx - 1), Some(PathCommand::Close));
                let body_end = if closed { idx - 1 } else { idx };
                ranges.push((start, body_end, closed));
            }
            current_start = Some(idx);
        }
    }
    if let Some(start) = current_start {
        let end = commands.len();
        let closed = matches!(commands.get(end - 1), Some(PathCommand::Close));
        let body_end = if closed { end - 1 } else { end };
        ranges.push((start, body_end, closed));
    }
    ranges
}

pub(crate) fn classify_reshape_vertices(
    commands: &[PathCommand],
) -> Vec<([f64; 2], ReshapeVertexKind)> {
    let mut out = Vec::new();
    let junction_kind = |prev: &ContourSegInfo, next: &ContourSegInfo| -> ReshapeVertexKind {
        if !prev.is_curve || !next.is_curve {
            return ReshapeVertexKind::Corner;
        }
        let l1 = prev.in_dir[0].hypot(prev.in_dir[1]);
        let l2 = next.out_dir[0].hypot(next.out_dir[1]);
        if l1 <= 1e-9 || l2 <= 1e-9 {
            return ReshapeVertexKind::Corner;
        }
        let dot = (prev.in_dir[0] * next.out_dir[0] + prev.in_dir[1] * next.out_dir[1]) / (l1 * l2);
        if dot >= 0.965 {
            ReshapeVertexKind::SmoothCurve
        } else {
            ReshapeVertexKind::Corner
        }
    };

    for (start, body_end, closed) in contour_ranges(commands) {
        if body_end <= start + 1 {
            continue;
        }
        let Some(start_pt) = anchor_endpoint(&commands[start]) else {
            continue;
        };
        let mut p0 = start_pt;
        let mut segs = Vec::new();
        for cmd in &commands[(start + 1)..body_end] {
            match *cmd {
                PathCommand::LineTo { x, y } => {
                    let p1 = [x, y];
                    let d = [p1[0] - p0[0], p1[1] - p0[1]];
                    if d[0].hypot(d[1]) > 1e-6 {
                        segs.push(ContourSegInfo {
                            p0,
                            p1,
                            is_curve: false,
                            out_dir: d,
                            in_dir: d,
                        });
                    }
                    p0 = p1;
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let p1 = [x, y];
                    let out_dir = if (cx - p0[0]).hypot(cy - p0[1]) > 1e-6 {
                        [cx - p0[0], cy - p0[1]]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    let in_dir = if (x - cx).hypot(y - cy) > 1e-6 {
                        [x - cx, y - cy]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    segs.push(ContourSegInfo {
                        p0,
                        p1,
                        is_curve: true,
                        out_dir,
                        in_dir,
                    });
                    p0 = p1;
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let p1 = [x, y];
                    let out_dir = if (cx1 - p0[0]).hypot(cy1 - p0[1]) > 1e-6 {
                        [cx1 - p0[0], cy1 - p0[1]]
                    } else if (cx2 - p0[0]).hypot(cy2 - p0[1]) > 1e-6 {
                        [cx2 - p0[0], cy2 - p0[1]]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    let in_dir = if (x - cx2).hypot(y - cy2) > 1e-6 {
                        [x - cx2, y - cy2]
                    } else if (x - cx1).hypot(y - cy1) > 1e-6 {
                        [x - cx1, y - cy1]
                    } else {
                        [x - p0[0], y - p0[1]]
                    };
                    segs.push(ContourSegInfo {
                        p0,
                        p1,
                        is_curve: true,
                        out_dir,
                        in_dir,
                    });
                    p0 = p1;
                }
                _ => {}
            }
        }
        if closed && (p0[0] - start_pt[0]).hypot(p0[1] - start_pt[1]) > 1e-6 {
            let d = [start_pt[0] - p0[0], start_pt[1] - p0[1]];
            segs.push(ContourSegInfo {
                p0,
                p1: start_pt,
                is_curve: false,
                out_dir: d,
                in_dir: d,
            });
        }
        if segs.is_empty() {
            continue;
        }
        let effectively_closed = closed
            || (segs.len() >= 2
                && (segs.last().unwrap().p1[0] - start_pt[0])
                    .hypot(segs.last().unwrap().p1[1] - start_pt[1])
                    <= 1e-4);
        let first_kind = if effectively_closed {
            junction_kind(segs.last().unwrap(), &segs[0])
        } else {
            ReshapeVertexKind::Corner
        };
        out.push((segs[0].p0, first_kind));
        for i in 0..(segs.len() - 1) {
            out.push((segs[i].p1, junction_kind(&segs[i], &segs[i + 1])));
        }
        if !effectively_closed {
            out.push((segs.last().unwrap().p1, ReshapeVertexKind::Corner));
        }
    }
    out
}

fn eval_command_segment_at(commands: &[PathCommand], cmd_idx: usize, t: f64) -> Option<[f64; 2]> {
    let t = t.clamp(0.0, 1.0);
    let mut subpath_start: Option<[f64; 2]> = None;
    let mut cursor: Option<[f64; 2]> = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if idx == cmd_idx {
            let p0 = cursor?;
            return match *cmd {
                PathCommand::MoveTo { .. } => None,
                PathCommand::LineTo { x, y } => {
                    Some([p0[0] + (x - p0[0]) * t, p0[1] + (y - p0[1]) * t])
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let u = 1.0 - t;
                    Some([
                        u * u * p0[0] + 2.0 * u * t * cx + t * t * x,
                        u * u * p0[1] + 2.0 * u * t * cy + t * t * y,
                    ])
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    let u = 1.0 - t;
                    let (u2, t2) = (u * u, t * t);
                    Some([
                        u2 * u * p0[0] + 3.0 * u2 * t * cx1 + 3.0 * u * t2 * cx2 + t2 * t * x,
                        u2 * u * p0[1] + 3.0 * u2 * t * cy1 + 3.0 * u * t2 * cy2 + t2 * t * y,
                    ])
                }
                PathCommand::Close => {
                    let start = subpath_start?;
                    Some([
                        p0[0] + (start[0] - p0[0]) * t,
                        p0[1] + (start[1] - p0[1]) * t,
                    ])
                }
            };
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = Some([x, y]);
                cursor = Some([x, y]);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                cursor = Some([x, y]);
            }
            PathCommand::Close => {
                cursor = subpath_start;
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn hit_reshape_on_commands(
    commands: &[PathCommand],
    stroke_width: f64,
    matrix: [f64; 6],
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
    pos: Pos2,
    alt: bool,
) -> Option<(ReshapeAction, neoflash_ui::cursors::Cursor, Pos2)> {
    let vertices = classify_reshape_vertices(commands);
    let mut best_vertex: Option<(f32, [f64; 2], ReshapeVertexKind, Pos2)> = None;
    for (local_v, kind) in vertices {
        let edit_pt = crate::stage::affine(matrix, Pos2::new(local_v[0] as f32, local_v[1] as f32));
        let screen_pt = mapping.edit_to_screen(origin, zoom, edit_pt);
        let dist = screen_pt.distance(pos);
        if dist <= 8.0 && best_vertex.is_none_or(|(best_d, ..)| dist < best_d) {
            best_vertex = Some((dist, local_v, kind, edit_pt));
        }
    }
    if let Some((_, local_vertex, kind, edit_pt)) = best_vertex {
        return match kind {
            ReshapeVertexKind::Corner => Some((
                ReshapeAction::Corner { local_vertex },
                neoflash_ui::cursors::ARROW_RESHAPE_CORNER,
                edit_pt,
            )),
            ReshapeVertexKind::SmoothCurve => {
                let cursor = if alt {
                    neoflash_ui::cursors::ARROW_RESHAPE_CORNER
                } else {
                    neoflash_ui::cursors::ARROW_RESHAPE_CURVE
                };
                Some((
                    ReshapeAction::SmoothVertex {
                        local_vertex,
                        break_corner: alt,
                    },
                    cursor,
                    edit_pt,
                ))
            }
        };
    }

    let stage_pt = Pos2::ZERO + (pos - origin) / zoom.max(1e-3);
    let composite = mapping.map_matrix(matrix);
    let stage_commands = map_commands_forward(commands, composite);
    let scale = composite[0]
        .hypot(composite[1])
        .max(composite[2].hypot(composite[3]));
    let stroke_tol = if stroke_width > 0.0 && scale.is_finite() {
        0.5 * stroke_width * scale + 2.0 / f64::from(zoom.max(1e-3))
    } else {
        0.0
    };
    let tolerance = (7.0 / f64::from(zoom.max(1e-3))).max(stroke_tol);
    let (cmd_idx, t, _closest, dist) = nearest_path_segment(
        &stage_commands,
        [f64::from(stage_pt.x), f64::from(stage_pt.y)],
    )?;
    if dist > tolerance {
        return None;
    }
    let local_point = eval_command_segment_at(commands, cmd_idx, t)?;
    let edit_pt = crate::stage::affine(
        matrix,
        Pos2::new(local_point[0] as f32, local_point[1] as f32),
    );
    let cursor = if alt {
        neoflash_ui::cursors::ARROW_RESHAPE_CORNER
    } else {
        neoflash_ui::cursors::ARROW_RESHAPE_CURVE
    };
    Some((
        ReshapeAction::Segment {
            cmd_idx,
            t,
            local_point,
            create_corner: alt,
        },
        cursor,
        edit_pt,
    ))
}

pub(crate) fn apply_reshape_action(
    commands: &[PathCommand],
    action: ReshapeAction,
    local_delta: [f64; 2],
) -> Option<Vec<PathCommand>> {
    if !local_delta[0].is_finite() || !local_delta[1].is_finite() {
        return None;
    }
    if local_delta[0].hypot(local_delta[1]) <= 1e-9 {
        return Some(commands.to_vec());
    }
    match action {
        ReshapeAction::Corner { local_vertex } => Some(move_reshape_vertex(
            commands,
            local_vertex,
            local_delta,
            false,
        )),
        ReshapeAction::SmoothVertex {
            local_vertex,
            break_corner,
        } => Some(move_reshape_vertex(
            commands,
            local_vertex,
            local_delta,
            break_corner,
        )),
        ReshapeAction::Segment {
            cmd_idx,
            t,
            local_point,
            create_corner,
        } => reshape_segment_at(
            commands,
            cmd_idx,
            t,
            local_point,
            local_delta,
            create_corner,
        ),
    }
}

fn move_reshape_vertex(
    commands: &[PathCommand],
    local_vertex: [f64; 2],
    delta: [f64; 2],
    break_corner: bool,
) -> Vec<PathCommand> {
    let [dx, dy] = delta;
    let moved_target = [local_vertex[0] + dx, local_vertex[1] + dy];
    let near_vertex =
        |pt: [f64; 2]| (pt[0] - local_vertex[0]).hypot(pt[1] - local_vertex[1]) <= 1e-4;
    let mut out = commands.to_vec();

    for (start, body_end, closed) in contour_ranges(commands) {
        if body_end <= start {
            continue;
        }
        let mut prev_moved = anchor_endpoint(&commands[start]).is_some_and(near_vertex);
        if prev_moved && let Some(PathCommand::MoveTo { x, y }) = out.get_mut(start) {
            *x += dx;
            *y += dy;
        }
        let first_moved = prev_moved;
        for idx in (start + 1)..body_end {
            let cur_moved = anchor_endpoint(&commands[idx]).is_some_and(near_vertex);
            let p0 = anchor_endpoint(&commands[idx - 1]).unwrap_or(local_vertex);
            let p1 = anchor_endpoint(&commands[idx]).unwrap_or(local_vertex);
            match out.get_mut(idx) {
                Some(PathCommand::LineTo { x, y }) => {
                    if cur_moved {
                        *x += dx;
                        *y += dy;
                    }
                }
                Some(PathCommand::QuadraticTo { cx, cy, x, y }) => {
                    if cur_moved {
                        *x += dx;
                        *y += dy;
                    }
                    if break_corner && (prev_moved || cur_moved) {
                        let a = if prev_moved { moved_target } else { p0 };
                        let b = if cur_moved { moved_target } else { p1 };
                        *cx = 0.5 * (a[0] + b[0]);
                        *cy = 0.5 * (a[1] + b[1]);
                    } else if prev_moved && cur_moved {
                        *cx += dx;
                        *cy += dy;
                    } else if prev_moved || cur_moved {
                        *cx += 0.5 * dx;
                        *cy += 0.5 * dy;
                    }
                }
                Some(PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }) => {
                    if prev_moved {
                        if break_corner {
                            *cx1 = moved_target[0];
                            *cy1 = moved_target[1];
                        } else {
                            *cx1 += dx;
                            *cy1 += dy;
                        }
                    }
                    if cur_moved {
                        if break_corner {
                            *cx2 = moved_target[0];
                            *cy2 = moved_target[1];
                        } else {
                            *cx2 += dx;
                            *cy2 += dy;
                        }
                        *x += dx;
                        *y += dy;
                    }
                }
                _ => {}
            }
            prev_moved = cur_moved;
        }
        let _ = (closed, first_moved);
    }
    out
}

fn reshape_segment_at(
    commands: &[PathCommand],
    cmd_idx: usize,
    t: f64,
    local_point: [f64; 2],
    delta: [f64; 2],
    create_corner: bool,
) -> Option<Vec<PathCommand>> {
    if cmd_idx >= commands.len() {
        return None;
    }
    let mut subpath_start: Option<[f64; 2]> = None;
    let mut cursor: Option<[f64; 2]> = None;
    let mut p0_at_cmd = None;
    let mut start_at_cmd = None;
    for (idx, cmd) in commands.iter().enumerate() {
        if idx == cmd_idx {
            p0_at_cmd = cursor;
            start_at_cmd = subpath_start;
            break;
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                subpath_start = Some([x, y]);
                cursor = Some([x, y]);
            }
            PathCommand::LineTo { x, y }
            | PathCommand::QuadraticTo { x, y, .. }
            | PathCommand::CubicTo { x, y, .. } => {
                cursor = Some([x, y]);
            }
            PathCommand::Close => {
                cursor = subpath_start;
            }
        }
    }
    let p0 = p0_at_cmd?;
    let corner = [local_point[0] + delta[0], local_point[1] + delta[1]];

    if create_corner {
        match commands[cmd_idx] {
            PathCommand::MoveTo { .. } => return None,
            PathCommand::LineTo { x, y } => {
                let mut out = Vec::with_capacity(commands.len() + 1);
                out.extend_from_slice(&commands[..cmd_idx]);
                out.push(PathCommand::LineTo {
                    x: corner[0],
                    y: corner[1],
                });
                out.push(PathCommand::LineTo { x, y });
                out.extend_from_slice(&commands[(cmd_idx + 1)..]);
                return Some(out);
            }
            PathCommand::Close => {
                let mut out = Vec::with_capacity(commands.len() + 1);
                out.extend_from_slice(&commands[..cmd_idx]);
                out.push(PathCommand::LineTo {
                    x: corner[0],
                    y: corner[1],
                });
                out.push(PathCommand::Close);
                out.extend_from_slice(&commands[(cmd_idx + 1)..]);
                return Some(out);
            }
            PathCommand::QuadraticTo { .. } | PathCommand::CubicTo { .. } => {
                let (mut split, inserted) = subdivide_path_at(commands, cmd_idx, t)?;
                let p_end = anchor_endpoint(split.get(inserted + 1)?)?;
                match split.get_mut(inserted)? {
                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                        *x = corner[0];
                        *y = corner[1];
                        *cx = 0.5 * (p0[0] + corner[0]);
                        *cy = 0.5 * (p0[1] + corner[1]);
                    }
                    PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
                        *x = corner[0];
                        *y = corner[1];
                        *cx2 = corner[0];
                        *cy2 = corner[1];
                    }
                    _ => {}
                }
                match split.get_mut(inserted + 1)? {
                    PathCommand::QuadraticTo { cx, cy, .. } => {
                        *cx = 0.5 * (corner[0] + p_end[0]);
                        *cy = 0.5 * (corner[1] + p_end[1]);
                    }
                    PathCommand::CubicTo { cx1, cy1, .. } => {
                        *cx1 = corner[0];
                        *cy1 = corner[1];
                    }
                    _ => {}
                }
                return Some(split);
            }
        }
    }

    let tau = t.clamp(0.15, 0.85);
    let quad_weight = (2.0 * (1.0 - tau) * tau).max(1e-4);
    let cubic_weight = (3.0 * (1.0 - tau) * tau).max(1e-4);
    let mut out = Vec::with_capacity(commands.len() + 1);
    out.extend_from_slice(&commands[..cmd_idx]);
    match commands[cmd_idx] {
        PathCommand::MoveTo { .. } => return None,
        PathCommand::LineTo { x, y } => {
            let cx = 0.5 * (p0[0] + x) + delta[0] / quad_weight;
            let cy = 0.5 * (p0[1] + y) + delta[1] / quad_weight;
            out.push(PathCommand::QuadraticTo { cx, cy, x, y });
        }
        PathCommand::Close => {
            let start = start_at_cmd?;
            let cx = 0.5 * (p0[0] + start[0]) + delta[0] / quad_weight;
            let cy = 0.5 * (p0[1] + start[1]) + delta[1] / quad_weight;
            out.push(PathCommand::QuadraticTo {
                cx,
                cy,
                x: start[0],
                y: start[1],
            });
            out.push(PathCommand::Close);
        }
        PathCommand::QuadraticTo { cx, cy, x, y } => {
            out.push(PathCommand::QuadraticTo {
                cx: cx + delta[0] / quad_weight,
                cy: cy + delta[1] / quad_weight,
                x,
                y,
            });
        }
        PathCommand::CubicTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        } => {
            out.push(PathCommand::CubicTo {
                cx1: cx1 + delta[0] / cubic_weight,
                cy1: cy1 + delta[1] / cubic_weight,
                cx2: cx2 + delta[0] / cubic_weight,
                cy2: cy2 + delta[1] / cubic_weight,
                x,
                y,
            });
        }
    }
    out.extend_from_slice(&commands[(cmd_idx + 1)..]);
    Some(out)
}

pub(crate) fn path_segment_hit_local(
    commands: &[PathCommand],
    matrix: [f64; 6],
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
    pos: Pos2,
) -> Option<(usize, f64)> {
    let stage_pt = Pos2::ZERO + (pos - origin) / zoom.max(1e-3);
    let composite = mapping.map_matrix(matrix);
    let stage_commands = map_commands_forward(commands, composite);
    let tolerance = 7.0 / f64::from(zoom.max(1e-3));
    let (cmd_idx, t, _closest, dist) = nearest_path_segment(
        &stage_commands,
        [f64::from(stage_pt.x), f64::from(stage_pt.y)],
    )?;
    (dist <= tolerance).then_some((cmd_idx, t))
}

pub(super) fn anchor_endpoint(cmd: &PathCommand) -> Option<[f64; 2]> {
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
