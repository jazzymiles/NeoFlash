//! Bounded Adobe Photoshop Action Descriptor (`Objc`) and Layer Styles (`lfx2`, `lmfx`, `lrFX`)
//! parser and NeoFlash [`FilterSpec`] / [`ColorTransform`] converter (`T-418`).
//!
//! Security & bounds invariants:
//! - Every length, item count, `VlLs` count, and UTF-16BE code-unit count is validated against
//!   both explicit caps and the remaining slice bytes *before* allocating.
//! - Recursion depth for nested `Objc` / `VlLs` values is capped at [`MAX_DESC_DEPTH`].
//! - Malformed UTF-16BE (including lone surrogates) decodes lossily without panicking.

use neoflash_core::{ColorTransform, FilterSpec, MAX_OBJECT_FILTERS};

const MAX_DESC_DEPTH: usize = 16;
const MAX_DESC_ITEMS: usize = 4_096;
const MAX_DESC_LIST_ITEMS: usize = 4_096;
const MAX_DESC_STRING_CHARS: usize = 524_288; // 1 MiB UTF-16BE
const MAX_DESC_KEY_BYTES: usize = 4_096;
const MAX_DESC_RAW_BYTES: usize = 16 * 1024 * 1024;
const MAX_LRFX_SUBBLOCKS: usize = 64;

/// Parsed Photoshop Action Descriptor (`Objc` / `GlbO`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PsdDescriptor {
    pub class_name: String,
    pub class_id: String,
    pub items: Vec<(String, PsdDescriptorValue)>,
}

/// Typed value inside a [`PsdDescriptor`].
#[derive(Clone, Debug, PartialEq)]
pub enum PsdDescriptorValue {
    Object(PsdDescriptor),
    List(Vec<PsdDescriptorValue>),
    Double(f64),
    UnitFloat { unit: [u8; 4], value: f64 },
    UnitFloats { unit: [u8; 4], values: Vec<f64> },
    Text(String),
    Enum { type_id: String, value: String },
    Long(i32),
    Comp(i64),
    Bool(bool),
    Class { name: String, class_id: String },
    RawLen(usize),
}

impl PsdDescriptor {
    pub fn get(&self, key: &str) -> Option<&PsdDescriptorValue> {
        self.items
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, val)| val)
    }

    pub fn get_any(&self, keys: &[&str]) -> Option<&PsdDescriptorValue> {
        keys.iter().find_map(|k| self.get(k))
    }

    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            Some(PsdDescriptorValue::Bool(v)) => *v,
            Some(PsdDescriptorValue::Long(v)) => *v != 0,
            _ => default,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            PsdDescriptorValue::Double(v) | PsdDescriptorValue::UnitFloat { value: v, .. } => {
                v.is_finite().then_some(*v)
            }
            PsdDescriptorValue::Long(v) => Some(f64::from(*v)),
            PsdDescriptorValue::Comp(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn number_any(&self, keys: &[&str]) -> Option<f64> {
        keys.iter().find_map(|k| self.number(k))
    }

    pub fn enum_value(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            PsdDescriptorValue::Enum { value, .. } => Some(value.as_str()),
            PsdDescriptorValue::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn object(&self, key: &str) -> Option<&PsdDescriptor> {
        match self.get(key)? {
            PsdDescriptorValue::Object(obj) => Some(obj),
            _ => None,
        }
    }
}

/// Converted editable NeoFlash layer effects for one PSD layer or folder group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PsdParsedLayerEffects {
    /// Editable display filters (already clamped and capped to `<= MAX_OBJECT_FILTERS`).
    pub filters: Vec<FilterSpec>,
    /// Optional `SoFi` (Color Overlay) linear tint for `Element.transform.color_transform`.
    pub color_overlay_tint: Option<ColorTransform>,
    /// Equivalent `ColorMatrix` filter when `SoFi` is attached to a `LayerRole::Folder`.
    pub folder_color_overlay_filter: Option<FilterSpec>,
    /// Actionable approximation messages to emit as `psd-layer-effect-approximated`.
    pub approximations: Vec<String>,
    /// True when the layer carries a visible effect that cannot be reconstructed 1:1 in
    /// NeoFlash (used by `PsdLayerMapping::Automatic` to prefer a saved composite).
    pub has_unreconstructable_effect: bool,
}

impl PsdParsedLayerEffects {
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
            && self.color_overlay_tint.is_none()
            && self.folder_color_overlay_filter.is_none()
            && self.approximations.is_empty()
    }
}

fn read_u8(data: &[u8], pos: &mut usize) -> Option<u8> {
    let end = pos.checked_add(1)?;
    let byte = *data.get(*pos)?;
    *pos = end;
    Some(byte)
}

fn read_u16_be(data: &[u8], pos: &mut usize) -> Option<u16> {
    let end = pos.checked_add(2)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32_be(data: &[u8], pos: &mut usize) -> Option<u32> {
    let end = pos.checked_add(4)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_i32_be(data: &[u8], pos: &mut usize) -> Option<i32> {
    read_u32_be(data, pos).map(|v| v as i32)
}

fn read_i64_be(data: &[u8], pos: &mut usize) -> Option<i64> {
    let end = pos.checked_add(8)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    let bytes: [u8; 8] = slice.try_into().ok()?;
    Some(i64::from_be_bytes(bytes))
}

fn read_f64_be(data: &[u8], pos: &mut usize) -> Option<f64> {
    let bits = read_i64_be(data, pos)? as u64;
    Some(f64::from_bits(bits))
}

fn read_fourcc(data: &[u8], pos: &mut usize) -> Option<[u8; 4]> {
    let end = pos.checked_add(4)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    slice.try_into().ok()
}

fn read_unicode_string(data: &[u8], pos: &mut usize) -> Option<String> {
    let char_count = read_u32_be(data, pos)? as usize;
    if char_count > MAX_DESC_STRING_CHARS {
        return None;
    }
    let byte_len = char_count.checked_mul(2)?;
    let end = pos.checked_add(byte_len)?;
    let raw = data.get(*pos..end)?;
    *pos = end;
    if char_count == 0 {
        return Some(String::new());
    }
    let mut out = String::with_capacity(char_count.min(256));
    let iter = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
    for decoded in char::decode_utf16(iter) {
        let ch = decoded.unwrap_or(char::REPLACEMENT_CHARACTER);
        if ch != '\0' {
            out.push(ch);
        }
    }
    Some(out)
}

fn read_key_or_class_id(data: &[u8], pos: &mut usize) -> Option<String> {
    let len = read_u32_be(data, pos)? as usize;
    let actual_len = if len == 0 { 4 } else { len };
    if actual_len > MAX_DESC_KEY_BYTES {
        return None;
    }
    let end = pos.checked_add(actual_len)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    Some(String::from_utf8_lossy(slice).into_owned())
}

fn skip_reference_list(data: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    let count = read_u32_be(data, pos)? as usize;
    if count > MAX_DESC_LIST_ITEMS
        || count
            .checked_mul(8)
            .is_none_or(|min_b| min_b > data.len().saturating_sub(*pos))
    {
        return None;
    }
    for _ in 0..count {
        let ref_type = read_fourcc(data, pos)?;
        match &ref_type {
            b"prop" => {
                let _ = read_unicode_string(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
            }
            b"Clss" => {
                let _ = read_unicode_string(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
            }
            b"Enmr" => {
                let _ = read_unicode_string(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
            }
            b"rele" => {
                let _ = read_unicode_string(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
                let _ = read_i32_be(data, pos)?;
            }
            b"Idnt" | b"indx" => {
                let _ = read_i32_be(data, pos)?;
            }
            b"name" => {
                let _ = read_unicode_string(data, pos)?;
                let _ = read_key_or_class_id(data, pos)?;
                let _ = read_unicode_string(data, pos)?;
            }
            _ => return None,
        }
    }
    Some(*pos - start)
}

fn parse_descriptor_value(
    data: &[u8],
    pos: &mut usize,
    ostype: &[u8; 4],
    depth: usize,
) -> Option<PsdDescriptorValue> {
    if depth > MAX_DESC_DEPTH {
        return None;
    }
    match ostype {
        b"Objc" | b"GlbO" => Some(PsdDescriptorValue::Object(parse_descriptor_body(
            data,
            pos,
            depth + 1,
        )?)),
        b"VlLs" => {
            let count = read_u32_be(data, pos)? as usize;
            // Every list element takes at least 4 (OSType) + 1 (bool) = 5 bytes.
            if count > MAX_DESC_LIST_ITEMS
                || count
                    .checked_mul(5)
                    .is_none_or(|min_b| min_b > data.len().saturating_sub(*pos))
            {
                return None;
            }
            let mut list = Vec::with_capacity(count.min(64));
            for _ in 0..count {
                let elem_type = read_fourcc(data, pos)?;
                list.push(parse_descriptor_value(data, pos, &elem_type, depth + 1)?);
            }
            Some(PsdDescriptorValue::List(list))
        }
        b"doub" => Some(PsdDescriptorValue::Double(read_f64_be(data, pos)?)),
        b"UntF" => {
            let unit = read_fourcc(data, pos)?;
            let value = read_f64_be(data, pos)?;
            Some(PsdDescriptorValue::UnitFloat { unit, value })
        }
        b"UnFl" => {
            let unit = read_fourcc(data, pos)?;
            let count = read_u32_be(data, pos)? as usize;
            let byte_len = count.checked_mul(8)?;
            if count > MAX_DESC_LIST_ITEMS || byte_len > data.len().saturating_sub(*pos) {
                return None;
            }
            let mut values = Vec::with_capacity(count.min(64));
            for _ in 0..count {
                values.push(read_f64_be(data, pos)?);
            }
            Some(PsdDescriptorValue::UnitFloats { unit, values })
        }
        b"TEXT" => Some(PsdDescriptorValue::Text(read_unicode_string(data, pos)?)),
        b"enum" => {
            let type_id = read_key_or_class_id(data, pos)?;
            let value = read_key_or_class_id(data, pos)?;
            Some(PsdDescriptorValue::Enum { type_id, value })
        }
        b"long" => Some(PsdDescriptorValue::Long(read_i32_be(data, pos)?)),
        b"comp" => Some(PsdDescriptorValue::Comp(read_i64_be(data, pos)?)),
        b"bool" => Some(PsdDescriptorValue::Bool(read_u8(data, pos)? != 0)),
        b"type" | b"GlbC" => {
            let name = read_unicode_string(data, pos)?;
            let class_id = read_key_or_class_id(data, pos)?;
            Some(PsdDescriptorValue::Class { name, class_id })
        }
        b"alis" | b"tdta" | b"Pth " => {
            let byte_len = read_u32_be(data, pos)? as usize;
            if byte_len > MAX_DESC_RAW_BYTES {
                return None;
            }
            let end = pos.checked_add(byte_len)?;
            if end > data.len() {
                return None;
            }
            *pos = end;
            Some(PsdDescriptorValue::RawLen(byte_len))
        }
        b"obj " => {
            let consumed = skip_reference_list(data, pos)?;
            Some(PsdDescriptorValue::RawLen(consumed))
        }
        b"ObAr" => {
            let start = *pos;
            let _row_count = read_u32_be(data, pos)?;
            let _class_name = read_unicode_string(data, pos)?;
            let _class_id = read_key_or_class_id(data, pos)?;
            let col_count = read_u32_be(data, pos)? as usize;
            if col_count > MAX_DESC_ITEMS
                || col_count
                    .checked_mul(9)
                    .is_none_or(|min_b| min_b > data.len().saturating_sub(*pos))
            {
                return None;
            }
            for _ in 0..col_count {
                let _key = read_key_or_class_id(data, pos)?;
                let elem_type = read_fourcc(data, pos)?;
                let _ = parse_descriptor_value(data, pos, &elem_type, depth + 1)?;
            }
            Some(PsdDescriptorValue::RawLen(*pos - start))
        }
        _ => None,
    }
}

/// Parse an Action Descriptor body (starting at the class-name Unicode string, after any
/// outer version header).
pub fn parse_descriptor_body(data: &[u8], pos: &mut usize, depth: usize) -> Option<PsdDescriptor> {
    if depth > MAX_DESC_DEPTH {
        return None;
    }
    let class_name = read_unicode_string(data, pos)?;
    let class_id = read_key_or_class_id(data, pos)?;
    let item_count = read_u32_be(data, pos)? as usize;
    // Minimum item size: 4 (key len >= 1) + 1 (key) + 4 (OSType) + 1 (bool) = 10 bytes.
    if item_count > MAX_DESC_ITEMS
        || item_count
            .checked_mul(10)
            .is_none_or(|min_b| min_b > data.len().saturating_sub(*pos))
    {
        return None;
    }
    let mut items = Vec::with_capacity(item_count.min(64));
    for _ in 0..item_count {
        let key = read_key_or_class_id(data, pos)?;
        let ostype = read_fourcc(data, pos)?;
        let val = parse_descriptor_value(data, pos, &ostype, depth)?;
        items.push((key, val));
    }
    Some(PsdDescriptor {
        class_name,
        class_id,
        items,
    })
}

/// Parse a versioned `lfx2` or `lmfx` block (`u32` effects_version == 0, `u32` descriptor_version == 16).
pub fn parse_versioned_effects_descriptor(block: &[u8]) -> Option<PsdDescriptor> {
    let mut pos = 0usize;
    let effects_version = read_u32_be(block, &mut pos)?;
    let descriptor_version = read_u32_be(block, &mut pos)?;
    if effects_version != 0 || descriptor_version != 16 {
        return None;
    }
    parse_descriptor_body(block, &mut pos, 0)
}

/// Extract Image Resource `1037` (`0x040D`, Global Lighting Angle in degrees) when present.
pub fn extract_psd_global_lighting_angle(mut resources: &[u8]) -> Option<f64> {
    while resources.starts_with(b"8BIM") && resources.len() >= 12 {
        let id = u16::from_be_bytes([resources[4], resources[5]]);
        let name_len = (usize::from(resources[6]) + 1).next_multiple_of(2);
        let start = 6 + name_len;
        let len_bytes = resources.get(start..start + 4)?;
        let len = u32::from_be_bytes(len_bytes.try_into().ok()?) as usize;
        let end = start.checked_add(4)?.checked_add(len)?;
        let data = resources.get(start + 4..end)?;
        if id == 1037 && data.len() >= 4 {
            let deg = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            return Some(f64::from(deg).clamp(-360.0, 360.0));
        }
        let next = start.checked_add(4)?.checked_add(len.next_multiple_of(2))?;
        resources = resources.get(next..)?;
    }
    None
}

/// Convert Photoshop light-source angle (`lagl`, CCW degrees where 90 = straight top,
/// 120 = top-left) into Flash/NeoFlash drop-shadow / bevel angle (`180 - lagl`) so shadow
/// and highlight directions match Photoshop 1:1.
pub fn photoshop_light_to_flash_angle(lagl: f64) -> f64 {
    if !lagl.is_finite() {
        return 60.0;
    }
    let raw = 180.0 - lagl;
    if (-360.0..=360.0).contains(&raw) {
        raw
    } else {
        raw.rem_euclid(360.0)
    }
}

fn hsv_to_rgb24(hue_deg: f64, sat_01: f64, val_01: f64) -> u32 {
    let h = hue_deg.rem_euclid(360.0);
    let s = sat_01.clamp(0.0, 1.0);
    let v = val_01.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let r = ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    let g = ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    let b = ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Convert a Photoshop color descriptor (`RGBC`, `CMYC`, `HSBC`, `Grsc`, `LbCl`) to 24-bit RGB.
pub fn descriptor_color_to_rgb24(color_desc: &PsdDescriptor, fallback: u32) -> u32 {
    // 1. RGB (`RGBC` or keys `Rd  `/`Grn `/`Bl  `)
    if color_desc.class_id == "RGBC"
        || color_desc.get("Rd  ").is_some()
        || color_desc.get("Grn ").is_some()
        || color_desc.get("Bl  ").is_some()
    {
        let r = color_desc
            .number("Rd  ")
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 255.0) as u32;
        let g = color_desc
            .number("Grn ")
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 255.0) as u32;
        let b = color_desc
            .number("Bl  ")
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 255.0) as u32;
        return (r << 16) | (g << 8) | b;
    }
    // 2. CMYK (`CMYC` or keys `Cyn `/`Mgnt`/`Ylw `/`Blck`)
    if color_desc.class_id == "CMYC" || color_desc.get("Cyn ").is_some() {
        let c = (color_desc.number("Cyn ").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let m = (color_desc.number("Mgnt").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let y = (color_desc.number("Ylw ").unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let k = (color_desc.number_any(&["Blck", "Bk  "]).unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        let r = (255.0 * (1.0 - c) * (1.0 - k)).round().clamp(0.0, 255.0) as u32;
        let g = (255.0 * (1.0 - m) * (1.0 - k)).round().clamp(0.0, 255.0) as u32;
        let b = (255.0 * (1.0 - y) * (1.0 - k)).round().clamp(0.0, 255.0) as u32;
        return (r << 16) | (g << 8) | b;
    }
    // 3. HSB (`HSBC` or keys `H   `/`Strt`/`Brgh`)
    if color_desc.class_id == "HSBC" || color_desc.get("H   ").is_some() {
        let h = color_desc.number("H   ").unwrap_or(0.0);
        let s = color_desc.number("Strt").unwrap_or(0.0) / 100.0;
        let v = color_desc.number("Brgh").unwrap_or(0.0) / 100.0;
        return hsv_to_rgb24(h, s, v);
    }
    // 4. Grayscale (`Grsc` or key `Gry ` / `Bk  `)
    if color_desc.class_id == "Grsc"
        || color_desc.get("Gry ").is_some()
        || color_desc.get("Bk  ").is_some()
    {
        let level = if let Some(gry) = color_desc.number("Gry ") {
            if gry > 100.0 {
                gry.round().clamp(0.0, 255.0) as u32
            } else {
                ((1.0 - (gry / 100.0).clamp(0.0, 1.0)) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u32
            }
        } else if let Some(bk) = color_desc.number("Bk  ") {
            ((1.0 - (bk / 100.0).clamp(0.0, 1.0)) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u32
        } else {
            0
        };
        return (level << 16) | (level << 8) | level;
    }
    // 5. CIE Lab (`LbCl` or key `Lmnc`)
    if color_desc.class_id == "LbCl" || color_desc.get("Lmnc").is_some() {
        let l = color_desc.number("Lmnc").unwrap_or(0.0).clamp(0.0, 100.0);
        let a = color_desc
            .number("A   ")
            .unwrap_or(0.0)
            .clamp(-128.0, 127.0);
        let b_lab = color_desc
            .number("B   ")
            .unwrap_or(0.0)
            .clamp(-128.0, 127.0);
        let fy = (l + 16.0) / 116.0;
        let fx = a / 500.0 + fy;
        let fz = fy - b_lab / 200.0;
        let inv_f = |t: f64| {
            let t3 = t * t * t;
            if t3 > 0.008_856 {
                t3
            } else {
                (t - 16.0 / 116.0) / 7.787
            }
        };
        let x = 0.950_47 * inv_f(fx);
        let y = 1.000_00 * inv_f(fy);
        let z = 1.088_83 * inv_f(fz);
        let to_srgb = |u: f64| -> u32 {
            let u = u.clamp(0.0, 1.0);
            let s = if u <= 0.003_130_8 {
                12.92 * u
            } else {
                1.055 * u.powf(1.0 / 2.4) - 0.055
            };
            (s * 255.0).round().clamp(0.0, 255.0) as u32
        };
        let r = to_srgb(3.2406 * x - 1.5372 * y - 0.4986 * z);
        let g = to_srgb(-0.9689 * x + 1.8758 * y + 0.0415 * z);
        let b = to_srgb(0.0557 * x - 0.2040 * y + 1.0570 * z);
        return (r << 16) | (g << 8) | b;
    }
    fallback & 0xff_ffff
}

fn lerp_rgb24(c0: u32, c1: u32, t: f64) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let r0 = ((c0 >> 16) & 0xff) as f64;
    let g0 = ((c0 >> 8) & 0xff) as f64;
    let b0 = (c0 & 0xff) as f64;
    let r1 = ((c1 >> 16) & 0xff) as f64;
    let g1 = ((c1 >> 8) & 0xff) as f64;
    let b1 = (c1 & 0xff) as f64;
    let r = (r0 + (r1 - r0) * t).round().clamp(0.0, 255.0) as u32;
    let g = (g0 + (g1 - g0) * t).round().clamp(0.0, 255.0) as u32;
    let b = (b0 + (b1 - b0) * t).round().clamp(0.0, 255.0) as u32;
    (r << 16) | (g << 8) | b
}

fn sample_color_stops(stops: &[(u8, u32)], ratio: u8) -> u32 {
    if stops.is_empty() {
        return 0xff_ffff;
    }
    let idx = stops.partition_point(|(r, _)| *r <= ratio);
    if idx == 0 {
        return stops[0].1;
    }
    if idx >= stops.len() {
        return stops[stops.len() - 1].1;
    }
    let (r0, c0) = stops[idx - 1];
    let (r1, c1) = stops[idx];
    if r1 <= r0 {
        return c1;
    }
    let t = f64::from(ratio - r0) / f64::from(r1 - r0);
    lerp_rgb24(c0, c1, t)
}

fn sample_alpha_stops(stops: &[(u8, f64)], ratio: u8) -> f64 {
    if stops.is_empty() {
        return 1.0;
    }
    let idx = stops.partition_point(|(r, _)| *r <= ratio);
    if idx == 0 {
        return stops[0].1;
    }
    if idx >= stops.len() {
        return stops[stops.len() - 1].1;
    }
    let (r0, a0) = stops[idx - 1];
    let (r1, a1) = stops[idx];
    if r1 <= r0 {
        return a1;
    }
    let t = f64::from(ratio - r0) / f64::from(r1 - r0);
    (a0 + (a1 - a0) * t).clamp(0.0, 1.0)
}

fn lctn_to_ratio(desc: &PsdDescriptor) -> u8 {
    match desc.get("Lctn") {
        Some(PsdDescriptorValue::UnitFloat {
            unit: [b'#', b'P', b'r', b'c'],
            value,
        }) if value.is_finite() => ((value.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8,
        _ => {
            let lctn = desc.number("Lctn").unwrap_or(0.0);
            ((lctn.clamp(0.0, 4096.0) / 4096.0) * 255.0).round() as u8
        }
    }
}

/// Extract `(colors, alphas, ratios)` for `FilterSpec::GradientGlow` from a `Grad` (`Grdn`) descriptor.
fn extract_gradient_stops(
    grad: &PsdDescriptor,
    overall_alpha: f64,
    reverse: bool,
) -> Option<(Vec<u32>, Vec<f64>, Vec<u8>)> {
    let mut color_stops = Vec::<(u8, u32)>::new();
    if let Some(PsdDescriptorValue::List(clrs)) = grad.get("Clrs") {
        for item in clrs {
            if let PsdDescriptorValue::Object(stop_obj) = item {
                let c = stop_obj
                    .object("Clr ")
                    .map(|co| descriptor_color_to_rgb24(co, 0xff_ffff))
                    .unwrap_or(0xff_ffff);
                let mut ratio = lctn_to_ratio(stop_obj);
                if reverse {
                    ratio = 255 - ratio;
                }
                color_stops.push((ratio, c));
            }
        }
    }
    if color_stops.is_empty() {
        return None;
    }
    color_stops.sort_by_key(|s| s.0);

    let mut alpha_stops = Vec::<(u8, f64)>::new();
    if let Some(PsdDescriptorValue::List(trns)) = grad.get("Trns") {
        for item in trns {
            if let PsdDescriptorValue::Object(stop_obj) = item {
                let opct = (stop_obj.number("Opct").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0);
                let mut ratio = lctn_to_ratio(stop_obj);
                if reverse {
                    ratio = 255 - ratio;
                }
                alpha_stops.push((ratio, opct));
            }
        }
    }
    alpha_stops.sort_by_key(|s| s.0);

    let mut all_ratios: Vec<u8> = color_stops
        .iter()
        .map(|s| s.0)
        .chain(alpha_stops.iter().map(|s| s.0))
        .collect();
    all_ratios.sort_unstable();
    all_ratios.dedup();
    if all_ratios.len() == 1 {
        let only = all_ratios[0];
        if only == 0 {
            all_ratios.push(255);
        } else {
            all_ratios.insert(0, 0);
        }
    }
    if all_ratios.len() > 16 {
        let last_idx = all_ratios.len() - 1;
        let sampled: Vec<u8> = (0..16).map(|i| all_ratios[(i * last_idx) / 15]).collect();
        all_ratios = sampled;
    }

    let overall = overall_alpha.clamp(0.0, 1.0);
    let mut colors = Vec::with_capacity(all_ratios.len());
    let mut alphas = Vec::with_capacity(all_ratios.len());
    for &r in &all_ratios {
        colors.push(sample_color_stops(&color_stops, r) & 0xff_ffff);
        alphas.push((sample_alpha_stops(&alpha_stops, r) * overall).clamp(0.0, 1.0));
    }
    Some((colors, alphas, all_ratios))
}

fn choke_to_strength(choke_pct: f64) -> f64 {
    let choke = (choke_pct.clamp(0.0, 100.0)) / 100.0;
    (1.0 + choke * 2.0).clamp(0.0, 255.0)
}

fn resolve_effect_light_angle(
    effect: &PsdDescriptor,
    root_global_angle: Option<f64>,
    doc_global_angle: Option<f64>,
) -> f64 {
    let use_global = effect.bool_or("uglg", false);
    if use_global && let Some(ga) = root_global_angle.or(doc_global_angle) {
        return ga;
    }
    effect
        .number("lagl")
        .or(root_global_angle)
        .or(doc_global_angle)
        .unwrap_or(120.0)
}

fn push_bounded_filter(
    out: &mut PsdParsedLayerEffects,
    spec: FilterSpec,
    excess_count: &mut usize,
) {
    if spec.validate().is_err() {
        return;
    }
    if out.filters.len() < MAX_OBJECT_FILTERS {
        out.filters.push(spec);
    } else {
        *excess_count += 1;
    }
}

fn make_tint_and_matrix(rgb24: u32, opacity_01: f64) -> (ColorTransform, FilterSpec) {
    let t = opacity_01.clamp(0.0, 1.0);
    let inv = (1.0 - t).clamp(0.0, 1.0);
    let r = ((rgb24 >> 16) & 0xff) as f64;
    let g = ((rgb24 >> 8) & 0xff) as f64;
    let b = (rgb24 & 0xff) as f64;
    let ct = ColorTransform {
        multipliers: [inv, inv, inv, 1.0],
        offsets: [
            (r * t).clamp(0.0, 255.0),
            (g * t).clamp(0.0, 255.0),
            (b * t).clamp(0.0, 255.0),
            0.0,
        ],
    };
    let cm = FilterSpec::ColorMatrix {
        matrix: [
            inv,
            0.0,
            0.0,
            0.0,
            r * t, // R
            0.0,
            inv,
            0.0,
            0.0,
            g * t, // G
            0.0,
            0.0,
            inv,
            0.0,
            b * t, // B
            0.0,
            0.0,
            0.0,
            1.0,
            0.0, // A
        ],
    };
    (ct, cm)
}

fn is_normal_effect_blend(mode: Option<&str>) -> bool {
    match mode {
        None => true,
        Some(m) => matches!(m.trim(), "Nrml" | "norm" | "pass"),
    }
}

fn convert_single_effect_object(
    key: &str,
    eff: &PsdDescriptor,
    scale: f64,
    root_global_angle: Option<f64>,
    doc_global_angle: Option<f64>,
    out: &mut PsdParsedLayerEffects,
    excess_filters: &mut usize,
) {
    if !eff.bool_or("enab", true) || !eff.bool_or("present", true) {
        return;
    }

    match key {
        "DrSh" | "dropShadow" | "dropShadowMulti" | "IrSh" | "innerShadow" | "innerShadowMulti" => {
            let inner = matches!(key, "IrSh" | "innerShadow" | "innerShadowMulti");
            let color = eff
                .object("Clr ")
                .map(|c| descriptor_color_to_rgb24(c, 0x00_0000))
                .unwrap_or(0x00_0000);
            let alpha = (eff.number("Opct").unwrap_or(75.0) / 100.0).clamp(0.0, 1.0);
            let lagl = resolve_effect_light_angle(eff, root_global_angle, doc_global_angle);
            let angle = photoshop_light_to_flash_angle(lagl);
            let distance = (eff.number("Dstn").unwrap_or(5.0) * scale).clamp(-8192.0, 8192.0);
            let blur_size = eff.number("blur").unwrap_or(5.0).clamp(0.0, 255.0);
            let strength = choke_to_strength(eff.number("Ckmt").unwrap_or(0.0));
            let knockout = eff.bool_or("knko", false);
            push_bounded_filter(
                out,
                FilterSpec::DropShadow {
                    distance,
                    angle,
                    color,
                    alpha,
                    blur_x: blur_size,
                    blur_y: blur_size,
                    strength,
                    quality: 2,
                    inner,
                    knockout,
                    hide_object: false,
                },
                excess_filters,
            );
        }
        "OrGl" | "outerGlow" | "outerGlowMulti" | "IrGl" | "innerGlow" | "innerGlowMulti" => {
            let inner = matches!(key, "IrGl" | "innerGlow" | "innerGlowMulti");
            let alpha = (eff.number("Opct").unwrap_or(75.0) / 100.0).clamp(0.0, 1.0);
            let blur_size = eff.number("blur").unwrap_or(5.0).clamp(0.0, 255.0);
            let strength = choke_to_strength(eff.number("Ckmt").unwrap_or(0.0));
            let reverse = eff.bool_or("Rvrs", false);
            if let Some(grad) = eff.object("Grad")
                && let Some((colors, alphas, ratios)) = extract_gradient_stops(grad, alpha, reverse)
            {
                push_bounded_filter(
                    out,
                    FilterSpec::GradientGlow {
                        distance: 0.0,
                        angle: 45.0,
                        colors,
                        alphas,
                        ratios,
                        blur_x: blur_size,
                        blur_y: blur_size,
                        strength,
                        quality: 2,
                        placement: if inner {
                            "inner".to_owned()
                        } else {
                            "outer".to_owned()
                        },
                        knockout: eff.bool_or("knko", false),
                    },
                    excess_filters,
                );
            } else {
                let color = eff
                    .object("Clr ")
                    .map(|c| descriptor_color_to_rgb24(c, 0xff_ffbe))
                    .unwrap_or(0xff_ffbe);
                push_bounded_filter(
                    out,
                    FilterSpec::Glow {
                        color,
                        alpha,
                        blur_x: blur_size,
                        blur_y: blur_size,
                        strength,
                        quality: 2,
                        inner,
                        knockout: eff.bool_or("knko", false),
                    },
                    excess_filters,
                );
            }
        }
        "ebbl" | "bevelEmboss" | "bevelEmbossMulti" => {
            let highlight_color = eff
                .object("hglC")
                .map(|c| descriptor_color_to_rgb24(c, 0xff_ffff))
                .unwrap_or(0xff_ffff);
            let highlight_alpha = (eff.number("hglO").unwrap_or(75.0) / 100.0).clamp(0.0, 1.0);
            let shadow_color = eff
                .object("sdwC")
                .map(|c| descriptor_color_to_rgb24(c, 0x00_0000))
                .unwrap_or(0x00_0000);
            let shadow_alpha = (eff.number("sdwO").unwrap_or(75.0) / 100.0).clamp(0.0, 1.0);
            let blur_raw = eff.number("blur").unwrap_or(5.0).max(0.0);
            let soften = eff.number("sftn").unwrap_or(0.0).max(0.0);
            let blur_xy = (blur_raw + soften * 0.5).clamp(0.0, 255.0);
            let distance = (blur_raw * scale).clamp(0.0, 8192.0);
            let depth_pct = eff.number_any(&["srgR", "srfs"]).unwrap_or(100.0);
            let strength = (depth_pct / 100.0).clamp(0.0, 255.0);
            let lagl = resolve_effect_light_angle(eff, root_global_angle, doc_global_angle);
            let dir_offset = if eff.enum_value("bvlD") == Some("Out ") {
                180.0
            } else {
                0.0
            };
            let angle = photoshop_light_to_flash_angle(lagl - dir_offset);
            let style_enum = eff.enum_value("bvlS").unwrap_or("InrB");
            let placement = match style_enum {
                "OtrB" => "outer".to_owned(),
                "Embs" | "PlEb" => "full".to_owned(),
                "SfBL" => {
                    out.has_unreconstructable_effect = true;
                    out.approximations.push(
                        "Photoshop Stroke Emboss ('SfBL') has no direct stroke-only bevel in NeoFlash; approximated as Inner Bevel.".to_owned(),
                    );
                    "inner".to_owned()
                }
                _ => "inner".to_owned(),
            };
            push_bounded_filter(
                out,
                FilterSpec::Bevel {
                    distance,
                    angle,
                    highlight_color,
                    highlight_alpha,
                    shadow_color,
                    shadow_alpha,
                    blur_x: blur_xy,
                    blur_y: blur_xy,
                    strength,
                    quality: 2,
                    placement,
                    knockout: false,
                },
                excess_filters,
            );
        }
        "SoFi" | "solidFill" | "solidFillMulti" => {
            let color = eff
                .object("Clr ")
                .map(|c| descriptor_color_to_rgb24(c, 0xff_0000))
                .unwrap_or(0xff_0000);
            let opacity = (eff.number("Opct").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0);
            let blend = eff.enum_value("Md  ");
            let (tint, cm) = make_tint_and_matrix(color, opacity);
            if is_normal_effect_blend(blend) {
                out.color_overlay_tint =
                    Some(out.color_overlay_tint.map_or(tint, |prev| prev.then(tint)));
                out.folder_color_overlay_filter = Some(cm);
            } else {
                out.has_unreconstructable_effect = true;
                let mode_label = blend.unwrap_or("unknown");
                if matches!(mode_label, "Mult" | "mult" | "mul ") {
                    let r = ((color >> 16) & 0xff) as f64 / 255.0;
                    let g = ((color >> 8) & 0xff) as f64 / 255.0;
                    let b = (color & 0xff) as f64 / 255.0;
                    let mult_ct = ColorTransform {
                        multipliers: [
                            (1.0 - opacity + opacity * r).clamp(0.0, 1.0),
                            (1.0 - opacity + opacity * g).clamp(0.0, 1.0),
                            (1.0 - opacity + opacity * b).clamp(0.0, 1.0),
                            1.0,
                        ],
                        offsets: [0.0; 4],
                    };
                    out.color_overlay_tint = Some(
                        out.color_overlay_tint
                            .map_or(mult_ct, |prev| prev.then(mult_ct)),
                    );
                } else {
                    out.color_overlay_tint =
                        Some(out.color_overlay_tint.map_or(tint, |prev| prev.then(tint)));
                }
                out.approximations.push(format!(
                    "Photoshop Color Overlay ('SoFi') blend mode '{mode_label}' cannot be expressed as an exact linear ColorTransform tint and was approximated."
                ));
            }
        }
        "GrFl" | "gradientFill" | "gradientFillMulti" => {
            out.has_unreconstructable_effect = true;
            if let Some(grad) = eff.object("Grad")
                && let Some((colors, _, _)) = extract_gradient_stops(grad, 1.0, false)
                && !colors.is_empty()
            {
                let mid = colors[colors.len() / 2];
                let opacity = (eff.number("Opct").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0);
                let (tint, cm) = make_tint_and_matrix(mid, opacity * 0.5);
                out.color_overlay_tint =
                    Some(out.color_overlay_tint.map_or(tint, |prev| prev.then(tint)));
                out.folder_color_overlay_filter = Some(cm);
            }
            out.approximations.push(
                "Photoshop Gradient Overlay ('GrFl') has no interior gradient-fill filter equivalent in NeoFlash and was approximated.".to_owned(),
            );
        }
        "ChFX" | "chromeFX" | "chromeFXMulti" => {
            out.has_unreconstructable_effect = true;
            out.approximations.push(
                "Photoshop Satin ('ChFX') layer style has no direct Flash/NeoFlash filter equivalent and was omitted.".to_owned(),
            );
        }
        "FrFX" | "frameFX" | "frameFXMulti" => {
            out.has_unreconstructable_effect = true;
            out.approximations.push(
                "Photoshop Stroke ('FrFX') layer style is not a separable blur/shadow filter and was omitted with an explicit approximation diagnostic.".to_owned(),
            );
        }
        "patternFill" | "patternFillMulti" => {
            out.has_unreconstructable_effect = true;
            out.approximations.push(
                "Photoshop Pattern Overlay ('patternFill') layer style has no runtime filter equivalent and was omitted.".to_owned(),
            );
        }
        _ => {}
    }
}

/// Convert a parsed `lfx2` or `lmfx` [`PsdDescriptor`] into [`PsdParsedLayerEffects`].
pub fn convert_lfx2_descriptor(
    desc: &PsdDescriptor,
    doc_global_angle: Option<f64>,
) -> PsdParsedLayerEffects {
    let mut out = PsdParsedLayerEffects::default();
    if !desc.bool_or("masterFXSwitch", true) || !desc.bool_or("enab", true) {
        return out;
    }
    let scale = desc
        .number("Scl ")
        .filter(|&s| s > 0.0)
        .map_or(1.0, |s| (s / 100.0).clamp(0.01, 100.0));
    let root_global_angle = desc.number("gblA").or_else(|| desc.number("lagl"));
    let mut excess_filters = 0usize;

    // If a `*Multi` list key exists alongside its single-effect backward-compatibility key in the
    // same descriptor, prefer the `*Multi` list so effects are not duplicated.
    let has_multi_drsh = matches!(
        desc.get("dropShadowMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_irsh = matches!(
        desc.get("innerShadowMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_orgl = matches!(
        desc.get("outerGlowMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_irgl = matches!(
        desc.get("innerGlowMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_ebbl = matches!(
        desc.get("bevelEmbossMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_sofi = matches!(
        desc.get("solidFillMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_grfl = matches!(
        desc.get("gradientFillMulti"),
        Some(PsdDescriptorValue::List(_))
    );
    let has_multi_chfx = matches!(desc.get("chromeFXMulti"), Some(PsdDescriptorValue::List(_)));
    let has_multi_frfx = matches!(desc.get("frameFXMulti"), Some(PsdDescriptorValue::List(_)));

    for (key, value) in &desc.items {
        if (key == "DrSh" && has_multi_drsh)
            || (key == "IrSh" && has_multi_irsh)
            || (key == "OrGl" && has_multi_orgl)
            || (key == "IrGl" && has_multi_irgl)
            || (key == "ebbl" && has_multi_ebbl)
            || (key == "SoFi" && has_multi_sofi)
            || (key == "GrFl" && has_multi_grfl)
            || (key == "ChFX" && has_multi_chfx)
            || (key == "FrFX" && has_multi_frfx)
        {
            continue;
        }
        match value {
            PsdDescriptorValue::Object(eff_obj) => {
                convert_single_effect_object(
                    key,
                    eff_obj,
                    scale,
                    root_global_angle,
                    doc_global_angle,
                    &mut out,
                    &mut excess_filters,
                );
            }
            PsdDescriptorValue::List(items) => {
                for elem in items {
                    if let PsdDescriptorValue::Object(eff_obj) = elem {
                        convert_single_effect_object(
                            key,
                            eff_obj,
                            scale,
                            root_global_angle,
                            doc_global_angle,
                            &mut out,
                            &mut excess_filters,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    if excess_filters > 0 {
        out.has_unreconstructable_effect = true;
        out.approximations.push(format!(
            "PSD layer carries {} active filter effects beyond NeoFlash's {MAX_OBJECT_FILTERS}-filter limit; kept the first {MAX_OBJECT_FILTERS} filters.",
            excess_filters
        ));
    }
    out
}

fn parse_lrfx_color_10(raw: &[u8]) -> u32 {
    if raw.len() < 10 {
        return 0;
    }
    let space = u16::from_be_bytes([raw[0], raw[1]]);
    let w0 = u16::from_be_bytes([raw[2], raw[3]]);
    let w1 = u16::from_be_bytes([raw[4], raw[5]]);
    let w2 = u16::from_be_bytes([raw[6], raw[7]]);
    let w3 = u16::from_be_bytes([raw[8], raw[9]]);
    match space {
        0 => {
            // Standard Photoshop 16-bit RGB uses 0..=65535 (each nonzero 8-bit value * 257 >= 257);
            // also tolerate synthetic 0..=255 stored directly in the low byte.
            let (r, g, b) = if (w0 | w1 | w2) > 0 && w0 <= 255 && w1 <= 255 && w2 <= 255 {
                (u32::from(w0), u32::from(w1), u32::from(w2))
            } else {
                (u32::from(w0 >> 8), u32::from(w1 >> 8), u32::from(w2 >> 8))
            };
            (r << 16) | (g << 8) | b
        }
        1 => hsv_to_rgb24(
            (f64::from(w0) / 65535.0) * 360.0,
            f64::from(w1) / 65535.0,
            f64::from(w2) / 65535.0,
        ),
        2 => {
            // Legacy Photoshop u16 CMYK stores 0 = 100% ink, 65535 = 0% ink.
            let c = 1.0 - f64::from(w0) / 65535.0;
            let m = 1.0 - f64::from(w1) / 65535.0;
            let y = 1.0 - f64::from(w2) / 65535.0;
            let k = 1.0 - f64::from(w3) / 65535.0;
            let r = (255.0 * (1.0 - c) * (1.0 - k)).round().clamp(0.0, 255.0) as u32;
            let g = (255.0 * (1.0 - m) * (1.0 - k)).round().clamp(0.0, 255.0) as u32;
            let b = (255.0 * (1.0 - y) * (1.0 - k)).round().clamp(0.0, 255.0) as u32;
            (r << 16) | (g << 8) | b
        }
        8 => {
            let level = ((1.0 - (f64::from(w0) / 10000.0).clamp(0.0, 1.0)) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u32;
            (level << 16) | (level << 8) | level
        }
        _ => 0,
    }
}

fn decode_lrfx_scalar(raw: i32, fixed16: bool) -> f64 {
    if fixed16 {
        f64::from(raw) / 65536.0
    } else {
        f64::from(raw)
    }
}

/// Parse a legacy Photoshop 5.0–7.0 `lrFX` (*Effects Layer*) block when `lfx2`/`lmfx` is absent.
pub fn parse_legacy_lrfx_block(
    block: &[u8],
    doc_global_angle: Option<f64>,
) -> Option<PsdParsedLayerEffects> {
    let mut pos = 0usize;
    let version = read_u16_be(block, &mut pos)?;
    if version != 0 {
        return None;
    }
    let count = read_u16_be(block, &mut pos)? as usize;
    if count > MAX_LRFX_SUBBLOCKS
        || count
            .checked_mul(12)
            .is_none_or(|min_b| min_b > block.len().saturating_sub(pos))
    {
        return None;
    }

    let mut out = PsdParsedLayerEffects::default();
    let mut master_visible = true;
    let mut excess_filters = 0usize;

    for _ in 0..count {
        let sig = read_fourcc(block, &mut pos)?;
        if &sig != b"8BIM" {
            return None;
        }
        let key = read_fourcc(block, &mut pos)?;
        let sub_len = read_u32_be(block, &mut pos)? as usize;
        let sub_end = pos.checked_add(sub_len)?;
        let sub = block.get(pos..sub_end)?;

        // Advance `pos` across optional even/4-byte sub-record padding inside `lrFX`.
        let mut next_pos = sub_end;
        if block.get(next_pos..next_pos + 4) != Some(b"8BIM") {
            let even_end = sub_end + (sub_len & 1);
            if even_end <= block.len() {
                next_pos = even_end;
                if block.get(next_pos..next_pos + 2) == Some(&[0, 0])
                    && block.get(next_pos + 2..next_pos + 6) == Some(b"8BIM")
                {
                    next_pos += 2;
                }
            }
        }
        pos = next_pos;

        match &key {
            b"cmnS" => {
                if sub.len() >= 5 {
                    master_visible = sub[4] != 0;
                }
            }
            b"dsdw" | b"isdw" => {
                if sub.len() < 41 {
                    return None;
                }
                let sub_ver = u32::from_be_bytes([sub[0], sub[1], sub[2], sub[3]]);
                let blur_raw = i32::from_be_bytes([sub[4], sub[5], sub[6], sub[7]]);
                let inten_raw = i32::from_be_bytes([sub[8], sub[9], sub[10], sub[11]]);
                let angle_raw = i32::from_be_bytes([sub[12], sub[13], sub[14], sub[15]]);
                let dist_raw = i32::from_be_bytes([sub[16], sub[17], sub[18], sub[19]]);
                let enabled = sub[38] != 0;
                if !master_visible || !enabled {
                    continue;
                }
                let use_global = sub[39] != 0;
                let opacity = (f64::from(sub[40]) / 255.0).clamp(0.0, 1.0);
                let fixed16 = blur_raw.abs() > 1024
                    || inten_raw.abs() > 1024
                    || angle_raw.abs() > 360
                    || dist_raw.abs() > 8192
                    || ((blur_raw | angle_raw | dist_raw) != 0
                        && ((blur_raw | angle_raw | dist_raw) & 0xffff) == 0);
                let blur_px = decode_lrfx_scalar(blur_raw, fixed16).clamp(0.0, 255.0);
                let intensity = decode_lrfx_scalar(inten_raw, fixed16);
                let light_deg = if use_global && let Some(ga) = doc_global_angle {
                    ga
                } else {
                    decode_lrfx_scalar(angle_raw, fixed16)
                };
                let distance = decode_lrfx_scalar(dist_raw, fixed16).clamp(-8192.0, 8192.0);
                let color_slice = if sub_ver >= 2 && sub.len() >= 51 {
                    &sub[41..51]
                } else {
                    &sub[20..30]
                };
                let color = parse_lrfx_color_10(color_slice);
                let strength = choke_to_strength(intensity);
                push_bounded_filter(
                    &mut out,
                    FilterSpec::DropShadow {
                        distance,
                        angle: photoshop_light_to_flash_angle(light_deg),
                        color,
                        alpha: opacity,
                        blur_x: blur_px,
                        blur_y: blur_px,
                        strength,
                        quality: 2,
                        inner: &key == b"isdw",
                        knockout: false,
                        hide_object: false,
                    },
                    &mut excess_filters,
                );
            }
            b"oglw" | b"iglw" => {
                if sub.len() < 32 {
                    return None;
                }
                let sub_ver = u32::from_be_bytes([sub[0], sub[1], sub[2], sub[3]]);
                let blur_raw = i32::from_be_bytes([sub[4], sub[5], sub[6], sub[7]]);
                let inten_raw = i32::from_be_bytes([sub[8], sub[9], sub[10], sub[11]]);
                let enabled = sub[30] != 0;
                if !master_visible || !enabled {
                    continue;
                }
                let opacity = (f64::from(sub[31]) / 255.0).clamp(0.0, 1.0);
                let fixed16 = blur_raw.abs() > 1024
                    || inten_raw.abs() > 1024
                    || ((blur_raw | inten_raw) != 0 && ((blur_raw | inten_raw) & 0xffff) == 0);
                let blur_px = decode_lrfx_scalar(blur_raw, fixed16).clamp(0.0, 255.0);
                let intensity = decode_lrfx_scalar(inten_raw, fixed16);
                let color_slice = if &key == b"oglw" && sub_ver >= 2 && sub.len() >= 42 {
                    &sub[32..42]
                } else if &key == b"iglw" && sub_ver >= 2 && sub.len() >= 43 {
                    &sub[33..43]
                } else {
                    &sub[12..22]
                };
                let color = parse_lrfx_color_10(color_slice);
                push_bounded_filter(
                    &mut out,
                    FilterSpec::Glow {
                        color,
                        alpha: opacity,
                        blur_x: blur_px,
                        blur_y: blur_px,
                        strength: choke_to_strength(intensity),
                        quality: 2,
                        inner: &key == b"iglw",
                        knockout: false,
                    },
                    &mut excess_filters,
                );
            }
            b"bevl" => {
                if sub.len() < 58 {
                    return None;
                }
                let sub_ver = u32::from_be_bytes([sub[0], sub[1], sub[2], sub[3]]);
                let angle_raw = i32::from_be_bytes([sub[4], sub[5], sub[6], sub[7]]);
                let strength_raw = i32::from_be_bytes([sub[8], sub[9], sub[10], sub[11]]);
                let blur_raw = i32::from_be_bytes([sub[12], sub[13], sub[14], sub[15]]);
                let bevel_style = sub[52];
                let hgl_opacity = (f64::from(sub[53]) / 255.0).clamp(0.0, 1.0);
                let sdw_opacity = (f64::from(sub[54]) / 255.0).clamp(0.0, 1.0);
                let enabled = sub[55] != 0;
                if !master_visible || !enabled {
                    continue;
                }
                let use_global = sub[56] != 0;
                let fixed16 = angle_raw.abs() > 360
                    || strength_raw.abs() > 1024
                    || blur_raw.abs() > 1024
                    || ((angle_raw | strength_raw | blur_raw) != 0
                        && ((angle_raw | strength_raw | blur_raw) & 0xffff) == 0);
                let light_deg = if use_global && let Some(ga) = doc_global_angle {
                    ga
                } else {
                    decode_lrfx_scalar(angle_raw, fixed16)
                };
                let depth_val = decode_lrfx_scalar(strength_raw, fixed16).max(0.0);
                let blur_px = decode_lrfx_scalar(blur_raw, fixed16).clamp(0.0, 255.0);
                let (hgl_slice, sdw_slice) = if sub_ver >= 2 && sub.len() >= 78 {
                    (&sub[58..68], &sub[68..78])
                } else {
                    (&sub[32..42], &sub[42..52])
                };
                let highlight_color = parse_lrfx_color_10(hgl_slice);
                let shadow_color = parse_lrfx_color_10(sdw_slice);
                // Legacy lrFX bevel styles: 1 = Outer, 2 = Inner, 3 = Emboss, 4 = Pillow Emboss.
                let placement = match bevel_style {
                    1 => "outer".to_owned(),
                    3 | 4 => "full".to_owned(),
                    _ => "inner".to_owned(),
                };
                let strength = if depth_val > 16.0 {
                    (depth_val / 100.0).clamp(0.0, 255.0)
                } else if depth_val > 0.0 {
                    depth_val.clamp(0.1, 255.0)
                } else {
                    1.0
                };
                push_bounded_filter(
                    &mut out,
                    FilterSpec::Bevel {
                        distance: blur_px.clamp(0.0, 8192.0),
                        angle: photoshop_light_to_flash_angle(light_deg),
                        highlight_color,
                        highlight_alpha: hgl_opacity,
                        shadow_color,
                        shadow_alpha: sdw_opacity,
                        blur_x: blur_px,
                        blur_y: blur_px,
                        strength,
                        quality: 2,
                        placement,
                        knockout: false,
                    },
                    &mut excess_filters,
                );
            }
            b"sofi" => {
                let parsed = if sub.len() >= 24 && &sub[4..8] == b"8BIM" {
                    Some((&sub[8..12], &sub[12..22], sub[22], sub[23] != 0))
                } else if sub.len() >= 20 {
                    Some((&sub[4..8], &sub[8..18], sub[18], sub[19] != 0))
                } else {
                    None
                };
                if let Some((blend_bytes, color_bytes, opct_u8, enabled)) = parsed {
                    if !master_visible || !enabled {
                        continue;
                    }
                    let color = parse_lrfx_color_10(color_bytes);
                    let opacity = (f64::from(opct_u8) / 255.0).clamp(0.0, 1.0);
                    let blend_str = std::str::from_utf8(blend_bytes).unwrap_or("norm");
                    let (tint, cm) = make_tint_and_matrix(color, opacity);
                    if is_normal_effect_blend(Some(blend_str)) {
                        out.color_overlay_tint =
                            Some(out.color_overlay_tint.map_or(tint, |prev| prev.then(tint)));
                        out.folder_color_overlay_filter = Some(cm);
                    } else {
                        out.has_unreconstructable_effect = true;
                        out.color_overlay_tint =
                            Some(out.color_overlay_tint.map_or(tint, |prev| prev.then(tint)));
                        out.approximations.push(format!(
                            "Legacy Photoshop Color Overlay ('sofi') blend mode '{blend_str}' was approximated as a linear ColorTransform tint."
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    if !master_visible {
        return Some(PsdParsedLayerEffects::default());
    }
    if excess_filters > 0 {
        out.has_unreconstructable_effect = true;
        out.approximations.push(format!(
            "Legacy PSD 'lrFX' carries {} active effects beyond NeoFlash's {MAX_OBJECT_FILTERS}-filter limit.",
            excess_filters
        ));
    }
    Some(out)
}
