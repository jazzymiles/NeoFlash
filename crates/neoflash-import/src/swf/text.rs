//! SWF font (`DefineFont` 1/2/3/4, `DefineFontInfo` 1/2, `DefineFontName`),
//! static text (`DefineText` / `DefineText2`) and dynamic/input text (`DefineEditText`)
//! decoders into NeoFlash `ElementKind::TextBox` (with vector glyph-outline fallback
//! when a font's Unicode code table was stripped).

use super::bits::SwfSlice;
use super::shape::read_pure_shape_commands;
use crate::ImportReport;
use neoflash_core::{
    Color, Element, ElementKind, FillRule, MAX_PATH_COMMANDS, MAX_TEXT_BYTES, PathCommand,
    TextAlign, TextBoxSize, TextFieldState, TextFormat, Transform, validate_path_commands,
};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub(crate) struct SwfFontDef {
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    pub code_table: Vec<Option<char>>,
    /// Unit-EM (`1.0 == 1 em`) glyph outlines for fallback rendering when `code_table` is absent.
    pub glyph_outlines: Vec<Vec<PathCommand>>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedSwfText {
    pub elements: Vec<Element>,
}

/// Decode `DefineFont` (tag 10).
pub(crate) fn decode_define_font(payload: &[u8], fonts: &mut HashMap<u16, SwfFontDef>) {
    let mut r = SwfSlice::new(payload);
    let Some(font_id) = r.u16() else { return };
    if r.remaining() < 2 {
        fonts.entry(font_id).or_default();
        return;
    }
    let table_start = r.pos();
    let Some(first_offset) = r.u16() else { return };
    let num_glyphs = usize::from(first_offset) / 2;
    let mut offsets = Vec::with_capacity(num_glyphs);
    offsets.push(usize::from(first_offset));
    for _ in 1..num_glyphs {
        let Some(off) = r.u16() else { break };
        offsets.push(usize::from(off));
    }
    let mut glyph_outlines = Vec::with_capacity(offsets.len());
    for (idx, &off) in offsets.iter().enumerate() {
        let start = table_start.saturating_add(off);
        let end = offsets
            .get(idx + 1)
            .map_or(payload.len(), |&next| table_start.saturating_add(next))
            .min(payload.len());
        if start < end {
            let mut gr = SwfSlice::new(&payload[start..end]);
            glyph_outlines
                .push(read_pure_shape_commands(&mut gr, 1.0 / 1024.0).unwrap_or_default());
        } else {
            glyph_outlines.push(Vec::new());
        }
    }
    let entry = fonts.entry(font_id).or_default();
    entry.glyph_outlines = glyph_outlines;
}

/// Decode `DefineFontInfo` (tag 13) or `DefineFontInfo2` (tag 62).
pub(crate) fn decode_define_font_info(
    tag_code: u16,
    payload: &[u8],
    swf_version: u8,
    fonts: &mut HashMap<u16, SwfFontDef>,
    legacy_cp_warned: &mut bool,
) {
    let mut r = SwfSlice::new(payload);
    let Some(font_id) = r.u16() else { return };
    let Some((name, used_cp)) = r.len_u8_string(swf_version) else {
        return;
    };
    *legacy_cp_warned |= used_cp;
    let Some(flags) = r.u8() else { return };
    if tag_code == 62 {
        let _lang = r.u8();
    }
    let italic = (flags & 0x04) != 0;
    let bold = (flags & 0x02) != 0;
    let wide_codes = (flags & 0x01) != 0 || tag_code == 62;

    let mut code_table = Vec::new();
    while r.remaining() >= if wide_codes { 2 } else { 1 } {
        let code = if wide_codes {
            u32::from(r.u16().unwrap_or(0))
        } else {
            u32::from(r.u8().unwrap_or(0))
        };
        code_table.push(decode_glyph_char(code, swf_version));
    }

    let entry = fonts.entry(font_id).or_default();
    if !name.trim().is_empty() {
        entry.name = clean_font_name(&name);
    }
    entry.bold = bold;
    entry.italic = italic;
    if !code_table.is_empty() {
        entry.code_table = code_table;
    }
}

/// Decode `DefineFont2` (tag 48) or `DefineFont3` (tag 75).
pub(crate) fn decode_define_font2_or_3(
    tag_code: u16,
    payload: &[u8],
    swf_version: u8,
    fonts: &mut HashMap<u16, SwfFontDef>,
    legacy_cp_warned: &mut bool,
) {
    let mut r = SwfSlice::new(payload);
    let Some(font_id) = r.u16() else { return };
    let Some(flags) = r.u8() else { return };
    let _lang = r.u8();
    let Some((name, used_cp)) = r.len_u8_string(swf_version) else {
        return;
    };
    *legacy_cp_warned |= used_cp;
    let Some(num_glyphs_u16) = r.u16() else {
        return;
    };
    let num_glyphs = usize::from(num_glyphs_u16);
    let wide_offsets = (flags & 0x08) != 0;
    let wide_codes = (flags & 0x04) != 0 || tag_code == 75;
    let italic = (flags & 0x02) != 0;
    let bold = (flags & 0x01) != 0;

    let mut glyph_outlines = Vec::new();
    let mut code_table = Vec::new();

    if num_glyphs > 0 {
        let table_start = r.pos();
        let mut offsets = Vec::with_capacity(num_glyphs);
        for _ in 0..num_glyphs {
            let off = if wide_offsets {
                r.u32().map(|v| v as usize)
            } else {
                r.u16().map(usize::from)
            };
            let Some(off) = off else { break };
            offsets.push(off);
        }
        let code_table_offset = if wide_offsets {
            r.u32().map(|v| v as usize)
        } else {
            r.u16().map(usize::from)
        };
        let em_scale = if tag_code == 75 {
            1.0 / 20480.0
        } else {
            1.0 / 1024.0
        };
        let glyphs_end = code_table_offset
            .map_or(payload.len(), |c| table_start.saturating_add(c))
            .min(payload.len());

        for (idx, &off) in offsets.iter().enumerate() {
            let start = table_start.saturating_add(off);
            let end = offsets
                .get(idx + 1)
                .map_or(glyphs_end, |&next| table_start.saturating_add(next))
                .min(glyphs_end);
            if start < end {
                let mut gr = SwfSlice::new(&payload[start..end]);
                glyph_outlines
                    .push(read_pure_shape_commands(&mut gr, em_scale).unwrap_or_default());
            } else {
                glyph_outlines.push(Vec::new());
            }
        }

        if let Some(ct_off) = code_table_offset {
            let ct_pos = table_start.saturating_add(ct_off);
            if ct_pos <= payload.len() {
                let mut cr = SwfSlice::new(&payload[ct_pos..]);
                for _ in 0..num_glyphs {
                    let code = if wide_codes {
                        cr.u16().map(u32::from)
                    } else {
                        cr.u8().map(u32::from)
                    };
                    let Some(c) = code else { break };
                    code_table.push(decode_glyph_char(c, swf_version));
                }
            }
        }
    }

    fonts.insert(
        font_id,
        SwfFontDef {
            name: clean_font_name(&name),
            bold,
            italic,
            code_table,
            glyph_outlines,
        },
    );
}

/// Decode `DefineFontName` (tag 88) or `DefineFont4` (tag 91).
pub(crate) fn decode_define_font_name_or_4(
    tag_code: u16,
    payload: &[u8],
    swf_version: u8,
    fonts: &mut HashMap<u16, SwfFontDef>,
) {
    let mut r = SwfSlice::new(payload);
    let Some(font_id) = r.u16() else { return };
    if tag_code == 91 {
        let Some(flags) = r.u8() else { return };
        let Some((name, _)) = r.c_string(swf_version) else {
            return;
        };
        let entry = fonts.entry(font_id).or_default();
        if !name.trim().is_empty() {
            entry.name = clean_font_name(&name);
        }
        entry.italic = (flags & 0x02) != 0;
        entry.bold = (flags & 0x01) != 0;
    } else {
        let Some((full_name, _)) = r.c_string(swf_version) else {
            return;
        };
        let entry = fonts.entry(font_id).or_default();
        if entry.name.is_empty() && !full_name.trim().is_empty() {
            entry.name = clean_font_name(&full_name);
        }
    }
}

/// Decode `DefineText` (tag 11) or `DefineText2` (tag 33).
pub(crate) fn decode_define_text(
    tag_code: u16,
    payload: &[u8],
    fonts: &HashMap<u16, SwfFontDef>,
    location: &str,
    report: &mut ImportReport,
    metrics_warned: &mut bool,
) -> Option<(u16, DecodedSwfText)> {
    let mut r = SwfSlice::new(payload);
    let text_id = r.u16()?;
    let bounds = r.rect_twips()?;
    let text_matrix = r.matrix_px()?;
    let glyph_bits = r.u8()? as usize;
    let advance_bits = r.u8()? as usize;
    let has_rgba = tag_code == 33;

    let mut cur_font_id: Option<u16> = None;
    let mut cur_color: Color = [0, 0, 0, 255];
    let mut cur_height_twips: u16 = 240;
    let mut cur_x_twips: i32 = 0;
    let mut cur_y_twips: i32 = 0;
    let mut prev_line_y: Option<i32> = None;

    let mut first_style: Option<(u16, Color, u16)> = None;
    let mut mixed_styles = false;
    let mut text_buf = String::new();
    let mut mapped_count = 0usize;
    let mut unmapped_count = 0usize;

    // Also record positioned glyphs `(font_id, glyph_idx, x_px, y_px, size_px, color)` for outline fallback.
    let mut positioned_glyphs: Vec<(u16, usize, f64, f64, f64, Color)> = Vec::new();

    for _ in 0..10_000 {
        let Some(flags) = r.u8() else { break };
        if flags == 0 {
            break;
        }
        let has_font = (flags & 0x08) != 0;
        let has_color = (flags & 0x04) != 0;
        let has_y = (flags & 0x02) != 0;
        let has_x = (flags & 0x01) != 0;

        if has_font {
            cur_font_id = Some(r.u16()?);
        }
        if has_color {
            cur_color = if has_rgba { r.rgba()? } else { r.rgb()? };
        }
        if has_x {
            cur_x_twips = i32::from(r.i16()?);
        }
        if has_y {
            let new_y = i32::from(r.i16()?);
            if let Some(py) = prev_line_y
                && (new_y - py).abs() > i32::from(cur_height_twips) / 3
                && !text_buf.is_empty()
                && !text_buf.ends_with('\n')
            {
                text_buf.push('\n');
            }
            cur_y_twips = new_y;
            prev_line_y = Some(new_y);
        } else if prev_line_y.is_none() {
            prev_line_y = Some(cur_y_twips);
        }
        if has_font {
            cur_height_twips = r.u16()?.max(20);
        }

        let glyph_count = usize::from(r.u8()?);
        let fid = cur_font_id.unwrap_or(0);
        if let Some(first) = first_style {
            if first != (fid, cur_color, cur_height_twips) && glyph_count > 0 {
                mixed_styles = true;
            }
        } else if glyph_count > 0 {
            first_style = Some((fid, cur_color, cur_height_twips));
        }

        r.align();
        let font = fonts.get(&fid);
        let size_px = (f64::from(cur_height_twips) / 20.0).clamp(1.0, 10_000.0);
        for _ in 0..glyph_count {
            let glyph_idx = r.ub(glyph_bits)? as usize;
            let advance = r.sb(advance_bits)?;
            let gx_px = f64::from(cur_x_twips) / 20.0;
            let gy_px = f64::from(cur_y_twips) / 20.0;
            positioned_glyphs.push((fid, glyph_idx, gx_px, gy_px, size_px, cur_color));
            cur_x_twips = cur_x_twips.saturating_add(advance);

            if let Some(ch) = font
                .and_then(|f| f.code_table.get(glyph_idx))
                .and_then(|c| *c)
            {
                if text_buf.len() + ch.len_utf8() <= MAX_TEXT_BYTES {
                    text_buf.push(ch);
                }
                mapped_count += 1;
            } else {
                unmapped_count += 1;
            }
        }
        r.align();
    }

    // Fallback: if the font's Unicode code table was completely stripped, render glyph outlines!
    if mapped_count == 0 && unmapped_count > 0 {
        let outline_elements =
            render_glyph_outlines_fallback(text_id, text_matrix, &positioned_glyphs, fonts);
        if !outline_elements.is_empty() {
            report.warning(
                "swf_text_glyph_outlines",
                format!("{location}#text:{text_id}"),
                "SWF static text references a font without a Unicode code table; imported as vector glyph outlines.",
            );
            return Some((
                text_id,
                DecodedSwfText {
                    elements: outline_elements,
                },
            ));
        }
    }

    let (fid, color, height_twips) = first_style.unwrap_or((0, [0, 0, 0, 255], 240));
    let font_def = fonts.get(&fid);
    let font_name = font_def
        .map(|f| f.name.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("sans-serif")
        .to_owned();
    let bold = font_def.is_some_and(|f| f.bold);
    let italic = font_def.is_some_and(|f| f.italic);
    let size_px = (f64::from(height_twips) / 20.0).clamp(1.0, 1000.0);

    if mixed_styles || unmapped_count > 0 {
        report.warning(
            "swf_text_approximated",
            format!("{location}#text:{text_id}"),
            "Multi-run static text uses the first run's font style.",
        );
    }
    if !*metrics_warned {
        *metrics_warned = true;
        report.warning(
            "swf_text_metrics",
            format!("{location}#text:{text_id}"),
            "SWF text, font names, style, bounds and placement are retained; host font metrics may differ slightly from Flash.",
        );
    }

    let elem = build_textbox_element(
        format!("swf-text-{text_id}-el-0"),
        String::new(),
        bounds,
        text_matrix,
        text_buf,
        font_name,
        size_px,
        color,
        bold,
        italic,
        TextAlign::Left,
        0.0,
        0.0,
        0.0,
        0.0,
        false,
    )?;

    Some((
        text_id,
        DecodedSwfText {
            elements: vec![elem],
        },
    ))
}

/// Decode `DefineEditText` (tag 37) into an `ElementKind::TextBox`.
pub(crate) fn decode_define_edit_text(
    payload: &[u8],
    swf_version: u8,
    fonts: &HashMap<u16, SwfFontDef>,
    location: &str,
    report: &mut ImportReport,
    metrics_warned: &mut bool,
    legacy_cp_warned: &mut bool,
) -> Option<(u16, DecodedSwfText)> {
    let mut r = SwfSlice::new(payload);
    let text_id = r.u16()?;
    let bounds = r.rect_twips()?;
    let flags0 = r.u8()?;
    let flags1 = r.u8()?;

    let has_text = (flags0 & 0x80) != 0;
    let word_wrap = (flags0 & 0x40) != 0;
    let multiline = (flags0 & 0x20) != 0;
    let password = (flags0 & 0x10) != 0;
    let read_only = (flags0 & 0x08) != 0;
    let has_text_color = (flags0 & 0x04) != 0;
    let has_max_length = (flags0 & 0x02) != 0;
    let has_font = (flags0 & 0x01) != 0;

    let has_font_class = (flags1 & 0x80) != 0;
    let has_layout = (flags1 & 0x20) != 0;
    let was_static = (flags1 & 0x04) != 0;
    let is_html = (flags1 & 0x02) != 0;

    let font_id = if has_font { Some(r.u16()?) } else { None };
    let font_class = if has_font_class {
        let (fc, cp) = r.c_string(swf_version)?;
        *legacy_cp_warned |= cp;
        Some(fc)
    } else {
        None
    };
    let font_height_twips = if has_font { r.u16()? } else { 240 };
    let color = if has_text_color {
        r.rgba()?
    } else {
        [0, 0, 0, 255]
    };
    if has_max_length {
        let _max_len = r.u16()?;
    }
    let (align, left_margin, right_margin, indent, leading) = if has_layout {
        let al = match r.u8()? {
            1 => TextAlign::Right,
            2 => TextAlign::Center,
            _ => TextAlign::Left,
        };
        let lm = (f64::from(r.u16()?) / 20.0).clamp(0.0, 2000.0);
        let rm = (f64::from(r.u16()?) / 20.0).clamp(0.0, 2000.0);
        let ind = (f64::from(r.i16()?) / 20.0).clamp(-2000.0, 2000.0);
        let lead = (f64::from(r.i16()?) / 20.0).clamp(-1000.0, 2000.0);
        (al, lm, rm, ind, lead)
    } else {
        (TextAlign::Left, 0.0, 0.0, 0.0, 0.0)
    };

    let (variable_name, cp1) = r.c_string(swf_version)?;
    *legacy_cp_warned |= cp1;
    let raw_text = if has_text {
        let (t, cp2) = r.c_string(swf_version)?;
        *legacy_cp_warned |= cp2;
        t
    } else {
        String::new()
    };

    let plain_text = if is_html {
        strip_flash_html(&raw_text)
    } else {
        raw_text
    };

    if !was_static || !read_only || password || is_html || !variable_name.is_empty() {
        report.warning(
            "swf_text_approximated",
            format!("{location}#edittext:{text_id}"),
            format!(
                "SWF DefineEditText imported as authored text box{}.",
                if variable_name.is_empty() {
                    String::new()
                } else {
                    format!(" (variable \"{variable_name}\")")
                }
            ),
        );
    }
    if !*metrics_warned {
        *metrics_warned = true;
        report.warning(
            "swf_text_metrics",
            format!("{location}#edittext:{text_id}"),
            "SWF text, font names, style, bounds and placement are retained; host font metrics may differ slightly from Flash.",
        );
    }

    let font_def = font_id.and_then(|id| fonts.get(&id));
    let font_name = font_def
        .map(|f| f.name.as_str())
        .filter(|s| !s.is_empty())
        .or(font_class.as_deref().filter(|s| !s.is_empty()))
        .unwrap_or("sans-serif")
        .to_owned();
    let bold = font_def.is_some_and(|f| f.bold);
    let italic = font_def.is_some_and(|f| f.italic);
    let size_px = (f64::from(font_height_twips.max(20)) / 20.0).clamp(1.0, 1000.0);

    let elem = build_textbox_element(
        format!("swf-edittext-{text_id}-el-0"),
        String::new(),
        bounds,
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        plain_text,
        font_name,
        size_px,
        color,
        bold,
        italic,
        align,
        left_margin,
        right_margin,
        indent,
        leading,
        word_wrap,
    )?;

    let mut elem = elem;
    if multiline && let ElementKind::TextBox { multiline: ml, .. } = &mut elem.kind {
        *ml = true;
    }

    Some((
        text_id,
        DecodedSwfText {
            elements: vec![elem],
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_textbox_element(
    id: String,
    name: String,
    bounds_twips: [i32; 4],
    matrix: [f64; 6],
    mut text: String,
    font_name: String,
    size_px: f64,
    color: Color,
    bold: bool,
    italic: bool,
    align: TextAlign,
    left_margin: f64,
    right_margin: f64,
    indent: f64,
    leading: f64,
    word_wrap: bool,
) -> Option<Element> {
    if text.len() > MAX_TEXT_BYTES {
        while text.len() > MAX_TEXT_BYTES {
            text.pop();
        }
    }
    let [left, right, top, bottom] = bounds_twips.map(|v| f64::from(v) / 20.0);
    // Ensure non-zero positive box dimensions within TextBoxSize::validate() limits.
    let width = (right - left).clamp(4.0, 32_768.0);
    let height = (bottom - top).clamp(size_px.clamp(4.0, 32_768.0), 32_768.0);
    let dims = TextBoxSize { width, height };
    if dims.validate().is_err() {
        return None;
    }

    let [a, b, c, d, tx, ty] = matrix;
    let mut transform =
        Transform::from_matrix([a, b, c, d, tx + a * left + c * top, ty + b * left + d * top])?;
    transform.alpha = (f64::from(color[3]) / 255.0).clamp(0.0, 1.0);

    let safe_font: String = font_name
        .chars()
        .filter(|ch| !ch.is_control())
        .take(256)
        .collect();
    let font = if safe_font.trim().is_empty() {
        "sans-serif".to_owned()
    } else {
        safe_font
    };
    let rgb = (u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2]);
    let multiline = text.contains(['\r', '\n']);

    let kind = ElementKind::TextBox {
        text,
        width,
        height,
        format: TextFormat {
            font,
            size: size_px.clamp(1.0, 1000.0),
            color: rgb,
            bold,
            italic,
            leading,
            align,
            left_margin,
            right_margin,
            indent,
            ..TextFormat::default()
        },
        word_wrap,
        multiline,
    };

    if TextFieldState::from_kind(&kind).is_some_and(|s| s.validate().is_err()) {
        return None;
    }

    Some(Element {
        id,
        name,
        transform,
        kind,
        ..Element::default()
    })
}

fn render_glyph_outlines_fallback(
    text_id: u16,
    text_matrix: [f64; 6],
    positioned: &[(u16, usize, f64, f64, f64, Color)],
    fonts: &HashMap<u16, SwfFontDef>,
) -> Vec<Element> {
    let [a, b, c, d, tx, ty] = text_matrix;
    let mut by_color: Vec<(Color, Vec<PathCommand>)> = Vec::new();
    for &(fid, gidx, gx, gy, size_px, color) in positioned {
        let Some(outline) = fonts
            .get(&fid)
            .and_then(|f| f.glyph_outlines.get(gidx))
            .filter(|cmds| !cmds.is_empty())
        else {
            continue;
        };
        let bucket = if let Some((_, cmds)) = by_color
            .iter_mut()
            .find(|(col, cmds)| *col == color && cmds.len() + outline.len() <= MAX_PATH_COMMANDS)
        {
            cmds
        } else {
            by_color.push((color, Vec::new()));
            &mut by_color.last_mut().unwrap().1
        };
        let map_pt = |ux: f64, uy: f64| -> (f64, f64) {
            let lx = gx + ux * size_px;
            let ly = gy + uy * size_px;
            (a * lx + c * ly + tx, b * lx + d * ly + ty)
        };
        for &cmd in outline {
            match cmd {
                PathCommand::MoveTo { x, y } => {
                    let (nx, ny) = map_pt(x, y);
                    bucket.push(PathCommand::MoveTo { x: nx, y: ny });
                }
                PathCommand::LineTo { x, y } => {
                    let (nx, ny) = map_pt(x, y);
                    bucket.push(PathCommand::LineTo { x: nx, y: ny });
                }
                PathCommand::QuadraticTo { cx, cy, x, y } => {
                    let (ncx, ncy) = map_pt(cx, cy);
                    let (nx, ny) = map_pt(x, y);
                    bucket.push(PathCommand::QuadraticTo {
                        cx: ncx,
                        cy: ncy,
                        x: nx,
                        y: ny,
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
                    let (nc1x, nc1y) = map_pt(cx1, cy1);
                    let (nc2x, nc2y) = map_pt(cx2, cy2);
                    let (nx, ny) = map_pt(x, y);
                    bucket.push(PathCommand::CubicTo {
                        cx1: nc1x,
                        cy1: nc1y,
                        cx2: nc2x,
                        cy2: nc2y,
                        x: nx,
                        y: ny,
                    });
                }
                PathCommand::Close => bucket.push(PathCommand::Close),
            }
        }
    }

    let mut out = Vec::new();
    for (idx, (color, cmds)) in by_color.into_iter().enumerate() {
        if !cmds.is_empty() && validate_path_commands(&cmds).is_ok() {
            out.push(Element {
                id: format!("swf-text-{text_id}-glyphs-{idx}"),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Path {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill: color,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
        }
    }
    out
}

fn decode_glyph_char(code: u32, swf_version: u8) -> Option<char> {
    if code == 0 {
        return None;
    }
    if swf_version <= 5 && (0x80..=0xFF).contains(&code) {
        let byte = [code as u8];
        let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(&byte);
        return cow
            .chars()
            .next()
            .filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'));
    }
    char::from_u32(code).filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
}

fn clean_font_name(raw: &str) -> String {
    let cleaned: String = raw
        .trim_matches('\0')
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect();
    cleaned.trim().to_owned()
}

fn strip_flash_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for tc in chars.by_ref() {
                if tc == '>' {
                    break;
                }
                if tag.len() < 32 {
                    tag.push(tc.to_ascii_lowercase());
                }
            }
            let tag_trimmed = tag.trim().trim_end_matches('/').trim();
            if matches!(tag_trimmed, "br" | "/p" | "/li" | "/div")
                && !out.ends_with('\n')
                && !out.is_empty()
            {
                out.push('\n');
            }
        } else if ch == '&' {
            let mut ent = String::new();
            let mut closed = false;
            while let Some(&nc) = chars.peek() {
                if nc == ';' {
                    chars.next();
                    closed = true;
                    break;
                }
                if ent.len() >= 10 || nc.is_whitespace() || nc == '<' {
                    break;
                }
                ent.push(nc);
                chars.next();
            }
            if closed {
                match ent.as_str() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    "nbsp" => out.push(' '),
                    s if s.starts_with("#x") || s.starts_with("#X") => {
                        if let Ok(code) = u32::from_str_radix(&s[2..], 16)
                            && let Some(decoded) = char::from_u32(code)
                        {
                            out.push(decoded);
                        }
                    }
                    s if s.starts_with('#') => {
                        if let Ok(code) = s[1..].parse::<u32>()
                            && let Some(decoded) = char::from_u32(code)
                        {
                            out.push(decoded);
                        }
                    }
                    _ => {
                        out.push('&');
                        out.push_str(&ent);
                        out.push(';');
                    }
                }
            } else {
                out.push('&');
                out.push_str(&ent);
            }
        } else {
            out.push(ch);
        }
    }
    out
}
