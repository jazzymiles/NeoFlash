use super::*;

pub(super) const KNOWN_CPIC_TEXT_FLAGS: u32 = 0x0003_ffff;
pub(super) const KNOWN_FONT_TECH_FLAGS: u32 = 0xc03f_0061;

pub(super) fn convert_accessibility(
    stream: &BinaryTimelineStream,
    acc: Option<&TimelineAccessibility>,
    fallback_name: &str,
    role: AccessibleRole,
) -> Result<Option<Accessibility>, String> {
    let Some(acc) = acc else {
        return Ok(None);
    };
    if acc.revision == 0 {
        return Ok(None);
    }
    let raw_name = acc
        .historical_wide_name
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            acc.name
                .as_ref()
                .and_then(|sp| optional_text(&stream.original_bytes, Some(sp)).ok())
        })
        .unwrap_or_default();
    let raw_desc = acc
        .historical_wide_description
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            acc.description
                .as_ref()
                .and_then(|sp| optional_text(&stream.original_bytes, Some(sp)).ok())
        })
        .unwrap_or_default();
    let raw_shortcut = optional_text(&stream.original_bytes, acc.shortcut.as_ref())?;
    let raw_tab = optional_text(&stream.original_bytes, acc.tab_index.as_ref())?;
    let tab_order = raw_tab.trim().parse::<u32>().unwrap_or(0).min(10_000);
    let disabled = acc.flags.unwrap_or(0) != 0;
    if raw_name.trim().is_empty()
        && raw_desc.trim().is_empty()
        && raw_shortcut.trim().is_empty()
        && tab_order == 0
        && !disabled
    {
        return Ok(None);
    }
    let chosen_name = if !raw_name.trim().is_empty() {
        raw_name.trim()
    } else if !fallback_name.trim().is_empty() {
        fallback_name.trim()
    } else {
        "Accessible Item"
    };
    let mut name: String = chosen_name
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    if name.trim().is_empty() {
        name = "Accessible Item".to_owned();
    }
    let description: String = raw_desc
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(4096)
        .collect();
    Ok(Some(Accessibility {
        name,
        description,
        role,
        tab_order,
        disabled,
    }))
}

pub(super) fn decode_cpic_text_style(
    stream: &BinaryTimelineStream,
    style: &TimelineTextStyle,
    single_line_inert_leading: bool,
    lossy: &mut Vec<&'static str>,
) -> Result<(TextFormat, f64), String> {
    if style.revision < 2 {
        return Err(
            "Text has no archived font or size; a revision-only style cannot be reconstructed."
                .into(),
        );
    }
    if style.rotation.unwrap_or(0) != 0 {
        lossy.push("rotated characters are laid out horizontally");
    }
    let face = optional_text_with_charset(
        &stream.original_bytes,
        style.font_face.as_ref(),
        style.char_set,
    )?;
    let family = optional_text_with_charset(
        &stream.original_bytes,
        style.font_family.as_ref(),
        style.char_set,
    )?;
    let [red, green, blue, alpha] = style
        .color_rgba
        .ok_or("Text style has no archived color")?
        .to_le_bytes();
    let (align, align_mode) = match style.alignment.unwrap_or(0) {
        0 => (TextAlign::Left, None),
        1 => (TextAlign::Right, None),
        2 => (TextAlign::Center, None),
        3 => (TextAlign::Left, Some("justify".to_owned())),
        _ => {
            lossy.push("unknown alignment is shown left-aligned");
            (TextAlign::Left, None)
        }
    };
    let twips = |value: Option<i16>| f64::from(value.unwrap_or(0)) / 20.;
    let mut value = TextFormat {
        font: if face.is_empty() { family } else { face },
        size: f64::from(
            style
                .font_size_twips
                .ok_or("Text style has no archived size")?,
        ) / 20.,
        color: (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue),
        bold: style.bold.unwrap_or(0) != 0,
        italic: style.italic.unwrap_or(0) != 0,
        leading: twips(style.line_spacing),
        align,
        left_margin: twips(style.left_margin),
        right_margin: twips(style.right_margin),
        indent: twips(style.indent),
        letter_spacing: twips(style.letter_spacing),
        kerning: style.auto_kern.map(|value| value != 0),
        url: optional_text(
            &stream.original_bytes,
            style.url_override.as_ref().or(style.url.as_ref()),
        )?,
        target: optional_text(&stream.original_bytes, style.target.as_ref())?,
        ..TextFormat::default()
    };
    // Single-line auto-expanding Static Point Text has only one line and never
    // wraps or edits at runtime, so negative Flash CS3/CS4 paragraph leading on it
    // has zero visual or semantic effect.
    if single_line_inert_leading
        && (1.0..=9999.0).contains(&value.size)
        && value.size * 1.2 + value.leading <= 0.0
    {
        value.leading = 0.0;
    }
    let target_needs_clean = value.target.len() > 256 || value.target.chars().any(char::is_control);
    if value.validate().is_err() || target_needs_clean {
        if value.font.trim().is_empty()
            || value.font.len() > 1024
            || value.font.chars().any(char::is_control)
        {
            let cleaned: String = value
                .font
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect();
            value.font = if cleaned.trim().is_empty() {
                "Times New Roman".into()
            } else {
                cleaned
            };
        }
        if value.url.len() > 4096 || value.url.chars().any(char::is_control) {
            value.url = clean_bounded_text(&value.url, 4096);
        }
        if target_needs_clean {
            value.target = clean_bounded_text(&value.target, 256);
        }
        value.size = value.size.clamp(1., 9999.);
        value.leading = value.leading.clamp(-9999., 9999.);
        value.left_margin = value.left_margin.clamp(0., 9999.);
        value.right_margin = value.right_margin.clamp(0., 9999.);
        value.indent = value.indent.clamp(-9999., 9999.);
        value.letter_spacing = value.letter_spacing.clamp(-1000., 1000.);
        if value.size * 1.2 + value.leading <= 0. {
            value.leading = (-value.size * 1.2 + 0.5).clamp(-9999., 9999.);
        }
        lossy.push("out-of-range size, spacing or margins are clamped");
    }
    if let Some(mode) = align_mode {
        value.set_align_mode(Some(mode));
    }
    value.validate().map_err(|error| error.to_string())?;
    Ok((
        value,
        if style.revision < 4 {
            1.
        } else {
            f64::from(alpha) / 255.
        },
    ))
}

pub(super) fn clean_bounded_text(value: &str, max_bytes: usize) -> String {
    let mut cleaned = String::with_capacity(value.len().min(max_bytes));
    for character in value.chars().filter(|character| !character.is_control()) {
        if cleaned.len() + character.len_utf8() > max_bytes {
            break;
        }
        cleaned.push(character);
    }
    cleaned
}

pub(in crate::legacy) fn convert_timeline_text(
    stream: &BinaryTimelineStream,
    text: &TimelineText,
    id: &str,
    name: &str,
    _contents_revision: Option<u8>,
    report: &mut ImportReport,
) -> Result<Element, String> {
    let flags = text.flags.unwrap_or(0);
    let is_dynamic_or_input = (flags & 0x0001) != 0;
    let is_input = is_dynamic_or_input && (flags & 0x0002) == 0;
    let display_as_password = is_dynamic_or_input && (flags & 0x0004) != 0;
    let mut lossy = Vec::new();
    if (flags & !KNOWN_CPIC_TEXT_FLAGS) != 0 {
        lossy.push("unrecognized text flag bits remain in the embedded source");
    }
    let raw_variable = optional_text(&stream.original_bytes, text.variable_name.as_ref())?;
    if !is_dynamic_or_input && !raw_variable.is_empty() {
        lossy.push("the field's variable binding is not connected");
    }
    let clean_variable: String = raw_variable
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    let instance_name = optional_text(&stream.original_bytes, text.instance_name.as_ref())?;

    let bounds = text.bounds_words.ok_or("Text has no archived bounds")?;
    let [left, right, top, bottom] = bounds.map(|value| f64::from(value) / 20.);
    let dimensions = TextBoxSize {
        width: right - left,
        height: bottom - top,
    };
    dimensions.validate().map_err(|error| error.to_string())?;

    let style = text
        .runs
        .first()
        .map(|run| &run.style)
        .or(text.default_style.as_ref())
        .ok_or("Text has no run or default style")?;

    let raw_has_newlines = text.runs.iter().any(|run| {
        read_text_run_string(&stream.original_bytes, &run.characters, run.style.char_set)
            .is_ok_and(|s| s.contains(['\r', '\n']))
    });
    let single_line_inert_leading =
        !is_dynamic_or_input && text.auto_expand.unwrap_or(0) != 0 && !raw_has_newlines;

    let (mut format, mut alpha) =
        decode_cpic_text_style(stream, style, single_line_inert_leading, &mut Vec::new())?;
    let html = is_dynamic_or_input && (flags & (0x8000 | 0x0080)) != 0;
    let mut characters = String::new();
    let mut format_runs: Vec<TextFormatRun> = Vec::new();
    let mut nonempty_styles: Vec<&TimelineTextStyle> = Vec::new();
    let mut visible_alphas: Vec<f64> = Vec::new();
    let mut nonempty_alphas: Vec<f64> = Vec::new();
    let mut nonempty_style_lossy: Vec<&'static str> = Vec::new();
    let mut fallback_style_lossy: Vec<&'static str> = Vec::new();
    let _ = decode_cpic_text_style(
        stream,
        style,
        single_line_inert_leading,
        &mut fallback_style_lossy,
    )?;
    let mut utf16_cursor = 0u32;
    let mut prev_ended_cr = false;
    for run in &text.runs {
        let mut run_lossy = Vec::new();
        let (run_format, run_alpha) = decode_cpic_text_style(
            stream,
            &run.style,
            single_line_inert_leading,
            &mut run_lossy,
        )?;
        // A Flash paragraph break can end one style run and continue with LF
        // in the next. Count UTF-16 offsets after normalizing that boundary.
        let raw_chars =
            read_text_run_string(&stream.original_bytes, &run.characters, run.style.char_set)?;
        let trimmed = if prev_ended_cr {
            raw_chars.strip_prefix('\n').unwrap_or(&raw_chars)
        } else {
            &raw_chars
        };
        if !raw_chars.is_empty() {
            prev_ended_cr = raw_chars.ends_with('\r');
        }
        let run_chars = normalize_text(trimmed);
        let run_units = run_chars.encode_utf16().count() as u32;
        if run_units > 0 {
            nonempty_styles.push(&run.style);
            nonempty_alphas.push(run_alpha);
            nonempty_style_lossy.extend(run_lossy);
            if run_chars.chars().any(|c| !c.is_whitespace()) {
                visible_alphas.push(run_alpha);
            }
            if let Some(last) = format_runs.last_mut()
                && last.end == utf16_cursor
                && last.format == run_format
            {
                last.end = utf16_cursor + run_units;
            } else {
                format_runs.push(TextFormatRun {
                    start: utf16_cursor,
                    end: utf16_cursor + run_units,
                    format: run_format,
                });
            }
            utf16_cursor += run_units;
        }
        characters.push_str(&run_chars);
    }
    if nonempty_styles.is_empty() {
        lossy.extend(fallback_style_lossy);
    } else {
        lossy.extend(nonempty_style_lossy);
    }
    if let Some(&first_vis_alpha) = visible_alphas.first() {
        alpha = first_vis_alpha;
        if visible_alphas
            .iter()
            .any(|&a| (a - first_vis_alpha).abs() > 1e-4)
        {
            lossy.push("per-run alpha uses the first run's alpha");
        }
    } else if let Some(&first_nonempty_alpha) = nonempty_alphas.first() {
        alpha = first_nonempty_alpha;
    }
    if format_runs.len() > 1 {
        format = format_runs[0].format.clone();
    } else if let Some(single) = format_runs.pop() {
        format = single.format;
    }

    if nonempty_styles.is_empty() {
        nonempty_styles.push(style);
    }
    let first_pos = nonempty_styles[0].character_position.unwrap_or(0);
    let (uniform_superscript, uniform_subscript) = if nonempty_styles
        .iter()
        .all(|s| s.character_position.unwrap_or(0) == first_pos)
    {
        match first_pos {
            0 => (false, false),
            1 => (true, false),
            2 => (false, true),
            _ => {
                lossy.push("superscript/subscript is shown on the baseline");
                (false, false)
            }
        }
    } else {
        lossy.push("mixed per-run superscript/subscript uses uniform baseline");
        (false, false)
    };

    let first_vert = nonempty_styles[0].vertical.unwrap_or(0) != 0;
    let uniform_vertical = if nonempty_styles
        .iter()
        .all(|s| (s.vertical.unwrap_or(0) != 0) == first_vert)
    {
        first_vert && nonempty_styles.iter().all(|s| s.rotation.unwrap_or(0) == 0)
    } else {
        lossy.push("mixed vertical/horizontal runs are laid out horizontally");
        false
    };
    let uniform_rtl = nonempty_styles
        .iter()
        .all(|s| s.right_to_left.unwrap_or(0) != 0);

    let decode_style_aa = |st: &TimelineTextStyle| -> Option<(u8, f64, f64)> {
        if (flags & 0x20000) != 0 || st.bitmap_rendering.unwrap_or(0) != 0 {
            return None;
        }
        let aa_span = st.anti_alias?;
        let aa_bytes = stream.original_bytes.get(aa_span.start..aa_span.end)?;
        if aa_bytes.len() < 2 {
            return None;
        }
        let rev = aa_bytes[0];
        let mode = aa_bytes[1];
        let float_offset = if rev == 1 { 8 } else { 2 };
        if mode == 2 && aa_bytes.len() >= float_offset + 8 {
            let raw_thickness =
                f32::from_le_bytes(aa_bytes[float_offset..float_offset + 4].try_into().unwrap());
            let raw_sharpness = f32::from_le_bytes(
                aa_bytes[float_offset + 4..float_offset + 8]
                    .try_into()
                    .unwrap(),
            );
            let t = if raw_thickness.is_finite() {
                f64::from(raw_thickness).clamp(-200.0, 200.0)
            } else {
                0.0
            };
            let s = if raw_sharpness.is_finite() {
                f64::from(raw_sharpness).clamp(-400.0, 400.0)
            } else {
                0.0
            };
            Some((2, s, t))
        } else if mode == 1 {
            Some((1, 0.0, 0.0))
        } else {
            Some((0, 0.0, 0.0))
        }
    };

    let has_bitmap_aa = (flags & 0x20000) == 0
        && nonempty_styles
            .iter()
            .any(|s| s.bitmap_rendering.unwrap_or(0) != 0);
    if has_bitmap_aa {
        lossy.push("bitmap-aliased text rendering uses normal antialiasing");
    }
    let decoded_aas: Vec<(u8, f64, f64)> = nonempty_styles
        .iter()
        .filter_map(|st| decode_style_aa(st))
        .collect();
    if decoded_aas.windows(2).any(|w| w[0].0 != w[1].0) {
        lossy.push("mixed per-run anti-aliasing uses uniform field anti-aliasing");
    }
    let (custom_aa, aa_sharpness, aa_thickness) =
        if let Some(&(_, s, t)) = decoded_aas.iter().find(|&&(mode, _, _)| mode == 2) {
            (true, s, t)
        } else if decoded_aas.iter().any(|&(mode, _, _)| mode == 1) {
            (true, 0.0, 0.0)
        } else {
            (false, 0.0, 0.0)
        };

    let localization_id = optional_text(&stream.original_bytes, text.localization_id.as_ref())?;
    if !localization_id.is_empty() {
        lossy.push("text localization string ID remains in the embedded source");
    }

    let name = if name.is_empty() {
        instance_name.as_str()
    } else {
        name
    };
    let accessibility = convert_accessibility(
        stream,
        text.accessibility.as_ref(),
        name,
        AccessibleRole::Button,
    )?;
    let words = text.matrix_words.ok_or("Text has no archived matrix")?;
    let [a, b, c, d] =
        [words[0], words[1], words[2], words[3]].map(|value| f64::from(value) / 65536.);
    let mut transform = Transform::from_matrix([
        a,
        b,
        c,
        d,
        f64::from(words[4]) / 20. + a * left + c * top,
        f64::from(words[5]) / 20. + b * left + d * top,
    ])
    .ok_or("Text matrix is outside supported transform bounds")?;
    transform.alpha = alpha;

    let location = if name.is_empty() { id } else { name };
    if !lossy.is_empty() {
        lossy.dedup();
        let var_suffix = if !is_dynamic_or_input && !raw_variable.is_empty() {
            format!(" (variable \"{raw_variable}\")")
        } else {
            String::new()
        };
        report.warning(
            "fla_text_approximated",
            location,
            format!(
                "Text imported with approximations: {}{var_suffix}. The original text record is retained in the embedded source.",
                lossy.join("; "),
            ),
        );
    }
    if !report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "fla_text_metrics")
    {
        report.warning(
            "fla_text_metrics",
            location,
            "Text, font names, style, bounds and placement are retained. Shared text layout uses host font metrics and text-box padding, so static glyph placement/antialiasing may differ from Flash.",
        );
    }
    if text
        .runs
        .iter()
        .any(|run| (run.style.font_flags.unwrap_or(0) & !KNOWN_FONT_TECH_FLAGS) != 0)
    {
        report.warning(
            "fla_text_font_flags",
            location,
            "Additional archived font flags remain in the embedded source; named font, size, bold and italic are imported.",
        );
    }

    let filters = convert_filters(&text.filters, location, report);
    let has_newlines = characters.contains(['\r', '\n']);
    let static_fixed_wrap =
        !is_dynamic_or_input && text.auto_expand.unwrap_or(0) == 0 && !uniform_vertical;
    let multiline = has_newlines
        || static_fixed_wrap
        || (is_dynamic_or_input && !display_as_password && (flags & 0x0010) != 0);
    let word_wrap = if is_dynamic_or_input {
        !display_as_password && (flags & 0x0008) != 0
    } else {
        static_fixed_wrap
    };
    let has_typography_opts = uniform_superscript
        || uniform_subscript
        || uniform_vertical
        || custom_aa
        || (is_dynamic_or_input && uniform_rtl);

    if is_dynamic_or_input || !format_runs.is_empty() || html || has_typography_opts {
        let mut opts = AuthoredTextFieldOptions {
            field_type: if is_input {
                TextFieldType::Input
            } else {
                TextFieldType::Dynamic
            },
            selectable: if is_input {
                true
            } else if is_dynamic_or_input {
                (flags & 0x10000) != 0 || (flags & 0x0080) == 0
            } else {
                true
            },
            display_as_password,
            max_chars: if is_dynamic_or_input {
                u32::from(text.max_characters.unwrap_or(0))
            } else {
                0
            },
            variable: if is_dynamic_or_input && !clean_variable.is_empty() {
                Some(clean_variable)
            } else {
                None
            },
            auto_size: if is_dynamic_or_input && text.auto_expand.unwrap_or(0) != 0 {
                if format.align_mode() == Some("justify") {
                    "left".into()
                } else {
                    match format.align {
                        TextAlign::Left => "left".into(),
                        TextAlign::Center => "center".into(),
                        TextAlign::Right => "right".into(),
                    }
                }
            } else {
                "none".into()
            },
            border: is_dynamic_or_input && (flags & 0x0040) != 0,
            background: is_dynamic_or_input && (flags & 0x0040) != 0,
            html,
            embed_fonts: is_dynamic_or_input
                && (flags & 0x20000) == 0
                && ((flags & (0x0020 | 0x3f00)) != 0
                    || text.embed_variant_glyphs.unwrap_or(0) != 0),
            orientation: if uniform_vertical {
                "vertical".into()
            } else {
                "horizontal".into()
            },
            direction: if (is_dynamic_or_input || uniform_vertical) && uniform_rtl {
                "rtl".into()
            } else {
                "ltr".into()
            },
            superscript: uniform_superscript,
            subscript: uniform_subscript,
            anti_alias_type: if custom_aa {
                "advanced".into()
            } else {
                "normal".into()
            },
            sharpness: aa_sharpness,
            thickness: aa_thickness,
            format_runs,
            ..AuthoredTextFieldOptions::default()
        };
        if opts.field_type == TextFieldType::Input && format.bullet {
            opts.field_type = TextFieldType::Dynamic;
        }
        format.set_field_options((!opts.is_default()).then_some(opts));
    }

    Ok(Element {
        id: id.into(),
        name: name.into(),
        transform,
        kind: ElementKind::TextBox {
            text: characters,
            width: dimensions.width,
            height: dimensions.height,
            format,
            word_wrap,
            multiline,
        },
        scripts: vec![],
        accessibility,
        filters,
        ..Element::default()
    })
}
