//! Layered Figma (`.fig` Kiwi archives & `.figma.json` REST/plugin exports) composition importer (`T-249`).
//!
//! Decomposes Figma pages, artboard frames, groups, reusable components/instances, clipping & sibling
//! masks, blend modes, vector networks/paths, solid/linear/radial/image paints, strokes, rounded
//! corners, drop-shadow/blur effects and editable text into multi-layer NeoFlash compositions and
//! Library `MovieClip` symbols with located approximation diagnostics.

use crate::{
    Diagnostic, ImportError, ImportFormat, ImportReport, Severity, Statistics,
    vector_fidelity::parse_svg_path_data,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::DeflateDecoder;
use neoflash_core::{
    BlendMode, Color, Element, ElementKind, FillRule, FilterSpec, GradientKind, GradientPaint,
    GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode,
    OvalOptions, PathCommand, PrimitiveOptions, Project, RectangleOptions, ResourceUnit, Symbol,
    SymbolKind, TextAlign, TextFormat, Timeline, Transform, new_id, oval_path_commands,
    polystar_path_commands, rectangle_path_commands, validate_path_commands,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

const MAX_FIGMA_FILE_BYTES: usize = 256 * 1024 * 1024;
const MAX_FIGMA_DECODE_BYTES: usize = 128 * 1024 * 1024;
const MAX_FIGMA_IMAGE_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_FIGMA_NODES: usize = 50_000;
const MAX_KIWI_VALUES: usize = 2_000_000;

/// How top-level Figma artboard/frame children map into the NeoFlash timeline (`T-249`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigmaLayerMapping {
    /// Each child in the composition artboard becomes its own NeoFlash `Layer` (with `Folder` and
    /// `Mask` hierarchy preserved), matching After Effects / Animate layered composition import.
    #[default]
    TimelineLayers,
    /// Wrap each top-level child in its own `SymbolKind::MovieClip` symbol and place on its layer.
    MovieClipSymbols,
    /// Place all converted elements onto a single timeline `Layer`.
    FlattenSingleLayer,
}

/// Options controlling Figma `.fig` and `.figma.json` import (`T-249`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FigmaImportOptions {
    pub layer_mapping: FigmaLayerMapping,
    pub import_hidden_layers: bool,
    pub preserve_masks: bool,
    pub preserve_blend_modes: bool,
    pub resize_stage_to_artboard: bool,
}

impl Default for FigmaImportOptions {
    fn default() -> Self {
        Self {
            layer_mapping: FigmaLayerMapping::TimelineLayers,
            import_hidden_layers: true,
            preserve_masks: true,
            preserve_blend_modes: true,
            resize_stage_to_artboard: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct FigmaDoc {
    name: String,
    pages: Vec<FigmaNode>,
    images: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
struct FigmaNode {
    id: String,
    name: String,
    node_type: String,
    visible: bool,
    locked: bool,
    opacity: f64,
    blend_mode: String,
    is_mask: bool,
    mask_type: String,
    clips_content: bool,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    rotation_deg: f64,
    relative_transform: Option<[f64; 6]>,
    corner_radii: [f64; 4],
    arc_data: Option<(f64, f64, f64)>, // (startingAngle_rad, endingAngle_rad, innerRadius_ratio)
    point_count: Option<u32>,
    star_inner_radius: Option<f64>,
    fills: Vec<FigmaPaint>,
    strokes: Vec<FigmaPaint>,
    stroke_weight: f64,
    fill_paths: Vec<(Vec<PathCommand>, FillRule)>,
    stroke_paths: Vec<Vec<PathCommand>>,
    text: Option<FigmaTextData>,
    effects: Vec<FigmaEffect>,
    component_id: Option<String>,
    background_color: Option<Color>,
    children: Vec<FigmaNode>,
}

impl Default for FigmaNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::from("Layer"),
            node_type: String::from("GROUP"),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend_mode: String::from("PASS_THROUGH"),
            is_mask: false,
            mask_type: String::from("ALPHA"),
            clips_content: false,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            rotation_deg: 0.0,
            relative_transform: None,
            corner_radii: [0.0; 4],
            arc_data: None,
            point_count: None,
            star_inner_radius: None,
            fills: Vec::new(),
            strokes: Vec::new(),
            stroke_weight: 0.0,
            fill_paths: Vec::new(),
            stroke_paths: Vec::new(),
            text: None,
            effects: Vec::new(),
            component_id: None,
            background_color: None,
            children: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
enum FigmaPaint {
    Solid {
        color: Color,
    },
    Gradient {
        kind: GradientKind,
        stops: Vec<GradientStop>,
        start: [f64; 2],
        end: [f64; 2],
    },
    Image {
        image_ref: String,
        alpha: f64,
    },
}

#[derive(Clone, Debug)]
struct FigmaTextData {
    characters: String,
    font_family: String,
    font_size: f64,
    bold: bool,
    italic: bool,
    align: TextAlign,
    letter_spacing: f64,
    line_height: Option<f64>,
}

#[derive(Clone, Debug)]
enum FigmaEffect {
    DropShadow {
        color: Color,
        offset_x: f64,
        offset_y: f64,
        radius: f64,
        inner: bool,
    },
    LayerBlur {
        radius: f64,
    },
    BackgroundBlur {
        radius: f64,
    },
}

/// Returns `true` if `bytes` looks like a `.fig` Kiwi archive (ZIP with `canvas.fig` or `fig-kiwi` magic)
/// or a `.figma.json` document.
pub fn is_figma_bytes(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        return true;
    }
    if bytes.starts_with(b"PK\x03\x04")
        && let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes))
    {
        for i in 0..zip.len().min(64) {
            if let Ok(entry) = zip.by_index(i)
                && (entry.name() == "canvas.fig" || entry.name().ends_with(".fig"))
            {
                return true;
            }
        }
    }
    let trimmed = std::str::from_utf8(&bytes[..bytes.len().min(4096)])
        .ok()
        .map(str::trim_start);
    if let Some(s) = trimmed
        && s.starts_with('{')
        && (s.contains("\"schemaVersion\"")
            || (s.contains("\"document\"") && s.contains("\"CANVAS\""))
            || (s.contains("\"nodes\"") && s.contains("\"DOCUMENT\"")))
    {
        return true;
    }
    false
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Binary `.fig` (`fig-kiwi` / `fig-jam.`) Sequential Kiwi Schema + Message Decoder
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum KiwiDefKind {
    Enum,
    Struct,
    Message,
}

#[derive(Clone, Debug)]
struct KiwiField {
    name: String,
    type_id: i32,
    is_array: bool,
    value: u32,
}

#[derive(Clone, Debug)]
struct KiwiDef {
    name: String,
    kind: KiwiDefKind,
    fields: Vec<KiwiField>,
}

#[derive(Clone, Debug)]
enum KiwiVal {
    Bool(bool),
    Byte(u8),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Enum(String),
    Array(Vec<KiwiVal>),
    Object(BTreeMap<String, KiwiVal>),
}

impl KiwiVal {
    fn as_obj(&self) -> Option<&BTreeMap<String, KiwiVal>> {
        match self {
            Self::Object(m) => Some(m),
            _ => None,
        }
    }
    fn as_arr(&self) -> Option<&[KiwiVal]> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) | Self::Enum(s) => Some(s),
            _ => None,
        }
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(i) => Some(*i as f64),
            Self::Uint(u) => Some(*u as f64),
            Self::Byte(b) => Some(*b as f64),
            _ => None,
        }
    }
    fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Uint(u) => Some(*u),
            Self::Int(i) if *i >= 0 => Some(*i as u64),
            Self::Byte(b) => Some(*b as u64),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

struct KiwiReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> KiwiReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn read_byte(&mut self) -> Result<u8, String> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| "Unexpected EOF in Kiwi stream".to_owned())?;
        self.pos += 1;
        Ok(b)
    }

    fn read_var_uint(&mut self) -> Result<u64, String> {
        let mut result = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.read_byte()?;
            result |= ((b & 0x7F) as u64) << shift;
            if (b & 0x80) == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err("VarUint overflow in Kiwi stream".into());
            }
        }
    }

    fn read_var_int(&mut self) -> Result<i64, String> {
        let u = self.read_var_uint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn read_var_float(&mut self) -> Result<f64, String> {
        let first = self.read_byte()?;
        if first == 0 {
            return Ok(0.0);
        }
        let b1 = self.read_byte()?;
        let b2 = self.read_byte()?;
        let b3 = self.read_byte()?;
        let bits = (first as u32) | ((b1 as u32) << 8) | ((b2 as u32) << 16) | ((b3 as u32) << 24);
        let ieee = bits.rotate_right(9);
        let f = f32::from_bits(ieee) as f64;
        Ok(if f.is_finite() { f } else { 0.0 })
    }

    fn read_string(&mut self) -> Result<String, String> {
        let start = self.pos;
        while self.pos < self.buf.len() && self.buf[self.pos] != 0 {
            self.pos += 1;
        }
        if self.pos >= self.buf.len() {
            return Err("Unterminated string in Kiwi stream".into());
        }
        let slice = &self.buf[start..self.pos];
        self.pos += 1;
        Ok(String::from_utf8_lossy(slice).into_owned())
    }
}

fn parse_kiwi_schema(bytes: &[u8]) -> Result<Vec<KiwiDef>, String> {
    let mut r = KiwiReader::new(bytes);
    let def_count = usize::try_from(r.read_var_uint()?)
        .map_err(|_| "Kiwi schema definition count overflow".to_owned())?;
    if def_count > 4096 {
        return Err("Kiwi schema exceeds 4096 definitions".into());
    }
    let mut defs = Vec::with_capacity(def_count);
    for _ in 0..def_count {
        let name = r.read_string()?;
        let kind_byte = r.read_byte()?;
        let kind = match kind_byte {
            0 => KiwiDefKind::Enum,
            1 => KiwiDefKind::Struct,
            2 => KiwiDefKind::Message,
            other => return Err(format!("Unknown Kiwi definition kind {other}")),
        };
        let field_count = usize::try_from(r.read_var_uint()?)
            .map_err(|_| "Kiwi definition field count overflow".to_owned())?;
        if field_count > 4096 {
            return Err("Kiwi definition exceeds 4096 fields".into());
        }
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            let fname = r.read_string()?;
            let type_id = i32::try_from(r.read_var_int()?)
                .map_err(|_| "Kiwi field type_id out of i32 range".to_owned())?;
            let is_array = r.read_byte()? != 0;
            let value = u32::try_from(r.read_var_uint()?)
                .map_err(|_| "Kiwi field tag out of u32 range".to_owned())?;
            fields.push(KiwiField {
                name: fname,
                type_id,
                is_array,
                value,
            });
        }
        defs.push(KiwiDef { name, kind, fields });
    }
    Ok(defs)
}

fn decode_kiwi_field_value(
    r: &mut KiwiReader<'_>,
    defs: &[KiwiDef],
    type_id: i32,
    is_array: bool,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<KiwiVal, String> {
    if depth > 64 {
        return Err("Kiwi message nesting exceeds depth 64".into());
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| "Kiwi stream exceeds 2M decoded value budget".to_owned())?;
    if is_array {
        let len = usize::try_from(r.read_var_uint()?)
            .map_err(|_| "Kiwi array length overflow".to_owned())?;
        if len > MAX_KIWI_VALUES {
            return Err("Kiwi array exceeds 2M elements".into());
        }
        // Fast path for byte arrays (`bytes` field in `Blob` uses type_id == -2, is_array == true)
        if type_id == -2 {
            let end = r
                .pos
                .checked_add(len)
                .ok_or_else(|| "Kiwi byte array overflow".to_owned())?;
            if end > r.buf.len() {
                return Err("Truncated Kiwi byte array".into());
            }
            let out = r.buf[r.pos..end].to_vec();
            r.pos = end;
            return Ok(KiwiVal::Bytes(out));
        }
        let mut items = Vec::with_capacity(len.min(4096));
        for _ in 0..len {
            items.push(decode_kiwi_scalar(
                r,
                defs,
                type_id,
                depth + 1,
                budget,
                active_structs,
            )?);
        }
        return Ok(KiwiVal::Array(items));
    }
    decode_kiwi_scalar(r, defs, type_id, depth, budget, active_structs)
}

fn decode_kiwi_scalar(
    r: &mut KiwiReader<'_>,
    defs: &[KiwiDef],
    type_id: i32,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<KiwiVal, String> {
    if depth > 64 {
        return Err("Kiwi message nesting exceeds depth 64".into());
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| "Kiwi stream exceeds 2M decoded value budget".to_owned())?;
    match type_id {
        -1 => Ok(KiwiVal::Bool(r.read_byte()? != 0)),
        -2 => Ok(KiwiVal::Byte(r.read_byte()?)),
        -3 => Ok(KiwiVal::Int(r.read_var_int()?)),
        -4 => Ok(KiwiVal::Uint(r.read_var_uint()?)),
        -5 => Ok(KiwiVal::Float(r.read_var_float()?)),
        -6 => Ok(KiwiVal::Str(r.read_string()?)),
        -7 => Ok(KiwiVal::Int(r.read_var_int()?)),
        -8 => Ok(KiwiVal::Uint(r.read_var_uint()?)),
        idx if idx >= 0 => {
            let def_idx = idx as usize;
            let def = defs
                .get(def_idx)
                .ok_or_else(|| format!("Invalid Kiwi definition index {idx}"))?;
            match def.kind {
                KiwiDefKind::Enum => {
                    let val = u32::try_from(r.read_var_uint()?)
                        .map_err(|_| "Kiwi enum value out of u32 range".to_owned())?;
                    let name = def
                        .fields
                        .iter()
                        .find(|f| f.value == val)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| val.to_string());
                    Ok(KiwiVal::Enum(name))
                }
                KiwiDefKind::Struct => {
                    if active_structs.contains(&def_idx) {
                        return Err(format!("Cyclic Kiwi struct definition '{}'", def.name));
                    }
                    active_structs.push(def_idx);
                    let mut map = BTreeMap::new();
                    for f in &def.fields {
                        let res = decode_kiwi_field_value(
                            r,
                            defs,
                            f.type_id,
                            f.is_array,
                            depth + 1,
                            budget,
                            active_structs,
                        );
                        match res {
                            Ok(v) => {
                                map.insert(f.name.clone(), v);
                            }
                            Err(e) => {
                                active_structs.pop();
                                return Err(e);
                            }
                        }
                    }
                    active_structs.pop();
                    Ok(KiwiVal::Object(map))
                }
                KiwiDefKind::Message => {
                    let mut map = BTreeMap::new();
                    loop {
                        let tag = u32::try_from(r.read_var_uint()?)
                            .map_err(|_| "Kiwi message tag out of u32 range".to_owned())?;
                        if tag == 0 {
                            break;
                        }
                        let field = def.fields.iter().find(|f| f.value == tag).ok_or_else(|| {
                            format!(
                                "Unknown tag {tag} in Kiwi message '{}' (cannot skip unknown Kiwi field)",
                                def.name
                            )
                        })?;
                        let v = decode_kiwi_field_value(
                            r,
                            defs,
                            field.type_id,
                            field.is_array,
                            depth + 1,
                            budget,
                            active_structs,
                        )?;
                        map.insert(field.name.clone(), v);
                    }
                    Ok(KiwiVal::Object(map))
                }
            }
        }
        other => Err(format!("Unsupported Kiwi scalar type {other}")),
    }
}

fn decompress_fig_chunk(chunk: &[u8]) -> Result<Vec<u8>, String> {
    if chunk.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Err(
            "Figma chunk uses Zstandard compression; export as .figma.json or standard deflate .fig."
                .into(),
        );
    }
    // Try raw DEFLATE (`fig-kiwi` standard), then zlib, then uncompressed fallback.
    let mut out = Vec::new();
    if DeflateDecoder::new(chunk)
        .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .is_ok()
        && !out.is_empty()
    {
        if out.len() > MAX_FIGMA_DECODE_BYTES {
            return Err("Decompressed Figma Kiwi chunk exceeds 128 MiB limit".into());
        }
        return Ok(out);
    }
    out.clear();
    if flate2::read::ZlibDecoder::new(chunk)
        .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .is_ok()
        && !out.is_empty()
    {
        if out.len() > MAX_FIGMA_DECODE_BYTES {
            return Err("Decompressed Figma Kiwi chunk exceeds 128 MiB limit".into());
        }
        return Ok(out);
    }
    if chunk.len() > MAX_FIGMA_DECODE_BYTES {
        return Err("Uncompressed Figma Kiwi chunk exceeds 128 MiB limit".into());
    }
    Ok(chunk.to_vec())
}

/// Decode Figma binary `commandsBlob` (`0x00` Close, `0x01` MoveTo, `0x02` LineTo, `0x03` QuadTo, `0x04` CubicTo).
fn decode_figma_commands_blob(blob: &[u8]) -> Vec<PathCommand> {
    let mut cmds = Vec::new();
    let mut pos = 0usize;
    let mut cur_x = 0.0f64;
    let mut cur_y = 0.0f64;
    let read_f32 = |b: &[u8], off: usize| -> Option<f64> {
        let s = b.get(off..off + 4)?;
        let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64;
        v.is_finite().then_some(v)
    };
    while pos < blob.len() {
        let op = blob[pos];
        pos += 1;
        match op {
            0 => cmds.push(PathCommand::Close),
            1 => {
                let (Some(x), Some(y)) = (read_f32(blob, pos), read_f32(blob, pos + 4)) else {
                    break;
                };
                pos += 8;
                cur_x = x;
                cur_y = y;
                cmds.push(PathCommand::MoveTo { x, y });
            }
            2 => {
                let (Some(x), Some(y)) = (read_f32(blob, pos), read_f32(blob, pos + 4)) else {
                    break;
                };
                pos += 8;
                cur_x = x;
                cur_y = y;
                cmds.push(PathCommand::LineTo { x, y });
            }
            3 => {
                let (Some(qx), Some(qy), Some(x), Some(y)) = (
                    read_f32(blob, pos),
                    read_f32(blob, pos + 4),
                    read_f32(blob, pos + 8),
                    read_f32(blob, pos + 12),
                ) else {
                    break;
                };
                pos += 16;
                let c1x = cur_x + (2.0 / 3.0) * (qx - cur_x);
                let c1y = cur_y + (2.0 / 3.0) * (qy - cur_y);
                let c2x = x + (2.0 / 3.0) * (qx - x);
                let c2y = y + (2.0 / 3.0) * (qy - y);
                cur_x = x;
                cur_y = y;
                cmds.push(PathCommand::CubicTo {
                    cx1: c1x,
                    cy1: c1y,
                    cx2: c2x,
                    cy2: c2y,
                    x,
                    y,
                });
            }
            4 => {
                let (Some(c1x), Some(c1y), Some(c2x), Some(c2y), Some(x), Some(y)) = (
                    read_f32(blob, pos),
                    read_f32(blob, pos + 4),
                    read_f32(blob, pos + 8),
                    read_f32(blob, pos + 12),
                    read_f32(blob, pos + 16),
                    read_f32(blob, pos + 20),
                ) else {
                    break;
                };
                pos += 24;
                cur_x = x;
                cur_y = y;
                cmds.push(PathCommand::CubicTo {
                    cx1: c1x,
                    cy1: c1y,
                    cx2: c2x,
                    cy2: c2y,
                    x,
                    y,
                });
            }
            _ => break,
        }
    }
    cmds
}

fn guid_string(obj: &BTreeMap<String, KiwiVal>) -> Option<String> {
    let s = obj.get("sessionID")?.as_u64()?;
    let l = obj.get("localID")?.as_u64()?;
    Some(format!("{s}:{l}"))
}

fn kiwi_color_to_rgba(obj: &BTreeMap<String, KiwiVal>, extra_opacity: f64) -> Color {
    let r = (obj
        .get("r")
        .and_then(KiwiVal::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let g = (obj
        .get("g")
        .and_then(KiwiVal::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let b = (obj
        .get("b")
        .and_then(KiwiVal::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let a = ((obj.get("a").and_then(KiwiVal::as_f64).unwrap_or(1.0) * extra_opacity)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    [r, g, b, a]
}

fn parse_kiwi_paint_list(arr: &[KiwiVal], w: f64, h: f64) -> Vec<FigmaPaint> {
    let mut out = Vec::new();
    for item in arr {
        let Some(p) = item.as_obj() else { continue };
        if !p.get("visible").and_then(KiwiVal::as_bool).unwrap_or(true) {
            continue;
        }
        let opacity = p.get("opacity").and_then(KiwiVal::as_f64).unwrap_or(1.0);
        let ptype = p.get("type").and_then(KiwiVal::as_str).unwrap_or("SOLID");
        match ptype {
            "SOLID" => {
                if let Some(cobj) = p.get("color").and_then(KiwiVal::as_obj) {
                    out.push(FigmaPaint::Solid {
                        color: kiwi_color_to_rgba(cobj, opacity),
                    });
                }
            }
            "GRADIENT_LINEAR" | "GRADIENT_RADIAL" => {
                let kind = if ptype == "GRADIENT_RADIAL" {
                    GradientKind::Radial
                } else {
                    GradientKind::Linear
                };
                let mut stops = Vec::new();
                if let Some(sarr) = p.get("stops").and_then(KiwiVal::as_arr) {
                    for s in sarr {
                        if let Some(sobj) = s.as_obj()
                            && let Some(cobj) = sobj.get("color").and_then(KiwiVal::as_obj)
                        {
                            let pos = sobj
                                .get("position")
                                .and_then(KiwiVal::as_f64)
                                .unwrap_or(0.0)
                                .clamp(0.0, 1.0);
                            stops.push(GradientStop {
                                ratio: (pos * 255.0).round().clamp(0.0, 255.0) as u8,
                                color: kiwi_color_to_rgba(cobj, opacity),
                            });
                        }
                    }
                }
                stops.sort_by_key(|s| s.ratio);
                if stops.len() >= 2 {
                    out.push(FigmaPaint::Gradient {
                        kind,
                        stops,
                        start: [0.0, h * 0.5],
                        end: [w.max(1.0), h * 0.5],
                    });
                }
            }
            "IMAGE" => {
                if let Some(img_obj) = p.get("image").and_then(KiwiVal::as_obj)
                    && let Some(hash_bytes) = img_obj.get("hash").and_then(KiwiVal::as_bytes)
                {
                    let hex: String = hash_bytes.iter().map(|b| format!("{b:02x}")).collect();
                    out.push(FigmaPaint::Image {
                        image_ref: hex,
                        alpha: opacity.clamp(0.0, 1.0),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_fig_kiwi_canvas(
    canvas_bytes: &[u8],
    source_name: &str,
    images: BTreeMap<String, Vec<u8>>,
) -> Result<FigmaDoc, ImportError> {
    if canvas_bytes.len() < 16
        || (!canvas_bytes.starts_with(b"fig-kiwi") && !canvas_bytes.starts_with(b"fig-jam."))
    {
        return Err(ImportError::Container(
            "Invalid .fig binary header (expected 'fig-kiwi' or 'fig-jam.')".into(),
        ));
    }
    let mut pos = 12usize; // 8-byte magic + 4-byte LE version
    let mut chunks = Vec::new();
    while pos + 4 <= canvas_bytes.len() && chunks.len() < 8 {
        let clen = u32::from_le_bytes([
            canvas_bytes[pos],
            canvas_bytes[pos + 1],
            canvas_bytes[pos + 2],
            canvas_bytes[pos + 3],
        ]) as usize;
        pos += 4;
        let end = pos
            .checked_add(clen)
            .ok_or_else(|| ImportError::Container("Chunk size overflow in .fig".into()))?;
        if end > canvas_bytes.len() {
            return Err(ImportError::Container("Truncated chunk in .fig".into()));
        }
        chunks.push(decompress_fig_chunk(&canvas_bytes[pos..end]).map_err(ImportError::Container)?);
        pos = end;
    }
    if chunks.len() < 2 {
        return Err(ImportError::Container(
            ".fig file requires at least schema and message chunks".into(),
        ));
    }

    let defs = parse_kiwi_schema(&chunks[0]).map_err(ImportError::Container)?;
    let root_def_idx = defs
        .iter()
        .position(|d| d.name == "Message")
        .unwrap_or_else(|| defs.len().saturating_sub(1));
    let mut mr = KiwiReader::new(&chunks[1]);
    let mut budget = MAX_KIWI_VALUES;
    let mut active_structs = Vec::new();
    let root_val = decode_kiwi_scalar(
        &mut mr,
        &defs,
        root_def_idx as i32,
        0,
        &mut budget,
        &mut active_structs,
    )
    .map_err(ImportError::Container)?;
    let root_obj = root_val
        .as_obj()
        .ok_or_else(|| ImportError::Container("Decoded Kiwi root is not an object".into()))?;

    let blobs: Vec<Vec<u8>> = root_obj
        .get("blobs")
        .and_then(KiwiVal::as_arr)
        .map(|arr| {
            arr.iter()
                .map(|b| {
                    b.as_obj()
                        .and_then(|o| o.get("bytes"))
                        .and_then(KiwiVal::as_bytes)
                        .map(|s| s.to_vec())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    let node_changes = root_obj
        .get("nodeChanges")
        .and_then(KiwiVal::as_arr)
        .ok_or_else(|| ImportError::Container("Kiwi Message contains no 'nodeChanges'".into()))?;
    if node_changes.len() > MAX_FIGMA_NODES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {MAX_FIGMA_NODES} node budget"
        )));
    }

    let mut nodes_by_id: HashMap<String, FigmaNode> = HashMap::new();
    let mut parent_links: Vec<(String, String, String)> = Vec::new(); // (child_id, parent_id, position_key)
    let mut doc_id: Option<String> = None;

    for nc in node_changes {
        let Some(obj) = nc.as_obj() else { continue };
        let Some(guid_obj) = obj.get("guid").and_then(KiwiVal::as_obj) else {
            continue;
        };
        let Some(id) = guid_string(guid_obj) else {
            continue;
        };
        let ntype = obj
            .get("type")
            .and_then(KiwiVal::as_str)
            .unwrap_or("GROUP")
            .to_owned();
        if ntype == "DOCUMENT" {
            doc_id = Some(id.clone());
        }

        let name = obj
            .get("name")
            .and_then(KiwiVal::as_str)
            .unwrap_or(&ntype)
            .to_owned();
        let visible = obj
            .get("visible")
            .and_then(KiwiVal::as_bool)
            .unwrap_or(true);
        let locked = obj
            .get("locked")
            .and_then(KiwiVal::as_bool)
            .unwrap_or(false);
        let opacity = obj.get("opacity").and_then(KiwiVal::as_f64).unwrap_or(1.0);
        let blend_mode = obj
            .get("blendMode")
            .and_then(KiwiVal::as_str)
            .unwrap_or("PASS_THROUGH")
            .to_owned();
        let is_mask = obj.get("mask").and_then(KiwiVal::as_bool).unwrap_or(false);
        let mask_type = obj
            .get("maskType")
            .and_then(KiwiVal::as_str)
            .unwrap_or("ALPHA")
            .to_owned();
        let clips_content = !obj
            .get("frameMaskDisabled")
            .and_then(KiwiVal::as_bool)
            .unwrap_or(true);

        let (width, height) = obj
            .get("size")
            .and_then(KiwiVal::as_obj)
            .map(|s| {
                (
                    s.get("x")
                        .and_then(KiwiVal::as_f64)
                        .unwrap_or(100.0)
                        .max(0.0),
                    s.get("y")
                        .and_then(KiwiVal::as_f64)
                        .unwrap_or(100.0)
                        .max(0.0),
                )
            })
            .unwrap_or((100.0, 100.0));

        let mut x = 0.0;
        let mut y = 0.0;
        let mut relative_transform = None;
        if let Some(m) = obj.get("transform").and_then(KiwiVal::as_obj) {
            let m00 = m.get("m00").and_then(KiwiVal::as_f64).unwrap_or(1.0);
            let m01 = m.get("m01").and_then(KiwiVal::as_f64).unwrap_or(0.0);
            let m02 = m.get("m02").and_then(KiwiVal::as_f64).unwrap_or(0.0);
            let m10 = m.get("m10").and_then(KiwiVal::as_f64).unwrap_or(0.0);
            let m11 = m.get("m11").and_then(KiwiVal::as_f64).unwrap_or(1.0);
            let m12 = m.get("m12").and_then(KiwiVal::as_f64).unwrap_or(0.0);
            x = m02;
            y = m12;
            relative_transform = Some([m00, m10, m01, m11, m02, m12]);
        }

        let r_uni = obj
            .get("cornerRadius")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(0.0);
        let corner_radii = [
            obj.get("rectangleTopLeftCornerRadius")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(r_uni),
            obj.get("rectangleTopRightCornerRadius")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(r_uni),
            obj.get("rectangleBottomRightCornerRadius")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(r_uni),
            obj.get("rectangleBottomLeftCornerRadius")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(r_uni),
        ];

        let fills = obj
            .get("fillPaints")
            .and_then(KiwiVal::as_arr)
            .map(|a| parse_kiwi_paint_list(a, width, height))
            .unwrap_or_default();
        let strokes = obj
            .get("strokePaints")
            .and_then(KiwiVal::as_arr)
            .map(|a| parse_kiwi_paint_list(a, width, height))
            .unwrap_or_default();
        let stroke_weight = obj
            .get("strokeWeight")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(0.0);

        let mut fill_paths = Vec::new();
        if let Some(fg_arr) = obj.get("fillGeometry").and_then(KiwiVal::as_arr) {
            for g in fg_arr {
                if let Some(go) = g.as_obj()
                    && let Some(bidx) = go.get("commandsBlob").and_then(KiwiVal::as_u64)
                    && let Some(blob) = blobs.get(bidx as usize)
                {
                    let cmds = decode_figma_commands_blob(blob);
                    if !cmds.is_empty() {
                        let rule = match go.get("windingRule").and_then(KiwiVal::as_str) {
                            Some("EVENODD") | Some("ODD") => FillRule::EvenOdd,
                            _ => FillRule::NonZero,
                        };
                        fill_paths.push((cmds, rule));
                    }
                }
            }
        }
        let mut stroke_paths = Vec::new();
        if let Some(sg_arr) = obj.get("strokeGeometry").and_then(KiwiVal::as_arr) {
            for g in sg_arr {
                if let Some(go) = g.as_obj()
                    && let Some(bidx) = go.get("commandsBlob").and_then(KiwiVal::as_u64)
                    && let Some(blob) = blobs.get(bidx as usize)
                {
                    let cmds = decode_figma_commands_blob(blob);
                    if !cmds.is_empty() {
                        stroke_paths.push(cmds);
                    }
                }
            }
        }

        let text = obj
            .get("textData")
            .and_then(KiwiVal::as_obj)
            .and_then(|td| {
                let chars = td.get("characters").and_then(KiwiVal::as_str)?.to_owned();
                let font_size = obj
                    .get("fontSize")
                    .and_then(KiwiVal::as_f64)
                    .unwrap_or(16.0);
                let (font_family, style_str) = obj
                    .get("fontName")
                    .and_then(KiwiVal::as_obj)
                    .map(|fn_obj| {
                        (
                            fn_obj
                                .get("family")
                                .and_then(KiwiVal::as_str)
                                .unwrap_or("Inter")
                                .to_owned(),
                            fn_obj
                                .get("style")
                                .and_then(KiwiVal::as_str)
                                .unwrap_or("Regular")
                                .to_ascii_lowercase(),
                        )
                    })
                    .unwrap_or_else(|| ("Inter".into(), "regular".into()));
                let align = match obj.get("textAlignHorizontal").and_then(KiwiVal::as_str) {
                    Some("CENTER") => TextAlign::Center,
                    Some("RIGHT") => TextAlign::Right,
                    _ => TextAlign::Left,
                };
                Some(FigmaTextData {
                    characters: chars,
                    font_family,
                    font_size,
                    bold: style_str.contains("bold") || style_str.contains("black"),
                    italic: style_str.contains("italic") || style_str.contains("oblique"),
                    align,
                    letter_spacing: 0.0,
                    line_height: None,
                })
            });

        let component_id = obj
            .get("symbolData")
            .and_then(KiwiVal::as_obj)
            .and_then(|sd| sd.get("symbolID"))
            .and_then(KiwiVal::as_obj)
            .and_then(guid_string);

        let background_color = obj
            .get("backgroundColor")
            .and_then(KiwiVal::as_obj)
            .map(|c| kiwi_color_to_rgba(c, 1.0));

        if let Some(pidx) = obj.get("parentIndex").and_then(KiwiVal::as_obj)
            && let Some(pguid) = pidx.get("guid").and_then(KiwiVal::as_obj)
            && let Some(pid) = guid_string(pguid)
        {
            let pos_key = pidx
                .get("position")
                .and_then(KiwiVal::as_str)
                .unwrap_or("")
                .to_owned();
            parent_links.push((id.clone(), pid, pos_key));
        }

        nodes_by_id.insert(
            id.clone(),
            FigmaNode {
                id,
                name,
                node_type: ntype,
                visible,
                locked,
                opacity,
                blend_mode,
                is_mask,
                mask_type,
                clips_content,
                x,
                y,
                width,
                height,
                rotation_deg: 0.0,
                relative_transform,
                corner_radii,
                arc_data: None,
                point_count: obj.get("count").and_then(KiwiVal::as_u64).map(|v| v as u32),
                star_inner_radius: obj.get("starInnerScale").and_then(KiwiVal::as_f64),
                fills,
                strokes,
                stroke_weight,
                fill_paths,
                stroke_paths,
                text,
                effects: Vec::new(),
                component_id,
                background_color,
                children: Vec::new(),
            },
        );
    }

    let mut children_map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (cid, pid, pos_key) in parent_links {
        children_map.entry(pid).or_default().push((pos_key, cid));
    }
    for list in children_map.values_mut() {
        list.sort_by(|a, b| a.0.cmp(&b.0));
    }

    fn assemble_kiwi_tree(
        id: &str,
        nodes: &mut HashMap<String, FigmaNode>,
        children_map: &HashMap<String, Vec<(String, String)>>,
        depth: usize,
    ) -> Option<FigmaNode> {
        if depth > 64 {
            return None;
        }
        let mut node = nodes.remove(id)?;
        if let Some(ch_list) = children_map.get(id) {
            for (_, cid) in ch_list {
                if let Some(child) = assemble_kiwi_tree(cid, nodes, children_map, depth + 1) {
                    node.children.push(child);
                }
            }
        }
        Some(node)
    }

    let mut pages = Vec::new();
    if let Some(did) = doc_id
        && let Some(doc_node) = assemble_kiwi_tree(&did, &mut nodes_by_id, &children_map, 0)
    {
        pages = doc_node
            .children
            .into_iter()
            .filter(|n| n.node_type != "INTERNAL_ONLY_CANVAS")
            .collect();
    }
    if pages.is_empty() {
        // Fallback: collect remaining CANVAS nodes directly
        let canvas_ids: Vec<String> = nodes_by_id
            .iter()
            .filter(|(_, n)| n.node_type == "CANVAS")
            .map(|(k, _)| k.clone())
            .collect();
        for cid in canvas_ids {
            if let Some(cnode) = assemble_kiwi_tree(&cid, &mut nodes_by_id, &children_map, 0) {
                pages.push(cnode);
            }
        }
    }

    Ok(FigmaDoc {
        name: source_name.to_owned(),
        pages,
        images,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. `.figma.json` REST API & Plugin JSON Parser
// ─────────────────────────────────────────────────────────────────────────────

fn json_color_to_rgba(c: &Value, extra_opacity: f64) -> Color {
    let r = (c
        .get("r")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let g = (c
        .get("g")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let b = (c
        .get("b")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    let a = ((c.get("a").and_then(Value::as_f64).unwrap_or(1.0) * extra_opacity).clamp(0.0, 1.0)
        * 255.0)
        .round() as u8;
    [r, g, b, a]
}

fn parse_json_paints(arr: &[Value], w: f64, h: f64) -> Vec<FigmaPaint> {
    let mut out = Vec::new();
    for p in arr {
        if !p.get("visible").and_then(Value::as_bool).unwrap_or(true) {
            continue;
        }
        let opacity = p.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
        let ptype = p.get("type").and_then(Value::as_str).unwrap_or("SOLID");
        match ptype {
            "SOLID" => {
                if let Some(c) = p.get("color") {
                    out.push(FigmaPaint::Solid {
                        color: json_color_to_rgba(c, opacity),
                    });
                }
            }
            "GRADIENT_LINEAR" | "GRADIENT_RADIAL" => {
                let kind = if ptype == "GRADIENT_RADIAL" {
                    GradientKind::Radial
                } else {
                    GradientKind::Linear
                };
                let mut stops = Vec::new();
                if let Some(sarr) = p.get("gradientStops").and_then(Value::as_array) {
                    for s in sarr {
                        if let Some(c) = s.get("color") {
                            let pos = s
                                .get("position")
                                .and_then(Value::as_f64)
                                .unwrap_or(0.0)
                                .clamp(0.0, 1.0);
                            stops.push(GradientStop {
                                ratio: (pos * 255.0).round().clamp(0.0, 255.0) as u8,
                                color: json_color_to_rgba(c, opacity),
                            });
                        }
                    }
                }
                stops.sort_by_key(|s| s.ratio);
                let (mut start, mut end) = ([0.0, h * 0.5], [w.max(1.0), h * 0.5]);
                if let Some(handles) = p.get("gradientHandlePositions").and_then(Value::as_array)
                    && handles.len() >= 2
                {
                    let h0x = handles[0].get("x").and_then(Value::as_f64).unwrap_or(0.0);
                    let h0y = handles[0].get("y").and_then(Value::as_f64).unwrap_or(0.5);
                    let h1x = handles[1].get("x").and_then(Value::as_f64).unwrap_or(1.0);
                    let h1y = handles[1].get("y").and_then(Value::as_f64).unwrap_or(0.5);
                    start = [h0x * w.max(1.0), h0y * h.max(1.0)];
                    end = [h1x * w.max(1.0), h1y * h.max(1.0)];
                }
                if stops.len() >= 2 {
                    out.push(FigmaPaint::Gradient {
                        kind,
                        stops,
                        start,
                        end,
                    });
                }
            }
            "IMAGE" => {
                if let Some(iref) = p
                    .get("imageRef")
                    .or_else(|| p.get("imageHash"))
                    .and_then(Value::as_str)
                {
                    out.push(FigmaPaint::Image {
                        image_ref: iref.to_owned(),
                        alpha: opacity.clamp(0.0, 1.0),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn decode_vector_network_json(vn: &Value) -> Vec<(Vec<PathCommand>, FillRule)> {
    let Some(verts) = vn.get("vertices").and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(segs) = vn.get("segments").and_then(Value::as_array) else {
        return Vec::new();
    };
    let vcoords: Vec<[f64; 2]> = verts
        .iter()
        .map(|v| {
            [
                v.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                v.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            ]
        })
        .collect();

    let append_seg = |cmds: &mut Vec<PathCommand>, seg_idx: usize, forward: bool| -> Option<()> {
        let seg = segs.get(seg_idx)?;
        let s_i = seg.get("start")?.as_u64()? as usize;
        let e_i = seg.get("end")?.as_u64()? as usize;
        let (p0, p1) = if forward {
            (*vcoords.get(s_i)?, *vcoords.get(e_i)?)
        } else {
            (*vcoords.get(e_i)?, *vcoords.get(s_i)?)
        };
        let ts = seg.get("tangentStart");
        let te = seg.get("tangentEnd");
        let t0 = [
            ts.and_then(|t| t.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            ts.and_then(|t| t.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ];
        let t1 = [
            te.and_then(|t| t.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            te.and_then(|t| t.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
        ];
        let (tan_a, tan_b) = if forward { (t0, t1) } else { (t1, t0) };
        if tan_a[0].abs() < 1e-9
            && tan_a[1].abs() < 1e-9
            && tan_b[0].abs() < 1e-9
            && tan_b[1].abs() < 1e-9
        {
            cmds.push(PathCommand::LineTo { x: p1[0], y: p1[1] });
        } else {
            cmds.push(PathCommand::CubicTo {
                cx1: p0[0] + tan_a[0],
                cy1: p0[1] + tan_a[1],
                cx2: p1[0] + tan_b[0],
                cy2: p1[1] + tan_b[1],
                x: p1[0],
                y: p1[1],
            });
        }
        Some(())
    };

    if let Some(regions) = vn.get("regions").and_then(Value::as_array)
        && !regions.is_empty()
    {
        let mut out = Vec::new();
        for reg in regions {
            let rule = match reg.get("windingRule").and_then(Value::as_str) {
                Some("EVENODD") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            let mut cmds = Vec::new();
            if let Some(loops) = reg.get("loops").and_then(Value::as_array) {
                for lp in loops {
                    let Some(sidxs) = lp.as_array() else { continue };
                    let mut cur_v: Option<usize> = None;
                    for (k, sv) in sidxs.iter().enumerate() {
                        let Some(si) = sv.as_u64().map(|u| u as usize) else {
                            continue;
                        };
                        let Some(seg) = segs.get(si) else { continue };
                        let s_v = seg.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let e_v = seg.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let forward = match cur_v {
                            Some(cv) => cv == s_v,
                            None => true,
                        };
                        if k == 0 {
                            let start_pt = if forward {
                                vcoords.get(s_v)
                            } else {
                                vcoords.get(e_v)
                            };
                            if let Some(&[sx, sy]) = start_pt {
                                cmds.push(PathCommand::MoveTo { x: sx, y: sy });
                            }
                        }
                        append_seg(&mut cmds, si, forward);
                        cur_v = Some(if forward { e_v } else { s_v });
                    }
                    if !cmds.is_empty() {
                        cmds.push(PathCommand::Close);
                    }
                }
            }
            if !cmds.is_empty() {
                out.push((cmds, rule));
            }
        }
        return out;
    }

    // Open stroke-only vector network
    let mut cmds = Vec::new();
    let mut last_v: Option<usize> = None;
    for i in 0..segs.len() {
        let Some(seg) = segs.get(i) else { continue };
        let s_v = seg.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let e_v = seg.get("end").and_then(Value::as_u64).unwrap_or(0) as usize;
        if last_v != Some(s_v)
            && let Some(&[sx, sy]) = vcoords.get(s_v)
        {
            cmds.push(PathCommand::MoveTo { x: sx, y: sy });
        }
        append_seg(&mut cmds, i, true);
        last_v = Some(e_v);
    }
    if cmds.is_empty() {
        Vec::new()
    } else {
        vec![(cmds, FillRule::NonZero)]
    }
}

fn parse_figma_json_node(
    v: &Value,
    parent_abs: Option<(f64, f64)>,
    node_count: &mut usize,
    depth: usize,
) -> Result<FigmaNode, ImportError> {
    if depth > 64 {
        return Err(ImportError::Container(
            "Figma JSON node hierarchy exceeds depth 64".into(),
        ));
    }
    *node_count += 1;
    if *node_count > MAX_FIGMA_NODES {
        return Err(ImportError::Container(format!(
            "Figma JSON exceeds {MAX_FIGMA_NODES} node budget"
        )));
    }

    let node_type = v
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("GROUP")
        .to_owned();
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("0:0")
        .to_owned();
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&node_type)
        .to_owned();
    let visible = v.get("visible").and_then(Value::as_bool).unwrap_or(true);
    let locked = v.get("locked").and_then(Value::as_bool).unwrap_or(false);
    let opacity = v.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let blend_mode = v
        .get("blendMode")
        .and_then(Value::as_str)
        .unwrap_or("PASS_THROUGH")
        .to_owned();
    let is_mask = v.get("isMask").and_then(Value::as_bool).unwrap_or(false);
    let mask_type = v
        .get("maskType")
        .and_then(Value::as_str)
        .unwrap_or("ALPHA")
        .to_owned();
    let clips_content = v
        .get("clipsContent")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let bbox = v
        .get("absoluteBoundingBox")
        .or_else(|| v.get("boundingBox"));
    let abs_x = bbox.and_then(|b| b.get("x")).and_then(Value::as_f64);
    let abs_y = bbox.and_then(|b| b.get("y")).and_then(Value::as_f64);
    let width = v
        .get("size")
        .and_then(|s| s.get("x"))
        .and_then(Value::as_f64)
        .or_else(|| bbox.and_then(|b| b.get("width")).and_then(Value::as_f64))
        .unwrap_or(100.0)
        .max(0.0);
    let height = v
        .get("size")
        .and_then(|s| s.get("y"))
        .and_then(Value::as_f64)
        .or_else(|| bbox.and_then(|b| b.get("height")).and_then(Value::as_f64))
        .unwrap_or(100.0)
        .max(0.0);

    let mut relative_transform = None;
    if let Some(rows) = v.get("relativeTransform").and_then(Value::as_array)
        && rows.len() == 2
        && let (Some(r0), Some(r1)) = (rows[0].as_array(), rows[1].as_array())
        && r0.len() >= 3
        && r1.len() >= 3
    {
        let m00 = r0[0].as_f64().unwrap_or(1.0);
        let m01 = r0[1].as_f64().unwrap_or(0.0);
        let m02 = r0[2].as_f64().unwrap_or(0.0);
        let m10 = r1[0].as_f64().unwrap_or(0.0);
        let m11 = r1[1].as_f64().unwrap_or(1.0);
        let m12 = r1[2].as_f64().unwrap_or(0.0);
        relative_transform = Some([m00, m10, m01, m11, m02, m12]);
    }

    let (x, y) = if let Some(rt) = relative_transform {
        (rt[4], rt[5])
    } else if let (Some(ax), Some(ay)) = (abs_x, abs_y) {
        if let Some((px, py)) = parent_abs {
            (ax - px, ay - py)
        } else {
            (ax, ay)
        }
    } else {
        (
            v.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            v.get("y").and_then(Value::as_f64).unwrap_or(0.0),
        )
    };

    let rotation_deg = v
        .get("rotation")
        .and_then(Value::as_f64)
        .map(|r| -r) // Figma REST rotation is counter-clockwise degrees when relativeTransform is absent
        .unwrap_or(0.0);

    let r_uni = v.get("cornerRadius").and_then(Value::as_f64).unwrap_or(0.0);
    let corner_radii = if let Some(arr) = v.get("rectangleCornerRadii").and_then(Value::as_array)
        && arr.len() >= 4
    {
        [
            arr[0].as_f64().unwrap_or(r_uni),
            arr[1].as_f64().unwrap_or(r_uni),
            arr[2].as_f64().unwrap_or(r_uni),
            arr[3].as_f64().unwrap_or(r_uni),
        ]
    } else {
        [r_uni; 4]
    };

    let arc_data = v.get("arcData").and_then(|ad| {
        let s = ad.get("startingAngle")?.as_f64()?;
        let e = ad.get("endingAngle")?.as_f64()?;
        let inner = ad.get("innerRadius").and_then(Value::as_f64).unwrap_or(0.0);
        Some((s, e, inner))
    });

    let fills = v
        .get("fills")
        .and_then(Value::as_array)
        .map(|a| parse_json_paints(a, width, height))
        .unwrap_or_default();
    let strokes = v
        .get("strokes")
        .and_then(Value::as_array)
        .map(|a| parse_json_paints(a, width, height))
        .unwrap_or_default();
    let stroke_weight = v.get("strokeWeight").and_then(Value::as_f64).unwrap_or(0.0);

    let mut fill_paths = Vec::new();
    if let Some(fg) = v.get("fillGeometry").and_then(Value::as_array) {
        for g in fg {
            if let Some(d) = g.get("path").and_then(Value::as_str) {
                let cmds = parse_svg_path_data(d);
                if !cmds.is_empty() {
                    let rule = match g.get("windingRule").and_then(Value::as_str) {
                        Some("EVENODD") => FillRule::EvenOdd,
                        _ => FillRule::NonZero,
                    };
                    fill_paths.push((cmds, rule));
                }
            }
        }
    }
    let mut stroke_paths = Vec::new();
    if let Some(sg) = v.get("strokeGeometry").and_then(Value::as_array) {
        for g in sg {
            if let Some(d) = g.get("path").and_then(Value::as_str) {
                let cmds = parse_svg_path_data(d);
                if !cmds.is_empty() {
                    stroke_paths.push(cmds);
                }
            }
        }
    }
    if fill_paths.is_empty()
        && stroke_paths.is_empty()
        && let Some(vn) = v.get("vectorNetwork")
    {
        let decoded = decode_vector_network_json(vn);
        if !fills.is_empty() {
            fill_paths = decoded;
        } else {
            stroke_paths = decoded.into_iter().map(|(c, _)| c).collect();
        }
    }

    let text = if node_type == "TEXT" {
        let chars = v
            .get("characters")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let style = v.get("style");
        let font_family = style
            .and_then(|s| s.get("fontFamily"))
            .and_then(Value::as_str)
            .unwrap_or("Inter")
            .to_owned();
        let font_size = style
            .and_then(|s| s.get("fontSize"))
            .and_then(Value::as_f64)
            .unwrap_or(16.0);
        let weight = style
            .and_then(|s| s.get("fontWeight"))
            .and_then(Value::as_u64)
            .unwrap_or(400);
        let italic = style
            .and_then(|s| s.get("italic"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let align = match style
            .and_then(|s| s.get("textAlignHorizontal"))
            .and_then(Value::as_str)
        {
            Some("CENTER") => TextAlign::Center,
            Some("RIGHT") => TextAlign::Right,
            _ => TextAlign::Left,
        };
        let letter_spacing = style
            .and_then(|s| s.get("letterSpacing"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let line_height = style
            .and_then(|s| s.get("lineHeightPx"))
            .and_then(Value::as_f64);
        Some(FigmaTextData {
            characters: chars,
            font_family,
            font_size,
            bold: weight >= 600,
            italic,
            align,
            letter_spacing,
            line_height,
        })
    } else {
        None
    };

    let mut effects = Vec::new();
    if let Some(earr) = v.get("effects").and_then(Value::as_array) {
        for eff in earr {
            if !eff.get("visible").and_then(Value::as_bool).unwrap_or(true) {
                continue;
            }
            let etype = eff.get("type").and_then(Value::as_str).unwrap_or("");
            let radius = eff.get("radius").and_then(Value::as_f64).unwrap_or(4.0);
            match etype {
                "DROP_SHADOW" | "INNER_SHADOW" => {
                    let color = eff
                        .get("color")
                        .map(|c| json_color_to_rgba(c, 1.0))
                        .unwrap_or([0, 0, 0, 128]);
                    let ox = eff
                        .get("offset")
                        .and_then(|o| o.get("x"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let oy = eff
                        .get("offset")
                        .and_then(|o| o.get("y"))
                        .and_then(Value::as_f64)
                        .unwrap_or(4.0);
                    effects.push(FigmaEffect::DropShadow {
                        color,
                        offset_x: ox,
                        offset_y: oy,
                        radius,
                        inner: etype == "INNER_SHADOW",
                    });
                }
                "LAYER_BLUR" => effects.push(FigmaEffect::LayerBlur { radius }),
                "BACKGROUND_BLUR" => effects.push(FigmaEffect::BackgroundBlur { radius }),
                _ => {}
            }
        }
    }

    let child_parent_abs = match (abs_x, abs_y) {
        (Some(ax), Some(ay)) => Some((ax, ay)),
        _ => parent_abs,
    };

    let mut children = Vec::new();
    if let Some(carr) = v.get("children").and_then(Value::as_array) {
        for c in carr {
            children.push(parse_figma_json_node(
                c,
                child_parent_abs,
                node_count,
                depth + 1,
            )?);
        }
    }

    let background_color = v.get("backgroundColor").map(|c| json_color_to_rgba(c, 1.0));

    Ok(FigmaNode {
        id,
        name,
        node_type,
        visible,
        locked,
        opacity,
        blend_mode,
        is_mask,
        mask_type,
        clips_content,
        x,
        y,
        width,
        height,
        rotation_deg,
        relative_transform,
        corner_radii,
        arc_data,
        point_count: v
            .get("pointCount")
            .or_else(|| v.get("count"))
            .and_then(Value::as_u64)
            .map(|u| u as u32),
        star_inner_radius: v.get("innerRadius").and_then(Value::as_f64),
        fills,
        strokes,
        stroke_weight,
        fill_paths,
        stroke_paths,
        text,
        effects,
        component_id: v
            .get("componentId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        background_color,
        children,
    })
}

fn parse_figma_json_doc(json_str: &str, source_name: &str) -> Result<FigmaDoc, ImportError> {
    let root: Value = serde_json::from_str(json_str)
        .map_err(|e| ImportError::Container(format!("Invalid Figma JSON: {e}")))?;

    let doc_name = root
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(source_name)
        .to_owned();

    let mut images = BTreeMap::new();
    let mut total_image_bytes = 0usize;
    for key in ["images", "imageFills"] {
        if let Some(map) = root.get(key).and_then(Value::as_object) {
            for (iref, val) in map {
                if let Some(s) = val.as_str() {
                    let b64 = s
                        .split_once(";base64,")
                        .map(|(_, tail)| tail)
                        .unwrap_or(s)
                        .trim();
                    if b64.len() > neoflash_core::MAX_RESOURCE_BYTES * 2 {
                        return Err(ImportError::Container(format!(
                            "Figma JSON image '{iref}' exceeds resource size budget"
                        )));
                    }
                    if let Ok(decoded) = BASE64.decode(b64.as_bytes())
                        && !decoded.is_empty()
                    {
                        if decoded.len() > neoflash_core::MAX_RESOURCE_BYTES {
                            return Err(ImportError::Container(format!(
                                "Figma JSON image '{iref}' exceeds resource size budget"
                            )));
                        }
                        total_image_bytes = total_image_bytes.saturating_add(decoded.len());
                        if total_image_bytes > MAX_FIGMA_IMAGE_TOTAL_BYTES {
                            return Err(ImportError::Container(
                                "Figma JSON embedded images exceed 64 MiB aggregate budget".into(),
                            ));
                        }
                        images.insert(iref.clone(), decoded);
                    }
                }
            }
        }
    }

    let mut node_count = 0usize;
    let mut pages = Vec::new();
    if let Some(doc) = root.get("document") {
        let parsed = parse_figma_json_node(doc, None, &mut node_count, 0)?;
        if parsed.node_type == "DOCUMENT" {
            pages = parsed.children;
        } else {
            pages.push(parsed);
        }
    } else if let Some(nodes_map) = root.get("nodes").and_then(Value::as_object) {
        for entry in nodes_map.values() {
            let target = entry.get("document").unwrap_or(entry);
            let parsed = parse_figma_json_node(target, None, &mut node_count, 0)?;
            if parsed.node_type == "DOCUMENT" {
                pages.extend(parsed.children);
            } else if parsed.node_type == "CANVAS" {
                pages.push(parsed);
            } else {
                let mut page = FigmaNode {
                    id: "page_0".into(),
                    name: "Page 1".into(),
                    node_type: "CANVAS".into(),
                    ..FigmaNode::default()
                };
                page.children.push(parsed);
                pages.push(page);
            }
        }
    } else {
        let parsed = parse_figma_json_node(&root, None, &mut node_count, 0)?;
        if parsed.node_type == "DOCUMENT" {
            pages = parsed.children;
        } else if parsed.node_type == "CANVAS" {
            pages.push(parsed);
        } else {
            let mut page = FigmaNode {
                id: "page_0".into(),
                name: "Page 1".into(),
                node_type: "CANVAS".into(),
                ..FigmaNode::default()
            };
            page.children.push(parsed);
            pages.push(page);
        }
    }

    Ok(FigmaDoc {
        name: doc_name,
        pages,
        images,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Lowering `FigmaDoc` into Layered NeoFlash Compositions & Symbols
// ─────────────────────────────────────────────────────────────────────────────

struct FigmaBuildCtx<'a> {
    options: &'a FigmaImportOptions,
    images: &'a BTreeMap<String, Vec<u8>>,
    image_resources: HashMap<String, (String, u32, u32)>,
    component_symbols: HashMap<String, String>, // figma component id -> NeoFlash symbol id
    project: &'a mut Project,
    report: &'a mut ImportReport,
}

impl<'a> FigmaBuildCtx<'a> {
    fn map_blend_mode(&mut self, raw: &str, location: &str) -> BlendMode {
        if !self.options.preserve_blend_modes {
            return BlendMode::Normal;
        }
        match raw {
            "" | "PASS_THROUGH" | "NORMAL" => BlendMode::Normal,
            "MULTIPLY" | "LINEAR_BURN" => BlendMode::Multiply,
            "SCREEN" => BlendMode::Screen,
            "LIGHTEN" => BlendMode::Lighten,
            "DARKEN" => BlendMode::Darken,
            "DIFFERENCE" | "EXCLUSION" => BlendMode::Difference,
            "LINEAR_DODGE" | "ADD" | "COLOR_DODGE" => BlendMode::Add,
            "OVERLAY" | "SOFT_LIGHT" => BlendMode::Overlay,
            "HARD_LIGHT" => BlendMode::Hardlight,
            other => {
                self.report.warning(
                    "figma-blend-approximated",
                    location.to_owned(),
                    format!("Figma blend mode '{other}' approximated to Normal."),
                );
                BlendMode::Normal
            }
        }
    }

    fn map_effects(&mut self, effects: &[FigmaEffect], location: &str) -> Vec<FilterSpec> {
        let mut out = Vec::new();
        for eff in effects {
            match eff {
                FigmaEffect::DropShadow {
                    color,
                    offset_x,
                    offset_y,
                    radius,
                    inner,
                } => {
                    let distance = offset_x.hypot(*offset_y);
                    let angle = offset_y.atan2(*offset_x).to_degrees();
                    let rgb = (u32::from(color[0]) << 16)
                        | (u32::from(color[1]) << 8)
                        | u32::from(color[2]);
                    let alpha = (f64::from(color[3]) / 255.0).clamp(0.0, 1.0);
                    out.push(FilterSpec::DropShadow {
                        distance: distance.clamp(0.0, 255.0),
                        angle,
                        color: rgb,
                        alpha,
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        strength: 1.0,
                        quality: 2,
                        inner: *inner,
                        knockout: false,
                        hide_object: false,
                    });
                }
                FigmaEffect::LayerBlur { radius } => {
                    out.push(FilterSpec::Blur {
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        quality: 2,
                    });
                }
                FigmaEffect::BackgroundBlur { radius } => {
                    self.report.warning(
                        "figma-background-blur-approximated",
                        location.to_owned(),
                        format!(
                            "Figma BACKGROUND_BLUR ({radius:.1}px) approximated as layer Blur filter."
                        ),
                    );
                    out.push(FilterSpec::Blur {
                        blur_x: (*radius).clamp(0.0, 255.0),
                        blur_y: (*radius).clamp(0.0, 255.0),
                        quality: 2,
                    });
                }
            }
        }
        out.truncate(neoflash_core::MAX_OBJECT_FILTERS);
        out
    }

    fn ensure_image_resource(
        &mut self,
        image_ref: &str,
        location: &str,
    ) -> Option<(String, u32, u32)> {
        if let Some(cached) = self.image_resources.get(image_ref) {
            return Some(cached.clone());
        }
        let Some(bytes) = self.images.get(image_ref) else {
            self.report.warning(
                "figma-missing-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' has no embedded bytes; converted to placeholder shape."),
            );
            return None;
        };
        let Ok(fmt) = image::guess_format(bytes) else {
            self.report.warning(
                "figma-corrupt-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' has unrecognized image format; converted to placeholder shape."),
            );
            return None;
        };
        let (ext, mime) = match fmt {
            image::ImageFormat::Png => ("png", "image/png"),
            image::ImageFormat::Jpeg => ("jpg", "image/jpeg"),
            image::ImageFormat::Gif => ("gif", "image/gif"),
            image::ImageFormat::WebP => ("webp", "image/webp"),
            image::ImageFormat::Bmp => ("bmp", "image/bmp"),
            image::ImageFormat::Tiff => ("tiff", "image/tiff"),
            _ => ("png", "image/png"),
        };
        let res_id = new_id("figimg");
        let short_ref: String = image_ref
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect();
        let rel_path = format!(
            "figma/images/{}_{}.{}",
            if short_ref.is_empty() {
                "img"
            } else {
                &short_ref
            },
            self.project.resources.len(),
            ext
        );
        let Ok(unit) = ResourceUnit::from_bytes(res_id.clone(), rel_path, mime, bytes) else {
            self.report.warning(
                "figma-corrupt-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' exceeded resource limits; converted to placeholder shape."),
            );
            return None;
        };
        let Ok(decoded) = unit.decode_image() else {
            self.report.warning(
                "figma-corrupt-image-blob",
                location.to_owned(),
                format!("Figma image fill '{image_ref}' could not be decoded; converted to placeholder shape."),
            );
            return None;
        };
        let (w, h) = (decoded.width, decoded.height);
        self.project.resources.push(unit);
        self.image_resources
            .insert(image_ref.to_owned(), (res_id.clone(), w, h));
        Some((res_id, w, h))
    }

    fn node_transform(&self, node: &FigmaNode, offset_x: f64, offset_y: f64) -> Transform {
        let mut tf = if let Some(mut mat) = node.relative_transform {
            mat[4] -= offset_x;
            mat[5] -= offset_y;
            Transform::from_matrix(mat).unwrap_or_else(|| Transform {
                x: node.x - offset_x,
                y: node.y - offset_y,
                ..Transform::default()
            })
        } else {
            Transform {
                x: node.x - offset_x,
                y: node.y - offset_y,
                rotation: node.rotation_deg,
                ..Transform::default()
            }
        };
        let finite_or = |v: f64, def: f64| if v.is_finite() { v } else { def };
        tf.x = finite_or(tf.x, 0.0).clamp(-100_000.0, 100_000.0);
        tf.y = finite_or(tf.y, 0.0).clamp(-100_000.0, 100_000.0);
        tf.scale_x = finite_or(tf.scale_x, 1.0).clamp(-1000.0, 1000.0);
        if tf.scale_x.abs() < 1e-4 {
            tf.scale_x = if tf.scale_x.is_sign_negative() {
                -1e-4
            } else {
                1e-4
            };
        }
        tf.scale_y = finite_or(tf.scale_y, 1.0).clamp(-1000.0, 1000.0);
        if tf.scale_y.abs() < 1e-4 {
            tf.scale_y = if tf.scale_y.is_sign_negative() {
                -1e-4
            } else {
                1e-4
            };
        }
        tf.rotation = finite_or(tf.rotation, 0.0).rem_euclid(360.0);
        tf.skew = finite_or(tf.skew, 0.0).clamp(-89.9, 89.9);
        tf.alpha = finite_or(node.opacity, 1.0).clamp(0.0, 1.0);
        tf
    }

    fn fallback_shape_commands(
        &self,
        node: &FigmaNode,
    ) -> (Vec<PathCommand>, Option<PrimitiveOptions>) {
        let w = (if node.width.is_finite() {
            node.width
        } else {
            100.0
        })
        .clamp(1.0, 32_768.0);
        let h = (if node.height.is_finite() {
            node.height
        } else {
            100.0
        })
        .clamp(1.0, 32_768.0);
        match node.node_type.as_str() {
            "ELLIPSE" => {
                if let Some((start_rad, end_rad, inner_ratio)) = node.arc_data {
                    let s_deg = if start_rad.is_finite() {
                        start_rad.to_degrees().rem_euclid(360.0)
                    } else {
                        0.0
                    };
                    let span_rad = (end_rad - start_rad).abs();
                    let e_deg = if !end_rad.is_finite() || span_rad >= std::f64::consts::TAU - 1e-4
                    {
                        360.0
                    } else {
                        end_rad.to_degrees().rem_euclid(360.0)
                    };
                    let oval_opts = OvalOptions {
                        start_angle: s_deg,
                        end_angle: e_deg,
                        inner_radius: (if inner_ratio.is_finite() {
                            inner_ratio * 100.0
                        } else {
                            0.0
                        })
                        .clamp(0.0, 99.0),
                        close_path: true,
                    };
                    (
                        oval_path_commands(w, h, oval_opts),
                        (!oval_opts.is_default()).then_some(PrimitiveOptions::Oval(oval_opts)),
                    )
                } else {
                    (oval_path_commands(w, h, OvalOptions::default()), None)
                }
            }
            "STAR" => {
                let pts = node.point_count.unwrap_or(5).clamp(3, 32);
                let inner = node
                    .star_inner_radius
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.382)
                    .clamp(0.05, 0.95);
                let (cmds, _) = polystar_path_commands(
                    [w * 0.5, h * 0.5],
                    w.min(h) * 0.5,
                    -std::f64::consts::FRAC_PI_2,
                    pts,
                    true,
                    inner,
                );
                (cmds, None)
            }
            "REGULAR_POLYGON" => {
                let pts = node.point_count.unwrap_or(5).clamp(3, 32);
                let (cmds, _) = polystar_path_commands(
                    [w * 0.5, h * 0.5],
                    w.min(h) * 0.5,
                    -std::f64::consts::FRAC_PI_2,
                    pts,
                    false,
                    0.5,
                );
                (cmds, None)
            }
            "LINE" => (
                vec![
                    PathCommand::MoveTo { x: 0.0, y: 0.0 },
                    PathCommand::LineTo { x: w, y: 0.0 },
                ],
                None,
            ),
            _ => {
                let max_r = 0.5 * w.min(h);
                let r = node.corner_radii.map(|v| {
                    if v.is_finite() {
                        v.clamp(0.0, max_r)
                    } else {
                        0.0
                    }
                });
                if r.iter().any(|&v| v > 0.0) {
                    let rect_opts = RectangleOptions {
                        corner_radii: r,
                        corners_locked: (r[0] - r[1]).abs() < 1e-6
                            && (r[0] - r[2]).abs() < 1e-6
                            && (r[0] - r[3]).abs() < 1e-6,
                    };
                    (
                        rectangle_path_commands(w, h, rect_opts),
                        (!rect_opts.is_default()).then_some(PrimitiveOptions::Rectangle(rect_opts)),
                    )
                } else {
                    (
                        rectangle_path_commands(w, h, RectangleOptions::default()),
                        None,
                    )
                }
            }
        }
    }

    fn collect_component_nodes<'n>(
        &mut self,
        node: &'n FigmaNode,
        out: &mut Vec<&'n FigmaNode>,
        depth: usize,
    ) {
        if depth > 48 {
            return;
        }
        if node.node_type == "COMPONENT" && !self.component_symbols.contains_key(&node.id) {
            let sym_id = new_id("figsym");
            self.component_symbols
                .insert(node.id.clone(), sym_id.clone());
            self.project.symbols.push(Symbol {
                id: sym_id,
                name: node.name.clone(),
                folder: "Figma Components".into(),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 1,
                    layers: vec![Layer {
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties::default(),
                    }],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
                class_name: None,
                linkage_id: None,
            });
            out.push(node);
        }
        for child in &node.children {
            self.collect_component_nodes(child, out, depth + 1);
        }
    }

    fn populate_component_symbols(&mut self, components: &[&FigmaNode]) {
        for comp in components {
            let Some(sym_id) = self.component_symbols.get(&comp.id).cloned() else {
                continue;
            };
            let layers = self.build_layers_for_children(
                &comp.children,
                0.0,
                0.0,
                if comp.clips_content {
                    Some((comp.width.max(1.0), comp.height.max(1.0)))
                } else {
                    None
                },
                &comp.name,
                1,
            );
            if !layers.is_empty()
                && let Some(sym) = self.project.symbol_mut(&sym_id)
            {
                sym.timeline.layers = layers;
            }
        }
        self.report.statistics.symbols = self.project.symbols.len();
    }

    fn build_symbol_from_container(
        &mut self,
        container: &FigmaNode,
        folder: &str,
        depth: usize,
    ) -> String {
        let sym_id = new_id("figsym");
        let layers = self.build_layers_for_children(
            &container.children,
            0.0,
            0.0,
            if container.clips_content {
                Some((container.width.max(1.0), container.height.max(1.0)))
            } else {
                None
            },
            &container.name,
            depth + 1,
        );
        let sym = Symbol {
            id: sym_id.clone(),
            name: container.name.clone(),
            folder: folder.to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: if layers.is_empty() {
                    vec![Layer {
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties::default(),
                    }]
                } else {
                    layers
                },
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        };
        self.project.symbols.push(sym);
        self.report.statistics.symbols = self.project.symbols.len();
        sym_id
    }

    fn convert_leaf_to_elements(
        &mut self,
        node: &FigmaNode,
        offset_x: f64,
        offset_y: f64,
        location: &str,
        depth: usize,
    ) -> Vec<Element> {
        if depth > 48 {
            return Vec::new();
        }
        let blend_mode = self.map_blend_mode(&node.blend_mode, location);
        let filters = self.map_effects(&node.effects, location);
        let base_tf = self.node_transform(node, offset_x, offset_y);

        // 1. Instance of a registered Figma Component (when children aren't overridden/detached or matching component exists)
        if node.node_type == "INSTANCE"
            && let Some(cid) = &node.component_id
            && let Some(sym_id) = self.component_symbols.get(cid).cloned()
        {
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: base_tf,
                kind: ElementKind::Instance {
                    symbol_id: sym_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.report.statistics.elements += 1;
            return vec![el];
        }

        // 2. Non-empty container (`FRAME`, `GROUP`, `SECTION`, `COMPONENT`, `INSTANCE`, `BOOLEAN_OPERATION` with children)
        if !node.children.is_empty() && node.fill_paths.is_empty() && node.text.is_none() {
            // Also include container background fill inside a nested MovieClip symbol!
            let sym_id = self.build_symbol_with_optional_bg(node, "Figma Layers", depth + 1);
            let el = Element {
                id: new_id("el"),
                name: sanitize_instance_name(&node.name),
                transform: base_tf,
                kind: ElementKind::Instance {
                    symbol_id: sym_id,
                    playback: InstancePlayback::Independent,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.report.statistics.elements += 1;
            return vec![el];
        }

        // 3. Editable Text (`TEXT`)
        if let Some(td) = &node.text {
            let text_color = node
                .fills
                .iter()
                .find_map(|p| match p {
                    FigmaPaint::Solid { color } => Some(*color),
                    FigmaPaint::Gradient { stops, .. } => stops.first().map(|s| s.color),
                    _ => None,
                })
                .unwrap_or([255, 255, 255, 255]);
            let text_rgb = (u32::from(text_color[0]) << 16)
                | (u32::from(text_color[1]) << 8)
                | u32::from(text_color[2]);
            let mut text_tf = base_tf;
            text_tf.alpha = (text_tf.alpha * (f64::from(text_color[3]) / 255.0)).clamp(0.0, 1.0);
            let fmt = TextFormat {
                font: td.font_family.clone(),
                size: td.font_size.clamp(1.0, 1024.0),
                color: text_rgb,
                bold: td.bold,
                italic: td.italic,
                align: td.align,
                letter_spacing: td.letter_spacing.clamp(-60.0, 60.0),
                leading: td
                    .line_height
                    .map(|lh| (lh - td.font_size).clamp(-200.0, 500.0))
                    .unwrap_or(2.0),
                ..TextFormat::default()
            };
            let el = Element {
                id: new_id("el"),
                name: node.name.clone(),
                transform: text_tf,
                kind: ElementKind::TextBox {
                    text: td.characters.clone(),
                    width: node.width.max(16.0),
                    height: node.height.max(td.font_size * 1.25).max(16.0),
                    format: fmt,
                    word_wrap: true,
                    multiline: td.characters.contains('\n') || node.height > td.font_size * 1.6,
                },
                visible: node.visible,
                cache_as_bitmap: !filters.is_empty(),
                blend_mode,
                filters,
                ..Element::default()
            };
            self.report.statistics.elements += 1;
            return vec![el];
        }

        // 4. Vector / Primitive / Image Fill Elements
        let mut out = Vec::new();
        let (fallback_cmds, prim_opt) = self.fallback_shape_commands(node);
        let stroke_w = if node.strokes.is_empty() {
            0.0
        } else {
            node.stroke_weight.clamp(0.0, 256.0)
        };
        let (stroke_solid, stroke_grad) = match node.strokes.first() {
            Some(FigmaPaint::Solid { color }) => (*color, None),
            Some(FigmaPaint::Gradient {
                kind,
                stops,
                start,
                end,
            }) => (
                stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]),
                Some(build_figma_gradient_paint(*kind, stops, *start, *end)),
            ),
            _ => ([0, 0, 0, 0], None),
        };

        let paths_to_fill: Vec<(Vec<PathCommand>, FillRule)> = if !node.fill_paths.is_empty() {
            node.fill_paths
                .iter()
                .filter(|(c, _)| validate_path_commands(c).is_ok())
                .cloned()
                .collect()
        } else if validate_path_commands(&fallback_cmds).is_ok() {
            vec![(fallback_cmds.clone(), FillRule::NonZero)]
        } else {
            Vec::new()
        };

        if node.fills.is_empty() && stroke_w > 0.0 {
            let scmds = node
                .stroke_paths
                .first()
                .filter(|c| validate_path_commands(c).is_ok())
                .cloned()
                .unwrap_or(fallback_cmds);
            if validate_path_commands(&scmds).is_ok() {
                let kind = if stroke_grad.is_some() {
                    ElementKind::PaintedPath {
                        commands: scmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: stroke_solid,
                        fill_gradient: None,
                        stroke_gradient: stroke_grad,
                        stroke_width: stroke_w,
                    }
                } else {
                    ElementKind::Path {
                        commands: scmds,
                        fill_rule: FillRule::NonZero,
                        fill: [0, 0, 0, 0],
                        stroke: stroke_solid,
                        stroke_width: stroke_w,
                    }
                };
                out.push(Element {
                    id: new_id("el"),
                    name: node.name.clone(),
                    transform: base_tf,
                    kind,
                    visible: node.visible,
                    cache_as_bitmap: !filters.is_empty(),
                    blend_mode,
                    filters,
                    ..Element::default()
                });
                self.report.statistics.elements += 1;
            }
            return out;
        }

        for (fi, paint) in node.fills.iter().enumerate() {
            let is_last_fill = fi + 1 == node.fills.len();
            let cur_stroke = if is_last_fill {
                stroke_solid
            } else {
                [0, 0, 0, 0]
            };
            let cur_stroke_w = if is_last_fill { stroke_w } else { 0.0 };
            let cur_stroke_grad = if is_last_fill {
                stroke_grad.clone()
            } else {
                None
            };

            match paint {
                FigmaPaint::Image { image_ref, alpha } => {
                    if let Some((res_id, iw, ih)) = self.ensure_image_resource(image_ref, location)
                    {
                        let mut img_tf = base_tf;
                        if iw > 0 && ih > 0 && node.width > 0.0 && node.height > 0.0 {
                            img_tf.scale_x *= node.width / (iw as f64);
                            img_tf.scale_y *= node.height / (ih as f64);
                        }
                        img_tf.alpha = (img_tf.alpha * alpha).clamp(0.0, 1.0);
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: img_tf,
                            kind: ElementKind::Bitmap {
                                resource_id: res_id,
                                width: iw,
                                height: ih,
                                smoothing: true,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    } else if let Some((cmds, rule)) = paths_to_fill.first() {
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: base_tf,
                            kind: ElementKind::Path {
                                commands: cmds.clone(),
                                fill_rule: *rule,
                                fill: [180, 180, 190, 255],
                                stroke: cur_stroke,
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    }
                }
                FigmaPaint::Solid { color } => {
                    if node.node_type == "RECTANGLE" && node.fill_paths.is_empty() {
                        let mut tf = base_tf;
                        tf.primitive = prim_opt;
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: tf,
                            kind: if cur_stroke_grad.is_some() {
                                ElementKind::PaintedPath {
                                    commands: fallback_cmds.clone(),
                                    fill_rule: FillRule::NonZero,
                                    fill: *color,
                                    stroke: cur_stroke,
                                    fill_gradient: None,
                                    stroke_gradient: cur_stroke_grad,
                                    stroke_width: cur_stroke_w,
                                }
                            } else {
                                ElementKind::Rectangle {
                                    width: node.width.max(1.0),
                                    height: node.height.max(1.0),
                                    fill: *color,
                                    stroke: cur_stroke,
                                    stroke_width: cur_stroke_w,
                                }
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    } else if node.node_type == "ELLIPSE" && node.fill_paths.is_empty() {
                        let mut tf = base_tf;
                        tf.primitive = prim_opt;
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: tf,
                            kind: ElementKind::Ellipse {
                                width: node.width.max(1.0),
                                height: node.height.max(1.0),
                                fill: *color,
                                stroke: cur_stroke,
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    } else {
                        for (cmds, rule) in &paths_to_fill {
                            out.push(Element {
                                id: new_id("el"),
                                name: node.name.clone(),
                                transform: base_tf,
                                kind: if cur_stroke_grad.is_some() {
                                    ElementKind::PaintedPath {
                                        commands: cmds.clone(),
                                        fill_rule: *rule,
                                        fill: *color,
                                        stroke: cur_stroke,
                                        fill_gradient: None,
                                        stroke_gradient: cur_stroke_grad.clone(),
                                        stroke_width: cur_stroke_w,
                                    }
                                } else {
                                    ElementKind::Path {
                                        commands: cmds.clone(),
                                        fill_rule: *rule,
                                        fill: *color,
                                        stroke: cur_stroke,
                                        stroke_width: cur_stroke_w,
                                    }
                                },
                                visible: node.visible,
                                cache_as_bitmap: !filters.is_empty(),
                                blend_mode,
                                filters: if is_last_fill {
                                    filters.clone()
                                } else {
                                    Vec::new()
                                },
                                ..Element::default()
                            });
                            self.report.statistics.elements += 1;
                        }
                    }
                }
                FigmaPaint::Gradient {
                    kind,
                    stops,
                    start,
                    end,
                } => {
                    let grad = build_figma_gradient_paint(*kind, stops, *start, *end);
                    let fallback_c = stops.first().map(|s| s.color).unwrap_or([0, 0, 0, 255]);
                    for (cmds, rule) in &paths_to_fill {
                        out.push(Element {
                            id: new_id("el"),
                            name: node.name.clone(),
                            transform: base_tf,
                            kind: ElementKind::PaintedPath {
                                commands: cmds.clone(),
                                fill_rule: *rule,
                                fill: fallback_c,
                                stroke: cur_stroke,
                                fill_gradient: Some(grad.clone()),
                                stroke_gradient: cur_stroke_grad.clone(),
                                stroke_width: cur_stroke_w,
                            },
                            visible: node.visible,
                            cache_as_bitmap: !filters.is_empty(),
                            blend_mode,
                            filters: if is_last_fill {
                                filters.clone()
                            } else {
                                Vec::new()
                            },
                            ..Element::default()
                        });
                        self.report.statistics.elements += 1;
                    }
                }
            }
        }
        out
    }

    fn build_symbol_with_optional_bg(
        &mut self,
        container: &FigmaNode,
        folder: &str,
        depth: usize,
    ) -> String {
        let sym_id = new_id("figsym");
        let mut layers = Vec::new();
        // Optional frame background fill layer at bottom (`layers[0]`)
        if !container.fills.is_empty() && container.width > 0.0 && container.height > 0.0 {
            let mut bg_node = container.clone();
            bg_node.children.clear();
            bg_node.x = 0.0;
            bg_node.y = 0.0;
            bg_node.relative_transform = None;
            let bg_els = self.convert_leaf_to_elements(
                &bg_node,
                0.0,
                0.0,
                &format!("{} / Background", container.name),
                depth + 1,
            );
            if !bg_els.is_empty() {
                layers.push(Layer {
                    id: new_id("layer"),
                    name: "Background".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: bg_els,
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties::default(),
                });
                self.report.statistics.layers += 1;
                self.report.statistics.keyframes += 1;
            }
        }
        let child_layers = self.build_layers_for_children(
            &container.children,
            0.0,
            0.0,
            if container.clips_content {
                Some((container.width.max(1.0), container.height.max(1.0)))
            } else {
                None
            },
            &container.name,
            depth + 1,
        );
        layers.extend(child_layers);
        if layers.is_empty() {
            layers.push(Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties::default(),
            });
        }
        self.project.symbols.push(Symbol {
            id: sym_id.clone(),
            name: container.name.clone(),
            folder: folder.to_owned(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers,
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        self.report.statistics.symbols = self.project.symbols.len();
        sym_id
    }

    fn build_layers_for_children(
        &mut self,
        children: &[FigmaNode],
        offset_x: f64,
        offset_y: f64,
        clip_bounds: Option<(f64, f64)>,
        parent_loc: &str,
        depth: usize,
    ) -> Vec<Layer> {
        if depth > 48 {
            return Vec::new();
        }
        let mut layers = Vec::new();
        let mut active_mask_layer_id: Option<String> = None;

        // Optional frame clip mask (`clipsContent: true`)
        if self.options.preserve_masks
            && let Some((cw, ch)) = clip_bounds
            && !children.is_empty()
        {
            let clip_id = new_id("layer");
            let clip_el = Element {
                id: new_id("el"),
                name: "Clip Bounds".into(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: cw,
                    height: ch,
                    fill: [0, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            };
            layers.push(Layer {
                id: clip_id.clone(),
                name: "Frame Clip".into(),
                visible: true,
                locked: true,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![clip_el],
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    role: LayerRole::Mask,
                    mask_mode: MaskMode::Geometry,
                    ..LayerProperties::default()
                },
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            self.report.statistics.elements += 1;
            active_mask_layer_id = Some(clip_id);
        }

        if self.options.layer_mapping == FigmaLayerMapping::FlattenSingleLayer && depth <= 1 {
            let mut all_elements = Vec::new();
            for child in children {
                if !child.visible && !self.options.import_hidden_layers {
                    continue;
                }
                let loc = format!("{parent_loc} / {}", child.name);
                all_elements.extend(self.convert_leaf_to_elements(
                    child,
                    offset_x,
                    offset_y,
                    &loc,
                    depth + 1,
                ));
            }
            let lid = new_id("layer");
            layers.push(Layer {
                id: lid,
                name: parent_loc.to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: all_elements,
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    mask: active_mask_layer_id,
                    ..LayerProperties::default()
                },
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
            return layers;
        }

        for child in children {
            if !child.visible && !self.options.import_hidden_layers {
                continue;
            }
            let loc = format!("{parent_loc} / {}", child.name);

            // Decompose pure `GROUP` / `SECTION` at root composition level into a `Folder` + child layers
            // when no group-level mask/filter/instance forces symbol wrapping!
            if matches!(child.node_type.as_str(), "GROUP" | "SECTION")
                && !child.is_mask
                && child.effects.is_empty()
                && (child.opacity - 1.0).abs() < 1e-6
                && matches!(child.blend_mode.as_str(), "" | "PASS_THROUGH" | "NORMAL")
                && self.options.layer_mapping == FigmaLayerMapping::TimelineLayers
                && !child.children.is_empty()
                && depth <= 3
            {
                let folder_id = new_id("folder");
                layers.push(Layer {
                    id: folder_id.clone(),
                    name: child.name.clone(),
                    visible: child.visible,
                    locked: child.locked,
                    keyframes: Vec::new(),
                    properties: LayerProperties {
                        role: LayerRole::Folder,
                        ..LayerProperties::default()
                    },
                });
                self.report.statistics.layers += 1;

                // Note: in Figma JSON, `GROUP` children already have coordinates relative to the
                // enclosing Frame (via `parent_abs` or `relativeTransform`); in `.fig` Kiwi, group
                // children use local transforms relative to parent if group has an offset, so apply
                // `-offset` cleanly.
                let group_sub_layers = self.build_layers_for_children(
                    &child.children,
                    offset_x - child.x,
                    offset_y - child.y,
                    None,
                    &loc,
                    depth + 1,
                );
                for mut sl in group_sub_layers {
                    if sl.properties.parent.is_none() {
                        sl.properties.parent = Some(folder_id.clone());
                    }
                    if sl.properties.mask.is_none() && sl.properties.role == LayerRole::Normal {
                        sl.properties.mask = active_mask_layer_id.clone();
                    }
                    layers.push(sl);
                }
                continue;
            }

            let mut elements =
                self.convert_leaf_to_elements(child, offset_x, offset_y, &loc, depth + 1);

            if self.options.layer_mapping == FigmaLayerMapping::MovieClipSymbols
                && elements.len() == 1
                && !matches!(elements[0].kind, ElementKind::Instance { .. })
            {
                let sym_id = new_id("figmc");
                let inner_el = elements.remove(0);
                self.project.symbols.push(Symbol {
                    id: sym_id.clone(),
                    name: child.name.clone(),
                    folder: "Figma Layers".into(),
                    kind: SymbolKind::MovieClip,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: new_id("layer"),
                            name: child.name.clone(),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                elements: vec![inner_el],
                                ..Keyframe::default()
                            }],
                            properties: LayerProperties::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                    class_name: None,
                    linkage_id: None,
                });
                self.report.statistics.symbols = self.project.symbols.len();
                elements.push(Element {
                    id: new_id("el"),
                    name: sanitize_instance_name(&child.name),
                    transform: Transform::default(),
                    kind: ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::Independent,
                    },
                    ..Element::default()
                });
            }

            let lid = new_id("layer");
            let mut props = LayerProperties {
                blend_mode: self.map_blend_mode(&child.blend_mode, &loc),
                ..LayerProperties::default()
            };

            if child.is_mask && self.options.preserve_masks {
                props.role = LayerRole::Mask;
                props.mask_mode = match child.mask_type.as_str() {
                    "VECTOR" | "OUTLINE" => MaskMode::Geometry,
                    "LUMINANCE" => MaskMode::Luminance,
                    _ => MaskMode::Alpha,
                };
                // Sanitize mask elements so `validate_layers` always passes:
                // - `TextBox` has no vector path in mask evaluation -> convert to bounding rectangle
                // - `Bitmap` on a `Geometry` mask requires paint coverage -> upgrade to `MaskMode::Alpha`
                let mut had_text = false;
                let mut had_bitmap = false;
                for el in &mut elements {
                    match &el.kind {
                        ElementKind::TextBox { width, height, .. } => {
                            let w = (*width).clamp(1.0, 32_768.0);
                            let h = (*height).clamp(1.0, 32_768.0);
                            el.kind = ElementKind::Rectangle {
                                width: w,
                                height: h,
                                fill: [0, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            };
                            had_text = true;
                        }
                        ElementKind::Bitmap { .. } => {
                            had_bitmap = true;
                        }
                        _ => {}
                    }
                }
                if had_text {
                    self.report.warning(
                        "figma-mask-text-bounds-approximated",
                        loc.clone(),
                        "Figma text mask approximated as its bounding rectangle mask.",
                    );
                }
                if had_bitmap && props.mask_mode == MaskMode::Geometry {
                    props.mask_mode = MaskMode::Alpha;
                    self.report.warning(
                        "figma-mask-bitmap-alpha",
                        loc.clone(),
                        "Figma vector mask with bitmap fill upgraded to Alpha mask mode.",
                    );
                }
                active_mask_layer_id = Some(lid.clone());
            } else if let Some(mid) = &active_mask_layer_id {
                props.mask = Some(mid.clone());
            }

            layers.push(Layer {
                id: lid,
                name: child.name.clone(),
                visible: child.visible,
                locked: child.locked,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements,
                    ..Keyframe::default()
                }],
                properties: props,
            });
            self.report.statistics.layers += 1;
            self.report.statistics.keyframes += 1;
        }
        layers
    }
}

fn sanitize_instance_name(raw: &str) -> String {
    let mut s = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            s.push(ch);
        } else if (ch == ' ' || ch == '-') && !s.ends_with('_') && !s.is_empty() {
            s.push('_');
        }
    }
    let trimmed = s.trim_matches('_');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.as_bytes()[0].is_ascii_digit() {
        format!("_{trimmed}")
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn build_project_from_figma_doc(
    doc: FigmaDoc,
    options: &FigmaImportOptions,
    source_label: &str,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    project.name = doc.name.clone();
    let mut report = ImportReport {
        source: source_label.to_owned(),
        format: ImportFormat::Figma,
        version: Some("Figma".into()),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if doc.pages.is_empty() {
        return Err(ImportError::Blocked(
            "Figma document contains no importable pages or frames.".into(),
        ));
    }

    let mut ctx = FigmaBuildCtx {
        options,
        images: &doc.images,
        image_resources: HashMap::new(),
        component_symbols: HashMap::new(),
        project: &mut project,
        report: &mut report,
    };

    // Pass 1: Pre-allocate all `COMPONENT` symbol IDs first, then populate their timelines,
    // so forward-referenced or nested `INSTANCE`s resolve deterministically and cycles cannot loop.
    let mut comp_nodes = Vec::new();
    for page in &doc.pages {
        ctx.collect_component_nodes(page, &mut comp_nodes, 0);
    }
    ctx.populate_component_symbols(&comp_nodes);

    // Pass 2: Pick primary page & primary composition artboard (if the page has a main artboard Frame,
    // decompose its children directly into the composition's Timeline layers!).
    let primary_page = doc
        .pages
        .iter()
        .find(|p| p.visible && !p.children.is_empty())
        .unwrap_or(&doc.pages[0]);

    if let Some(bg) = primary_page.background_color {
        ctx.project.background = bg;
    }

    let visible_top: Vec<&FigmaNode> = primary_page
        .children
        .iter()
        .filter(|n| n.visible || options.import_hidden_layers)
        .collect();

    let root_layers = if visible_top.len() == 1
        && matches!(
            visible_top[0].node_type.as_str(),
            "FRAME" | "COMPONENT" | "SECTION"
        ) {
        let artboard = visible_top[0];
        if options.resize_stage_to_artboard && artboard.width >= 1.0 && artboard.height >= 1.0 {
            ctx.project.width = (artboard.width.round() as u32).clamp(1, 8192);
            ctx.project.height = (artboard.height.round() as u32).clamp(1, 8192);
        }
        if let Some(FigmaPaint::Solid { color }) = artboard.fills.first() {
            ctx.project.background = *color;
        }
        ctx.build_layers_for_children(
            &artboard.children,
            0.0,
            0.0,
            if artboard.clips_content {
                Some((artboard.width.max(1.0), artboard.height.max(1.0)))
            } else {
                None
            },
            &artboard.name,
            1,
        )
    } else {
        // Multiple top-level frames/elements on the page: fit stage to bounding box or first frame
        if options.resize_stage_to_artboard
            && let Some(first_frame) = visible_top
                .iter()
                .find(|n| matches!(n.node_type.as_str(), "FRAME" | "SECTION"))
            && first_frame.width >= 1.0
            && first_frame.height >= 1.0
        {
            ctx.project.width = (first_frame.width.round() as u32).clamp(1, 8192);
            ctx.project.height = (first_frame.height.round() as u32).clamp(1, 8192);
        }
        let base_x = visible_top.first().map(|n| n.x).unwrap_or(0.0);
        let base_y = visible_top.first().map(|n| n.y).unwrap_or(0.0);
        ctx.build_layers_for_children(
            &primary_page.children,
            base_x,
            base_y,
            None,
            &primary_page.name,
            1,
        )
    };

    // Additional pages -> imported as Library MovieClip compositions so multi-page Figma files lose nothing!
    for extra_page in doc.pages.iter().filter(|p| p.id != primary_page.id) {
        if extra_page.children.is_empty() {
            continue;
        }
        ctx.build_symbol_from_container(extra_page, "Figma Pages", 1);
    }

    let root_id = project.root.clone();
    if let Some(root_sym) = project.symbol_mut(&root_id)
        && !root_layers.is_empty()
    {
        root_sym.timeline.layers = root_layers;
    }

    if !project.resources.is_empty() {
        project.format_version = project.format_version.max(3);
    }
    if project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .any(|l| !l.properties.is_default())
    {
        project.format_version = project.format_version.max(4);
    }
    project.upgrade_format_for_features();
    project.validate().map_err(|e| {
        ImportError::Blocked(format!("Imported Figma composition failed validation: {e}"))
    })?;
    report.statistics.symbols = project.symbols.len();
    report.diagnostics.push(Diagnostic {
        severity: Severity::Info,
        code: "figma-composition-imported".into(),
        location: source_label.to_owned(),
        message: format!(
            "Imported Figma composition '{}' into {} layer(s), {} symbol(s) and {} element(s).",
            project.name,
            report.statistics.layers,
            report.statistics.symbols,
            report.statistics.elements
        ),
    });
    report.can_import = true;
    Ok((project, report))
}

/// Import a `.figma.json` UTF-8 string into a multi-layer NeoFlash [`Project`].
pub fn import_figma_json_str(
    json_str: &str,
    source_name: &str,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let doc = parse_figma_json_doc(json_str, source_name)?;
    build_project_from_figma_doc(doc, options, source_name)
}

/// Import a Figma `.fig` archive (ZIP with `canvas.fig` + `images/*` or standalone `fig-kiwi`) or `.figma.json` byte slice.
pub fn import_figma_bytes(
    bytes: &[u8],
    source_name: &str,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    if bytes.len() > MAX_FIGMA_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    if bytes.starts_with(b"fig-kiwi") || bytes.starts_with(b"fig-jam.") {
        let doc = parse_fig_kiwi_canvas(bytes, source_name, BTreeMap::new())?;
        return build_project_from_figma_doc(doc, options, source_name);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Container(format!("Invalid .fig ZIP archive: {e}")))?;
        if zip.len() > 4096 {
            return Err(ImportError::Container(
                ".fig ZIP archive exceeds 4096 entries".into(),
            ));
        }
        let mut canvas_bytes: Option<Vec<u8>> = None;
        let mut json_bytes: Option<Vec<u8>> = None;
        let mut images = BTreeMap::new();
        let mut total_image_bytes = 0usize;
        let mut meta_name: Option<String> = None;

        for i in 0..zip.len() {
            let entry = zip
                .by_index(i)
                .map_err(|e| ImportError::Container(e.to_string()))?;
            let ename = entry.name().to_owned();
            crate::source::safe_name(&ename)?;
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(ImportError::Container(format!(
                    "Symlink ZIP entry in .fig archive: {ename}"
                )));
            }
            if entry.is_dir() {
                continue;
            }
            if ename == "canvas.fig" || ename.ends_with(".fig") {
                let mut raw = Vec::new();
                entry
                    .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX_FIGMA_DECODE_BYTES {
                    return Err(ImportError::Container(
                        "canvas.fig entry exceeds 128 MiB limit".into(),
                    ));
                }
                canvas_bytes = Some(raw);
            } else if ename == "meta.json" {
                let mut raw = String::new();
                let _ = entry.take(1024 * 1024).read_to_string(&mut raw);
                if let Ok(v) = serde_json::from_str::<Value>(&raw)
                    && let Some(fname) = v.get("file_name").and_then(Value::as_str)
                {
                    meta_name = Some(fname.to_owned());
                }
            } else if ename.starts_with("images/") {
                let key = ename
                    .trim_start_matches("images/")
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_owned();
                if !key.is_empty() {
                    let mut raw = Vec::new();
                    entry
                        .take(neoflash_core::MAX_RESOURCE_BYTES as u64 + 1)
                        .read_to_end(&mut raw)?;
                    if raw.len() > neoflash_core::MAX_RESOURCE_BYTES {
                        return Err(ImportError::Container(format!(
                            ".fig image '{ename}' exceeds resource size budget"
                        )));
                    }
                    total_image_bytes = total_image_bytes.saturating_add(raw.len());
                    if total_image_bytes > MAX_FIGMA_IMAGE_TOTAL_BYTES {
                        return Err(ImportError::Container(
                            ".fig archive images exceed 64 MiB aggregate budget".into(),
                        ));
                    }
                    images.insert(key, raw);
                }
            } else if ename.ends_with(".json") && json_bytes.is_none() {
                let mut raw = Vec::new();
                entry
                    .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
                    .read_to_end(&mut raw)?;
                if raw.len() > MAX_FIGMA_DECODE_BYTES {
                    return Err(ImportError::Container(
                        ".fig JSON entry exceeds 128 MiB limit".into(),
                    ));
                }
                json_bytes = Some(raw);
            }
        }

        let effective_name = meta_name.as_deref().unwrap_or(source_name);
        if let Some(cb) = canvas_bytes {
            let doc = parse_fig_kiwi_canvas(&cb, effective_name, images)?;
            return build_project_from_figma_doc(doc, options, source_name);
        }
        if let Some(jb) = json_bytes {
            let s = std::str::from_utf8(&jb)
                .map_err(|_| ImportError::Container("Figma JSON entry must be UTF-8".into()))?;
            let mut doc = parse_figma_json_doc(s, effective_name)?;
            doc.images.extend(images);
            return build_project_from_figma_doc(doc, options, source_name);
        }
        return Err(ImportError::Container(
            ".fig ZIP archive contains neither canvas.fig nor a Figma JSON document".into(),
        ));
    }

    let json_str = std::str::from_utf8(bytes).map_err(|_| {
        ImportError::Container("Figma file is neither a binary .fig archive nor UTF-8 JSON".into())
    })?;
    import_figma_json_str(json_str, source_name, options)
}

fn build_figma_gradient_paint(
    kind: GradientKind,
    stops: &[GradientStop],
    start: [f64; 2],
    end: [f64; 2],
) -> GradientPaint {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len = dx.hypot(dy).max(1.0);
    let matrix = match kind {
        GradientKind::Linear => {
            let cos_a = dx / len;
            let sin_a = dy / len;
            let s = len / 1638.4;
            let mx = (start[0] + end[0]) * 0.5;
            let my = (start[1] + end[1]) * 0.5;
            [s * cos_a, s * sin_a, -s * sin_a, s * cos_a, mx, my]
        }
        GradientKind::Radial => {
            let s = len / 819.2;
            [s, 0.0, 0.0, s, start[0], start[1]]
        }
    };
    let mut sorted = stops.to_vec();
    sorted.sort_by_key(|st| st.ratio);
    sorted.truncate(15);
    GradientPaint {
        kind,
        matrix,
        stops: sorted,
        focal_point_ratio: 0.0,
        ..Default::default()
    }
}

/// Import a `.fig` or `.figma.json` file from disk into a fresh [`Project`].
pub fn import_figma_path(
    path: &Path,
    options: &FigmaImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    if std::fs::metadata(path)?.len() > MAX_FIGMA_FILE_BYTES as u64 {
        return Err(ImportError::Container(format!(
            "Figma file exceeds {} MiB input limit",
            MAX_FIGMA_FILE_BYTES / (1024 * 1024)
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_FIGMA_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim_end_matches(".figma"))
        .filter(|s| !s.is_empty())
        .unwrap_or("Figma Import");
    let (project, mut report) = import_figma_bytes(&bytes, stem, options)?;
    report.source = path.display().to_string();
    Ok((project, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::DeflateEncoder};
    use std::io::Write;

    fn tiny_png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 120, 40, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn figma_json_decomposes_artboard_into_layers_components_masks_vectors_and_text() {
        let png_b64 = BASE64.encode(tiny_png_bytes());
        let figma_json = format!(
            r#"{{
              "name": "Hero Banner Design",
              "schemaVersion": 0,
              "images": {{
                "img_hero_1": "{png_b64}"
              }},
              "document": {{
                "id": "0:0",
                "name": "Document",
                "type": "DOCUMENT",
                "children": [
                  {{
                    "id": "0:1",
                    "name": "Landing Page",
                    "type": "CANVAS",
                    "backgroundColor": {{"r": 0.08, "g": 0.09, "b": 0.12, "a": 1.0}},
                    "children": [
                      {{
                        "id": "1:10",
                        "name": "CTA_Button",
                        "type": "COMPONENT",
                        "visible": false,
                        "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 160, "height": 48}},
                        "children": [
                          {{
                            "id": "1:11",
                            "name": "Btn_BG",
                            "type": "RECTANGLE",
                            "cornerRadius": 12,
                            "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 160, "height": 48}},
                            "fills": [{{"type": "SOLID", "color": {{"r": 0.2, "g": 0.5, "b": 1.0, "a": 1.0}}}}]
                          }}
                        ]
                      }},
                      {{
                        "id": "2:1",
                        "name": "Main Composition",
                        "type": "FRAME",
                        "clipsContent": true,
                        "absoluteBoundingBox": {{"x": 100, "y": 50, "width": 1280, "height": 720}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 0.05, "g": 0.06, "b": 0.1, "a": 1.0}}}}],
                        "children": [
                          {{
                            "id": "2:2",
                            "name": "Photo Mask",
                            "type": "ELLIPSE",
                            "isMask": true,
                            "maskType": "VECTOR",
                            "absoluteBoundingBox": {{"x": 140, "y": 90, "width": 400, "height": 400}},
                            "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}]
                          }},
                          {{
                            "id": "2:3",
                            "name": "Hero Photo",
                            "type": "RECTANGLE",
                            "absoluteBoundingBox": {{"x": 140, "y": 90, "width": 400, "height": 400}},
                            "fills": [{{"type": "IMAGE", "imageRef": "img_hero_1"}}]
                          }},
                          {{
                            "id": "2:4",
                            "name": "Accent Ribbon",
                            "type": "VECTOR",
                            "blendMode": "SCREEN",
                            "absoluteBoundingBox": {{"x": 200, "y": 150, "width": 300, "height": 120}},
                            "fillGeometry": [{{"path": "M 0 0 L 300 0 L 260 120 L 0 120 Z", "windingRule": "NONZERO"}}],
                            "fills": [{{
                              "type": "GRADIENT_LINEAR",
                              "gradientStops": [
                                {{"position": 0.0, "color": {{"r": 1.0, "g": 0.3, "b": 0.2, "a": 1.0}}}},
                                {{"position": 1.0, "color": {{"r": 0.9, "g": 0.8, "b": 0.1, "a": 1.0}}}}
                              ]
                            }}],
                            "effects": [{{
                              "type": "DROP_SHADOW",
                              "visible": true,
                              "color": {{"r": 0, "g": 0, "b": 0, "a": 0.5}},
                              "offset": {{"x": 0, "y": 8}},
                              "radius": 16
                            }}]
                          }},
                          {{
                            "id": "2:5",
                            "name": "Headline",
                            "type": "TEXT",
                            "absoluteBoundingBox": {{"x": 600, "y": 180, "width": 520, "height": 80}},
                            "characters": "Motion Meets Design",
                            "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                            "style": {{"fontFamily": "Inter", "fontSize": 48, "fontWeight": 700, "textAlignHorizontal": "LEFT"}}
                          }},
                          {{
                            "id": "2:6",
                            "name": "Primary CTA",
                            "type": "INSTANCE",
                            "componentId": "1:10",
                            "absoluteBoundingBox": {{"x": 600, "y": 300, "width": 160, "height": 48}}
                          }}
                        ]
                      }}
                    ]
                  }}
                ]
              }}
            }}"#
        );

        let opts = FigmaImportOptions {
            import_hidden_layers: false,
            ..FigmaImportOptions::default()
        };
        let (project, report) =
            import_figma_json_str(&figma_json, "Hero Banner", &opts).expect("figma json import");
        project
            .validate()
            .expect("imported Figma project validates");
        assert!(report.can_import);
        assert_eq!(project.width, 1280);
        assert_eq!(project.height, 720);
        assert_eq!(project.resources.len(), 1);

        let root = project.symbol(&project.root).unwrap();
        // Frame Clip + Photo Mask + Hero Photo + Accent Ribbon + Headline + Primary CTA = 6 layers!
        assert_eq!(root.timeline.layers.len(), 6);
        assert_eq!(root.timeline.layers[1].properties.role, LayerRole::Mask);
        assert_eq!(
            root.timeline.layers[2].properties.mask.as_deref(),
            Some(root.timeline.layers[1].id.as_str())
        );
        assert!(
            project
                .symbols
                .iter()
                .any(|s| s.name == "CTA_Button" && s.kind == SymbolKind::MovieClip)
        );
    }

    fn write_var_uint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let mut b = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
    }

    fn write_var_int(out: &mut Vec<u8>, v: i64) {
        let zigzag = ((v << 1) ^ (v >> 63)) as u64;
        write_var_uint(out, zigzag);
    }

    fn write_var_float(out: &mut Vec<u8>, f: f32) {
        if f == 0.0 {
            out.push(0);
            return;
        }
        let bits = f.to_bits();
        let rotated = bits.rotate_left(9);
        out.extend_from_slice(&rotated.to_le_bytes());
    }

    fn write_cstr(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }

    #[test]
    fn binary_fig_kiwi_zip_archive_decodes_into_layered_project() {
        // Build minimal Kiwi schema:
        // 0: GUID (STRUCT: sessionID uint, localID uint)
        // 1: ParentIndex (STRUCT: guid GUID, position string)
        // 2: Vector (STRUCT: x float, y float)
        // 3: Color (STRUCT: r float, g float, b float, a float)
        // 4: Paint (MESSAGE: 1=type string, 2=color Color)
        // 5: NodeChange (MESSAGE: 1=guid GUID, 2=parentIndex ParentIndex, 3=type string, 4=name string, 5=size Vector, 6=fillPaints Paint[])
        // 6: Message (MESSAGE: 1=nodeChanges NodeChange[])
        let mut schema = Vec::new();
        write_var_uint(&mut schema, 7);
        // 0: GUID
        write_cstr(&mut schema, "GUID");
        schema.push(1);
        write_var_uint(&mut schema, 2);
        write_cstr(&mut schema, "sessionID");
        write_var_int(&mut schema, -4);
        schema.push(0);
        write_var_uint(&mut schema, 1);
        write_cstr(&mut schema, "localID");
        write_var_int(&mut schema, -4);
        schema.push(0);
        write_var_uint(&mut schema, 2);
        // 1: ParentIndex
        write_cstr(&mut schema, "ParentIndex");
        schema.push(1);
        write_var_uint(&mut schema, 2);
        write_cstr(&mut schema, "guid");
        write_var_int(&mut schema, 0);
        schema.push(0);
        write_var_uint(&mut schema, 1);
        write_cstr(&mut schema, "position");
        write_var_int(&mut schema, -6);
        schema.push(0);
        write_var_uint(&mut schema, 2);
        // 2: Vector
        write_cstr(&mut schema, "Vector");
        schema.push(1);
        write_var_uint(&mut schema, 2);
        write_cstr(&mut schema, "x");
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, 1);
        write_cstr(&mut schema, "y");
        write_var_int(&mut schema, -5);
        schema.push(0);
        write_var_uint(&mut schema, 2);
        // 3: Color
        write_cstr(&mut schema, "Color");
        schema.push(1);
        write_var_uint(&mut schema, 4);
        for (i, ch) in ["r", "g", "b", "a"].iter().enumerate() {
            write_cstr(&mut schema, ch);
            write_var_int(&mut schema, -5);
            schema.push(0);
            write_var_uint(&mut schema, (i + 1) as u64);
        }
        // 4: Paint
        write_cstr(&mut schema, "Paint");
        schema.push(2);
        write_var_uint(&mut schema, 2);
        write_cstr(&mut schema, "type");
        write_var_int(&mut schema, -6);
        schema.push(0);
        write_var_uint(&mut schema, 1);
        write_cstr(&mut schema, "color");
        write_var_int(&mut schema, 3);
        schema.push(0);
        write_var_uint(&mut schema, 2);
        // 5: NodeChange
        write_cstr(&mut schema, "NodeChange");
        schema.push(2);
        write_var_uint(&mut schema, 6);
        for (tag, fname, tid, arr) in [
            (1u64, "guid", 0i64, 0u8),
            (2, "parentIndex", 1, 0),
            (3, "type", -6, 0),
            (4, "name", -6, 0),
            (5, "size", 2, 0),
            (6, "fillPaints", 4, 1),
        ] {
            write_cstr(&mut schema, fname);
            write_var_int(&mut schema, tid);
            schema.push(arr);
            write_var_uint(&mut schema, tag);
        }
        // 6: Message
        write_cstr(&mut schema, "Message");
        schema.push(2);
        write_var_uint(&mut schema, 1);
        write_cstr(&mut schema, "nodeChanges");
        write_var_int(&mut schema, 5);
        schema.push(1);
        write_var_uint(&mut schema, 1);

        // Encode root Message with 4 nodes: DOCUMENT(0:0) -> CANVAS(0:1) -> FRAME(1:1, 800x600) -> RECTANGLE(1:2) + ELLIPSE(1:3)
        let mut msg = Vec::new();
        write_var_uint(&mut msg, 1); // field 1 = nodeChanges
        write_var_uint(&mut msg, 5); // array len 5

        // Node 0: DOCUMENT 0:0
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 0);
        write_var_uint(&mut msg, 0);
        write_var_uint(&mut msg, 3);
        write_cstr(&mut msg, "DOCUMENT");
        write_var_uint(&mut msg, 4);
        write_cstr(&mut msg, "Doc");
        write_var_uint(&mut msg, 0);

        // Node 1: CANVAS 0:1 parent 0:0
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 0);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 2);
        write_var_uint(&mut msg, 0);
        write_var_uint(&mut msg, 0);
        write_cstr(&mut msg, "!");
        write_var_uint(&mut msg, 3);
        write_cstr(&mut msg, "CANVAS");
        write_var_uint(&mut msg, 4);
        write_cstr(&mut msg, "Page 1");
        write_var_uint(&mut msg, 0);

        // Node 2: FRAME 1:1 parent 0:1 size 800x600
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 1);
        write_var_uint(&mut msg, 2);
        write_var_uint(&mut msg, 0);
        write_var_uint(&mut msg, 1);
        write_cstr(&mut msg, "!");
        write_var_uint(&mut msg, 3);
        write_cstr(&mut msg, "FRAME");
        write_var_uint(&mut msg, 4);
        write_cstr(&mut msg, "Board");
        write_var_uint(&mut msg, 5);
        write_var_float(&mut msg, 800.0);
        write_var_float(&mut msg, 600.0);
        write_var_uint(&mut msg, 0);

        // Node 3 & 4: RECTANGLE & ELLIPSE inside 1:1
        for (lid, pos_s, ntype, lname) in [
            (2u64, "a", "RECTANGLE", "Card BG"),
            (3u64, "b", "ELLIPSE", "Avatar Badge"),
        ] {
            write_var_uint(&mut msg, 1);
            write_var_uint(&mut msg, 1);
            write_var_uint(&mut msg, lid);
            write_var_uint(&mut msg, 2);
            write_var_uint(&mut msg, 1);
            write_var_uint(&mut msg, 1);
            write_cstr(&mut msg, pos_s);
            write_var_uint(&mut msg, 3);
            write_cstr(&mut msg, ntype);
            write_var_uint(&mut msg, 4);
            write_cstr(&mut msg, lname);
            write_var_uint(&mut msg, 5);
            write_var_float(&mut msg, 120.0);
            write_var_float(&mut msg, 90.0);
            write_var_uint(&mut msg, 6); // fillPaints len 1
            write_var_uint(&mut msg, 1);
            write_var_uint(&mut msg, 1);
            write_cstr(&mut msg, "SOLID");
            write_var_uint(&mut msg, 2);
            write_var_float(&mut msg, 0.25);
            write_var_float(&mut msg, 0.6);
            write_var_float(&mut msg, 0.95);
            write_var_float(&mut msg, 1.0);
            write_var_uint(&mut msg, 0); // end Paint
            write_var_uint(&mut msg, 0); // end NodeChange
        }
        write_var_uint(&mut msg, 0); // end Message

        let deflate = |raw: &[u8]| -> Vec<u8> {
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            enc.write_all(raw).unwrap();
            enc.finish().unwrap()
        };
        let c0 = deflate(&schema);
        let c1 = deflate(&msg);
        let mut canvas_fig = Vec::new();
        canvas_fig.extend_from_slice(b"fig-kiwi");
        canvas_fig.extend_from_slice(&15u32.to_le_bytes());
        canvas_fig.extend_from_slice(&(c0.len() as u32).to_le_bytes());
        canvas_fig.extend_from_slice(&c0);
        canvas_fig.extend_from_slice(&(c1.len() as u32).to_le_bytes());
        canvas_fig.extend_from_slice(&c1);

        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut zip_buf);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("meta.json", opts).unwrap();
            zw.write_all(br#"{"file_name":"UI Kit Board"}"#).unwrap();
            zw.start_file("canvas.fig", opts).unwrap();
            zw.write_all(&canvas_fig).unwrap();
            zw.finish().unwrap();
        }
        let fig_zip_bytes = zip_buf.into_inner();
        assert!(is_figma_bytes(&fig_zip_bytes));

        let (project, report) =
            import_figma_bytes(&fig_zip_bytes, "fallback", &FigmaImportOptions::default())
                .expect("binary .fig zip import");
        project.validate().expect("project validates");
        assert!(report.can_import);
        assert_eq!(project.name, "UI Kit Board");
        assert_eq!(project.width, 800);
        assert_eq!(project.height, 600);
        let root = project.symbol(&project.root).unwrap();
        assert_eq!(root.timeline.layers.len(), 2);
        assert_eq!(root.timeline.layers[0].name, "Card BG");
        assert_eq!(root.timeline.layers[1].name, "Avatar Badge");
    }

    #[test]
    fn t259_figma_hardening_handles_forward_components_text_masks_arcs_and_cyclic_kiwi_structs() {
        // 1. Forward-referenced INSTANCE before COMPONENT + TEXT mask + >360° arc + 20 gradient stops + oversized corner radii
        let stops_json: String = (0..20)
            .map(|i| {
                let p = f64::from(i) / 19.0;
                format!(r#"{{"position": {p:.4}, "color": {{"r": {p:.4}, "g": 0.4, "b": 0.8, "a": 1.0}}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        let figma_json = format!(
            r#"{{
              "name": "Seam Torture",
              "schemaVersion": 0,
              "document": {{
                "id": "0:0", "name": "Doc", "type": "DOCUMENT",
                "children": [{{
                  "id": "0:1", "name": "Page 1", "type": "CANVAS",
                  "children": [
                    {{
                      "id": "2:1", "name": "Board", "type": "FRAME",
                      "absoluteBoundingBox": {{"x": 0, "y": 0, "width": 600, "height": 400}},
                      "children": [
                        {{
                          "id": "2:2", "name": "Forward CTA", "type": "INSTANCE",
                          "componentId": "9:99",
                          "absoluteBoundingBox": {{"x": 20, "y": 20, "width": 120, "height": 40}}
                        }},
                        {{
                          "id": "2:3", "name": "Text Mask", "type": "TEXT",
                          "isMask": true, "maskType": "VECTOR",
                          "characters": "CLIP",
                          "absoluteBoundingBox": {{"x": 40, "y": 80, "width": 200, "height": 60}},
                          "fills": [{{"type": "SOLID", "color": {{"r": 1, "g": 1, "b": 1, "a": 1}}}}],
                          "style": {{"fontFamily": "Inter", "fontSize": 32}}
                        }},
                        {{
                          "id": "2:4", "name": "Masked Arc", "type": "ELLIPSE",
                          "arcData": {{"startingAngle": -3.14159, "endingAngle": 9.42477, "innerRadius": 1.5}},
                          "absoluteBoundingBox": {{"x": 50, "y": 90, "width": 100, "height": 100}},
                          "fills": [{{"type": "GRADIENT_LINEAR", "gradientStops": [{stops_json}]}}]
                        }},
                        {{
                          "id": "2:5", "name": "Huge Radius Pill", "type": "RECTANGLE",
                          "cornerRadius": 9999.0,
                          "absoluteBoundingBox": {{"x": 10, "y": 200, "width": 80, "height": 30}},
                          "fills": [{{"type": "SOLID", "color": {{"r": 0.3, "g": 0.9, "b": 0.5, "a": 1.0}}}}]
                        }}
                      ]
                    }},
                    {{
                      "id": "9:99", "name": "Late_Defined_Component", "type": "COMPONENT",
                      "absoluteBoundingBox": {{"x": 700, "y": 0, "width": 120, "height": 40}},
                      "children": [{{
                        "id": "9:100", "name": "Inner Pill", "type": "RECTANGLE",
                        "cornerRadius": 20.0,
                        "absoluteBoundingBox": {{"x": 700, "y": 0, "width": 120, "height": 40}},
                        "fills": [{{"type": "SOLID", "color": {{"r": 0.9, "g": 0.2, "b": 0.4, "a": 1.0}}}}]
                      }}]
                    }}
                  ]
                }}]
              }}
            }}"#
        );
        let opts = FigmaImportOptions::default();
        let (project, report) = import_figma_json_str(&figma_json, "Seam Torture", &opts)
            .expect("torture figma json must import and validate");
        project.validate().expect("validated");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "figma-mask-text-bounds-approximated")
        );

        // 2. Cyclic Kiwi Struct definition must fail cleanly without stack overflow
        let mut cyc_schema = Vec::new();
        write_var_uint(&mut cyc_schema, 1);
        write_cstr(&mut cyc_schema, "SelfCycle");
        cyc_schema.push(1); // Struct
        write_var_uint(&mut cyc_schema, 1);
        write_cstr(&mut cyc_schema, "self");
        write_var_int(&mut cyc_schema, 0); // type_id 0 -> SelfCycle!
        cyc_schema.push(0);
        write_var_uint(&mut cyc_schema, 1);
        let defs = parse_kiwi_schema(&cyc_schema).unwrap();
        let mut mr = KiwiReader::new(&[0; 16]);
        let mut budget = MAX_KIWI_VALUES;
        let mut active = Vec::new();
        let err = decode_kiwi_scalar(&mut mr, &defs, 0, 0, &mut budget, &mut active)
            .expect_err("cyclic Kiwi struct must error");
        assert!(err.contains("Cyclic Kiwi struct"));

        // 3. Path-traversal ZIP entry inside .fig archive must be rejected
        let mut zip_buf = Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut zip_buf);
            let zopts = zip::write::SimpleFileOptions::default();
            zw.start_file("../escape.fig", zopts).unwrap();
            zw.write_all(b"fig-kiwi").unwrap();
            zw.finish().unwrap();
        }
        assert!(import_figma_bytes(&zip_buf.into_inner(), "bad", &opts).is_err());
    }
}
