use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// 1. Binary `.fig` (`fig-kiwi` / `fig-jam.`) Sequential Kiwi Schema + Message Decoder
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub(super) enum KiwiDefKind {
    Enum,
    Struct,
    Message,
}

#[derive(Clone, Debug)]
pub(super) struct KiwiField {
    name: String,
    type_id: i32,
    is_array: bool,
    value: u32,
}

#[derive(Clone, Debug)]
pub(super) struct KiwiDef {
    name: String,
    kind: KiwiDefKind,
    fields: Vec<KiwiField>,
}

#[derive(Clone, Debug)]
pub(super) enum KiwiVal {
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

pub(super) struct KiwiReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> KiwiReader<'a> {
    pub(super) fn new(buf: &'a [u8]) -> Self {
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

pub(super) fn decode_kiwi_scalar(
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

pub(super) fn parse_fig_kiwi_canvas(
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
