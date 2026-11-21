//! Selection-cropped vector PDF 1.4 (`com.adobe.pdf`) and PostScript Level 3 EPS
//! (`com.adobe.encapsulated-postscript`) clipboard exporters (`T-415`).
//!
//! Preserves editable cubic/quadratic paths, compound `EvenOdd`/`NonZero` holes, strokes,
//! Type 2 & Type 3 `/Shading` gradients (including multi-stop `/FunctionType 3`), `W`/`W*`
//! clipping masks, `/ExtGState` alpha & blend modes, `BT..ET` + `/ToUnicode` `CMap` text,
//! nested `Instance` `/Subtype /Form` XObjects, and `FlateDecode` RGB + `/SMask` alpha bitmaps.

use super::*;
use flate2::{Compression, write::ZlibEncoder};
use neoflash_core::{
    BlendMode, RuntimeClip, TextFieldState, affine_multiply, invert_affine_matrix, sample_layer_in,
    transform_matrix,
};
use std::collections::BTreeMap;
use std::io::Write as _;

fn compress_zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

fn pdf_blend_mode_name(bm: BlendMode) -> &'static str {
    match bm {
        BlendMode::Normal | BlendMode::Layer | BlendMode::Alpha | BlendMode::Erase => "Normal",
        BlendMode::Multiply | BlendMode::Subtract => "Multiply",
        BlendMode::Screen => "Screen",
        BlendMode::Overlay => "Overlay",
        BlendMode::Darken => "Darken",
        BlendMode::Lighten => "Lighten",
        BlendMode::Hardlight => "HardLight",
        BlendMode::Difference | BlendMode::Invert => "Difference",
        BlendMode::Add => "ColorDodge",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExtGStateKey {
    fill_alpha_milli: u16,
    stroke_alpha_milli: u16,
    blend_mode: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FontKey {
    base_font: String,
}

fn sanitize_pdf_base_font(family: &str, bold: bool, italic: bool) -> String {
    let mut clean: String = family
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if clean.is_empty() {
        clean = "Helvetica".to_owned();
    }
    let suffix = match (bold, italic) {
        (true, true) => "-BoldItalic",
        (true, false) => "-Bold",
        (false, true) => "-Italic",
        (false, false) => "",
    };
    format!("{clean}{suffix}")
}

struct PdfFontBuilder {
    base_font: String,
    /// Ordered map from UTF-16 code unit / BMP scalar (or surrogate code) -> 2-byte glyph ID (`1..=65535`).
    /// For non-BMP codepoints (`> 0xFFFF`), maps a dedicated glyph ID to the full UTF-16BE surrogate pair!
    glyph_by_str: BTreeMap<String, u16>,
    utf16be_by_glyph: Vec<(u16, Vec<u8>)>,
}

impl PdfFontBuilder {
    fn new(base_font: String) -> Self {
        Self {
            base_font,
            glyph_by_str: BTreeMap::new(),
            utf16be_by_glyph: Vec::new(),
        }
    }

    fn encode_text_hex(&mut self, text: &str) -> String {
        let mut hex = String::with_capacity(text.len() * 4 + 2);
        hex.push('<');
        for ch in text.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            let key = ch.to_string();
            let gid = if let Some(&existing) = self.glyph_by_str.get(&key) {
                existing
            } else {
                let next_id = u16::try_from(self.utf16be_by_glyph.len() + 1).unwrap_or(0xFFFD);
                let mut u16_buf = [0u16; 2];
                let encoded = ch.encode_utf16(&mut u16_buf);
                let mut be = Vec::with_capacity(encoded.len() * 2);
                for &unit in &*encoded {
                    be.extend_from_slice(&unit.to_be_bytes());
                }
                self.glyph_by_str.insert(key, next_id);
                self.utf16be_by_glyph.push((next_id, be));
                next_id
            };
            let _ = write!(hex, "{gid:04X}");
        }
        hex.push('>');
        hex
    }

    fn build_tounicode_cmap(&self) -> Vec<u8> {
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n\
             12 dict begin\n\
             begincmap\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
             /CMapName /Adobe-Identity-UCS def\n\
             /CMapType 2 def\n\
             1 begincodespacerange\n\
             <0000> <FFFF>\n\
             endcodespacerange\n",
        );
        for chunk in self.utf16be_by_glyph.chunks(100) {
            let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
            for (gid, be) in chunk {
                let mut dst_hex = String::with_capacity(be.len() * 2);
                for &b in be {
                    let _ = write!(dst_hex, "{b:02X}");
                }
                let _ = writeln!(cmap, "<{gid:04X}> <{dst_hex}>");
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str(
            "endcmap\n\
             CMapName currentdict /CMap defineresource pop\n\
             end\n\
             end\n",
        );
        cmap.into_bytes()
    }
}

struct PdfImageAsset {
    width: u32,
    height: u32,
    rgb_zlib: Vec<u8>,
    alpha_zlib: Option<Vec<u8>>,
}

struct PdfFormAsset {
    width: f64,
    height: f64,
    stream_body: String,
}

#[derive(Default)]
struct PdfResourceCollector {
    shadings: Vec<String>,
    ext_gstates: BTreeMap<ExtGStateKey, usize>,
    ext_gstate_list: Vec<ExtGStateKey>,
    fonts: BTreeMap<FontKey, usize>,
    font_list: Vec<PdfFontBuilder>,
    images: Vec<PdfImageAsset>,
    image_cache: HashMap<(String, [u8; 16]), usize>,
    forms: Vec<PdfFormAsset>,
}

impl PdfResourceCollector {
    fn intern_ext_gstate(&mut self, fill_a: f64, stroke_a: f64, bm: BlendMode) -> Option<usize> {
        let fa = (fill_a.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let sa = (stroke_a.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let bm_str = pdf_blend_mode_name(bm);
        if fa == 1000 && sa == 1000 && bm_str == "Normal" {
            return None;
        }
        let key = ExtGStateKey {
            fill_alpha_milli: fa,
            stroke_alpha_milli: sa,
            blend_mode: bm_str,
        };
        if let Some(&idx) = self.ext_gstates.get(&key) {
            return Some(idx);
        }
        let idx = self.ext_gstate_list.len();
        self.ext_gstate_list.push(key.clone());
        self.ext_gstates.insert(key, idx);
        Some(idx)
    }

    fn intern_font(&mut self, family: &str, bold: bool, italic: bool) -> usize {
        let base_font = sanitize_pdf_base_font(family, bold, italic);
        let key = FontKey {
            base_font: base_font.clone(),
        };
        if let Some(&idx) = self.fonts.get(&key) {
            return idx;
        }
        let idx = self.font_list.len();
        self.font_list.push(PdfFontBuilder::new(base_font));
        self.fonts.insert(key, idx);
        idx
    }

    fn intern_bitmap(
        &mut self,
        project: &Project,
        resource_id: &str,
        ct: ColorTransform,
    ) -> Option<(usize, u32, u32)> {
        let ct_sig = if ct.is_identity() {
            [0u8; 16]
        } else {
            let mut s = [0u8; 16];
            for (i, &m) in ct.multipliers.iter().enumerate() {
                let q = (m.clamp(-16.0, 16.0) * 1024.0).round() as i16;
                s[i * 2..i * 2 + 2].copy_from_slice(&q.to_le_bytes());
            }
            for (i, &o) in ct.offsets.iter().enumerate() {
                let q = o.clamp(-255.0, 255.0).round() as i16;
                s[8 + i * 2..8 + i * 2 + 2].copy_from_slice(&q.to_le_bytes());
            }
            s
        };
        if let Some(&idx) = self.image_cache.get(&(resource_id.to_owned(), ct_sig)) {
            let img = &self.images[idx];
            return Some((idx, img.width, img.height));
        }

        let res = project.resources.iter().find(|r| r.id == resource_id)?;
        let decoded = res.decode_image().ok()?;
        let w = decoded.width;
        let h = decoded.height;
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return None;
        }
        let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
        let mut alpha = Vec::with_capacity((w as usize) * (h as usize));
        let mut has_transparency = false;
        for px in decoded.rgba.as_chunks::<4>().0 {
            let c = ct.apply(*px);
            rgb.push(c[0]);
            rgb.push(c[1]);
            rgb.push(c[2]);
            alpha.push(c[3]);
            if c[3] < 255 {
                has_transparency = true;
            }
        }
        let idx = self.images.len();
        self.images.push(PdfImageAsset {
            width: w,
            height: h,
            rgb_zlib: compress_zlib(&rgb),
            alpha_zlib: has_transparency.then(|| compress_zlib(&alpha)),
        });
        self.image_cache
            .insert((resource_id.to_owned(), ct_sig), idx);
        Some((idx, w, h))
    }
}

fn transform_rect_corners(m: [f64; 6], w: f64, h: f64) -> [f64; 4] {
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for (cx, cy) in corners {
        let tx = m[0] * cx + m[2] * cy + m[4];
        let ty = m[1] * cx + m[3] * cy + m[5];
        x0 = x0.min(tx);
        y0 = y0.min(ty);
        x1 = x1.max(tx);
        y1 = y1.max(ty);
    }
    [x0, y0, x1, y1]
}

fn compute_scene_bounds(scene: &AuthoredScene, fallback_w: u32, fallback_h: u32) -> [f64; 4] {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    let mut include_box = |b: [f64; 4]| {
        if b.iter().all(|v| v.is_finite()) && b[2] >= b[0] && b[3] >= b[1] {
            min_x = min_x.min(b[0]);
            min_y = min_y.min(b[1]);
            max_x = max_x.max(b[2]);
            max_y = max_y.max(b[3]);
        }
    };

    for draw in &scene.draws {
        let m = draw.item.transform;
        match &draw.item.kind {
            ElementKind::Rectangle {
                width,
                height,
                stroke_width,
                ..
            }
            | ElementKind::Ellipse {
                width,
                height,
                stroke_width,
                ..
            } => {
                let mut b = transform_rect_corners(m, *width, *height);
                let pad = (*stroke_width * 0.5).max(0.0);
                b[0] -= pad;
                b[1] -= pad;
                b[2] += pad;
                b[3] += pad;
                include_box(b);
            }
            ElementKind::Path {
                commands,
                stroke_width,
                ..
            }
            | ElementKind::PaintedPath {
                commands,
                stroke_width,
                ..
            } => {
                let mut x0 = f64::INFINITY;
                let mut y0 = f64::INFINITY;
                let mut x1 = f64::NEG_INFINITY;
                let mut y1 = f64::NEG_INFINITY;
                let mut visit = |px: f64, py: f64| {
                    let tx = m[0] * px + m[2] * py + m[4];
                    let ty = m[1] * px + m[3] * py + m[5];
                    x0 = x0.min(tx);
                    y0 = y0.min(ty);
                    x1 = x1.max(tx);
                    y1 = y1.max(ty);
                };
                for cmd in commands {
                    match *cmd {
                        PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => visit(x, y),
                        PathCommand::QuadraticTo { cx, cy, x, y } => {
                            visit(cx, cy);
                            visit(x, y);
                        }
                        PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        } => {
                            visit(cx1, cy1);
                            visit(cx2, cy2);
                            visit(x, y);
                        }
                        PathCommand::Close => {}
                    }
                }
                let pad = (*stroke_width * 0.5).max(0.0);
                include_box([x0 - pad, y0 - pad, x1 + pad, y1 + pad]);
            }
            ElementKind::Bitmap { width, height, .. } => {
                include_box(transform_rect_corners(
                    m,
                    f64::from(*width),
                    f64::from(*height),
                ));
            }
            ElementKind::Video { width, height } => {
                include_box(transform_rect_corners(m, *width, *height));
            }
            ElementKind::Text {
                text, font_size, ..
            } => {
                let w = ((text.chars().count() as f64) * font_size * 0.62).max(*font_size);
                let h = (*font_size * 1.25).max(4.0);
                include_box(transform_rect_corners(m, w, h));
            }
            ElementKind::TextBox { width, height, .. } => {
                include_box(transform_rect_corners(m, *width, *height));
            }
            ElementKind::Instance { .. } => {}
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x > min_x && max_y > min_y {
        [min_x, min_y, max_x, max_y]
    } else {
        [
            0.0,
            0.0,
            f64::from(fallback_w.max(1)),
            f64::from(fallback_h.max(1)),
        ]
    }
}

fn emit_pdf_clips_open(
    out: &mut String,
    clip_ids: &[String],
    clips_map: &HashMap<&str, &RuntimeClip>,
) -> usize {
    let mut opened = 0usize;
    for cid in clip_ids {
        if let Some(clip) = clips_map.get(cid.as_str()) {
            let mut any = false;
            let mut path_buf = String::new();
            let mut rule = FillRule::NonZero;
            for comp in &clip.components {
                rule = comp.fill_rule;
                let transformed: Vec<PathCommand> = comp
                    .commands
                    .iter()
                    .map(|cmd| match *cmd {
                        PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
                            x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                            y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                        },
                        PathCommand::LineTo { x, y } => PathCommand::LineTo {
                            x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                            y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                        },
                        PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
                            cx: comp.matrix[0] * cx + comp.matrix[2] * cy + comp.matrix[4],
                            cy: comp.matrix[1] * cx + comp.matrix[3] * cy + comp.matrix[5],
                            x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                            y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                        },
                        PathCommand::CubicTo {
                            cx1,
                            cy1,
                            cx2,
                            cy2,
                            x,
                            y,
                        } => PathCommand::CubicTo {
                            cx1: comp.matrix[0] * cx1 + comp.matrix[2] * cy1 + comp.matrix[4],
                            cy1: comp.matrix[1] * cx1 + comp.matrix[3] * cy1 + comp.matrix[5],
                            cx2: comp.matrix[0] * cx2 + comp.matrix[2] * cy2 + comp.matrix[4],
                            cy2: comp.matrix[1] * cx2 + comp.matrix[3] * cy2 + comp.matrix[5],
                            x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                            y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                        },
                        PathCommand::Close => PathCommand::Close,
                    })
                    .collect();
                any |= pdf_path(&mut path_buf, &transformed);
            }
            if any {
                out.push_str("q\n");
                out.push_str(&path_buf);
                out.push_str(if rule == FillRule::EvenOdd {
                    "W* n\n"
                } else {
                    "W n\n"
                });
                opened += 1;
            }
        }
    }
    opened
}

fn emit_pdf_draw(
    out: &mut String,
    project: &Project,
    draw: &AuthoredDraw,
    local_matrix_override: Option<[f64; 6]>,
    clips_map: &HashMap<&str, &RuntimeClip>,
    collector: &mut PdfResourceCollector,
) {
    let item = &draw.item;
    let m = local_matrix_override.unwrap_or(item.transform);
    let clip_opens = emit_pdf_clips_open(out, &draw.clip_ids, clips_map);

    let _ = writeln!(
        out,
        "q\n{} {} {} {} {} {} cm",
        ps_num(m[0]),
        ps_num(m[1]),
        ps_num(m[2]),
        ps_num(m[3]),
        ps_num(m[4]),
        ps_num(m[5]),
    );

    let item_alpha = item.alpha.clamp(0.0, 1.0);

    let mut emit_styled_shape = |commands: &[PathCommand],
                                 rule: FillRule,
                                 fill: Color,
                                 stroke: Color,
                                 fill_gradient: Option<&GradientPaint>,
                                 stroke_gradient: Option<&GradientPaint>,
                                 stroke_width: f64| {
        let eff_fill = item.color_transform.apply(fill);
        let eff_stroke = item.color_transform.apply(stroke);
        let fill_a = if fill_gradient.is_some() {
            item_alpha
        } else {
            item_alpha * (f64::from(eff_fill[3]) / 255.0)
        };
        let stroke_a = if stroke_gradient.is_some() {
            item_alpha
        } else {
            item_alpha * (f64::from(eff_stroke[3]) / 255.0)
        };
        if let Some(gs_idx) = collector.intern_ext_gstate(fill_a, stroke_a, draw.blend_mode) {
            let _ = writeln!(out, "/GS{gs_idx} gs");
        }

        if let Some(fg) = fill_gradient {
            pdf_shading_clipped(
                out,
                &mut collector.shadings,
                commands,
                rule,
                fg,
                item.color_transform,
            );
        } else if eff_fill[3] > 0 && pdf_path(out, commands) {
            let _ = writeln!(
                out,
                "{} {} {} rg {}",
                ps_num(f64::from(eff_fill[0]) / 255.0),
                ps_num(f64::from(eff_fill[1]) / 255.0),
                ps_num(f64::from(eff_fill[2]) / 255.0),
                if rule == FillRule::EvenOdd { "f*" } else { "f" }
            );
        }

        if stroke_width > 0.0 {
            if let Some(sg) = stroke_gradient {
                let outline = neoflash_core::expand_variable_width_stroke(
                    commands,
                    stroke_width,
                    &neoflash_core::StrokeWidthProfile::default(),
                );
                pdf_shading_clipped(
                    out,
                    &mut collector.shadings,
                    &outline,
                    FillRule::NonZero,
                    sg,
                    item.color_transform,
                );
            } else if eff_stroke[3] > 0 && pdf_path(out, commands) {
                let _ = writeln!(
                    out,
                    "{} {} {} RG {} w 1 J 1 j S",
                    ps_num(f64::from(eff_stroke[0]) / 255.0),
                    ps_num(f64::from(eff_stroke[1]) / 255.0),
                    ps_num(f64::from(eff_stroke[2]) / 255.0),
                    ps_num(stroke_width),
                );
            }
        }
    };

    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            emit_styled_shape(
                &rectangle_path(*width, *height),
                FillRule::NonZero,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            );
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            emit_styled_shape(
                &ellipse_path(*width, *height),
                FillRule::NonZero,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            emit_styled_shape(
                commands,
                *fill_rule,
                *fill,
                *stroke,
                None,
                None,
                *stroke_width,
            );
        }
        ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
        } => {
            emit_styled_shape(
                commands,
                *fill_rule,
                *fill,
                *stroke,
                fill_gradient.as_ref(),
                stroke_gradient.as_ref(),
                *stroke_width,
            );
        }
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            ..
        } => {
            if let Some((img_idx, _, _)) =
                collector.intern_bitmap(project, resource_id, item.color_transform)
            {
                if let Some(gs_idx) =
                    collector.intern_ext_gstate(item_alpha, item_alpha, draw.blend_mode)
                {
                    let _ = writeln!(out, "/GS{gs_idx} gs");
                }
                let w = f64::from(*width).max(1.0);
                let h = f64::from(*height).max(1.0);
                let _ = writeln!(
                    out,
                    "q\n{} 0 0 -{} 0 {} cm\n/Im{img_idx} Do\nQ",
                    ps_num(w),
                    ps_num(h),
                    ps_num(h),
                );
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let eff = item.color_transform.apply(*color);
            if eff[3] > 0 && !text.is_empty() {
                let fill_a = item_alpha * (f64::from(eff[3]) / 255.0);
                if let Some(gs_idx) = collector.intern_ext_gstate(fill_a, fill_a, draw.blend_mode) {
                    let _ = writeln!(out, "/GS{gs_idx} gs");
                }
                let f_idx = collector.intern_font(font_family, false, false);
                let sz = font_size.max(1.0);
                let _ = writeln!(
                    out,
                    "{} {} {} rg",
                    ps_num(f64::from(eff[0]) / 255.0),
                    ps_num(f64::from(eff[1]) / 255.0),
                    ps_num(f64::from(eff[2]) / 255.0),
                );
                for (line_idx, raw_line) in text.split('\n').enumerate() {
                    let line = raw_line.trim_end_matches('\r');
                    if line.is_empty() {
                        continue;
                    }
                    let hex = collector.font_list[f_idx].encode_text_hex(line);
                    let baseline_y = sz * 0.8 + (line_idx as f64) * sz * 1.2;
                    let _ = writeln!(
                        out,
                        "BT /F{f_idx} {} Tf 1 0 0 -1 0 {} Tm {hex} Tj ET",
                        ps_num(sz),
                        ps_num(baseline_y),
                    );
                }
            }
        }
        ElementKind::TextBox { .. } => {
            if let Some(field) = TextFieldState::from_kind(&item.kind)
                && field.alpha > 0
                && !field.text.is_empty()
                && let Ok(layout) = field.layout_portable(None)
            {
                let base_alpha = item_alpha * (f64::from(field.alpha) / 255.0);
                if let Some(gs_idx) =
                    collector.intern_ext_gstate(base_alpha, base_alpha, draw.blend_mode)
                {
                    let _ = writeln!(out, "/GS{gs_idx} gs");
                }
                struct CoalescedPdfTextRun {
                    x: f64,
                    y: f64,
                    text: String,
                    fmt: neoflash_core::TextFormat,
                }
                let mut merged_runs: Vec<CoalescedPdfTextRun> = Vec::new();
                if layout.runs.is_empty() {
                    for line in &layout.lines {
                        let piece = line.text.trim_end_matches(['\r', '\n']);
                        if piece.is_empty() {
                            continue;
                        }
                        merged_runs.push(CoalescedPdfTextRun {
                            x: line.x,
                            y: line.y,
                            text: piece.to_owned(),
                            fmt: field.format.clone(),
                        });
                    }
                } else {
                    for run in &layout.runs {
                        let piece = run.text.trim_end_matches(['\r', '\n']);
                        if piece.is_empty() {
                            continue;
                        }
                        let fmt = run.format.as_ref().unwrap_or(&field.format);
                        if let Some(last) = merged_runs.last_mut()
                            && (last.y - run.y).abs() <= 1e-3
                            && last.fmt.font == fmt.font
                            && (last.fmt.size - fmt.size).abs() <= 1e-3
                            && last.fmt.bold == fmt.bold
                            && last.fmt.italic == fmt.italic
                            && last.fmt.color == fmt.color
                            && (last.fmt.letter_spacing - fmt.letter_spacing).abs() <= 1e-3
                        {
                            last.text.push_str(piece);
                        } else {
                            merged_runs.push(CoalescedPdfTextRun {
                                x: run.x,
                                y: run.y,
                                text: piece.to_owned(),
                                fmt: fmt.clone(),
                            });
                        }
                    }
                }
                if !merged_runs.is_empty() {
                    out.push_str("BT\n");
                    for mrun in &merged_runs {
                        if mrun.text.trim().is_empty() {
                            continue;
                        }
                        let fmt = &mrun.fmt;
                        let raw_col = [
                            ((fmt.color >> 16) & 0xff) as u8,
                            ((fmt.color >> 8) & 0xff) as u8,
                            (fmt.color & 0xff) as u8,
                            255,
                        ];
                        let eff = item.color_transform.apply(raw_col);
                        let f_idx = collector.intern_font(&fmt.font, fmt.bold, fmt.italic);
                        let hex = collector.font_list[f_idx].encode_text_hex(&mrun.text);
                        let sz = fmt.size.max(1.0);
                        let baseline_y = mrun.y + sz * 0.8;
                        let _ = writeln!(
                            out,
                            "{} {} {} rg",
                            ps_num(f64::from(eff[0]) / 255.0),
                            ps_num(f64::from(eff[1]) / 255.0),
                            ps_num(f64::from(eff[2]) / 255.0),
                        );
                        if fmt.letter_spacing.abs() > 1e-6 {
                            let _ = writeln!(
                                out,
                                "/F{f_idx} {} Tf {} Tc 1 0 0 -1 {} {} Tm {hex} Tj 0 Tc",
                                ps_num(sz),
                                ps_num(fmt.letter_spacing),
                                ps_num(mrun.x),
                                ps_num(baseline_y),
                            );
                        } else {
                            let _ = writeln!(
                                out,
                                "/F{f_idx} {} Tf 1 0 0 -1 {} {} Tm {hex} Tj",
                                ps_num(sz),
                                ps_num(mrun.x),
                                ps_num(baseline_y),
                            );
                        }
                    }
                    out.push_str("ET\n");
                }
            }
        }
        ElementKind::Instance { .. } | ElementKind::Video { .. } => {}
    }

    out.push_str("Q\n");
    for _ in 0..clip_opens {
        out.push_str("Q\n");
    }
}

/// Export a selection-cropped vector PDF 1.4 (`com.adobe.pdf` / `NSPasteboardTypePDF`)
/// with `/MediaBox [0 0 stage_w stage_h]` and `/ArtBox` + `/CropBox` set to exact selection bounds.
pub fn export_clipboard_pdf(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    selection_bounds: Option<[f64; 4]>,
) -> Result<Vec<u8>, ExportError> {
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    let eval_project = if symbol_id.is_none() && !project.scenes.is_empty() {
        flatten_scenes_with_ranges(project).0
    } else {
        project.clone()
    };
    let target = symbol_id.unwrap_or(eval_project.root.as_str());
    let scene = evaluate_scene(&eval_project, target, frame, false, true);

    let stage_w = eval_project.width.max(1);
    let stage_h = eval_project.height.max(1);
    let stage_h_f = f64::from(stage_h);

    let [sel_x0, sel_y0, sel_x1, sel_y1] = selection_bounds
        .filter(|b| b.iter().all(|v| v.is_finite()) && b[2] > b[0] && b[3] > b[1])
        .unwrap_or_else(|| compute_scene_bounds(&scene, stage_w, stage_h));

    let pdf_y0 = stage_h_f - sel_y1;
    let pdf_y1 = stage_h_f - sel_y0;

    // Collect top-level Instance matrices on `target` at `frame` so nested instances export as `/Subtype /Form` XObjects!
    let mut top_instances: HashMap<String, ([f64; 6], BlendMode)> = HashMap::new();
    if let Some(sym) = eval_project.symbol(target) {
        for layer in &sym.timeline.layers {
            if !layer_effectively_visible(&sym.timeline, layer)
                || matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder)
            {
                continue;
            }
            for el in sample_layer_in(&sym.timeline, layer, frame) {
                if !el.visible {
                    continue;
                }
                if matches!(el.kind, ElementKind::Instance { .. }) {
                    top_instances.insert(
                        el.id.clone(),
                        (transform_matrix(el.transform), el.blend_mode),
                    );
                }
            }
        }
    }

    let clips_map: HashMap<&str, &RuntimeClip> =
        scene.clips.iter().map(|c| (c.id.as_str(), c)).collect();

    let mut collector = PdfResourceCollector::default();
    let mut page_stream = String::new();
    let _ = writeln!(page_stream, "q\n1 0 0 -1 0 {} cm", ps_num(stage_h_f));

    let mut idx = 0usize;
    while idx < scene.draws.len() {
        let first_draw = &scene.draws[idx];
        if first_draw.item.instance_path.len() >= 2
            && let root_inst_id = &first_draw.item.instance_path[0]
            && let Some(&(inst_mat, inst_bm)) = top_instances.get(root_inst_id)
            && let Some(inv_inst) = invert_affine_matrix(inst_mat)
        {
            // Group contiguous draws belonging to this top-level instance into a `/Subtype /Form` XObject!
            let mut end = idx + 1;
            while end < scene.draws.len()
                && scene.draws[end].item.instance_path.len() >= 2
                && &scene.draws[end].item.instance_path[0] == root_inst_id
            {
                end += 1;
            }

            let mut shared_outer_clips = first_draw.clip_ids.clone();
            for d in &scene.draws[idx + 1..end] {
                shared_outer_clips.retain(|cid| d.clip_ids.contains(cid));
            }

            let owned_local_clips: Vec<RuntimeClip> = scene
                .clips
                .iter()
                .map(|c| {
                    let mut lc = c.clone();
                    for comp in &mut lc.components {
                        comp.matrix = affine_multiply(inv_inst, comp.matrix);
                    }
                    lc
                })
                .collect();
            let local_clips_map: HashMap<&str, &RuntimeClip> = owned_local_clips
                .iter()
                .map(|c| (c.id.as_str(), c))
                .collect();

            let mut local_max_x = 100.0f64;
            let mut local_max_y = 100.0f64;
            let mut local_draws: Vec<[f64; 6]> = Vec::with_capacity(end - idx);
            for d in &scene.draws[idx..end] {
                let lm = affine_multiply(inv_inst, d.item.transform);
                local_max_x = local_max_x.max(lm[4].abs() + 256.0);
                local_max_y = local_max_y.max(lm[5].abs() + 256.0);
                local_draws.push(lm);
            }
            let form_w = local_max_x.clamp(16.0, 8_192.0);
            let form_h = local_max_y.clamp(16.0, 8_192.0);

            let mut form_body = String::new();
            let _ = writeln!(form_body, "q\n1 0 0 -1 0 {} cm", ps_num(form_h));
            for (offset, d) in scene.draws[idx..end].iter().enumerate() {
                let mut inner_draw = d.clone();
                inner_draw
                    .clip_ids
                    .retain(|cid| !shared_outer_clips.contains(cid));
                emit_pdf_draw(
                    &mut form_body,
                    &eval_project,
                    &inner_draw,
                    Some(local_draws[offset]),
                    &local_clips_map,
                    &mut collector,
                );
            }
            form_body.push_str("Q\n");

            let form_idx = collector.forms.len();
            collector.forms.push(PdfFormAsset {
                width: form_w,
                height: form_h,
                stream_body: form_body,
            });

            let clip_opens = emit_pdf_clips_open(&mut page_stream, &shared_outer_clips, &clips_map);
            page_stream.push_str("q\n");
            if let Some(gs_idx) = collector.intern_ext_gstate(1.0, 1.0, inst_bm) {
                let _ = writeln!(page_stream, "/GS{gs_idx} gs");
            }
            let _ = writeln!(
                page_stream,
                "{} {} {} {} {} {} cm\n1 0 0 -1 0 {} cm\n/Fm{form_idx} Do\nQ",
                ps_num(inst_mat[0]),
                ps_num(inst_mat[1]),
                ps_num(inst_mat[2]),
                ps_num(inst_mat[3]),
                ps_num(inst_mat[4]),
                ps_num(inst_mat[5]),
                ps_num(form_h),
            );
            for _ in 0..clip_opens {
                page_stream.push_str("Q\n");
            }
            idx = end;
            continue;
        }

        emit_pdf_draw(
            &mut page_stream,
            &eval_project,
            first_draw,
            None,
            &clips_map,
            &mut collector,
        );
        idx += 1;
    }

    page_stream.push_str("Q\n");

    // Assign PDF indirect object IDs deterministically:
    // 1 = Catalog, 2 = Pages, 3 = Page, 4 = Page Contents, 5 = Shared Resources Dictionary
    let mut next_obj_id = 6usize;

    let shading_obj_ids: Vec<usize> = (0..collector.shadings.len())
        .map(|_| {
            let id = next_obj_id;
            next_obj_id += 1;
            id
        })
        .collect();

    let gs_obj_ids: Vec<usize> = (0..collector.ext_gstate_list.len())
        .map(|_| {
            let id = next_obj_id;
            next_obj_id += 1;
            id
        })
        .collect();

    // Each font uses 3 objects: (Type0 Font, CIDFontType2 Descendant, ToUnicode CMap stream)
    let font_obj_triples: Vec<(usize, usize, usize)> = (0..collector.font_list.len())
        .map(|_| {
            let t0 = next_obj_id;
            let cid = next_obj_id + 1;
            let cmap = next_obj_id + 2;
            next_obj_id += 3;
            (t0, cid, cmap)
        })
        .collect();

    // Each image uses 1 or 2 objects: (Image stream, optional SMask stream)
    let image_obj_pairs: Vec<(usize, Option<usize>)> = collector
        .images
        .iter()
        .map(|img| {
            let im_id = next_obj_id;
            next_obj_id += 1;
            let sm_id = img.alpha_zlib.as_ref().map(|_| {
                let s = next_obj_id;
                next_obj_id += 1;
                s
            });
            (im_id, sm_id)
        })
        .collect();

    let form_obj_ids: Vec<usize> = (0..collector.forms.len())
        .map(|_| {
            let id = next_obj_id;
            next_obj_id += 1;
            id
        })
        .collect();

    let total_objs = next_obj_id;
    let mut offsets = vec![0usize; total_objs];
    let mut pdf = Vec::with_capacity(page_stream.len() + 4096);
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    offsets[1] = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets[2] = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets[3] = pdf.len();
    let page_dict = format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {stage_w} {stage_h}] \
         /CropBox [{} {} {} {}] /ArtBox [{} {} {} {}] \
         /Resources 5 0 R /Contents 4 0 R >>\nendobj\n",
        ps_num(sel_x0),
        ps_num(pdf_y0),
        ps_num(sel_x1),
        ps_num(pdf_y1),
        ps_num(sel_x0),
        ps_num(pdf_y0),
        ps_num(sel_x1),
        ps_num(pdf_y1),
    );
    pdf.extend_from_slice(page_dict.as_bytes());

    offsets[4] = pdf.len();
    let page_zlib = compress_zlib(page_stream.as_bytes());
    let _ = write!(
        pdf,
        "4 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
        page_zlib.len()
    );
    pdf.extend_from_slice(&page_zlib);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");

    // Shared Resources Dictionary (Object 5) — shared by Page AND Form XObjects!
    offsets[5] = pdf.len();
    let mut res_str = String::from("5 0 obj\n<< ");
    if !shading_obj_ids.is_empty() {
        res_str.push_str("/Shading << ");
        for (i, &oid) in shading_obj_ids.iter().enumerate() {
            let _ = write!(res_str, "/Sh{i} {oid} 0 R ");
        }
        res_str.push_str(">> ");
    }
    if !gs_obj_ids.is_empty() {
        res_str.push_str("/ExtGState << ");
        for (i, &oid) in gs_obj_ids.iter().enumerate() {
            let _ = write!(res_str, "/GS{i} {oid} 0 R ");
        }
        res_str.push_str(">> ");
    }
    if !font_obj_triples.is_empty() {
        res_str.push_str("/Font << ");
        for (i, &(t0, _, _)) in font_obj_triples.iter().enumerate() {
            let _ = write!(res_str, "/F{i} {t0} 0 R ");
        }
        res_str.push_str(">> ");
    }
    if !image_obj_pairs.is_empty() || !form_obj_ids.is_empty() {
        res_str.push_str("/XObject << ");
        for (i, &(im_id, _)) in image_obj_pairs.iter().enumerate() {
            let _ = write!(res_str, "/Im{i} {im_id} 0 R ");
        }
        for (i, &fm_id) in form_obj_ids.iter().enumerate() {
            let _ = write!(res_str, "/Fm{i} {fm_id} 0 R ");
        }
        res_str.push_str(">> ");
    }
    res_str.push_str(">>\nendobj\n");
    pdf.extend_from_slice(res_str.as_bytes());

    for (i, sh_dict) in collector.shadings.iter().enumerate() {
        let oid = shading_obj_ids[i];
        offsets[oid] = pdf.len();
        let _ = write!(pdf, "{oid} 0 obj\n{sh_dict}\nendobj\n");
    }

    for (i, gs) in collector.ext_gstate_list.iter().enumerate() {
        let oid = gs_obj_ids[i];
        offsets[oid] = pdf.len();
        let _ = write!(
            pdf,
            "{oid} 0 obj\n<< /Type /ExtGState /ca {} /CA {} /BM /{} >>\nendobj\n",
            ps_num(f64::from(gs.fill_alpha_milli) / 1000.0),
            ps_num(f64::from(gs.stroke_alpha_milli) / 1000.0),
            gs.blend_mode,
        );
    }

    for (i, fb) in collector.font_list.iter().enumerate() {
        let (t0, cid, cmap_oid) = font_obj_triples[i];
        offsets[t0] = pdf.len();
        let _ = write!(
            pdf,
            "{t0} 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H \
             /DescendantFonts [{cid} 0 R] /ToUnicode {cmap_oid} 0 R >>\nendobj\n",
            fb.base_font,
        );

        offsets[cid] = pdf.len();
        let _ = write!(
            pdf,
            "{cid} 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{} \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /DW 600 >>\nendobj\n",
            fb.base_font,
        );

        offsets[cmap_oid] = pdf.len();
        let cmap_zlib = compress_zlib(&fb.build_tounicode_cmap());
        let _ = write!(
            pdf,
            "{cmap_oid} 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            cmap_zlib.len(),
        );
        pdf.extend_from_slice(&cmap_zlib);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }

    for (i, img) in collector.images.iter().enumerate() {
        let (im_id, sm_opt) = image_obj_pairs[i];
        offsets[im_id] = pdf.len();
        let smask_entry = sm_opt
            .map(|sid| format!(" /SMask {sid} 0 R"))
            .unwrap_or_default();
        let _ = write!(
            pdf,
            "{im_id} 0 obj\n<< /Type /XObject /Subtype /Image /Width {} /Height {} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode{smask_entry} \
             /Length {} >>\nstream\n",
            img.width,
            img.height,
            img.rgb_zlib.len(),
        );
        pdf.extend_from_slice(&img.rgb_zlib);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");

        if let (Some(sid), Some(alpha_zlib)) = (sm_opt, &img.alpha_zlib) {
            offsets[sid] = pdf.len();
            let _ = write!(
                pdf,
                "{sid} 0 obj\n<< /Type /XObject /Subtype /Image /Width {} /Height {} \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode \
                 /Length {} >>\nstream\n",
                img.width,
                img.height,
                alpha_zlib.len(),
            );
            pdf.extend_from_slice(alpha_zlib);
            pdf.extend_from_slice(b"\nendstream\nendobj\n");
        }
    }

    for (i, fm) in collector.forms.iter().enumerate() {
        let oid = form_obj_ids[i];
        offsets[oid] = pdf.len();
        let fm_zlib = compress_zlib(fm.stream_body.as_bytes());
        let _ = write!(
            pdf,
            "{oid} 0 obj\n<< /Type /XObject /Subtype /Form /FormType 1 \
             /BBox [-4096 -4096 {} {}] /Matrix [1 0 0 1 0 0] /Resources 5 0 R \
             /Filter /FlateDecode /Length {} >>\nstream\n",
            ps_num(fm.width + 4096.0),
            ps_num(fm.height + 4096.0),
            fm_zlib.len(),
        );
        pdf.extend_from_slice(&fm_zlib);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }

    let xref_offset = pdf.len();
    let _ = writeln!(pdf, "xref\n0 {total_objs}\n0000000000 65535 f ");
    for &off in &offsets[1..total_objs] {
        let _ = writeln!(pdf, "{off:010} 00000 n ");
    }
    let _ = write!(
        pdf,
        "trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
    );
    Ok(pdf)
}

/// Export a selection-cropped PostScript Level 3 EPS (`com.adobe.encapsulated-postscript`)
/// preserving vector paths, strokes, compound holes, `clip`/`eoclip`, Level 3 `shfill`
/// shadings, text, and per-element `colorimage` bitmaps.
pub fn export_clipboard_eps(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    selection_bounds: Option<[f64; 4]>,
) -> Result<Vec<u8>, ExportError> {
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;
    let eval_project = if symbol_id.is_none() && !project.scenes.is_empty() {
        flatten_scenes_with_ranges(project).0
    } else {
        project.clone()
    };
    let target = symbol_id.unwrap_or(eval_project.root.as_str());
    let scene = evaluate_scene(&eval_project, target, frame, false, true);

    let stage_w = eval_project.width.max(1);
    let stage_h = eval_project.height.max(1);
    let stage_h_f = f64::from(stage_h);

    let [sel_x0, sel_y0, sel_x1, sel_y1] = selection_bounds
        .filter(|b| b.iter().all(|v| v.is_finite()) && b[2] > b[0] && b[3] > b[1])
        .unwrap_or_else(|| compute_scene_bounds(&scene, stage_w, stage_h));

    let ps_y0 = stage_h_f - sel_y1;
    let ps_y1 = stage_h_f - sel_y0;
    let ibx0 = sel_x0.floor() as i64;
    let iby0 = ps_y0.floor() as i64;
    let ibx1 = sel_x1.ceil() as i64;
    let iby1 = ps_y1.ceil() as i64;

    let mut out = format!(
        "%!PS-Adobe-3.0 EPSF-3.0\n\
         %%Creator: NeoFlash\n\
         %%Title: {}\n\
         %%BoundingBox: {ibx0} {iby0} {ibx1} {iby1}\n\
         %%HiResBoundingBox: {} {} {} {}\n\
         %%LanguageLevel: 3\n\
         %%Pages: 1\n\
         %AI5_FileFormat 12.0\n\
         %AI5_ArtSize: {stage_w} {stage_h}\n\
         %%EndComments\n\
         %%Page: 1 1\n\
         gsave\n\
         [1 0 0 -1 0 {}] concat\n",
        ps_comment(&project.name),
        ps_num(sel_x0),
        ps_num(ps_y0),
        ps_num(sel_x1),
        ps_num(ps_y1),
        ps_num(stage_h_f),
    );

    let clips_map: HashMap<&str, &RuntimeClip> =
        scene.clips.iter().map(|c| (c.id.as_str(), c)).collect();
    let layer_names: HashMap<&str, &str> = eval_project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .map(|l| (l.id.as_str(), l.name.as_str()))
        .collect();

    let mut current_layer: Option<&str> = None;
    let ai_opts = ImageExportOptions {
        format: ImageExportFormat::Ai,
        ai_strokes_as_strokes: true,
        ai_text_as_outlines: false,
        transparent_background: true,
        ..ImageExportOptions::default()
    };

    for draw in &scene.draws {
        let lid = draw.item.layer_id.as_str();
        if current_layer != Some(lid) {
            if current_layer.is_some() {
                out.push_str("%AI5_EndLayer--\n");
            }
            out.push_str("%AI5_BeginLayer\n");
            let lname = layer_names.get(lid).copied().unwrap_or(lid);
            let _ = writeln!(out, "%AI5_Name: ({})", ps_string(lname));
            current_layer = Some(lid);
        }

        let mut clip_depth = 0usize;
        for cid in &draw.clip_ids {
            if let Some(clip) = clips_map.get(cid.as_str()) {
                let mut buf = String::new();
                let mut rule = FillRule::NonZero;
                let mut any = false;
                for comp in &clip.components {
                    rule = comp.fill_rule;
                    let transformed: Vec<PathCommand> = comp
                        .commands
                        .iter()
                        .map(|cmd| match *cmd {
                            PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
                                x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                                y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                            },
                            PathCommand::LineTo { x, y } => PathCommand::LineTo {
                                x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                                y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                            },
                            PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
                                cx: comp.matrix[0] * cx + comp.matrix[2] * cy + comp.matrix[4],
                                cy: comp.matrix[1] * cx + comp.matrix[3] * cy + comp.matrix[5],
                                x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                                y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                            },
                            PathCommand::CubicTo {
                                cx1,
                                cy1,
                                cx2,
                                cy2,
                                x,
                                y,
                            } => PathCommand::CubicTo {
                                cx1: comp.matrix[0] * cx1 + comp.matrix[2] * cy1 + comp.matrix[4],
                                cy1: comp.matrix[1] * cx1 + comp.matrix[3] * cy1 + comp.matrix[5],
                                cx2: comp.matrix[0] * cx2 + comp.matrix[2] * cy2 + comp.matrix[4],
                                cy2: comp.matrix[1] * cx2 + comp.matrix[3] * cy2 + comp.matrix[5],
                                x: comp.matrix[0] * x + comp.matrix[2] * y + comp.matrix[4],
                                y: comp.matrix[1] * x + comp.matrix[3] * y + comp.matrix[5],
                            },
                            PathCommand::Close => PathCommand::Close,
                        })
                        .collect();
                    any |= ps_path(&mut buf, &transformed);
                }
                if any {
                    out.push_str("gsave\n");
                    out.push_str(&buf);
                    out.push_str(if rule == FillRule::EvenOdd {
                        "eoclip\nnewpath\n"
                    } else {
                        "clip\nnewpath\n"
                    });
                    clip_depth += 1;
                }
            }
        }

        if let ElementKind::Bitmap {
            resource_id,
            width,
            height,
            ..
        } = &draw.item.kind
        {
            if let Some(res) = eval_project.resources.iter().find(|r| r.id == *resource_id)
                && let Ok(decoded) = res.decode_image()
                && decoded.width > 0
                && decoded.height > 0
            {
                let m = draw.item.transform;
                let _ = writeln!(
                    out,
                    "gsave\n[{} {} {} {} {} {}] concat\n{} {} scale",
                    ps_num(m[0]),
                    ps_num(m[1]),
                    ps_num(m[2]),
                    ps_num(m[3]),
                    ps_num(m[4]),
                    ps_num(m[5]),
                    ps_num(f64::from(*width).max(1.0)),
                    ps_num(f64::from(*height).max(1.0)),
                );
                let iw = decoded.width;
                let ih = decoded.height;
                let _ = writeln!(out, "/picstr {} string def", iw.saturating_mul(3));
                let _ = writeln!(out, "{iw} {ih} 8 [{iw} 0 0 -{ih} 0 {ih}]");
                out.push_str("{ currentfile picstr readhexstring pop } false 3 colorimage\n");
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let mut line = 0usize;
                for px in decoded.rgba.as_chunks::<4>().0 {
                    let c = draw.item.color_transform.apply(*px);
                    let a = u16::from(c[3]);
                    for &ch in &c[..3] {
                        let v = ((u16::from(ch) * a + 255 * (255 - a)) / 255) as u8;
                        out.push(char::from(HEX[(v >> 4) as usize]));
                        out.push(char::from(HEX[(v & 15) as usize]));
                        line += 2;
                        if line >= 72 {
                            out.push('\n');
                            line = 0;
                        }
                    }
                }
                if line != 0 {
                    out.push('\n');
                }
                out.push_str("grestore\n");
            }
        } else {
            ps_draw(&mut out, draw, &ai_opts);
        }

        for _ in 0..clip_depth {
            out.push_str("grestore\n");
        }
    }

    if current_layer.is_some() {
        out.push_str("%AI5_EndLayer--\n");
    }
    out.push_str("grestore\nshowpage\n%%EOF\n");
    Ok(out.into_bytes())
}
