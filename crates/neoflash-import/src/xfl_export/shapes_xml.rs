//! XFL DOMShape contour quantization, cubic-to-quadratic subdivision, and XML formatting helpers.
use super::*;

pub(super) struct ShapePaintSpec {
    pub(super) fill_color: Color,
    pub(super) fill_gradient: Option<GradientPaint>,
    pub(super) fill_bitmap: Option<(String, [f64; 6], bool)>,
    pub(super) stroke_color: Color,
    pub(super) stroke_gradient: Option<GradientPaint>,
    pub(super) stroke_width: f64,
}

#[derive(Clone, Copy)]
pub(super) struct ShapeSeg {
    start: Point24_8,
    ctrl: Option<Point24_8>,
    end: Point24_8,
}

#[derive(Clone, Copy)]
pub(super) struct StyledSeg {
    seg: ShapeSeg,
    has_fill: bool,
    has_stroke: bool,
}

pub(super) fn render_dom_shapes(
    element: &Element,
    commands: &[PathCommand],
    paint: ShapePaintSpec,
) -> Vec<String> {
    let valid_fill_grad = paint.fill_gradient.as_ref().and_then(sanitize_gradient);
    let valid_fill_bmp = paint
        .fill_bitmap
        .as_ref()
        .filter(|(_, m, _)| m.iter().all(|v| v.is_finite() && v.abs() <= 1.0e9));
    let has_fill_paint = valid_fill_bmp.is_some()
        || valid_fill_grad
            .as_ref()
            .is_some_and(|g| g.stops.iter().any(|s| s.color[3] > 0))
        || paint.fill_color[3] > 0;

    let valid_stroke_grad = paint.stroke_gradient.as_ref().and_then(sanitize_gradient);
    let has_stroke_paint = paint.stroke_width.is_finite()
        && paint.stroke_width > 0.0
        && (valid_stroke_grad
            .as_ref()
            .is_some_and(|g| g.stops.iter().any(|s| s.color[3] > 0))
            || paint.stroke_color[3] > 0);

    if !has_fill_paint && !has_stroke_paint {
        return Vec::new();
    }

    let contour_batches = build_styled_contours(commands, has_fill_paint, has_stroke_paint);
    if contour_batches.is_empty() {
        return Vec::new();
    }

    // Group contours into `<DOMShape>` chunks of at most `MAX_SHAPE_SEGMENTS` segments.
    let mut shape_chunks: Vec<Vec<StyledSeg>> = Vec::new();
    let mut current_chunk: Vec<StyledSeg> = Vec::new();
    for contour in contour_batches {
        if !current_chunk.is_empty() && current_chunk.len() + contour.len() > MAX_SHAPE_SEGMENTS {
            shape_chunks.push(std::mem::take(&mut current_chunk));
        }
        if contour.len() <= MAX_SHAPE_SEGMENTS {
            current_chunk.extend(contour);
        } else if contour.len() <= 16_384 {
            shape_chunks.push(contour);
        } else if !has_fill_paint {
            // Very long stroke-only polyline can be sliced directly at MAX_SHAPE_SEGMENTS.
            for slice in contour.chunks(MAX_SHAPE_SEGMENTS) {
                shape_chunks.push(slice.to_vec());
            }
        }
    }
    if !current_chunk.is_empty() {
        shape_chunks.push(current_chunk);
    }

    let mut out = Vec::with_capacity(shape_chunks.len());
    for segs in shape_chunks {
        let used_fill = segs.iter().any(|s| s.has_fill);
        let used_stroke = segs.iter().any(|s| s.has_stroke);
        if !used_fill && !used_stroke {
            continue;
        }
        let mut xml = String::with_capacity(512);
        writeln!(
            xml,
            "<DOMShape name=\"{}\">",
            escape_xml_attr(&element.name)
        )
        .unwrap();
        append_matrix_and_pivot(&mut xml, element.transform);

        if used_fill {
            xml.push_str("  <fills>\n    <FillStyle index=\"1\">\n");
            render_paint_element(
                &mut xml,
                "      ",
                paint.fill_color,
                valid_fill_grad.as_ref(),
                valid_fill_bmp,
            );
            xml.push_str("    </FillStyle>\n  </fills>\n");
        }
        if used_stroke {
            let weight = paint.stroke_width.clamp(0.05, 100_000.0);
            xml.push_str("  <strokes>\n    <StrokeStyle index=\"1\">\n");
            writeln!(
                xml,
                "      <SolidStroke scaleMode=\"normal\" weight=\"{}\">",
                fmt_num(weight)
            )
            .unwrap();
            xml.push_str("        <fill>\n");
            render_paint_element(
                &mut xml,
                "          ",
                paint.stroke_color,
                valid_stroke_grad.as_ref(),
                None,
            );
            xml.push_str(
                "        </fill>\n      </SolidStroke>\n    </StrokeStyle>\n  </strokes>\n",
            );
        }

        xml.push_str("  <edges>\n");
        render_edge_elements(&mut xml, &segs);
        xml.push_str("  </edges>\n</DOMShape>");
        out.push(xml);
    }
    out
}

pub(super) fn sanitize_gradient(grad: &GradientPaint) -> Option<GradientPaint> {
    if grad.stops.is_empty() {
        return None;
    }
    let mut stops = grad.stops.clone();
    stops.sort_by_key(|s| s.ratio);
    if stops.len() > 15 {
        let total = stops.len();
        let mut sampled = Vec::with_capacity(15);
        for i in 0..15 {
            let idx = (i * (total - 1)) / 14;
            sampled.push(stops[idx].clone());
        }
        stops = sampled;
    }
    let focal = if grad.focal_point_ratio.is_finite() {
        grad.focal_point_ratio.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let candidate = GradientPaint {
        kind: grad.kind,
        matrix: grad.matrix,
        stops,
        focal_point_ratio: focal,
        spread: grad.spread,
        linear_rgb: grad.linear_rgb,
    };
    candidate.validate().is_ok().then_some(candidate)
}

pub(super) fn render_paint_element(
    xml: &mut String,
    indent: &str,
    fallback_color: Color,
    gradient: Option<&GradientPaint>,
    bitmap: Option<&(String, [f64; 6], bool)>,
) {
    if let Some((bmp_path, m, clipped)) = bitmap {
        write!(
            xml,
            "{indent}<BitmapFill bitmapPath=\"{}\"",
            escape_xml_attr(bmp_path)
        )
        .unwrap();
        if *clipped {
            xml.push_str(" bitmapIsClipped=\"true\"");
        }
        xml.push_str(">\n");
        append_raw_matrix(
            xml,
            &format!("{indent}  "),
            [
                m[0] * 20.0,
                m[1] * 20.0,
                m[2] * 20.0,
                m[3] * 20.0,
                m[4],
                m[5],
            ],
            true,
        );
        writeln!(xml, "{indent}</BitmapFill>").unwrap();
    } else if let Some(grad) = gradient {
        let tag = match grad.kind {
            GradientKind::Linear => "LinearGradient",
            GradientKind::Radial => "RadialGradient",
        };
        write!(xml, "{indent}<{tag}").unwrap();
        match grad.spread {
            neoflash_core::GradientSpread::Extend => {}
            neoflash_core::GradientSpread::Reflect => xml.push_str(" spreadMethod=\"reflect\""),
            neoflash_core::GradientSpread::Repeat => xml.push_str(" spreadMethod=\"repeat\""),
        }
        if grad.linear_rgb {
            xml.push_str(" interpolationMethod=\"linearRGB\"");
        }
        if grad.kind == GradientKind::Radial && grad.focal_point_ratio.abs() > 1e-6 {
            write!(
                xml,
                " focalPointRatio=\"{}\"",
                fmt_num(grad.focal_point_ratio)
            )
            .unwrap();
        }
        xml.push_str(">\n");
        append_raw_matrix(xml, &format!("{indent}  "), grad.matrix, true);
        for stop in &grad.stops {
            let (hex, alpha_opt) = fmt_color_alpha(stop.color);
            let ratio = f64::from(stop.ratio) / 255.0;
            write!(
                xml,
                "{indent}  <GradientEntry color=\"{hex}\" ratio=\"{}\"",
                fmt_num(ratio)
            )
            .unwrap();
            if let Some(a) = alpha_opt {
                write!(xml, " alpha=\"{a}\"").unwrap();
            }
            xml.push_str("/>\n");
        }
        writeln!(xml, "{indent}</{tag}>").unwrap();
    } else {
        let (hex, alpha_opt) = fmt_color_alpha(fallback_color);
        write!(xml, "{indent}<SolidColor color=\"{hex}\"").unwrap();
        if let Some(a) = alpha_opt {
            write!(xml, " alpha=\"{a}\"").unwrap();
        }
        xml.push_str("/>\n");
    }
}

pub(super) fn build_styled_contours(
    commands: &[PathCommand],
    has_fill: bool,
    has_stroke: bool,
) -> Vec<Vec<StyledSeg>> {
    let mut contours: Vec<Vec<StyledSeg>> = Vec::new();
    let mut start_px: Option<[f64; 2]> = None;
    let mut start_pt: Option<Point24_8> = None;
    let mut cur_px: Option<[f64; 2]> = None;
    let mut cur_pt: Option<Point24_8> = None;
    let mut explicit_close = false;
    let mut raw_segs: Vec<ShapeSeg> = Vec::new();

    let flush_contour = |raw_segs: &mut Vec<ShapeSeg>,
                         start_pt: Option<Point24_8>,
                         cur_pt: Option<Point24_8>,
                         explicit_close: bool,
                         contours: &mut Vec<Vec<StyledSeg>>| {
        let (Some(s_pt), Some(mut end_pt)) = (start_pt, cur_pt) else {
            raw_segs.clear();
            return;
        };
        if explicit_close && end_pt != s_pt {
            push_quantized_segment(raw_segs, end_pt, None, s_pt);
            end_pt = s_pt;
        }
        if raw_segs.is_empty() {
            return;
        }
        let is_closed = explicit_close || end_pt == s_pt;
        let mut styled: Vec<StyledSeg> = raw_segs
            .drain(..)
            .map(|seg| StyledSeg {
                seg,
                has_fill,
                has_stroke,
            })
            .collect();
        if has_fill && !is_closed {
            // Implicitly close the fill boundary without adding a closing stroke edge.
            styled.push(StyledSeg {
                seg: ShapeSeg {
                    start: end_pt,
                    ctrl: None,
                    end: s_pt,
                },
                has_fill: true,
                has_stroke: false,
            });
        }
        contours.push(styled);
    };

    for &cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                flush_contour(
                    &mut raw_segs,
                    start_pt,
                    cur_pt,
                    explicit_close,
                    &mut contours,
                );
                let q = point_px_to_24_8(x, y);
                start_px = Some([x, y]);
                start_pt = Some(q);
                cur_px = Some([x, y]);
                cur_pt = Some(q);
                explicit_close = false;
            }
            PathCommand::LineTo { x, y } => {
                if let Some(c_pt) = cur_pt {
                    let next_pt = point_px_to_24_8(x, y);
                    push_quantized_segment(&mut raw_segs, c_pt, None, next_pt);
                    cur_px = Some([x, y]);
                    cur_pt = Some(next_pt);
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(c_pt) = cur_pt {
                    let ctrl_pt = point_px_to_24_8(cx, cy);
                    let next_pt = point_px_to_24_8(x, y);
                    push_quantized_segment(&mut raw_segs, c_pt, Some(ctrl_pt), next_pt);
                    cur_px = Some([x, y]);
                    cur_pt = Some(next_pt);
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
                if let (Some(c_px), Some(mut c_pt)) = (cur_px, cur_pt) {
                    let mut quads = Vec::with_capacity(4);
                    cubic_to_quadratics(c_px, [cx1, cy1], [cx2, cy2], [x, y], 0, &mut quads);
                    let total_q = quads.len();
                    let final_pt = point_px_to_24_8(x, y);
                    for (qi, (ctrl_px, end_px)) in quads.into_iter().enumerate() {
                        let q_ctrl = point_px_to_24_8(ctrl_px[0], ctrl_px[1]);
                        let q_end = if qi + 1 == total_q {
                            final_pt
                        } else {
                            point_px_to_24_8(end_px[0], end_px[1])
                        };
                        push_quantized_segment(&mut raw_segs, c_pt, Some(q_ctrl), q_end);
                        c_pt = q_end;
                    }
                    cur_px = Some([x, y]);
                    cur_pt = Some(final_pt);
                }
            }
            PathCommand::Close => {
                if start_px.is_some() {
                    explicit_close = true;
                    flush_contour(
                        &mut raw_segs,
                        start_pt,
                        cur_pt,
                        explicit_close,
                        &mut contours,
                    );
                    start_px = None;
                    start_pt = None;
                    cur_px = None;
                    cur_pt = None;
                    explicit_close = false;
                }
            }
        }
    }
    flush_contour(
        &mut raw_segs,
        start_pt,
        cur_pt,
        explicit_close,
        &mut contours,
    );
    contours
}

pub(super) fn push_quantized_segment(
    out: &mut Vec<ShapeSeg>,
    start: Point24_8,
    ctrl: Option<Point24_8>,
    end: Point24_8,
) {
    let simplified_ctrl = ctrl.filter(|&c| c != start && c != end);
    match simplified_ctrl {
        None => {
            if start != end {
                out.push(ShapeSeg {
                    start,
                    ctrl: None,
                    end,
                });
            }
        }
        Some(c) => {
            if start != end {
                out.push(ShapeSeg {
                    start,
                    ctrl: Some(c),
                    end,
                });
            } else {
                // Split degenerate self-loop quadratic (`start == end`, `c != start`) at t = 0.5
                // into two non-self-loop segments so `order_stroke_chain` never sees `a == b`.
                let mid = [(start[0] + c[0]) / 2, (start[1] + c[1]) / 2];
                if mid != start {
                    out.push(ShapeSeg {
                        start,
                        ctrl: None,
                        end: mid,
                    });
                    out.push(ShapeSeg {
                        start: mid,
                        ctrl: None,
                        end: start,
                    });
                }
            }
        }
    }
}

pub(super) fn cubic_to_quadratics(
    p0: [f64; 2],
    p1: [f64; 2],
    p2: [f64; 2],
    p3: [f64; 2],
    depth: u8,
    out: &mut Vec<([f64; 2], [f64; 2])>,
) {
    // Degree-reduction error bound between cubic (p0, p1, p2, p3) and midpoint-matched
    // quadratic with control c = (3*(p1 + p2) - (p0 + p3)) / 4 is (sqrt(3)/36) * |p3 - 3*p2 + 3*p1 - p0|.
    let dx = p3[0] - 3.0 * p2[0] + 3.0 * p1[0] - p0[0];
    let dy = p3[1] - 3.0 * p2[1] + 3.0 * p1[1] - p0[1];
    let err = dx.hypot(dy) * 0.048_112_522_432_468_81;

    if depth >= CUBIC_SUBDIV_MAX_DEPTH || !err.is_finite() || err <= CUBIC_SUBDIV_TOL_PX {
        let ctrl = [
            0.75 * (p1[0] + p2[0]) - 0.25 * (p0[0] + p3[0]),
            0.75 * (p1[1] + p2[1]) - 0.25 * (p0[1] + p3[1]),
        ];
        out.push((ctrl, p3));
        return;
    }

    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let mid = midpoint(p012, p123);

    cubic_to_quadratics(p0, p01, p012, mid, depth + 1, out);
    cubic_to_quadratics(mid, p123, p23, p3, depth + 1, out);
}

pub(super) fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])]
}

pub(super) fn render_edge_elements(xml: &mut String, segs: &[StyledSeg]) {
    let mut idx = 0usize;
    while idx < segs.len() {
        let fill_flag = segs[idx].has_fill;
        let stroke_flag = segs[idx].has_stroke;
        let mut edge_str = String::new();
        let mut cursor: Option<Point24_8> = None;

        while idx < segs.len()
            && segs[idx].has_fill == fill_flag
            && segs[idx].has_stroke == stroke_flag
        {
            let s = segs[idx].seg;
            let mut token = String::with_capacity(48);
            if cursor != Some(s.start) {
                if !edge_str.is_empty() {
                    token.push(' ');
                }
                write!(token, "!{} {}", fmt_24_8(s.start[0]), fmt_24_8(s.start[1])).unwrap();
            }
            match s.ctrl {
                None => {
                    write!(token, "|{} {}", fmt_24_8(s.end[0]), fmt_24_8(s.end[1])).unwrap();
                }
                Some(c) => {
                    write!(
                        token,
                        "[{} {} {} {}",
                        fmt_24_8(c[0]),
                        fmt_24_8(c[1]),
                        fmt_24_8(s.end[0]),
                        fmt_24_8(s.end[1])
                    )
                    .unwrap();
                }
            }
            if !edge_str.is_empty() && edge_str.len() + token.len() > MAX_EDGE_ATTR_BYTES {
                break;
            }
            edge_str.push_str(&token);
            cursor = Some(s.end);
            idx += 1;
        }

        if !edge_str.is_empty() {
            xml.push_str("    <Edge");
            if fill_flag {
                xml.push_str(" fillStyle1=\"1\"");
            }
            if stroke_flag {
                xml.push_str(" strokeStyle=\"1\"");
            }
            writeln!(xml, " edges=\"{edge_str}\"/>").unwrap();
        }
    }
}

pub(super) fn px_to_24_8(px: f64) -> i64 {
    if !px.is_finite() {
        return 0;
    }
    // 1 pixel = 20 twips; 24.8 fixed-point twips = 20 * 256 = 5120 units per pixel.
    (px * 5120.0)
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64
}

pub(super) fn point_px_to_24_8(x: f64, y: f64) -> Point24_8 {
    [px_to_24_8(x), px_to_24_8(y)]
}

/// Formats a signed 24.8 fixed-point twip value (`val_24_8` within `i32::MIN..=i32::MAX`).
/// Exact integer twips emit decimal integers so `xfl/shape.rs::solid_polygon` fast-path
/// applies; fractional sub-twips emit canonical XFL `#HHHHHH.HH` 24.8 two's-complement hex.
pub(super) fn fmt_24_8(val_24_8: i64) -> String {
    let clamped = val_24_8.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    if (clamped & 0xff) == 0 {
        return (clamped >> 8).to_string();
    }
    let bits = clamped as u32;
    let raw_whole = (bits >> 8) & 0xff_ffff;
    let frac = (bits & 0xff) as u8;
    if clamped < 0 {
        format!("#{raw_whole:06X}.{frac:02X}")
    } else {
        format!("#{raw_whole:X}.{frac:02X}")
    }
}

pub(super) fn safe_placement_matrix(mut t: Transform) -> [f64; 6] {
    if !t.x.is_finite() {
        t.x = 0.0;
    }
    if !t.y.is_finite() {
        t.y = 0.0;
    }
    if !t.rotation.is_finite() {
        t.rotation = 0.0;
    }
    if !t.skew.is_finite() {
        t.skew = 0.0;
    }
    if !t.scale_x.is_finite() {
        t.scale_x = 1.0;
    }
    if !t.scale_y.is_finite() {
        t.scale_y = 1.0;
    }
    t.x = t.x.clamp(-1.0e9, 1.0e9);
    t.y = t.y.clamp(-1.0e9, 1.0e9);

    let clamp_scale = |s: f64| -> f64 {
        if s.abs() < 1.0e-4 {
            if s.is_sign_negative() && s != 0.0 {
                -1.0e-4
            } else {
                1.0e-4
            }
        } else {
            s.clamp(-1.0e4, 1.0e4)
        }
    };
    t.scale_x = clamp_scale(t.scale_x);
    t.scale_y = clamp_scale(t.scale_y);

    // Keep X and Y basis vectors non-collinear (`skew` away from ±90° mod 180°).
    let skew_mod = t.skew.rem_euclid(180.0);
    if (skew_mod - 90.0).abs() < 0.05 {
        t.skew += if skew_mod >= 90.0 { 0.05 } else { -0.05 };
    }

    transform_matrix(t)
}

pub(super) fn append_matrix_and_pivot(out: &mut String, transform: Transform) {
    let m = safe_placement_matrix(transform);
    append_raw_matrix(out, "  ", m, false);
    if let Some([px, py]) = transform.pivot
        && px.is_finite()
        && py.is_finite()
        && (px.abs() > 1.0e-9 || py.abs() > 1.0e-9)
    {
        writeln!(
            out,
            "  <transformationPoint>\n    <Point x=\"{}\" y=\"{}\"/>\n  </transformationPoint>",
            fmt_num(px),
            fmt_num(py),
        )
        .unwrap();
    }
}

pub(super) fn append_raw_matrix(out: &mut String, indent: &str, m: [f64; 6], always_emit: bool) {
    let [a, b, c, d, tx, ty] = m;
    let is_identity = (a - 1.0).abs() <= 1.0e-9
        && b.abs() <= 1.0e-9
        && c.abs() <= 1.0e-9
        && (d - 1.0).abs() <= 1.0e-9
        && tx.abs() <= 1.0e-9
        && ty.abs() <= 1.0e-9;
    if is_identity && !always_emit {
        return;
    }
    writeln!(out, "{indent}<matrix>").unwrap();
    write!(out, "{indent}  <Matrix").unwrap();
    if (a - 1.0).abs() > 1.0e-9 {
        write!(out, " a=\"{}\"", fmt_num(a)).unwrap();
    }
    if b.abs() > 1.0e-9 {
        write!(out, " b=\"{}\"", fmt_num(b)).unwrap();
    }
    if c.abs() > 1.0e-9 {
        write!(out, " c=\"{}\"", fmt_num(c)).unwrap();
    }
    if (d - 1.0).abs() > 1.0e-9 {
        write!(out, " d=\"{}\"", fmt_num(d)).unwrap();
    }
    if tx.abs() > 1.0e-9 {
        write!(out, " tx=\"{}\"", fmt_num(tx)).unwrap();
    }
    if ty.abs() > 1.0e-9 {
        write!(out, " ty=\"{}\"", fmt_num(ty)).unwrap();
    }
    out.push_str("/>\n");
    writeln!(out, "{indent}</matrix>").unwrap();
}

pub(super) fn is_valid_xml_char(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

pub(super) fn escape_xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if !is_valid_xml_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(super) fn escape_xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if !is_valid_xml_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(super) fn escape_xfl_characters(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push_str("&#xD;");
            continue;
        }
        if ch == '\n' {
            out.push_str("&#xD;");
            continue;
        }
        if !is_valid_xml_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(super) fn cdata_wrap(s: &str) -> String {
    let filtered: String = s.chars().filter(|&c| is_valid_xml_char(c)).collect();
    let escaped = filtered.replace("]]>", "]]]]><![CDATA[>");
    format!("<![CDATA[{escaped}]]>")
}

pub(super) fn fmt_num(v: f64) -> String {
    if !v.is_finite() || v.abs() < 1.0e-10 {
        return "0".to_owned();
    }
    let rounded = v.round();
    if v.abs() < 1.0e15 && (v - rounded).abs() < 1.0e-10 {
        let int_val = rounded as i64;
        return if int_val == 0 {
            "0".to_owned()
        } else {
            int_val.to_string()
        };
    }
    let formatted = format!("{v:.9}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(super) fn fmt_hex_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub(super) fn fmt_color_alpha(c: Color) -> (String, Option<String>) {
    let hex = fmt_hex_rgb([c[0], c[1], c[2]]);
    let alpha = (c[3] != 255).then(|| fmt_num(f64::from(c[3]) / 255.0));
    (hex, alpha)
}
