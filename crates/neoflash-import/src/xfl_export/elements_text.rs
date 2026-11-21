//! XFL symbol instance, filter, color, media, and text element XML serialization.
use super::*;

/// Renders one `Element` to zero, one, or more XFL `<elements>` child blocks and returns
/// `(xml_blocks, is_vector_shape)`.
pub(super) fn render_element(
    element: &Element,
    bitmap_fills: &[AuthoredBitmapFill],
    frame_video_resource: Option<&str>,
    ctx: &ExportContext,
) -> (Vec<String>, bool) {
    let ct = element
        .transform
        .color_transform
        .with_alpha(element.transform.alpha);
    let fill_bitmap = bitmap_fills
        .iter()
        .find(|bf| bf.element_id == element.id)
        .and_then(|bf| {
            ctx.bitmaps
                .get(&bf.resource_id)
                .map(|bmp_info| (bmp_info.item_name.clone(), bf.matrix, !bf.repeat))
        });

    match &element.kind {
        ElementKind::Instance {
            symbol_id,
            playback,
        } => {
            let Some(sym_info) = ctx.symbols.get(symbol_id) else {
                return (Vec::new(), false);
            };
            (
                vec![render_symbol_instance(element, sym_info, *playback, ct)],
                false,
            )
        }
        ElementKind::Bitmap { resource_id, .. } => {
            let Some(bmp_info) = ctx.bitmaps.get(resource_id) else {
                return (Vec::new(), false);
            };
            (vec![render_bitmap_instance(element, bmp_info)], false)
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => (
            vec![render_plain_static_text(
                element,
                text,
                font_family,
                *font_size,
                ct.apply(*color),
            )],
            false,
        ),
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } => (
            vec![render_textbox_element(
                element, text, *width, *height, *word_wrap, *multiline, format, ct,
            )],
            false,
        ),
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if !width.is_finite() || !height.is_finite() || *width <= 0.0 || *height <= 0.0 {
                return (Vec::new(), true);
            }
            let eff_fill = ct.apply(*fill);
            let eff_stroke = ct.apply(*stroke);
            let has_stroke = *stroke_width > 0.0 && eff_stroke[3] > 0;
            if !has_stroke && !element.has_parametric_options() && fill_bitmap.is_none() {
                (
                    vec![render_primitive_object(
                        "DOMRectangleObject",
                        element,
                        *width,
                        *height,
                        eff_fill,
                    )],
                    true,
                )
            } else if let Some((commands, _)) = element.to_path_commands() {
                (
                    render_dom_shapes(
                        element,
                        &commands,
                        ShapePaintSpec {
                            fill_color: eff_fill,
                            fill_gradient: None,
                            fill_bitmap,
                            stroke_color: eff_stroke,
                            stroke_gradient: None,
                            stroke_width: *stroke_width,
                        },
                    ),
                    true,
                )
            } else {
                (Vec::new(), true)
            }
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if !width.is_finite() || !height.is_finite() || *width <= 0.0 || *height <= 0.0 {
                return (Vec::new(), true);
            }
            let close_path = element.oval_options().close_path;
            let eff_fill = if close_path {
                ct.apply(*fill)
            } else {
                [0, 0, 0, 0]
            };
            let eff_stroke = ct.apply(*stroke);
            let has_stroke = *stroke_width > 0.0 && eff_stroke[3] > 0;
            if !has_stroke && !element.has_parametric_options() && fill_bitmap.is_none() {
                (
                    vec![render_primitive_object(
                        "DOMOvalObject",
                        element,
                        *width,
                        *height,
                        eff_fill,
                    )],
                    true,
                )
            } else if let Some((commands, _)) = element.to_path_commands() {
                (
                    render_dom_shapes(
                        element,
                        &commands,
                        ShapePaintSpec {
                            fill_color: eff_fill,
                            fill_gradient: None,
                            fill_bitmap: close_path.then_some(fill_bitmap).flatten(),
                            stroke_color: eff_stroke,
                            stroke_gradient: None,
                            stroke_width: *stroke_width,
                        },
                    ),
                    true,
                )
            } else {
                (Vec::new(), true)
            }
        }
        ElementKind::Path {
            commands,
            fill,
            stroke,
            stroke_width,
            ..
        } => (
            render_dom_shapes(
                element,
                commands,
                ShapePaintSpec {
                    fill_color: ct.apply(*fill),
                    fill_gradient: None,
                    fill_bitmap,
                    stroke_color: ct.apply(*stroke),
                    stroke_gradient: None,
                    stroke_width: *stroke_width,
                },
            ),
            true,
        ),
        ElementKind::PaintedPath {
            commands,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => (
            render_dom_shapes(
                element,
                commands,
                ShapePaintSpec {
                    fill_color: ct.apply(*fill),
                    fill_gradient: fill_gradient.as_ref().map(|g| transform_gradient(g, ct)),
                    fill_bitmap,
                    stroke_color: ct.apply(*stroke),
                    stroke_gradient: stroke_gradient.as_ref().map(|g| transform_gradient(g, ct)),
                    stroke_width: *stroke_width,
                },
            ),
            true,
        ),
        ElementKind::Video { width, height } => {
            let eff_res = frame_video_resource
                .filter(|rid| ctx.embedded_video_ids.contains(*rid))
                .map(str::to_owned);
            let key = (eff_res, video_size_key(*width, *height));
            if let Some(vid_info) = ctx.videos.get(&key) {
                (vec![render_video_instance(element, vid_info)], false)
            } else {
                (Vec::new(), false)
            }
        }
    }
}

pub(super) fn transform_gradient(grad: &GradientPaint, ct: ColorTransform) -> GradientPaint {
    GradientPaint {
        kind: grad.kind,
        matrix: grad.matrix,
        stops: grad
            .stops
            .iter()
            .map(|s| GradientStop {
                ratio: s.ratio,
                color: ct.apply(s.color),
            })
            .collect(),
        focal_point_ratio: grad.focal_point_ratio,
        spread: grad.spread,
        linear_rgb: grad.linear_rgb,
    }
}

pub(super) fn render_symbol_instance(
    element: &Element,
    sym_info: &SymbolExport,
    playback: InstancePlayback,
    ct: ColorTransform,
) -> String {
    let mut xml = String::with_capacity(256);
    write!(
        xml,
        "<DOMSymbolInstance libraryItemName=\"{}\" name=\"{}\"",
        escape_xml_attr(&sym_info.item_name),
        escape_xml_attr(&element.name),
    )
    .unwrap();

    let write_graphic_loop = |xml: &mut String, playback: InstancePlayback| match playback {
        InstancePlayback::Independent | InstancePlayback::Loop => {
            xml.push_str(" symbolType=\"graphic\" loop=\"loop\"");
        }
        InstancePlayback::LoopFrom(first) => {
            xml.push_str(" symbolType=\"graphic\" loop=\"loop\"");
            if first > 0 {
                write!(xml, " firstFrame=\"{first}\"").unwrap();
            }
        }
        InstancePlayback::PlayOnce => {
            xml.push_str(" symbolType=\"graphic\" loop=\"play once\"");
        }
        InstancePlayback::PlayOnceFrom(first) => {
            xml.push_str(" symbolType=\"graphic\" loop=\"play once\"");
            if first > 0 {
                write!(xml, " firstFrame=\"{first}\"").unwrap();
            }
        }
        InstancePlayback::SingleFrame(first) => {
            xml.push_str(" symbolType=\"graphic\" loop=\"single frame\"");
            if first > 0 {
                write!(xml, " firstFrame=\"{first}\"").unwrap();
            }
        }
    };
    match element.instance_behavior {
        Some(SymbolKind::Button) => {
            xml.push_str(" symbolType=\"button\"");
        }
        Some(SymbolKind::MovieClip) => {
            xml.push_str(" symbolType=\"movie clip\"");
        }
        Some(SymbolKind::Graphic) => {
            write_graphic_loop(&mut xml, playback);
        }
        None => match sym_info.kind {
            SymbolKind::Button => {
                xml.push_str(" symbolType=\"button\"");
            }
            SymbolKind::MovieClip | SymbolKind::Graphic => match playback {
                InstancePlayback::Independent => {
                    xml.push_str(" symbolType=\"movie clip\"");
                }
                other => {
                    write_graphic_loop(&mut xml, other);
                }
            },
        },
    }

    if !element.visible {
        xml.push_str(" isVisible=\"false\"");
    }
    if element.cache_as_bitmap {
        xml.push_str(" cacheAsBitmap=\"true\"");
    }
    if !element.blend_mode.is_normal() {
        write!(xml, " blendMode=\"{}\"", element.blend_mode.as_str()).unwrap();
    }
    if let Some(acc) = &element.accessibility {
        xml.push_str(" hasAccessibleData=\"true\"");
        if !acc.name.is_empty() {
            write!(xml, " accName=\"{}\"", escape_xml_attr(&acc.name)).unwrap();
        }
        if !acc.description.is_empty() {
            write!(
                xml,
                " description=\"{}\"",
                escape_xml_attr(&acc.description)
            )
            .unwrap();
        }
        if acc.tab_order > 0 {
            write!(xml, " tabIndex=\"{}\"", acc.tab_order).unwrap();
        }
        if acc.disabled {
            xml.push_str(" silent=\"true\"");
        }
    }

    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    append_instance_color(&mut children, ct);
    append_instance_filters(&mut children, &element.filters);
    append_object_actionscript(&mut children, &element.scripts);

    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        xml.push_str("</DOMSymbolInstance>");
    }
    xml
}

pub(super) fn append_instance_filters(out: &mut String, filters: &[FilterSpec]) {
    if filters.is_empty() {
        return;
    }
    out.push_str("  <filters>\n");
    for filter in filters {
        match filter {
            FilterSpec::Blur {
                blur_x,
                blur_y,
                quality,
            } => {
                writeln!(
                    out,
                    "    <BlurFilter blurX=\"{}\" blurY=\"{}\" quality=\"{quality}\"/>",
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                )
                .unwrap();
            }
            FilterSpec::Glow {
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
            } => {
                write!(
                    out,
                    "    <GlowFilter blurX=\"{}\" blurY=\"{}\" color=\"#{:06X}\" alpha=\"{}\" strength=\"{}\" quality=\"{quality}\"",
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    color & 0xff_ffff,
                    fmt_num(*alpha),
                    fmt_num(*strength),
                )
                .unwrap();
                if *inner {
                    out.push_str(" inner=\"true\"");
                }
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                out.push_str("/>\n");
            }
            FilterSpec::DropShadow {
                distance,
                angle,
                color,
                alpha,
                blur_x,
                blur_y,
                strength,
                quality,
                inner,
                knockout,
                hide_object,
            } => {
                write!(
                    out,
                    "    <DropShadowFilter angle=\"{}\" blurX=\"{}\" blurY=\"{}\" distance=\"{}\" color=\"#{:06X}\" alpha=\"{}\" strength=\"{}\" quality=\"{quality}\"",
                    fmt_num(*angle),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*distance),
                    color & 0xff_ffff,
                    fmt_num(*alpha),
                    fmt_num(*strength),
                )
                .unwrap();
                if *inner {
                    out.push_str(" inner=\"true\"");
                }
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                if *hide_object {
                    out.push_str(" hideObject=\"true\"");
                }
                out.push_str("/>\n");
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
                quality,
                placement,
                knockout,
            } => {
                write!(
                    out,
                    "    <BevelFilter angle=\"{}\" blurX=\"{}\" blurY=\"{}\" distance=\"{}\" highlightColor=\"#{:06X}\" highlightAlpha=\"{}\" shadowColor=\"#{:06X}\" shadowAlpha=\"{}\" strength=\"{}\" quality=\"{quality}\" type=\"{}\"",
                    fmt_num(*angle),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*distance),
                    highlight_color & 0xff_ffff,
                    fmt_num(*highlight_alpha),
                    shadow_color & 0xff_ffff,
                    fmt_num(*shadow_alpha),
                    fmt_num(*strength),
                    escape_xml_attr(placement),
                )
                .unwrap();
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                out.push_str("/>\n");
            }
            FilterSpec::GradientGlow {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            }
            | FilterSpec::GradientBevel {
                distance,
                angle,
                colors,
                alphas,
                ratios,
                blur_x,
                blur_y,
                strength,
                quality,
                placement,
                knockout,
            } => {
                let tag = if matches!(filter, FilterSpec::GradientGlow { .. }) {
                    "GradientGlowFilter"
                } else {
                    "GradientBevelFilter"
                };
                write!(
                    out,
                    "    <{tag} angle=\"{}\" blurX=\"{}\" blurY=\"{}\" distance=\"{}\" strength=\"{}\" quality=\"{quality}\" type=\"{}\"",
                    fmt_num(*angle),
                    fmt_num(*blur_x),
                    fmt_num(*blur_y),
                    fmt_num(*distance),
                    fmt_num(*strength),
                    escape_xml_attr(placement),
                )
                .unwrap();
                if *knockout {
                    out.push_str(" knockout=\"true\"");
                }
                out.push_str(">\n");
                let count = colors.len().min(alphas.len()).min(ratios.len());
                for i in 0..count {
                    writeln!(
                        out,
                        "      <GradientEntry color=\"#{:06X}\" alpha=\"{}\" ratio=\"{}\"/>",
                        colors[i] & 0xff_ffff,
                        fmt_num(alphas[i]),
                        fmt_num(f64::from(ratios[i]) / 255.0),
                    )
                    .unwrap();
                }
                writeln!(out, "    </{tag}>").unwrap();
            }
            FilterSpec::ColorMatrix { .. } | FilterSpec::Convolution { .. } => {
                out.push_str(
                    "    <AdjustColorFilter brightness=\"0\" contrast=\"0\" saturation=\"0\" hue=\"0\"/>\n",
                );
            }
        }
    }
    out.push_str("  </filters>\n");
}

pub(super) fn append_instance_color(out: &mut String, ct: ColorTransform) {
    let m = ct.multipliers.map(|v| if v.is_finite() { v } else { 1.0 });
    let o = ct.offsets.map(|v| if v.is_finite() { v } else { 0.0 });
    let rgb_identity = (m[0] - 1.0).abs() <= 1e-6
        && (m[1] - 1.0).abs() <= 1e-6
        && (m[2] - 1.0).abs() <= 1e-6
        && o.iter().all(|v| v.abs() <= 1e-6);
    let alpha_identity = (m[3] - 1.0).abs() <= 1e-6;
    if rgb_identity && alpha_identity {
        return;
    }
    out.push_str("  <color>\n    <Color");
    if !alpha_identity {
        write!(out, " alphaMultiplier=\"{}\"", fmt_num(m[3])).unwrap();
    }
    if !rgb_identity {
        for (val, attr) in [
            (m[0], "redMultiplier"),
            (m[1], "greenMultiplier"),
            (m[2], "blueMultiplier"),
        ] {
            if (val - 1.0).abs() > 1e-6 {
                write!(out, " {attr}=\"{}\"", fmt_num(val)).unwrap();
            }
        }
        for (val, attr) in [
            (o[3], "alphaOffset"),
            (o[0], "redOffset"),
            (o[1], "greenOffset"),
            (o[2], "blueOffset"),
        ] {
            if val.abs() > 1e-6 {
                write!(out, " {attr}=\"{}\"", fmt_num(val)).unwrap();
            }
        }
    }
    out.push_str("/>\n  </color>\n");
}

pub(super) fn append_object_actionscript(out: &mut String, scripts: &[ObjectScript]) {
    let mut blocks: Vec<String> = Vec::new();
    for os in scripts {
        let trimmed = os.script.source.trim();
        if trimmed.is_empty() {
            continue;
        }
        let body = encode_script_with_language_tag(&os.script);
        if is_actionscript(os.script.language) && starts_with_flash_handler(trimmed) {
            blocks.push(body);
        } else {
            let header = match os.event {
                ObjectEvent::Click => "on (release)",
                ObjectEvent::Load => "onClipEvent (load)",
                ObjectEvent::EnterFrame => "onClipEvent (enterFrame)",
            };
            blocks.push(format!("{header} {{\n{body}\n}}"));
        }
    }
    if !blocks.is_empty() {
        let joined = blocks.join("\n");
        writeln!(
            out,
            "  <Actionscript>\n    <script>{}</script>\n  </Actionscript>",
            cdata_wrap(&joined)
        )
        .unwrap();
    }
}

pub(super) fn starts_with_flash_handler(trimmed: &str) -> bool {
    for prefix in ["on", "onClipEvent"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && rest.trim_start().starts_with('(')
        {
            return true;
        }
    }
    false
}

pub(super) fn render_bitmap_instance(element: &Element, bmp_info: &BitmapExport) -> String {
    let mut xml = String::with_capacity(160);
    write!(
        xml,
        "<DOMBitmapInstance libraryItemName=\"{}\"",
        escape_xml_attr(&bmp_info.item_name)
    )
    .unwrap();
    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        xml.push_str("</DOMBitmapInstance>");
    }
    xml
}

pub(super) fn render_video_instance(element: &Element, vid_info: &VideoExport) -> String {
    let mut xml = String::with_capacity(180);
    write!(
        xml,
        "<DOMVideoInstance libraryItemName=\"{}\"",
        escape_xml_attr(&vid_info.item_name)
    )
    .unwrap();
    if !element.name.trim().is_empty() {
        write!(xml, " name=\"{}\"", escape_xml_attr(&element.name)).unwrap();
    }
    write!(
        xml,
        " frameLeft=\"0\" frameRight=\"{}\" frameTop=\"0\" frameBottom=\"{}\"",
        fmt_num(vid_info.width * 20.0),
        fmt_num(vid_info.height * 20.0)
    )
    .unwrap();
    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        xml.push_str("</DOMVideoInstance>");
    }
    xml
}

pub(super) fn render_plain_static_text(
    element: &Element,
    text: &str,
    font_family: &str,
    font_size: f64,
    rgba: Color,
) -> String {
    let size = if font_size.is_finite() {
        font_size.clamp(1.0, 10_000.0)
    } else {
        12.0
    };
    let face = if font_family.trim().is_empty() {
        "Arial"
    } else {
        font_family
    };
    let char_count = text.chars().count().max(1) as f64;
    let width = (char_count * size * 0.6).max(4.0);
    let height = (size * 1.2).max(4.0);

    let mut xml = String::with_capacity(320);
    writeln!(
        xml,
        "<DOMStaticText name=\"{}\" width=\"{}\" height=\"{}\" isSelectable=\"false\">",
        escape_xml_attr(&element.name),
        fmt_num(width),
        fmt_num(height),
    )
    .unwrap();
    append_matrix_and_pivot(&mut xml, element.transform);
    xml.push_str("  <textRuns>\n    <DOMTextRun>\n");
    writeln!(
        xml,
        "      <characters>{}</characters>",
        escape_xfl_characters(text)
    )
    .unwrap();
    xml.push_str("      <textAttrs>\n");
    let (hex, alpha_opt) = fmt_color_alpha(rgba);
    let bitmap_size = (size * 20.0).round().clamp(20.0, 200_000.0) as u32;
    write!(
        xml,
        "        <DOMTextAttrs face=\"{}\" size=\"{}\" bitmapSize=\"{bitmap_size}\" fillColor=\"{hex}\"",
        escape_xml_attr(face),
        fmt_num(size),
    )
    .unwrap();
    if let Some(alpha_str) = alpha_opt {
        write!(xml, " alpha=\"{alpha_str}\"").unwrap();
    }
    xml.push_str("/>\n      </textAttrs>\n    </DOMTextRun>\n  </textRuns>\n</DOMStaticText>");
    xml
}

pub(super) fn slice_utf16_runs<'a>(
    text: &'a str,
    base_format: &'a TextFormat,
    runs: &'a [TextFormatRun],
) -> Vec<(&'a str, &'a TextFormat)> {
    if runs.is_empty() || text.is_empty() {
        return vec![(text, base_format)];
    }
    let mut utf16_to_byte: HashMap<u32, usize> = HashMap::new();
    let mut u16_pos = 0u32;
    for (byte_idx, ch) in text.char_indices() {
        utf16_to_byte.insert(u16_pos, byte_idx);
        u16_pos = u16_pos.saturating_add(ch.len_utf16() as u32);
    }
    utf16_to_byte.insert(u16_pos, text.len());

    let mut cursor = 0u32;
    let mut out = Vec::with_capacity(runs.len());
    for run in runs {
        if run.start != cursor || run.end <= run.start || run.end > u16_pos {
            return vec![(text, base_format)];
        }
        let (Some(&b_start), Some(&b_end)) =
            (utf16_to_byte.get(&run.start), utf16_to_byte.get(&run.end))
        else {
            return vec![(text, base_format)];
        };
        out.push((&text[b_start..b_end], &run.format));
        cursor = run.end;
    }
    if cursor != u16_pos {
        return vec![(text, base_format)];
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_textbox_element(
    element: &Element,
    text: &str,
    width: f64,
    height: f64,
    word_wrap: bool,
    multiline: bool,
    format: &TextFormat,
    ct: ColorTransform,
) -> String {
    let base_size = if format.size.is_finite() {
        format.size.clamp(1.0, 10_000.0)
    } else {
        12.0
    };
    let w = if width.is_finite() && width > 0.0 {
        width
    } else {
        100.0
    };
    let h = if height.is_finite() && height > 0.0 {
        height
    } else {
        (base_size * 1.2).max(4.0)
    };

    let opts: Option<AuthoredTextFieldOptions> = format.field_options();
    let is_input = opts
        .as_ref()
        .is_some_and(|o| o.field_type == TextFieldType::Input);
    let has_dynamic_opts = opts.as_ref().is_some_and(|o| {
        o.variable.is_some()
            || o.border
            || o.background
            || !o.selectable
            || o.auto_size != "none"
            || o.html
            || o.embed_fonts
            || o.display_as_password
            || o.max_chars > 0
            || o.direction != "ltr"
    });
    let tag = if is_input {
        "DOMInputText"
    } else if has_dynamic_opts
        || (opts.is_none()
            && !text.contains(['\r', '\n'])
            && format.is_plain_point_text_compatible())
    {
        "DOMDynamicText"
    } else {
        "DOMStaticText"
    };

    let selectable = if tag == "DOMStaticText" {
        false
    } else {
        opts.as_ref().is_none_or(|o| o.selectable)
    };

    let mut xml = String::with_capacity(420);
    write!(
        xml,
        "<{tag} name=\"{}\" width=\"{}\" height=\"{}\" isSelectable=\"{selectable}\"",
        escape_xml_attr(&element.name),
        fmt_num(w),
        fmt_num(h),
    )
    .unwrap();

    let password = opts.as_ref().is_some_and(|o| o.display_as_password);
    if password {
        xml.push_str(" lineType=\"password\"");
    } else if multiline {
        if word_wrap {
            xml.push_str(" lineType=\"multiline\"");
        } else {
            xml.push_str(" lineType=\"multiline no wrap\"");
        }
    }

    if let Some(o) = &opts {
        if o.html {
            xml.push_str(" renderAsHTML=\"true\"");
        }
        if tag != "DOMStaticText" && (o.border || o.background) {
            xml.push_str(" border=\"true\"");
        }
        if tag != "DOMStaticText"
            && let Some(var_name) = o.variable.as_deref().filter(|s| !s.is_empty())
        {
            write!(xml, " variableName=\"{}\"", escape_xml_attr(var_name)).unwrap();
        }
        if tag != "DOMStaticText" && o.max_chars > 0 {
            write!(xml, " maxCharacters=\"{}\"", o.max_chars).unwrap();
        }
        if tag != "DOMStaticText" && o.auto_size != "none" && !o.auto_size.is_empty() {
            xml.push_str(" autoExpand=\"true\"");
        }
        if tag != "DOMStaticText" && o.embed_fonts {
            xml.push_str(" includeOutlines=\"true\"");
        }
        if o.orientation == "vertical" {
            xml.push_str(" orientation=\"vertical left to right\"");
        }
        if o.anti_alias_type == "advanced" {
            if o.sharpness.abs() > 1e-6 || o.thickness.abs() > 1e-6 {
                write!(
                    xml,
                    " fontRenderingMode=\"customThicknessSharpness\" antiAliasSharpness=\"{}\" antiAliasThickness=\"{}\"",
                    fmt_num(o.sharpness),
                    fmt_num(o.thickness),
                )
                .unwrap();
            } else {
                xml.push_str(" fontRenderingMode=\"advanced\"");
            }
        }
    }
    xml.push_str(">\n");
    append_matrix_and_pivot(&mut xml, element.transform);

    let runs_slice = opts
        .as_ref()
        .map(|o| o.format_runs.as_slice())
        .unwrap_or(&[]);
    let slices = slice_utf16_runs(text, format, runs_slice);
    let char_pos = opts.as_ref().and_then(|o| {
        if o.superscript {
            Some("superscript")
        } else if o.subscript {
            Some("subscript")
        } else {
            None
        }
    });

    xml.push_str("  <textRuns>\n");
    for (run_text, run_fmt) in slices {
        let size = if run_fmt.size.is_finite() {
            run_fmt.size.clamp(1.0, 10_000.0)
        } else {
            12.0
        };
        let face = if run_fmt.font.trim().is_empty() {
            "Times New Roman"
        } else {
            run_fmt.font.as_str()
        };
        let run_rgba = ct.apply([
            ((run_fmt.color >> 16) & 0xff) as u8,
            ((run_fmt.color >> 8) & 0xff) as u8,
            (run_fmt.color & 0xff) as u8,
            255,
        ]);
        let (hex, alpha_opt) = fmt_color_alpha(run_rgba);
        let bitmap_size = (size * 20.0).round().clamp(20.0, 200_000.0) as u32;

        xml.push_str("    <DOMTextRun>\n");
        writeln!(
            xml,
            "      <characters>{}</characters>",
            escape_xfl_characters(run_text)
        )
        .unwrap();
        xml.push_str("      <textAttrs>\n");
        write!(
            xml,
            "        <DOMTextAttrs face=\"{}\" size=\"{}\" bitmapSize=\"{bitmap_size}\" fillColor=\"{hex}\"",
            escape_xml_attr(face),
            fmt_num(size),
        )
        .unwrap();
        if let Some(alpha_str) = alpha_opt {
            write!(xml, " alpha=\"{alpha_str}\"").unwrap();
        }
        if run_fmt.bold {
            xml.push_str(" bold=\"true\"");
        }
        if run_fmt.italic {
            xml.push_str(" italic=\"true\"");
        }
        if run_fmt.underline {
            xml.push_str(" underline=\"true\"");
        }
        if run_fmt.kerning == Some(true) {
            xml.push_str(" autoKern=\"true\"");
        }
        if let Some(pos) = char_pos {
            write!(xml, " characterPosition=\"{pos}\"").unwrap();
        }
        if run_fmt.align_mode() == Some("justify") {
            xml.push_str(" alignment=\"justify\"");
        } else {
            match run_fmt.align {
                TextAlign::Left => {}
                TextAlign::Center => xml.push_str(" alignment=\"center\""),
                TextAlign::Right => xml.push_str(" alignment=\"right\""),
            }
        }
        for (val, attr) in [
            (run_fmt.leading, "lineSpacing"),
            (run_fmt.letter_spacing, "letterSpacing"),
            (run_fmt.left_margin, "leftMargin"),
            (run_fmt.right_margin, "rightMargin"),
            (run_fmt.indent, "indent"),
        ] {
            if val.is_finite() && val.abs() > 1e-6 {
                write!(xml, " {attr}=\"{}\"", fmt_num(val)).unwrap();
            }
        }
        if !run_fmt.url.is_empty() {
            write!(xml, " url=\"{}\"", escape_xml_attr(&run_fmt.url)).unwrap();
        }
        let target_base = run_fmt.target_str();
        if !target_base.is_empty() {
            write!(xml, " target=\"{}\"", escape_xml_attr(target_base)).unwrap();
        }
        xml.push_str("/>\n      </textAttrs>\n    </DOMTextRun>\n");
    }
    writeln!(xml, "  </textRuns>\n</{tag}>").unwrap();
    if xml.ends_with('\n') {
        xml.pop();
    }
    xml
}

pub(super) fn render_primitive_object(
    tag: &str,
    element: &Element,
    width: f64,
    height: f64,
    fill: Color,
) -> String {
    let mut xml = String::with_capacity(220);
    write!(
        xml,
        "<{tag} name=\"{}\" objectWidth=\"{}\" objectHeight=\"{}\"",
        escape_xml_attr(&element.name),
        fmt_num(width),
        fmt_num(height),
    )
    .unwrap();

    let mut children = String::new();
    append_matrix_and_pivot(&mut children, element.transform);
    if fill[3] > 0 {
        let (hex, alpha_opt) = fmt_color_alpha(fill);
        children.push_str("  <fill>\n");
        write!(children, "    <SolidColor color=\"{hex}\"").unwrap();
        if let Some(a) = alpha_opt {
            write!(children, " alpha=\"{a}\"").unwrap();
        }
        children.push_str("/>\n  </fill>\n");
    }

    if children.is_empty() {
        xml.push_str("/>");
    } else {
        xml.push_str(">\n");
        xml.push_str(&children);
        write!(xml, "</{tag}>").unwrap();
    }
    xml
}
