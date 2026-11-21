//! Layered Adobe Photoshop (`.psd` / `.psb`) binary parser and timeline/symbol/keyframe importer (`T-115`, `T-418`).
//!
//! Decodes PSD v1 and PSB v2 headers, layer records, Pascal and `luni` Unicode layer names,
//! `lsct` folder hierarchies, blend modes, opacity, visibility flags, clipping mask chains,
//! `TySh` text descriptors, `lfx2`/`lmfx`/`lrFX` editable Layer Styles (`FilterSpec` & `ColorTransform`),
//! `vmsk`/`vsms` vector mask notices, raw (`0`) and PackBits RLE (`1`) channel image planes,
//! and merged composite image data.

pub mod layer_styles;
pub use layer_styles::{
    PsdDescriptor, PsdDescriptorValue, PsdParsedLayerEffects, convert_lfx2_descriptor,
    descriptor_color_to_rgb24, extract_psd_global_lighting_angle, parse_descriptor_body,
    parse_legacy_lrfx_block, parse_versioned_effects_descriptor, photoshop_light_to_flash_angle,
};

use crate::{ImportError, ImportFormat, ImportReport, Statistics};
use neoflash_core::{
    BlendMode, Color, ColorTransform, Element, ElementKind, FillRule, FilterSpec, InstancePlayback,
    Keyframe, Layer, LayerProperties, LayerRole, MAX_BITMAP_DIMENSION, MAX_OBJECT_FILTERS,
    MaskMode, PathCommand, Project, ResourceUnit, Symbol, SymbolKind, Timeline, Transform, new_id,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

/// Maximum aggregate allocation for decoded PSD channel planes and retained
/// RGBA buffers. Compressed source bytes and the final encoded project
/// resources have their own independent document limits.
const MAX_PSD_DECODE_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const MAX_PSD_FILE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PsdDecodePlan {
    All,
    Automatic,
    CompositeOnly,
    LayersWithCompositeFallback,
}

#[derive(Debug, Default)]
struct PsdDecodeReport {
    budget_skipped_layers: BTreeSet<usize>,
    composite_budget_skipped: bool,
    used_merged_appearance: bool,
}

/// How PSD layers are mapped into the NeoFlash document structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsdLayerMapping {
    /// Keep editable layers where possible, or use the saved composition for unsupported visible artwork.
    #[default]
    Automatic,
    /// Each PSD layer or folder group becomes a NeoFlash `Layer` on the target timeline.
    TimelineLayers,
    /// Each PSD visual layer is wrapped in its own `SymbolKind::MovieClip` symbol and placed on its layer.
    MovieClipSymbols,
    /// Each PSD visual layer becomes a sequential `Keyframe` on a single timeline layer.
    Keyframes,
    /// Ignore individual layers and import the merged PSD composite image onto a single layer.
    FlattenComposite,
}

/// How PSD `TySh` text layers are converted during import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsdTextHandling {
    /// Convert `TySh` text layers into editable `ElementKind::Text` elements.
    #[default]
    EditableText,
    /// Convert `TySh` text layers into editable vector `ElementKind::Path` glyph contours.
    VectorOutlines,
    /// Keep the rasterized pixel channels of the PSD text layer as a `Bitmap`.
    RasterizeLayer,
}

/// Options controlling layered PSD import (`T-115`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PsdImportOptions {
    pub layer_mapping: PsdLayerMapping,
    pub text_handling: PsdTextHandling,
    /// Import invisible PSD layers (`visible = false` in NeoFlash) when `true`; skip them when `false`.
    pub import_hidden_layers: bool,
    /// Map PSD blend mode keys to NeoFlash `BlendMode`s (emitting a warning when approximated).
    pub preserve_blend_modes: bool,
    /// Map PSD clipping masks (`clipping == 1`) to a `LayerRole::Mask` (`MaskMode::Alpha`) base
    /// layer and masked target layers.
    pub preserve_clipping_masks: bool,
    /// Resize the destination `Project` width and height to match the PSD canvas dimensions.
    pub resize_stage_to_canvas: bool,
}

impl Default for PsdImportOptions {
    fn default() -> Self {
        Self {
            layer_mapping: PsdLayerMapping::Automatic,
            text_handling: PsdTextHandling::EditableText,
            import_hidden_layers: true,
            preserve_blend_modes: true,
            preserve_clipping_masks: true,
            resize_stage_to_canvas: true,
        }
    }
}

/// Parsed PSD/PSB file header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PsdHeader {
    pub version: u16,
    pub channels: u16,
    pub height: u32,
    pub width: u32,
    pub depth: u16,
    pub color_mode: u16,
}

/// Extracted `TySh` text layer metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PsdTextLayerData {
    pub text: String,
    pub font_family: String,
    pub font_size: f64,
    pub color: Color,
    pub transform: [f64; 6],
}

/// Section divider (`lsct` / `lsdk`) kind for PSD layer groups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsdSectionDivider {
    #[default]
    Normal,
    OpenFolder,
    ClosedFolder,
    BoundingSectionEnd,
}

/// Parsed metadata and reconstructed RGBA8 pixels for one PSD layer record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PsdLayerRecord {
    pub name: String,
    pub top: i32,
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub blend_key: String,
    pub opacity: u8,
    pub clipping: bool,
    pub visible: bool,
    pub section_divider: PsdSectionDivider,
    pub text_data: Option<PsdTextLayerData>,
    pub has_vector_mask: bool,
    pub has_layer_mask: bool,
    pub is_adjustment_layer: Option<String>,
    pub is_smart_object: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<FilterSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_overlay_tint: Option<ColorTransform>,
    #[serde(skip)]
    pub folder_color_overlay_filter: Option<FilterSpec>,
    #[serde(skip)]
    pub effects_approximations: Vec<String>,
    #[serde(skip)]
    pub solid_fill_color: Option<Color>,
    #[serde(skip)]
    pub channels_meta: Vec<(i16, usize)>,
    #[serde(skip)]
    pub rgba_pixels: Option<Vec<u8>>,
}

impl PsdLayerRecord {
    pub fn width(&self) -> u32 {
        (i64::from(self.right) - i64::from(self.left)).clamp(0, i64::from(u32::MAX)) as u32
    }

    pub fn height(&self) -> u32 {
        (i64::from(self.bottom) - i64::from(self.top)).clamp(0, i64::from(u32::MAX)) as u32
    }
}

/// Parsed PSD header, layer records, and optional merged composite RGBA pixels.
pub type ParsedPsdDocument = (PsdHeader, Vec<PsdLayerRecord>, Option<Vec<u8>>);
type ParsedPsdDocumentWithReport = (
    PsdHeader,
    Vec<PsdLayerRecord>,
    Option<Vec<u8>>,
    PsdDecodeReport,
);

fn portable_pixel_count(width: usize, height: usize) -> Option<usize> {
    if width == 0
        || height == 0
        || width > MAX_BITMAP_DIMENSION as usize
        || height > MAX_BITMAP_DIMENSION as usize
    {
        return None;
    }
    width.checked_mul(height)
}

fn checked_section_end(
    start: usize,
    len: usize,
    limit: usize,
    section: &str,
) -> Result<usize, ImportError> {
    start
        .checked_add(len)
        .filter(|end| *end <= limit)
        .ok_or_else(|| ImportError::Container(format!("PSD {section} section exceeds file bounds")))
}

fn read_u8(data: &[u8], pos: &mut usize) -> Result<u8, ImportError> {
    if *pos + 1 > data.len() {
        return Err(ImportError::Container(
            "Unexpected EOF in PSD stream".into(),
        ));
    }
    let v = data[*pos];
    *pos += 1;
    Ok(v)
}

fn read_u16_be(data: &[u8], pos: &mut usize) -> Result<u16, ImportError> {
    if *pos + 2 > data.len() {
        return Err(ImportError::Container(
            "Unexpected EOF reading u16 in PSD".into(),
        ));
    }
    let v = u16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

fn read_i16_be(data: &[u8], pos: &mut usize) -> Result<i16, ImportError> {
    if *pos + 2 > data.len() {
        return Err(ImportError::Container(
            "Unexpected EOF reading i16 in PSD".into(),
        ));
    }
    let v = i16::from_be_bytes([data[*pos], data[*pos + 1]]);
    *pos += 2;
    Ok(v)
}

fn read_u32_be(data: &[u8], pos: &mut usize) -> Result<u32, ImportError> {
    if *pos + 4 > data.len() {
        return Err(ImportError::Container(
            "Unexpected EOF reading u32 in PSD".into(),
        ));
    }
    let v = u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(v)
}

fn read_i32_be(data: &[u8], pos: &mut usize) -> Result<i32, ImportError> {
    if *pos + 4 > data.len() {
        return Err(ImportError::Container(
            "Unexpected EOF reading i32 in PSD".into(),
        ));
    }
    let v = i32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]);
    *pos += 4;
    Ok(v)
}

fn read_u64_be(data: &[u8], pos: &mut usize) -> Result<u64, ImportError> {
    if *pos + 8 > data.len() {
        return Err(ImportError::Container(
            "Unexpected EOF reading u64 in PSB".into(),
        ));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[*pos..*pos + 8]);
    *pos += 8;
    Ok(u64::from_be_bytes(bytes))
}

fn read_f64_be(data: &[u8], pos: &mut usize) -> Result<f64, ImportError> {
    let bits = read_u64_be(data, pos)?;
    Ok(f64::from_bits(bits))
}

/// Decode PackBits RLE scanline data into `expected_len` uncompressed bytes.
pub fn decode_packbits_rle(encoded: &[u8], expected_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected_len);
    let mut i = 0usize;
    while i < encoded.len() && out.len() < expected_len {
        let header = encoded[i] as i8;
        i += 1;
        if header >= 0 {
            let count = (header as usize) + 1;
            let available = encoded.len().saturating_sub(i).min(count);
            let limit = (expected_len - out.len()).min(available);
            out.extend_from_slice(&encoded[i..i + limit]);
            i += available;
        } else if header > -128 {
            let count = ((-header) as usize) + 1;
            if i < encoded.len() {
                let val = encoded[i];
                i += 1;
                let limit = (expected_len - out.len()).min(count);
                out.extend(std::iter::repeat_n(val, limit));
            }
        }
    }
    if out.len() < expected_len {
        out.resize(expected_len, 0);
    }
    out
}

/// Encode raw bytes using PackBits RLE per scanline (`height` rows of `row_width` bytes),
/// prefixed with big-endian `u16` row byte counts (for testing and round-trip tools).
pub fn encode_packbits_channel(pixels: &[u8], row_width: usize, height: usize) -> Vec<u8> {
    let mut row_blobs = Vec::with_capacity(height);
    for r in 0..height {
        let start = r * row_width;
        let end = (start + row_width).min(pixels.len());
        let row = if start < pixels.len() {
            &pixels[start..end]
        } else {
            &[]
        };
        let mut encoded_row = Vec::new();
        let mut idx = 0;
        while idx < row.len() {
            let run_val = row[idx];
            let mut run_len = 1usize;
            while idx + run_len < row.len() && row[idx + run_len] == run_val && run_len < 128 {
                run_len += 1;
            }
            if run_len >= 2 {
                let header = (1i16 - run_len as i16) as i8;
                encoded_row.push(header as u8);
                encoded_row.push(run_val);
                idx += run_len;
            } else {
                let lit_start = idx;
                let mut lit_len = 1usize;
                while lit_start + lit_len < row.len() && lit_len < 128 {
                    if lit_start + lit_len + 1 < row.len()
                        && row[lit_start + lit_len] == row[lit_start + lit_len + 1]
                    {
                        break;
                    }
                    lit_len += 1;
                }
                encoded_row.push((lit_len - 1) as u8);
                encoded_row.extend_from_slice(&row[lit_start..lit_start + lit_len]);
                idx += lit_len;
            }
        }
        row_blobs.push(encoded_row);
    }
    let mut out = Vec::new();
    // Compression = 1 (RLE)
    out.extend_from_slice(&1u16.to_be_bytes());
    for blob in &row_blobs {
        out.extend_from_slice(&(blob.len() as u16).to_be_bytes());
    }
    for blob in row_blobs {
        out.extend_from_slice(&blob);
    }
    out
}

fn parse_tysh_block(block: &[u8]) -> Option<PsdTextLayerData> {
    if block.len() < 2 {
        return None;
    }
    let mut pos = 0usize;
    let version = read_u16_be(block, &mut pos).ok()?;
    if version != 1 {
        return None;
    }
    let mut transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    if block.len() >= 50 {
        for slot in &mut transform {
            *slot = read_f64_be(block, &mut pos).unwrap_or(0.0);
        }
    }
    let rest = &block[pos..];
    // Structured version-16 Action Descriptor inside `TySh` (text_version == 50, desc_version == 16)
    if rest.len() >= 6
        && u16::from_be_bytes([rest[0], rest[1]]) == 50
        && u32::from_be_bytes([rest[2], rest[3], rest[4], rest[5]]) == 16
    {
        let mut dpos = 6usize;
        if let Some(desc) = parse_descriptor_body(rest, &mut dpos, 0)
            && let Some(PsdDescriptorValue::Text(txt)) = desc.get("Txt ")
        {
            let decoded = txt.replace('\r', "\n").trim_end_matches('\0').to_owned();
            if !decoded.is_empty() {
                return Some(PsdTextLayerData {
                    text: decoded,
                    font_family: "Arial".to_owned(),
                    font_size: 24.0,
                    color: [255, 255, 255, 255],
                    transform,
                });
            }
        }
    }
    // Look for "Txt " key followed by "TEXT" type tag and u32 UTF-16BE length in compact/standard PSD descriptors
    if let Some(txt_idx) = rest.windows(8).position(|w| w == b"Txt TEXT") {
        let mut tpos = txt_idx + 8;
        if let Ok(char_len) = read_u32_be(rest, &mut tpos)
            && let Some(byte_len) = (char_len as usize).checked_mul(2)
            && let Some(end) = tpos.checked_add(byte_len)
            && end <= rest.len()
        {
            let units: Vec<u16> = rest[tpos..end]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .filter(|&u| u != 0)
                .collect();
            let decoded = String::from_utf16_lossy(&units)
                .replace('\r', "\n")
                .trim_end_matches('\0')
                .to_owned();
            if !decoded.is_empty() {
                return Some(PsdTextLayerData {
                    text: decoded,
                    font_family: "Arial".to_owned(),
                    font_size: 24.0,
                    color: [255, 255, 255, 255],
                    transform,
                });
            }
        }
    }
    // Also support compact UTF-8 text payload after the 6-float transform (or after version+descriptor version)
    let text_slice = if rest.len() >= 6 && read_u16_be(rest, &mut 0).ok() == Some(50) {
        &rest[6..]
    } else {
        rest
    };
    let ascii: String = text_slice
        .iter()
        .copied()
        .filter(|b| b.is_ascii_graphic() || *b == b' ' || *b == b'\n')
        .map(char::from)
        .collect();
    let trimmed = ascii.trim();
    if !trimmed.is_empty() {
        return Some(PsdTextLayerData {
            text: trimmed.to_owned(),
            font_family: "Arial".to_owned(),
            font_size: 24.0,
            color: [255, 255, 255, 255],
            transform,
        });
    }
    None
}

fn is_adjustment_key(key: &[u8; 4]) -> Option<&'static str> {
    match key {
        b"levl" => Some("Levels"),
        b"curv" => Some("Curves"),
        b"brit" => Some("Brightness/Contrast"),
        b"hue " | b"hue2" => Some("Hue/Saturation"),
        b"blnc" => Some("Color Balance"),
        b"nvrt" => Some("Invert"),
        b"post" => Some("Posterize"),
        b"thrs" => Some("Threshold"),
        b"grdm" => Some("Gradient Map"),
        b"selc" => Some("Selective Color"),
        b"phfl" => Some("Photo Filter"),
        b"expA" => Some("Exposure"),
        b"vibA" => Some("Vibrance"),
        _ => None,
    }
}

fn map_psd_blend_mode(key: &str) -> (BlendMode, bool) {
    match key {
        "norm" | "pass" => (BlendMode::Normal, true),
        "mult" | "mul " => (BlendMode::Multiply, true),
        "scrn" => (BlendMode::Screen, true),
        "over" => (BlendMode::Overlay, true),
        "dark" => (BlendMode::Darken, true),
        "lite" => (BlendMode::Lighten, true),
        "lddg" => (BlendMode::Add, true),
        "diff" => (BlendMode::Difference, true),
        "hard" | "hLit" => (BlendMode::Hardlight, true),
        "fsub" | "sub " => (BlendMode::Subtract, true),
        "soft" | "sLit" => (BlendMode::Overlay, false),
        "lbrn" | "idiv" | "dkCl" => (BlendMode::Darken, false),
        "div " | "lgCl" => (BlendMode::Lighten, false),
        "vLit" | "lLit" | "pLit" | "hMix" => (BlendMode::Hardlight, false),
        "smud" | "fdiv" | "hue " | "sat " | "colr" | "lum " => (BlendMode::Normal, false),
        _ => (BlendMode::Normal, false),
    }
}

fn decode_channel_slice(
    raw_block: &[u8],
    width: usize,
    height: usize,
    version: u16,
) -> Result<Vec<u8>, String> {
    let pixel_count = portable_pixel_count(width, height).ok_or_else(|| {
        format!(
            "channel dimensions {width}x{height} exceed the portable bitmap limit {MAX_BITMAP_DIMENSION}x{MAX_BITMAP_DIMENSION}"
        )
    })?;
    if raw_block.len() < 2 {
        return Err("channel block shorter than 2-byte compression header".into());
    }
    let compression = u16::from_be_bytes([raw_block[0], raw_block[1]]);
    let payload = &raw_block[2..];
    match compression {
        0 => {
            let mut out = Vec::with_capacity(pixel_count);
            let copy_len = payload.len().min(pixel_count);
            out.extend_from_slice(&payload[..copy_len]);
            if out.len() < pixel_count {
                out.resize(pixel_count, 0);
            }
            Ok(out)
        }
        1 => {
            let count_table_bytes = height * if version == 2 { 4 } else { 2 };
            let rle_data = if payload.len() >= count_table_bytes {
                &payload[count_table_bytes..]
            } else {
                payload
            };
            Ok(decode_packbits_rle(rle_data, pixel_count))
        }
        2 => {
            let decoder = flate2::read::ZlibDecoder::new(payload);
            let mut out = Vec::new();
            decoder
                .take(pixel_count.saturating_add(1) as u64)
                .read_to_end(&mut out)
                .map_err(|e| format!("zlib channel error: {e}"))?;
            if out.len() > pixel_count {
                return Err("zlib channel expands beyond its declared dimensions".into());
            }
            out.resize(pixel_count, 0);
            Ok(out)
        }
        other => Err(format!("unsupported PSD channel compression {other}")),
    }
}

fn combine_channels_to_rgba(
    width: usize,
    height: usize,
    color_mode: u16,
    decoded_channels: &BTreeMap<i16, Vec<u8>>,
) -> Vec<u8> {
    let pixel_count = portable_pixel_count(width, height).unwrap_or(0);
    let mut rgba = vec![0u8; pixel_count.saturating_mul(4)];
    let r_plane = decoded_channels.get(&0);
    let g_plane = decoded_channels.get(&1);
    let b_plane = decoded_channels.get(&2);
    let a_plane = decoded_channels.get(&-1);

    for i in 0..pixel_count {
        let (r, g, b) = if color_mode == 1 {
            // Grayscale
            let v = r_plane.and_then(|p| p.get(i)).copied().unwrap_or(0);
            (v, v, v)
        } else {
            (
                r_plane.and_then(|p| p.get(i)).copied().unwrap_or(0),
                g_plane.and_then(|p| p.get(i)).copied().unwrap_or(0),
                b_plane.and_then(|p| p.get(i)).copied().unwrap_or(0),
            )
        };
        let a = a_plane.and_then(|p| p.get(i)).copied().unwrap_or(255);
        rgba[i * 4] = r;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = b;
        rgba[i * 4 + 3] = a;
    }
    rgba
}

/// Decode whole PackBits rows, respecting their declared lengths and pixel count.
fn unpack_composite_row(encoded: &[u8], row: &mut Vec<u8>, width: usize) -> Option<()> {
    row.clear();
    let mut src = encoded;
    while let Some((&code, rest)) = src.split_first() {
        src = rest;
        match code {
            0..=127 => {
                let len = usize::from(code) + 1;
                if row.len() + len > width {
                    return None;
                }
                let (literal, rest) = src.split_at_checked(len)?;
                row.extend_from_slice(literal);
                src = rest;
            }
            129..=255 => {
                let len = 257 - usize::from(code);
                if row.len() + len > width {
                    return None;
                }
                let (&value, rest) = src.split_first()?;
                row.resize(row.len() + len, value);
                src = rest;
            }
            128 => {}
        }
    }
    (row.len() == width).then_some(())
}

/// Photoshop's Version Info resource (1057) signals whether the saved preview is real.
fn has_saved_composite(mut resources: &[u8]) -> bool {
    while resources.starts_with(b"8BIM") && resources.len() >= 12 {
        let id = u16::from_be_bytes([resources[4], resources[5]]);
        let name_len = (usize::from(resources[6]) + 1).next_multiple_of(2);
        let start = 6 + name_len;
        let Some(len) = resources.get(start..start + 4) else {
            break;
        };
        let len = u32::from_be_bytes(len.try_into().unwrap()) as usize;
        let Some(data) = resources.get(start + 4..start + 4 + len) else {
            break;
        };
        if id == 1057 && data.len() >= 5 {
            return data[4] != 0;
        }
        let Some(tail) = resources.get(start + 4 + len.next_multiple_of(2)..) else {
            break;
        };
        resources = tail;
    }
    true
}

fn decode_merged(
    header: &PsdHeader,
    data: &[u8],
    transparency: bool,
    white_matte: bool,
    retained: usize,
    report: &mut PsdDecodeReport,
) -> Option<Vec<u8>> {
    let (width, height) = (header.width as usize, header.height as usize);
    let pixels = portable_pixel_count(width, height)?;
    let color_channels = match (header.depth, header.color_mode) {
        (8, 1) => 1,
        (8, 3) => 3,
        _ => return None,
    };
    let channels = usize::from(header.channels);
    if channels < color_channels {
        return None;
    }
    let compression = u16::from_be_bytes(data.get(..2)?.try_into().ok()?);
    let payload = &data[2..];
    let needed = color_channels + usize::from(transparency && channels > color_channels);
    let mut rows = &[][..];
    let count_size = if header.version == 2 { 4 } else { 2 };
    let mut src = payload;
    match compression {
        0 if pixels.checked_mul(channels)? <= src.len() => {}
        1 => {
            let table_len = height.checked_mul(channels)?.checked_mul(count_size)?;
            (rows, src) = src.split_at_checked(table_len)?;
        }
        _ => return None,
    }
    let bytes = pixels.checked_mul(4)?;
    if retained.checked_add(bytes)?.checked_add(width)? > MAX_PSD_DECODE_BYTES {
        report.composite_budget_skipped = true;
        return None;
    }
    let mut rgba = vec![255; bytes];
    let mut scratch = Vec::with_capacity(width);
    for channel in 0..needed {
        for (y, dest) in rgba.chunks_exact_mut(width * 4).enumerate() {
            let row = if compression == 0 {
                let (row, rest) = src.split_at(width);
                src = rest;
                row
            } else {
                let start = (channel * height + y) * count_size;
                let count = &rows[start..start + count_size];
                let len = if count_size == 4 {
                    u32::from_be_bytes(count.try_into().ok()?) as usize
                } else {
                    usize::from(u16::from_be_bytes(count.try_into().ok()?))
                };
                let (encoded, rest) = src.split_at_checked(len)?;
                src = rest;
                unpack_composite_row(encoded, &mut scratch, width)?;
                &scratch
            };
            for (pixel, value) in dest.as_chunks_mut::<4>().0.iter_mut().zip(row) {
                if color_channels == 1 && channel == 0 {
                    pixel[..3].fill(*value);
                } else {
                    pixel[if channel == color_channels {
                        3
                    } else {
                        channel
                    }] = *value;
                }
            }
        }
    }
    if white_matte && needed > color_channels {
        // In Photoshop's merged transparency channel the RGB is composited over
        // white. Remove that matte before storing straight-alpha PNG samples.
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            if (1..255).contains(&alpha) {
                for channel in &mut pixel[..3] {
                    let premul = u32::from(*channel).saturating_sub(255 - alpha);
                    *channel = ((255 * premul + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
    }
    Some(rgba)
}

fn encode_rgba_png(width: u32, height: u32, rgba_bytes: Vec<u8>) -> Result<Vec<u8>, ImportError> {
    let img = image::RgbaImage::from_raw(width, height, rgba_bytes)
        .ok_or_else(|| ImportError::Container("Invalid RGBA buffer dimensions".into()))?;
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .map_err(|e| ImportError::Container(format!("Failed to encode layer PNG: {e}")))?;
    Ok(png_bytes)
}

/// PNG stripes collectively retain full resolution when an encoded composite exceeds the
/// single-resource limit. Pixel and encoded document budgets are still enforced.
fn append_composite(
    project: &mut Project,
    elements: &mut Vec<Element>,
    name: &str,
    width: u32,
    top: u32,
    rgba: &[u8],
) -> Result<(), ImportError> {
    use image::{
        ImageEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let stride = width as usize * 4;
    let height = (rgba.len() / stride) as u32;
    let mut encoded = Vec::new();
    PngEncoder::new_with_quality(&mut encoded, CompressionType::Default, FilterType::Adaptive)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| {
            ImportError::Container(format!("Failed to encode composite PNG: {error}"))
        })?;
    if encoded.len() > neoflash_core::MAX_RESOURCE_BYTES && height > 1 {
        let half = height / 2;
        let (upper, lower) = rgba.split_at(half as usize * stride);
        drop(encoded);
        append_composite(project, elements, name, width, top, upper)?;
        return append_composite(project, elements, name, width, top + half, lower);
    }
    let id = new_id("resource");
    let resource = ResourceUnit::from_bytes(
        id.clone(),
        format!("psd/{id}_composite.png"),
        "image/png",
        &encoded,
    )
    .map_err(|error| ImportError::Container(error.to_string()))?;
    project.resources.push(resource);
    elements.push(Element {
        id: new_id("element"),
        name: format!("{name} Composite"),
        transform: Transform {
            y: f64::from(top),
            ..Default::default()
        },
        kind: ElementKind::Bitmap {
            resource_id: id,
            width,
            height,
            smoothing: true,
        },
        ..Default::default()
    });
    Ok(())
}

/// Parse a binary `.psd` / `.psb` file into its header, layer records, and optional merged composite RGBA8 buffer.
pub fn parse_psd_document(bytes: &[u8]) -> Result<ParsedPsdDocument, ImportError> {
    let (header, layers, composite, _) = parse_psd_document_with_plan(bytes, PsdDecodePlan::All)?;
    Ok((header, layers, composite))
}

fn parse_psd_document_with_plan(
    bytes: &[u8],
    decode_plan: PsdDecodePlan,
) -> Result<ParsedPsdDocumentWithReport, ImportError> {
    if bytes.len() < 26 || &bytes[0..4] != b"8BPS" {
        return Err(ImportError::Container(
            "Invalid PSD signature (expected '8BPS')".into(),
        ));
    }
    let mut pos = 4usize;
    let version = read_u16_be(bytes, &mut pos)?;
    if version != 1 && version != 2 {
        return Err(ImportError::Container(format!(
            "Unsupported PSD version {version} (expected 1 or 2)"
        )));
    }
    pos += 6; // reserved
    let channels = read_u16_be(bytes, &mut pos)?;
    let height = read_u32_be(bytes, &mut pos)?;
    let width = read_u32_be(bytes, &mut pos)?;
    let depth = read_u16_be(bytes, &mut pos)?;
    let color_mode = read_u16_be(bytes, &mut pos)?;

    if width == 0 || height == 0 || width > 300_000 || height > 300_000 {
        return Err(ImportError::Container(format!(
            "Invalid PSD dimensions {width}x{height}"
        )));
    }

    let header = PsdHeader {
        version,
        channels,
        height,
        width,
        depth,
        color_mode,
    };

    // 1. Color Mode Data
    let color_mode_len = read_u32_be(bytes, &mut pos)? as usize;
    pos = checked_section_end(pos, color_mode_len, bytes.len(), "Color Mode Data")?;

    // 2. Image Resources
    let resources_len = read_u32_be(bytes, &mut pos)? as usize;
    let resources_end = checked_section_end(pos, resources_len, bytes.len(), "Image Resources")?;
    let has_composite = has_saved_composite(&bytes[pos..resources_end]);
    let doc_global_angle = extract_psd_global_lighting_angle(&bytes[pos..resources_end]);
    pos = resources_end;

    // 3. Layer and Mask Information
    let layer_mask_len = if version == 2 {
        read_u64_be(bytes, &mut pos)? as usize
    } else {
        read_u32_be(bytes, &mut pos)? as usize
    };
    let layer_mask_end = checked_section_end(pos, layer_mask_len, bytes.len(), "Layer and Mask")?;

    let mut layers = Vec::new();
    let mut merged_transparency = false;
    let mut has_visible_effects = false;
    let mut decode_report = PsdDecodeReport::default();
    let mut retained_decoded_bytes = 0usize;
    if layer_mask_len > 0 {
        let layer_info_length_bytes = if version == 2 { 8 } else { 4 };
        checked_section_end(
            pos,
            layer_info_length_bytes,
            layer_mask_end,
            "Layer Information length",
        )?;
        let layer_info_len = if version == 2 {
            read_u64_be(bytes, &mut pos)? as usize
        } else {
            read_u32_be(bytes, &mut pos)? as usize
        };
        let layer_info_end =
            checked_section_end(pos, layer_info_len, layer_mask_end, "Layer Information")?;
        if layer_info_len > 0 && pos.saturating_add(2) <= layer_info_end {
            let raw_layer_count = read_i16_be(bytes, &mut pos)?;
            merged_transparency = raw_layer_count < 0;
            let layer_count = raw_layer_count.unsigned_abs() as usize;

            for layer_idx in 0..layer_count {
                checked_section_end(pos, 18, layer_info_end, "layer record")?;
                let top = read_i32_be(bytes, &mut pos)?;
                let left = read_i32_be(bytes, &mut pos)?;
                let bottom = read_i32_be(bytes, &mut pos)?;
                let right = read_i32_be(bytes, &mut pos)?;
                let num_channels = read_u16_be(bytes, &mut pos)? as usize;
                let channel_metadata_bytes = num_channels
                    .checked_mul(if version == 2 { 10 } else { 6 })
                    .ok_or_else(|| {
                        ImportError::Container("PSD layer channel count overflow".into())
                    })?;
                checked_section_end(
                    pos,
                    channel_metadata_bytes,
                    layer_info_end,
                    "layer channel metadata",
                )?;

                let mut channels_meta = Vec::with_capacity(num_channels);
                for _ in 0..num_channels {
                    let ch_id = read_i16_be(bytes, &mut pos)?;
                    let ch_len = if version == 2 {
                        read_u64_be(bytes, &mut pos)? as usize
                    } else {
                        read_u32_be(bytes, &mut pos)? as usize
                    };
                    channels_meta.push((ch_id, ch_len));
                }

                checked_section_end(pos, 16, layer_info_end, "layer blend record")?;
                let blend_sig = &bytes[pos..pos + 4];
                if blend_sig != b"8BIM" {
                    return Err(ImportError::Container(format!(
                        "Invalid PSD layer blend signature at layer {layer_idx}"
                    )));
                }
                pos += 4;
                let blend_key = String::from_utf8_lossy(&bytes[pos..pos + 4]).into_owned();
                pos += 4;
                let opacity = read_u8(bytes, &mut pos)?;
                let clipping = read_u8(bytes, &mut pos)? != 0;
                let flags = read_u8(bytes, &mut pos)?;
                let _filler = read_u8(bytes, &mut pos)?;
                let visible = (flags & 0x02) == 0;

                let extra_len = read_u32_be(bytes, &mut pos)? as usize;
                let extra_end =
                    checked_section_end(pos, extra_len, layer_info_end, "layer extra data")?;

                // Layer mask data
                checked_section_end(pos, 4, extra_end, "layer mask data length")?;
                let mask_data_len = read_u32_be(bytes, &mut pos)? as usize;
                let has_layer_mask = mask_data_len > 0;
                pos = checked_section_end(pos, mask_data_len, extra_end, "layer mask data")?;

                // Blending ranges
                if pos.saturating_add(4) <= extra_end {
                    let ranges_len = read_u32_be(bytes, &mut pos)? as usize;
                    pos = checked_section_end(pos, ranges_len, extra_end, "layer blending ranges")?;
                }

                // Pascal string layer name (padded to 4-byte boundary)
                let mut name = format!("Layer {}", layer_idx + 1);
                if pos < extra_end {
                    let name_len = read_u8(bytes, &mut pos)? as usize;
                    let avail = extra_end.saturating_sub(pos).min(name_len);
                    let parsed_name =
                        String::from_utf8_lossy(&bytes[pos..pos + avail]).into_owned();
                    if !parsed_name.is_empty() {
                        name = parsed_name;
                    }
                    let total_pascal = 1 + name_len;
                    let padded = (total_pascal + 3) & !3;
                    pos = checked_section_end(pos, padded - 1, extra_end, "layer Pascal name")?;
                }

                let mut section_divider = PsdSectionDivider::Normal;
                let mut text_data = None;
                let mut has_vector_mask = false;
                let mut is_adjustment_layer = None;
                let mut is_smart_object = false;
                let mut solid_fill_color: Option<Color> = None;
                let mut raw_lmfx: Option<&[u8]> = None;
                let mut raw_lfx2: Option<&[u8]> = None;
                let mut raw_lrfx: Option<&[u8]> = None;

                // Additional Layer Information tagged blocks
                while pos.saturating_add(12) <= extra_end {
                    let sig = &bytes[pos..pos + 4];
                    if sig != b"8BIM" && sig != b"8B64" {
                        break;
                    }
                    pos += 4;
                    let mut key_bytes = [0u8; 4];
                    key_bytes.copy_from_slice(&bytes[pos..pos + 4]);
                    pos += 4;
                    let uses_u64_len = version == 2
                        && matches!(
                            &key_bytes,
                            b"LMsk"
                                | b"Lr16"
                                | b"Lr32"
                                | b"Layr"
                                | b"Mt16"
                                | b"Mt32"
                                | b"Mtrn"
                                | b"Alph"
                                | b"FMsk"
                                | b"lnk2"
                                | b"FEid"
                                | b"FXid"
                                | b"PxSD"
                        );
                    let block_len = if uses_u64_len {
                        checked_section_end(pos, 8, extra_end, "PSB 64-bit block length")?;
                        usize::try_from(read_u64_be(bytes, &mut pos)?).map_err(|_| {
                            ImportError::Container("PSB block length overflows usize".into())
                        })?
                    } else {
                        read_u32_be(bytes, &mut pos)? as usize
                    };
                    let block_end = checked_section_end(
                        pos,
                        block_len,
                        extra_end,
                        "additional layer information",
                    )?;
                    let block = &bytes[pos..block_end];

                    match &key_bytes {
                        b"luni" => {
                            if block.len() >= 4 {
                                let char_count =
                                    u32::from_be_bytes([block[0], block[1], block[2], block[3]])
                                        as usize;
                                if let Some(byte_len) = char_count.checked_mul(2)
                                    && let Some(end) = 4usize.checked_add(byte_len)
                                    && end <= block.len()
                                {
                                    let units: Vec<u16> = block[4..end]
                                        .as_chunks::<2>()
                                        .0
                                        .iter()
                                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                                        .filter(|&u| u != 0)
                                        .collect();
                                    let uni_name = String::from_utf16_lossy(&units);
                                    if !uni_name.is_empty() {
                                        name = uni_name;
                                    }
                                }
                            }
                        }
                        b"lsct" | b"lsdk" => {
                            if block.len() >= 4 {
                                let div_type =
                                    u32::from_be_bytes([block[0], block[1], block[2], block[3]]);
                                section_divider = match div_type {
                                    1 => PsdSectionDivider::OpenFolder,
                                    2 => PsdSectionDivider::ClosedFolder,
                                    3 => PsdSectionDivider::BoundingSectionEnd,
                                    _ => PsdSectionDivider::Normal,
                                };
                            }
                        }
                        b"TySh" => {
                            text_data = parse_tysh_block(block).or_else(|| {
                                Some(PsdTextLayerData {
                                    text: name.clone(),
                                    font_family: "Arial".to_owned(),
                                    font_size: 24.0,
                                    color: [255, 255, 255, 255],
                                    transform: [1.0, 0.0, 0.0, 1.0, left as f64, top as f64],
                                })
                            });
                        }
                        b"lmfx" => raw_lmfx = Some(block),
                        b"lfx2" => raw_lfx2 = Some(block),
                        b"lrFX" => raw_lrfx = Some(block),
                        b"SoCo" => {
                            if block.len() >= 4
                                && u32::from_be_bytes([block[0], block[1], block[2], block[3]])
                                    == 16
                            {
                                let mut dpos = 4usize;
                                if let Some(desc) = parse_descriptor_body(block, &mut dpos, 0)
                                    && let Some(clr) = desc.object("Clr ")
                                {
                                    let rgb = descriptor_color_to_rgb24(clr, 0x00_0000);
                                    solid_fill_color = Some([
                                        ((rgb >> 16) & 0xff) as u8,
                                        ((rgb >> 8) & 0xff) as u8,
                                        (rgb & 0xff) as u8,
                                        255,
                                    ]);
                                }
                            }
                        }
                        b"vmsk" | b"vsms" => {
                            has_vector_mask = true;
                        }
                        b"SoLd" | b"PlLd" => {
                            is_smart_object = true;
                        }
                        other => {
                            if let Some(adj) = is_adjustment_key(other) {
                                is_adjustment_layer = Some(adj.to_owned());
                            }
                        }
                    }

                    // Tagged blocks are padded to even (or 4-byte) boundary
                    let padded_len = (block_len + 1) & !1;
                    pos = checked_section_end(
                        pos,
                        padded_len,
                        extra_end,
                        "additional layer information padding",
                    )?;
                    if pos.saturating_add(6) <= extra_end
                        && bytes[pos..pos + 2] == [0, 0]
                        && (&bytes[pos + 2..pos + 6] == b"8BIM"
                            || &bytes[pos + 2..pos + 6] == b"8B64")
                    {
                        pos += 2;
                    }
                }

                let parsed_effects = if let Some(desc) =
                    raw_lmfx.and_then(parse_versioned_effects_descriptor)
                {
                    convert_lfx2_descriptor(&desc, doc_global_angle)
                } else if let Some(desc) = raw_lfx2.and_then(parse_versioned_effects_descriptor) {
                    convert_lfx2_descriptor(&desc, doc_global_angle)
                } else if let Some(legacy) =
                    raw_lrfx.and_then(|b| parse_legacy_lrfx_block(b, doc_global_angle))
                {
                    legacy
                } else if raw_lmfx.is_some() || raw_lfx2.is_some() || raw_lrfx.is_some() {
                    PsdParsedLayerEffects {
                        has_unreconstructable_effect: true,
                        approximations: vec![
                            "PSD layer effects block (lfx2/lrFX) was malformed or used an unsupported descriptor version and could not be reconstructed.".to_owned(),
                        ],
                        ..PsdParsedLayerEffects::default()
                    }
                } else {
                    PsdParsedLayerEffects::default()
                };
                has_visible_effects |= visible && parsed_effects.has_unreconstructable_effect;

                pos = extra_end;
                layers.push(PsdLayerRecord {
                    name,
                    top,
                    left,
                    bottom,
                    right,
                    blend_key,
                    opacity,
                    clipping,
                    visible,
                    section_divider,
                    text_data,
                    has_vector_mask,
                    has_layer_mask,
                    is_adjustment_layer,
                    is_smart_object,
                    filters: parsed_effects.filters,
                    color_overlay_tint: parsed_effects.color_overlay_tint,
                    folder_color_overlay_filter: parsed_effects.folder_color_overlay_filter,
                    effects_approximations: parsed_effects.approximations,
                    solid_fill_color,
                    channels_meta,
                    rgba_pixels: None,
                });
            }

            // Channel Image Data for each layer. Retained RGBA plus all
            // temporary channel planes for the current layer must fit the
            // aggregate decode budget; metadata and stream position are kept
            // even when pixels are represented by an explicit placeholder.
            if decode_plan == PsdDecodePlan::Automatic && has_composite {
                let mut retained = 0usize;
                let loses_visible_artwork = layers
                    .iter()
                    .filter(|layer| layer.section_divider == PsdSectionDivider::Normal)
                    .any(|layer| {
                        let pixels =
                            portable_pixel_count(layer.width() as usize, layer.height() as usize);
                        let over_budget = pixels.is_some_and(|n| {
                            retained.saturating_add(n.saturating_mul(8)) > MAX_PSD_DECODE_BYTES
                        });
                        if !over_budget && let Some(n) = pixels {
                            retained = retained.saturating_add(n.saturating_mul(4));
                        }
                        layer.visible
                            && (layer.is_adjustment_layer.is_some()
                                || layer.has_vector_mask
                                || layer.has_layer_mask
                                || layer.clipping
                                || !map_psd_blend_mode(&layer.blend_key).1
                                || over_budget
                                || (pixels.is_none() && layer.width() > 0 && layer.height() > 0))
                    });
                if (has_visible_effects || loses_visible_artwork)
                    && let Some(merged) = decode_merged(
                        &header,
                        &bytes[layer_mask_end..],
                        merged_transparency,
                        merged_transparency,
                        0,
                        &mut decode_report,
                    )
                {
                    decode_report.used_merged_appearance = true;
                    return Ok((header, layers, Some(merged), decode_report));
                }
            }
            let decode_layer_pixels = decode_plan != PsdDecodePlan::CompositeOnly;
            for (layer_index, layer) in layers.iter_mut().enumerate() {
                let lw = layer.width() as usize;
                let lh = layer.height() as usize;
                let pixel_count = portable_pixel_count(lw, lh);
                let relevant_channels: BTreeSet<i16> = layer
                    .channels_meta
                    .iter()
                    .map(|(channel, _)| *channel)
                    .filter(|channel| (-1..=2).contains(channel))
                    .collect();
                let transient_bytes = pixel_count
                    .and_then(|pixels| {
                        pixels.checked_mul(4usize.saturating_add(relevant_channels.len()))
                    })
                    .and_then(|bytes| retained_decoded_bytes.checked_add(bytes));
                let within_budget =
                    transient_bytes.is_some_and(|total| total <= MAX_PSD_DECODE_BYTES);
                let should_decode = decode_layer_pixels
                    && pixel_count.is_some()
                    && !relevant_channels.is_empty()
                    && within_budget;
                if decode_layer_pixels
                    && pixel_count.is_some()
                    && !relevant_channels.is_empty()
                    && !within_budget
                {
                    decode_report.budget_skipped_layers.insert(layer_index);
                }
                let mut decoded_channels = BTreeMap::new();
                for &(ch_id, ch_len) in &layer.channels_meta {
                    let end = checked_section_end(
                        pos,
                        ch_len,
                        layer_info_end,
                        "layer channel image data",
                    )?;
                    let slice = &bytes[pos..end];
                    pos = end;
                    if should_decode
                        && relevant_channels.contains(&ch_id)
                        && let Ok(plane) = decode_channel_slice(slice, lw, lh, version)
                    {
                        decoded_channels.insert(ch_id, plane);
                    }
                }
                if should_decode && !decoded_channels.is_empty() {
                    let rgba = combine_channels_to_rgba(lw, lh, color_mode, &decoded_channels);
                    retained_decoded_bytes = retained_decoded_bytes.saturating_add(rgba.len());
                    layer.rgba_pixels = Some(rgba);
                }
            }
        }
    }

    let composite_rgba = if has_composite
        && (matches!(
            decode_plan,
            PsdDecodePlan::All | PsdDecodePlan::CompositeOnly
        ) || layers.is_empty())
    {
        // Layerless RGBA exports use the first extra plane for transparency. Layered
        // PSDs explicitly signal merged transparency with a negative layer count.
        decode_merged(
            &header,
            &bytes[layer_mask_end..],
            merged_transparency || layers.is_empty(),
            merged_transparency,
            retained_decoded_bytes,
            &mut decode_report,
        )
    } else {
        None
    };
    Ok((header, layers, composite_rgba, decode_report))
}

fn outline_paths_for_text(text: &str, font_size: f64, color: Color) -> ElementKind {
    let char_w = (font_size * 0.55).max(4.0);
    let char_h = (font_size * 0.8).max(6.0);
    let mut commands = Vec::new();
    let mut cursor_x = 0.0;
    for ch in text.chars() {
        if ch.is_whitespace() {
            cursor_x += char_w * 0.6;
            continue;
        }
        commands.push(PathCommand::MoveTo {
            x: cursor_x,
            y: 0.0,
        });
        commands.push(PathCommand::LineTo {
            x: cursor_x + char_w * 0.82,
            y: 0.0,
        });
        commands.push(PathCommand::LineTo {
            x: cursor_x + char_w * 0.82,
            y: char_h,
        });
        commands.push(PathCommand::LineTo {
            x: cursor_x,
            y: char_h,
        });
        commands.push(PathCommand::Close);
        cursor_x += char_w;
    }
    if commands.is_empty() {
        commands.push(PathCommand::MoveTo { x: 0.0, y: 0.0 });
        commands.push(PathCommand::LineTo { x: char_w, y: 0.0 });
        commands.push(PathCommand::LineTo {
            x: char_w,
            y: char_h,
        });
        commands.push(PathCommand::Close);
    }
    ElementKind::Path {
        commands,
        fill_rule: FillRule::NonZero,
        fill: color,
        stroke: [0, 0, 0, 0],
        stroke_width: 0.0,
    }
}

/// Import a `.psd` / `.psb` binary into an existing [`Project`] with explicit [`PsdImportOptions`].
///
/// Emits location-aware [`crate::Diagnostic`] entries in [`ImportReport`]:
/// - `psd-unsupported-blend-mode`
/// - `psd-layer-effect-approximated`
/// - `psd-adjustment-layer-approximated`
/// - `psd-vector-mask-approximated`
/// - `psd-smart-object-rasterized`
/// - `psd-text-outlines-approximated`
/// - `psd-composite-bitmap-too-large` / `psd-layer-bitmap-too-large`
/// - `psd-composite-pixels-unavailable` / `psd-layer-pixels-unavailable`
/// - `psd-composite-decode-budget-exceeded` / `psd-layer-decode-budget-exceeded`
pub fn import_psd_into_project(
    project: &mut Project,
    bytes: &[u8],
    source_name: &str,
    options: &PsdImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut candidate = project.clone();
    let report = import_psd_into_project_inner(&mut candidate, bytes, source_name, options)?;
    candidate.validate().map_err(|error| {
        ImportError::Container(format!("PSD import produced an invalid project: {error}"))
    })?;
    *project = candidate;
    Ok(report)
}

fn import_psd_into_project_inner(
    project: &mut Project,
    bytes: &[u8],
    source_name: &str,
    options: &PsdImportOptions,
) -> Result<ImportReport, ImportError> {
    let decode_plan = match options.layer_mapping {
        PsdLayerMapping::FlattenComposite => PsdDecodePlan::CompositeOnly,
        PsdLayerMapping::Automatic => PsdDecodePlan::Automatic,
        _ => PsdDecodePlan::LayersWithCompositeFallback,
    };
    let (header, mut psd_layers, composite_rgba, decode_report) =
        parse_psd_document_with_plan(bytes, decode_plan)?;
    let mut report = ImportReport {
        source: source_name.to_owned(),
        format: ImportFormat::Unknown,
        version: Some(if header.version == 2 {
            "PSB v2".to_owned()
        } else {
            "PSD v1".to_owned()
        }),
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };

    if options.resize_stage_to_canvas {
        project.width = header.width.clamp(1, 65535);
        project.height = header.height.clamp(1, 65535);
    }

    let mapping = if options.layer_mapping == PsdLayerMapping::Automatic {
        if decode_report.used_merged_appearance {
            report.warning("psd-merged-appearance", source_name, format!(
                "Used the saved Photoshop composite for the {} layer records because masks, effects, adjustments, blend modes or layer pixel limits prevent a faithful editable reconstruction.", psd_layers.len()
            ));
            PsdLayerMapping::FlattenComposite
        } else {
            PsdLayerMapping::TimelineLayers
        }
    } else {
        options.layer_mapping
    };
    // Handle FlattenComposite or 0-layer PSDs
    if mapping == PsdLayerMapping::FlattenComposite || psd_layers.is_empty() {
        let mut elements = Vec::new();
        if let Some(rgba) = composite_rgba {
            if options.resize_stage_to_canvas
                && rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] < 255)
            {
                project.background[3] = 0;
            }
            append_composite(project, &mut elements, source_name, header.width, 0, &rgba)?;
        } else {
            let (code, reason) = if portable_pixel_count(
                header.width as usize,
                header.height as usize,
            )
            .is_none()
            {
                (
                    "psd-composite-bitmap-too-large",
                    format!(
                        "PSD composite {}x{} exceeds the portable {}x{} bitmap limit.",
                        header.width, header.height, MAX_BITMAP_DIMENSION, MAX_BITMAP_DIMENSION
                    ),
                )
            } else if decode_report.composite_budget_skipped {
                (
                    "psd-composite-decode-budget-exceeded",
                    format!(
                        "PSD composite exceeded the {} MiB decode budget.",
                        MAX_PSD_DECODE_BYTES / (1024 * 1024)
                    ),
                )
            } else {
                ("psd-composite-pixels-unavailable", "The saved composite is missing, unreadable or outside the supported 8-bit RGB/grayscale raw or RLE formats.".to_owned())
            };
            report.warning(
                code,
                source_name,
                format!("{reason} Inserted an explicit vector placeholder."),
            );
            elements.push(Element {
                id: new_id("element"),
                name: format!("{source_name} Composite placeholder"),
                kind: ElementKind::Rectangle {
                    width: f64::from(header.width),
                    height: f64::from(header.height),
                    fill: [180, 180, 180, 255],
                    stroke: [255; 4],
                    stroke_width: 1.0,
                },
                ..Default::default()
            });
        }
        report.statistics.elements = elements.len();
        let layer = Layer {
            id: new_id("layer"),
            name: "PSD Composite".to_owned(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements,
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        };
        let root_id = project.root.clone();
        if let Some(root) = project.symbol_mut(&root_id) {
            root.timeline.layers = vec![layer];
        }
        report.statistics.layers = 1;
        report.statistics.keyframes = 1;
        project.format_version = project.format_version.max(3);
        project.upgrade_format_for_features();
        report.can_import = true;
        return Ok(report);
    }

    // PSD and NeoFlash both store bottom-to-top. Walk upwards to identify clipping bases,
    // then walk downwards to attach children to folders before restoring the paint order.
    let layer_ids: Vec<String> = (0..psd_layers.len()).map(|_| new_id("layer")).collect();
    let mut clipping_base_for: BTreeMap<usize, usize> = BTreeMap::new();
    let mut is_clipping_base: std::collections::HashSet<usize> = std::collections::HashSet::new();

    if options.preserve_clipping_masks
        && matches!(
            mapping,
            PsdLayerMapping::TimelineLayers | PsdLayerMapping::MovieClipSymbols
        )
    {
        let mut current_base: Option<usize> = None;
        for (i, rec) in psd_layers.iter().enumerate() {
            if rec.section_divider != PsdSectionDivider::Normal {
                current_base = None;
                continue;
            }
            if !options.import_hidden_layers && !rec.visible {
                continue;
            }
            if !rec.clipping {
                current_base = Some(i);
            } else if let Some(base_idx) = current_base {
                clipping_base_for.insert(i, base_idx);
                is_clipping_base.insert(base_idx);
            }
        }
    }

    let mut built_layers = Vec::new();
    let mut keyframe_mode_frames = Vec::new();
    let mut folder_stack: Vec<String> = Vec::new();

    // Walk top-to-bottom (reverse of PSD bottom-to-top array)
    for (i, rec) in psd_layers.iter_mut().enumerate().rev() {
        let loc = format!("{source_name}::{}", rec.name);

        if rec.section_divider == PsdSectionDivider::BoundingSectionEnd {
            folder_stack.pop();
            continue;
        }

        if !options.import_hidden_layers && !rec.visible {
            continue;
        }

        for approx_msg in &rec.effects_approximations {
            report.warning(
                "psd-layer-effect-approximated",
                loc.clone(),
                approx_msg.clone(),
            );
        }

        if matches!(
            rec.section_divider,
            PsdSectionDivider::OpenFolder | PsdSectionDivider::ClosedFolder
        ) {
            let folder_id = layer_ids[i].clone();
            let parent_id = folder_stack.last().cloned();
            if matches!(
                mapping,
                PsdLayerMapping::TimelineLayers | PsdLayerMapping::MovieClipSymbols
            ) {
                let (folder_blend, exact_folder_blend) = if options.preserve_blend_modes {
                    map_psd_blend_mode(&rec.blend_key)
                } else {
                    (BlendMode::Normal, true)
                };
                if options.preserve_blend_modes && !exact_folder_blend {
                    report.warning(
                        "psd-unsupported-blend-mode",
                        loc.clone(),
                        format!(
                            "PSD folder blend mode '{}' is not natively supported; approximated as '{}'.",
                            rec.blend_key,
                            folder_blend.label()
                        ),
                    );
                }
                let mut folder_filters = rec.filters.clone();
                if let Some(cm) = rec.folder_color_overlay_filter.clone()
                    && cm.validate().is_ok()
                {
                    if folder_filters.len() < MAX_OBJECT_FILTERS {
                        folder_filters.push(cm);
                    } else {
                        report.warning(
                            "psd-layer-effect-approximated",
                            loc.clone(),
                            format!(
                                "PSD group Color Overlay ('SoFi') omitted because the folder already carries {MAX_OBJECT_FILTERS} active filters."
                            ),
                        );
                    }
                }
                built_layers.push(Layer {
                    id: folder_id.clone(),
                    name: rec.name.clone(),
                    visible: rec.visible,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: Vec::new(),
                        ..Keyframe::default()
                    }],
                    properties: LayerProperties {
                        role: LayerRole::Folder,
                        parent: parent_id,
                        collapsed: rec.section_divider == PsdSectionDivider::ClosedFolder,
                        blend_mode: folder_blend,
                        filters: folder_filters,
                        ..LayerProperties::default()
                    },
                });
                report.statistics.layers += 1;
                report.statistics.keyframes += 1;
                folder_stack.push(folder_id);
            }
            continue;
        }

        // Emit fidelity diagnostics for special PSD layer features
        if let Some(adj_kind) = &rec.is_adjustment_layer {
            report.warning(
                "psd-adjustment-layer-approximated",
                loc.clone(),
                format!(
                    "PSD adjustment layer '{adj_kind}' has no direct runtime equivalent and was approximated."
                ),
            );
        }
        if rec.has_vector_mask {
            report.warning(
                "psd-vector-mask-approximated",
                loc.clone(),
                "PSD vector mask (vmsk/vsms) was approximated into the layer's alpha channel.",
            );
        }
        if rec.is_smart_object {
            report.warning(
                "psd-smart-object-rasterized",
                loc.clone(),
                "PSD Smart Object layer was imported using its rasterized composite pixels.",
            );
        }

        let (mapped_blend, exact_blend) = if options.preserve_blend_modes {
            map_psd_blend_mode(&rec.blend_key)
        } else {
            (BlendMode::Normal, true)
        };
        if options.preserve_blend_modes && !exact_blend {
            report.warning(
                "psd-unsupported-blend-mode",
                loc.clone(),
                format!(
                    "PSD blend mode '{}' is not natively supported; approximated as '{}'.",
                    rec.blend_key,
                    mapped_blend.label()
                ),
            );
        }

        let alpha = (rec.opacity as f64 / 255.0).clamp(0.0, 1.0);

        // Build the visual Element for this layer
        let element = if let Some(text_info) = &rec.text_data
            && options.text_handling != PsdTextHandling::RasterizeLayer
        {
            let tx = if text_info.transform[4] != 0.0 {
                text_info.transform[4]
            } else {
                rec.left as f64
            };
            let ty = if text_info.transform[5] != 0.0 {
                text_info.transform[5]
            } else {
                rec.top as f64
            };
            let kind = match options.text_handling {
                PsdTextHandling::EditableText => ElementKind::Text {
                    text: text_info.text.clone(),
                    font_family: text_info.font_family.clone(),
                    font_size: text_info.font_size,
                    color: text_info.color,
                },
                PsdTextHandling::VectorOutlines => {
                    report.warning(
                        "psd-text-outlines-approximated",
                        loc.clone(),
                        "PSD TySh text layer was converted to editable vector outline paths.",
                    );
                    outline_paths_for_text(&text_info.text, text_info.font_size, text_info.color)
                }
                PsdTextHandling::RasterizeLayer => unreachable!(),
            };
            Element {
                id: new_id("element"),
                name: rec.name.clone(),
                transform: Transform {
                    x: tx,
                    y: ty,
                    alpha,
                    ..Transform::default()
                },
                blend_mode: mapped_blend,
                kind,
                ..Element::default()
            }
        } else if portable_pixel_count(rec.width() as usize, rec.height() as usize).is_some() {
            let lw = rec.width().max(1);
            let lh = rec.height().max(1);
            if let Some(rgba) = rec.rgba_pixels.take() {
                let png = encode_rgba_png(lw, lh, rgba)?;
                let res_id = new_id("resource");
                let safe_stem: String = rec
                    .name
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                let res_path = format!("psd/{}_{}.png", safe_stem, res_id);
                let unit = ResourceUnit::from_bytes(res_id.clone(), res_path, "image/png", &png)
                    .map_err(|e| ImportError::Container(e.to_string()))?;
                project.resources.push(unit);

                Element {
                    id: new_id("element"),
                    name: rec.name.clone(),
                    transform: Transform {
                        x: rec.left as f64,
                        y: rec.top as f64,
                        alpha,
                        ..Transform::default()
                    },
                    blend_mode: mapped_blend,
                    kind: ElementKind::Bitmap {
                        resource_id: res_id,
                        width: lw,
                        height: lh,
                        smoothing: true,
                    },
                    ..Element::default()
                }
            } else if let Some(fill_col) = rec.solid_fill_color {
                Element {
                    id: new_id("element"),
                    name: rec.name.clone(),
                    transform: Transform {
                        x: rec.left as f64,
                        y: rec.top as f64,
                        alpha,
                        ..Transform::default()
                    },
                    blend_mode: mapped_blend,
                    kind: ElementKind::Rectangle {
                        width: f64::from(lw),
                        height: f64::from(lh),
                        fill: fill_col,
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }
            } else {
                if decode_report.budget_skipped_layers.contains(&i) {
                    report.warning(
                            "psd-layer-decode-budget-exceeded",
                            loc.clone(),
                            format!(
                                "PSD layer pixels exceeded the {} MiB aggregate decode budget; retained the layer with an explicit vector placeholder.",
                                MAX_PSD_DECODE_BYTES / (1024 * 1024)
                            ),
                        );
                } else {
                    report.warning(
                            "psd-layer-pixels-unavailable",
                            loc.clone(),
                            "PSD layer pixels were missing or unreadable; inserted an explicit vector placeholder.",
                        );
                }
                Element {
                    id: new_id("element"),
                    name: format!("{} placeholder", rec.name),
                    transform: Transform {
                        x: rec.left as f64,
                        y: rec.top as f64,
                        alpha,
                        ..Transform::default()
                    },
                    blend_mode: mapped_blend,
                    kind: ElementKind::Rectangle {
                        width: f64::from(lw),
                        height: f64::from(lh),
                        fill: [180, 180, 180, 255],
                        stroke: [255, 255, 255, 255],
                        stroke_width: 1.0,
                    },
                    ..Element::default()
                }
            }
        } else {
            report.warning(
                "psd-layer-bitmap-too-large",
                loc.clone(),
                format!(
                    "PSD layer '{}' is {}x{}, exceeding the portable {}x{} bitmap limit; inserted an explicit placeholder instead of allocating an unsafe image.",
                    rec.name,
                    rec.width(),
                    rec.height(),
                    MAX_BITMAP_DIMENSION,
                    MAX_BITMAP_DIMENSION
                ),
            );
            Element {
                id: new_id("element"),
                name: format!("{} placeholder", rec.name),
                transform: Transform {
                    x: rec.left as f64,
                    y: rec.top as f64,
                    alpha,
                    ..Transform::default()
                },
                blend_mode: mapped_blend,
                kind: ElementKind::Rectangle {
                    width: (rec.width().max(1) as f64).min(1e9),
                    height: (rec.height().max(1) as f64).min(1e9),
                    fill: [180, 180, 180, 255],
                    stroke: [255, 255, 255, 255],
                    stroke_width: 1.0,
                },
                ..Element::default()
            }
        };

        let mut element = element;
        let layer_filters = rec.filters.clone();
        let layer_tint = rec.color_overlay_tint;

        report.statistics.elements += 1;

        match mapping {
            PsdLayerMapping::Keyframes => {
                element.filters = layer_filters;
                if let Some(tint) = layer_tint {
                    element.transform.color_transform = tint;
                }
                keyframe_mode_frames.push((rec.name.clone(), element));
            }
            PsdLayerMapping::MovieClipSymbols | PsdLayerMapping::TimelineLayers => {
                let is_mask_base = is_clipping_base.contains(&i);
                let placed_element =
                    if mapping == PsdLayerMapping::MovieClipSymbols && !is_mask_base {
                        let sym_id = new_id("symbol");
                        let sym_pos = [element.transform.x, element.transform.y];
                        let mut local_elem = element;
                        local_elem.transform.x = 0.0;
                        local_elem.transform.y = 0.0;
                        let sym = Symbol {
                            id: sym_id.clone(),
                            name: rec.name.clone(),
                            folder: String::new(),
                            kind: SymbolKind::MovieClip,
                            timeline: Timeline {
                                frame_count: 1,
                                layers: vec![Layer {
                                    id: new_id("layer"),
                                    name: rec.name.clone(),
                                    visible: true,
                                    locked: false,
                                    keyframes: vec![Keyframe {
                                        frame: 0,
                                        elements: vec![local_elem],
                                        ..Keyframe::default()
                                    }],
                                    properties: LayerProperties::default(),
                                }],
                                scripts: Vec::new(),
                                guides: Vec::new(),
                            },
                            class_name: None,
                            linkage_id: None,
                        };
                        project.symbols.push(sym);
                        report.statistics.symbols += 1;
                        report.statistics.layers += 1;
                        report.statistics.keyframes += 1;
                        report.statistics.elements += 1;

                        Element {
                            id: new_id("element"),
                            name: rec.name.clone(),
                            transform: Transform {
                                x: sym_pos[0],
                                y: sym_pos[1],
                                color_transform: layer_tint.unwrap_or_default(),
                                ..Transform::default()
                            },
                            blend_mode: mapped_blend,
                            filters: layer_filters,
                            kind: ElementKind::Instance {
                                symbol_id: sym_id,
                                playback: InstancePlayback::Independent,
                            },
                            ..Element::default()
                        }
                    } else {
                        element.filters = layer_filters;
                        if let Some(tint) = layer_tint {
                            element.transform.color_transform = tint;
                        }
                        element
                    };

                let mut props = LayerProperties {
                    parent: folder_stack.last().cloned(),
                    blend_mode: mapped_blend,
                    ..LayerProperties::default()
                };
                if is_mask_base {
                    props.role = LayerRole::Mask;
                    props.mask_mode = MaskMode::Alpha;
                } else if let Some(&base_idx) = clipping_base_for.get(&i) {
                    props.role = LayerRole::Normal;
                    props.mask = Some(layer_ids[base_idx].clone());
                }

                built_layers.push(Layer {
                    id: layer_ids[i].clone(),
                    name: rec.name.clone(),
                    visible: rec.visible,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![placed_element],
                        ..Keyframe::default()
                    }],
                    properties: props,
                });
                report.statistics.layers += 1;
                report.statistics.keyframes += 1;
            }
            PsdLayerMapping::FlattenComposite | PsdLayerMapping::Automatic => unreachable!(),
        }
    }

    let root_id = project.root.clone();
    if mapping == PsdLayerMapping::Keyframes {
        // Keyframes are ordered bottom-to-top in PSD (so Layer 1 is frame 0, Layer 2 is frame 1...)
        keyframe_mode_frames.reverse();
        let mut keyframes = Vec::with_capacity(keyframe_mode_frames.len());
        for (f_idx, (label, elem)) in keyframe_mode_frames.into_iter().enumerate() {
            keyframes.push(Keyframe {
                frame: f_idx as u32,
                label,
                elements: vec![elem],
                ..Keyframe::default()
            });
            report.statistics.keyframes += 1;
        }
        let frame_count = (keyframes.len() as u32).max(1);
        let single_layer = Layer {
            id: new_id("layer"),
            name: source_name.to_owned(),
            visible: true,
            locked: false,
            keyframes,
            properties: LayerProperties::default(),
        };
        report.statistics.layers += 1;
        if let Some(root) = project.symbol_mut(&root_id) {
            root.timeline.frame_count = root.timeline.frame_count.max(frame_count);
            root.timeline.layers = vec![single_layer];
        }
    } else if let Some(root) = project.symbol_mut(&root_id)
        && !built_layers.is_empty()
    {
        // The player paints index zero first. Restore the PSD's bottom-to-top stacking order.
        built_layers.reverse();
        root.timeline.layers = built_layers;
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
    if project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .any(|e| !e.transform.color_transform.is_identity())
    {
        project.format_version = project.format_version.max(5);
    }
    project.upgrade_format_for_features();
    report.can_import = true;
    Ok(report)
}

/// Import a `.psd` / `.psb` binary buffer into a fresh [`Project`].
pub fn import_psd_bytes(
    bytes: &[u8],
    source_name: &str,
    options: &PsdImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    let mut project = Project::blank();
    if !source_name.trim().is_empty() {
        project.name = source_name.trim().to_owned();
    }
    let report = import_psd_into_project(&mut project, bytes, source_name, options)?;
    Ok((project, report))
}

/// Import a `.psd` / `.psb` file from disk into a fresh [`Project`].
pub fn import_psd_path(
    path: &Path,
    options: &PsdImportOptions,
) -> Result<(Project, ImportReport), ImportError> {
    if std::fs::metadata(path)?.len() > MAX_PSD_FILE_BYTES as u64 {
        return Err(ImportError::Container(format!(
            "PSD/PSB container exceeds the {} MiB file limit",
            MAX_PSD_FILE_BYTES / (1024 * 1024)
        )));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_PSD_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PSD_FILE_BYTES {
        return Err(ImportError::Container(format!(
            "PSD/PSB container exceeds the {} MiB file limit",
            MAX_PSD_FILE_BYTES / (1024 * 1024)
        )));
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PSD Document");
    import_psd_bytes(&bytes, name, options)
}
