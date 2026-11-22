//! Font, text, sprite, placement, and timeline reconstruction for SWF Loader bundles.

use super::tags_extra::{
    attach_element_object_script, build_glyph_path_commands, ensure_morph_ratio_symbol,
    parse_clip_actions_to_js, parse_glyph_shape_records,
};
use super::vm::compile_avm1_actions;
use super::{
    BitReader, MAX_SWF_FRAMES, MAX_SWF_KEYFRAMES, MAX_SWF_PLACEMENTS, SwfDecodeCtx, SwfFontRecord,
    attach_compiled_frame_script, make_default_element, parse_tag_slice,
};
use neoflash_core::{
    BlendMode, Color, ColorTransform, Element, ElementKind, FillRule, FilterSpec, FrameLabelType,
    InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MAX_OBJECT_FILTERS, PathCommand,
    Symbol, SymbolKind, TextFormat, Timeline, Transform,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, PartialEq)]
pub(super) struct ActivePlacement {
    pub(super) char_id: u16,
    pub(super) matrix: [f64; 6],
    pub(super) cxform: ColorTransform,
    pub(super) name: String,
    pub(super) clip_depth: u16,
    pub(super) visible: bool,
    pub(super) cache_as_bitmap: bool,
    pub(super) blend_mode: BlendMode,
    pub(super) filters: Vec<FilterSpec>,
    pub(super) ratio: u16,
    pub(super) clip_script_js: Option<String>,
}

pub(super) fn decode_font_tag(
    ctx: &mut SwfDecodeCtx,
    data: &[u8],
    tag_code: u16,
) -> Result<(), String> {
    if data.len() < 4 {
        return Ok(());
    }
    let font_id = u16::from_le_bytes([data[0], data[1]]);
    if tag_code == 10 {
        let mut glyph_shapes = Vec::new();
        if data.len() >= 4 {
            let first_off = usize::from(u16::from_le_bytes([data[2], data[3]]));
            let num_glyphs = (first_off / 2).min(4096);
            if 2 + num_glyphs * 2 <= data.len() {
                for i in 0..num_glyphs {
                    let off =
                        usize::from(u16::from_le_bytes([data[2 + i * 2], data[2 + i * 2 + 1]]));
                    let shape_pos = 2usize.saturating_add(off);
                    if shape_pos < data.len() {
                        let mut sr = BitReader::new(data, shape_pos);
                        glyph_shapes.push(parse_glyph_shape_records(&mut sr));
                    } else {
                        glyph_shapes.push(Vec::new());
                    }
                }
            }
        }
        ctx.fonts.entry(font_id).or_insert_with(|| SwfFontRecord {
            name: "sans-serif".into(),
            codes: Vec::new(),
            glyph_shapes,
            em_units: 1024.0,
        });
        return Ok(());
    }
    let em_units = if tag_code == 75 { 20480.0 } else { 1024.0 };
    let flags = data[2];
    let wide_offsets = (flags & 0x08) != 0;
    let wide_codes = (flags & 0x04) != 0 || tag_code == 75;
    let name_len = usize::from(data[4]);
    if 5 + name_len + 2 > data.len() {
        return Ok(());
    }
    let raw_name = String::from_utf8_lossy(&data[5..5 + name_len])
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    let mut pos = 5 + name_len;
    let num_glyphs = usize::from(u16::from_le_bytes([data[pos], data[pos + 1]]));
    pos += 2;
    if num_glyphs == 0 {
        ctx.fonts.insert(
            font_id,
            SwfFontRecord {
                name: if raw_name.is_empty() {
                    "sans-serif".into()
                } else {
                    raw_name
                },
                codes: Vec::new(),
                glyph_shapes: Vec::new(),
                em_units,
            },
        );
        return Ok(());
    }
    let offset_table_base = pos;
    let mut glyph_offsets = Vec::with_capacity(num_glyphs.min(8192));
    let code_table_offset = if wide_offsets {
        let need = (num_glyphs + 1) * 4;
        if pos + need > data.len() {
            return Ok(());
        }
        for i in 0..num_glyphs.min(8192) {
            let p = pos + i * 4;
            glyph_offsets.push(
                u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize,
            );
        }
        let off_pos = pos + num_glyphs * 4;
        u32::from_le_bytes([
            data[off_pos],
            data[off_pos + 1],
            data[off_pos + 2],
            data[off_pos + 3],
        ]) as usize
    } else {
        let need = (num_glyphs + 1) * 2;
        if pos + need > data.len() {
            return Ok(());
        }
        for i in 0..num_glyphs.min(8192) {
            let p = pos + i * 2;
            glyph_offsets.push(usize::from(u16::from_le_bytes([data[p], data[p + 1]])));
        }
        let off_pos = pos + num_glyphs * 2;
        usize::from(u16::from_le_bytes([data[off_pos], data[off_pos + 1]]))
    };
    let ct_start = offset_table_base + code_table_offset;
    let mut glyph_shapes = Vec::with_capacity(glyph_offsets.len());
    for g_off in glyph_offsets {
        let s_pos = offset_table_base.saturating_add(g_off);
        if s_pos < ct_start.min(data.len()) {
            let mut sr = BitReader::new(&data[..ct_start.min(data.len())], s_pos);
            glyph_shapes.push(parse_glyph_shape_records(&mut sr));
        } else {
            glyph_shapes.push(Vec::new());
        }
    }
    let mut codes = Vec::with_capacity(num_glyphs);
    if wide_codes {
        for i in 0..num_glyphs {
            let p = ct_start + i * 2;
            if p + 2 > data.len() {
                break;
            }
            codes.push(u16::from_le_bytes([data[p], data[p + 1]]));
        }
    } else {
        for i in 0..num_glyphs {
            let p = ct_start + i;
            if p >= data.len() {
                break;
            }
            codes.push(u16::from(data[p]));
        }
    }
    ctx.fonts.insert(
        font_id,
        SwfFontRecord {
            name: if raw_name.is_empty() {
                "sans-serif".into()
            } else {
                raw_name
            },
            codes,
            glyph_shapes,
            em_units,
        },
    );
    Ok(())
}

pub(super) fn decode_font_name_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let font_id = r.read_u16()?;
    let font_name = r.read_c_string()?;
    if !font_name.trim().is_empty()
        && let Some(rec) = ctx.fonts.get_mut(&font_id)
    {
        rec.name = font_name;
    }
    Ok(())
}

pub(super) fn decode_static_text_tag(
    ctx: &mut SwfDecodeCtx,
    data: &[u8],
    with_alpha: bool,
) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let char_id = r.read_u16()?;
    let _bounds = r.read_rect_twips()?;
    let text_matrix = r.read_matrix()?;
    let glyph_bits = r.read_u8()? as usize;
    let advance_bits = r.read_u8()? as usize;

    let mut cur_font_id = 0u16;
    let mut cur_color: Color = [0, 0, 0, 255];
    let mut cur_height = 12.0;
    let mut cur_x = 0.0;
    let mut cur_y = 0.0;
    let mut elements = Vec::new();
    let mut run_idx = 0usize;

    while r.remaining_bytes() > 0 {
        let flags = r.read_u8()?;
        if flags == 0 {
            break;
        }
        if (flags & 0x08) != 0 {
            cur_font_id = r.read_u16()?;
        }
        if (flags & 0x04) != 0 {
            cur_color = if with_alpha {
                r.read_rgba()?
            } else {
                r.read_rgb()?
            };
        }
        if (flags & 0x01) != 0 {
            cur_x = f64::from(r.read_i16()?) / 20.0;
        }
        if (flags & 0x02) != 0 {
            cur_y = f64::from(r.read_i16()?) / 20.0;
        }
        if (flags & 0x08) != 0 {
            let raw_h = f64::from(r.read_u16()?) / 20.0;
            cur_height = if tag_uses_em_divide(&ctx.fonts, cur_font_id, raw_h) {
                (raw_h / 20.0).max(6.0)
            } else {
                raw_h.max(6.0)
            };
        }
        let glyph_count = usize::from(r.read_u8()?);
        let run_start_x = cur_x;
        let mut units = Vec::with_capacity(glyph_count);
        let mut glyph_cmds: Vec<PathCommand> = Vec::new();
        let mut any_glyph_outline = false;
        for _ in 0..glyph_count {
            let g_idx = r.read_ubits(glyph_bits)? as usize;
            let adv = r.read_sbits(advance_bits)?;
            if let Some(font) = ctx.fonts.get(&cur_font_id) {
                if let Some(cmds) = build_glyph_path_commands(font, g_idx, cur_x, cur_y, cur_height)
                {
                    any_glyph_outline = true;
                    glyph_cmds.extend(cmds);
                }
                let code = font.codes.get(g_idx).copied().unwrap_or(b'?' as u16);
                units.push(code);
            } else {
                units.push(b'?' as u16);
            }
            cur_x += f64::from(adv) / 20.0;
        }
        r.align();
        if any_glyph_outline && !glyph_cmds.is_empty() {
            run_idx += 1;
            let [ma, mb, mc, md, mtx, mty] = text_matrix;
            let transformed_cmds: Vec<PathCommand> = glyph_cmds
                .into_iter()
                .map(|cmd| match cmd {
                    PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
                        x: ma * x + mc * y + mtx,
                        y: mb * x + md * y + mty,
                    },
                    PathCommand::LineTo { x, y } => PathCommand::LineTo {
                        x: ma * x + mc * y + mtx,
                        y: mb * x + md * y + mty,
                    },
                    PathCommand::QuadraticTo { cx, cy, x, y } => PathCommand::QuadraticTo {
                        cx: ma * cx + mc * cy + mtx,
                        cy: mb * cx + md * cy + mty,
                        x: ma * x + mc * y + mtx,
                        y: mb * x + md * y + mty,
                    },
                    PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x,
                        y,
                    } => PathCommand::CubicTo {
                        cx1: ma * cx1 + mc * cy1 + mtx,
                        cy1: mb * cx1 + md * cy1 + mty,
                        cx2: ma * cx2 + mc * cy2 + mtx,
                        cy2: mb * cx2 + md * cy2 + mty,
                        x: ma * x + mc * y + mtx,
                        y: mb * x + md * y + mty,
                    },
                    PathCommand::Close => PathCommand::Close,
                })
                .collect();
            elements.push(make_default_element(
                format!("swf-{}-txt{char_id}-{run_idx}", ctx.mount_seq),
                ElementKind::Path {
                    commands: transformed_cmds,
                    fill_rule: FillRule::EvenOdd,
                    fill: cur_color,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                0.0,
                0.0,
            ));
        } else {
            let text = String::from_utf16_lossy(&units);
            if !text.is_empty() {
                run_idx += 1;
                let font_family = ctx
                    .fonts
                    .get(&cur_font_id)
                    .map_or_else(|| "sans-serif".to_owned(), |f| f.name.clone());
                let mut el = make_default_element(
                    format!("swf-{}-txt{char_id}-{run_idx}", ctx.mount_seq),
                    ElementKind::Text {
                        text,
                        font_family,
                        font_size: cur_height,
                        color: cur_color,
                    },
                    run_start_x,
                    (cur_y - cur_height * 0.85).max(0.0),
                );
                if let Some(mut tf) = Transform::from_matrix(text_matrix) {
                    tf.x += el.transform.x;
                    tf.y += el.transform.y;
                    el.transform = tf;
                }
                elements.push(el);
            }
        }
    }

    let sym_id = format!("swf-{}-char-{char_id}", ctx.mount_seq);
    let kf = Keyframe {
        elements,
        ..Default::default()
    };
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("Text_{char_id}"),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: format!("swf-{}-txtlyr-{char_id}", ctx.mount_seq),
                name: "Text".into(),
                visible: true,
                locked: false,
                keyframes: vec![kf],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    ctx.char_symbols.insert(char_id, sym_id);
    Ok(())
}

fn tag_uses_em_divide(_fonts: &HashMap<u16, SwfFontRecord>, _font_id: u16, raw_h: f64) -> bool {
    raw_h > 4096.0
}

fn strip_basic_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

pub(super) fn decode_edit_text_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let char_id = r.read_u16()?;
    let bounds = r.read_rect_twips()?;
    let flags1 = r.read_u8()?;
    let flags2 = r.read_u8()?;
    let has_text = (flags1 & 0x80) != 0;
    let word_wrap = (flags1 & 0x40) != 0;
    let multiline = (flags1 & 0x20) != 0;
    let has_text_color = (flags1 & 0x04) != 0;
    let has_max_length = (flags1 & 0x02) != 0;
    let has_font = (flags1 & 0x01) != 0;
    let has_font_class = (flags2 & 0x80) != 0;
    let is_html = (flags2 & 0x02) != 0;
    let has_layout = (flags2 & 0x20) != 0;

    let mut font_id = 0u16;
    let mut font_height = 12.0;
    if has_font {
        font_id = r.read_u16()?;
    }
    if has_font_class {
        let _ = r.read_c_string()?;
    }
    if has_font {
        font_height = (f64::from(r.read_u16()?) / 20.0).max(6.0);
    }
    let mut color = [0, 0, 0, 255];
    if has_text_color {
        color = r.read_rgba()?;
    }
    if has_max_length {
        let _ = r.read_u16()?;
    }
    if has_layout {
        let _align = r.read_u8()?;
        let _lm = r.read_u16()?;
        let _rm = r.read_u16()?;
        let _ind = r.read_u16()?;
        let _lead = r.read_i16()?;
    }
    let _var_name = r.read_c_string()?;
    let raw_text = if has_text {
        r.read_c_string()?
    } else {
        String::new()
    };
    let clean_text = if is_html {
        strip_basic_html(&raw_text)
    } else {
        raw_text
    };
    let font_family = ctx
        .fonts
        .get(&font_id)
        .map_or_else(|| "sans-serif".to_owned(), |f| f.name.clone());
    let bx = f64::from(bounds[0]) / 20.0;
    let by = f64::from(bounds[2]) / 20.0;
    let bw = (f64::from(bounds[1] - bounds[0]) / 20.0).max(20.0);
    let bh = (f64::from(bounds[3] - bounds[2]) / 20.0).max(font_height * 1.4);

    let fmt = TextFormat {
        font: font_family,
        size: font_height,
        color: (u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2]),
        ..Default::default()
    };

    let tb_kind = ElementKind::TextBox {
        text: clean_text,
        width: bw,
        height: bh,
        format: fmt,
        word_wrap,
        multiline,
    };
    let text_alpha = (f64::from(color[3]) / 255.0).clamp(0.0, 1.0);
    ctx.char_edit_texts
        .insert(char_id, (tb_kind.clone(), bx, by, text_alpha));

    let sym_id = format!("swf-{}-char-{char_id}", ctx.mount_seq);
    let mut kf = Keyframe::default();
    let mut ed_el = make_default_element(
        format!("swf-{}-edtxt-{char_id}", ctx.mount_seq),
        tb_kind,
        bx,
        by,
    );
    ed_el.transform.alpha = text_alpha;
    kf.elements.push(ed_el);
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("EditText_{char_id}"),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: format!("swf-{}-edlyr-{char_id}", ctx.mount_seq),
                name: "EditText".into(),
                visible: true,
                locked: false,
                keyframes: vec![kf],
                properties: LayerProperties::default(),
            }],
            scripts: Vec::new(),
            guides: Vec::new(),
        },
        class_name: None,
        linkage_id: None,
    });
    ctx.char_symbols.insert(char_id, sym_id);
    Ok(())
}

pub(super) fn decode_sprite_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) -> Result<(), String> {
    if data.len() < 4 {
        return Ok(());
    }
    let sprite_id = u16::from_le_bytes([data[0], data[1]]);
    let declared_frames =
        u32::from(u16::from_le_bytes([data[2], data[3]])).clamp(1, MAX_SWF_FRAMES);
    let sub_tags = parse_tag_slice(&data[4..])?;
    let mut f0_active: BTreeMap<u16, (u16, String)> = BTreeMap::new();
    for &(sc, spay) in &sub_tags {
        match sc {
            1 => break,
            4 if spay.len() >= 4 => {
                let cid = u16::from_le_bytes([spay[0], spay[1]]);
                let dep = u16::from_le_bytes([spay[2], spay[3]]);
                f0_active.insert(dep, (cid, String::new()));
            }
            26 | 70 => {
                let mut pr = BitReader::new(spay, 0);
                if let Ok(fl1) = pr.read_u8() {
                    let fl2 = if sc == 70 {
                        pr.read_u8().unwrap_or(0)
                    } else {
                        0
                    };
                    if let Ok(dep) = pr.read_u16() {
                        if (fl2 & 0x08) != 0 || ((fl2 & 0x10) != 0 && (fl1 & 0x02) != 0) {
                            let _ = pr.read_c_string();
                        }
                        let cid_opt = if (fl1 & 0x02) != 0 {
                            pr.read_u16().ok()
                        } else {
                            None
                        };
                        if (fl1 & 0x04) != 0 {
                            let _ = pr.read_matrix();
                        }
                        if (fl1 & 0x08) != 0 {
                            let _ = pr.read_cxform(true);
                        }
                        if (fl1 & 0x10) != 0 {
                            let _ = pr.read_u16();
                        }
                        let nm = if (fl1 & 0x20) != 0 {
                            pr.read_c_string().unwrap_or_default()
                        } else {
                            String::new()
                        };
                        if let Some(cid) = cid_opt {
                            f0_active.insert(dep, (cid, nm));
                        } else if (fl1 & 0x01) != 0
                            && !nm.is_empty()
                            && let Some(ent) = f0_active.get_mut(&dep)
                        {
                            ent.1 = nm;
                        }
                    }
                }
            }
            5 if spay.len() >= 4 => {
                f0_active.remove(&u16::from_le_bytes([spay[2], spay[3]]));
            }
            28 if spay.len() >= 2 => {
                f0_active.remove(&u16::from_le_bytes([spay[0], spay[1]]));
            }
            _ => {}
        }
    }
    ctx.char_sprite_frame0_children
        .insert(sprite_id, f0_active.into_values().collect());
    let label = format!("spr{sprite_id}");
    let timeline = build_timeline_from_tags(ctx, &sub_tags, Some(declared_frames), &label);
    let sym_id = format!("swf-{}-char-{sprite_id}", ctx.mount_seq);
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("Sprite_{sprite_id}"),
        folder: String::new(),
        kind: SymbolKind::MovieClip,
        timeline,
        class_name: None,
        linkage_id: None,
    });
    ctx.char_symbols.insert(sprite_id, sym_id);
    Ok(())
}

pub(super) fn decode_export_assets_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let count = usize::from(r.read_u16()?);
    for _ in 0..count.min(4096) {
        if r.remaining_bytes() < 3 {
            break;
        }
        let cid = r.read_u16()?;
        let name = r.read_c_string()?;
        if !name.is_empty() {
            ctx.export_assets.insert(cid, name);
        }
    }
    Ok(())
}

pub(super) fn decode_symbol_class_tag(ctx: &mut SwfDecodeCtx, data: &[u8]) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let count = usize::from(r.read_u16()?);
    for _ in 0..count.min(4096) {
        if r.remaining_bytes() < 3 {
            break;
        }
        let cid = r.read_u16()?;
        let name = r.read_c_string()?;
        if !name.is_empty() {
            ctx.symbol_classes.insert(cid, name);
        }
    }
    Ok(())
}

fn placement_to_element(
    ctx: &mut SwfDecodeCtx,
    scope_prefix: &str,
    depth: u16,
    p: &ActivePlacement,
) -> Option<Element> {
    let mut tf = Transform::from_matrix(p.matrix).unwrap_or_default();
    tf.alpha = (p.cxform.multipliers[3] + p.cxform.offsets[3] / 255.0).clamp(0.0, 1.0);
    tf.color_transform = p.cxform;
    let kind = if let Some((tb_kind, bx, by, text_alpha)) = ctx.char_edit_texts.get(&p.char_id) {
        tf.x += *bx;
        tf.y += *by;
        tf.alpha =
            (p.cxform.multipliers[3] * *text_alpha + p.cxform.offsets[3] / 255.0).clamp(0.0, 1.0);
        tb_kind.clone()
    } else if ctx.char_morphs.contains_key(&p.char_id) {
        let sym_id = ensure_morph_ratio_symbol(ctx, p.char_id, p.ratio);
        ElementKind::Instance {
            symbol_id: sym_id,
            playback: InstancePlayback::Independent,
        }
    } else {
        let sym_id = ctx.char_symbols.get(&p.char_id)?.clone();
        ElementKind::Instance {
            symbol_id: sym_id,
            playback: InstancePlayback::Independent,
        }
    };
    let mut el = make_default_element(
        format!(
            "swf-{}-{scope_prefix}-d{depth}-c{}",
            ctx.mount_seq, p.char_id
        ),
        kind,
        tf.x,
        tf.y,
    );
    el.name = p.name.clone();
    el.transform = tf;
    el.visible = p.visible;
    el.cache_as_bitmap = p.cache_as_bitmap;
    el.blend_mode = p.blend_mode;
    el.filters.clone_from(&p.filters);
    let mut combined_obj_js = String::new();
    if let Some(btn_js) = ctx.char_button_scripts.get(&p.char_id) {
        combined_obj_js.push_str(btn_js);
    }
    if let Some(clip_js) = &p.clip_script_js {
        combined_obj_js.push_str(clip_js);
    }
    if !combined_obj_js.is_empty() {
        attach_element_object_script(
            ctx,
            &mut el,
            &combined_obj_js,
            &format!("{scope_prefix}-d{depth}"),
        );
    }
    Some(el)
}

pub(super) fn build_timeline_from_tags(
    ctx: &mut SwfDecodeCtx,
    tags: &[(u16, &[u8])],
    declared_frames: Option<u32>,
    scope_prefix: &str,
) -> Timeline {
    let mut active: BTreeMap<u16, ActivePlacement> = BTreeMap::new();
    let mut dirty_depths: BTreeSet<u16> = BTreeSet::new();
    let mut depth_tracks: BTreeMap<u16, Vec<(u32, Option<ActivePlacement>)>> = BTreeMap::new();
    let mut max_clip_by_depth: BTreeMap<u16, u16> = BTreeMap::new();
    let mut frame_labels: BTreeMap<u32, Vec<(String, FrameLabelType)>> = BTreeMap::new();
    let mut frame_actions: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut pre_place_action_frames: BTreeSet<u32> = BTreeSet::new();
    let mut frame_saw_place = false;
    let mut frame_actions_before_place = false;
    let root_init_js = if scope_prefix == "root" && !ctx.init_actions.is_empty() {
        Some(std::mem::take(&mut ctx.init_actions).join("\n"))
    } else {
        None
    };
    let mut shown_frames = 0u32;

    let flush_dirty = |ctx: &mut SwfDecodeCtx,
                       frame_idx: u32,
                       active: &BTreeMap<u16, ActivePlacement>,
                       dirty_depths: &mut BTreeSet<u16>,
                       depth_tracks: &mut BTreeMap<u16, Vec<(u32, Option<ActivePlacement>)>>,
                       max_clip_by_depth: &mut BTreeMap<u16, u16>| {
        for depth in std::mem::take(dirty_depths) {
            if ctx.total_keyframes >= MAX_SWF_KEYFRAMES {
                ctx.warn(
                    "SWF keyframe budget (65536) reached; truncating excess timeline keyframes.",
                );
                break;
            }
            let cur = active.get(&depth).cloned();
            if let Some(p) = &cur
                && p.clip_depth > depth
            {
                max_clip_by_depth
                    .entry(depth)
                    .and_modify(|m| *m = (*m).max(p.clip_depth))
                    .or_insert(p.clip_depth);
            }
            let track = depth_tracks.entry(depth).or_default();
            if track.last().is_some_and(|(_, prev)| prev == &cur) {
                continue;
            }
            track.push((frame_idx, cur));
            ctx.total_keyframes += 1;
        }
    };

    for &(code, payload) in tags {
        match code {
            4 => {
                if let Ok(Some(depth)) = apply_place_object1(ctx, &mut active, payload) {
                    dirty_depths.insert(depth);
                    frame_saw_place = true;
                }
            }
            26 | 70 => {
                if let Ok(Some(depth)) =
                    apply_place_object2_or_3(ctx, &mut active, payload, code == 70)
                {
                    dirty_depths.insert(depth);
                    frame_saw_place = true;
                }
            }
            5 if payload.len() >= 4 => {
                let depth = u16::from_le_bytes([payload[2], payload[3]]);
                if active.remove(&depth).is_some() {
                    dirty_depths.insert(depth);
                }
            }
            28 if payload.len() >= 2 => {
                let depth = u16::from_le_bytes([payload[0], payload[1]]);
                if active.remove(&depth).is_some() {
                    dirty_depths.insert(depth);
                }
            }
            43 => {
                let mut r = BitReader::new(payload, 0);
                if let Ok(lbl) = r.read_c_string()
                    && !lbl.is_empty()
                {
                    let kind = if r.read_u8().ok() == Some(1) {
                        FrameLabelType::Anchor
                    } else {
                        FrameLabelType::Name
                    };
                    frame_labels
                        .entry(shown_frames)
                        .or_default()
                        .push((lbl, kind));
                }
            }
            12 => {
                ctx.saw_do_action = true;
                if let Some(js) = compile_avm1_actions(
                    ctx,
                    payload,
                    &format!("{scope_prefix} frame {} DoAction", shown_frames + 1),
                ) {
                    if !frame_saw_place {
                        frame_actions_before_place = true;
                    }
                    frame_actions.entry(shown_frames).or_default().push(js);
                }
            }
            1 if shown_frames < MAX_SWF_FRAMES => {
                if frame_actions_before_place && frame_saw_place {
                    pre_place_action_frames.insert(shown_frames);
                }
                frame_saw_place = false;
                frame_actions_before_place = false;
                flush_dirty(
                    ctx,
                    shown_frames,
                    &active,
                    &mut dirty_depths,
                    &mut depth_tracks,
                    &mut max_clip_by_depth,
                );
                shown_frames += 1;
            }
            _ => {}
        }
    }

    if shown_frames == 0 {
        flush_dirty(
            ctx,
            0,
            &active,
            &mut dirty_depths,
            &mut depth_tracks,
            &mut max_clip_by_depth,
        );
        shown_frames = 1;
    }

    let total_frames = declared_frames
        .unwrap_or(shown_frames)
        .max(shown_frames)
        .clamp(1, MAX_SWF_FRAMES);

    let mut layers = Vec::with_capacity(depth_tracks.len());
    let mut clip_ranges: Vec<(u16, u16, String)> = Vec::new();

    for (depth, changes) in depth_tracks {
        let layer_id = format!("swf-{}-{scope_prefix}-d{depth}", ctx.mount_seq);
        let max_clip_depth = max_clip_by_depth.get(&depth).copied().unwrap_or(0);
        if max_clip_depth > depth {
            clip_ranges.push((depth, max_clip_depth, layer_id.clone()));
        }

        let mut keyframes: Vec<Keyframe> = Vec::with_capacity(changes.len() + 1);
        if changes.first().is_none_or(|(f_idx, _)| *f_idx > 0) {
            keyframes.push(Keyframe {
                frame: 0,
                ..Default::default()
            });
        }
        for (f_idx, placement_opt) in changes {
            let element = placement_opt
                .as_ref()
                .and_then(|p| placement_to_element(ctx, scope_prefix, depth, p));
            let same_as_prev = keyframes.last().is_some_and(|prev_kf| {
                match (prev_kf.elements.first(), element.as_ref()) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                }
            });
            if keyframes.is_empty() || !same_as_prev {
                let mut kf = Keyframe {
                    frame: f_idx,
                    ..Default::default()
                };
                if let Some(el) = element {
                    kf.elements.push(el);
                }
                keyframes.push(kf);
            }
        }

        let mut props = LayerProperties::default();
        if max_clip_depth > depth {
            props.role = LayerRole::Mask;
        }
        layers.push((
            depth,
            Layer {
                id: layer_id,
                name: format!("Depth {depth}"),
                visible: true,
                locked: false,
                keyframes,
                properties: props,
            },
        ));
    }

    for (clip_start, clip_end, mask_layer_id) in clip_ranges {
        for (d, layer) in &mut layers {
            if *d > clip_start && *d <= clip_end && layer.properties.role == LayerRole::Normal {
                layer.properties.mask = Some(mask_layer_id.clone());
            }
        }
    }

    let mut final_layers: Vec<Layer> = layers.into_iter().map(|(_, l)| l).collect();
    let (label_layers, omitted) = neoflash_core::swf_label_layers(
        &format!("swf-{}-{scope_prefix}-labels", ctx.mount_seq),
        frame_labels
            .range(..total_frames)
            .map(|(&frame, names)| (frame, names.as_slice())),
        MAX_SWF_KEYFRAMES.saturating_sub(ctx.total_keyframes),
    );
    ctx.total_keyframes += label_layers
        .iter()
        .map(|layer| layer.keyframes.len())
        .sum::<usize>();
    if omitted > 0 {
        ctx.warn(format!(
            "{scope_prefix}: SWF keyframe budget reached; omitted {omitted} frame label(s)."
        ));
    }
    final_layers.extend(label_layers);
    if final_layers.is_empty() {
        final_layers.push(Layer {
            id: format!("swf-{}-{scope_prefix}-empty", ctx.mount_seq),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe::default()],
            properties: LayerProperties::default(),
        });
    }

    let mut timeline = Timeline {
        frame_count: total_frames,
        layers: final_layers,
        scripts: Vec::new(),
        guides: Vec::new(),
    };

    if scope_prefix == "root" && ctx.action_script_version < 3 && ctx.saw_do_action {
        let mut linkage_setup = String::new();
        for (idx, sym) in ctx.symbols.iter().enumerate() {
            if let Some(lid) = &sym.linkage_id {
                let lid_lit = super::vm::js_string_literal(lid);
                linkage_setup.push_str(&format!(
                    "  __avm1.symLinkage['s{}'] = {lid_lit};\n  __avm1.exportLinkages.add({lid_lit});\n",
                    idx + 1
                ));
            }
        }
        for frame in 0..total_frames {
            let pre_place = pre_place_action_frames.contains(&frame);
            let mut combined = format!(
                "(() => {{\n{}if (__avm1) {{\n",
                super::vm::AVM1_RUNTIME_INIT_JS
            );
            if frame == 0 && !linkage_setup.is_empty() {
                combined.push_str(&linkage_setup);
            }
            combined.push_str("}\n");
            if frame == 0
                && let Some(init) = &root_init_js
            {
                combined.push_str(&format!(
                    "if (__nfSwfEnv && !__nfSwfEnv.avm1Initialized) {{\n  __nfSwfEnv.avm1Initialized = true;\n{init}\n}}\n"
                ));
            }
            combined.push_str(
                "if (__avm1) {\n  const __t = ++__avm1.tick;\n  __avm1.flushUnloads();\n  __avm1.runEnterFrames(__t);\n  __avm1.flushInitConstruct(__t, this);\n",
            );
            if !pre_place {
                combined.push_str("  __avm1.flushLoadsFor(null);\n");
            }
            combined.push_str("}\n})();\n");
            if let Some(blocks) = frame_actions.remove(&frame) {
                combined.push_str(&blocks.join("\n"));
                combined.push('\n');
            }
            combined.push_str(&format!(
                "(() => {{\n{}if (__avm1) __avm1.sealTimelineClips(this);\n}})();\n",
                super::vm::AVM1_RUNTIME_INIT_JS
            ));
            attach_compiled_frame_script(
                ctx,
                &mut timeline,
                frame,
                &combined,
                scope_prefix,
                neoflash_core::ScriptLanguage::ActionScript2,
            );
        }
    } else {
        if scope_prefix != "root" && ctx.action_script_version < 3 {
            let flush_self = format!(
                "(() => {{\n{}if (__avm1) {{ __avm1.constructRegisteredClip(this, null); __avm1.flushLoadsFor(this); }}\n}})();\n",
                super::vm::AVM1_RUNTIME_INIT_JS
            );
            frame_actions.entry(0).or_default().insert(0, flush_self);
        }
        for (frame, blocks) in frame_actions {
            if frame < total_frames {
                let joined = blocks.join("\n");
                attach_compiled_frame_script(
                    ctx,
                    &mut timeline,
                    frame,
                    &joined,
                    scope_prefix,
                    neoflash_core::ScriptLanguage::ActionScript2,
                );
            }
        }
    }

    timeline
}

fn apply_place_object1(
    ctx: &mut SwfDecodeCtx,
    active: &mut BTreeMap<u16, ActivePlacement>,
    data: &[u8],
) -> Result<Option<u16>, String> {
    if ctx.total_placements >= MAX_SWF_PLACEMENTS {
        ctx.warn("SWF placement budget (65536) reached; truncating excess timeline placements.");
        return Ok(None);
    }
    let mut r = BitReader::new(data, 0);
    let char_id = r.read_u16()?;
    let depth = r.read_u16()?;
    let matrix = r.read_matrix()?;
    let cxform = if r.remaining_bytes() > 0 {
        r.read_cxform(false).unwrap_or_default()
    } else {
        ColorTransform::default()
    };
    ctx.total_placements += 1;
    active.insert(
        depth,
        ActivePlacement {
            char_id,
            matrix,
            cxform,
            name: String::new(),
            clip_depth: 0,
            visible: true,
            cache_as_bitmap: false,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            ratio: 0,
            clip_script_js: None,
        },
    );
    Ok(Some(depth))
}

fn apply_place_object2_or_3(
    ctx: &mut SwfDecodeCtx,
    active: &mut BTreeMap<u16, ActivePlacement>,
    data: &[u8],
    is_v3: bool,
) -> Result<Option<u16>, String> {
    if ctx.total_placements >= MAX_SWF_PLACEMENTS {
        ctx.warn("SWF placement budget (65536) reached; truncating excess timeline placements.");
        return Ok(None);
    }
    let mut r = BitReader::new(data, 0);
    let flags1 = r.read_u8()?;
    let flags2 = if is_v3 { r.read_u8()? } else { 0 };
    let depth = r.read_u16()?;

    let has_move = (flags1 & 0x01) != 0;
    let has_character = (flags1 & 0x02) != 0;
    let has_matrix = (flags1 & 0x04) != 0;
    let has_cxform = (flags1 & 0x08) != 0;
    let has_ratio = (flags1 & 0x10) != 0;
    let has_name = (flags1 & 0x20) != 0;
    let has_clip_depth = (flags1 & 0x40) != 0;
    let has_clip_actions = (flags1 & 0x80) != 0;

    let has_class_name = (flags2 & 0x08) != 0;
    let has_image = (flags2 & 0x10) != 0;
    let has_filter_list = (flags2 & 0x01) != 0;
    let has_blend_mode = (flags2 & 0x02) != 0;
    let has_cache_bitmap = (flags2 & 0x04) != 0;
    let has_visible = (flags2 & 0x20) != 0;
    let has_bg_color = (flags2 & 0x40) != 0;

    if has_class_name || (has_image && has_character) {
        let _ = r.read_c_string()?;
    }
    let char_id = if has_character {
        Some(r.read_u16()?)
    } else {
        None
    };
    let matrix = if has_matrix {
        Some(r.read_matrix()?)
    } else {
        None
    };
    let cxform = if has_cxform {
        Some(r.read_cxform(true)?)
    } else {
        None
    };
    let ratio = if has_ratio { Some(r.read_u16()?) } else { None };
    let name = if has_name {
        Some(r.read_c_string()?)
    } else {
        None
    };
    let clip_depth = if has_clip_depth {
        Some(r.read_u16()?)
    } else {
        None
    };
    let filters = if has_filter_list {
        Some(decode_swf_filter_list(ctx, &mut r)?)
    } else {
        None
    };
    let blend_mode = if has_blend_mode {
        let mode_byte = r.read_u8()?;
        Some(decode_swf_blend_mode(ctx, mode_byte))
    } else {
        None
    };
    let cache_as_bitmap = if has_cache_bitmap {
        Some(r.read_u8()? != 0)
    } else {
        None
    };
    let visible = if has_visible {
        Some(r.read_u8()? != 0)
    } else {
        None
    };
    if has_bg_color {
        let _ = r.read_rgba()?;
    }
    let clip_script_js = if has_clip_actions {
        parse_clip_actions_to_js(ctx, &mut r, depth).ok().flatten()
    } else {
        None
    };

    if has_move && let Some(existing) = active.get_mut(&depth) {
        ctx.total_placements += 1;
        if let Some(cid) = char_id {
            existing.char_id = cid;
        }
        if let Some(m) = matrix {
            existing.matrix = m;
        }
        if let Some(cx) = cxform {
            existing.cxform = cx;
        }
        if let Some(rat) = ratio {
            existing.ratio = rat;
        }
        if let Some(n) = name {
            existing.name = n;
        }
        if let Some(cd) = clip_depth {
            existing.clip_depth = cd;
        }
        if let Some(v) = visible {
            existing.visible = v;
        }
        if let Some(cab) = cache_as_bitmap {
            existing.cache_as_bitmap = cab;
        }
        if let Some(bm) = blend_mode {
            existing.blend_mode = bm;
        }
        if let Some(fl) = filters {
            existing.filters = fl;
        }
        if clip_script_js.is_some() {
            existing.clip_script_js = clip_script_js;
        }
        return Ok(Some(depth));
    } else if let Some(cid) = char_id {
        ctx.total_placements += 1;
        active.insert(
            depth,
            ActivePlacement {
                char_id: cid,
                matrix: matrix.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
                cxform: cxform.unwrap_or_default(),
                name: name.unwrap_or_default(),
                clip_depth: clip_depth.unwrap_or(0),
                visible: visible.unwrap_or(true),
                cache_as_bitmap: cache_as_bitmap.unwrap_or(false),
                blend_mode: blend_mode.unwrap_or(BlendMode::Normal),
                filters: filters.unwrap_or_default(),
                ratio: ratio.unwrap_or(0),
                clip_script_js,
            },
        );
        return Ok(Some(depth));
    }
    Ok(None)
}

fn rgba_to_rgb_alpha(rgba: Color) -> (u32, f64) {
    let rgb = (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
    let alpha = (f64::from(rgba[3]) / 255.0).clamp(0.0, 1.0);
    (rgb, alpha)
}

fn sanitize_finite(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { 0.0 }
}

pub(super) fn decode_swf_blend_mode(ctx: &mut SwfDecodeCtx, mode_byte: u8) -> BlendMode {
    match mode_byte {
        0 | 1 => BlendMode::Normal,
        2 => BlendMode::Layer,
        3 => BlendMode::Multiply,
        4 => BlendMode::Screen,
        5 => BlendMode::Lighten,
        6 => BlendMode::Darken,
        7 => BlendMode::Difference,
        8 => BlendMode::Add,
        9 => BlendMode::Subtract,
        10 => BlendMode::Invert,
        11 => BlendMode::Alpha,
        12 => BlendMode::Erase,
        13 => BlendMode::Overlay,
        14 => BlendMode::Hardlight,
        other => {
            ctx.warn(format!(
                "SWF PlaceObject3 blendMode {other} is not supported in the bounded SWF loader; defaulted to Normal."
            ));
            BlendMode::Normal
        }
    }
}

pub(super) fn decode_swf_filter_list(
    ctx: &mut SwfDecodeCtx,
    r: &mut BitReader<'_>,
) -> Result<Vec<FilterSpec>, String> {
    let count = usize::from(r.read_u8()?);
    let mut out = Vec::with_capacity(count.min(MAX_OBJECT_FILTERS));
    let mut warned_cap = false;

    for _ in 0..count {
        let filter_id = r.read_u8()?;
        let spec_opt = match filter_id {
            0 => {
                // DropShadowFilter (23 bytes)
                let (color, alpha) = rgba_to_rgb_alpha(r.read_rgba()?);
                let blur_x = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let blur_y = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let angle = sanitize_finite(r.read_fixed16()?.to_degrees(), -1.0e9, 1.0e9);
                let distance = sanitize_finite(r.read_fixed16()?, -8192.0, 8192.0);
                let strength = sanitize_finite(r.read_fixed8()?, 0.0, 255.0);
                let flags = r.read_u8()?;
                let inner = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let hide_object = (flags & 0x20) == 0;
                let quality = (flags & 0x1F).clamp(1, 15);
                Some(FilterSpec::DropShadow {
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
                })
            }
            1 => {
                // BlurFilter (9 bytes)
                let blur_x = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let blur_y = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let flags = r.read_u8()?;
                let quality = ((flags >> 3) & 0x1F).clamp(1, 15);
                Some(FilterSpec::Blur {
                    blur_x,
                    blur_y,
                    quality,
                })
            }
            2 => {
                // GlowFilter (15 bytes)
                let (color, alpha) = rgba_to_rgb_alpha(r.read_rgba()?);
                let blur_x = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let blur_y = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let strength = sanitize_finite(r.read_fixed8()?, 0.0, 255.0);
                let flags = r.read_u8()?;
                let inner = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let quality = (flags & 0x1F).clamp(1, 15);
                Some(FilterSpec::Glow {
                    color,
                    alpha,
                    blur_x,
                    blur_y,
                    strength,
                    quality,
                    inner,
                    knockout,
                })
            }
            3 => {
                // BevelFilter (27 bytes: ShadowColor first, HighlightColor second)
                let (shadow_color, shadow_alpha) = rgba_to_rgb_alpha(r.read_rgba()?);
                let (highlight_color, highlight_alpha) = rgba_to_rgb_alpha(r.read_rgba()?);
                let blur_x = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let blur_y = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let angle = sanitize_finite(r.read_fixed16()?.to_degrees(), -1.0e9, 1.0e9);
                let distance = sanitize_finite(r.read_fixed16()?, -8192.0, 8192.0);
                let strength = sanitize_finite(r.read_fixed8()?, 0.0, 255.0);
                let flags = r.read_u8()?;
                let inner_shadow = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let on_top = (flags & 0x10) != 0;
                let placement = if on_top {
                    "full".to_owned()
                } else if inner_shadow {
                    "inner".to_owned()
                } else {
                    "outer".to_owned()
                };
                let quality = (flags & 0x0F).clamp(1, 15);
                Some(FilterSpec::Bevel {
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
                })
            }
            4 | 7 => {
                // GradientGlowFilter (4) / GradientBevelFilter (7)
                let num_colors = usize::from(r.read_u8()?);
                let mut raw_rgba = Vec::with_capacity(num_colors.min(256));
                for _ in 0..num_colors {
                    raw_rgba.push(rgba_to_rgb_alpha(r.read_rgba()?));
                }
                let mut raw_ratios = Vec::with_capacity(num_colors.min(256));
                for _ in 0..num_colors {
                    raw_ratios.push(r.read_u8()?);
                }
                let blur_x = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let blur_y = sanitize_finite(r.read_fixed16()?, 0.0, 255.0);
                let angle = sanitize_finite(r.read_fixed16()?.to_degrees(), -1.0e9, 1.0e9);
                let distance = sanitize_finite(r.read_fixed16()?, -8192.0, 8192.0);
                let strength = sanitize_finite(r.read_fixed8()?, 0.0, 255.0);
                let flags = r.read_u8()?;
                let inner_shadow = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let on_top = (flags & 0x10) != 0;
                let placement = if on_top {
                    "full".to_owned()
                } else if inner_shadow {
                    "inner".to_owned()
                } else {
                    "outer".to_owned()
                };
                let quality = (flags & 0x0F).clamp(1, 15);

                let take_n = num_colors.min(16);
                let mut stops: Vec<(u8, u32, f64)> = (0..take_n)
                    .map(|i| (raw_ratios[i], raw_rgba[i].0, raw_rgba[i].1))
                    .collect();
                stops.sort_by_key(|(rat, _, _)| *rat);
                let ratios: Vec<u8> = stops.iter().map(|(rat, _, _)| *rat).collect();
                let colors: Vec<u32> = stops.iter().map(|(_, c, _)| *c).collect();
                let alphas: Vec<f64> = stops.iter().map(|(_, _, a)| *a).collect();
                if filter_id == 4 {
                    Some(FilterSpec::GradientGlow {
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
                    })
                } else {
                    Some(FilterSpec::GradientBevel {
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
                    })
                }
            }
            5 => {
                // ConvolutionFilter
                let mx = r.read_u8()?;
                let my = r.read_u8()?;
                let divisor = sanitize_finite(r.read_f32()?, -65536.0, 65536.0);
                let bias = sanitize_finite(r.read_f32()?, -65536.0, 65536.0);
                let total = usize::from(mx).saturating_mul(usize::from(my));
                let mut matrix = Vec::with_capacity(total.min(225));
                for i in 0..total {
                    let coeff = sanitize_finite(r.read_f32()?, -65536.0, 65536.0);
                    if i < 225 {
                        matrix.push(coeff);
                    }
                }
                let (color, alpha) = rgba_to_rgb_alpha(r.read_rgba()?);
                let flags = r.read_u8()?;
                let clamp = (flags & 0x02) != 0;
                let preserve_alpha = (flags & 0x01) != 0;
                if mx <= 15 && my <= 15 && total <= 225 {
                    Some(FilterSpec::Convolution {
                        matrix_x: mx,
                        matrix_y: my,
                        matrix,
                        divisor,
                        bias,
                        preserve_alpha,
                        clamp,
                        color,
                        alpha,
                    })
                } else {
                    ctx.warn(
                        "SWF PlaceObject3 ConvolutionFilter exceeds 15x15 matrix limit and was skipped.",
                    );
                    None
                }
            }
            6 => {
                // ColorMatrixFilter (80 bytes = 20 f32s)
                let mut matrix = [0.0f64; 20];
                for slot in &mut matrix {
                    *slot = sanitize_finite(r.read_f32()?, -65536.0, 65536.0);
                }
                Some(FilterSpec::ColorMatrix { matrix })
            }
            other => {
                ctx.warn(format!(
                    "SWF PlaceObject3 filter ID {other} is not supported in the bounded SWF loader and was skipped."
                ));
                return Ok(out);
            }
        };

        if let Some(spec) = spec_opt
            && spec.validate().is_ok()
        {
            if out.len() < MAX_OBJECT_FILTERS {
                out.push(spec);
            } else if !warned_cap {
                warned_cap = true;
                ctx.warn(
                    "SWF PlaceObject3 filter list exceeds the 8-filter display limit; excess filters were clamped.",
                );
            }
        }
    }
    Ok(out)
}
