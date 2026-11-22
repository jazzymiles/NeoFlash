//! SWF vector shape, gradient, bitmap fill, and morph-shape encoders.

use super::*;

enum MorphFill<'a> {
    Solid,
    Gradient(&'a GradientPaint, &'a GradientPaint),
}

fn morph_fill<'a>(start: &'a Element, end: &'a Element) -> Option<MorphFill<'a>> {
    fn compatible_fill(el: &Element) -> Option<Option<&GradientPaint>> {
        match &el.kind {
            ElementKind::PaintedPath {
                stroke_gradient: Some(_),
                ..
            } => None,
            ElementKind::PaintedPath { fill_gradient, .. } => Some(fill_gradient.as_ref()),
            _ => Some(None),
        }
    }
    match (compatible_fill(start)?, compatible_fill(end)?) {
        (None, None) => Some(MorphFill::Solid),
        (Some(a), Some(b))
            if a.kind == b.kind
                && (1..=15).contains(&a.stops.len())
                && a.stops.len() == b.stops.len()
                && a.spread == b.spread
                && a.linear_rgb == b.linear_rgb
                && [a, b].iter().all(|g| {
                    g.focal_point_ratio.is_finite()
                        && g.stops.windows(2).all(|w| w[0].ratio <= w[1].ratio)
                }) =>
        {
            Some(MorphFill::Gradient(a, b))
        }
        _ => None,
    }
}

pub(super) fn can_morph_keys(
    start: &neoflash_core::Keyframe,
    end: &neoflash_core::Keyframe,
) -> bool {
    prepare_morph_keys(start, end).is_some()
}

pub(super) fn element_vector_commands(el: &Element) -> Option<Vec<PathCommand>> {
    el.to_path_commands().map(|(commands, _)| commands)
}

fn open_oval(el: &Element) -> bool {
    matches!(el.kind, ElementKind::Ellipse { .. }) && !el.oval_options().close_path
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SwfBitmapBrush {
    pub(super) character: u16,
    pub(super) matrix: [f64; 6],
    pub(super) repeat: bool,
    pub(super) smoothing: bool,
}

#[derive(Clone, Debug)]
pub(super) struct SwfShapeFillStyle {
    pub(super) bitmap: Option<SwfBitmapBrush>,
    pub(super) color: Color,
    pub(super) gradient: Option<GradientPaint>,
}

#[derive(Clone, Debug)]
pub(super) struct SwfShapeLineStyle {
    pub(super) width_twips: u16,
    pub(super) color: Color,
    pub(super) gradient: Option<GradientPaint>,
    pub(super) stroke_cap: StrokeCap,
    pub(super) stroke_join: StrokeJoin,
    pub(super) miter_limit: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SwfEdge {
    pub(super) fill_1: u16,
    pub(super) line: u16,
    pub(super) start: [i32; 2],
    pub(super) control: Option<[i32; 2]>,
    pub(super) end: [i32; 2],
}

pub(super) fn px_to_twips(v: f64) -> i32 {
    if v.is_finite() {
        (v * 20.0).round().clamp(-1_000_000.0, 1_000_000.0) as i32
    } else {
        0
    }
}

pub(super) type SwfEdgeSeg = ([i32; 2], Option<[i32; 2]>, [i32; 2]);
pub(super) type SwfSubpath = (Vec<SwfEdgeSeg>, bool);

pub(super) fn convert_commands_to_swf_edges(
    commands: &[PathCommand],
    fill_idx: u16,
    line_idx: u16,
    force_close: bool,
    out_edges: &mut Vec<SwfEdge>,
) {
    let mut subpaths: Vec<SwfSubpath> = Vec::new();
    let mut cur = [0.0_f64, 0.0_f64];
    let mut start = [0.0_f64, 0.0_f64];
    let mut has_start = false;
    let mut segs: Vec<SwfEdgeSeg> = Vec::new();

    let push_seg =
        |segs: &mut Vec<SwfEdgeSeg>, p0: [f64; 2], ctrl: Option<[f64; 2]>, p1: [f64; 2]| {
            let s = [px_to_twips(p0[0]), px_to_twips(p0[1])];
            let e = [px_to_twips(p1[0]), px_to_twips(p1[1])];
            let c = ctrl.map(|cp| [px_to_twips(cp[0]), px_to_twips(cp[1])]);
            if s == e && c.is_none() {
                return;
            }
            segs.push((s, c, e));
        };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if !segs.is_empty() {
                    subpaths.push((std::mem::take(&mut segs), false));
                }
                cur = [x, y];
                start = [x, y];
                has_start = true;
            }
            PathCommand::LineTo { x, y } => {
                if !has_start {
                    start = cur;
                    has_start = true;
                }
                push_seg(&mut segs, cur, None, [x, y]);
                cur = [x, y];
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if !has_start {
                    start = cur;
                    has_start = true;
                }
                push_seg(&mut segs, cur, Some([cx, cy]), [x, y]);
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
                if !has_start {
                    start = cur;
                    has_start = true;
                }
                let p0 = cur;
                let p1 = [cx1, cy1];
                let p2 = [cx2, cy2];
                let p3 = [x, y];
                let q0 = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
                let q1 = [0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])];
                let q2 = [0.5 * (p2[0] + p3[0]), 0.5 * (p2[1] + p3[1])];
                let r0 = [0.5 * (q0[0] + q1[0]), 0.5 * (q0[1] + q1[1])];
                let r1 = [0.5 * (q1[0] + q2[0]), 0.5 * (q1[1] + q2[1])];
                let mid = [0.5 * (r0[0] + r1[0]), 0.5 * (r0[1] + r1[1])];
                push_seg(&mut segs, p0, Some(q0), mid);
                push_seg(&mut segs, mid, Some(q2), p3);
                cur = p3;
            }
            PathCommand::Close => {
                if has_start {
                    push_seg(&mut segs, cur, None, start);
                    cur = start;
                }
                if !segs.is_empty() {
                    subpaths.push((std::mem::take(&mut segs), true));
                }
            }
        }
    }
    if !segs.is_empty() {
        subpaths.push((segs, false));
    }

    for (mut sp, closed) in subpaths {
        if sp.is_empty() {
            continue;
        }
        let first_pt = sp[0].0;
        let last_pt = sp.last().unwrap().2;
        if (fill_idx != 0 || closed || force_close) && first_pt != last_pt {
            sp.push((last_pt, None, first_pt));
        }
        for (s, c, e) in sp {
            out_edges.push(SwfEdge {
                fill_1: fill_idx,
                line: line_idx,
                start: s,
                control: c,
                end: e,
            });
        }
    }
}

fn swf_srgb_u8_to_linear_255(c: u8) -> f64 {
    let s = f64::from(c) / 255.0;
    let lin = if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    };
    lin * 255.0
}

fn swf_linear_255_to_srgb_u8(v: f64) -> u8 {
    let l = (v / 255.0).clamp(0.0, 1.0);
    let s = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

fn swf_color_to_interp_space(c: Color, linear_rgb: bool) -> [f64; 4] {
    if linear_rgb {
        [
            swf_srgb_u8_to_linear_255(c[0]),
            swf_srgb_u8_to_linear_255(c[1]),
            swf_srgb_u8_to_linear_255(c[2]),
            f64::from(c[3]),
        ]
    } else {
        [
            f64::from(c[0]),
            f64::from(c[1]),
            f64::from(c[2]),
            f64::from(c[3]),
        ]
    }
}

fn swf_color_from_interp_space(v: [f64; 4], linear_rgb: bool) -> Color {
    if linear_rgb {
        [
            swf_linear_255_to_srgb_u8(v[0]),
            swf_linear_255_to_srgb_u8(v[1]),
            swf_linear_255_to_srgb_u8(v[2]),
            v[3].round().clamp(0.0, 255.0) as u8,
        ]
    } else {
        [
            v[0].round().clamp(0.0, 255.0) as u8,
            v[1].round().clamp(0.0, 255.0) as u8,
            v[2].round().clamp(0.0, 255.0) as u8,
            v[3].round().clamp(0.0, 255.0) as u8,
        ]
    }
}

/// Deterministically resamples `>15` gradient stops down to `15` strictly monotonic unique `u8`
/// `GRADRECORD` stops, preserving exact first and last endpoints and selecting interior stops via
/// greedy Douglas-Peucker maximum piecewise-linear RGBA interpolation error reduction.
pub(super) fn resample_swf_gradient_stops(
    stops: &[neoflash_core::GradientStop],
    linear_rgb: bool,
) -> Vec<neoflash_core::GradientStop> {
    if stops.len() <= 15 {
        return stops.to_vec();
    }

    let mut sorted = stops.to_vec();
    sorted.sort_by_key(|s| s.ratio);

    let first = sorted[0].clone();
    let last = sorted[sorted.len() - 1].clone();
    if first.ratio == last.ratio {
        if first.ratio < 255 {
            return vec![
                first.clone(),
                neoflash_core::GradientStop {
                    ratio: first.ratio + 1,
                    color: last.color,
                },
            ];
        }
        return vec![
            neoflash_core::GradientStop {
                ratio: 254,
                color: first.color,
            },
            last,
        ];
    }

    let mut dedup = Vec::with_capacity(sorted.len());
    dedup.push(first.clone());
    for stop in &sorted[1..sorted.len() - 1] {
        if stop.ratio == last.ratio {
            continue;
        }
        let prev_ratio = dedup.last().expect("non-empty dedup").ratio;
        if stop.ratio > prev_ratio {
            dedup.push(stop.clone());
        } else if dedup.len() > 1 {
            *dedup.last_mut().expect("interior dedup stop") = stop.clone();
        }
    }
    dedup.push(last.clone());

    if dedup.len() >= 15 {
        let pts: Vec<[f64; 4]> = dedup
            .iter()
            .map(|s| swf_color_to_interp_space(s.color, linear_rgb))
            .collect();
        let mut selected: Vec<usize> = vec![0, dedup.len() - 1];
        while selected.len() < 15 {
            let mut best_idx: Option<usize> = None;
            let mut best_err = -1.0_f64;
            let mut best_gap = 0u8;

            for seg in selected.windows(2) {
                let left = seg[0];
                let right = seg[1];
                if right <= left + 1 {
                    continue;
                }
                let r_left = dedup[left].ratio;
                let r_right = dedup[right].ratio;
                let span = f64::from(r_right - r_left);
                for k in (left + 1)..right {
                    let r_k = dedup[k].ratio;
                    let t = f64::from(r_k - r_left) / span;
                    let mut err_sq = 0.0_f64;
                    for ((&pl, &pr), &pk) in pts[left].iter().zip(&pts[right]).zip(&pts[k]) {
                        let pred = pl + (pr - pl) * t;
                        let diff = pk - pred;
                        err_sq += diff * diff;
                    }
                    let min_gap = (r_k - r_left).min(r_right - r_k);
                    let better = if (err_sq - best_err).abs() > 1e-6 {
                        err_sq > best_err
                    } else {
                        min_gap > best_gap
                    };
                    if better {
                        best_err = err_sq;
                        best_gap = min_gap;
                        best_idx = Some(k);
                    }
                }
            }

            let Some(chosen) = best_idx else {
                break;
            };
            selected.push(chosen);
            selected.sort_unstable();
        }
        return selected.into_iter().map(|i| dedup[i].clone()).collect();
    }

    let available_unique = usize::from(last.ratio) - usize::from(first.ratio) + 1;
    let target_len = available_unique.min(15);
    let mut out = dedup;
    while out.len() < target_len {
        let Some((best_seg, _)) = out
            .windows(2)
            .enumerate()
            .map(|(i, w)| (i, w[1].ratio - w[0].ratio))
            .filter(|&(_, gap)| gap >= 2)
            .max_by_key(|&(_, gap)| gap)
        else {
            break;
        };
        let a = &out[best_seg];
        let b = &out[best_seg + 1];
        let mid_ratio = ((u16::from(a.ratio) + u16::from(b.ratio)) / 2) as u8;
        let t = f64::from(mid_ratio - a.ratio) / f64::from(b.ratio - a.ratio);
        let va = swf_color_to_interp_space(a.color, linear_rgb);
        let vb = swf_color_to_interp_space(b.color, linear_rgb);
        let vmid = [
            va[0] + (vb[0] - va[0]) * t,
            va[1] + (vb[1] - va[1]) * t,
            va[2] + (vb[2] - va[2]) * t,
            va[3] + (vb[3] - va[3]) * t,
        ];
        out.insert(
            best_seg + 1,
            neoflash_core::GradientStop {
                ratio: mid_ratio,
                color: swf_color_from_interp_space(vmid, linear_rgb),
            },
        );
    }
    out
}

pub(super) fn write_single_fill_style(w: &mut SwfBitWriter, fill: &SwfShapeFillStyle) {
    if let Some(brush) = fill.bitmap {
        w.u8(match (brush.repeat, brush.smoothing) {
            (true, true) => 0x40,
            (false, true) => 0x41,
            (true, false) => 0x42,
            (false, false) => 0x43,
        });
        w.u16(brush.character);
        let [a, b, c, d, x, y] = brush.matrix;
        w.matrix_px([a * 20., b * 20., c * 20., d * 20., x, y]);
        return;
    }
    if let Some(grad) = &fill.gradient
        && !grad.stops.is_empty()
    {
        let focal = if grad.focal_point_ratio.is_finite() {
            grad.focal_point_ratio
        } else {
            0.0
        };
        let fill_type: u8 = match grad.kind {
            GradientKind::Linear => 0x10,
            GradientKind::Radial => {
                if focal.abs() > 1e-4 {
                    0x13
                } else {
                    0x12
                }
            }
        };
        w.u8(fill_type);
        w.matrix_px(grad.matrix);
        let resampled_storage;
        let stops_to_write: &[neoflash_core::GradientStop] = if grad.stops.len() > 15 {
            resampled_storage = resample_swf_gradient_stops(&grad.stops, grad.linear_rgb);
            &resampled_storage
        } else {
            &grad.stops
        };
        let count = stops_to_write.len().min(15);
        let spread_bits: u8 = match grad.spread {
            GradientSpread::Extend => 0,
            GradientSpread::Reflect => 1,
            GradientSpread::Repeat => 2,
        };
        let interp_bits: u8 = u8::from(grad.linear_rgb);
        w.u8((spread_bits << 6) | (interp_bits << 4) | (count as u8));
        for stop in stops_to_write.iter().take(count) {
            w.u8(stop.ratio);
            w.rgba(stop.color);
        }
        if fill_type == 0x13 {
            let focal_fixed8 = (focal.clamp(-1.0, 1.0) * 256.0).round() as i16;
            w.i16(focal_fixed8);
        }
    } else {
        w.u8(0x00); // Solid fill
        w.rgba(fill.color);
    }
}

/// Flash edge records use at most 17 signed bits per delta (3,276.75 pixels).
/// Subdivide both halves of a morph identically so corresponding contours remain paired.
impl SwfEdge {
    pub(super) fn fits_record(self) -> bool {
        let segments = self
            .control
            .map_or([self.start, self.start, self.end], |c| {
                [self.start, c, self.end]
            });
        segments.windows(2).all(|p| {
            (0..2).all(|i| (-65_536..=65_535).contains(&(i64::from(p[1][i]) - i64::from(p[0][i]))))
        })
    }

    pub(super) fn halves(self) -> (Self, Self) {
        let midpoint = |a: [i32; 2], b: [i32; 2]| [a[0].midpoint(b[0]), a[1].midpoint(b[1])];
        let (left, right, mid) = if let Some(ctrl) = self.control {
            let left = midpoint(self.start, ctrl);
            let right = midpoint(ctrl, self.end);
            (Some(left), Some(right), midpoint(left, right))
        } else {
            (None, None, midpoint(self.start, self.end))
        };
        (
            Self {
                end: mid,
                control: left,
                ..self
            },
            Self {
                start: mid,
                control: right,
                ..self
            },
        )
    }
}

pub(super) fn write_edge_record(w: &mut SwfBitWriter, edge: SwfEdge, morph_peer: Option<SwfEdge>) {
    if !edge.fits_record() || morph_peer.is_some_and(|peer| !peer.fits_record()) {
        let (left, right) = edge.halves();
        let peers = morph_peer.map(SwfEdge::halves).unzip();
        write_edge_record(w, left, peers.0);
        write_edge_record(w, right, peers.1);
        return;
    }
    if let Some(ctrl) = edge.control {
        let cdx = ctrl[0] - edge.start[0];
        let cdy = ctrl[1] - edge.start[1];
        let adx = edge.end[0] - ctrl[0];
        let ady = edge.end[1] - ctrl[1];
        let nbits = needed_sb_bits(&[cdx, cdy, adx, ady]).max(2);
        w.bit(true); // Edge record
        w.bit(false); // Curved edge
        w.ub(4, (nbits - 2) as u32);
        w.sb(nbits, cdx);
        w.sb(nbits, cdy);
        w.sb(nbits, adx);
        w.sb(nbits, ady);
    } else {
        let dx = edge.end[0] - edge.start[0];
        let dy = edge.end[1] - edge.start[1];
        let nbits = needed_sb_bits(&[dx, dy]).max(2);
        w.bit(true); // Edge record
        w.bit(true); // Straight edge
        w.ub(4, (nbits - 2) as u32);
        w.bit(true); // general line (dx, dy)
        w.sb(nbits, dx);
        w.sb(nbits, dy);
    }
}

pub(super) fn write_shape_edges_records(
    w: &mut SwfBitWriter,
    edges: &[SwfEdge],
    num_fill_bits: usize,
    num_line_bits: usize,
    morph_peers: Option<&[SwfEdge]>,
) {
    w.align();
    w.ub(4, num_fill_bits as u32);
    w.ub(4, num_line_bits as u32);

    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut cur_f1 = 0u16;
    let mut cur_ls = 0u16;
    let mut first = true;

    for (index, edge) in edges.iter().enumerate() {
        let need_move = first || edge.start[0] != cur_x || edge.start[1] != cur_y;
        let need_f1 = num_fill_bits > 0 && (first || edge.fill_1 != cur_f1);
        let need_ls = num_line_bits > 0 && (first || edge.line != cur_ls);
        first = false;

        if need_move || need_f1 || need_ls {
            w.bit(false); // StyleChangeRecord
            w.bit(false); // state_new_styles = 0
            w.bit(need_ls);
            w.bit(need_f1);
            w.bit(false); // state_fill_style0 = 0
            w.bit(need_move);

            if need_move {
                let mx = edge.start[0];
                let my = edge.start[1];
                let mbits = needed_sb_bits(&[mx, my]).max(1);
                w.ub(5, mbits as u32);
                w.sb(mbits, mx);
                w.sb(mbits, my);
            }
            if need_f1 {
                w.ub(num_fill_bits, u32::from(edge.fill_1));
                cur_f1 = edge.fill_1;
            }
            if need_ls {
                w.ub(num_line_bits, u32::from(edge.line));
                cur_ls = edge.line;
            }
        }

        write_edge_record(w, *edge, morph_peers.map(|peers| peers[index]));
        cur_x = edge.end[0];
        cur_y = edge.end[1];
    }

    // EndShapeRecord (6 zero bits)
    w.ub(6, 0);
    w.align();
}

pub(super) fn compute_edges_bounds(edges: &[SwfEdge]) -> [i32; 4] {
    if edges.is_empty() {
        return [0, 20, 0, 20];
    }
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    let mut include = |pt: [i32; 2]| {
        min_x = min_x.min(pt[0]);
        max_x = max_x.max(pt[0]);
        min_y = min_y.min(pt[1]);
        max_y = max_y.max(pt[1]);
    };
    for e in edges {
        include(e.start);
        if let Some(c) = e.control {
            include(c);
        }
        include(e.end);
    }
    if max_x <= min_x {
        max_x = min_x + 20;
    }
    if max_y <= min_y {
        max_y = min_y + 20;
    }
    [min_x, max_x, min_y, max_y]
}

/// Build a `DefineShape3` (tag 32) or `DefineShape4` (tag 83) tag code and payload for a vector element.
pub(super) fn build_define_shape_for_element(
    project: &Project,
    shape_id: u16,
    element: &Element,
    fonts: &mut SwfOutlineFonts,
    bitmap: Option<SwfBitmapBrush>,
    options: &SwfPublishOptions,
) -> Result<Option<(u16, Vec<u8>)>, ExportError> {
    let mut fills: Vec<SwfShapeFillStyle> = Vec::new();
    let mut lines: Vec<SwfShapeLineStyle> = Vec::new();
    let mut edges: Vec<SwfEdge> = Vec::new();

    let mut add_fill =
        |color: Color, gradient: Option<GradientPaint>, bitmap: Option<SwfBitmapBrush>| -> u16 {
            if color[3] == 0 && gradient.is_none() && bitmap.is_none() {
                return 0;
            }
            fills.push(SwfShapeFillStyle {
                color,
                gradient,
                bitmap,
            });
            fills.len().min(usize::from(u16::MAX)) as u16
        };

    #[allow(clippy::too_many_arguments)]
    let mut add_line = |color: Color,
                        width: f64,
                        gradient: Option<GradientPaint>,
                        cap: StrokeCap,
                        join: StrokeJoin,
                        miter: f64|
     -> u16 {
        if width <= 0.0 || (color[3] == 0 && gradient.is_none()) {
            return 0;
        }
        let width_twips = (width * 20.0).round().clamp(1.0, 65535.0) as u16;
        lines.push(SwfShapeLineStyle {
            width_twips,
            color,
            gradient,
            stroke_cap: cap,
            stroke_join: join,
            miter_limit: miter,
        });
        lines.len().min(usize::from(u16::MAX)) as u16
    };

    match &element.kind {
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
            let f_idx = if open_oval(element) {
                0
            } else {
                add_fill(*fill, None, bitmap)
            };
            let l_idx = add_line(
                *stroke,
                *stroke_width,
                None,
                StrokeCap::Round,
                StrokeJoin::Round,
                3.0,
            );
            let Some(cmds) = element_vector_commands(element) else {
                return Ok(None);
            };
            convert_commands_to_swf_edges(&cmds, f_idx, l_idx, false, &mut edges);
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, None, bitmap);
            let profile = element.stroke_width_profile();
            if !profile.is_default() && options.expand_variable_width_strokes {
                if f_idx != 0 {
                    convert_commands_to_swf_edges(commands, f_idx, 0, true, &mut edges);
                }
                let expanded =
                    neoflash_core::expand_variable_width_stroke(commands, *stroke_width, &profile);
                if !expanded.is_empty() {
                    let sf_idx = add_fill(*stroke, None, None);
                    if sf_idx != 0 {
                        convert_commands_to_swf_edges(&expanded, sf_idx, 0, true, &mut edges);
                    }
                }
            } else {
                let l_idx = add_line(
                    *stroke,
                    *stroke_width,
                    None,
                    StrokeCap::Round,
                    StrokeJoin::Round,
                    3.0,
                );
                convert_commands_to_swf_edges(commands, f_idx, l_idx, false, &mut edges);
            }
        }
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let f_idx = add_fill(*fill, fill_gradient.clone(), bitmap);
            let profile = element.stroke_width_profile();
            if !profile.is_default() && options.expand_variable_width_strokes {
                if f_idx != 0 {
                    convert_commands_to_swf_edges(commands, f_idx, 0, true, &mut edges);
                }
                let expanded =
                    neoflash_core::expand_variable_width_stroke(commands, *stroke_width, &profile);
                if !expanded.is_empty() {
                    let sf_idx = add_fill(*stroke, stroke_gradient.clone(), None);
                    if sf_idx != 0 {
                        convert_commands_to_swf_edges(&expanded, sf_idx, 0, true, &mut edges);
                    }
                }
            } else {
                let l_idx = add_line(
                    *stroke,
                    *stroke_width,
                    stroke_gradient.clone(),
                    StrokeCap::Round,
                    StrokeJoin::Round,
                    3.0,
                );
                convert_commands_to_swf_edges(commands, f_idx, l_idx, false, &mut edges);
            }
        }
        ElementKind::TextBox { .. } | ElementKind::Text { .. } if options.outline_text => {
            let (commands, color) = swf_text_outline(project, element, fonts)?;
            let fill = add_fill(color, None, None);
            convert_commands_to_swf_edges(&commands, fill, 0, true, &mut edges);
        }
        _ => return Ok(None),
    }

    if fills.len() > usize::from(u16::MAX) {
        return Err(ExportError::Options(format!(
            "Element '{}' has {} fill styles; SWF DefineShape supports at most {}",
            element.name,
            fills.len(),
            u16::MAX
        )));
    }
    if lines.len() > usize::from(u16::MAX) {
        return Err(ExportError::Options(format!(
            "Element '{}' has {} line styles; SWF DefineShape supports at most {}",
            element.name,
            lines.len(),
            u16::MAX
        )));
    }
    if edges.is_empty() {
        return Ok(None);
    }

    let use_shape4 = options.profile.supports_define_shape4();
    let tag_code: u16 = if use_shape4 { 83 } else { 32 };
    let bounds = compute_edges_bounds(&edges);

    let mut w = SwfBitWriter::new();
    w.u16(shape_id);
    w.rect_twips(bounds);
    if use_shape4 {
        w.rect_twips(bounds); // edge_bounds
        let nonzero = matches!(
            element.kind,
            ElementKind::Path {
                fill_rule: FillRule::NonZero,
                ..
            } | ElementKind::PaintedPath {
                fill_rule: FillRule::NonZero,
                ..
            }
        );
        w.u8(if nonzero { 4 } else { 0 });
    }

    // FillStyleArray
    if fills.len() >= 0xff {
        w.u8(0xff);
        w.u16(fills.len() as u16);
    } else {
        w.u8(fills.len() as u8);
    }
    for fill in &fills {
        write_single_fill_style(&mut w, fill);
    }

    // LineStyleArray
    if lines.len() >= 0xff {
        w.u8(0xff);
        w.u16(lines.len() as u16);
    } else {
        w.u8(lines.len() as u8);
    }
    for line in &lines {
        w.u16(line.width_twips);
        if use_shape4 {
            let cap_bits: u32 = match line.stroke_cap {
                StrokeCap::Round => 0,
                StrokeCap::Butt => 1,
                StrokeCap::Square => 2,
            };
            let join_bits: u32 = match line.stroke_join {
                StrokeJoin::Round => 0,
                StrokeJoin::Bevel => 1,
                StrokeJoin::Miter => 2,
            };
            let has_fill = line.gradient.is_some();
            w.align();
            w.ub(2, cap_bits);
            w.ub(2, join_bits);
            w.bit(has_fill);
            w.bit(false); // no_hscale
            w.bit(false); // no_vscale
            w.bit(false); // pixel_hinting
            w.ub(5, 0); // reserved
            w.bit(false); // no_close
            w.ub(2, cap_bits); // end_cap
            w.align();
            if join_bits == 2 {
                let miter_fixed8 = (line.miter_limit.clamp(1.0, 255.0) * 256.0).round() as u16;
                w.u16(miter_fixed8);
            }
            if has_fill {
                write_single_fill_style(
                    &mut w,
                    &SwfShapeFillStyle {
                        bitmap: None,
                        color: line.color,
                        gradient: line.gradient.clone(),
                    },
                );
            } else {
                w.rgba(line.color);
            }
        } else {
            w.rgba(line.color);
        }
    }

    let num_fill_bits = needed_ub_bits(fills.len() as u32).max(1);
    let num_line_bits = needed_ub_bits(lines.len() as u32).max(1);
    write_shape_edges_records(&mut w, &edges, num_fill_bits, num_line_bits, None);

    Ok(Some((tag_code, w.buf)))
}

fn bake_affine_into_commands(cmds: &[PathCommand], m: [f64; 6]) -> Vec<PathCommand> {
    if m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        return cmds.to_vec();
    }
    let map_pt =
        |x: f64, y: f64| -> (f64, f64) { (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) };
    cmds.iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (nx, ny) = map_pt(x, y);
                PathCommand::MoveTo { x: nx, y: ny }
            }
            PathCommand::LineTo { x, y } => {
                let (nx, ny) = map_pt(x, y);
                PathCommand::LineTo { x: nx, y: ny }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (ncx, ncy) = map_pt(cx, cy);
                let (nx, ny) = map_pt(x, y);
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
                let (ncx1, ncy1) = map_pt(cx1, cy1);
                let (ncx2, ncy2) = map_pt(cx2, cy2);
                let (nx, ny) = map_pt(x, y);
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
        .collect()
}

struct MorphEndpoint {
    matrix: [f64; 6],
    fill: Color,
    stroke: Color,
    stroke_width: f64,
    edges: Vec<SwfEdge>,
}

pub(super) struct PreparedMorph<'a> {
    start: MorphEndpoint,
    end: MorphEndpoint,
    fill: MorphFill<'a>,
    has_fill: bool,
    has_line: bool,
}

pub(super) fn prepare_morph_keys<'a>(
    start: &'a neoflash_core::Keyframe,
    end: &'a neoflash_core::Keyframe,
) -> Option<PreparedMorph<'a>> {
    let ([start_el], [end_el]) = (start.elements.as_slice(), end.elements.as_slice()) else {
        return None;
    };
    // The SWF ratio cannot express geometry beyond either endpoint. These easing
    // curves can overshoot, so publish the core's evaluated frames for them.
    if matches!(start.tween, Some(Easing::Elastic))
        || matches!(start.tween, Some(Easing::CubicBezier { y1, y2, .. })
            if !(0.0..=1.0).contains(&y1) || !(0.0..=1.0).contains(&y2))
        || start.property_expressions.iter().any(|expr| expr.enabled)
    {
        return None;
    }
    if [start, end].iter().any(|key| {
        !key.bitmap_fills.is_empty()
            || !key.appearances.is_empty()
            || !key.shape_hints.is_empty()
            || !key.vector_effects.is_empty()
    }) || [start_el, end_el]
        .iter()
        .any(|el| !el.stroke_width_profile().is_default())
        || start_el.transform.color_transform != end_el.transform.color_transform
    {
        return None;
    }
    let morph_fill = morph_fill(start_el, end_el)?;
    let raw_start_cmds = element_vector_commands(start_el)?;
    let raw_end_cmds = element_vector_commands(end_el)?;
    if raw_start_cmds.is_empty()
        || !neoflash_core::paths_have_same_topology(&raw_start_cmds, &raw_end_cmds)
    {
        // Different command sequences use the core's contour alignment and resampling.
        // Matching only edge counts would pair unrelated points in the native SWF morph.
        return None;
    }
    let start_m = transform_matrix(start_el.transform);
    let end_m = transform_matrix(end_el.transform);
    if let MorphFill::Gradient(a, b) = morph_fill {
        let sa = GradientPaint {
            matrix: neoflash_core::affine_multiply(start_m, a.matrix),
            ..a.clone()
        };
        let eb = GradientPaint {
            matrix: neoflash_core::affine_multiply(end_m, b.matrix),
            ..b.clone()
        };
        if sa.validate().is_err() || eb.validate().is_err() {
            return None;
        }
    }
    let start_cmds = bake_affine_into_commands(&raw_start_cmds, start_m);
    let end_cmds = bake_affine_into_commands(&raw_end_cmds, end_m);
    let start_stroke_scale = (start_m[0] * start_m[3] - start_m[1] * start_m[2])
        .abs()
        .sqrt();
    let end_stroke_scale = (end_m[0] * end_m[3] - end_m[1] * end_m[2]).abs().sqrt();

    let extract_paints = |el: &Element| -> (Color, Color, f64) {
        match &el.kind {
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
            }
            | ElementKind::Path {
                fill,
                stroke,
                stroke_width,
                ..
            }
            | ElementKind::PaintedPath {
                fill,
                stroke,
                stroke_width,
                ..
            } => (
                if open_oval(el) { [0; 4] } else { *fill },
                *stroke,
                *stroke_width,
            ),
            _ => ([0, 0, 0, 255], [0, 0, 0, 0], 0.0),
        }
    };
    let (s_fill, s_stroke, s_raw_width) = extract_paints(start_el);
    let (e_fill, e_stroke, e_raw_width) = extract_paints(end_el);
    let s_width = s_raw_width * start_stroke_scale;
    let e_width = e_raw_width * end_stroke_scale;
    let has_fill = matches!(morph_fill, MorphFill::Gradient(..)) || s_fill[3] > 0 || e_fill[3] > 0;
    let has_line = (s_width > 0.0 && s_stroke[3] > 0) || (e_width > 0.0 && e_stroke[3] > 0);
    let fill_idx = u16::from(has_fill);
    let line_idx = u16::from(has_line);

    let mut start_edges = Vec::new();
    let mut end_edges = Vec::new();
    convert_commands_to_swf_edges(&start_cmds, fill_idx, line_idx, has_fill, &mut start_edges);
    convert_commands_to_swf_edges(&end_cmds, fill_idx, line_idx, has_fill, &mut end_edges);
    if start_edges.is_empty() || start_edges.len() != end_edges.len() {
        return None;
    }

    // A MoveTo in only one endpoint changes contour correspondence. Use evaluated
    // keyframes when those contours cannot share the StartEdges style sequence.
    if start_edges
        .windows(2)
        .zip(end_edges.windows(2))
        .any(|(s, e)| (s[0].end != s[1].start) != (e[0].end != e[1].start))
    {
        return None;
    }
    Some(PreparedMorph {
        start: MorphEndpoint {
            matrix: start_m,
            fill: s_fill,
            stroke: s_stroke,
            stroke_width: s_width,
            edges: start_edges,
        },
        end: MorphEndpoint {
            matrix: end_m,
            fill: e_fill,
            stroke: e_stroke,
            stroke_width: e_width,
            edges: end_edges,
        },
        fill: morph_fill,
        has_fill,
        has_line,
    })
}

/// Emit the same prepared geometry used to choose native morph or baked frame export.
/// Uses `DefineMorphShape2` (tag 84) for gradient morphs (`0x10` linear, `0x12` radial,
/// `0x13` focal radial, spread/interpolation flags, up to 15 stops) and `DefineMorphShape`
/// (tag 46) for solid/stroke-only morphs.
pub(super) fn build_define_morph_shape_tag(
    morph_id: u16,
    morph: PreparedMorph<'_>,
) -> (u16, Vec<u8>) {
    let PreparedMorph {
        start,
        end,
        fill,
        has_fill,
        has_line,
    } = morph;
    let use_morph2 = matches!(fill, MorphFill::Gradient(..));
    let tag_code = if use_morph2 { 84 } else { 46 };

    let start_edge_bounds = compute_edges_bounds(&start.edges);
    let end_edge_bounds = compute_edges_bounds(&end.edges);
    let expand_bounds = |b: [i32; 4], stroke_w: f64| -> [i32; 4] {
        let pad = if has_line {
            (stroke_w * 10.0).round().max(0.0) as i32
        } else {
            0
        };
        [
            b[0].saturating_sub(pad),
            b[1].saturating_add(pad),
            b[2].saturating_sub(pad),
            b[3].saturating_add(pad),
        ]
    };
    let start_bounds = if use_morph2 {
        expand_bounds(start_edge_bounds, start.stroke_width)
    } else {
        start_edge_bounds
    };
    let end_bounds = if use_morph2 {
        expand_bounds(end_edge_bounds, end.stroke_width)
    } else {
        end_edge_bounds
    };

    let mut styles_and_start = SwfBitWriter::new();
    if has_fill {
        styles_and_start.u8(1); // 1 morph fill style
        match fill {
            MorphFill::Solid => {
                styles_and_start.u8(0);
                styles_and_start.rgba(start.fill);
                styles_and_start.rgba(end.fill);
            }
            MorphFill::Gradient(a, b) => {
                let sfocal = a.focal_point_ratio.clamp(-1.0, 1.0);
                let efocal = b.focal_point_ratio.clamp(-1.0, 1.0);
                let fill_type: u8 = match a.kind {
                    GradientKind::Linear => 0x10,
                    GradientKind::Radial => {
                        if sfocal.abs() > 1e-4 || efocal.abs() > 1e-4 {
                            0x13
                        } else {
                            0x12
                        }
                    }
                };
                styles_and_start.u8(fill_type);
                styles_and_start.matrix_px(neoflash_core::affine_multiply(start.matrix, a.matrix));
                styles_and_start.matrix_px(neoflash_core::affine_multiply(end.matrix, b.matrix));
                let count = a.stops.len().min(15);
                let spread_bits: u8 = match a.spread {
                    GradientSpread::Extend => 0,
                    GradientSpread::Reflect => 1,
                    GradientSpread::Repeat => 2,
                };
                let interp_bits: u8 = u8::from(a.linear_rgb);
                styles_and_start.u8((spread_bits << 6) | (interp_bits << 4) | (count as u8));
                for (s_stop, e_stop) in a.stops.iter().zip(&b.stops).take(count) {
                    styles_and_start.u8(s_stop.ratio);
                    styles_and_start.rgba(s_stop.color);
                    styles_and_start.u8(e_stop.ratio);
                    styles_and_start.rgba(e_stop.color);
                }
                if fill_type == 0x13 {
                    styles_and_start.i16((sfocal * 256.0).round() as i16);
                    styles_and_start.i16((efocal * 256.0).round() as i16);
                }
            }
        }
    } else {
        styles_and_start.u8(0);
    }
    if has_line {
        styles_and_start.u8(1); // 1 morph line style
        styles_and_start.u16((start.stroke_width * 20.0).round().clamp(1.0, 65535.0) as u16);
        styles_and_start.u16((end.stroke_width * 20.0).round().clamp(1.0, 65535.0) as u16);
        if use_morph2 {
            styles_and_start.u16(0); // MORPHLINESTYLE2 flags: round caps/joins, scaling, solid
        }
        styles_and_start.rgba(start.stroke);
        styles_and_start.rgba(end.stroke);
    } else {
        styles_and_start.u8(0);
    }
    write_shape_edges_records(&mut styles_and_start, &start.edges, 1, 1, Some(&end.edges));

    let mut end_edges_writer = SwfBitWriter::new();
    // EndEdges inherits fill and line selection from the matching StartEdges.
    write_shape_edges_records(&mut end_edges_writer, &end.edges, 0, 0, Some(&start.edges));

    let mut w = SwfBitWriter::new();
    w.u16(morph_id);
    w.rect_twips(start_bounds);
    w.rect_twips(end_bounds);
    if use_morph2 {
        w.rect_twips(start_edge_bounds);
        w.rect_twips(end_edge_bounds);
        w.u8(u8::from(has_line));
    }
    w.u32(styles_and_start.buf.len() as u32);
    w.bytes(&styles_and_start.buf);
    w.bytes(&end_edges_writer.buf);
    (tag_code, w.buf)
}
