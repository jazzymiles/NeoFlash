//! Embedded `DefineFont3` (tag 75) vector glyph table and `DefineText2` (tag 33) encoders.

use super::*;
use ab_glyph::{Font, FontRef, OutlineCurve, PxScale};

#[derive(Clone, Debug)]
pub(super) struct SwfFont3Table {
    pub(super) font_id: u16,
    pub(super) code_to_glyph: BTreeMap<u16, usize>,
    pub(super) advances_20480: Vec<i16>,
    pub(super) ascent_20480: i16,
    pub(super) descent_20480: i16,
    pub(super) leading_20480: i16,
}

impl SwfFont3Table {
    pub(super) fn has_glyphs(&self) -> bool {
        !self.code_to_glyph.is_empty()
    }
}

pub(super) struct ProjectFont3Usage {
    pub(super) required_chars: BTreeMap<(String, bool, bool), BTreeSet<u16>>,
    pub(super) dynamic_faces: BTreeSet<(String, bool, bool)>,
}

pub(super) fn collect_project_font3_usage(project: &Project) -> ProjectFont3Usage {
    let mut required_chars: BTreeMap<(String, bool, bool), BTreeSet<u16>> = BTreeMap::new();
    let mut dynamic_faces: BTreeSet<(String, bool, bool)> = BTreeSet::new();

    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            if !layer.visible
                || matches!(layer.properties.role, LayerRole::Folder | LayerRole::Guide)
            {
                continue;
            }
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    match &el.kind {
                        ElementKind::Text {
                            text, font_family, ..
                        } => {
                            let key = (font_family.clone(), false, false);
                            let set = required_chars.entry(key).or_default();
                            set.insert(0x0020);
                            for ch in text.chars() {
                                if !matches!(ch, '\r' | '\n')
                                    && let Ok(code) = u16::try_from(u32::from(ch))
                                {
                                    set.insert(code);
                                }
                            }
                        }
                        ElementKind::TextBox { text, format, .. } => {
                            let key = (format.font.clone(), format.bold, format.italic);
                            let is_dynamic =
                                !el.name.trim().is_empty() || format.field_options().is_some();
                            if is_dynamic {
                                dynamic_faces.insert(key.clone());
                            }
                            let set = required_chars.entry(key).or_default();
                            set.insert(0x0020);
                            for ch in text.chars() {
                                if !matches!(ch, '\r' | '\n')
                                    && let Ok(code) = u16::try_from(u32::from(ch))
                                {
                                    set.insert(code);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    ProjectFont3Usage {
        required_chars,
        dynamic_faces,
    }
}

fn sfnt_slice_covers_chars(raw: &[u8], family: &str, chars: &[char]) -> bool {
    let sample: String = chars.iter().copied().collect();
    if !neoflash_core::TextFormat::validate_text_outlines_for_font_face(
        "SwfFont3", family, &sample, raw, 0,
    )
    .is_empty()
    {
        return false;
    }
    let Ok(font) = FontRef::try_from_slice(raw) else {
        return false;
    };
    chars.iter().all(|&ch| {
        if ch.is_whitespace() {
            return true;
        }
        let gid = font.glyph_id(ch);
        if gid.0 == 0 {
            return false;
        }
        font.outline_glyph(gid.with_scale(PxScale::from(24.0)))
            .is_some()
            && font.outline(gid).is_some_and(|o| !o.curves.is_empty())
    })
}

pub(super) fn resolve_swf_font3_sfnt_bytes(
    project: &Project,
    family: &str,
    bold: bool,
    italic: bool,
    chars: &[char],
    outline_fonts: &mut SwfOutlineFonts,
) -> Result<Vec<u8>, String> {
    if let Some(non_bmp) = chars
        .iter()
        .copied()
        .find(|&ch| !matches!(ch, '\r' | '\n') && u32::from(ch) > 0xFFFF)
    {
        return Err(format!(
            "character U+{:04X} is outside the 16-bit BMP CodeTable of SWF DefineFont3; enable outline_text for supplementary-plane text",
            u32::from(non_bmp)
        ));
    }

    // 1. Project embedded font resource (single SFNT, WOFF, or TTC collection).
    if let Some(raw) = crate::fonts::resolve_project_font_bytes(project, family, bold, italic) {
        let face_count = neoflash_core::sfnt_face_count(&raw);
        if face_count > 1 {
            let best = neoflash_core::select_best_sfnt_face_index(&raw, family, bold, italic);
            let mut order = vec![best];
            for idx in 0..face_count {
                if idx != best {
                    order.push(idx);
                }
            }
            for idx in order {
                if let Some(sfnt) = neoflash_core::extract_ttc_face_to_sfnt(&raw, idx)
                    && sfnt_slice_covers_chars(&sfnt, family, chars)
                {
                    return Ok(sfnt);
                }
            }
        } else {
            let decoded = neoflash_core::sfnt_view_bytes(&raw)
                .map(std::borrow::Cow::into_owned)
                .unwrap_or(raw);
            if sfnt_slice_covers_chars(&decoded, family, chars) {
                return Ok(decoded);
            }
        }
    }

    // 2. Recently resolved system SFNT faces.
    for (name, b, i, bytes) in &outline_fonts.recent {
        if name == family
            && *b == bold
            && *i == italic
            && sfnt_slice_covers_chars(bytes, family, chars)
        {
            return Ok(bytes.clone());
        }
    }

    // 3. System font database (`fontdb`).
    let db = outline_fonts.system.get_or_insert_with(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    });
    let weight = if bold {
        fontdb::Weight::BOLD
    } else {
        fontdb::Weight::NORMAL
    };
    let mut faces: Vec<_> = db.faces().collect();
    faces.sort_by_key(|face| {
        (
            !face
                .families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(family)),
            (face.style != fontdb::Style::Normal) != italic,
            face.weight.0.abs_diff(weight.0),
            face.index,
            face.post_script_name.as_str(),
        )
    });
    for face in faces {
        let face_idx = face.index;
        let exact_family = face
            .families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(family));
        let post_script = face.post_script_name.clone();
        if let Some(Some(sfnt)) = db.with_face_data(face.id, |bytes, _| {
            let candidate = if face_idx > 0 || bytes.starts_with(b"ttcf") {
                neoflash_core::extract_ttc_face_to_sfnt(bytes, face_idx)?
            } else {
                neoflash_core::sfnt_view_bytes(bytes)
                    .map_or_else(|| bytes.to_vec(), std::borrow::Cow::into_owned)
            };
            if candidate.len() > 32 * 1024 * 1024
                || !sfnt_slice_covers_chars(&candidate, family, chars)
            {
                return None;
            }
            Some(candidate)
        }) {
            if !exact_family {
                outline_fonts
                    .substitutions
                    .insert((family.to_owned(), post_script));
            }
            if outline_fonts.recent.len() == 4 {
                outline_fonts.recent.remove(0);
            }
            outline_fonts
                .recent
                .push((family.to_owned(), bold, italic, sfnt.clone()));
            return Ok(sfnt);
        }
    }

    // 4. Deterministic bundled SFNT fallback (Inter / RobotoMono) if it covers all required glyphs.
    let bundled = crate::fonts::fallback_font(family, bold);
    if sfnt_slice_covers_chars(bundled, family, chars) {
        return Ok(bundled.to_vec());
    }

    Err(format!(
        "no embedded or system SFNT vector font covers all glyphs in '{family}'"
    ))
}

pub(super) fn check_element_font3_support(
    project: &Project,
    element: &Element,
    outline_fonts: &mut SwfOutlineFonts,
) -> Result<(), String> {
    match &element.kind {
        ElementKind::Text {
            text, font_family, ..
        } => {
            let chars: Vec<char> = text
                .chars()
                .filter(|&c| !matches!(c, '\r' | '\n'))
                .collect();
            let _ = resolve_swf_font3_sfnt_bytes(
                project,
                font_family,
                false,
                false,
                &chars,
                outline_fonts,
            )?;
            Ok(())
        }
        ElementKind::TextBox { text, format, .. } => {
            let chars: Vec<char> = text
                .chars()
                .filter(|&c| !matches!(c, '\r' | '\n'))
                .collect();
            let _ = resolve_swf_font3_sfnt_bytes(
                project,
                &format.font,
                format.bold,
                format.italic,
                &chars,
                outline_fonts,
            )?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn subdivide_cubic_to_quads(
    p0: [f64; 2],
    p1: [f64; 2],
    p2: [f64; 2],
    p3: [f64; 2],
    depth: usize,
    out: &mut Vec<([f64; 2], [f64; 2], [f64; 2])>,
) {
    if depth == 0 {
        let ctrl = [
            0.25 * (3.0 * (p1[0] + p2[0]) - (p0[0] + p3[0])),
            0.25 * (3.0 * (p1[1] + p2[1]) - (p0[1] + p3[1])),
        ];
        out.push((p0, ctrl, p3));
        return;
    }
    let m01 = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
    let m12 = [0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])];
    let m23 = [0.5 * (p2[0] + p3[0]), 0.5 * (p2[1] + p3[1])];
    let m012 = [0.5 * (m01[0] + m12[0]), 0.5 * (m01[1] + m12[1])];
    let m123 = [0.5 * (m12[0] + m23[0]), 0.5 * (m12[1] + m23[1])];
    let mid = [0.5 * (m012[0] + m123[0]), 0.5 * (m012[1] + m123[1])];
    subdivide_cubic_to_quads(p0, m01, m012, mid, depth - 1, out);
    subdivide_cubic_to_quads(mid, m123, m23, p3, depth - 1, out);
}

fn encode_glyph_shape_20480(font: &FontRef<'_>, ch: char, em_scale: f64) -> (Vec<u8>, [i32; 4]) {
    if ch.is_whitespace() {
        return (vec![0x00, 0x00], [0, 0, 0, 0]);
    }
    let gid = font.glyph_id(ch);
    let Some(outline) = (gid.0 != 0).then(|| font.outline(gid)).flatten() else {
        return (vec![0x00, 0x00], [0, 0, 0, 0]);
    };
    if outline.curves.is_empty() {
        return (vec![0x00, 0x00], [0, 0, 0, 0]);
    }

    let map_em = |pt: ab_glyph::Point| -> [f64; 2] {
        [f64::from(pt.x) * em_scale, -f64::from(pt.y) * em_scale]
    };
    let to_i32 = |pt: [f64; 2]| -> [i32; 2] {
        [
            pt[0].round().clamp(-32767.0, 32767.0) as i32,
            pt[1].round().clamp(-32767.0, 32767.0) as i32,
        ]
    };
    let same_f = |a: [f64; 2], b: [f64; 2]| -> bool {
        (a[0] - b[0]).abs() <= 0.25 && (a[1] - b[1]).abs() <= 0.25
    };

    type GlyphQuadSeg = ([i32; 2], Option<[i32; 2]>, [i32; 2]);
    let mut contours: Vec<Vec<GlyphQuadSeg>> = Vec::new();
    let mut cur_segs: Vec<GlyphQuadSeg> = Vec::new();
    let mut cur_end: Option<[f64; 2]> = None;

    let push_seg = |segs: &mut Vec<GlyphQuadSeg>, s: [f64; 2], c: Option<[f64; 2]>, e: [f64; 2]| {
        let si = to_i32(s);
        let ei = to_i32(e);
        let ci = c.map(to_i32);
        if si == ei && ci.is_none() {
            return;
        }
        segs.push((si, ci, ei));
    };

    for curve in &outline.curves {
        match *curve {
            OutlineCurve::Line(p0, p1) => {
                let s = map_em(p0);
                let e = map_em(p1);
                if cur_end.is_some_and(|prev| !same_f(prev, s)) && !cur_segs.is_empty() {
                    contours.push(std::mem::take(&mut cur_segs));
                }
                push_seg(&mut cur_segs, s, None, e);
                cur_end = Some(e);
            }
            OutlineCurve::Quad(p0, c, p1) => {
                let s = map_em(p0);
                let ctrl = map_em(c);
                let e = map_em(p1);
                if cur_end.is_some_and(|prev| !same_f(prev, s)) && !cur_segs.is_empty() {
                    contours.push(std::mem::take(&mut cur_segs));
                }
                push_seg(&mut cur_segs, s, Some(ctrl), e);
                cur_end = Some(e);
            }
            OutlineCurve::Cubic(p0, c1, c2, p1) => {
                let s = map_em(p0);
                let q1 = map_em(c1);
                let q2 = map_em(c2);
                let e = map_em(p1);
                if cur_end.is_some_and(|prev| !same_f(prev, s)) && !cur_segs.is_empty() {
                    contours.push(std::mem::take(&mut cur_segs));
                }
                let mut quads = Vec::with_capacity(4);
                subdivide_cubic_to_quads(s, q1, q2, e, 2, &mut quads);
                for (qs, qc, qe) in quads {
                    push_seg(&mut cur_segs, qs, Some(qc), qe);
                }
                cur_end = Some(e);
            }
        }
    }
    if !cur_segs.is_empty() {
        contours.push(cur_segs);
    }

    let mut edges: Vec<SwfEdge> = Vec::new();
    for mut contour in contours {
        if contour.is_empty() {
            continue;
        }
        let first = contour[0].0;
        let last = contour.last().unwrap().2;
        if first != last {
            contour.push((last, None, first));
        }
        for (s, c, e) in contour {
            edges.push(SwfEdge {
                fill_1: 1,
                line: 0,
                start: s,
                control: c,
                end: e,
            });
        }
    }

    if edges.is_empty() {
        return (vec![0x00, 0x00], [0, 0, 0, 0]);
    }
    let bounds = compute_edges_bounds(&edges);
    let mut gw = SwfBitWriter::new();
    write_shape_edges_records(&mut gw, &edges, 1, 0, None);
    (gw.buf, bounds)
}

pub(super) fn build_define_font3_with_glyphs_payload(
    font_id: u16,
    name: &str,
    bold: bool,
    italic: bool,
    sfnt_bytes: &[u8],
    base_chars: &BTreeSet<u16>,
    include_ascii_if_present: bool,
) -> Result<(SwfFont3Table, Vec<u8>), String> {
    let font = FontRef::try_from_slice(sfnt_bytes)
        .map_err(|e| format!("failed to parse SFNT font '{name}': {e}"))?;
    let upm = f64::from(font.units_per_em().unwrap_or(1000.0)).max(1.0);
    let em_scale = 20480.0 / upm;

    let mut codes_set = base_chars.clone();
    codes_set.insert(0x0020);
    if include_ascii_if_present {
        for code in 0x0021u16..=0x007Eu16 {
            if let Some(ch) = char::from_u32(u32::from(code)) {
                let gid = font.glyph_id(ch);
                if gid.0 != 0 && font.outline(gid).is_some_and(|o| !o.curves.is_empty()) {
                    codes_set.insert(code);
                }
            }
        }
    }

    let sorted_codes: Vec<u16> = codes_set.into_iter().take(usize::from(u16::MAX)).collect();
    let num_glyphs = sorted_codes.len();

    let mut glyph_shapes: Vec<Vec<u8>> = Vec::with_capacity(num_glyphs);
    let mut glyph_bounds: Vec<[i32; 4]> = Vec::with_capacity(num_glyphs);
    let mut advances_20480: Vec<i16> = Vec::with_capacity(num_glyphs);
    let mut code_to_glyph: BTreeMap<u16, usize> = BTreeMap::new();

    for (idx, &code) in sorted_codes.iter().enumerate() {
        code_to_glyph.insert(code, idx);
        let ch = char::from_u32(u32::from(code)).unwrap_or(' ');
        let (shape_bytes, bounds) = encode_glyph_shape_20480(&font, ch, em_scale);
        let gid = font.glyph_id(ch);
        let adv = (f64::from(font.h_advance_unscaled(gid)) * em_scale)
            .round()
            .clamp(-32768.0, 32767.0) as i16;
        glyph_shapes.push(shape_bytes);
        glyph_bounds.push(bounds);
        advances_20480.push(adv.max(0));
    }

    let ascent_20480 = (f64::from(font.ascent_unscaled()) * em_scale)
        .round()
        .clamp(0.0, 32767.0) as i16;
    let descent_20480 = ((-f64::from(font.descent_unscaled())).max(0.0) * em_scale)
        .round()
        .clamp(0.0, 32767.0) as i16;
    let leading_20480 = (f64::from(font.line_gap_unscaled()) * em_scale)
        .round()
        .clamp(-32768.0, 32767.0) as i16;

    let mut w = SwfBitWriter::new();
    w.u16(font_id);
    // FontFlagsHasLayout (0x80) | FontFlagsWideOffsets (0x08) | FontFlagsWideCodes (0x04) | Italic (0x02) | Bold (0x01)
    let flags: u8 =
        0x80 | 0x08 | 0x04 | (if italic { 0x02 } else { 0 }) | (if bold { 0x01 } else { 0 });
    w.u8(flags);
    w.u8(1); // LanguageCode = Latin/Unicode
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(255);
    w.u8(name_len as u8);
    w.bytes(&name_bytes[..name_len]);
    w.u16(num_glyphs as u16);

    // OffsetTable (u32[num_glyphs]) + CodeTableOffset (u32), measured from start of OffsetTable.
    let first_shape_off = (num_glyphs + 1) * 4;
    let mut running_off = first_shape_off;
    for shape in &glyph_shapes {
        w.u32(running_off as u32);
        running_off += shape.len();
    }
    w.u32(running_off as u32); // CodeTableOffset

    for shape in &glyph_shapes {
        w.bytes(shape);
    }
    for &code in &sorted_codes {
        w.u16(code);
    }

    // Layout metrics
    w.i16(ascent_20480);
    w.i16(descent_20480);
    w.i16(leading_20480);
    for &adv in &advances_20480 {
        w.i16(adv);
    }
    for &b in &glyph_bounds {
        w.rect_twips(b);
    }
    w.u16(0); // KerningCount = 0

    let table = SwfFont3Table {
        font_id,
        code_to_glyph,
        advances_20480,
        ascent_20480,
        descent_20480,
        leading_20480,
    };
    Ok((table, w.buf))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_define_text2_payload(
    text_id: u16,
    table: &SwfFont3Table,
    text: &str,
    width: f64,
    height: f64,
    font_size: f64,
    color: Color,
    align: TextAlign,
    leading_px: f64,
) -> Vec<u8> {
    let size_px = font_size.clamp(1.0, 1000.0);
    let height_twips = (size_px * 20.0).round().clamp(20.0, 65535.0) as u16;
    let ascent_twips = (f64::from(table.ascent_20480) * size_px / 1024.0).round() as i32;
    let descent_twips = (f64::from(table.descent_20480) * size_px / 1024.0).round() as i32;
    let font_lead_twips = (f64::from(table.leading_20480) * size_px / 1024.0).round() as i32;
    let extra_lead_twips = (leading_px * 20.0).round() as i32;
    let line_step_twips = (ascent_twips + descent_twips + font_lead_twips + extra_lead_twips)
        .max(i32::from(height_twips));

    let space_glyph = table.code_to_glyph.get(&0x0020).copied().unwrap_or(0);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let raw_lines: Vec<&str> = normalized.split('\n').collect();

    type PositionedTextLine = (i16, i16, Vec<(u32, i32)>);
    let mut line_runs: Vec<PositionedTextLine> = Vec::new();
    let mut max_line_adv_twips = 0i32;
    let mut all_advances: Vec<i32> = Vec::new();
    let mut max_glyph_idx = 0u32;

    let req_w_twips = (width.max(4.0) * 20.0).round() as i32;

    for (line_idx, line) in raw_lines.iter().enumerate() {
        let mut entries: Vec<(u32, i32)> = Vec::with_capacity(line.chars().count());
        let mut line_adv = 0i32;
        for ch in line.chars() {
            let code = u16::try_from(u32::from(ch)).unwrap_or(0x0020);
            let g_idx = table
                .code_to_glyph
                .get(&code)
                .copied()
                .unwrap_or(space_glyph);
            let adv_20480 = table.advances_20480.get(g_idx).copied().unwrap_or(10240);
            let adv_twips = (f64::from(adv_20480) * size_px / 1024.0)
                .round()
                .clamp(-32767.0, 32767.0) as i32;
            max_glyph_idx = max_glyph_idx.max(g_idx as u32);
            all_advances.push(adv_twips);
            line_adv = line_adv.saturating_add(adv_twips);
            entries.push((g_idx as u32, adv_twips));
        }
        max_line_adv_twips = max_line_adv_twips.max(line_adv);
        if entries.is_empty() {
            continue;
        }
        let x_off_i32 = match align {
            TextAlign::Left => 0,
            TextAlign::Center => ((req_w_twips - line_adv) / 2).max(0),
            TextAlign::Right => (req_w_twips - line_adv).max(0),
        };
        let y_off_i32 = ascent_twips.max(20) + (line_idx as i32) * line_step_twips;
        let x_off = x_off_i32.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let y_off = y_off_i32.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        line_runs.push((x_off, y_off, entries));
    }

    let box_w_twips = req_w_twips.max(max_line_adv_twips).max(80);
    let computed_h_twips = (raw_lines.len().max(1) as i32) * line_step_twips + descent_twips.max(0);
    let box_h_twips = ((height.max(4.0) * 20.0).round() as i32)
        .max(computed_h_twips)
        .max(80);

    let glyph_bits = needed_ub_bits(max_glyph_idx).max(1);
    let advance_bits = if all_advances.is_empty() {
        1
    } else {
        needed_sb_bits(&all_advances).max(1)
    };

    let mut w = SwfBitWriter::new();
    w.u16(text_id);
    w.rect_twips([0, box_w_twips, 0, box_h_twips]);
    w.matrix_px([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    w.u8(glyph_bits as u8);
    w.u8(advance_bits as u8);

    let mut first_record = true;
    for (x_off, y_off, entries) in line_runs {
        for (chunk_idx, chunk) in entries.chunks(127).enumerate() {
            w.align();
            let include_style = first_record;
            let include_pos = chunk_idx == 0;
            first_record = false;
            let mut rec_flags: u8 = 0x80;
            if include_style {
                rec_flags |= 0x08 | 0x04;
            }
            if include_pos {
                rec_flags |= 0x02 | 0x01;
            }
            w.u8(rec_flags);
            if include_style {
                w.u16(table.font_id);
                w.rgba(color);
            }
            if include_pos {
                w.i16(x_off);
                w.i16(y_off);
            }
            if include_style {
                w.u16(height_twips);
            }
            w.u8(chunk.len() as u8);
            for &(g_idx, adv) in chunk {
                w.ub(glyph_bits, g_idx);
                w.sb(advance_bits, adv);
            }
            w.align();
        }
    }

    w.u8(0); // EndOfRecordsFlag
    w.buf
}
