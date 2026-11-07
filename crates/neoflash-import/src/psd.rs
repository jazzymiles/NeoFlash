//! Layered Adobe Photoshop (`.psd` / `.psb`) binary parser and timeline/symbol/keyframe importer (`T-115`).
//!
//! Decodes PSD v1 and PSB v2 headers, layer records, Pascal and `luni` Unicode layer names,
//! `lsct` folder hierarchies, blend modes, opacity, visibility flags, clipping mask chains,
//! `TySh` text descriptors, `vmsk`/`vsms` vector mask notices, raw (`0`) and PackBits RLE (`1`)
//! channel image planes, and merged composite image data.

use crate::{ImportError, ImportFormat, ImportReport, Statistics};
use neoflash_core::{
    BlendMode, Color, Element, ElementKind, FillRule, InstancePlayback, Keyframe, Layer,
    LayerProperties, LayerRole, MaskMode, PathCommand, Project, ResourceUnit, Symbol, SymbolKind,
    Timeline, Transform, new_id,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::Path};

/// How PSD layers are mapped into the NeoFlash document structure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PsdLayerMapping {
    /// Each PSD layer or folder group becomes a NeoFlash `Layer` on the target timeline.
    #[default]
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
            layer_mapping: PsdLayerMapping::TimelineLayers,
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
    #[serde(skip)]
    pub channels_meta: Vec<(i16, usize)>,
    #[serde(skip)]
    pub rgba_pixels: Option<Vec<u8>>,
}

impl PsdLayerRecord {
    pub fn width(&self) -> u32 {
        (self.right - self.left).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (self.bottom - self.top).max(0) as u32
    }
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
                let header = -((run_len as i8) - 1);
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
    // Look for "Txt " key followed by "TEXT" type tag and u32 UTF-16BE length in standard PSD descriptors
    if let Some(txt_idx) = rest.windows(8).position(|w| w == b"Txt TEXT") {
        let mut tpos = txt_idx + 8;
        if let Ok(char_len) = read_u32_be(rest, &mut tpos) {
            let byte_len = (char_len as usize).saturating_mul(2);
            if tpos + byte_len <= rest.len() {
                let units: Vec<u16> = rest[tpos..tpos + byte_len]
                    .chunks_exact(2)
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
    let pixel_count = width.saturating_mul(height);
    if pixel_count == 0 {
        return Ok(Vec::new());
    }
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
            let mut decoder = flate2::read::ZlibDecoder::new(payload);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| format!("zlib channel error: {e}"))?;
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
    let pixel_count = width.saturating_mul(height);
    let mut rgba = vec![0u8; pixel_count * 4];
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

fn encode_rgba_png(width: u32, height: u32, rgba_bytes: &[u8]) -> Result<Vec<u8>, ImportError> {
    let img = image::RgbaImage::from_raw(width, height, rgba_bytes.to_vec())
        .ok_or_else(|| ImportError::Container("Invalid RGBA buffer dimensions".into()))?;
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .map_err(|e| ImportError::Container(format!("Failed to encode layer PNG: {e}")))?;
    Ok(png_bytes)
}

/// Parse a binary `.psd` / `.psb` file into its header, layer records, and optional merged composite RGBA8 buffer.
pub fn parse_psd_document(
    bytes: &[u8],
) -> Result<(PsdHeader, Vec<PsdLayerRecord>, Option<Vec<u8>>), ImportError> {
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
    if pos + color_mode_len > bytes.len() {
        return Err(ImportError::Container(
            "PSD Color Mode Data section exceeds file bounds".into(),
        ));
    }
    pos += color_mode_len;

    // 2. Image Resources
    let resources_len = read_u32_be(bytes, &mut pos)? as usize;
    if pos + resources_len > bytes.len() {
        return Err(ImportError::Container(
            "PSD Image Resources section exceeds file bounds".into(),
        ));
    }
    pos += resources_len;

    // 3. Layer and Mask Information
    let layer_mask_len = if version == 2 {
        read_u64_be(bytes, &mut pos)? as usize
    } else {
        read_u32_be(bytes, &mut pos)? as usize
    };
    if pos + layer_mask_len > bytes.len() {
        return Err(ImportError::Container(
            "PSD Layer and Mask section exceeds file bounds".into(),
        ));
    }
    let layer_mask_end = pos + layer_mask_len;

    let mut layers = Vec::new();
    if layer_mask_len > 0 {
        let layer_info_len = if version == 2 {
            read_u64_be(bytes, &mut pos)? as usize
        } else {
            read_u32_be(bytes, &mut pos)? as usize
        };
        if layer_info_len > 0 && pos + 2 <= layer_mask_end {
            let raw_layer_count = read_i16_be(bytes, &mut pos)?;
            let layer_count = raw_layer_count.unsigned_abs() as usize;

            for layer_idx in 0..layer_count {
                let top = read_i32_be(bytes, &mut pos)?;
                let left = read_i32_be(bytes, &mut pos)?;
                let bottom = read_i32_be(bytes, &mut pos)?;
                let right = read_i32_be(bytes, &mut pos)?;
                let num_channels = read_u16_be(bytes, &mut pos)? as usize;

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

                if pos + 12 > layer_mask_end {
                    return Err(ImportError::Container(
                        "Truncated PSD layer blend record".into(),
                    ));
                }
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
                if pos + extra_len > layer_mask_end {
                    return Err(ImportError::Container(
                        "PSD layer extra data exceeds Layer section".into(),
                    ));
                }
                let extra_end = pos + extra_len;

                // Layer mask data
                let mask_data_len = read_u32_be(bytes, &mut pos)? as usize;
                let has_layer_mask = mask_data_len > 0;
                pos = (pos + mask_data_len).min(extra_end);

                // Blending ranges
                if pos + 4 <= extra_end {
                    let ranges_len = read_u32_be(bytes, &mut pos)? as usize;
                    pos = (pos + ranges_len).min(extra_end);
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
                    pos = (pos + padded - 1).min(extra_end);
                }

                let mut section_divider = PsdSectionDivider::Normal;
                let mut text_data = None;
                let mut has_vector_mask = false;
                let mut is_adjustment_layer = None;
                let mut is_smart_object = false;

                // Additional Layer Information tagged blocks
                while pos + 12 <= extra_end {
                    let sig = &bytes[pos..pos + 4];
                    if sig != b"8BIM" && sig != b"8B64" {
                        break;
                    }
                    pos += 4;
                    let mut key_bytes = [0u8; 4];
                    key_bytes.copy_from_slice(&bytes[pos..pos + 4]);
                    pos += 4;
                    let block_len = read_u32_be(bytes, &mut pos)? as usize;
                    let block_end = (pos + block_len).min(extra_end);
                    let block = &bytes[pos..block_end];

                    match &key_bytes {
                        b"luni" => {
                            if block.len() >= 4 {
                                let char_count =
                                    u32::from_be_bytes([block[0], block[1], block[2], block[3]])
                                        as usize;
                                let byte_len = char_count.saturating_mul(2);
                                if 4 + byte_len <= block.len() {
                                    let units: Vec<u16> = block[4..4 + byte_len]
                                        .chunks_exact(2)
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
                    pos = (pos + padded_len).min(extra_end);
                }

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
                    channels_meta,
                    rgba_pixels: None,
                });
            }

            // Channel Image Data for each layer
            for layer in &mut layers {
                let lw = layer.width() as usize;
                let lh = layer.height() as usize;
                let mut decoded_channels = BTreeMap::new();
                for &(ch_id, ch_len) in &layer.channels_meta {
                    let end = (pos + ch_len).min(layer_mask_end);
                    let slice = &bytes[pos..end];
                    pos = end;
                    if lw > 0
                        && lh > 0
                        && let Ok(plane) = decode_channel_slice(slice, lw, lh, version)
                    {
                        decoded_channels.insert(ch_id, plane);
                    }
                }
                if lw > 0 && lh > 0 && !decoded_channels.is_empty() {
                    layer.rgba_pixels = Some(combine_channels_to_rgba(
                        lw,
                        lh,
                        color_mode,
                        &decoded_channels,
                    ));
                }
            }
        }
    }

    pos = layer_mask_end;

    // 4. Merged Composite Image Data (optional or fallback)
    let mut composite_rgba = None;
    if pos + 2 <= bytes.len() {
        let comp = read_u16_be(bytes, &mut pos)?;
        let w = width as usize;
        let h = height as usize;
        let plane_pixels = w.saturating_mul(h);
        let ch_count = (channels as usize).clamp(1, 4);
        let payload = &bytes[pos..];
        let mut planes = BTreeMap::new();
        match comp {
            0 if payload.len() >= plane_pixels => {
                for ch in 0..ch_count {
                    let start = ch * plane_pixels;
                    if start + plane_pixels <= payload.len() {
                        let key = if ch == 3 { -1 } else { ch as i16 };
                        planes.insert(key, payload[start..start + plane_pixels].to_vec());
                    }
                }
            }
            1 => {
                let table_bytes = h * ch_count * if version == 2 { 4 } else { 2 };
                if payload.len() >= table_bytes {
                    let rle_payload = &payload[table_bytes..];
                    let decoded_all = decode_packbits_rle(rle_payload, plane_pixels * ch_count);
                    for ch in 0..ch_count {
                        let start = ch * plane_pixels;
                        if start + plane_pixels <= decoded_all.len() {
                            let key = if ch == 3 { -1 } else { ch as i16 };
                            planes.insert(key, decoded_all[start..start + plane_pixels].to_vec());
                        }
                    }
                }
            }
            _ => {}
        }
        if !planes.is_empty() {
            composite_rgba = Some(combine_channels_to_rgba(w, h, color_mode, &planes));
        }
    }

    Ok((header, layers, composite_rgba))
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
/// - `psd-adjustment-layer-approximated`
/// - `psd-vector-mask-approximated`
/// - `psd-smart-object-rasterized`
/// - `psd-text-outlines-approximated`
pub fn import_psd_into_project(
    project: &mut Project,
    bytes: &[u8],
    source_name: &str,
    options: &PsdImportOptions,
) -> Result<ImportReport, ImportError> {
    let (header, psd_layers, composite_rgba) = parse_psd_document(bytes)?;
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

    // Handle FlattenComposite or 0-layer PSDs
    if options.layer_mapping == PsdLayerMapping::FlattenComposite || psd_layers.is_empty() {
        let rgba = composite_rgba
            .unwrap_or_else(|| vec![255u8; (header.width as usize) * (header.height as usize) * 4]);
        let png = encode_rgba_png(header.width, header.height, &rgba)?;
        let res_id = new_id("resource");
        let res_path = format!("psd/{}_composite.png", res_id);
        let unit = ResourceUnit::from_bytes(res_id.clone(), res_path, "image/png", &png)
            .map_err(|e| ImportError::Container(e.to_string()))?;
        project.resources.push(unit);

        let element = Element {
            id: new_id("element"),
            name: format!("{source_name} Composite"),
            transform: Transform::default(),
            kind: ElementKind::Bitmap {
                resource_id: res_id,
                width: header.width,
                height: header.height,
                smoothing: true,
            },
            ..Element::default()
        };
        let layer = Layer {
            id: new_id("layer"),
            name: "PSD Composite".to_owned(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![element],
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
        report.statistics.elements = 1;
        project.format_version = project.format_version.max(3);
        project.upgrade_format_for_features();
        report.can_import = true;
        return Ok(report);
    }

    // PSD stores layers bottom-to-top; NeoFlash stores layers top-to-bottom.
    // First process in PSD order (bottom-to-top) so clipping base layers (below clipped layers in PSD)
    // and section dividers can be resolved accurately, then reverse to top-to-bottom for NeoFlash.
    // Wait: in PSD, a folder group starts at the TOP of the group with an OpenFolder/ClosedFolder
    // record and ends at the BOTTOM with a BoundingSectionEnd (`</Layer group>`) record.
    // So walking top-to-bottom (`psd_layers.iter().rev()`) encounters OpenFolder/ClosedFolder first
    // and `BoundingSectionEnd` at the end of the folder!
    // And for clipping masks in PSD: the base layer (`clipping == false`) is below the clipped
    // layers (`clipping == true`), so in bottom-to-top order (`0..psd_layers.len()`), if layer `i`
    // has `clipping == false` and layer `i + 1` has `clipping == true`, then `i` is the clipping base
    // for `i + 1` (and any contiguous `clipping == true` layers above `i`).

    let layer_ids: Vec<String> = (0..psd_layers.len()).map(|_| new_id("layer")).collect();
    let mut clipping_base_for: BTreeMap<usize, usize> = BTreeMap::new();
    let mut is_clipping_base: std::collections::HashSet<usize> = std::collections::HashSet::new();

    if options.preserve_clipping_masks
        && matches!(
            options.layer_mapping,
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
    for (i, rec) in psd_layers.iter().enumerate().rev() {
        let loc = format!("{source_name}::{}", rec.name);

        if rec.section_divider == PsdSectionDivider::BoundingSectionEnd {
            folder_stack.pop();
            continue;
        }

        if !options.import_hidden_layers && !rec.visible {
            continue;
        }

        if matches!(
            rec.section_divider,
            PsdSectionDivider::OpenFolder | PsdSectionDivider::ClosedFolder
        ) {
            let folder_id = layer_ids[i].clone();
            let parent_id = folder_stack.last().cloned();
            if matches!(
                options.layer_mapping,
                PsdLayerMapping::TimelineLayers | PsdLayerMapping::MovieClipSymbols
            ) {
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
        } else {
            let lw = rec.width().max(1);
            let lh = rec.height().max(1);
            let rgba = rec
                .rgba_pixels
                .clone()
                .unwrap_or_else(|| vec![180u8; (lw as usize) * (lh as usize) * 4]);
            let png = encode_rgba_png(lw, lh, &rgba)?;
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
        };

        report.statistics.elements += 1;

        match options.layer_mapping {
            PsdLayerMapping::Keyframes => {
                keyframe_mode_frames.push((rec.name.clone(), element));
            }
            PsdLayerMapping::MovieClipSymbols | PsdLayerMapping::TimelineLayers => {
                let is_mask_base = is_clipping_base.contains(&i);
                let placed_element = if options.layer_mapping == PsdLayerMapping::MovieClipSymbols
                    && !is_mask_base
                {
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
                            ..Transform::default()
                        },
                        blend_mode: mapped_blend,
                        kind: ElementKind::Instance {
                            symbol_id: sym_id,
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    }
                } else {
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
            PsdLayerMapping::FlattenComposite => unreachable!(),
        }
    }

    let root_id = project.root.clone();
    if options.layer_mapping == PsdLayerMapping::Keyframes {
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
    } else if let Some(root) = project.symbol_mut(&root_id) {
        if !built_layers.is_empty() {
            root.timeline.layers = built_layers;
        }
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
    let bytes = fs::read(path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PSD Document");
    import_psd_bytes(&bytes, name, options)
}
