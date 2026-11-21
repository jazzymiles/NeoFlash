//! SWF vector shape, gradient, bitmap fill, and morph-shape encoders.

use super::*;

fn element_has_gradient(el: &Element) -> bool {
    matches!(
        &el.kind,
        ElementKind::PaintedPath {
            fill_gradient: Some(_),
            ..
        } | ElementKind::PaintedPath {
            stroke_gradient: Some(_),
            ..
        }
    )
}

pub(super) fn can_morph_keys(
    start: &neoflash_core::Keyframe,
    end: &neoflash_core::Keyframe,
) -> bool {
    start.bitmap_fills.is_empty()
        && end.bitmap_fills.is_empty()
        && start.appearances.is_empty()
        && end.appearances.is_empty()
        && can_build_morph_shape(&start.elements, &end.elements)
}

pub(super) fn can_build_morph_shape(start_els: &[Element], end_els: &[Element]) -> bool {
    if start_els.len() != 1 || end_els.len() != 1 {
        return false;
    }
    if element_has_gradient(&start_els[0]) || element_has_gradient(&end_els[0]) {
        return false;
    }
    let a_cmds = element_vector_commands(&start_els[0]);
    let b_cmds = element_vector_commands(&end_els[0]);
    match (a_cmds, b_cmds) {
        (Some(a), Some(b)) => a.len() == b.len() && !a.is_empty(),
        _ => false,
    }
}

pub(super) fn element_vector_commands(el: &Element) -> Option<Vec<PathCommand>> {
    match &el.kind {
        ElementKind::Rectangle { width, height, .. } => Some(vec![
            PathCommand::MoveTo { x: 0.0, y: 0.0 },
            PathCommand::LineTo { x: *width, y: 0.0 },
            PathCommand::LineTo {
                x: *width,
                y: *height,
            },
            PathCommand::LineTo { x: 0.0, y: *height },
            PathCommand::Close,
        ]),
        ElementKind::Ellipse { width, height, .. } => Some(oval_commands(*width, *height)),
        ElementKind::Path { commands, .. } | ElementKind::PaintedPath { commands, .. } => {
            Some(commands.clone())
        }
        _ => None,
    }
}

pub(super) fn oval_commands(width: f64, height: f64) -> Vec<PathCommand> {
    let rx = width * 0.5;
    let ry = height * 0.5;
    let cx = rx;
    let cy = ry;
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let t = (std::f64::consts::PI / 8.0).tan();
    vec![
        PathCommand::MoveTo { x: cx + rx, y: cy },
        PathCommand::QuadraticTo {
            cx: cx + rx,
            cy: cy + ry * t,
            x: cx + rx * k,
            y: cy + ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx + rx * t,
            cy: cy + ry,
            x: cx,
            y: cy + ry,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx * t,
            cy: cy + ry,
            x: cx - rx * k,
            y: cy + ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx,
            cy: cy + ry * t,
            x: cx - rx,
            y: cy,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx,
            cy: cy - ry * t,
            x: cx - rx * k,
            y: cy - ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx - rx * t,
            cy: cy - ry,
            x: cx,
            y: cy - ry,
        },
        PathCommand::QuadraticTo {
            cx: cx + rx * t,
            cy: cy - ry,
            x: cx + rx * k,
            y: cy - ry * k,
        },
        PathCommand::QuadraticTo {
            cx: cx + rx,
            cy: cy - ry * t,
            x: cx + rx,
            y: cy,
        },
        PathCommand::Close,
    ]
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
        let count = grad.stops.len().min(15);
        let spread_bits: u8 = match grad.spread {
            GradientSpread::Extend => 0,
            GradientSpread::Reflect => 1,
            GradientSpread::Repeat => 2,
        };
        let interp_bits: u8 = u8::from(grad.linear_rgb);
        w.u8((spread_bits << 6) | (interp_bits << 4) | (count as u8));
        for stop in grad.stops.iter().take(count) {
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
        let need_f1 = first || edge.fill_1 != cur_f1;
        let need_ls = first || edge.line != cur_ls;
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
        } => {
            let f_idx = add_fill(*fill, None, bitmap);
            let l_idx = add_line(
                *stroke,
                *stroke_width,
                None,
                StrokeCap::Round,
                StrokeJoin::Round,
                3.0,
            );
            let Some(cmds) = element
                .to_path_commands()
                .map(|(c, _)| c)
                .or_else(|| element_vector_commands(element))
            else {
                return Ok(None);
            };
            convert_commands_to_swf_edges(&cmds, f_idx, l_idx, true, &mut edges);
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            let f_idx = add_fill(*fill, None, bitmap);
            let l_idx = add_line(
                *stroke,
                *stroke_width,
                None,
                StrokeCap::Round,
                StrokeJoin::Round,
                3.0,
            );
            let cmds = element
                .to_path_commands()
                .map(|(c, _)| c)
                .unwrap_or_else(|| oval_commands(*width, *height));
            convert_commands_to_swf_edges(&cmds, f_idx, l_idx, true, &mut edges);
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

/// Build a `DefineMorphShape` (tag 46) payload for a compatible start/end shape pair.
pub(super) fn build_define_morph_shape_payload(
    morph_id: u16,
    start_el: &Element,
    end_el: &Element,
) -> Option<Vec<u8>> {
    if element_has_gradient(start_el) || element_has_gradient(end_el) {
        return None;
    }
    let raw_start_cmds = element_vector_commands(start_el)?;
    let raw_end_cmds = element_vector_commands(end_el)?;
    if raw_start_cmds.len() != raw_end_cmds.len() || raw_start_cmds.is_empty() {
        return None;
    }
    let start_m = transform_matrix(start_el.transform);
    let end_m = transform_matrix(end_el.transform);
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
            } => (*fill, *stroke, *stroke_width),
            _ => ([0, 0, 0, 255], [0, 0, 0, 0], 0.0),
        }
    };
    let (s_fill, s_stroke, s_raw_width) = extract_paints(start_el);
    let (e_fill, e_stroke, e_raw_width) = extract_paints(end_el);
    let s_width = s_raw_width * start_stroke_scale;
    let e_width = e_raw_width * end_stroke_scale;
    let has_fill = s_fill[3] > 0 || e_fill[3] > 0;
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

    let start_bounds = compute_edges_bounds(&start_edges);
    let end_bounds = compute_edges_bounds(&end_edges);

    let mut styles_and_start = SwfBitWriter::new();
    if has_fill {
        styles_and_start.u8(1); // 1 morph fill style
        styles_and_start.u8(0x00); // solid morph fill
        styles_and_start.rgba(s_fill);
        styles_and_start.rgba(e_fill);
    } else {
        styles_and_start.u8(0);
    }
    if has_line {
        styles_and_start.u8(1); // 1 morph line style
        styles_and_start.u16((s_width * 20.0).round().clamp(1.0, 65535.0) as u16);
        styles_and_start.u16((e_width * 20.0).round().clamp(1.0, 65535.0) as u16);
        styles_and_start.rgba(s_stroke);
        styles_and_start.rgba(e_stroke);
    } else {
        styles_and_start.u8(0);
    }
    write_shape_edges_records(&mut styles_and_start, &start_edges, 1, 1, Some(&end_edges));

    let mut end_edges_writer = SwfBitWriter::new();
    write_shape_edges_records(&mut end_edges_writer, &end_edges, 1, 1, Some(&start_edges));

    let mut w = SwfBitWriter::new();
    w.u16(morph_id);
    w.rect_twips(start_bounds);
    w.rect_twips(end_bounds);
    w.u32(styles_and_start.buf.len() as u32);
    w.bytes(&styles_and_start.buf);
    w.bytes(&end_edges_writer.buf);
    Some(w.buf)
}
