//! Bounded pure-Rust SWF (`FWS`/`CWS`/`ZWS`) decoder and timeline/AVM1/AVM2 bundle builder
//! for `Loader.load` / `Loader.loadBytes` in HTML5/WASM.

use neoflash_core::{
    BlendMode, Color, ColorTransform, Element, ElementKind, FillRule, FilterSpec, FrameScript,
    GradientKind, GradientPaint, GradientSpread, GradientStop, Keyframe, Layer, LayerProperties,
    PathCommand, Project, ResourceUnit, Script, ScriptLanguage, Symbol, SymbolKind, Timeline,
    Transform,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use wasm_bindgen::prelude::*;

#[cfg(test)]
mod tests;
mod timeline;
mod vm;

use self::timeline::{
    build_timeline_from_tags, decode_edit_text_tag, decode_export_assets_tag, decode_font_name_tag,
    decode_font_tag, decode_sprite_tag, decode_static_text_tag, decode_symbol_class_tag,
};
use self::vm::{AbcFile, apply_abc_scripts, compile_avm1_actions, parse_abc_tag};

const MAX_SWF_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_BITMAP_RAW_BYTES: usize = 16 * 1024 * 1024;
const MAX_SWF_FRAMES: u32 = 4096;
const MAX_SWF_TAGS: usize = 32768;
pub(super) const MAX_SWF_KEYFRAMES: usize = 65_536;
pub(super) const MAX_SWF_PLACEMENTS: usize = 65_536;

static SWF_MOUNT_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedSwfInfo {
    pub swf_version: u32,
    pub action_script_version: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub frame_count: u32,
    pub background_color: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledSwfScriptUnit {
    pub language: ScriptLanguage,
    pub source: String,
    pub code: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedSwfBundle {
    pub swf_info: DecodedSwfInfo,
    pub project: Project,
    pub scripts: Vec<CompiledSwfScriptUnit>,
    pub warnings: Vec<String>,
}

#[wasm_bindgen]
pub fn decode_swf_bundle_json(bytes: &[u8]) -> Result<String, JsValue> {
    let bundle = decode_swf_bundle(bytes).map_err(|e| JsValue::from_str(&e))?;
    serde_json::to_string(&bundle).map_err(|e| JsValue::from_str(&e.to_string()))
}

struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], byte_offset: usize) -> Self {
        Self {
            data,
            bit_pos: byte_offset * 8,
        }
    }

    fn align(&mut self) {
        let rem = self.bit_pos & 7;
        if rem != 0 {
            self.bit_pos += 8 - rem;
        }
    }

    fn byte_pos(&mut self) -> usize {
        self.align();
        self.bit_pos / 8
    }

    fn remaining_bytes(&self) -> usize {
        self.data.len().saturating_sub(self.bit_pos.div_ceil(8))
    }

    fn read_ubits(&mut self, count: usize) -> Result<u32, String> {
        if count == 0 {
            return Ok(0);
        }
        if count > 32 || self.bit_pos + count > self.data.len() * 8 {
            return Err("Truncated SWF bit stream".into());
        }
        let mut val = 0u32;
        for _ in 0..count {
            let b = self.data[self.bit_pos >> 3];
            let bit = (b >> (7 - (self.bit_pos & 7))) & 1;
            val = (val << 1) | u32::from(bit);
            self.bit_pos += 1;
        }
        Ok(val)
    }

    fn read_sbits(&mut self, count: usize) -> Result<i32, String> {
        if count == 0 {
            return Ok(0);
        }
        let raw = self.read_ubits(count)?;
        let shift = 32 - count;
        Ok(((raw << shift) as i32) >> shift)
    }

    fn read_fbits(&mut self, count: usize) -> Result<f64, String> {
        Ok(f64::from(self.read_sbits(count)?) / 65536.0)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let pos = self.byte_pos();
        let b = *self.data.get(pos).ok_or("Truncated SWF byte")?;
        self.bit_pos += 8;
        Ok(b)
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let lo = u16::from(self.read_u8()?);
        let hi = u16::from(self.read_u8()?);
        Ok(lo | (hi << 8))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        Ok(self.read_u16()? as i16)
    }

    fn read_fixed8(&mut self) -> Result<f64, String> {
        Ok(f64::from(self.read_i16()?) / 256.0)
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let lo = u32::from(self.read_u16()?);
        let hi = u32::from(self.read_u16()?);
        Ok(lo | (hi << 16))
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        Ok(self.read_u32()? as i32)
    }

    fn read_fixed16(&mut self) -> Result<f64, String> {
        Ok(f64::from(self.read_i32()?) / 65536.0)
    }

    fn read_f32(&mut self) -> Result<f64, String> {
        let v = f32::from_bits(self.read_u32()?);
        let d = f64::from(v);
        Ok(if d.is_finite() { d } else { 0.0 })
    }

    fn read_c_string(&mut self) -> Result<String, String> {
        let start = self.byte_pos();
        let mut end = start;
        while end < self.data.len() && self.data[end] != 0 {
            end += 1;
        }
        if end >= self.data.len() {
            self.bit_pos = self.data.len() * 8;
            return Ok(String::from_utf8_lossy(&self.data[start..]).into_owned());
        }
        let text = String::from_utf8_lossy(&self.data[start..end]).into_owned();
        self.bit_pos = (end + 1) * 8;
        Ok(text)
    }

    fn read_rect_twips(&mut self) -> Result<[i32; 4], String> {
        self.align();
        let nbits = self.read_ubits(5)? as usize;
        let xmin = self.read_sbits(nbits)?;
        let xmax = self.read_sbits(nbits)?;
        let ymin = self.read_sbits(nbits)?;
        let ymax = self.read_sbits(nbits)?;
        self.align();
        Ok([xmin, xmax, ymin, ymax])
    }

    fn read_rgb(&mut self) -> Result<Color, String> {
        Ok([self.read_u8()?, self.read_u8()?, self.read_u8()?, 255])
    }

    fn read_rgba(&mut self) -> Result<Color, String> {
        Ok([
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
            self.read_u8()?,
        ])
    }

    fn read_matrix(&mut self) -> Result<[f64; 6], String> {
        self.align();
        let mut a = 1.0;
        let mut d = 1.0;
        if self.read_ubits(1)? == 1 {
            let bits = self.read_ubits(5)? as usize;
            a = self.read_fbits(bits)?;
            d = self.read_fbits(bits)?;
        }
        let mut b = 0.0;
        let mut c = 0.0;
        if self.read_ubits(1)? == 1 {
            let bits = self.read_ubits(5)? as usize;
            b = self.read_fbits(bits)?;
            c = self.read_fbits(bits)?;
        }
        let tbits = self.read_ubits(5)? as usize;
        let tx = f64::from(self.read_sbits(tbits)?) / 20.0;
        let ty = f64::from(self.read_sbits(tbits)?) / 20.0;
        self.align();
        Ok([a, b, c, d, tx, ty])
    }

    fn read_cxform(&mut self, with_alpha: bool) -> Result<ColorTransform, String> {
        self.align();
        let has_add = self.read_ubits(1)? == 1;
        let has_mult = self.read_ubits(1)? == 1;
        let nbits = self.read_ubits(4)? as usize;
        let mut mult = [1.0, 1.0, 1.0, 1.0];
        let mut off = [0.0, 0.0, 0.0, 0.0];
        if has_mult {
            mult[0] = f64::from(self.read_sbits(nbits)?) / 256.0;
            mult[1] = f64::from(self.read_sbits(nbits)?) / 256.0;
            mult[2] = f64::from(self.read_sbits(nbits)?) / 256.0;
            if with_alpha {
                mult[3] = f64::from(self.read_sbits(nbits)?) / 256.0;
            }
        }
        if has_add {
            off[0] = f64::from(self.read_sbits(nbits)?).clamp(-255.0, 255.0);
            off[1] = f64::from(self.read_sbits(nbits)?).clamp(-255.0, 255.0);
            off[2] = f64::from(self.read_sbits(nbits)?).clamp(-255.0, 255.0);
            if with_alpha {
                off[3] = f64::from(self.read_sbits(nbits)?).clamp(-255.0, 255.0);
            }
        }
        self.align();
        Ok(ColorTransform {
            multipliers: mult,
            offsets: off,
        })
    }
}

#[derive(Clone)]
enum SwfFillStyle {
    Solid(Color),
    Gradient(GradientPaint, Color),
    Bitmap { char_id: u16 },
}

#[derive(Clone)]
struct SwfLineStyle {
    width: f64,
    color: Color,
    gradient: Option<GradientPaint>,
}

#[derive(Clone, Copy)]
enum DirectedSeg {
    Line {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
    },
    Quad {
        x0: i32,
        y0: i32,
        cx: i32,
        cy: i32,
        x1: i32,
        y1: i32,
    },
}

impl DirectedSeg {
    fn start(self) -> (i32, i32) {
        match self {
            Self::Line { x0, y0, .. } | Self::Quad { x0, y0, .. } => (x0, y0),
        }
    }

    fn end(self) -> (i32, i32) {
        match self {
            Self::Line { x1, y1, .. } | Self::Quad { x1, y1, .. } => (x1, y1),
        }
    }

    fn reversed(self) -> Self {
        match self {
            Self::Line { x0, y0, x1, y1 } => Self::Line {
                x0: x1,
                y0: y1,
                x1: x0,
                y1: y0,
            },
            Self::Quad {
                x0,
                y0,
                cx,
                cy,
                x1,
                y1,
            } => Self::Quad {
                x0: x1,
                y0: y1,
                cx,
                cy,
                x1: x0,
                y1: y0,
            },
        }
    }
}

#[derive(Clone)]
struct SwfFontRecord {
    name: String,
    codes: Vec<u16>,
}

#[derive(Clone)]
struct ActivePlacement {
    char_id: u16,
    matrix: [f64; 6],
    cxform: ColorTransform,
    name: String,
    clip_depth: u16,
    visible: bool,
    cache_as_bitmap: bool,
    blend_mode: BlendMode,
    filters: Vec<FilterSpec>,
}

pub fn decode_swf_bundle(bytes: &[u8]) -> Result<DecodedSwfBundle, String> {
    if bytes.len() < 12 {
        return Err("SWF payload is shorter than 12 bytes.".into());
    }
    let sig = &bytes[0..3];
    if sig != b"FWS" && sig != b"CWS" && sig != b"ZWS" {
        return Err("Unsupported SWF signature.".into());
    }
    let swf_version = u32::from(bytes[3]);
    let file_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let body: Vec<u8> = if sig == b"FWS" {
        if bytes.len() - 8 > MAX_SWF_DECOMPRESSED_BYTES {
            return Err("SWF payload exceeds 16 MiB limit.".into());
        }
        bytes[8..].to_vec()
    } else if sig == b"CWS" {
        neoflash_core::as_binary::binary_transform(
            "uncompress",
            &bytes[8..],
            "",
            "zlib",
            MAX_SWF_DECOMPRESSED_BYTES,
        )
        .map_err(|e| format!("Invalid CWS zlib stream: {e}"))?
    } else {
        if bytes.len() < 17 {
            return Err("Truncated ZWS LZMA header.".into());
        }
        let uncomp_len = u64::from(file_len.saturating_sub(8));
        if uncomp_len as usize > MAX_SWF_DECOMPRESSED_BYTES {
            return Err("ZWS payload exceeds 16 MiB limit.".into());
        }
        let mut lzma_stream = Vec::with_capacity(13 + (bytes.len() - 17));
        lzma_stream.extend_from_slice(&bytes[12..17]);
        lzma_stream.extend_from_slice(&uncomp_len.to_le_bytes());
        lzma_stream.extend_from_slice(&bytes[17..]);
        neoflash_core::as_binary::binary_transform(
            "uncompress",
            &lzma_stream,
            "",
            "lzma",
            MAX_SWF_DECOMPRESSED_BYTES,
        )
        .map_err(|e| format!("Invalid ZWS LZMA stream: {e}"))?
    };

    let mut reader = BitReader::new(&body, 0);
    let [xmin, xmax, ymin, ymax] = reader.read_rect_twips()?;
    let width = (((i64::from(xmax) - i64::from(xmin)).max(20) / 20) as u64).min(8192) as u32;
    let height = (((i64::from(ymax) - i64::from(ymin)).max(20) / 20) as u64).min(8192) as u32;
    let fr_raw = reader.read_u16()?;
    let frame_rate = (f64::from(fr_raw) / 256.0).clamp(1.0, 120.0);
    let header_frame_count = u32::from(reader.read_u16()?).clamp(1, MAX_SWF_FRAMES);

    let mount_seq = SWF_MOUNT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut ctx = SwfDecodeCtx {
        mount_seq,
        action_script_version: if swf_version >= 9 { 3 } else { 2 },
        saw_file_attributes: false,
        saw_do_action: false,
        saw_do_abc: false,
        bg_color: None,
        resources: Vec::new(),
        symbols: Vec::new(),
        char_symbols: HashMap::new(),
        char_edit_texts: HashMap::new(),
        char_bitmaps: HashMap::new(),
        fonts: HashMap::new(),
        symbol_classes: HashMap::new(),
        export_assets: HashMap::new(),
        scripts: Vec::new(),
        abc_blocks: Vec::new(),
        init_actions: Vec::new(),
        total_keyframes: 0,
        total_placements: 0,
        warnings: Vec::new(),
        warning_set: HashSet::new(),
    };

    let tags = parse_tag_slice(&body[reader.byte_pos()..])?;
    for &(tag_code, tag_bytes) in &tags {
        match tag_code {
            69 if !tag_bytes.is_empty() => {
                ctx.saw_file_attributes = true;
                ctx.action_script_version = if (tag_bytes[0] & 0x08) != 0 { 3 } else { 2 };
            }
            9 if tag_bytes.len() >= 3 => {
                ctx.bg_color = Some(
                    (u32::from(tag_bytes[0]) << 16)
                        | (u32::from(tag_bytes[1]) << 8)
                        | u32::from(tag_bytes[2]),
                );
            }
            2 | 22 | 32 | 83 => {
                let shape_ver = match tag_code {
                    2 => 1,
                    22 => 2,
                    32 => 3,
                    _ => 4,
                };
                let _ = decode_shape_tag(&mut ctx, tag_bytes, shape_ver);
            }
            20 | 36 => {
                let _ = decode_lossless_bitmap_tag(&mut ctx, tag_bytes, tag_code == 36);
            }
            21 | 35 => {
                let _ = decode_jpeg_bitmap_tag(&mut ctx, tag_bytes, tag_code == 35);
            }
            10 | 48 | 75 => {
                let _ = decode_font_tag(&mut ctx, tag_bytes, tag_code);
            }
            88 => {
                let _ = decode_font_name_tag(&mut ctx, tag_bytes);
            }
            11 | 33 => {
                let _ = decode_static_text_tag(&mut ctx, tag_bytes, tag_code == 33);
            }
            37 => {
                let _ = decode_edit_text_tag(&mut ctx, tag_bytes);
            }
            39 => {
                let _ = decode_sprite_tag(&mut ctx, tag_bytes);
            }
            56 => {
                let _ = decode_export_assets_tag(&mut ctx, tag_bytes);
            }
            76 => {
                let _ = decode_symbol_class_tag(&mut ctx, tag_bytes);
            }
            72 | 82 => {
                ctx.saw_do_abc = true;
                match parse_abc_tag(tag_bytes, tag_code == 82) {
                    Ok(abc) => ctx.abc_blocks.push(abc),
                    Err(err) => ctx.warn(format!("SWF DoABC tag skipped: {err}")),
                }
            }
            59 if tag_bytes.len() > 2 => {
                ctx.saw_do_action = true;
                let payload = &tag_bytes[2..];
                let sprite = u16::from_le_bytes([tag_bytes[0], tag_bytes[1]]);
                if let Some(js) = compile_avm1_actions(
                    &mut ctx,
                    payload,
                    &format!("DoInitAction sprite {sprite}"),
                ) {
                    ctx.init_actions.push(js);
                }
            }
            _ => {}
        }
    }

    let mut root_timeline =
        build_timeline_from_tags(&mut ctx, &tags, Some(header_frame_count), "root");

    if !ctx.saw_file_attributes {
        if ctx.saw_do_abc {
            ctx.action_script_version = 3;
        } else if ctx.saw_do_action {
            ctx.action_script_version = 2;
        }
    }

    for (&cid, linkage) in &ctx.export_assets {
        if let Some(sym_id) = ctx.char_symbols.get(&cid)
            && let Some(sym) = ctx.symbols.iter_mut().find(|s| &s.id == sym_id)
            && matches!(sym.kind, SymbolKind::MovieClip | SymbolKind::Button)
        {
            sym.linkage_id = Some(linkage.clone());
        }
    }

    if !ctx.abc_blocks.is_empty() || !ctx.symbol_classes.is_empty() {
        apply_abc_scripts(&mut ctx, &mut root_timeline);
    }

    let root_id = format!("swf-{mount_seq}-root");
    let frame_count = root_timeline.frame_count.max(1);
    ctx.symbols.insert(
        0,
        Symbol {
            id: root_id.clone(),
            name: "MainTimeline".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: root_timeline,
            class_name: None,
            linkage_id: None,
        },
    );

    let bg_rgba = ctx.bg_color.map_or([255, 255, 255, 0], |rgb| {
        [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255]
    });
    let mut project = Project {
        format_version: 6,
        name: "Loaded SWF".into(),
        width: width.max(1),
        height: height.max(1),
        fps: (frame_rate.round() as u32).clamp(1, 120),
        background: bg_rgba,
        root: root_id,
        symbols: ctx.symbols,
        library_folders: Vec::new(),
        sources: Vec::new(),
        resources: ctx.resources,
        document_class: None,
        scenes: Vec::new(),
        script_language: Some(ScriptLanguage::JavaScript),
        also_compile_compatible_scripts: true,
    };
    project.upgrade_format_for_features();

    let swf_info = DecodedSwfInfo {
        swf_version,
        action_script_version: ctx.action_script_version,
        width: width.max(1),
        height: height.max(1),
        frame_rate,
        frame_count,
        background_color: ctx.bg_color,
    };

    Ok(DecodedSwfBundle {
        swf_info,
        project,
        scripts: ctx.scripts,
        warnings: ctx.warnings,
    })
}

impl PartialEq for ActivePlacement {
    fn eq(&self, other: &Self) -> bool {
        self.char_id == other.char_id
            && self.matrix == other.matrix
            && self.cxform == other.cxform
            && self.name == other.name
            && self.clip_depth == other.clip_depth
            && self.visible == other.visible
            && self.cache_as_bitmap == other.cache_as_bitmap
            && self.blend_mode == other.blend_mode
            && self.filters == other.filters
    }
}

struct SwfDecodeCtx {
    mount_seq: u64,
    action_script_version: u32,
    saw_file_attributes: bool,
    saw_do_action: bool,
    saw_do_abc: bool,
    bg_color: Option<u32>,
    resources: Vec<ResourceUnit>,
    symbols: Vec<Symbol>,
    char_symbols: HashMap<u16, String>,
    char_edit_texts: HashMap<u16, (ElementKind, f64, f64)>,
    char_bitmaps: HashMap<u16, (String, u32, u32)>,
    fonts: HashMap<u16, SwfFontRecord>,
    symbol_classes: HashMap<u16, String>,
    export_assets: HashMap<u16, String>,
    scripts: Vec<CompiledSwfScriptUnit>,
    abc_blocks: Vec<AbcFile>,
    init_actions: Vec<String>,
    total_keyframes: usize,
    total_placements: usize,
    warnings: Vec<String>,
    warning_set: HashSet<String>,
}

impl SwfDecodeCtx {
    fn warn_opcode(&mut self, vm: &str, opcode: u8, offset: usize) {
        let key = format!("{vm} opcode 0x{opcode:02X}");
        if self.warnings.len() < 256 && self.warning_set.insert(key.clone()) {
            self.warnings.push(format!(
                "SWF {key} is not supported in the bounded SWF loader and was skipped (bytecode offset 0x{offset:X})."
            ));
        }
    }

    pub(super) fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.warning_set.insert(msg.clone()) && self.warnings.len() < 256 {
            self.warnings.push(msg);
        }
    }
}

fn parse_tag_slice(data: &[u8]) -> Result<Vec<(u16, &[u8])>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 2 <= data.len() && out.len() < MAX_SWF_TAGS {
        let hdr = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;
        let code = hdr >> 6;
        let mut len = usize::from(hdr & 0x3F);
        if len == 0x3F {
            if pos + 4 > data.len() {
                break;
            }
            len = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize;
            pos += 4;
        }
        if code == 0 {
            break;
        }
        let end = pos.saturating_add(len).min(data.len());
        out.push((code, &data[pos..end]));
        pos = end;
    }
    Ok(out)
}

fn attach_compiled_frame_script(
    ctx: &mut SwfDecodeCtx,
    timeline: &mut Timeline,
    frame: u32,
    js_code: &str,
    label: &str,
    language: ScriptLanguage,
) {
    if js_code.trim().is_empty() {
        return;
    }
    let idx = ctx.scripts.len() + 1;
    let source_id = format!("/*swf-{}-{label}-{idx}-f{frame}*/", ctx.mount_seq);
    if let Some(existing) = timeline.scripts.iter_mut().find(|s| s.frame == frame)
        && let Some(unit) = ctx
            .scripts
            .iter_mut()
            .find(|u| u.source == existing.script.source)
    {
        unit.code.push('\n');
        unit.code.push_str(js_code);
        return;
    }
    ctx.scripts.push(CompiledSwfScriptUnit {
        language,
        source: source_id.clone(),
        code: js_code.to_owned(),
        location: format!("{label}:frame-{}", frame + 1),
    });
    timeline.scripts.push(FrameScript {
        layer: None,
        frame,
        script: Script {
            language,
            source: source_id,
        },
    });
}

fn parse_fill_styles(r: &mut BitReader<'_>, shape_ver: u8) -> Result<Vec<SwfFillStyle>, String> {
    r.align();
    let mut count = usize::from(r.read_u8()?);
    if count == 0xFF && shape_ver >= 2 {
        count = usize::from(r.read_u16()?);
    }
    let mut styles = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        let kind = r.read_u8()?;
        match kind {
            0x00 => {
                let c = if shape_ver >= 3 {
                    r.read_rgba()?
                } else {
                    r.read_rgb()?
                };
                styles.push(SwfFillStyle::Solid(c));
            }
            0x10 | 0x12 | 0x13 => {
                let matrix = r.read_matrix()?;
                r.align();
                let spread_raw = r.read_ubits(2)?;
                let interp_raw = r.read_ubits(2)?;
                let num_grads = (r.read_ubits(4)? as usize).clamp(1, 15);
                let mut stops = Vec::with_capacity(num_grads);
                for _ in 0..num_grads {
                    let ratio = r.read_u8()?;
                    let color = if shape_ver >= 3 {
                        r.read_rgba()?
                    } else {
                        r.read_rgb()?
                    };
                    stops.push(GradientStop { ratio, color });
                }
                let focal = if kind == 0x13 {
                    r.read_fixed8()?.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                let fallback = stops.first().map_or([0, 0, 0, 255], |s| s.color);
                let spread = match spread_raw {
                    1 => GradientSpread::Reflect,
                    2 => GradientSpread::Repeat,
                    _ => GradientSpread::Extend,
                };
                let gp = GradientPaint {
                    kind: if kind == 0x10 {
                        GradientKind::Linear
                    } else {
                        GradientKind::Radial
                    },
                    matrix,
                    stops,
                    focal_point_ratio: focal,
                    spread,
                    linear_rgb: interp_raw == 1,
                };
                styles.push(SwfFillStyle::Gradient(gp, fallback));
            }
            0x40..=0x43 => {
                let char_id = r.read_u16()?;
                let _bmp_matrix = r.read_matrix()?;
                styles.push(SwfFillStyle::Bitmap { char_id });
            }
            _ => styles.push(SwfFillStyle::Solid([0, 0, 0, 0])),
        }
    }
    Ok(styles)
}

fn parse_line_styles(r: &mut BitReader<'_>, shape_ver: u8) -> Result<Vec<SwfLineStyle>, String> {
    r.align();
    let mut count = usize::from(r.read_u8()?);
    if count == 0xFF && shape_ver >= 2 {
        count = usize::from(r.read_u16()?);
    }
    let mut styles = Vec::with_capacity(count.min(256));
    for _ in 0..count {
        let width_twips = r.read_u16()?;
        let width = (f64::from(width_twips) / 20.0).max(if width_twips > 0 { 0.5 } else { 0.0 });
        if shape_ver < 4 {
            let color = if shape_ver >= 3 {
                r.read_rgba()?
            } else {
                r.read_rgb()?
            };
            styles.push(SwfLineStyle {
                width,
                color,
                gradient: None,
            });
        } else {
            let _start_cap = r.read_ubits(2)?;
            let join_style = r.read_ubits(2)?;
            let has_fill = r.read_ubits(1)? == 1;
            let _no_hscale = r.read_ubits(1)?;
            let _no_vscale = r.read_ubits(1)?;
            let _pixel_hint = r.read_ubits(1)?;
            let _reserved = r.read_ubits(5)?;
            let _no_close = r.read_ubits(1)?;
            let _end_cap = r.read_ubits(2)?;
            if join_style == 2 {
                let _miter = r.read_u16()?;
            }
            if has_fill {
                let sub = parse_fill_styles_single(r, shape_ver)?;
                match sub {
                    SwfFillStyle::Gradient(gp, fb) => styles.push(SwfLineStyle {
                        width,
                        color: fb,
                        gradient: Some(gp),
                    }),
                    SwfFillStyle::Solid(c) => styles.push(SwfLineStyle {
                        width,
                        color: c,
                        gradient: None,
                    }),
                    SwfFillStyle::Bitmap { .. } => styles.push(SwfLineStyle {
                        width,
                        color: [0, 0, 0, 255],
                        gradient: None,
                    }),
                }
            } else {
                let color = r.read_rgba()?;
                styles.push(SwfLineStyle {
                    width,
                    color,
                    gradient: None,
                });
            }
        }
    }
    Ok(styles)
}

fn parse_fill_styles_single(r: &mut BitReader<'_>, shape_ver: u8) -> Result<SwfFillStyle, String> {
    let kind = r.read_u8()?;
    match kind {
        0x00 => Ok(SwfFillStyle::Solid(if shape_ver >= 3 {
            r.read_rgba()?
        } else {
            r.read_rgb()?
        })),
        0x10 | 0x12 | 0x13 => {
            let matrix = r.read_matrix()?;
            r.align();
            let spread_raw = r.read_ubits(2)?;
            let interp_raw = r.read_ubits(2)?;
            let num_grads = (r.read_ubits(4)? as usize).clamp(1, 15);
            let mut stops = Vec::with_capacity(num_grads);
            for _ in 0..num_grads {
                let ratio = r.read_u8()?;
                let color = if shape_ver >= 3 {
                    r.read_rgba()?
                } else {
                    r.read_rgb()?
                };
                stops.push(GradientStop { ratio, color });
            }
            let focal = if kind == 0x13 {
                r.read_fixed8()?.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let fb = stops.first().map_or([0, 0, 0, 255], |s| s.color);
            Ok(SwfFillStyle::Gradient(
                GradientPaint {
                    kind: if kind == 0x10 {
                        GradientKind::Linear
                    } else {
                        GradientKind::Radial
                    },
                    matrix,
                    stops,
                    focal_point_ratio: focal,
                    spread: match spread_raw {
                        1 => GradientSpread::Reflect,
                        2 => GradientSpread::Repeat,
                        _ => GradientSpread::Extend,
                    },
                    linear_rgb: interp_raw == 1,
                },
                fb,
            ))
        }
        0x40..=0x43 => {
            let char_id = r.read_u16()?;
            let _ = r.read_matrix()?;
            Ok(SwfFillStyle::Bitmap { char_id })
        }
        _ => Ok(SwfFillStyle::Solid([0, 0, 0, 0])),
    }
}

fn chain_segments_to_commands(segs: &[DirectedSeg], close_contours: bool) -> Vec<PathCommand> {
    if segs.is_empty() {
        return Vec::new();
    }
    let mut used = vec![false; segs.len()];
    let mut commands = Vec::with_capacity(segs.len() + 4);
    for start_idx in 0..segs.len() {
        if used[start_idx] {
            continue;
        }
        used[start_idx] = true;
        let first = segs[start_idx];
        let (sx, sy) = first.start();
        commands.push(PathCommand::MoveTo {
            x: f64::from(sx) / 20.0,
            y: f64::from(sy) / 20.0,
        });
        push_seg_cmd(&mut commands, first);
        let mut cur_end = first.end();
        loop {
            if close_contours && cur_end == (sx, sy) {
                commands.push(PathCommand::Close);
                break;
            }
            let mut next_found = None;
            for (i, candidate) in segs.iter().enumerate() {
                if !used[i] && candidate.start() == cur_end {
                    next_found = Some(i);
                    break;
                }
            }
            if let Some(ni) = next_found {
                used[ni] = true;
                push_seg_cmd(&mut commands, segs[ni]);
                cur_end = segs[ni].end();
            } else {
                if close_contours {
                    commands.push(PathCommand::Close);
                }
                break;
            }
        }
    }
    commands
}

fn push_seg_cmd(commands: &mut Vec<PathCommand>, seg: DirectedSeg) {
    match seg {
        DirectedSeg::Line { x1, y1, .. } => commands.push(PathCommand::LineTo {
            x: f64::from(x1) / 20.0,
            y: f64::from(y1) / 20.0,
        }),
        DirectedSeg::Quad { cx, cy, x1, y1, .. } => commands.push(PathCommand::QuadraticTo {
            cx: f64::from(cx) / 20.0,
            cy: f64::from(cy) / 20.0,
            x: f64::from(x1) / 20.0,
            y: f64::from(y1) / 20.0,
        }),
    }
}

fn decode_shape_tag(ctx: &mut SwfDecodeCtx, data: &[u8], shape_ver: u8) -> Result<(), String> {
    let mut r = BitReader::new(data, 0);
    let char_id = r.read_u16()?;
    let shape_bounds = r.read_rect_twips()?;
    if shape_ver >= 4 {
        let _edge_bounds = r.read_rect_twips()?;
        let _flags = r.read_u8()?;
    }

    let mut fill_styles = parse_fill_styles(&mut r, shape_ver)?;
    let mut line_styles = parse_line_styles(&mut r, shape_ver)?;
    r.align();
    let mut num_fill_bits = r.read_ubits(4)? as usize;
    let mut num_line_bits = r.read_ubits(4)? as usize;

    let mut cur_x = 0i32;
    let mut cur_y = 0i32;
    let mut fs0 = 0usize;
    let mut fs1 = 0usize;
    let mut ls = 0usize;

    let mut fill_groups: BTreeMap<usize, Vec<DirectedSeg>> = BTreeMap::new();
    let mut line_groups: BTreeMap<usize, Vec<DirectedSeg>> = BTreeMap::new();
    let mut elements: Vec<Element> = Vec::new();
    let mut elem_idx = 0usize;

    let flush_groups = |elements: &mut Vec<Element>,
                        elem_idx: &mut usize,
                        fill_styles: &[SwfFillStyle],
                        line_styles: &[SwfLineStyle],
                        fill_groups: &mut BTreeMap<usize, Vec<DirectedSeg>>,
                        line_groups: &mut BTreeMap<usize, Vec<DirectedSeg>>,
                        char_bitmaps: &HashMap<u16, (String, u32, u32)>,
                        shape_bounds: [i32; 4],
                        mount_seq: u64,
                        char_id: u16| {
        for (&f_idx, segs) in fill_groups.iter() {
            let Some(style) = fill_styles.get(f_idx.saturating_sub(1)) else {
                continue;
            };
            let cmds = chain_segments_to_commands(segs, true);
            if cmds.is_empty() {
                continue;
            }
            *elem_idx += 1;
            let el_id = format!("swf-{mount_seq}-sh{char_id}-e{elem_idx}");
            match style {
                SwfFillStyle::Solid(c) => {
                    elements.push(make_default_element(
                        el_id,
                        ElementKind::Path {
                            commands: cmds,
                            fill_rule: FillRule::EvenOdd,
                            fill: *c,
                            stroke: [0, 0, 0, 0],
                            stroke_width: 0.0,
                        },
                        0.0,
                        0.0,
                    ));
                }
                SwfFillStyle::Gradient(gp, fb) => {
                    elements.push(make_default_element(
                        el_id,
                        ElementKind::PaintedPath {
                            commands: cmds,
                            fill_rule: FillRule::EvenOdd,
                            fill: *fb,
                            stroke: [0, 0, 0, 0],
                            fill_gradient: Some(gp.clone()),
                            stroke_gradient: None,
                            stroke_width: 0.0,
                        },
                        0.0,
                        0.0,
                    ));
                }
                SwfFillStyle::Bitmap { char_id: bmp_cid } => {
                    if let Some((res_id, bw, bh)) = char_bitmaps.get(bmp_cid) {
                        let bx = f64::from(shape_bounds[0]) / 20.0;
                        let by = f64::from(shape_bounds[2]) / 20.0;
                        elements.push(make_default_element(
                            el_id,
                            ElementKind::Bitmap {
                                resource_id: res_id.clone(),
                                width: *bw,
                                height: *bh,
                                smoothing: true,
                            },
                            bx,
                            by,
                        ));
                    } else {
                        elements.push(make_default_element(
                            el_id,
                            ElementKind::Path {
                                commands: cmds,
                                fill_rule: FillRule::EvenOdd,
                                fill: [128, 128, 128, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            0.0,
                            0.0,
                        ));
                    }
                }
            }
        }
        fill_groups.clear();

        for (&l_idx, segs) in line_groups.iter() {
            let Some(style) = line_styles.get(l_idx.saturating_sub(1)) else {
                continue;
            };
            if style.width <= 0.0 {
                continue;
            }
            let cmds = chain_segments_to_commands(segs, false);
            if cmds.is_empty() {
                continue;
            }
            *elem_idx += 1;
            let el_id = format!("swf-{mount_seq}-sh{char_id}-s{elem_idx}");
            let kind = if let Some(gp) = &style.gradient {
                ElementKind::PaintedPath {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: style.color,
                    fill_gradient: None,
                    stroke_gradient: Some(gp.clone()),
                    stroke_width: style.width,
                }
            } else {
                ElementKind::Path {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill: [0, 0, 0, 0],
                    stroke: style.color,
                    stroke_width: style.width,
                }
            };
            elements.push(make_default_element(el_id, kind, 0.0, 0.0));
        }
        line_groups.clear();
    };

    let mut guard = 0usize;
    while guard < 65536 {
        guard += 1;
        let Ok(type_flag) = r.read_ubits(1) else {
            break;
        };
        if type_flag == 0 {
            let flags = r.read_ubits(5).unwrap_or(0);
            if flags == 0 {
                break;
            }
            if (flags & 0x01) != 0 {
                let move_bits = r.read_ubits(5)? as usize;
                cur_x = r.read_sbits(move_bits)?;
                cur_y = r.read_sbits(move_bits)?;
            }
            if (flags & 0x02) != 0 {
                fs0 = r.read_ubits(num_fill_bits)? as usize;
            }
            if (flags & 0x04) != 0 {
                fs1 = r.read_ubits(num_fill_bits)? as usize;
            }
            if (flags & 0x08) != 0 {
                ls = r.read_ubits(num_line_bits)? as usize;
            }
            if (flags & 0x10) != 0 && shape_ver >= 2 {
                flush_groups(
                    &mut elements,
                    &mut elem_idx,
                    &fill_styles,
                    &line_styles,
                    &mut fill_groups,
                    &mut line_groups,
                    &ctx.char_bitmaps,
                    shape_bounds,
                    ctx.mount_seq,
                    char_id,
                );
                fill_styles = parse_fill_styles(&mut r, shape_ver)?;
                line_styles = parse_line_styles(&mut r, shape_ver)?;
                r.align();
                num_fill_bits = r.read_ubits(4)? as usize;
                num_line_bits = r.read_ubits(4)? as usize;
                fs0 = 0;
                fs1 = 0;
                ls = 0;
            }
        } else {
            let straight = r.read_ubits(1)? == 1;
            let nbits = (r.read_ubits(4)? as usize) + 2;
            let seg = if straight {
                let general = r.read_ubits(1)? == 1;
                let (dx, dy) = if general {
                    (r.read_sbits(nbits)?, r.read_sbits(nbits)?)
                } else if r.read_ubits(1)? == 1 {
                    (0, r.read_sbits(nbits)?)
                } else {
                    (r.read_sbits(nbits)?, 0)
                };
                let nx = cur_x + dx;
                let ny = cur_y + dy;
                let s = DirectedSeg::Line {
                    x0: cur_x,
                    y0: cur_y,
                    x1: nx,
                    y1: ny,
                };
                cur_x = nx;
                cur_y = ny;
                s
            } else {
                let cdx = r.read_sbits(nbits)?;
                let cdy = r.read_sbits(nbits)?;
                let adx = r.read_sbits(nbits)?;
                let ady = r.read_sbits(nbits)?;
                let cx = cur_x + cdx;
                let cy = cur_y + cdy;
                let nx = cx + adx;
                let ny = cy + ady;
                let s = DirectedSeg::Quad {
                    x0: cur_x,
                    y0: cur_y,
                    cx,
                    cy,
                    x1: nx,
                    y1: ny,
                };
                cur_x = nx;
                cur_y = ny;
                s
            };
            if fs1 > 0 {
                fill_groups.entry(fs1).or_default().push(seg);
            }
            if fs0 > 0 {
                fill_groups.entry(fs0).or_default().push(seg.reversed());
            }
            if ls > 0 {
                line_groups.entry(ls).or_default().push(seg);
            }
        }
    }

    flush_groups(
        &mut elements,
        &mut elem_idx,
        &fill_styles,
        &line_styles,
        &mut fill_groups,
        &mut line_groups,
        &ctx.char_bitmaps,
        shape_bounds,
        ctx.mount_seq,
        char_id,
    );

    let sym_id = format!("swf-{}-char-{char_id}", ctx.mount_seq);
    let kf = Keyframe {
        elements,
        ..Default::default()
    };
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("Shape_{char_id}"),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: format!("swf-{}-shlyr-{char_id}", ctx.mount_seq),
                name: "Shape".into(),
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

fn make_default_element(id: String, kind: ElementKind, x: f64, y: f64) -> Element {
    Element {
        id,
        name: String::new(),
        transform: Transform {
            x,
            y,
            ..Transform::default()
        },
        kind,
        scripts: Vec::new(),
        accessibility: None,
        visible: true,
        cache_as_bitmap: false,
        blend_mode: BlendMode::Normal,
        filters: Vec::new(),
        merge: false,
        instance_behavior: None,
    }
}

fn crc32_png(tag: &[u8], payload: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in tag.iter().chain(payload.iter()) {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if (crc & 1) != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || rgba.len() != w * h * 4 {
        return Err("Invalid RGBA buffer for PNG synthesis".into());
    }
    let mut raw_scanlines = Vec::with_capacity(h * (1 + w * 4));
    for row in 0..h {
        raw_scanlines.push(0u8);
        raw_scanlines.extend_from_slice(&rgba[row * w * 4..(row + 1) * w * 4]);
    }
    let idat = neoflash_core::as_binary::binary_transform(
        "compress",
        &raw_scanlines,
        "",
        "zlib",
        MAX_BITMAP_RAW_BYTES,
    )
    .map_err(|e| format!("PNG zlib compress failed: {e}"))?;

    let mut out = Vec::with_capacity(64 + idat.len());
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    let write_chunk = |buf: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]| {
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(tag);
        buf.extend_from_slice(payload);
        buf.extend_from_slice(&crc32_png(tag, payload).to_be_bytes());
    };
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &idat);
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

fn decode_lossless_bitmap_tag(
    ctx: &mut SwfDecodeCtx,
    data: &[u8],
    with_alpha: bool,
) -> Result<(), String> {
    if data.len() < 8 {
        return Err("Truncated DefineBitsLossless".into());
    }
    let char_id = u16::from_le_bytes([data[0], data[1]]);
    let format = data[2];
    let width = u32::from(u16::from_le_bytes([data[3], data[4]]));
    let height = u32::from(u16::from_le_bytes([data[5], data[6]]));
    if width == 0 || height == 0 || (width as usize) * (height as usize) > 4 * 1024 * 1024 {
        return Err("Bitmap dimensions out of bounds".into());
    }
    let (color_table_size, z_start) = if format == 3 {
        if data.len() < 9 {
            return Err("Truncated DefineBitsLossless colormap".into());
        }
        (usize::from(data[7]) + 1, 8usize)
    } else {
        (0usize, 7usize)
    };
    let raw = neoflash_core::as_binary::binary_transform(
        "uncompress",
        &data[z_start..],
        "",
        "zlib",
        MAX_BITMAP_RAW_BYTES,
    )
    .map_err(|e| format!("Invalid lossless bitmap zlib: {e}"))?;

    let w = width as usize;
    let h = height as usize;
    let mut rgba = vec![0u8; w * h * 4];
    match format {
        3 => {
            let entry_bytes = if with_alpha { 4 } else { 3 };
            let ct_bytes = color_table_size * entry_bytes;
            if raw.len() < ct_bytes {
                return Err("Truncated colormap".into());
            }
            let pitch = (w + 3) & !3;
            let pix = &raw[ct_bytes..];
            for y in 0..h {
                for x in 0..w {
                    let idx = usize::from(*pix.get(y * pitch + x).unwrap_or(&0));
                    let c_off = idx.min(color_table_size.saturating_sub(1)) * entry_bytes;
                    let r = raw[c_off];
                    let g = raw[c_off + 1];
                    let b = raw[c_off + 2];
                    let a = if with_alpha { raw[c_off + 3] } else { 255 };
                    let (ur, ug, ub) = unpremultiply_rgb(r, g, b, a, with_alpha);
                    let di = (y * w + x) * 4;
                    rgba[di..di + 4].copy_from_slice(&[ur, ug, ub, a]);
                }
            }
        }
        5 => {
            for i in 0..(w * h) {
                let si = i * 4;
                if si + 4 > raw.len() {
                    break;
                }
                let a = if with_alpha { raw[si] } else { 255 };
                let (ur, ug, ub) =
                    unpremultiply_rgb(raw[si + 1], raw[si + 2], raw[si + 3], a, with_alpha);
                rgba[si..si + 4].copy_from_slice(&[ur, ug, ub, a]);
            }
        }
        _ => return Err("Unsupported lossless format".into()),
    }

    let png_bytes = encode_rgba_png(width, height, &rgba)?;
    register_bitmap_resource_and_symbol(ctx, char_id, width, height, "image/png", "png", &png_bytes)
}

fn unpremultiply_rgb(r: u8, g: u8, b: u8, a: u8, premul: bool) -> (u8, u8, u8) {
    if !premul || a == 0 || a == 255 {
        if premul && a == 0 {
            return (0, 0, 0);
        }
        return (r, g, b);
    }
    let scale = 255.0 / f64::from(a);
    (
        (f64::from(r) * scale).round().clamp(0.0, 255.0) as u8,
        (f64::from(g) * scale).round().clamp(0.0, 255.0) as u8,
        (f64::from(b) * scale).round().clamp(0.0, 255.0) as u8,
    )
}

fn decode_jpeg_bitmap_tag(
    ctx: &mut SwfDecodeCtx,
    data: &[u8],
    has_alpha: bool,
) -> Result<(), String> {
    if data.len() < 6 {
        return Err("Truncated DefineBitsJPEG".into());
    }
    let char_id = u16::from_le_bytes([data[0], data[1]]);
    let img_slice = if has_alpha {
        if data.len() < 10 {
            return Err("Truncated DefineBitsJPEG3".into());
        }
        let alpha_off = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
        &data[6..(6 + alpha_off).min(data.len())]
    } else {
        &data[2..]
    };
    let clean = if img_slice.len() >= 4 && img_slice[0..4] == [0xFF, 0xD9, 0xFF, 0xD8] {
        &img_slice[4..]
    } else {
        img_slice
    };
    let (mime, ext, w, h) = sniff_image_dimensions(clean);
    register_bitmap_resource_and_symbol(ctx, char_id, w, h, mime, ext, clean)
}

fn sniff_image_dimensions(bytes: &[u8]) -> (&'static str, &'static str, u32, u32) {
    if bytes.len() >= 24 && bytes[0..8] == [137, 80, 78, 71, 13, 10, 26, 10] {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]).max(1);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]).max(1);
        return ("image/png", "png", w, h);
    }
    if bytes.len() >= 10 && &bytes[0..3] == b"GIF" {
        let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]])).max(1);
        let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]])).max(1);
        return ("image/gif", "gif", w, h);
    }
    let mut pos = 2usize;
    while pos + 9 < bytes.len() {
        if bytes[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = bytes[pos + 1];
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u32::from(u16::from_be_bytes([bytes[pos + 5], bytes[pos + 6]])).max(1);
            let w = u32::from(u16::from_be_bytes([bytes[pos + 7], bytes[pos + 8]])).max(1);
            return ("image/jpeg", "jpg", w, h);
        }
        if marker == 0xD8 || marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            pos += 2;
            continue;
        }
        let seg_len = usize::from(u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]));
        if seg_len < 2 {
            break;
        }
        pos += 2 + seg_len;
    }
    ("image/jpeg", "jpg", 64, 64)
}

fn register_bitmap_resource_and_symbol(
    ctx: &mut SwfDecodeCtx,
    char_id: u16,
    width: u32,
    height: u32,
    mime: &str,
    ext: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let res_id = format!("swf-{}-bmp-{char_id}", ctx.mount_seq);
    let path = format!("swf_bmp_{}_{char_id}.{ext}", ctx.mount_seq);
    let unit =
        ResourceUnit::from_bytes(res_id.clone(), path, mime, bytes).map_err(|e| e.to_string())?;
    ctx.resources.push(unit);
    ctx.char_bitmaps
        .insert(char_id, (res_id.clone(), width, height));

    let sym_id = format!("swf-{}-char-{char_id}", ctx.mount_seq);
    let mut kf = Keyframe::default();
    kf.elements.push(make_default_element(
        format!("swf-{}-bmpel-{char_id}", ctx.mount_seq),
        ElementKind::Bitmap {
            resource_id: res_id,
            width,
            height,
            smoothing: true,
        },
        0.0,
        0.0,
    ));
    ctx.symbols.push(Symbol {
        id: sym_id.clone(),
        name: format!("Bitmap_{char_id}"),
        folder: String::new(),
        kind: SymbolKind::Graphic,
        timeline: Timeline {
            frame_count: 1,
            layers: vec![Layer {
                id: format!("swf-{}-bmplyr-{char_id}", ctx.mount_seq),
                name: "Bitmap".into(),
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
