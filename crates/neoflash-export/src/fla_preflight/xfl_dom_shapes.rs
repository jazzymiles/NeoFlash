use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn write_dom_shape_xml(
    el: &Element,
    commands: &[PathCommand],
    fill: Color,
    fill_gradient: Option<&GradientPaint>,
    bitmap_paint: Option<(&AuthoredBitmapFill, &str)>,
    stroke: Color,
    stroke_gradient: Option<&GradientPaint>,
    stroke_width: f64,
    indent: &str,
) -> String {
    let has_fill = bitmap_paint.is_some() || fill_gradient.is_some() || fill[3] > 0;
    let has_stroke = stroke_width > 0.0 && (stroke_gradient.is_some() || stroke[3] > 0);
    let edge_chunks = path_commands_to_xfl_edges(commands, has_fill);
    if edge_chunks.is_empty() || (!has_fill && !has_stroke) {
        return String::new();
    }

    let mut attrs = String::new();
    if !el.name.trim().is_empty() && el.name != "DOMShape" {
        attrs.push_str(&format!(" name=\"{}\"", xml_escape(&el.name)));
    }
    let matrix_xml = write_matrix_xml(&el.transform, &format!("{indent}  "));

    let fills_xml = if has_fill {
        let paint_indent = format!("{indent}      ");
        let paint_xml = if let Some((bitmap, name)) = bitmap_paint {
            let [a, b, c, d, tx, ty] = bitmap.matrix;
            format!(
                "{paint_indent}<BitmapFill bitmapPath=\"{}\" bitmapIsClipped=\"{}\">\n{paint_indent}  <matrix>\n{paint_indent}    <Matrix a=\"{}\" b=\"{}\" c=\"{}\" d=\"{}\" tx=\"{}\" ty=\"{}\"/>\n{paint_indent}  </matrix>\n{paint_indent}</BitmapFill>\n",
                xml_escape(name),
                !bitmap.repeat,
                fmt_num(a * 20.),
                fmt_num(b * 20.),
                fmt_num(c * 20.),
                fmt_num(d * 20.),
                fmt_num(tx),
                fmt_num(ty)
            )
        } else {
            write_paint_xml(fill, fill_gradient, &paint_indent)
        };
        format!(
            "{indent}  <fills>\n{indent}    <FillStyle index=\"1\">\n{paint_xml}{indent}    </FillStyle>\n{indent}  </fills>\n"
        )
    } else {
        String::new()
    };

    let strokes_xml = if has_stroke {
        let paint_xml = write_paint_xml(stroke, stroke_gradient, &format!("{indent}          "));
        format!(
            "{indent}  <strokes>\n{indent}    <StrokeStyle index=\"1\">\n{indent}      <SolidStroke scaleMode=\"normal\" weight=\"{}\">\n{indent}        <fill>\n{paint_xml}{indent}        </fill>\n{indent}      </SolidStroke>\n{indent}    </StrokeStyle>\n{indent}  </strokes>\n",
            fmt_num(stroke_width)
        )
    } else {
        String::new()
    };

    let mut edge_attrs = String::new();
    if has_fill {
        edge_attrs.push_str(" fillStyle1=\"1\"");
    }
    if has_stroke {
        edge_attrs.push_str(" strokeStyle=\"1\"");
    }

    let mut edges_xml = String::new();
    for edge in edge_chunks {
        edges_xml.push_str(&format!(
            "{indent}    <Edge{edge_attrs} edges=\"{edge}\"/>\n"
        ));
    }

    format!(
        "{indent}<DOMShape{attrs}>\n{matrix_xml}{fills_xml}{strokes_xml}{indent}  <edges>\n{edges_xml}{indent}  </edges>\n{indent}</DOMShape>\n"
    )
}

pub(super) fn write_paint_xml(
    color: Color,
    gradient: Option<&GradientPaint>,
    indent: &str,
) -> String {
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        let tag = match grad.kind {
            GradientKind::Linear => "LinearGradient",
            GradientKind::Radial => "RadialGradient",
        };
        let mut attrs = String::new();
        match grad.spread {
            GradientSpread::Extend => {}
            GradientSpread::Reflect => attrs.push_str(" spreadMethod=\"reflect\""),
            GradientSpread::Repeat => attrs.push_str(" spreadMethod=\"repeat\""),
        }
        if grad.linear_rgb {
            attrs.push_str(" interpolationMethod=\"linearRGB\"");
        }
        if grad.kind == GradientKind::Radial && grad.focal_point_ratio != 0.0 {
            attrs.push_str(&format!(
                " focalPointRatio=\"{}\"",
                fmt_num(grad.focal_point_ratio.clamp(-1.0, 1.0))
            ));
        }
        let m = grad.matrix;
        let mut out = format!(
            "{indent}<{tag}{attrs}>\n{indent}  <matrix>\n{indent}    <Matrix a=\"{}\" b=\"{}\" c=\"{}\" d=\"{}\" tx=\"{}\" ty=\"{}\"/>\n{indent}  </matrix>\n",
            fmt_num(m[0]),
            fmt_num(m[1]),
            fmt_num(m[2]),
            fmt_num(m[3]),
            fmt_num(m[4]),
            fmt_num(m[5])
        );
        for stop in &grad.stops {
            out.push_str(&format!(
                "{indent}  <GradientEntry color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\" ratio=\"{}\"/>\n",
                stop.color[0],
                stop.color[1],
                stop.color[2],
                fmt_num(f64::from(stop.color[3]) / 255.0),
                fmt_num(f64::from(stop.ratio) / 255.0)
            ));
        }
        out.push_str(&format!("{indent}</{tag}>\n"));
        out
    } else {
        format!(
            "{indent}<SolidColor color=\"#{:02X}{:02X}{:02X}\" alpha=\"{}\"/>\n",
            color[0],
            color[1],
            color[2],
            fmt_num(f64::from(color[3]) / 255.0)
        )
    }
}

pub(super) fn path_commands_to_xfl_edges(
    commands: &[PathCommand],
    close_for_fill: bool,
) -> Vec<String> {
    let to_twips = |v: f64| -> i32 { (v * 20.0).round().clamp(-1_000_000.0, 1_000_000.0) as i32 };
    let mut chunks = Vec::new();
    let mut out = String::new();
    let mut cur = [0i32, 0i32];
    let mut contour_start = [0i32, 0i32];
    let mut has_contour = false;

    for cmd in commands {
        // XFL limits each Edge attribute to 65,535 bytes. Each new Edge
        // starts with an absolute move, so curves can span records losslessly.
        if out.len() >= 48_000 {
            if out.contains(['|', '[']) {
                chunks.push(std::mem::take(&mut out));
            } else {
                out.clear();
            }
            out.push_str(&format!("!{} {}", cur[0], cur[1]));
        }
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                if close_for_fill && has_contour && cur != contour_start {
                    out.push_str(&format!("|{} {}", contour_start[0], contour_start[1]));
                }
                cur = [to_twips(x), to_twips(y)];
                contour_start = cur;
                has_contour = true;
                out.push_str(&format!("!{} {}", cur[0], cur[1]));
            }
            PathCommand::LineTo { x, y } => {
                let next = [to_twips(x), to_twips(y)];
                if !has_contour {
                    contour_start = cur;
                    has_contour = true;
                    out.push_str(&format!("!{} {}", cur[0], cur[1]));
                }
                if next != cur {
                    out.push_str(&format!("|{} {}", next[0], next[1]));
                    cur = next;
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let ctrl = [to_twips(cx), to_twips(cy)];
                let next = [to_twips(x), to_twips(y)];
                if !has_contour {
                    contour_start = cur;
                    has_contour = true;
                    out.push_str(&format!("!{} {}", cur[0], cur[1]));
                }
                if next != cur || ctrl != cur {
                    out.push_str(&format!("[{} {} {} {}", ctrl[0], ctrl[1], next[0], next[1]));
                    cur = next;
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
                if !has_contour {
                    contour_start = cur;
                    has_contour = true;
                    out.push_str(&format!("!{} {}", cur[0], cur[1]));
                }
                // Approximate cubic with two quadratics
                let p0 = [cur[0] as f64 / 20.0, cur[1] as f64 / 20.0];
                let c1 = [cx1, cy1];
                let c2 = [cx2, cy2];
                let p3 = [x, y];
                let q1 = [0.25 * p0[0] + 0.75 * c1[0], 0.25 * p0[1] + 0.75 * c1[1]];
                let q2 = [0.25 * p3[0] + 0.75 * c2[0], 0.25 * p3[1] + 0.75 * c2[1]];
                let mid = [0.5 * (q1[0] + q2[0]), 0.5 * (q1[1] + q2[1])];
                let q1_t = [to_twips(q1[0]), to_twips(q1[1])];
                let mid_t = [to_twips(mid[0]), to_twips(mid[1])];
                let q2_t = [to_twips(q2[0]), to_twips(q2[1])];
                let end_t = [to_twips(p3[0]), to_twips(p3[1])];
                if mid_t != cur {
                    out.push_str(&format!(
                        "[{} {} {} {}",
                        q1_t[0], q1_t[1], mid_t[0], mid_t[1]
                    ));
                    cur = mid_t;
                }
                if end_t != cur {
                    out.push_str(&format!(
                        "[{} {} {} {}",
                        q2_t[0], q2_t[1], end_t[0], end_t[1]
                    ));
                    cur = end_t;
                }
            }
            PathCommand::Close => {
                if has_contour && cur != contour_start {
                    out.push_str(&format!("|{} {}", contour_start[0], contour_start[1]));
                    cur = contour_start;
                }
            }
        }
    }
    if close_for_fill && has_contour && cur != contour_start {
        out.push_str(&format!("|{} {}", contour_start[0], contour_start[1]));
    }
    // Fallback if all commands collapsed to a single point
    if chunks.is_empty() && !out.contains(['|', '[']) {
        if close_for_fill {
            return vec![format!(
                "!{} {}|{} {}|{} {}|{} {}",
                contour_start[0],
                contour_start[1],
                contour_start[0] + 20,
                contour_start[1],
                contour_start[0] + 20,
                contour_start[1] + 20,
                contour_start[0],
                contour_start[1]
            )];
        } else {
            return vec![format!(
                "!{} {}|{} {}",
                contour_start[0],
                contour_start[1],
                contour_start[0] + 20,
                contour_start[1]
            )];
        }
    }
    if out.contains(['|', '[']) {
        chunks.push(out);
    }
    chunks
}

pub(super) fn write_matrix_xml(t: &Transform, indent: &str) -> String {
    let m = transform_matrix(*t);
    if (m[0] - 1.0).abs() < 1e-6
        && m[1].abs() < 1e-6
        && m[2].abs() < 1e-6
        && (m[3] - 1.0).abs() < 1e-6
        && m[4].abs() < 1e-6
        && m[5].abs() < 1e-6
    {
        return String::new();
    }
    format!(
        "{indent}<matrix>\n{indent}  <Matrix a=\"{}\" b=\"{}\" c=\"{}\" d=\"{}\" tx=\"{}\" ty=\"{}\"/>\n{indent}</matrix>\n",
        fmt_num(m[0]),
        fmt_num(m[1]),
        fmt_num(m[2]),
        fmt_num(m[3]),
        fmt_num(m[4]),
        fmt_num(m[5])
    )
}

pub(super) fn write_color_transform_xml(t: &Transform, indent: &str) -> String {
    let ct = &t.color_transform;
    if (t.alpha - 1.0).abs() < 1e-6 && ct.is_identity() {
        return String::new();
    }
    if ct.is_identity() {
        return format!(
            "{indent}<color>\n{indent}  <Color alphaMultiplier=\"{}\"/>\n{indent}</color>\n",
            fmt_num(t.alpha.clamp(0.0, 1.0))
        );
    }
    format!(
        "{indent}<color>\n{indent}  <Color alphaMultiplier=\"{}\" redMultiplier=\"{}\" greenMultiplier=\"{}\" blueMultiplier=\"{}\" redOffset=\"{}\" greenOffset=\"{}\" blueOffset=\"{}\" alphaOffset=\"{}\"/>\n{indent}</color>\n",
        fmt_num(t.alpha.clamp(0.0, 1.0)),
        fmt_num(ct.multipliers[0]),
        fmt_num(ct.multipliers[1]),
        fmt_num(ct.multipliers[2]),
        fmt_num(ct.offsets[0]),
        fmt_num(ct.offsets[1]),
        fmt_num(ct.offsets[2]),
        fmt_num(ct.offsets[3])
    )
}

pub(super) fn write_filters_xml(filters: &[FilterSpec], indent: &str) -> String {
    let mut inner = String::new();
    for f in filters {
        let enabled = f.is_enabled();
        let quality_level = f.quality_level().clamp(1, 15);
        match f {
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner: is_inner,
                knockout,
                hide_object,
            } => {
                inner.push_str(&format!(
                    "{indent}  <DropShadowFilter distance=\"{}\" angle=\"{}\" color=\"#{:06X}\" alpha=\"{}\" blurX=\"{}\" blurY=\"{}\" strength=\"{}\" quality=\"{}\" inner=\"{}\" knockout=\"{}\" hideObject=\"{}\" isEnabled=\"{}\"/>\n",
                    fmt_num(*distance),
                    fmt_num(*angle),
                    color & 0xFFFFFF,
                    fmt_num(*alpha),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*strength),
                    quality_level,
                    is_inner,
                    knockout,
                    hide_object,
                    enabled
                ));
            }
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality: _,
            } => {
                inner.push_str(&format!(
                    "{indent}  <BlurFilter blurX=\"{}\" blurY=\"{}\" quality=\"{}\" isEnabled=\"{}\"/>\n",
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    quality_level,
                    enabled
                ));
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                inner: is_inner,
                knockout,
            } => {
                inner.push_str(&format!(
                    "{indent}  <GlowFilter color=\"#{:06X}\" alpha=\"{}\" blurX=\"{}\" blurY=\"{}\" strength=\"{}\" quality=\"{}\" inner=\"{}\" knockout=\"{}\" isEnabled=\"{}\"/>\n",
                    color & 0xFFFFFF,
                    fmt_num(*alpha),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*strength),
                    quality_level,
                    is_inner,
                    knockout,
                    enabled
                ));
            }
            FilterSpec::Bevel {
                distance,
                angle,
                highlight_color,
                highlight_alpha,
                shadow_color,
                shadow_alpha,
                blur_x,
                blur_y,
                strength,
                quality: _,
                placement,
                knockout,
            } => {
                inner.push_str(&format!(
                    "{indent}  <BevelFilter distance=\"{}\" angle=\"{}\" highlightColor=\"#{:06X}\" highlightAlpha=\"{}\" shadowColor=\"#{:06X}\" shadowAlpha=\"{}\" blurX=\"{}\" blurY=\"{}\" strength=\"{}\" quality=\"{}\" type=\"{}\" knockout=\"{}\" isEnabled=\"{}\"/>\n",
                    fmt_num(*distance),
                    fmt_num(*angle),
                    highlight_color & 0xFFFFFF,
                    fmt_num(*highlight_alpha),
                    shadow_color & 0xFFFFFF,
                    fmt_num(*shadow_alpha),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*strength),
                    quality_level,
                    xml_escape(placement),
                    knockout,
                    enabled
                ));
            }
            unsupported => unreachable!(
                "{} is blocked by FLA/XFL preflight before XML serialization",
                filter_kind_name(unsupported)
            ),
        }
    }
    if inner.is_empty() {
        String::new()
    } else {
        format!("{indent}<filters>\n{inner}{indent}</filters>\n")
    }
}

pub(crate) fn skip_leading_ws_and_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(after_line_comment) = s.strip_prefix("//") {
            match after_line_comment.find('\n') {
                Some(nl) => s = &after_line_comment[nl + 1..],
                None => return "",
            }
        } else if let Some(after_block_start) = s.strip_prefix("/*") {
            match after_block_start.find("*/") {
                Some(end) => s = &after_block_start[end + 2..],
                None => return "",
            }
        } else {
            return s;
        }
    }
}

pub(crate) fn script_starts_with_event_wrapper(source: &str) -> bool {
    let s = skip_leading_ws_and_comments(source);
    let after_kw = if let Some(prefix) = s.get(..11)
        && prefix.eq_ignore_ascii_case("onClipEvent")
    {
        let rest = &s[11..];
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        {
            return false;
        }
        rest
    } else if let Some(prefix) = s.get(..2)
        && prefix.eq_ignore_ascii_case("on")
    {
        let rest = &s[2..];
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$'))
        {
            return false;
        }
        rest
    } else {
        return false;
    };
    skip_leading_ws_and_comments(after_kw).starts_with('(')
}

pub(super) fn write_object_scripts_xml(scripts: &[ObjectScript], indent: &str) -> String {
    if scripts.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::with_capacity(scripts.len());
    for os in scripts {
        let marker = match os.script.language {
            ScriptLanguage::JavaScript => "/* neoflash:language=javascript */\n",
            ScriptLanguage::TypeScript => "/* neoflash:language=typescript */\n",
            ScriptLanguage::ActionScript1 => "/* neoflash:language=actionscript1 */\n",
            ScriptLanguage::ActionScript2 => "/* neoflash:language=actionscript2 */\n",
            ScriptLanguage::ActionScript3 => "/* neoflash:language=actionscript3 */\n",
        };
        let trimmed = os.script.source.trim();
        if script_starts_with_event_wrapper(trimmed) {
            blocks.push(format!("{marker}{trimmed}"));
        } else {
            let header = match os.event {
                ObjectEvent::Click => "on (release) {\n",
                ObjectEvent::Load => "onClipEvent (load) {\n",
                ObjectEvent::EnterFrame => "onClipEvent (enterFrame) {\n",
            };
            blocks.push(format!("{header}{marker}{}\n}}", os.script.source));
        }
    }
    format!(
        "{indent}<Actionscript>\n{indent}  <script>{}</script>\n{indent}</Actionscript>\n",
        xml_escape(&blocks.join("\n"))
    )
}

pub(super) fn sanitize_xfl_item_name(raw: &str, fallback: &str) -> String {
    let base = raw.rsplit_once('/').map_or(raw, |(_, b)| b).trim();
    let mut out = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn sanitize_source_rel_path(raw: &str, idx: usize) -> String {
    let clean: Vec<String> = raw
        .split(['/', '\\'])
        .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
        .map(|seg| {
            seg.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .collect();
    let mut joined = if clean.is_empty() {
        format!("script_{}.as", idx + 1)
    } else {
        clean.join("/")
    };
    let lower = joined.to_ascii_lowercase();
    if !lower.ends_with(".as") && !lower.ends_with(".js") && !lower.ends_with(".ts") {
        joined.push_str(".as");
    }
    joined
}

pub(super) fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let rounded = (v * 10000.0).round() / 10000.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

pub(super) fn xml_escape(s: &str) -> String {
    crate::xml_escape(s)
}
