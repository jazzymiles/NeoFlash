#![forbid(unsafe_code)]
//! Binary `fig-kiwi` / `fig-jam.` container, Deflate + RFC 8878 Zstandard chunk decompression,
//! schema-driven Kiwi decoder with zero-copy strings/slices and fast field skipping, plus
//! `commandsBlob` and `vectorNetworkBlob` geometry decoders (`T-249`, `T-312`).

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::io::Read;

use flate2::read::DeflateDecoder;
use neoflash_core::{Color, FillRule, GradientKind, GradientStop, PathCommand, TextAlign};

use super::{
    FigmaArcData, FigmaDoc, FigmaDocumentSummary, FigmaEffect, FigmaImportOptions, FigmaNode,
    FigmaPageMapping, FigmaPageSummary, FigmaPaint, FigmaSymbolOverride, FigmaTextData,
    FigmaTextStyleOverride, FigmaTextStyleRun, MAX_FIGMA_DECODE_BYTES, MAX_FIGMA_NODES,
    MAX_KIWI_VALUES, gradient_handles_from_paint_matrix,
};
use crate::ImportError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KiwiDefKind {
    Enum,
    Struct,
    Message,
}

#[derive(Clone, Debug)]
pub(super) struct KiwiField {
    pub name: String,
    pub type_id: i32,
    pub is_array: bool,
    pub value: u32,
}

#[derive(Clone, Debug)]
pub(super) struct KiwiDef {
    pub name: String,
    pub kind: KiwiDefKind,
    pub fields: Vec<KiwiField>,
    /// Fast tag-to-field-index lookup for message field tags `< 512`.
    tag_index: Vec<u16>,
    /// Fast enum value-to-field-index lookup for enum values `< 512`.
    enum_index: Vec<u16>,
}

const NO_IDX: u16 = u16::MAX;

impl KiwiDef {
    fn new(name: String, kind: KiwiDefKind, fields: Vec<KiwiField>) -> Self {
        let mut tag_index = Vec::new();
        let mut enum_index = Vec::new();
        if kind == KiwiDefKind::Message {
            let max_tag = fields
                .iter()
                .map(|f| f.value as usize)
                .filter(|&t| t < 512)
                .max()
                .unwrap_or(0);
            if max_tag > 0 {
                tag_index = vec![NO_IDX; max_tag + 1];
                for (i, f) in fields.iter().enumerate() {
                    if (f.value as usize) < tag_index.len()
                        && let Ok(idx16) = u16::try_from(i)
                    {
                        tag_index[f.value as usize] = idx16;
                    }
                }
            }
        } else if kind == KiwiDefKind::Enum {
            let max_val = fields
                .iter()
                .map(|f| f.value as usize)
                .filter(|&v| v < 512)
                .max()
                .unwrap_or(0);
            enum_index = vec![NO_IDX; max_val + 1];
            for (i, f) in fields.iter().enumerate() {
                if (f.value as usize) < enum_index.len()
                    && let Ok(idx16) = u16::try_from(i)
                {
                    enum_index[f.value as usize] = idx16;
                }
            }
        }
        Self {
            name,
            kind,
            fields,
            tag_index,
            enum_index,
        }
    }

    #[inline]
    fn field_by_tag(&self, tag: u32) -> Option<&KiwiField> {
        let t = tag as usize;
        if t < self.tag_index.len() {
            let idx = self.tag_index[t];
            return (idx != NO_IDX).then(|| &self.fields[idx as usize]);
        }
        self.fields.iter().find(|f| f.value == tag)
    }

    #[inline]
    fn enum_name(&self, val: u32) -> Option<&str> {
        let v = val as usize;
        if v < self.enum_index.len() {
            let idx = self.enum_index[v];
            if idx != NO_IDX {
                return Some(self.fields[idx as usize].name.as_str());
            }
        }
        self.fields
            .iter()
            .find(|f| f.value == val)
            .map(|f| f.name.as_str())
    }
}

/// Compact zero-copy field list borrowing field names from `&'a [KiwiDef]` and byte/string slices
/// from the decompressed Kiwi message buffer.
#[derive(Clone, Debug, Default)]
pub(super) struct KiwiObj<'a>(Vec<(&'a str, KiwiVal<'a>)>);

impl<'a> KiwiObj<'a> {
    #[inline]
    pub(super) fn get(&self, key: &str) -> Option<&KiwiVal<'a>> {
        self.0.iter().rev().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}

#[derive(Clone, Debug)]
pub(super) enum KiwiVal<'a> {
    Bool(bool),
    Byte(u8),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(Cow<'a, str>),
    Bytes(&'a [u8]),
    Enum(&'a str),
    EnumRaw(u32),
    Array(Vec<KiwiVal<'a>>),
    Object(KiwiObj<'a>),
}

impl<'a> KiwiVal<'a> {
    #[inline]
    pub(super) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Int(v) => Some(*v as f64),
            Self::Uint(v) => Some(*v as f64),
            Self::Byte(v) => Some(*v as f64),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Uint(v) => Some(*v),
            Self::Int(v) if *v >= 0 => Some(*v as u64),
            Self::Byte(v) => Some(*v as u64),
            Self::EnumRaw(v) => Some(*v as u64),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_ref()),
            Self::Enum(s) => Some(s),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn as_obj(&self) -> Option<&KiwiObj<'a>> {
        match self {
            Self::Object(m) => Some(m),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn as_array(&self) -> Option<&[KiwiVal<'a>]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    #[inline]
    pub(super) fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

pub(super) struct KiwiReader<'a> {
    buf: &'a [u8],
    pub(super) pos: usize,
}

impl<'a> KiwiReader<'a> {
    pub(super) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    fn read_byte(&mut self) -> Result<u8, String> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| "Unexpected EOF in Kiwi stream".to_owned())?;
        self.pos += 1;
        Ok(b)
    }

    #[inline]
    fn read_var_uint(&mut self) -> Result<u64, String> {
        let mut result = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.read_byte()?;
            if shift == 56 {
                result |= (b as u64) << 56;
                return Ok(result);
            }
            result |= ((b & 0x7F) as u64) << shift;
            if (b & 0x80) == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift > 63 {
                return Err("VarUint overflow in Kiwi stream".into());
            }
        }
    }

    #[inline]
    fn read_var_int(&mut self) -> Result<i64, String> {
        let u = self.read_var_uint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    #[inline]
    fn read_var_float(&mut self) -> Result<f64, String> {
        let first = self.read_byte()?;
        if first == 0 {
            return Ok(0.0);
        }
        let s = self
            .buf
            .get(self.pos..self.pos + 3)
            .ok_or_else(|| "Unexpected EOF in Kiwi float".to_owned())?;
        self.pos += 3;
        let bits =
            (first as u32) | ((s[0] as u32) << 8) | ((s[1] as u32) << 16) | ((s[2] as u32) << 24);
        let ieee = bits.rotate_right(9);
        let f = f32::from_bits(ieee) as f64;
        Ok(if f.is_finite() { f } else { 0.0 })
    }

    #[inline]
    fn skip_cstr(&mut self) -> Result<(), String> {
        let tail = &self.buf[self.pos..];
        let Ok(cstr) = std::ffi::CStr::from_bytes_until_nul(tail) else {
            return Err("Unterminated string in Kiwi stream".into());
        };
        self.pos += cstr.to_bytes_with_nul().len();
        Ok(())
    }

    #[inline]
    fn read_cow_str(&mut self) -> Result<Cow<'a, str>, String> {
        let tail = &self.buf[self.pos..];
        let Ok(cstr) = std::ffi::CStr::from_bytes_until_nul(tail) else {
            return Err("Unterminated string in Kiwi stream".into());
        };
        let slice = cstr.to_bytes();
        self.pos += slice.len() + 1;
        Ok(match std::str::from_utf8(slice) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => Cow::Owned(String::from_utf8_lossy(slice).into_owned()),
        })
    }

    fn read_string(&mut self) -> Result<String, String> {
        Ok(self.read_cow_str()?.into_owned())
    }
}

pub(super) fn parse_kiwi_schema(bytes: &[u8]) -> Result<Vec<KiwiDef>, String> {
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
        defs.push(KiwiDef::new(name, kind, fields));
    }
    Ok(defs)
}

#[inline]
fn is_used_message_field(name: &str) -> bool {
    matches!(name, "nodeChanges" | "blobs" | "blobBaseIndex")
}

#[inline]
fn is_used_node_change_field(name: &str) -> bool {
    matches!(
        name,
        "guid"
            | "guidPath"
            | "overrideKey"
            | "phase"
            | "parentIndex"
            | "type"
            | "name"
            | "visible"
            | "locked"
            | "internalOnly"
            | "isStateGroup"
            | "size"
            | "transform"
            | "opacity"
            | "blendMode"
            | "mask"
            | "maskType"
            | "frameMaskDisabled"
            | "clipsContent"
            | "cornerRadius"
            | "rectangleCornerRadiiIndependent"
            | "rectangleTopLeftCornerRadius"
            | "rectangleTopRightCornerRadius"
            | "rectangleBottomRightCornerRadius"
            | "rectangleBottomLeftCornerRadius"
            | "arcData"
            | "count"
            | "starInnerScale"
            | "fillPaints"
            | "backgroundColor"
            | "backgroundEnabled"
            | "strokePaints"
            | "strokeWeight"
            | "borderStrokeWeightsIndependent"
            | "borderTopWeight"
            | "borderRightWeight"
            | "borderBottomWeight"
            | "borderLeftWeight"
            | "strokeAlign"
            | "strokeCap"
            | "strokeJoin"
            | "dashPattern"
            | "fillGeometry"
            | "strokeGeometry"
            | "vectorData"
            | "effects"
            | "textData"
            | "fontSize"
            | "fontName"
            | "textAlignHorizontal"
            | "textAlignVertical"
            | "textDecoration"
            | "textAutoResize"
            | "letterSpacing"
            | "lineHeight"
            | "symbolData"
            | "overriddenSymbolID"
    )
}

/// Exact schema-driven Kiwi wire skipper for unused heavy fields (`derivedTextData`,
/// `logicalIndexToCharacterOffsetMap`, `pluginData`, `variableConsumptionMap`, etc.), avoiding
/// millions of wasted heap allocations on multi-megabyte `canvas.fig` streams.
fn skip_kiwi_field_value(
    r: &mut KiwiReader<'_>,
    defs: &[KiwiDef],
    type_id: i32,
    is_array: bool,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<(), String> {
    if depth > 64 {
        return Err("Kiwi message nesting exceeds depth 64".into());
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
    if is_array {
        let len = usize::try_from(r.read_var_uint()?)
            .map_err(|_| "Kiwi array length overflow".to_owned())?;
        if len > MAX_KIWI_VALUES {
            return Err("Kiwi array exceeds maximum element limit".into());
        }
        if type_id == -1 || type_id == -2 {
            let end = r
                .pos
                .checked_add(len)
                .ok_or_else(|| "Kiwi byte/bool array overflow".to_owned())?;
            if end > r.buf.len() {
                return Err("Truncated Kiwi byte/bool array".into());
            }
            r.pos = end;
            return Ok(());
        }
        for _ in 0..len {
            skip_kiwi_scalar(r, defs, type_id, depth + 1, budget, active_structs)?;
        }
        return Ok(());
    }
    skip_kiwi_scalar(r, defs, type_id, depth, budget, active_structs)
}

fn skip_kiwi_scalar(
    r: &mut KiwiReader<'_>,
    defs: &[KiwiDef],
    type_id: i32,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<(), String> {
    if depth > 64 {
        return Err("Kiwi message nesting exceeds depth 64".into());
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
    match type_id {
        -1 | -2 => {
            let _ = r.read_byte()?;
            Ok(())
        }
        -3 | -4 | -7 | -8 => {
            let _ = r.read_var_uint()?;
            Ok(())
        }
        -5 => {
            let _ = r.read_var_float()?;
            Ok(())
        }
        -6 => r.skip_cstr(),
        idx if idx >= 0 => {
            let def_idx = idx as usize;
            let def = defs
                .get(def_idx)
                .ok_or_else(|| format!("Invalid Kiwi definition index {idx}"))?;
            match def.kind {
                KiwiDefKind::Enum => {
                    let _ = r.read_var_uint()?;
                    Ok(())
                }
                KiwiDefKind::Struct => {
                    if active_structs.contains(&def_idx) {
                        return Err(format!(
                            "Cyclic Kiwi struct definition '{}' cannot terminate",
                            def.name
                        ));
                    }
                    active_structs.push(def_idx);
                    for field in &def.fields {
                        skip_kiwi_field_value(
                            r,
                            defs,
                            field.type_id,
                            field.is_array,
                            depth + 1,
                            budget,
                            active_structs,
                        )?;
                    }
                    active_structs.pop();
                    Ok(())
                }
                KiwiDefKind::Message => {
                    loop {
                        let fid = r.read_var_uint()?;
                        if fid == 0 {
                            break;
                        }
                        let fid_u32 = u32::try_from(fid)
                            .map_err(|_| format!("Kiwi field tag {fid} out of u32 range"))?;
                        let field = def.field_by_tag(fid_u32).ok_or_else(|| {
                            format!("Unknown field tag {fid} on Kiwi message '{}'", def.name)
                        })?;
                        skip_kiwi_field_value(
                            r,
                            defs,
                            field.type_id,
                            field.is_array,
                            depth + 1,
                            budget,
                            active_structs,
                        )?;
                    }
                    Ok(())
                }
            }
        }
        other => Err(format!("Unsupported Kiwi scalar type_id {other}")),
    }
}

fn decode_kiwi_field_value<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    type_id: i32,
    is_array: bool,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<KiwiVal<'a>, String> {
    if depth > 64 {
        return Err("Kiwi message nesting exceeds depth 64".into());
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
    if is_array {
        let len = usize::try_from(r.read_var_uint()?)
            .map_err(|_| "Kiwi array length overflow".to_owned())?;
        if len > MAX_KIWI_VALUES {
            return Err("Kiwi array exceeds maximum element limit".into());
        }
        if type_id == -2 {
            let end = r
                .pos
                .checked_add(len)
                .ok_or_else(|| "Kiwi byte array overflow".to_owned())?;
            let out = r
                .buf
                .get(r.pos..end)
                .ok_or_else(|| "Truncated Kiwi byte array".to_owned())?;
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

pub(super) fn decode_kiwi_scalar<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    type_id: i32,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<KiwiVal<'a>, String> {
    if depth > 64 {
        return Err("Kiwi message nesting exceeds depth 64".into());
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
    match type_id {
        -1 => Ok(KiwiVal::Bool(r.read_byte()? != 0)),
        -2 => Ok(KiwiVal::Byte(r.read_byte()?)),
        -3 | -7 => Ok(KiwiVal::Int(r.read_var_int()?)),
        -4 | -8 => Ok(KiwiVal::Uint(r.read_var_uint()?)),
        -5 => Ok(KiwiVal::Float(r.read_var_float()?)),
        -6 => Ok(KiwiVal::Str(r.read_cow_str()?)),
        idx if idx >= 0 => {
            let def_idx = idx as usize;
            let def = defs
                .get(def_idx)
                .ok_or_else(|| format!("Invalid Kiwi definition index {idx}"))?;
            match def.kind {
                KiwiDefKind::Enum => {
                    let v = r.read_var_uint()?;
                    if let Ok(vu32) = u32::try_from(v)
                        && let Some(ename) = def.enum_name(vu32)
                    {
                        return Ok(KiwiVal::Enum(ename));
                    }
                    Ok(KiwiVal::EnumRaw(u32::try_from(v).unwrap_or(u32::MAX)))
                }
                KiwiDefKind::Struct => {
                    if active_structs.contains(&def_idx) {
                        return Err(format!(
                            "Cyclic Kiwi struct definition '{}' cannot terminate",
                            def.name
                        ));
                    }
                    active_structs.push(def_idx);
                    let mut pairs = Vec::with_capacity(def.fields.len());
                    for field in &def.fields {
                        let val = decode_kiwi_field_value(
                            r,
                            defs,
                            field.type_id,
                            field.is_array,
                            depth + 1,
                            budget,
                            active_structs,
                        )?;
                        pairs.push((field.name.as_str(), val));
                    }
                    active_structs.pop();
                    Ok(KiwiVal::Object(KiwiObj(pairs)))
                }
                KiwiDefKind::Message => {
                    let filter_msg = def.name == "Message";
                    let filter_nc = def.name == "NodeChange";
                    let mut pairs = Vec::with_capacity(12);
                    loop {
                        let fid = r.read_var_uint()?;
                        if fid == 0 {
                            break;
                        }
                        let fid_u32 = u32::try_from(fid)
                            .map_err(|_| format!("Kiwi field tag {fid} out of u32 range"))?;
                        let field = def.field_by_tag(fid_u32).ok_or_else(|| {
                            format!("Unknown field tag {fid} on Kiwi message '{}'", def.name)
                        })?;
                        let keep = if filter_msg {
                            is_used_message_field(&field.name)
                        } else if filter_nc {
                            is_used_node_change_field(&field.name)
                        } else {
                            true
                        };
                        if !keep {
                            skip_kiwi_field_value(
                                r,
                                defs,
                                field.type_id,
                                field.is_array,
                                depth + 1,
                                budget,
                                active_structs,
                            )?;
                            continue;
                        }
                        let val = decode_kiwi_field_value(
                            r,
                            defs,
                            field.type_id,
                            field.is_array,
                            depth + 1,
                            budget,
                            active_structs,
                        )?;
                        pairs.push((field.name.as_str(), val));
                    }
                    Ok(KiwiVal::Object(KiwiObj(pairs)))
                }
            }
        }
        other => Err(format!("Unsupported Kiwi scalar type_id {other}")),
    }
}

fn decompress_fig_chunk(chunk: &[u8]) -> Result<(Vec<u8>, bool), String> {
    if super::zstd::is_zstd_magic(chunk) {
        let out = super::zstd::decompress_zstd(chunk, MAX_FIGMA_DECODE_BYTES)?;
        return Ok((out, true));
    }
    let mut out = Vec::new();
    if DeflateDecoder::new(chunk)
        .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .is_ok()
        && !out.is_empty()
    {
        if out.len() > MAX_FIGMA_DECODE_BYTES {
            return Err("Decompressed Figma Kiwi chunk exceeds 256 MiB limit".into());
        }
        return Ok((out, false));
    }
    out.clear();
    if flate2::read::ZlibDecoder::new(chunk)
        .take(MAX_FIGMA_DECODE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .is_ok()
        && !out.is_empty()
    {
        if out.len() > MAX_FIGMA_DECODE_BYTES {
            return Err("Decompressed Figma Kiwi chunk exceeds 256 MiB limit".into());
        }
        return Ok((out, false));
    }
    if chunk.len() > MAX_FIGMA_DECODE_BYTES {
        return Err("Uncompressed Figma Kiwi chunk exceeds 256 MiB limit".into());
    }
    Ok((chunk.to_vec(), false))
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

/// Decode Figma binary `vectorNetworkBlob` (`[num_vertices: u32, num_segments: u32, num_regions: u32]`,
/// followed by `num_vertices * 12B` `(style_id, x, y)`, `num_segments * 28B`
/// `(style_id, start, dx1, dy1, end, dx2, dy2)`, and `num_regions` loop lists).
type DecodedVectorNetwork = (Vec<(Vec<PathCommand>, FillRule)>, Vec<Vec<PathCommand>>);

fn decode_figma_vector_network_blob(
    blob: &[u8],
    width: f64,
    height: f64,
    norm_size: Option<(f64, f64)>,
) -> DecodedVectorNetwork {
    if blob.len() < 12 {
        return (Vec::new(), Vec::new());
    }
    let read_u32 = |off: usize| -> Option<u32> {
        let s = blob.get(off..off + 4)?;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    let read_f32 = |off: usize| -> Option<f64> {
        let s = blob.get(off..off + 4)?;
        let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64;
        Some(if v.is_finite() { v } else { 0.0 })
    };

    let (Some(nv_u32), Some(ns_u32), Some(nr_u32)) = (read_u32(0), read_u32(4), read_u32(8)) else {
        return (Vec::new(), Vec::new());
    };
    let nv = nv_u32 as usize;
    let ns = ns_u32 as usize;
    let nr = nr_u32 as usize;
    if nv > 200_000 || ns > 200_000 || nr > 50_000 {
        return (Vec::new(), Vec::new());
    }
    let Some(seg_base) = 12usize.checked_add(nv.saturating_mul(12)) else {
        return (Vec::new(), Vec::new());
    };
    let Some(reg_base) = seg_base.checked_add(ns.saturating_mul(28)) else {
        return (Vec::new(), Vec::new());
    };
    if reg_base > blob.len() || nv == 0 || ns == 0 {
        return (Vec::new(), Vec::new());
    }

    let (sx, sy) = if let Some((nx, ny)) = norm_size {
        let fx = if nx > 1e-6 && width > 0.0 {
            width / nx
        } else {
            1.0
        };
        let fy = if ny > 1e-6 && height > 0.0 {
            height / ny
        } else {
            1.0
        };
        (fx, fy)
    } else {
        (1.0, 1.0)
    };

    let mut verts = Vec::with_capacity(nv);
    for i in 0..nv {
        let off = 12 + i * 12;
        let x = read_f32(off + 4).unwrap_or(0.0) * sx;
        let y = read_f32(off + 8).unwrap_or(0.0) * sy;
        verts.push([x, y]);
    }

    #[derive(Clone, Copy)]
    struct VnSeg {
        start: usize,
        dx1: f64,
        dy1: f64,
        end: usize,
        dx2: f64,
        dy2: f64,
    }

    let mut segs = Vec::with_capacity(ns);
    for i in 0..ns {
        let off = seg_base + i * 28;
        let start = read_u32(off + 4).unwrap_or(0) as usize;
        let dx1 = read_f32(off + 8).unwrap_or(0.0) * sx;
        let dy1 = read_f32(off + 12).unwrap_or(0.0) * sy;
        let end = read_u32(off + 16).unwrap_or(0) as usize;
        let dx2 = read_f32(off + 20).unwrap_or(0.0) * sx;
        let dy2 = read_f32(off + 24).unwrap_or(0.0) * sy;
        if start >= nv || end >= nv {
            return (Vec::new(), Vec::new());
        }
        segs.push(VnSeg {
            start,
            dx1,
            dy1,
            end,
            dx2,
            dy2,
        });
    }

    let emit_seg = |cmds: &mut Vec<PathCommand>, s: VnSeg, forward: bool| {
        let (v0, t0x, t0y, v1, t1x, t1y) = if forward {
            (verts[s.start], s.dx1, s.dy1, verts[s.end], s.dx2, s.dy2)
        } else {
            (verts[s.end], s.dx2, s.dy2, verts[s.start], s.dx1, s.dy1)
        };
        if t0x.abs() <= 1e-7 && t0y.abs() <= 1e-7 && t1x.abs() <= 1e-7 && t1y.abs() <= 1e-7 {
            cmds.push(PathCommand::LineTo { x: v1[0], y: v1[1] });
        } else {
            cmds.push(PathCommand::CubicTo {
                cx1: v0[0] + t0x,
                cy1: v0[1] + t0y,
                cx2: v1[0] + t1x,
                cy2: v1[1] + t1y,
                x: v1[0],
                y: v1[1],
            });
        }
    };

    let mut fill_paths = Vec::new();
    let mut pos = reg_base;
    for _ in 0..nr {
        let Some(style_and_winding) = read_u32(pos) else {
            break;
        };
        let Some(num_loops) = read_u32(pos + 4) else {
            break;
        };
        pos += 8;
        if num_loops > 10_000 {
            break;
        }
        let winding = if (style_and_winding & 1) == 0 {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        };
        let mut region_cmds = Vec::new();
        for _ in 0..num_loops {
            let Some(loop_len_u32) = read_u32(pos) else {
                break;
            };
            pos += 4;
            let loop_len = loop_len_u32 as usize;
            let Some(end_pos) = pos.checked_add(loop_len.saturating_mul(4)) else {
                break;
            };
            if end_pos > blob.len() || loop_len == 0 {
                pos = end_pos.min(blob.len());
                continue;
            }
            let mut loop_segs = Vec::with_capacity(loop_len);
            let mut valid = true;
            for k in 0..loop_len {
                let sidx = read_u32(pos + k * 4).unwrap_or(u32::MAX) as usize;
                if sidx >= segs.len() {
                    valid = false;
                    break;
                }
                loop_segs.push(segs[sidx]);
            }
            pos = end_pos;
            if !valid || loop_segs.is_empty() {
                continue;
            }
            let first_fwd = if loop_segs.len() == 1 {
                true
            } else {
                let s0 = loop_segs[0];
                let s1 = loop_segs[1];
                s0.end == s1.start || s0.end == s1.end
            };
            let mut cur_vert = if first_fwd {
                loop_segs[0].start
            } else {
                loop_segs[0].end
            };
            region_cmds.push(PathCommand::MoveTo {
                x: verts[cur_vert][0],
                y: verts[cur_vert][1],
            });
            for (idx, seg) in loop_segs.into_iter().enumerate() {
                let fwd = if idx == 0 {
                    first_fwd
                } else if seg.start == cur_vert {
                    true
                } else if seg.end == cur_vert {
                    false
                } else {
                    region_cmds.push(PathCommand::MoveTo {
                        x: verts[seg.start][0],
                        y: verts[seg.start][1],
                    });
                    true
                };
                emit_seg(&mut region_cmds, seg, fwd);
                cur_vert = if fwd { seg.end } else { seg.start };
            }
            region_cmds.push(PathCommand::Close);
        }
        if region_cmds.len() >= 2 {
            fill_paths.push((region_cmds, winding));
        }
    }

    let mut stroke_cmds = Vec::with_capacity(ns + 2);
    let mut cur_vert: Option<usize> = None;
    let mut contour_start: Option<usize> = None;
    for seg in &segs {
        let fwd = if cur_vert == Some(seg.start) {
            true
        } else if cur_vert == Some(seg.end) {
            false
        } else {
            if cur_vert.is_some() && cur_vert == contour_start {
                stroke_cmds.push(PathCommand::Close);
            }
            stroke_cmds.push(PathCommand::MoveTo {
                x: verts[seg.start][0],
                y: verts[seg.start][1],
            });
            contour_start = Some(seg.start);
            true
        };
        emit_seg(&mut stroke_cmds, *seg, fwd);
        let next_vert = if fwd { seg.end } else { seg.start };
        cur_vert = Some(next_vert);
        if contour_start == Some(next_vert) {
            stroke_cmds.push(PathCommand::Close);
            cur_vert = None;
            contour_start = None;
        }
    }
    let stroke_paths = if stroke_cmds.len() >= 2 {
        vec![stroke_cmds]
    } else {
        Vec::new()
    };

    (fill_paths, stroke_paths)
}

fn guid_string(obj: &KiwiObj<'_>) -> Option<String> {
    let s = obj.get("sessionID")?.as_u64()?;
    let l = obj.get("localID")?.as_u64()?;
    Some(format!("{s}:{l}"))
}

fn kiwi_color_to_rgba(obj: &KiwiObj<'_>, extra_opacity: f64) -> Color {
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

fn parse_kiwi_matrix6(obj: &KiwiObj<'_>) -> Option<[f64; 6]> {
    let m00 = obj.get("m00")?.as_f64()?;
    let m01 = obj.get("m01").and_then(KiwiVal::as_f64).unwrap_or(0.0);
    let m02 = obj.get("m02").and_then(KiwiVal::as_f64).unwrap_or(0.0);
    let m10 = obj.get("m10").and_then(KiwiVal::as_f64).unwrap_or(0.0);
    let m11 = obj.get("m11")?.as_f64()?;
    let m12 = obj.get("m12").and_then(KiwiVal::as_f64).unwrap_or(0.0);
    Some([m00, m01, m02, m10, m11, m12])
}

fn parse_kiwi_paint_list(arr: &[KiwiVal<'_>], w: f64, h: f64) -> Vec<FigmaPaint> {
    let mut out = Vec::new();
    for item in arr {
        let Some(p) = item.as_obj() else { continue };
        if !p.get("visible").and_then(KiwiVal::as_bool).unwrap_or(true) {
            continue;
        }
        let ptype = p.get("type").and_then(KiwiVal::as_str).unwrap_or("SOLID");
        let opacity = p
            .get("opacity")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        match ptype {
            "SOLID" => {
                if let Some(c) = p.get("color").and_then(KiwiVal::as_obj) {
                    out.push(FigmaPaint::Solid {
                        color: kiwi_color_to_rgba(c, opacity),
                    });
                }
            }
            "GRADIENT_LINEAR" | "GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND" => {
                let mut stops = Vec::new();
                let stop_source = p
                    .get("stops")
                    .and_then(KiwiVal::as_array)
                    .filter(|s| !s.is_empty())
                    .or_else(|| p.get("stopsVar").and_then(KiwiVal::as_array));
                if let Some(sarr) = stop_source {
                    for s in sarr {
                        if let Some(so) = s.as_obj() {
                            let pos = so
                                .get("position")
                                .and_then(KiwiVal::as_f64)
                                .unwrap_or(0.0)
                                .clamp(0.0, 1.0);
                            if let Some(co) = so.get("color").and_then(KiwiVal::as_obj) {
                                stops.push(GradientStop {
                                    ratio: (pos * 255.0).round() as u8,
                                    color: kiwi_color_to_rgba(co, opacity),
                                });
                            }
                        }
                    }
                }
                if stops.len() >= 2 {
                    let radial = ptype != "GRADIENT_LINEAR";
                    let (start, end) = if let Some(mat) = p
                        .get("transform")
                        .and_then(KiwiVal::as_obj)
                        .and_then(parse_kiwi_matrix6)
                    {
                        gradient_handles_from_paint_matrix(mat, w, h, radial)
                    } else if radial {
                        ([w * 0.5, h * 0.5], [w, h * 0.5])
                    } else {
                        ([0.0, h * 0.5], [w, h * 0.5])
                    };
                    let kind = if radial {
                        GradientKind::Radial
                    } else {
                        GradientKind::Linear
                    };
                    let approximated_from = match ptype {
                        "GRADIENT_ANGULAR" => Some("GRADIENT_ANGULAR"),
                        "GRADIENT_DIAMOND" => Some("GRADIENT_DIAMOND"),
                        _ => None,
                    };
                    out.push(FigmaPaint::Gradient {
                        kind,
                        stops,
                        start,
                        end,
                        approximated_from,
                    });
                }
            }
            "IMAGE" | "VIDEO" => {
                let img_obj = p
                    .get("image")
                    .or_else(|| p.get("imageThumbnail"))
                    .and_then(KiwiVal::as_obj);
                let mut hash_hex = String::new();
                if let Some(img) = img_obj
                    && let Some(hv) = img.get("hash")
                {
                    if let Some(bytes) = hv.as_bytes() {
                        hash_hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    } else if let Some(arr) = hv.as_array() {
                        hash_hex = arr
                            .iter()
                            .filter_map(KiwiVal::as_u64)
                            .map(|b| format!("{:02x}", b as u8))
                            .collect();
                    } else if let Some(s) = hv.as_str() {
                        hash_hex = s.to_ascii_lowercase();
                    }
                }
                if !hash_hex.is_empty() {
                    let scale_mode = p
                        .get("imageScaleMode")
                        .and_then(KiwiVal::as_str)
                        .unwrap_or("FILL")
                        .to_owned();
                    out.push(FigmaPaint::Image {
                        image_ref: hash_hex,
                        scale_mode,
                        alpha: opacity,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_kiwi_number_px<'a>(
    val: Option<&'a KiwiVal<'a>>,
    font_size: f64,
) -> Option<(f64, &'a str)> {
    let v = val?;
    if let Some(num) = v.as_f64() {
        return num.is_finite().then_some((num, "PIXELS"));
    }
    let obj = v.as_obj()?;
    let raw = obj.get("value")?.as_f64()?;
    if !raw.is_finite() {
        return None;
    }
    let units = obj
        .get("units")
        .and_then(KiwiVal::as_str)
        .unwrap_or("PIXELS");
    match units {
        "PERCENT" => Some((font_size * raw / 100.0, "PERCENT")),
        "RAW" => Some((font_size * raw, "RAW")),
        _ => Some((raw, "PIXELS")),
    }
}

fn parse_kiwi_effects(obj: &KiwiObj<'_>) -> Vec<FigmaEffect> {
    let mut effects = Vec::new();
    let Some(arr) = obj.get("effects").and_then(KiwiVal::as_array) else {
        return effects;
    };
    for item in arr {
        let Some(eo) = item.as_obj() else { continue };
        if !eo.get("visible").and_then(KiwiVal::as_bool).unwrap_or(true) {
            continue;
        }
        let etype = eo.get("type").and_then(KiwiVal::as_str).unwrap_or("");
        let radius = eo
            .get("radius")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(0.0)
            .max(0.0);
        match etype {
            "DROP_SHADOW" | "INNER_SHADOW" => {
                let color = eo
                    .get("color")
                    .and_then(KiwiVal::as_obj)
                    .map(|c| kiwi_color_to_rgba(c, 1.0))
                    .unwrap_or([0, 0, 0, 64]);
                let (ox, oy) = eo
                    .get("offset")
                    .and_then(KiwiVal::as_obj)
                    .map(|o| {
                        (
                            o.get("x").and_then(KiwiVal::as_f64).unwrap_or(0.0),
                            o.get("y").and_then(KiwiVal::as_f64).unwrap_or(0.0),
                        )
                    })
                    .unwrap_or((0.0, 2.0));
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
    effects
}

fn parse_kiwi_style_override(so: &KiwiObj<'_>, base_font_size: f64) -> FigmaTextStyleOverride {
    let mut out = FigmaTextStyleOverride::default();
    if let Some(fs) = so
        .get("fontSize")
        .and_then(KiwiVal::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
    {
        out.font_size = Some(fs);
    }
    let effective_fs = out.font_size.unwrap_or(base_font_size);
    if let Some(fn_obj) = so.get("fontName").and_then(KiwiVal::as_obj) {
        if let Some(fam) = fn_obj
            .get("family")
            .and_then(KiwiVal::as_str)
            .filter(|s| !s.is_empty())
        {
            out.font_family = Some(fam.to_owned());
        }
        if let Some(st) = fn_obj.get("style").and_then(KiwiVal::as_str) {
            let lower = st.to_ascii_lowercase();
            out.bold = Some(
                lower.contains("bold")
                    || lower.contains("black")
                    || lower.contains("heavy")
                    || lower.contains("semibold")
                    || lower.contains("demibold"),
            );
            out.italic = Some(lower.contains("italic") || lower.contains("oblique"));
        }
    }
    if let Some(dec) = so.get("textDecoration").and_then(KiwiVal::as_str) {
        out.underline = Some(dec == "UNDERLINE");
        out.strikethrough = Some(dec == "STRIKETHROUGH");
    }
    if let Some(fp_arr) = so.get("fillPaints").and_then(KiwiVal::as_array) {
        for p in parse_kiwi_paint_list(fp_arr, 100.0, 20.0) {
            if let FigmaPaint::Solid { color } = p {
                out.fill_color = Some(color);
                break;
            }
        }
    }
    if let Some((ls, _)) = parse_kiwi_number_px(so.get("letterSpacing"), effective_fs) {
        out.letter_spacing = Some(ls);
    }
    if let Some((lh, _)) = parse_kiwi_number_px(so.get("lineHeight"), effective_fs)
        && lh > 0.0
    {
        out.line_height = Some(lh);
    }
    out
}

pub(super) fn build_text_style_runs(
    raw_chars: &str,
    char_style_ids: &[u64],
    override_map: &HashMap<u64, FigmaTextStyleOverride>,
) -> Vec<FigmaTextStyleRun> {
    if char_style_ids.is_empty() || override_map.is_empty() || raw_chars.is_empty() {
        return Vec::new();
    }
    let raw_utf16: Vec<u16> = raw_chars.encode_utf16().collect();
    let mut runs: Vec<(u32, u32, u64)> = Vec::new();
    let mut norm_u16_pos = 0u32;
    let mut i = 0usize;
    while i < raw_utf16.len() {
        let sid = char_style_ids.get(i).copied().unwrap_or(0);
        if raw_utf16[i] == b'\r' as u16 && raw_utf16.get(i + 1) == Some(&(b'\n' as u16)) {
            i += 2;
        } else {
            i += 1;
        }
        let next_pos = norm_u16_pos + 1;
        if sid != 0 && override_map.contains_key(&sid) {
            if let Some(last) = runs.last_mut()
                && last.2 == sid
                && last.1 == norm_u16_pos
            {
                last.1 = next_pos;
            } else {
                runs.push((norm_u16_pos, next_pos, sid));
            }
        }
        norm_u16_pos = next_pos;
    }

    runs.into_iter()
        .filter_map(|(start_utf16, end_utf16, sid)| {
            let style = override_map.get(&sid)?.clone();
            Some(FigmaTextStyleRun {
                start_utf16,
                end_utf16,
                style,
            })
        })
        .collect()
}

fn is_non_visual_kiwi_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "VARIABLE"
            | "VARIABLE_SET"
            | "STYLE"
            | "SLICE"
            | "CONSTRAINTS"
            | "GUIDE"
            | "MANAGED_STRING"
            | "PLUGIN_DATA"
            | "WIDGET_SYNCED_STATE"
    )
}

fn resolve_kiwi_blob<'a>(
    val: &'a KiwiVal<'a>,
    blobs: &[&'a [u8]],
    blob_base: u64,
) -> Option<&'a [u8]> {
    if let Some(inline) = val.as_bytes() {
        return Some(inline);
    }
    let idx = val.as_u64()?.checked_sub(blob_base)? as usize;
    blobs.get(idx).copied()
}

fn is_top_frame_kiwi_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "FRAME" | "SECTION" | "COMPONENT" | "COMPONENT_SET" | "SYMBOL"
    )
}

#[inline]
fn format_guid_pair(g: (u64, u64)) -> String {
    format!("{}:{}", g.0, g.1)
}

struct ScannedKiwiNode<'a> {
    guid: (u64, u64),
    parent_guid: Option<(u64, u64)>,
    position: Cow<'a, str>,
    node_type: Cow<'a, str>,
    name: Cow<'a, str>,
    visible: bool,
    internal_only: bool,
    is_state_group: bool,
    ref_symbols: Vec<(u64, u64)>,
    raw_slice: &'a [u8],
}

struct ScannedKiwiStream<'a> {
    blob_base: u64,
    blobs: Vec<&'a [u8]>,
    nc_def_idx: usize,
    nodes: Vec<ScannedKiwiNode<'a>>,
    idx_by_guid: HashMap<(u64, u64), usize>,
    doc_idx: Option<usize>,
    doc_name: String,
}

fn read_guid_value<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    type_id: i32,
    is_array: bool,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<Option<(u64, u64)>, String> {
    if !is_array
        && type_id >= 0
        && let Some(def) = defs.get(type_id as usize)
        && def.kind == KiwiDefKind::Struct
        && def.fields.len() == 2
        && def.fields[0].name == "sessionID"
        && matches!(def.fields[0].type_id, -4 | -8)
        && !def.fields[0].is_array
        && def.fields[1].name == "localID"
        && matches!(def.fields[1].type_id, -4 | -8)
        && !def.fields[1].is_array
    {
        *budget = budget
            .checked_sub(2)
            .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
        let s = r.read_var_uint()?;
        let l = r.read_var_uint()?;
        return Ok(Some((s, l)));
    }
    let val = decode_kiwi_field_value(r, defs, type_id, is_array, depth, budget, active_structs)?;
    let Some(obj) = val.as_obj() else {
        return Ok(None);
    };
    let (Some(s), Some(l)) = (
        obj.get("sessionID").and_then(KiwiVal::as_u64),
        obj.get("localID").and_then(KiwiVal::as_u64),
    ) else {
        return Ok(None);
    };
    Ok(Some((s, l)))
}

type KiwiParentIndex<'a> = (Option<(u64, u64)>, Cow<'a, str>);

fn read_parent_index_value<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    type_id: i32,
    is_array: bool,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<KiwiParentIndex<'a>, String> {
    if !is_array
        && type_id >= 0
        && let Some(def) = defs.get(type_id as usize)
        && def.kind == KiwiDefKind::Struct
        && def.fields.len() == 2
        && def.fields[0].name == "guid"
        && !def.fields[0].is_array
        && def.fields[1].name == "position"
        && def.fields[1].type_id == -6
        && !def.fields[1].is_array
    {
        let pguid = read_guid_value(
            r,
            defs,
            def.fields[0].type_id,
            false,
            depth + 1,
            budget,
            active_structs,
        )?;
        *budget = budget
            .checked_sub(1)
            .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
        let pos = r.read_cow_str()?;
        return Ok((pguid, pos));
    }
    let val = decode_kiwi_field_value(r, defs, type_id, is_array, depth, budget, active_structs)?;
    if let Some(pi) = val.as_obj() {
        let pguid = pi
            .get("guid")
            .and_then(KiwiVal::as_obj)
            .and_then(|go| Some((go.get("sessionID")?.as_u64()?, go.get("localID")?.as_u64()?)));
        let pos = pi
            .get("position")
            .and_then(|v| match v {
                KiwiVal::Str(s) => Some(s.clone()),
                KiwiVal::Enum(s) => Some(Cow::Borrowed(*s)),
                _ => None,
            })
            .unwrap_or(Cow::Borrowed(""));
        Ok((pguid, pos))
    } else {
        Ok((None, Cow::Borrowed("")))
    }
}

fn read_str_or_enum_fast<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    type_id: i32,
    is_array: bool,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<Option<Cow<'a, str>>, String> {
    if !is_array {
        if type_id == -6 {
            *budget = budget
                .checked_sub(1)
                .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
            return Ok(Some(r.read_cow_str()?));
        }
        if type_id >= 0
            && let Some(def) = defs.get(type_id as usize)
            && def.kind == KiwiDefKind::Enum
        {
            *budget = budget
                .checked_sub(1)
                .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
            let v = r.read_var_uint()?;
            if let Ok(vu32) = u32::try_from(v)
                && let Some(ename) = def.enum_name(vu32)
            {
                return Ok(Some(Cow::Borrowed(ename)));
            }
            return Ok(None);
        }
    }
    let val = decode_kiwi_field_value(r, defs, type_id, is_array, depth, budget, active_structs)?;
    Ok(match val {
        KiwiVal::Str(s) => Some(s),
        KiwiVal::Enum(s) => Some(Cow::Borrowed(s)),
        _ => None,
    })
}

fn scan_symbol_data_refs<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    sd_field: &KiwiField,
    depth: usize,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
    out_refs: &mut Vec<(u64, u64)>,
) -> Result<(), String> {
    let type_id = sd_field.type_id;
    let is_array = sd_field.is_array;
    if is_array || type_id < 0 {
        return skip_kiwi_field_value(r, defs, type_id, is_array, depth, budget, active_structs);
    }
    let sd_def = defs
        .get(type_id as usize)
        .ok_or_else(|| format!("Invalid Kiwi SymbolData definition index {type_id}"))?;
    if sd_def.kind != KiwiDefKind::Message {
        return skip_kiwi_field_value(r, defs, type_id, is_array, depth, budget, active_structs);
    }
    loop {
        let fid = r.read_var_uint()?;
        if fid == 0 {
            break;
        }
        let fid_u32 =
            u32::try_from(fid).map_err(|_| format!("Kiwi field tag {fid} out of u32 range"))?;
        let field = sd_def
            .field_by_tag(fid_u32)
            .ok_or_else(|| format!("Unknown field tag {fid} on Kiwi message '{}'", sd_def.name))?;
        match field.name.as_str() {
            "symbolID" => {
                if let Some(g) = read_guid_value(
                    r,
                    defs,
                    field.type_id,
                    field.is_array,
                    depth + 1,
                    budget,
                    active_structs,
                )? {
                    out_refs.push(g);
                }
            }
            "symbolOverrides"
                if field.is_array
                    && field.type_id >= 0
                    && defs
                        .get(field.type_id as usize)
                        .is_some_and(|d| d.kind == KiwiDefKind::Message) =>
            {
                let ov_def = &defs[field.type_id as usize];
                let ov_len = usize::try_from(r.read_var_uint()?)
                    .map_err(|_| "Kiwi symbolOverrides length overflow".to_owned())?;
                if ov_len > MAX_KIWI_VALUES {
                    return Err("Kiwi symbolOverrides exceeds maximum element limit".into());
                }
                for _ in 0..ov_len {
                    loop {
                        let ofid = r.read_var_uint()?;
                        if ofid == 0 {
                            break;
                        }
                        let ofid_u32 = u32::try_from(ofid)
                            .map_err(|_| format!("Kiwi field tag {ofid} out of u32 range"))?;
                        let ofield = ov_def.field_by_tag(ofid_u32).ok_or_else(|| {
                            format!("Unknown field tag {ofid} on Kiwi message '{}'", ov_def.name)
                        })?;
                        if ofield.name == "overriddenSymbolID" {
                            if let Some(g) = read_guid_value(
                                r,
                                defs,
                                ofield.type_id,
                                ofield.is_array,
                                depth + 2,
                                budget,
                                active_structs,
                            )? {
                                out_refs.push(g);
                            }
                        } else {
                            skip_kiwi_field_value(
                                r,
                                defs,
                                ofield.type_id,
                                ofield.is_array,
                                depth + 2,
                                budget,
                                active_structs,
                            )?;
                        }
                    }
                }
            }
            _ => {
                skip_kiwi_field_value(
                    r,
                    defs,
                    field.type_id,
                    field.is_array,
                    depth + 1,
                    budget,
                    active_structs,
                )?;
            }
        }
    }
    Ok(())
}

fn scan_blobs_array<'a>(
    r: &mut KiwiReader<'a>,
    defs: &'a [KiwiDef],
    type_id: i32,
    is_array: bool,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<Vec<&'a [u8]>, String> {
    if !is_array || type_id < 0 {
        skip_kiwi_field_value(r, defs, type_id, is_array, 1, budget, active_structs)?;
        return Ok(Vec::new());
    }
    let len = usize::try_from(r.read_var_uint()?)
        .map_err(|_| "Kiwi blobs array length overflow".to_owned())?;
    if len > MAX_KIWI_VALUES {
        return Err("Kiwi blobs array exceeds maximum element limit".into());
    }
    let blob_def = defs
        .get(type_id as usize)
        .ok_or_else(|| format!("Invalid Kiwi Blob definition index {type_id}"))?;
    let mut blobs = Vec::with_capacity(len.min(65_536));
    if blob_def.kind == KiwiDefKind::Message {
        for _ in 0..len {
            *budget = budget
                .checked_sub(1)
                .ok_or_else(|| "Kiwi stream exceeds decoded value safety budget".to_owned())?;
            let mut slice: &'a [u8] = &[];
            loop {
                let fid = r.read_var_uint()?;
                if fid == 0 {
                    break;
                }
                let fid_u32 = u32::try_from(fid)
                    .map_err(|_| format!("Kiwi field tag {fid} out of u32 range"))?;
                let field = blob_def.field_by_tag(fid_u32).ok_or_else(|| {
                    format!(
                        "Unknown field tag {fid} on Kiwi message '{}'",
                        blob_def.name
                    )
                })?;
                if field.name == "bytes" && field.type_id == -2 && field.is_array {
                    let blen = usize::try_from(r.read_var_uint()?)
                        .map_err(|_| "Kiwi blob byte array length overflow".to_owned())?;
                    let end = r
                        .pos
                        .checked_add(blen)
                        .ok_or_else(|| "Kiwi blob byte array overflow".to_owned())?;
                    slice = r
                        .buf
                        .get(r.pos..end)
                        .ok_or_else(|| "Truncated Kiwi blob byte array".to_owned())?;
                    r.pos = end;
                } else {
                    skip_kiwi_field_value(
                        r,
                        defs,
                        field.type_id,
                        field.is_array,
                        2,
                        budget,
                        active_structs,
                    )?;
                }
            }
            blobs.push(slice);
        }
    } else {
        for _ in 0..len {
            let val = decode_kiwi_scalar(r, defs, type_id, 2, budget, active_structs)?;
            let slice = val
                .as_obj()
                .and_then(|bo| bo.get("bytes"))
                .and_then(KiwiVal::as_bytes)
                .unwrap_or(&[]);
            blobs.push(slice);
        }
    }
    Ok(blobs)
}

struct DecompressedKiwiContainer {
    version: u32,
    is_figjam: bool,
    any_zstd: bool,
    schema_chunk: Vec<u8>,
    msg_chunk: Vec<u8>,
}

fn decompress_kiwi_container(bytes: &[u8]) -> Result<DecompressedKiwiContainer, String> {
    if bytes.len() < 20 {
        return Err("Figma Kiwi stream is shorter than 20-byte header".into());
    }
    let is_figjam = bytes.starts_with(b"fig-jam.");
    if !bytes.starts_with(b"fig-kiwi") && !is_figjam {
        return Err("Missing fig-kiwi / fig-jam. magic header".into());
    }
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

    let mut pos = 12usize;
    let mut chunks = Vec::with_capacity(2);
    let mut any_zstd = false;
    while pos + 4 <= bytes.len() && chunks.len() < 2 {
        let len = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        pos += 4;
        if len > MAX_FIGMA_DECODE_BYTES {
            return Err("Figma Kiwi chunk length exceeds 256 MiB limit".into());
        }
        let end = pos
            .checked_add(len)
            .ok_or_else(|| "Figma Kiwi chunk length overflow".to_owned())?;
        let chunk_slice = bytes
            .get(pos..end)
            .ok_or_else(|| "Truncated Figma Kiwi chunk payload".to_owned())?;
        pos = end;
        let (decompressed, used_zstd) = decompress_fig_chunk(chunk_slice)?;
        any_zstd |= used_zstd;
        chunks.push(decompressed);
    }

    if chunks.len() < 2 {
        return Err("Figma Kiwi stream requires at least schema and message chunks".into());
    }
    let msg_chunk = chunks.pop().unwrap();
    let schema_chunk = chunks.pop().unwrap();
    Ok(DecompressedKiwiContainer {
        version,
        is_figjam,
        any_zstd,
        schema_chunk,
        msg_chunk,
    })
}

fn scan_kiwi_message_pass1<'a>(
    msg_buf: &'a [u8],
    defs: &'a [KiwiDef],
    default_name: &str,
    want_blobs: bool,
    want_refs: bool,
) -> Result<ScannedKiwiStream<'a>, String> {
    let msg_def_idx = defs
        .iter()
        .position(|d| d.name == "Message" && d.kind == KiwiDefKind::Message)
        .or_else(|| defs.iter().rposition(|d| d.kind == KiwiDefKind::Message))
        .ok_or_else(|| "No root Message definition found in Kiwi schema".to_owned())?;
    let msg_def = &defs[msg_def_idx];

    let mut r = KiwiReader::new(msg_buf);
    let mut budget = MAX_KIWI_VALUES;
    let mut active_structs = Vec::new();

    let mut blob_base = 0u64;
    let mut blobs: Vec<&'a [u8]> = Vec::new();
    let mut saw_node_changes = false;
    let mut nc_def_idx = 0usize;
    let mut nodes: Vec<ScannedKiwiNode<'a>> = Vec::new();
    let mut idx_by_guid: HashMap<(u64, u64), usize> = HashMap::new();
    let mut doc_idx: Option<usize> = None;
    let mut doc_name = default_name.to_owned();

    loop {
        let fid = r.read_var_uint()?;
        if fid == 0 {
            break;
        }
        let fid_u32 =
            u32::try_from(fid).map_err(|_| format!("Kiwi field tag {fid} out of u32 range"))?;
        let mfield = msg_def
            .field_by_tag(fid_u32)
            .ok_or_else(|| format!("Unknown field tag {fid} on Kiwi message '{}'", msg_def.name))?;
        match mfield.name.as_str() {
            "blobBaseIndex" if !want_blobs => {
                skip_kiwi_field_value(
                    &mut r,
                    defs,
                    mfield.type_id,
                    mfield.is_array,
                    1,
                    &mut budget,
                    &mut active_structs,
                )?;
            }
            "blobBaseIndex" => {
                let val = decode_kiwi_field_value(
                    &mut r,
                    defs,
                    mfield.type_id,
                    mfield.is_array,
                    1,
                    &mut budget,
                    &mut active_structs,
                )?;
                blob_base = val.as_u64().unwrap_or(0);
            }
            "blobs" if !want_blobs => {
                skip_kiwi_field_value(
                    &mut r,
                    defs,
                    mfield.type_id,
                    mfield.is_array,
                    1,
                    &mut budget,
                    &mut active_structs,
                )?;
            }
            "blobs" => {
                blobs = scan_blobs_array(
                    &mut r,
                    defs,
                    mfield.type_id,
                    mfield.is_array,
                    &mut budget,
                    &mut active_structs,
                )?;
            }
            "nodeChanges" => {
                if !mfield.is_array || mfield.type_id < 0 {
                    return Err("Kiwi nodeChanges field is not a message array".into());
                }
                nc_def_idx = mfield.type_id as usize;
                let nc_def = defs
                    .get(nc_def_idx)
                    .ok_or_else(|| format!("Invalid NodeChange definition index {nc_def_idx}"))?;
                if nc_def.kind != KiwiDefKind::Message {
                    return Err("Kiwi NodeChange definition must be a Message".into());
                }
                let nc_len = usize::try_from(r.read_var_uint()?)
                    .map_err(|_| "Kiwi nodeChanges length overflow".to_owned())?;
                if nc_len > MAX_FIGMA_NODES {
                    return Err(format!(
                        "Figma document exceeds maximum node count ({MAX_FIGMA_NODES})"
                    ));
                }
                saw_node_changes = true;
                nodes.reserve(nc_len.min(65_536));
                idx_by_guid.reserve(nc_len.min(65_536));

                for _ in 0..nc_len {
                    budget = budget.checked_sub(1).ok_or_else(|| {
                        "Kiwi stream exceeds decoded value safety budget".to_owned()
                    })?;
                    let slice_start = r.pos;
                    let mut removed = false;
                    let mut guid: Option<(u64, u64)> = None;
                    let mut parent_guid: Option<(u64, u64)> = None;
                    let mut position: Cow<'a, str> = Cow::Borrowed("");
                    let mut node_type: Cow<'a, str> = Cow::Borrowed("FRAME");
                    let mut has_name = false;
                    let mut name: Cow<'a, str> = Cow::Borrowed("");
                    let mut visible = true;
                    let mut internal_only = false;
                    let mut is_state_group = false;
                    let mut ref_symbols: Vec<(u64, u64)> = Vec::new();

                    loop {
                        let nfid = r.read_var_uint()?;
                        if nfid == 0 {
                            break;
                        }
                        let nfid_u32 = u32::try_from(nfid)
                            .map_err(|_| format!("Kiwi field tag {nfid} out of u32 range"))?;
                        let field = nc_def.field_by_tag(nfid_u32).ok_or_else(|| {
                            format!("Unknown field tag {nfid} on Kiwi message '{}'", nc_def.name)
                        })?;
                        match field.name.as_str() {
                            "phase" => {
                                if let Some(ph) = read_str_or_enum_fast(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )? && ph == "REMOVED"
                                {
                                    removed = true;
                                }
                            }
                            "guid" => {
                                guid = read_guid_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )?;
                            }
                            "parentIndex" => {
                                let (pg, pos) = read_parent_index_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )?;
                                parent_guid = pg;
                                position = pos;
                            }
                            "type" => {
                                if let Some(nt) = read_str_or_enum_fast(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )? {
                                    node_type = nt;
                                }
                            }
                            "name" => {
                                if let Some(nm) = read_str_or_enum_fast(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )? {
                                    has_name = true;
                                    name = nm;
                                }
                            }
                            "visible" if !field.is_array && field.type_id == -1 => {
                                visible = r.read_byte()? != 0;
                            }
                            "visible" => {
                                let v = decode_kiwi_field_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )?;
                                visible = v.as_bool().unwrap_or(true);
                            }
                            "internalOnly" if !field.is_array && field.type_id == -1 => {
                                internal_only = r.read_byte()? != 0;
                            }
                            "internalOnly" => {
                                let v = decode_kiwi_field_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )?;
                                internal_only = v.as_bool().unwrap_or(false);
                            }
                            "isStateGroup"
                                if want_refs && !field.is_array && field.type_id == -1 =>
                            {
                                is_state_group = r.read_byte()? != 0;
                            }
                            "isStateGroup" if want_refs => {
                                let v = decode_kiwi_field_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )?;
                                is_state_group = v.as_bool().unwrap_or(false);
                            }
                            "overriddenSymbolID" if want_refs => {
                                if let Some(g) = read_guid_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )? {
                                    ref_symbols.push(g);
                                }
                            }
                            "symbolData" if want_refs => {
                                scan_symbol_data_refs(
                                    &mut r,
                                    defs,
                                    field,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                    &mut ref_symbols,
                                )?;
                            }
                            _ => {
                                skip_kiwi_field_value(
                                    &mut r,
                                    defs,
                                    field.type_id,
                                    field.is_array,
                                    2,
                                    &mut budget,
                                    &mut active_structs,
                                )?;
                            }
                        }
                    }

                    let slice_end = r.pos;
                    if removed || is_non_visual_kiwi_node_type(&node_type) {
                        continue;
                    }
                    let Some(guid_pair) = guid else { continue };
                    if !has_name {
                        name = node_type.clone();
                    }
                    internal_only = internal_only
                        || node_type == "INTERNAL_ONLY_CANVAS"
                        || (node_type == "CANVAS" && name == "Internal Only Canvas");
                    is_state_group = is_state_group || node_type == "COMPONENT_SET";

                    if node_type == "DOCUMENT"
                        && (doc_name.is_empty() || doc_name == "Figma Import")
                        && !name.is_empty()
                        && name != "Document"
                        && name != "Doc"
                    {
                        doc_name = name.clone().into_owned();
                    }

                    let scanned = ScannedKiwiNode {
                        guid: guid_pair,
                        parent_guid,
                        position,
                        node_type,
                        name,
                        visible,
                        internal_only,
                        is_state_group,
                        ref_symbols,
                        raw_slice: &msg_buf[slice_start..slice_end],
                    };
                    let is_doc = scanned.node_type == "DOCUMENT";
                    if let Some(&existing_idx) = idx_by_guid.get(&guid_pair) {
                        nodes[existing_idx] = scanned;
                        if is_doc {
                            doc_idx = Some(existing_idx);
                        }
                    } else {
                        let new_idx = nodes.len();
                        nodes.push(scanned);
                        idx_by_guid.insert(guid_pair, new_idx);
                        if is_doc {
                            doc_idx = Some(new_idx);
                        }
                    }
                }
                if !want_blobs {
                    break;
                }
            }
            _ => {
                skip_kiwi_field_value(
                    &mut r,
                    defs,
                    mfield.type_id,
                    mfield.is_array,
                    1,
                    &mut budget,
                    &mut active_structs,
                )?;
            }
        }
    }

    if !saw_node_changes {
        return Err("Kiwi Message does not contain nodeChanges".into());
    }

    Ok(ScannedKiwiStream {
        blob_base,
        blobs,
        nc_def_idx,
        nodes,
        idx_by_guid,
        doc_idx,
        doc_name,
    })
}

fn build_scanned_children_graph(scanned: &ScannedKiwiStream<'_>) -> (Vec<Vec<usize>>, Vec<usize>) {
    let n = scanned.nodes.len();
    let mut children_by_parent: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (idx, node) in scanned.nodes.iter().enumerate() {
        if let Some(pg) = node.parent_guid
            && let Some(&pidx) = scanned.idx_by_guid.get(&pg)
            && pidx != idx
        {
            children_by_parent[pidx].push(idx);
        }
    }
    for list in &mut children_by_parent {
        if list.len() > 1 {
            list.sort_by(|&a, &b| {
                scanned.nodes[a]
                    .position
                    .as_ref()
                    .cmp(scanned.nodes[b].position.as_ref())
                    .then_with(|| scanned.nodes[a].guid.cmp(&scanned.nodes[b].guid))
            });
        }
    }

    let mut page_indices = Vec::new();
    if let Some(didx) = scanned.doc_idx {
        for &cidx in &children_by_parent[didx] {
            let cn = &scanned.nodes[cidx];
            if cn.node_type == "CANVAS" || cn.internal_only || !children_by_parent[cidx].is_empty()
            {
                page_indices.push(cidx);
            }
        }
    }
    if page_indices.is_empty() {
        let mut canvas_list: Vec<usize> = (0..n)
            .filter(|&i| scanned.nodes[i].node_type == "CANVAS")
            .collect();
        canvas_list.sort_by(|&a, &b| {
            scanned.nodes[a]
                .position
                .as_ref()
                .cmp(scanned.nodes[b].position.as_ref())
                .then_with(|| scanned.nodes[a].guid.cmp(&scanned.nodes[b].guid))
        });
        page_indices = canvas_list;
    }

    (children_by_parent, page_indices)
}

fn count_scanned_subtree(idx: usize, children_by_parent: &[Vec<usize>], depth: usize) -> usize {
    if depth > 128 {
        return 0;
    }
    let mut total = 1usize;
    for &cidx in &children_by_parent[idx] {
        total = total.saturating_add(count_scanned_subtree(cidx, children_by_parent, depth + 1));
    }
    total
}

pub(super) fn inspect_fig_kiwi_canvas(
    bytes: &[u8],
    default_name: &str,
) -> Result<FigmaDocumentSummary, ImportError> {
    inspect_fig_kiwi_stream(bytes, default_name).map_err(ImportError::Container)
}

fn inspect_fig_kiwi_stream(
    bytes: &[u8],
    default_name: &str,
) -> Result<FigmaDocumentSummary, String> {
    let DecompressedKiwiContainer {
        version,
        is_figjam,
        any_zstd,
        schema_chunk,
        msg_chunk,
    } = decompress_kiwi_container(bytes)?;
    let defs = parse_kiwi_schema(&schema_chunk)?;
    let scanned = scan_kiwi_message_pass1(&msg_chunk, &defs, default_name, false, false)?;
    let (children_by_parent, page_indices) = build_scanned_children_graph(&scanned);

    let mut pages = Vec::new();
    for pidx in page_indices {
        let pnode = &scanned.nodes[pidx];
        if pnode.internal_only {
            continue;
        }
        let mut top_frame_count = 0usize;
        let mut node_count = 0usize;
        for &cidx in &children_by_parent[pidx] {
            if is_top_frame_kiwi_type(&scanned.nodes[cidx].node_type) {
                top_frame_count += 1;
            }
            node_count =
                node_count.saturating_add(count_scanned_subtree(cidx, &children_by_parent, 1));
        }
        pages.push(FigmaPageSummary {
            id: format_guid_pair(pnode.guid),
            name: pnode.name.clone().into_owned(),
            visible: pnode.visible,
            top_frame_count,
            node_count,
        });
    }

    let prefix = if is_figjam { "fig-jam" } else { "fig-kiwi" };
    let codec = if any_zstd { "zstd" } else { "deflate" };
    let version_label = format!("{prefix} v{version} ({codec})");

    Ok(FigmaDocumentSummary {
        name: scanned.doc_name,
        version_label,
        pages,
    })
}

pub(super) fn parse_fig_kiwi_canvas_with_options(
    bytes: &[u8],
    default_name: &str,
    options: &FigmaImportOptions,
) -> Result<FigmaDoc, ImportError> {
    decode_fig_kiwi_stream(bytes, default_name, options).map_err(ImportError::Container)
}

fn decode_single_kiwi_node(
    snode: &ScannedKiwiNode<'_>,
    defs: &[KiwiDef],
    nc_def_idx: usize,
    blobs: &[&[u8]],
    blob_base: u64,
    budget: &mut usize,
    active_structs: &mut Vec<usize>,
) -> Result<FigmaNode, String> {
    let mut r = KiwiReader::new(snode.raw_slice);
    let val = decode_kiwi_scalar(&mut r, defs, nc_def_idx as i32, 1, budget, active_structs)?;
    let obj = val
        .as_obj()
        .ok_or_else(|| "Decoded NodeChange is not an object".to_owned())?;

    let guid = format_guid_pair(snode.guid);
    let node_type = snode.node_type.clone().into_owned();
    let name = snode.name.clone().into_owned();
    let visible = snode.visible;
    let internal_only = snode.internal_only;
    let is_state_group = snode.is_state_group;
    let locked = obj
        .get("locked")
        .and_then(KiwiVal::as_bool)
        .unwrap_or(false);

    let opacity = obj
        .get("opacity")
        .and_then(KiwiVal::as_f64)
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let blend_mode = obj
        .get("blendMode")
        .and_then(KiwiVal::as_str)
        .unwrap_or("PASS_THROUGH")
        .to_owned();

    let (width, height) = if let Some(sz) = obj.get("size").and_then(KiwiVal::as_obj) {
        (
            sz.get("x")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(100.0)
                .max(0.0),
            sz.get("y")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(100.0)
                .max(0.0),
        )
    } else {
        (100.0, 100.0)
    };

    let is_clippable_container = matches!(
        node_type.as_str(),
        "FRAME" | "SYMBOL" | "COMPONENT" | "COMPONENT_SET" | "INSTANCE" | "SECTION"
    );
    let clips_content = obj
        .get("clipsContent")
        .and_then(KiwiVal::as_bool)
        .unwrap_or_else(|| {
            is_clippable_container
                && obj.get("frameMaskDisabled").and_then(KiwiVal::as_bool) == Some(false)
        });
    let is_mask = obj.get("mask").and_then(KiwiVal::as_bool).unwrap_or(false);
    let mask_type = obj
        .get("maskType")
        .and_then(KiwiVal::as_str)
        .unwrap_or("ALPHA")
        .to_owned();

    let (mut x, mut y) = (0.0, 0.0);
    let relative_transform = if let Some(tr) = obj.get("transform").and_then(KiwiVal::as_obj) {
        let m00 = tr.get("m00").and_then(KiwiVal::as_f64).unwrap_or(1.0);
        let m01 = tr.get("m01").and_then(KiwiVal::as_f64).unwrap_or(0.0);
        let m02 = tr.get("m02").and_then(KiwiVal::as_f64).unwrap_or(0.0);
        let m10 = tr.get("m10").and_then(KiwiVal::as_f64).unwrap_or(0.0);
        let m11 = tr.get("m11").and_then(KiwiVal::as_f64).unwrap_or(1.0);
        let m12 = tr.get("m12").and_then(KiwiVal::as_f64).unwrap_or(0.0);
        x = m02;
        y = m12;
        Some([m00, m10, m01, m11, m02, m12])
    } else {
        None
    };

    let uniform_r = obj
        .get("cornerRadius")
        .and_then(KiwiVal::as_f64)
        .unwrap_or(0.0)
        .max(0.0);
    let independent_corners = obj
        .get("rectangleCornerRadiiIndependent")
        .and_then(KiwiVal::as_bool)
        .unwrap_or(false);
    let corner_radii = if independent_corners
        || obj.get("rectangleTopLeftCornerRadius").is_some()
        || obj.get("rectangleTopRightCornerRadius").is_some()
        || obj.get("rectangleBottomRightCornerRadius").is_some()
        || obj.get("rectangleBottomLeftCornerRadius").is_some()
    {
        let tl = obj
            .get("rectangleTopLeftCornerRadius")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(uniform_r)
            .max(0.0);
        let tr = obj
            .get("rectangleTopRightCornerRadius")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(uniform_r)
            .max(0.0);
        let br = obj
            .get("rectangleBottomRightCornerRadius")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(uniform_r)
            .max(0.0);
        let bl = obj
            .get("rectangleBottomLeftCornerRadius")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(uniform_r)
            .max(0.0);
        [tl, tr, br, bl]
    } else {
        [uniform_r; 4]
    };

    let arc_data = obj
        .get("arcData")
        .and_then(KiwiVal::as_obj)
        .map(|ao| FigmaArcData {
            start_angle_rad: ao
                .get("startingAngle")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(0.0),
            end_angle_rad: ao
                .get("endingAngle")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(std::f64::consts::TAU),
            inner_radius_ratio: ao
                .get("innerRadius")
                .and_then(KiwiVal::as_f64)
                .unwrap_or(0.0),
        });

    let point_count = obj
        .get("count")
        .and_then(KiwiVal::as_u64)
        .and_then(|v| u32::try_from(v).ok());
    let star_inner_radius = obj.get("starInnerScale").and_then(KiwiVal::as_f64);

    let fills = obj
        .get("fillPaints")
        .and_then(KiwiVal::as_array)
        .map(|a| parse_kiwi_paint_list(a, width, height))
        .unwrap_or_default();

    let background_color = if node_type == "CANVAS" {
        let bg_enabled = obj
            .get("backgroundEnabled")
            .and_then(KiwiVal::as_bool)
            .unwrap_or(true);
        if bg_enabled {
            obj.get("backgroundColor")
                .and_then(KiwiVal::as_obj)
                .map(|bg| kiwi_color_to_rgba(bg, 1.0))
        } else {
            None
        }
    } else {
        None
    };

    let strokes = obj
        .get("strokePaints")
        .and_then(KiwiVal::as_array)
        .map(|a| parse_kiwi_paint_list(a, width, height))
        .unwrap_or_default();
    let stroke_weight = obj
        .get("strokeWeight")
        .and_then(KiwiVal::as_f64)
        .unwrap_or(1.0)
        .max(0.0);

    let border_indep = obj
        .get("borderStrokeWeightsIndependent")
        .and_then(KiwiVal::as_bool)
        .unwrap_or(false);
    let border_weights = if border_indep {
        let bt = obj
            .get("borderTopWeight")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(stroke_weight)
            .max(0.0);
        let br = obj
            .get("borderRightWeight")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(stroke_weight)
            .max(0.0);
        let bb = obj
            .get("borderBottomWeight")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(stroke_weight)
            .max(0.0);
        let bl = obj
            .get("borderLeftWeight")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(stroke_weight)
            .max(0.0);
        Some([bt, br, bb, bl])
    } else {
        None
    };

    let mut fill_paths: Vec<(Vec<PathCommand>, FillRule)> = Vec::new();
    if let Some(garr) = obj.get("fillGeometry").and_then(KiwiVal::as_array) {
        for g in garr {
            let Some(go) = g.as_obj() else { continue };
            let rule = match go.get("windingRule").and_then(KiwiVal::as_str) {
                Some("ODD" | "EVENODD") => FillRule::EvenOdd,
                _ => FillRule::NonZero,
            };
            if let Some(cb) = go.get("commandsBlob")
                && let Some(blob) = resolve_kiwi_blob(cb, blobs, blob_base)
            {
                let cmds = decode_figma_commands_blob(blob);
                if !cmds.is_empty() {
                    fill_paths.push((cmds, rule));
                    continue;
                }
            }
            if let Some(svg_d) = go.get("commands").and_then(KiwiVal::as_str) {
                let cmds = crate::vector_fidelity::parse_svg_path_data(svg_d);
                if !cmds.is_empty() {
                    fill_paths.push((cmds, rule));
                }
            }
        }
    }

    let mut stroke_paths: Vec<Vec<PathCommand>> = Vec::new();
    if let Some(garr) = obj.get("strokeGeometry").and_then(KiwiVal::as_array) {
        for g in garr {
            let Some(go) = g.as_obj() else { continue };
            if let Some(cb) = go.get("commandsBlob")
                && let Some(blob) = resolve_kiwi_blob(cb, blobs, blob_base)
            {
                let cmds = decode_figma_commands_blob(blob);
                if !cmds.is_empty() {
                    stroke_paths.push(cmds);
                    continue;
                }
            }
            if let Some(svg_d) = go.get("commands").and_then(KiwiVal::as_str) {
                let cmds = crate::vector_fidelity::parse_svg_path_data(svg_d);
                if !cmds.is_empty() {
                    stroke_paths.push(cmds);
                }
            }
        }
    }

    if (fill_paths.is_empty() || (stroke_paths.is_empty() && !strokes.is_empty()))
        && let Some(vd) = obj.get("vectorData").and_then(KiwiVal::as_obj)
        && let Some(vnb) = vd.get("vectorNetworkBlob")
        && let Some(blob) = resolve_kiwi_blob(vnb, blobs, blob_base)
    {
        let norm_size = vd
            .get("normalizedSize")
            .and_then(KiwiVal::as_obj)
            .map(|ns| {
                (
                    ns.get("x").and_then(KiwiVal::as_f64).unwrap_or(width),
                    ns.get("y").and_then(KiwiVal::as_f64).unwrap_or(height),
                )
            });
        let (vn_fills, vn_strokes) =
            decode_figma_vector_network_blob(blob, width, height, norm_size);
        if fill_paths.is_empty() {
            if !vn_fills.is_empty() {
                fill_paths = vn_fills;
            } else if !fills.is_empty() && !vn_strokes.is_empty() {
                fill_paths = vn_strokes
                    .iter()
                    .cloned()
                    .map(|c| (c, FillRule::NonZero))
                    .collect();
            }
        }
        if stroke_paths.is_empty() && !vn_strokes.is_empty() {
            stroke_paths = vn_strokes;
        }
    }

    let effects = parse_kiwi_effects(obj);

    let text = if node_type == "TEXT" {
        let td_obj = obj.get("textData").and_then(KiwiVal::as_obj);
        let characters = td_obj
            .and_then(|td| td.get("characters"))
            .and_then(KiwiVal::as_str)
            .unwrap_or("")
            .to_owned();
        let font_size = obj
            .get("fontSize")
            .and_then(KiwiVal::as_f64)
            .unwrap_or(16.0)
            .max(1.0);
        let (font_family, bold, italic) =
            if let Some(fn_obj) = obj.get("fontName").and_then(KiwiVal::as_obj) {
                let fam = fn_obj
                    .get("family")
                    .and_then(KiwiVal::as_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Inter")
                    .to_owned();
                let style = fn_obj
                    .get("style")
                    .and_then(KiwiVal::as_str)
                    .unwrap_or("Regular")
                    .to_ascii_lowercase();
                (
                    fam,
                    style.contains("bold")
                        || style.contains("black")
                        || style.contains("heavy")
                        || style.contains("semibold")
                        || style.contains("demibold"),
                    style.contains("italic") || style.contains("oblique"),
                )
            } else {
                ("Inter".into(), false, false)
            };
        let (align, justify) = match obj.get("textAlignHorizontal").and_then(KiwiVal::as_str) {
            Some("CENTER") => (TextAlign::Center, false),
            Some("RIGHT") => (TextAlign::Right, false),
            Some("JUSTIFIED") => (TextAlign::Left, true),
            _ => (TextAlign::Left, false),
        };
        let align_v = match obj.get("textAlignVertical").and_then(KiwiVal::as_str) {
            Some("CENTER") => "middle",
            Some("BOTTOM") => "bottom",
            _ => "top",
        }
        .to_owned();
        let text_dec = obj
            .get("textDecoration")
            .and_then(KiwiVal::as_str)
            .unwrap_or("NONE");
        let underline = text_dec == "UNDERLINE";
        let strikethrough = text_dec == "STRIKETHROUGH";
        let auto_resize = obj
            .get("textAutoResize")
            .and_then(KiwiVal::as_str)
            .unwrap_or("NONE")
            .to_owned();
        let letter_spacing = parse_kiwi_number_px(obj.get("letterSpacing"), font_size)
            .map(|(v, _)| v)
            .unwrap_or(0.0);
        let line_height =
            parse_kiwi_number_px(obj.get("lineHeight"), font_size).and_then(|(v, units)| {
                if v <= 0.0 || (units == "PERCENT" && (v - font_size).abs() <= 0.05) {
                    None
                } else {
                    Some(v)
                }
            });

        let style_runs = if let Some(td) = td_obj {
            let char_ids: Vec<u64> = td
                .get("characterStyleIDs")
                .and_then(KiwiVal::as_array)
                .map(|arr| arr.iter().map(|v| v.as_u64().unwrap_or(0)).collect())
                .unwrap_or_default();
            let mut override_map = HashMap::new();
            if !char_ids.is_empty()
                && let Some(sot) = td.get("styleOverrideTable").and_then(KiwiVal::as_array)
            {
                for item in sot {
                    if let Some(so) = item.as_obj()
                        && let Some(sid) = so.get("styleID").and_then(KiwiVal::as_u64)
                        && sid != 0
                    {
                        override_map.insert(sid, parse_kiwi_style_override(so, font_size));
                    }
                }
            }
            build_text_style_runs(&characters, &char_ids, &override_map)
        } else {
            Vec::new()
        };

        Some(FigmaTextData {
            characters,
            font_family,
            font_size,
            bold,
            italic,
            underline,
            strikethrough,
            align,
            justify,
            align_v,
            letter_spacing,
            line_height,
            auto_resize,
            style_runs,
        })
    } else {
        None
    };

    let mut symbol_overrides = Vec::new();
    let component_id = if let Some(sd) = obj.get("symbolData").and_then(KiwiVal::as_obj) {
        if let Some(ov_arr) = sd.get("symbolOverrides").and_then(KiwiVal::as_array) {
            for ov_val in ov_arr {
                let Some(ov) = ov_val.as_obj() else { continue };
                let guid_path: Vec<String> = ov
                    .get("guidPath")
                    .and_then(|gp| {
                        gp.as_obj()
                            .and_then(|o| o.get("guids"))
                            .and_then(KiwiVal::as_array)
                            .or_else(|| gp.as_array())
                    })
                    .map(|garr| {
                        garr.iter()
                            .filter_map(|g| g.as_obj().and_then(guid_string))
                            .collect()
                    })
                    .or_else(|| {
                        ov.get("guid")
                            .and_then(KiwiVal::as_obj)
                            .and_then(guid_string)
                            .map(|g| vec![g])
                    })
                    .unwrap_or_default();
                if guid_path.is_empty() {
                    continue;
                }
                let ov_vis = ov.get("visible").and_then(KiwiVal::as_bool);
                let ov_op = ov
                    .get("opacity")
                    .and_then(KiwiVal::as_f64)
                    .map(|v| v.clamp(0.0, 1.0));
                let ov_chars = ov
                    .get("textData")
                    .and_then(KiwiVal::as_obj)
                    .and_then(|td| td.get("characters"))
                    .and_then(KiwiVal::as_str)
                    .map(str::to_owned);
                let ov_fills = ov
                    .get("fillPaints")
                    .and_then(KiwiVal::as_array)
                    .map(|a| parse_kiwi_paint_list(a, width, height));
                let ov_strokes = ov
                    .get("strokePaints")
                    .and_then(KiwiVal::as_array)
                    .map(|a| parse_kiwi_paint_list(a, width, height));
                let ov_sym = ov
                    .get("overriddenSymbolID")
                    .and_then(KiwiVal::as_obj)
                    .and_then(guid_string);
                if ov_vis.is_some()
                    || ov_op.is_some()
                    || ov_chars.is_some()
                    || ov_fills.is_some()
                    || ov_strokes.is_some()
                    || ov_sym.is_some()
                {
                    symbol_overrides.push(FigmaSymbolOverride {
                        guid_path,
                        visible: ov_vis,
                        opacity: ov_op,
                        characters: ov_chars,
                        fills: ov_fills,
                        strokes: ov_strokes,
                        overridden_symbol_id: ov_sym,
                    });
                }
            }
        }
        obj.get("overriddenSymbolID")
            .and_then(KiwiVal::as_obj)
            .and_then(guid_string)
            .or_else(|| {
                sd.get("symbolID")
                    .and_then(KiwiVal::as_obj)
                    .and_then(guid_string)
            })
    } else {
        obj.get("overriddenSymbolID")
            .and_then(KiwiVal::as_obj)
            .and_then(guid_string)
    };

    Ok(FigmaNode {
        id: guid,
        name,
        node_type,
        visible,
        locked,
        internal_only,
        is_state_group,
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
        arc_data,
        point_count,
        star_inner_radius,
        fills,
        strokes,
        stroke_weight,
        border_weights,
        fill_paths,
        stroke_paths,
        text,
        effects,
        component_id,
        symbol_overrides,
        background_color,
        children: Vec::new(),
    })
}

fn collect_ordered_master_indices(
    idx: usize,
    scanned: &ScannedKiwiStream<'_>,
    children_by_parent: &[Vec<usize>],
    out: &mut Vec<usize>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    let node = &scanned.nodes[idx];
    if matches!(node.node_type.as_ref(), "COMPONENT" | "SYMBOL") && !node.is_state_group {
        out.push(idx);
    }
    for &cidx in &children_by_parent[idx] {
        collect_ordered_master_indices(cidx, scanned, children_by_parent, out, depth + 1);
    }
}

fn attach_decoded_children(
    idx: usize,
    decoded: &mut [Option<FigmaNode>],
    children_by_parent: &[Vec<usize>],
    depth: usize,
) -> Option<FigmaNode> {
    if depth > 128 {
        return None;
    }
    let mut node = decoded[idx].take()?;
    let cids = &children_by_parent[idx];
    if !cids.is_empty() {
        node.children.reserve(cids.len());
        for &cidx in cids {
            if let Some(child_node) =
                attach_decoded_children(cidx, decoded, children_by_parent, depth + 1)
            {
                node.children.push(child_node);
            }
        }
    }
    Some(node)
}

fn decode_fig_kiwi_stream(
    bytes: &[u8],
    default_name: &str,
    options: &FigmaImportOptions,
) -> Result<FigmaDoc, String> {
    let DecompressedKiwiContainer {
        version,
        is_figjam,
        any_zstd,
        schema_chunk,
        msg_chunk,
    } = decompress_kiwi_container(bytes)?;
    let defs = parse_kiwi_schema(&schema_chunk)?;
    let scanned = scan_kiwi_message_pass1(&msg_chunk, &defs, default_name, true, true)?;
    let (children_by_parent, all_page_indices) = build_scanned_children_graph(&scanned);

    let n = scanned.nodes.len();
    let selective =
        options.selected_page_ids.is_some() || options.page_mapping == FigmaPageMapping::Layers;

    let non_internal_pages: Vec<usize> = {
        let ni: Vec<usize> = all_page_indices
            .iter()
            .copied()
            .filter(|&i| !scanned.nodes[i].internal_only)
            .collect();
        if ni.is_empty() {
            all_page_indices.clone()
        } else {
            ni
        }
    };

    let active_page_indices: Vec<usize> = if let Some(sel_ids) = &options.selected_page_ids {
        let mut by_str: HashMap<String, usize> = HashMap::with_capacity(non_internal_pages.len());
        for &pidx in &non_internal_pages {
            by_str.insert(format_guid_pair(scanned.nodes[pidx].guid), pidx);
        }
        let mut matched = Vec::with_capacity(sel_ids.len());
        for sid in sel_ids {
            if let Some(&pidx) = by_str.get(sid)
                && !matched.contains(&pidx)
            {
                matched.push(pidx);
            }
        }
        matched
    } else {
        non_internal_pages.clone()
    };

    let mut ordered_masters = Vec::new();
    for &pidx in &all_page_indices {
        collect_ordered_master_indices(
            pidx,
            &scanned,
            &children_by_parent,
            &mut ordered_masters,
            0,
        );
    }
    let mut masters_by_local: HashMap<u64, Vec<usize>> = HashMap::new();
    for &midx in &ordered_masters {
        masters_by_local
            .entry(scanned.nodes[midx].guid.1)
            .or_default()
            .push(midx);
    }

    // Track reachability in two states (`0` = unvisited, `1` = page-context, `2` = master-context)
    // so hidden descendants inside referenced master components are still decoded for overrides.
    let mut reach_state = vec![0u8; n];
    let mut queue: VecDeque<(usize, bool)> = VecDeque::new();
    let enqueue = |idx: usize,
                   in_master: bool,
                   reach_state: &mut [u8],
                   queue: &mut VecDeque<(usize, bool)>| {
        let target = if in_master { 2u8 } else { 1u8 };
        if reach_state[idx] < target {
            reach_state[idx] = target;
            queue.push_back((idx, in_master));
        }
    };

    for &pidx in &active_page_indices {
        enqueue(pidx, false, &mut reach_state, &mut queue);
    }
    if !selective {
        for &midx in ordered_masters.iter().take(1_500) {
            enqueue(midx, true, &mut reach_state, &mut queue);
        }
    }

    while let Some((uidx, in_master)) = queue.pop_front() {
        let unode = &scanned.nodes[uidx];
        let next_in_master = in_master
            || (matches!(unode.node_type.as_ref(), "COMPONENT" | "SYMBOL")
                && !unode.is_state_group);

        for &cidx in &children_by_parent[uidx] {
            let cnode = &scanned.nodes[cidx];
            if cnode.visible || options.import_hidden_layers || next_in_master || !selective {
                enqueue(cidx, next_in_master, &mut reach_state, &mut queue);
            }
        }

        for &(rs, rl) in &unode.ref_symbols {
            if let Some(&tidx) = scanned.idx_by_guid.get(&(rs, rl)) {
                enqueue(tidx, true, &mut reach_state, &mut queue);
            } else if let Some(cands) = masters_by_local.get(&rl) {
                for &cidx in cands {
                    if (rs == 0) != (scanned.nodes[cidx].guid.0 == 0) {
                        enqueue(cidx, true, &mut reach_state, &mut queue);
                    }
                }
            }
        }
    }

    let mut budget = MAX_KIWI_VALUES;
    let mut active_structs = Vec::new();
    let mut decoded_nodes: Vec<Option<FigmaNode>> = (0..n).map(|_| None).collect();
    for idx in 0..n {
        if reach_state[idx] == 0 {
            continue;
        }
        let fnode = decode_single_kiwi_node(
            &scanned.nodes[idx],
            &defs,
            scanned.nc_def_idx,
            &scanned.blobs,
            scanned.blob_base,
            &mut budget,
            &mut active_structs,
        )?;
        decoded_nodes[idx] = Some(fnode);
    }

    let mut pages = Vec::new();
    for &pidx in &active_page_indices {
        if let Some(pnode) =
            attach_decoded_children(pidx, &mut decoded_nodes, &children_by_parent, 0)
        {
            pages.push(pnode);
        }
    }

    // Attach any reachable master `COMPONENT` / `SYMBOL` definitions from `Internal Only Canvas`,
    // unselected pages, or skipped hidden containers under an internal-only library page so
    // `convert.rs` resolves every referenced component without adding extra Stage pages.
    let mut external_masters = Vec::new();
    for &midx in &ordered_masters {
        if decoded_nodes[midx].is_some()
            && let Some(mnode) =
                attach_decoded_children(midx, &mut decoded_nodes, &children_by_parent, 0)
        {
            external_masters.push(mnode);
        }
    }
    if !external_masters.is_empty() {
        pages.push(FigmaNode {
            id: "__internal_masters__".into(),
            name: "Internal Only Canvas".into(),
            node_type: "CANVAS".into(),
            visible: false,
            internal_only: true,
            children: external_masters,
            ..FigmaNode::default()
        });
    }

    let prefix = if is_figjam { "fig-jam" } else { "fig-kiwi" };
    let codec = if any_zstd { "zstd" } else { "deflate" };
    let version_label = format!("{prefix} v{version} ({codec})");

    Ok(FigmaDoc {
        name: scanned.doc_name,
        version_label,
        pages,
        inline_images: Default::default(),
    })
}
