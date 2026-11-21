//! Multi-format still and sequence image export (`DL-04`, `W3.1`, `W3.2`):
//! PNG, SVG, PSD (layered Photoshop v1), AI (Illustrator vector), and PDF 1.4.

use crate::{
    ExportError, SvgOptions, atomic_write, render_authored_scene_rgba, render_project_frame_rgba,
    render_scene_svg,
    sequence_set::{
        SequenceSetTransaction, ensure_directory_chain_tracked, remove_empty_dirs_reverse,
    },
    video_scene_ranges,
};
use image::{ImageFormat, RgbaImage};
use neoflash_core::{
    AuthoredDraw, AuthoredScene, Color, ColorTransform, ElementKind, FillRule, GradientKind,
    GradientPaint, GradientSpread, LayerRole, PathCommand, Project, evaluate_scene,
    flatten_scenes_with_ranges, layer_effectively_visible,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    io::Cursor,
    path::{Path, PathBuf},
};

/// Output format selected in `Dialog / Export image` (`DL-04`) or `Publish settings` sequence export (`T-255`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageExportFormat {
    #[default]
    Png,
    Svg,
    Psd,
    Ai,
    Pdf,
    Jpeg,
    Webp,
    Gif,
    Bmp,
}

impl ImageExportFormat {
    /// The five locked `DL-04` dialog tabs (`PNG`, `SVG`, `PSD`, `AI`, `PDF`).
    pub const ALL: [Self; 5] = [Self::Png, Self::Svg, Self::Psd, Self::Ai, Self::Pdf];

    /// Multi-format sequence formats (`PNG`, `JPEG`, `WebP`, `GIF`, `BMP`, `SVG`, `AI`) (`T-255`).
    pub const SEQUENCE_FORMATS: [Self; 7] = [
        Self::Png,
        Self::Jpeg,
        Self::Webp,
        Self::Gif,
        Self::Bmp,
        Self::Svg,
        Self::Ai,
    ];

    /// Every supported still and sequence image format.
    pub const ALL_FORMATS: [Self; 9] = [
        Self::Png,
        Self::Svg,
        Self::Psd,
        Self::Ai,
        Self::Pdf,
        Self::Jpeg,
        Self::Webp,
        Self::Gif,
        Self::Bmp,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Png => 0,
            Self::Svg => 1,
            Self::Psd => 2,
            Self::Ai => 3,
            Self::Pdf => 4,
            Self::Jpeg | Self::Webp | Self::Gif | Self::Bmp => 0,
        }
    }

    pub const fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Svg,
            2 => Self::Psd,
            3 => Self::Ai,
            4 => Self::Pdf,
            _ => Self::Png,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Svg => "SVG",
            Self::Psd => "PSD",
            Self::Ai => "AI",
            Self::Pdf => "PDF",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Gif => "GIF",
            Self::Bmp => "BMP",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
            Self::Psd => "psd",
            Self::Ai => "ai",
            Self::Pdf => "pdf",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
        }
    }

    pub const fn button_label(self) -> &'static str {
        match self {
            Self::Png => "Export PNG",
            Self::Svg => "Export SVG",
            Self::Psd => "Export PSD",
            Self::Ai => "Export AI",
            Self::Pdf => "Export PDF",
            Self::Jpeg => "Export JPEG",
            Self::Webp => "Export WebP",
            Self::Gif => "Export GIF",
            Self::Bmp => "Export BMP",
        }
    }
}

fn project_has_dynamic_motion(project: &Project) -> bool {
    !project.sources.is_empty()
        || project.symbols.iter().any(|s| {
            !s.timeline.scripts.is_empty()
                || (s.id != project.root && s.timeline.frame_count > 1)
                || s.timeline
                    .layers
                    .iter()
                    .flat_map(|l| &l.keyframes)
                    .flat_map(|k| &k.elements)
                    .any(|e| !e.scripts.is_empty())
        })
}

/// Frame range for `Dialog / Export image`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageExportRange {
    CurrentFrame(u32),
    WholeMovie,
    CurrentScene { scene_id: String },
    Frames { start: u32, end: u32 },
}

impl Default for ImageExportRange {
    fn default() -> Self {
        Self::CurrentFrame(0)
    }
}

impl ImageExportRange {
    pub fn label(&self) -> &'static str {
        match self {
            Self::CurrentFrame(_) => "Current frame",
            Self::WholeMovie => "Whole movie",
            Self::CurrentScene { .. } => "Current scene",
            Self::Frames { .. } => "Frame range",
        }
    }

    pub fn resolve_frames(&self, project: &Project, symbol_id: Option<&str>) -> (u32, u32) {
        let total = symbol_id
            .and_then(|id| project.symbol(id))
            .map(|s| s.timeline.frame_count.max(1))
            .unwrap_or_else(|| {
                let (flat, _) = flatten_scenes_with_ranges(project);
                flat.symbol(&flat.root)
                    .map(|s| s.timeline.frame_count.max(1))
                    .unwrap_or(1)
            });
        let last = total.saturating_sub(1);
        match self {
            Self::CurrentFrame(f) => {
                let f = (*f).min(last);
                (f, f)
            }
            Self::WholeMovie => (0, last),
            Self::CurrentScene { scene_id } => {
                if symbol_id == Some(scene_id.as_str()) {
                    return (0, last);
                }
                for (sym_id, _name, start_1, end_1) in video_scene_ranges(project) {
                    if &sym_id == scene_id {
                        let s = start_1.saturating_sub(1).min(last);
                        let e = end_1.saturating_sub(1).min(last);
                        return (s, e.max(s));
                    }
                }
                (0, last)
            }
            Self::Frames { start, end } => {
                let max_last = if last == 0 && project_has_dynamic_motion(project) {
                    9_999
                } else {
                    last
                };
                let s = (*start).min(max_last);
                let e = (*end).min(max_last).max(s);
                (s, e)
            }
        }
    }
}

const fn default_image_jpeg_quality() -> u8 {
    90
}

const fn default_image_runtime_capture() -> bool {
    true
}

/// Full configuration for `Dialog / Export image` (`DL-04`) and sequence export (`T-255`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageExportOptions {
    pub format: ImageExportFormat,
    pub symbol_id: Option<String>,
    pub range: ImageExportRange,
    pub width: u32,
    pub height: u32,
    pub constrain_proportions: bool,
    pub scale: f32,
    pub transparent_background: bool,
    #[serde(default = "default_image_jpeg_quality")]
    pub jpeg_quality: u8,
    #[serde(default = "default_image_runtime_capture")]
    pub runtime_capture: bool,
    // SVG options
    pub svg_embed_images: bool,
    pub svg_text_as_outlines: bool,
    pub svg_minify: bool,
    pub svg_css_classes: bool,
    pub svg_include_hidden_guides: bool,
    pub svg_precision: usize,
    pub svg_id_prefix: String,
    // PSD options
    pub psd_keep_layers: bool,
    pub psd_folders_to_groups: bool,
    pub psd_symbols_to_smart_objects: bool,
    pub psd_keep_text_editable: bool,
    pub psd_color_space: String,
    pub psd_resolution_ppi: u32,
    // AI options
    pub ai_version: String,
    pub ai_each_layer: bool,
    pub ai_strokes_as_strokes: bool,
    pub ai_text_as_outlines: bool,
}

impl Default for ImageExportOptions {
    fn default() -> Self {
        Self {
            format: ImageExportFormat::Png,
            symbol_id: None,
            range: ImageExportRange::CurrentFrame(0),
            width: 960,
            height: 600,
            constrain_proportions: true,
            scale: 1.0,
            transparent_background: false,
            jpeg_quality: default_image_jpeg_quality(),
            runtime_capture: true,
            svg_embed_images: true,
            svg_text_as_outlines: true,
            svg_minify: true,
            svg_css_classes: false,
            svg_include_hidden_guides: false,
            svg_precision: 2,
            svg_id_prefix: "nf-".to_owned(),
            psd_keep_layers: true,
            psd_folders_to_groups: true,
            psd_symbols_to_smart_objects: true,
            psd_keep_text_editable: true,
            psd_color_space: "sRGB 8-bit".to_owned(),
            psd_resolution_ppi: 144,
            ai_version: "Illustrator CS6+".to_owned(),
            ai_each_layer: true,
            ai_strokes_as_strokes: true,
            ai_text_as_outlines: false,
        }
    }
}

impl ImageExportOptions {
    pub fn for_project(project: &Project, current_frame: u32) -> Self {
        Self {
            width: project.width.max(1),
            height: project.height.max(1),
            range: ImageExportRange::CurrentFrame(current_frame),
            ..Self::default()
        }
    }

    pub fn resolved_dimensions(&self, project: &Project) -> (u32, u32) {
        let base_w = if self.width > 0 {
            self.width
        } else {
            project.width.max(1)
        };
        let base_h = if self.height > 0 {
            self.height
        } else {
            project.height.max(1)
        };
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale.clamp(0.1, 8.0)
        } else {
            1.0
        };
        let w = ((base_w as f32) * scale).round().clamp(1.0, 8192.0) as u32;
        let h = ((base_h as f32) * scale).round().clamp(1.0, 8192.0) as u32;
        (w, h)
    }
}

/// Summary of a completed `export_project_image` operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageExportReport {
    pub format: ImageExportFormat,
    pub files: Vec<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub total_bytes: usize,
    pub layer_count: usize,
    pub path_count: usize,
}

/// Estimate footer caption (`"~ 18 kB · 42 paths"`, `"~ 4.2 MB · 6 layers"`) for `Dialog / Export image`.
pub fn estimate_image_export_summary(project: &Project, options: &ImageExportOptions) -> String {
    let (w, h) = options.resolved_dimensions(project);
    let (start, end) = options
        .range
        .resolve_frames(project, options.symbol_id.as_deref());
    let frames = end.saturating_sub(start).saturating_add(1).max(1) as usize;
    let sym = options
        .symbol_id
        .as_deref()
        .and_then(|id| project.symbol(id))
        .or_else(|| project.symbol(&project.root));
    let layers = sym
        .map(|s| {
            s.timeline
                .layers
                .iter()
                .filter(|l| !matches!(l.properties.role, LayerRole::Guide | LayerRole::Folder))
                .count()
                .max(1)
        })
        .unwrap_or(1);
    let paths = sym
        .map(|s| {
            let scene = evaluate_scene(project, &s.id, start, false, true);
            scene.draws.len().max(1)
        })
        .unwrap_or(1);

    let approx_bytes = match options.format {
        ImageExportFormat::Png | ImageExportFormat::Webp | ImageExportFormat::Gif => {
            (w as usize * h as usize / 3).max(1024) * frames
        }
        ImageExportFormat::Jpeg => (w as usize * h as usize / 5).max(1024) * frames,
        ImageExportFormat::Bmp => (w as usize * h as usize * 4 + 54).max(1024) * frames,
        ImageExportFormat::Svg => (paths * 380 + 1024) * frames,
        ImageExportFormat::Psd => {
            let layer_mult = if options.psd_keep_layers {
                layers + 1
            } else {
                1
            };
            (w as usize * h as usize * 4 * layer_mult / 2).max(4096) * frames
        }
        ImageExportFormat::Ai => (paths * 520 + layers * 256 + 2048) * frames,
        ImageExportFormat::Pdf => (w as usize * h as usize / 2 + paths * 256 + 2048) * frames,
    };

    let size_str = if approx_bytes < 1024 {
        format!("~ {approx_bytes} B")
    } else if approx_bytes < 1024 * 1024 {
        format!("~ {} kB", (approx_bytes + 512) / 1024)
    } else {
        format!("~ {:.1} MB", approx_bytes as f64 / (1024.0 * 1024.0))
    };

    match options.format {
        ImageExportFormat::Svg => format!("{size_str} · {paths} paths"),
        ImageExportFormat::Psd | ImageExportFormat::Ai => {
            format!("{size_str} · {layers} layers")
        }
        ImageExportFormat::Png
        | ImageExportFormat::Pdf
        | ImageExportFormat::Jpeg
        | ImageExportFormat::Webp
        | ImageExportFormat::Gif
        | ImageExportFormat::Bmp => {
            if frames > 1 {
                format!("{size_str} · {frames} frames")
            } else {
                format!("{size_str} · {w} × {h}")
            }
        }
    }
}

fn sequence_output_stem(base: &Path) -> &str {
    let raw_stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("export");
    raw_stem
        .strip_suffix("_0001")
        .filter(|s| !s.is_empty())
        .unwrap_or(raw_stem)
}

fn numbered_output_path(
    base: &Path,
    frame_offset: u32,
    total_frames: u32,
    ext: &str,
    is_sequence_export: bool,
) -> PathBuf {
    if !is_sequence_export {
        return base.with_extension(ext);
    }
    let parent = base
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = sequence_output_stem(base);
    if total_frames <= 1 {
        parent.join(format!("{stem}.{ext}"))
    } else {
        parent.join(format!("{stem}_{:04}.{ext}", frame_offset + 1))
    }
}

fn safe_svg_id_prefix(value: &str) -> String {
    let mut prefix = String::with_capacity(value.len().max(3));
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':') {
            prefix.push(ch);
        } else {
            prefix.push('_');
        }
    }
    if prefix.is_empty() {
        return "nf-".to_owned();
    }
    if !prefix.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') {
        prefix.insert_str(0, "nf-");
    }
    prefix
}

fn encode_png_bytes(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ExportError> {
    crate::bitmap_encode::encode_rgba_png(rgba, width, height)
}

fn encode_jpeg_bytes(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    bg: Color,
) -> Result<Vec<u8>, ExportError> {
    crate::bitmap_encode::encode_rgba_jpeg_composited(rgba, width, height, quality, bg)
}

fn encode_webp_bytes(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ExportError> {
    crate::bitmap_encode::encode_rgba_webp_lossless(rgba, width, height)
}

fn encode_gif_bytes(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ExportError> {
    let img = RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        ExportError::InvalidProject(format!("Invalid GIF RGBA buffer {width}x{height}"))
    })?;
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, ImageFormat::Gif)
        .map_err(|e| ExportError::InvalidProject(format!("GIF encode failed: {e}")))?;
    Ok(cursor.into_inner())
}

fn encode_bmp_bytes(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ExportError> {
    let img = RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        ExportError::InvalidProject(format!("Invalid BMP RGBA buffer {width}x{height}"))
    })?;
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, ImageFormat::Bmp)
        .map_err(|e| ExportError::InvalidProject(format!("BMP encode failed: {e}")))?;
    Ok(cursor.into_inner())
}

const MAX_PSD_EXPORT_BYTES: usize = 256 * 1024 * 1024;

#[cfg(test)]
fn build_psd_bytes(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
) -> Result<(Vec<u8>, usize), ExportError> {
    build_psd_bytes_with_composite(project, symbol_id, frame, width, height, options, None)
}

fn build_psd_bytes_with_composite(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
    precomputed_composite: Option<&[u8]>,
) -> Result<(Vec<u8>, usize), ExportError> {
    let flattened_project = if symbol_id.is_none() && !project.scenes.is_empty() {
        Some(flatten_scenes_with_ranges(project).0)
    } else {
        None
    };
    let project = flattened_project.as_ref().unwrap_or(project);

    // Collect only layer identities and names. Pixel buffers are rendered and
    // appended one layer at a time below, so memory does not grow by a full
    // RGBA frame for every visible layer.
    let mut layer_records: Vec<(usize, String)> = Vec::new();
    let sym_id = symbol_id
        .map(str::to_owned)
        .unwrap_or_else(|| project.root.clone());
    if options.psd_keep_layers
        && let Some(sym) = project.symbol(&sym_id)
    {
        for (idx, layer) in sym.timeline.layers.iter().enumerate().rev() {
            if matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder) {
                continue;
            }
            if !layer_effectively_visible(&sym.timeline, layer) {
                continue;
            }
            layer_records.push((idx, layer.name.clone()));
        }
    }

    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| ExportError::InvalidProject("PSD pixel count overflowed".into()))?;
    let frame_bytes = pixel_count
        .checked_mul(4)
        .ok_or_else(|| ExportError::InvalidProject("PSD frame byte count overflowed".into()))?;
    let per_layer_bytes = frame_bytes
        .checked_add(8)
        .ok_or_else(|| ExportError::InvalidProject("PSD layer byte count overflowed".into()))?;
    let predicted_pixel_bytes = per_layer_bytes
        .checked_mul(layer_records.len())
        .and_then(|layers| layers.checked_add(frame_bytes))
        .and_then(|bytes| bytes.checked_add(2))
        .ok_or_else(|| ExportError::InvalidProject("PSD output size overflowed".into()))?;
    if predicted_pixel_bytes > MAX_PSD_EXPORT_BYTES {
        return Err(ExportError::InvalidProject(format!(
            "Layered PSD pixel data would require at least {} MiB, exceeding the {} MiB bounded export limit; reduce dimensions or disable Keep layers",
            predicted_pixel_bytes.div_ceil(1024 * 1024),
            MAX_PSD_EXPORT_BYTES / (1024 * 1024)
        )));
    }

    let mut out = Vec::new();
    out.try_reserve(predicted_pixel_bytes.saturating_add(4096))
        .map_err(|_| ExportError::InvalidProject("PSD output allocation failed".into()))?;
    // 1. File Header (26 bytes)
    out.extend_from_slice(b"8BPS");
    out.extend_from_slice(&1u16.to_be_bytes()); // version 1 (PSD)
    out.extend_from_slice(&[0u8; 6]); // reserved
    out.extend_from_slice(&4u16.to_be_bytes()); // 4 channels (R, G, B, A)
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes()); // 8 bits per channel
    out.extend_from_slice(&3u16.to_be_bytes()); // RGB color mode (3)

    // 2. Color Mode Data Section (0 length)
    out.extend_from_slice(&0u32.to_be_bytes());

    // 3. Image Resources Section (ResolutionInfo 0x03ED)
    let ppi = options.psd_resolution_ppi.clamp(36, 1200);
    let fixed_ppi = ppi << 16; // 16.16 fixed-point
    let mut res_block = Vec::new();
    res_block.extend_from_slice(b"8BIM");
    res_block.extend_from_slice(&0x03EDu16.to_be_bytes());
    res_block.extend_from_slice(&0u16.to_be_bytes()); // empty Pascal name (2 bytes padded)
    res_block.extend_from_slice(&16u32.to_be_bytes()); // data size = 16
    res_block.extend_from_slice(&fixed_ppi.to_be_bytes());
    res_block.extend_from_slice(&1u16.to_be_bytes()); // hResUnit = pixels/inch
    res_block.extend_from_slice(&1u16.to_be_bytes()); // widthUnit = inches
    res_block.extend_from_slice(&fixed_ppi.to_be_bytes());
    res_block.extend_from_slice(&1u16.to_be_bytes()); // vResUnit = pixels/inch
    res_block.extend_from_slice(&1u16.to_be_bytes()); // heightUnit = inches
    out.extend_from_slice(&(res_block.len() as u32).to_be_bytes());
    out.extend_from_slice(&res_block);

    // 4. Layer and Mask Information Section
    let channel_data_len = pixel_count
        .checked_add(2)
        .ok_or_else(|| ExportError::InvalidProject("PSD channel size overflowed".into()))?;
    if layer_records.is_empty() {
        out.extend_from_slice(&0u32.to_be_bytes());
    } else {
        let layer_count = i16::try_from(layer_records.len()).map_err(|_| {
            ExportError::InvalidProject("PSD v1 supports at most 32767 layer records".into())
        })?;
        let channel_data_len = u32::try_from(channel_data_len).map_err(|_| {
            ExportError::InvalidProject("PSD v1 channel data exceeds its 32-bit limit".into())
        })?;
        let mut layer_metadata = Vec::new();
        layer_metadata.extend_from_slice(&layer_count.to_be_bytes());
        for (_, name) in &layer_records {
            layer_metadata.extend_from_slice(&0i32.to_be_bytes()); // top
            layer_metadata.extend_from_slice(&0i32.to_be_bytes()); // left
            layer_metadata.extend_from_slice(&(height as i32).to_be_bytes()); // bottom
            layer_metadata.extend_from_slice(&(width as i32).to_be_bytes()); // right
            layer_metadata.extend_from_slice(&4u16.to_be_bytes()); // 4 channels
            for ch_id in [0i16, 1, 2, -1] {
                layer_metadata.extend_from_slice(&ch_id.to_be_bytes());
                layer_metadata.extend_from_slice(&channel_data_len.to_be_bytes());
            }
            layer_metadata.extend_from_slice(b"8BIM");
            layer_metadata.extend_from_slice(b"norm");
            layer_metadata.push(255); // opacity
            layer_metadata.push(0); // clipping
            layer_metadata.push(0); // flags (visible)
            layer_metadata.push(0); // filler

            // Extra data field: mask (0 bytes) + blending ranges (0 bytes) + Pascal name padded to 4 bytes
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len().min(255);
            let pascal_total = (1 + name_len + 3) & !3;
            let extra_len = 4 + 4 + pascal_total;
            layer_metadata.extend_from_slice(&(extra_len as u32).to_be_bytes());
            layer_metadata.extend_from_slice(&0u32.to_be_bytes()); // layer mask data length
            layer_metadata.extend_from_slice(&0u32.to_be_bytes()); // layer blending ranges length
            layer_metadata.push(name_len as u8);
            layer_metadata.extend_from_slice(&name_bytes[..name_len]);
            layer_metadata.resize(layer_metadata.len() + pascal_total - (1 + name_len), 0);
        }

        let layer_pixels_len = per_layer_bytes
            .checked_mul(layer_records.len())
            .ok_or_else(|| ExportError::InvalidProject("PSD layer data overflowed".into()))?;
        let layer_info_len = layer_metadata
            .len()
            .checked_add(layer_pixels_len)
            .ok_or_else(|| ExportError::InvalidProject("PSD layer section overflowed".into()))?;
        let padded_layer_info_len = layer_info_len + (layer_info_len & 1);
        let section_len = 4usize
            .checked_add(padded_layer_info_len)
            .and_then(|len| len.checked_add(4))
            .ok_or_else(|| ExportError::InvalidProject("PSD layer section overflowed".into()))?;
        let predicted_total = out
            .len()
            .checked_add(4)
            .and_then(|len| len.checked_add(section_len))
            .and_then(|len| len.checked_add(2))
            .and_then(|len| len.checked_add(frame_bytes))
            .ok_or_else(|| ExportError::InvalidProject("PSD output size overflowed".into()))?;
        if predicted_total > MAX_PSD_EXPORT_BYTES {
            return Err(ExportError::InvalidProject(format!(
                "Layered PSD output would require {} MiB, exceeding the {} MiB bounded export limit; reduce dimensions or disable Keep layers",
                predicted_total.div_ceil(1024 * 1024),
                MAX_PSD_EXPORT_BYTES / (1024 * 1024)
            )));
        }
        let section_len = u32::try_from(section_len).map_err(|_| {
            ExportError::InvalidProject("PSD v1 layer section exceeds its 32-bit limit".into())
        })?;
        let layer_info_len_u32 = u32::try_from(padded_layer_info_len).map_err(|_| {
            ExportError::InvalidProject("PSD v1 layer information exceeds its 32-bit limit".into())
        })?;
        out.extend_from_slice(&section_len.to_be_bytes());
        out.extend_from_slice(&layer_info_len_u32.to_be_bytes());
        out.extend_from_slice(&layer_metadata);

        let mut bitmap_cache = HashMap::new();
        // Channel image data for each layer (R, G, B, A planes), rendered one
        // layer at a time in the same bottom-to-top record order as before.
        for (layer_index, _) in &layer_records {
            let mut single_proj = project.clone();
            if let Some(target_sym) = single_proj.symbol_mut(&sym_id) {
                for (other_idx, other_layer) in target_sym.timeline.layers.iter_mut().enumerate() {
                    if other_idx != *layer_index {
                        other_layer.visible = false;
                    }
                }
            }
            let rgba = if options.runtime_capture {
                render_project_frame_rgba(
                    &single_proj,
                    Some(&sym_id),
                    frame,
                    width,
                    height,
                    true,
                    true,
                )?
            } else {
                let scene = evaluate_scene(&single_proj, &sym_id, frame, false, true);
                crate::render_authored_scene_rgba(
                    &single_proj,
                    &sym_id,
                    frame,
                    &scene,
                    width,
                    height,
                    true,
                    &mut bitmap_cache,
                )?
            };
            if rgba.len() != frame_bytes {
                return Err(ExportError::InvalidProject(format!(
                    "PSD layer renderer returned {} bytes for a {width}x{height} RGBA frame",
                    rgba.len()
                )));
            }
            for c_idx in [0usize, 1, 2, 3] {
                out.extend_from_slice(&0u16.to_be_bytes()); // raw compression
                for px in rgba.as_chunks::<4>().0 {
                    out.push(px[c_idx]);
                }
            }
        }
        if layer_info_len & 1 != 0 {
            out.push(0);
        }
        out.extend_from_slice(&0u32.to_be_bytes()); // global layer mask info length = 0
    }

    // 5. Composite Image Data Section (Raw planar R, G, B, A)
    let final_len = out
        .len()
        .checked_add(2)
        .and_then(|len| len.checked_add(frame_bytes))
        .ok_or_else(|| ExportError::InvalidProject("PSD output size overflowed".into()))?;
    if final_len > MAX_PSD_EXPORT_BYTES {
        return Err(ExportError::InvalidProject(format!(
            "PSD output would require {} MiB, exceeding the {} MiB bounded export limit; reduce dimensions",
            final_len.div_ceil(1024 * 1024),
            MAX_PSD_EXPORT_BYTES / (1024 * 1024)
        )));
    }
    let owned_composite;
    let composite: &[u8] = if let Some(pre) = precomputed_composite {
        pre
    } else {
        owned_composite = render_project_frame_rgba(
            project,
            symbol_id,
            frame,
            width,
            height,
            options.transparent_background,
            options.runtime_capture,
        )?;
        &owned_composite
    };
    if composite.len() != frame_bytes {
        return Err(ExportError::InvalidProject(format!(
            "PSD composite renderer returned {} bytes for a {width}x{height} RGBA frame",
            composite.len()
        )));
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // compression = 0 (Raw)
    for c_idx in [0usize, 1, 2, 3] {
        for px in composite.as_chunks::<4>().0 {
            out.push(px[c_idx]);
        }
    }

    Ok((out, layer_records.len().max(1)))
}

fn ps_num(value: f64) -> String {
    let value = if value.is_finite() { value } else { 0.0 };
    let mut value = format!("{value:.6}");
    while value.contains('.') && value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    if value == "-0" { "0".into() } else { value }
}

fn ps_comment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn ps_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(byte));
            }
            0x20..=0x7e => out.push(char::from(byte)),
            _ => {
                let _ = write!(out, "\\{byte:03o}");
            }
        }
    }
    out
}

fn ps_rgb(out: &mut String, color: Color) {
    let _ = writeln!(
        out,
        "{} {} {} setrgbcolor",
        ps_num(f64::from(color[0]) / 255.0),
        ps_num(f64::from(color[1]) / 255.0),
        ps_num(f64::from(color[2]) / 255.0)
    );
}

fn ps_path(out: &mut String, commands: &[PathCommand]) -> bool {
    if commands.is_empty() {
        return false;
    }
    out.push_str("newpath\n");
    let mut current = (0.0, 0.0);
    let mut start = current;
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                let _ = writeln!(out, "{} {} moveto", ps_num(x), ps_num(y));
                current = (x, y);
                start = current;
            }
            PathCommand::LineTo { x, y } => {
                let _ = writeln!(out, "{} {} lineto", ps_num(x), ps_num(y));
                current = (x, y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let c1 = (
                    current.0 + (cx - current.0) * 2.0 / 3.0,
                    current.1 + (cy - current.1) * 2.0 / 3.0,
                );
                let c2 = (x + (cx - x) * 2.0 / 3.0, y + (cy - y) * 2.0 / 3.0);
                let _ = writeln!(
                    out,
                    "{} {} {} {} {} {} curveto",
                    ps_num(c1.0),
                    ps_num(c1.1),
                    ps_num(c2.0),
                    ps_num(c2.1),
                    ps_num(x),
                    ps_num(y)
                );
                current = (x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let _ = writeln!(
                    out,
                    "{} {} {} {} {} {} curveto",
                    ps_num(cx1),
                    ps_num(cy1),
                    ps_num(cx2),
                    ps_num(cy2),
                    ps_num(x),
                    ps_num(y)
                );
                current = (x, y);
            }
            PathCommand::Close => {
                out.push_str("closepath\n");
                current = start;
            }
        }
    }
    true
}

fn ps_paint(
    out: &mut String,
    rule: FillRule,
    fill: Color,
    stroke: Color,
    stroke_width: f64,
    transform: ColorTransform,
) {
    let fill = transform.apply(fill);
    let stroke = transform.apply(stroke);
    if fill[3] > 0 {
        out.push_str("gsave\n");
        ps_rgb(out, fill);
        out.push_str(if rule == FillRule::EvenOdd {
            "eofill\n"
        } else {
            "fill\n"
        });
        out.push_str("grestore\n");
    }
    if stroke_width > 0.0 && stroke[3] > 0 {
        ps_rgb(out, stroke);
        let _ = writeln!(out, "{} setlinewidth", ps_num(stroke_width));
        out.push_str("1 setlinecap\n1 setlinejoin\nstroke\n");
    }
}

fn rectangle_path(width: f64, height: f64) -> [PathCommand; 5] {
    [
        PathCommand::MoveTo { x: 0.0, y: 0.0 },
        PathCommand::LineTo { x: width, y: 0.0 },
        PathCommand::LineTo {
            x: width,
            y: height,
        },
        PathCommand::LineTo { x: 0.0, y: height },
        PathCommand::Close,
    ]
}

fn ellipse_path(width: f64, height: f64) -> [PathCommand; 6] {
    const K: f64 = 0.552_284_749_830_793_6;
    let (rx, ry) = (width * 0.5, height * 0.5);
    [
        PathCommand::MoveTo { x: width, y: ry },
        PathCommand::CubicTo {
            cx1: width,
            cy1: ry + K * ry,
            cx2: rx + K * rx,
            cy2: height,
            x: rx,
            y: height,
        },
        PathCommand::CubicTo {
            cx1: rx - K * rx,
            cy1: height,
            cx2: 0.0,
            cy2: ry + K * ry,
            x: 0.0,
            y: ry,
        },
        PathCommand::CubicTo {
            cx1: 0.0,
            cy1: ry - K * ry,
            cx2: rx - K * rx,
            cy2: 0.0,
            x: rx,
            y: 0.0,
        },
        PathCommand::CubicTo {
            cx1: rx + K * rx,
            cy1: 0.0,
            cx2: width,
            cy2: ry - K * ry,
            x: width,
            y: ry,
        },
        PathCommand::Close,
    ]
}

fn ps_gradient_supported(grad: &GradientPaint, transform: ColorTransform) -> bool {
    grad.spread == GradientSpread::Extend
        && matches!(grad.kind, GradientKind::Linear | GradientKind::Radial)
        && !grad.stops.is_empty()
        && grad.validate().is_ok()
        && grad
            .stops
            .iter()
            .all(|s| transform.apply(s.color)[3] == 255)
}

fn normalized_shading_stops(
    grad: &GradientPaint,
    transform: ColorTransform,
) -> Vec<(f64, [f64; 3])> {
    let mut raw: Vec<(f64, [f64; 3])> = grad
        .stops
        .iter()
        .map(|s| {
            let c = transform.apply(s.color);
            (
                (f64::from(s.ratio) / 255.0).clamp(0.0, 1.0),
                [
                    f64::from(c[0]) / 255.0,
                    f64::from(c[1]) / 255.0,
                    f64::from(c[2]) / 255.0,
                ],
            )
        })
        .collect();
    if raw.is_empty() {
        return vec![(0.0, [0.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 0.0])];
    }
    raw.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, [f64; 3])> = Vec::with_capacity(raw.len() + 2);
    if raw[0].0 > 1e-6 {
        out.push((0.0, raw[0].1));
    } else {
        raw[0].0 = 0.0;
    }
    for (mut pos, col) in raw {
        if let Some(&(prev_pos, prev_col)) = out.last()
            && (pos - prev_pos).abs() < 1e-5
        {
            if prev_col == col {
                continue;
            }
            let nudged = (prev_pos + 1e-4).min(0.9999);
            if nudged > prev_pos + 1e-7 {
                pos = nudged;
            } else if out.len() >= 2 && out[out.len() - 2].0 < 0.9998 {
                if let Some(last) = out.last_mut() {
                    last.0 = 0.9999;
                }
                pos = 1.0;
            } else if let Some(last) = out.last_mut() {
                last.1 = col;
                continue;
            }
        }
        out.push((pos, col));
    }
    if let Some(&(last_pos, last_col)) = out.last() {
        if last_pos < 1.0 - 1e-6 {
            out.push((1.0, last_col));
        } else if let Some(last) = out.last_mut() {
            last.0 = 1.0;
        }
    }
    if out.len() == 1 {
        let col = out[0].1;
        out.push((1.0, col));
    }
    out
}

fn format_shading_function_dict(stops: &[(f64, [f64; 3])]) -> String {
    let rgb_arr = |c: [f64; 3]| format!("[{} {} {}]", ps_num(c[0]), ps_num(c[1]), ps_num(c[2]));
    if stops.len() <= 2 {
        let c0 = stops.first().map(|s| s.1).unwrap_or([0.0; 3]);
        let c1 = stops.last().map(|s| s.1).unwrap_or(c0);
        return format!(
            "<< /FunctionType 2 /Domain [0 1] /N 1 /C0 {} /C1 {} >>",
            rgb_arr(c0),
            rgb_arr(c1)
        );
    }
    let k = stops.len() - 1;
    let mut fns = String::new();
    let mut bounds = String::new();
    let mut encode = String::new();
    for i in 0..k {
        if i > 0 {
            fns.push(' ');
        }
        let _ = write!(
            fns,
            "<< /FunctionType 2 /Domain [0 1] /N 1 /C0 {} /C1 {} >>",
            rgb_arr(stops[i].1),
            rgb_arr(stops[i + 1].1)
        );
        if i + 1 < k {
            if !bounds.is_empty() {
                bounds.push(' ');
            }
            bounds.push_str(&ps_num(stops[i + 1].0));
        }
        if i > 0 {
            encode.push(' ');
        }
        encode.push_str("0 1");
    }
    format!(
        "<< /FunctionType 3 /Domain [0 1] /Functions [{fns}] /Bounds [{bounds}] /Encode [{encode}] >>"
    )
}

fn format_shading_dict(grad: &GradientPaint, transform: ColorTransform) -> String {
    let stops = normalized_shading_stops(grad, transform);
    let func = format_shading_function_dict(&stops);
    match grad.kind {
        GradientKind::Radial => {
            let fx = grad.focal_point_ratio.clamp(-0.999, 0.999) * 819.2;
            format!(
                "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [{} 0 0 0 0 819.2] /Extend [true true] /Function {} >>",
                ps_num(fx),
                func
            )
        }
        _ => format!(
            "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [-819.2 0 819.2 0] /Extend [true true] /Function {} >>",
            func
        ),
    }
}

fn ps_shfill_clipped(
    out: &mut String,
    commands: &[PathCommand],
    rule: FillRule,
    grad: &GradientPaint,
    transform: ColorTransform,
) {
    out.push_str("gsave\n");
    if ps_path(out, commands) {
        out.push_str(if rule == FillRule::EvenOdd {
            "eoclip\nnewpath\n"
        } else {
            "clip\nnewpath\n"
        });
        let m = grad.matrix;
        let _ = writeln!(
            out,
            "[{} {} {} {} {} {}] concat",
            ps_num(m[0]),
            ps_num(m[1]),
            ps_num(m[2]),
            ps_num(m[3]),
            ps_num(m[4]),
            ps_num(m[5])
        );
        let _ = writeln!(out, "{} shfill", format_shading_dict(grad, transform));
    }
    out.push_str("grestore\n");
}

fn ps_draw_supported(draw: &AuthoredDraw, options: &ImageExportOptions) -> bool {
    let transform = draw.item.color_transform;
    let opaque = |color: Color| matches!(transform.apply(color)[3], 0 | 255);
    if draw.item.bitmap_fill.is_some()
        || !draw.clip_ids.is_empty()
        || !draw.blend_mode.is_normal()
        || (draw.item.alpha - 1.0).abs() > 1e-9
    {
        return false;
    }
    match &draw.item.kind {
        ElementKind::Rectangle { fill, stroke, .. }
        | ElementKind::Ellipse { fill, stroke, .. }
        | ElementKind::Path { fill, stroke, .. } => opaque(*fill) && opaque(*stroke),
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            stroke_width,
            ..
        } => {
            let has_stroke = *stroke_width > 0.0;
            let fg_ok = fill_gradient
                .as_ref()
                .is_none_or(|g| ps_gradient_supported(g, transform));
            let sg_ok = !has_stroke
                || stroke_gradient
                    .as_ref()
                    .is_none_or(|g| ps_gradient_supported(g, transform));
            fg_ok
                && sg_ok
                && (fill_gradient.is_some() || opaque(*fill))
                && (!has_stroke || stroke_gradient.is_some() || opaque(*stroke))
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let outlines = options.ai_text_as_outlines || !text.is_ascii();
            opaque(*color)
                && (!outlines
                    || text.trim().is_empty()
                    || !neoflash_core::TextFieldState::static_text_to_outline_commands(
                        text,
                        font_family,
                        *font_size,
                        None,
                    )
                    .is_empty())
        }
        ElementKind::TextBox { .. } => {
            let Some(field) = neoflash_core::TextFieldState::from_kind(&draw.item.kind) else {
                return false;
            };
            let color_for = |rgb: u32| -> Color {
                [
                    ((rgb >> 16) & 0xff) as u8,
                    ((rgb >> 8) & 0xff) as u8,
                    (rgb & 0xff) as u8,
                    field.alpha,
                ]
            };
            let total_units = field.text.encode_utf16().count() as u32;
            let mat_runs = field.materialized_runs(total_units);
            if !opaque(color_for(field.format.color))
                || !mat_runs.iter().all(|r| opaque(color_for(r.format.color)))
            {
                return false;
            }
            let outlines = options.ai_text_as_outlines || !field.text.is_ascii();
            !outlines || field.text.trim().is_empty() || !textbox_outline_runs(&field).is_empty()
        }
        ElementKind::Instance { .. } => true,
        ElementKind::Bitmap { .. } | ElementKind::Video { .. } => false,
    }
}

fn ps_scene_supported(
    project: &Project,
    scene: &AuthoredScene,
    options: &ImageExportOptions,
) -> bool {
    let filtered: HashSet<&str> = project
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.timeline.layers)
        .flat_map(|layer| &layer.keyframes)
        .flat_map(|keyframe| &keyframe.elements)
        .filter(|element| !element.filters.is_empty())
        .map(|element| element.id.as_str())
        .collect();
    (options.transparent_background || matches!(project.background[3], 0 | 255))
        && scene.draws.iter().all(|draw| {
            !draw
                .item
                .instance_path
                .iter()
                .any(|id| filtered.contains(id.as_str()))
                && ps_draw_supported(draw, options)
        })
}

#[derive(Clone, Copy)]
struct PsShapeStyle<'a> {
    rule: FillRule,
    fill: Color,
    stroke: Color,
    fill_gradient: Option<&'a GradientPaint>,
    stroke_gradient: Option<&'a GradientPaint>,
    stroke_width: f64,
    transform: ColorTransform,
    strokes_as_strokes: bool,
}

fn ps_shape(out: &mut String, commands: &[PathCommand], style: PsShapeStyle<'_>) {
    if let Some(fg) = style.fill_gradient {
        ps_shfill_clipped(out, commands, style.rule, fg, style.transform);
    }
    let solid_fill = if style.fill_gradient.is_some() {
        [0; 4]
    } else {
        style.fill
    };
    let solid_stroke = if style.strokes_as_strokes && style.stroke_gradient.is_none() {
        style.stroke
    } else {
        [0; 4]
    };
    let solid_sw = if style.strokes_as_strokes && style.stroke_gradient.is_none() {
        style.stroke_width
    } else {
        0.0
    };
    if (style.transform.apply(solid_fill)[3] > 0
        || (solid_sw > 0.0 && style.transform.apply(solid_stroke)[3] > 0))
        && ps_path(out, commands)
    {
        ps_paint(
            out,
            style.rule,
            solid_fill,
            solid_stroke,
            solid_sw,
            style.transform,
        );
    }
    if style.stroke_width > 0.0 {
        if let Some(sg) = style.stroke_gradient {
            let outline = neoflash_core::expand_variable_width_stroke(
                commands,
                style.stroke_width,
                &neoflash_core::StrokeWidthProfile::default(),
            );
            ps_shfill_clipped(out, &outline, FillRule::NonZero, sg, style.transform);
        } else if !style.strokes_as_strokes && style.transform.apply(style.stroke)[3] > 0 {
            let outline = neoflash_core::expand_variable_width_stroke(
                commands,
                style.stroke_width,
                &neoflash_core::StrokeWidthProfile::default(),
            );
            if ps_path(out, &outline) {
                ps_paint(
                    out,
                    FillRule::NonZero,
                    style.stroke,
                    [0; 4],
                    0.0,
                    style.transform,
                );
            }
        }
    }
}

fn ps_type1_font_name(family: &str, bold: bool, italic: bool) -> &'static str {
    let lower = family.trim().to_ascii_lowercase();
    let is_mono = lower.contains("mono")
        || lower.contains("courier")
        || lower.contains("menlo")
        || lower.contains("consolas");
    let is_serif = !is_mono
        && !lower.contains("sans")
        && (lower.contains("serif") || lower.contains("times") || lower.contains("georgia"));
    if is_mono {
        match (bold, italic) {
            (true, true) => "Courier-BoldOblique",
            (true, false) => "Courier-Bold",
            (false, true) => "Courier-Oblique",
            (false, false) => "Courier",
        }
    } else if is_serif {
        match (bold, italic) {
            (true, true) => "Times-BoldItalic",
            (true, false) => "Times-Bold",
            (false, true) => "Times-Italic",
            (false, false) => "Times-Roman",
        }
    } else {
        match (bold, italic) {
            (true, true) => "Helvetica-BoldOblique",
            (true, false) => "Helvetica-Bold",
            (false, true) => "Helvetica-Oblique",
            (false, false) => "Helvetica",
        }
    }
}

fn shear_path_italic(commands: &mut [PathCommand], baseline_y: f64) {
    const ITALIC_SLANT: f64 = 0.2;
    let sx = |x: f64, y: f64| x + (baseline_y - y) * ITALIC_SLANT;
    for cmd in commands {
        match cmd {
            PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                *x = sx(*x, *y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                *cx = sx(*cx, *cy);
                *x = sx(*x, *y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                *cx1 = sx(*cx1, *cy1);
                *cx2 = sx(*cx2, *cy2);
                *x = sx(*x, *y);
            }
            PathCommand::Close => {}
        }
    }
}

struct OutlinedTextRun {
    color: Color,
    bold_stroke: f64,
    commands: Vec<PathCommand>,
}

fn textbox_outline_runs(field: &neoflash_core::TextFieldState) -> Vec<OutlinedTextRun> {
    if field.alpha == 0 || field.text.is_empty() {
        return Vec::new();
    }
    let Ok(layout) = field.layout_portable(None) else {
        return Vec::new();
    };
    let mut runs_out: Vec<OutlinedTextRun> = Vec::new();
    for run in &layout.runs {
        let fmt = run.format.as_ref().unwrap_or(&field.format);
        let size = fmt.size.max(1.0);
        let color = [
            ((fmt.color >> 16) & 0xff) as u8,
            ((fmt.color >> 8) & 0xff) as u8,
            (fmt.color & 0xff) as u8,
            field.alpha,
        ];
        let mut cursor_x = run.x;
        let mut cmds = Vec::new();
        let mut measure_fmt = fmt.clone();
        measure_fmt.letter_spacing = 0.0;
        for ch in run.text.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            let mut glyph_cmds = neoflash_core::TextFormat::glyph_outline_commands(
                ch, cursor_x, run.y, &fmt.font, size, None,
            );
            if fmt.italic && !glyph_cmds.is_empty() {
                shear_path_italic(&mut glyph_cmds, run.y + size * 0.8);
            }
            cmds.extend(glyph_cmds);
            let ch_str = ch.to_string();
            let adv = neoflash_core::TextFormat::portable_measure_strings(
                std::slice::from_ref(&ch_str),
                &measure_fmt,
                None,
            )
            .first()
            .copied()
            .unwrap_or(size * 0.5);
            cursor_x += adv + fmt.letter_spacing;
        }
        if !cmds.is_empty() {
            let bold_stroke = if fmt.bold {
                (size * 0.04).max(0.4)
            } else {
                0.0
            };
            runs_out.push(OutlinedTextRun {
                color,
                bold_stroke,
                commands: cmds,
            });
        }
    }
    for dec in &layout.decorations {
        let rgb = dec.color.unwrap_or(field.format.color);
        let color = [
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
            field.alpha,
        ];
        let mut rect = rectangle_path(dec.width.max(0.5), dec.height.max(0.5));
        for cmd in &mut rect {
            match cmd {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    *x += dec.x;
                    *y += dec.y;
                }
                _ => {}
            }
        }
        runs_out.push(OutlinedTextRun {
            color,
            bold_stroke: 0.0,
            commands: rect.to_vec(),
        });
    }
    runs_out
}

fn ps_text(
    out: &mut String,
    text: &str,
    font: &str,
    size: f64,
    color: Color,
    transform: ColorTransform,
    outlines: bool,
) {
    let effective = transform.apply(color);
    if effective[3] == 0 {
        return;
    }
    if outlines || !text.is_ascii() {
        let commands =
            neoflash_core::TextFieldState::static_text_to_outline_commands(text, font, size, None);
        if ps_path(out, &commands) {
            ps_paint(out, FillRule::NonZero, color, [0; 4], 0.0, transform);
        }
    } else {
        let sz = size.max(1.0);
        let ascent = sz * 0.8;
        let line_step = sz * 1.2;
        let ps_font = ps_type1_font_name(font, false, false);
        ps_rgb(out, effective);
        let _ = writeln!(
            out,
            "/{ps_font} findfont [{} 0 0 -{} 0 0] makefont setfont",
            ps_num(sz),
            ps_num(sz)
        );
        for (line_idx, raw_line) in text.split('\n').enumerate() {
            let line = raw_line.trim_end_matches('\r');
            let baseline_y = ascent + (line_idx as f64) * line_step;
            let _ = writeln!(
                out,
                "0 {} moveto ({}) show",
                ps_num(baseline_y),
                ps_string(line)
            );
        }
    }
}

fn ps_draw(out: &mut String, draw: &AuthoredDraw, options: &ImageExportOptions) {
    let item = &draw.item;
    let matrix = item.transform;
    out.push_str("gsave\n");
    let _ = writeln!(
        out,
        "[{} {} {} {} {} {}] concat",
        ps_num(matrix[0]),
        ps_num(matrix[1]),
        ps_num(matrix[2]),
        ps_num(matrix[3]),
        ps_num(matrix[4]),
        ps_num(matrix[5])
    );
    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            ps_shape(
                out,
                &rectangle_path(*width, *height),
                PsShapeStyle {
                    rule: FillRule::NonZero,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: None,
                    stroke_gradient: None,
                    stroke_width: *stroke_width,
                    transform: item.color_transform,
                    strokes_as_strokes: options.ai_strokes_as_strokes,
                },
            );
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            ps_shape(
                out,
                &ellipse_path(*width, *height),
                PsShapeStyle {
                    rule: FillRule::NonZero,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: None,
                    stroke_gradient: None,
                    stroke_width: *stroke_width,
                    transform: item.color_transform,
                    strokes_as_strokes: options.ai_strokes_as_strokes,
                },
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            ps_shape(
                out,
                commands,
                PsShapeStyle {
                    rule: *fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: None,
                    stroke_gradient: None,
                    stroke_width: *stroke_width,
                    transform: item.color_transform,
                    strokes_as_strokes: options.ai_strokes_as_strokes,
                },
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
            ps_shape(
                out,
                commands,
                PsShapeStyle {
                    rule: *fill_rule,
                    fill: *fill,
                    stroke: *stroke,
                    fill_gradient: fill_gradient.as_ref(),
                    stroke_gradient: stroke_gradient.as_ref(),
                    stroke_width: *stroke_width,
                    transform: item.color_transform,
                    strokes_as_strokes: options.ai_strokes_as_strokes,
                },
            );
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            ps_text(
                out,
                text,
                font_family,
                *font_size,
                *color,
                item.color_transform,
                options.ai_text_as_outlines,
            );
        }
        ElementKind::TextBox { .. } => {
            if let Some(field) = neoflash_core::TextFieldState::from_kind(&item.kind) {
                for run in textbox_outline_runs(&field) {
                    ps_shape(
                        out,
                        &run.commands,
                        PsShapeStyle {
                            rule: FillRule::NonZero,
                            fill: run.color,
                            stroke: if run.bold_stroke > 0.0 {
                                run.color
                            } else {
                                [0; 4]
                            },
                            fill_gradient: None,
                            stroke_gradient: None,
                            stroke_width: run.bold_stroke,
                            transform: item.color_transform,
                            strokes_as_strokes: options.ai_strokes_as_strokes,
                        },
                    );
                }
            }
        }
        ElementKind::Instance { .. } | ElementKind::Bitmap { .. } | ElementKind::Video { .. } => {}
    }
    out.push_str("grestore\n");
}

#[allow(clippy::too_many_arguments)]
fn ps_raster_fallback(
    project: &Project,
    symbol_id: &str,
    frame: u32,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    transparent: bool,
    mut out: String,
) -> Result<Vec<u8>, ExportError> {
    let mut cache = HashMap::new();
    let rgba = crate::render_authored_scene_rgba(
        project,
        symbol_id,
        frame,
        scene,
        width,
        height,
        transparent,
        &mut cache,
    )?;
    out.push_str("%%NeoFlashRasterFallback: unsupported vector feature\n");
    let _ = writeln!(out, "gsave\n{width} {height} scale");
    let _ = writeln!(out, "/picstr {} string def", width.saturating_mul(3));
    let _ = writeln!(
        out,
        "{width} {height} 8\n[{width} 0 0 -{height} 0 {height}]"
    );
    out.push_str("{ currentfile picstr readhexstring pop } false 3 colorimage\n");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut line = 0usize;
    for pixel in rgba.as_chunks::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &pixel[..3] {
            let value = ((u16::from(*channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
            out.push(char::from(HEX[(value >> 4) as usize]));
            out.push(char::from(HEX[(value & 15) as usize]));
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
    out.push_str("grestore\nshowpage\n%%EOF\n");
    Ok(out.into_bytes())
}

#[allow(dead_code)]
fn build_ai_bytes(
    project: &Project,
    frame: u32,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
) -> Result<(Vec<u8>, usize, usize), ExportError> {
    let mut eval_project = project.clone();
    if options.svg_include_hidden_guides {
        for symbol in &mut eval_project.symbols {
            for layer in &mut symbol.timeline.layers {
                if layer.properties.role == LayerRole::Guide {
                    layer.visible = true;
                }
            }
        }
    }
    if options.symbol_id.is_none() && !eval_project.scenes.is_empty() {
        eval_project = flatten_scenes_with_ranges(&eval_project).0;
    }
    let target = options
        .symbol_id
        .as_deref()
        .unwrap_or(eval_project.root.as_str());
    let scene = evaluate_scene(
        &eval_project,
        target,
        frame,
        options.svg_include_hidden_guides,
        true,
    );
    build_ai_bytes_with_scene(
        &eval_project,
        &project.name,
        target,
        frame,
        &scene,
        width,
        height,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_ai_bytes_with_scene(
    eval_project: &Project,
    project_name: &str,
    target: &str,
    frame: u32,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
) -> Result<(Vec<u8>, usize, usize), ExportError> {
    let layer_count = eval_project
        .symbol(target)
        .map(|symbol| symbol.timeline.layers.len().max(1))
        .unwrap_or(1);
    let path_count = scene.draws.len();
    let has_gradients = scene.draws.iter().any(|d| {
        matches!(
            &d.item.kind,
            ElementKind::PaintedPath {
                fill_gradient: Some(_),
                ..
            } | ElementKind::PaintedPath {
                stroke_gradient: Some(_),
                ..
            }
        )
    });
    let lang_level = if has_gradients { 3 } else { 2 };
    let mut content = format!(
        "%!PS-Adobe-3.0 EPSF-3.0\n\
         %%Creator: NeoFlash ({})\n\
         %%Title: {}\n\
         %%BoundingBox: 0 0 {} {}\n\
         %%HiResBoundingBox: 0.0 0.0 {}.0 {}.0\n\
         %%LanguageLevel: {}\n\
         %%Pages: 1\n\
         %AI5_FileFormat 12.0\n\
         %%EndComments\n\
         %%Page: 1 1\n",
        ps_comment(&options.ai_version),
        ps_comment(project_name),
        width,
        height,
        width,
        height,
        lang_level,
    );
    if !ps_scene_supported(eval_project, scene, options) {
        let bytes = ps_raster_fallback(
            eval_project,
            target,
            frame,
            scene,
            width,
            height,
            options.transparent_background,
            content,
        )?;
        return Ok((bytes, layer_count, path_count));
    }

    content.push_str("gsave\n");
    let scale_x = f64::from(width) / f64::from(eval_project.width.max(1));
    let scale_y = f64::from(height) / f64::from(eval_project.height.max(1));
    let _ = writeln!(
        content,
        "[{} 0 0 -{} 0 {height}] concat",
        ps_num(scale_x),
        ps_num(scale_y)
    );
    if !options.transparent_background && eval_project.background[3] == 255 {
        ps_path(
            &mut content,
            &rectangle_path(
                f64::from(eval_project.width.max(1)),
                f64::from(eval_project.height.max(1)),
            ),
        );
        ps_paint(
            &mut content,
            FillRule::NonZero,
            eval_project.background,
            [0; 4],
            0.0,
            ColorTransform::default(),
        );
    }

    let layer_names: HashMap<&str, &str> = eval_project
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.timeline.layers)
        .map(|layer| (layer.id.as_str(), layer.name.as_str()))
        .collect();
    let mut current_layer = None;
    for draw in &scene.draws {
        let layer_id = draw.item.layer_id.as_str();
        if options.ai_each_layer && current_layer != Some(layer_id) {
            if current_layer.is_some() {
                content.push_str("%AI5_EndLayer--\n");
            }
            content.push_str("%AI5_BeginLayer\n");
            let name = layer_names.get(layer_id).copied().unwrap_or(layer_id);
            let _ = writeln!(content, "%AI5_Name: ({})", ps_string(name));
            current_layer = Some(layer_id);
        }
        ps_draw(&mut content, draw, options);
    }
    if options.ai_each_layer && current_layer.is_some() {
        content.push_str("%AI5_EndLayer--\n");
    }
    content.push_str("grestore\nshowpage\n%%EOF\n");
    Ok((content.into_bytes(), layer_count, path_count))
}

fn pdf_path(out: &mut String, commands: &[PathCommand]) -> bool {
    if commands.is_empty() {
        return false;
    }
    let mut current = (0.0, 0.0);
    let mut start = current;
    for command in commands {
        match *command {
            PathCommand::MoveTo { x, y } => {
                let _ = writeln!(out, "{} {} m", ps_num(x), ps_num(y));
                current = (x, y);
                start = current;
            }
            PathCommand::LineTo { x, y } => {
                let _ = writeln!(out, "{} {} l", ps_num(x), ps_num(y));
                current = (x, y);
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let c1 = (
                    current.0 + (cx - current.0) * 2.0 / 3.0,
                    current.1 + (cy - current.1) * 2.0 / 3.0,
                );
                let c2 = (x + (cx - x) * 2.0 / 3.0, y + (cy - y) * 2.0 / 3.0);
                let _ = writeln!(
                    out,
                    "{} {} {} {} {} {} c",
                    ps_num(c1.0),
                    ps_num(c1.1),
                    ps_num(c2.0),
                    ps_num(c2.1),
                    ps_num(x),
                    ps_num(y)
                );
                current = (x, y);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let _ = writeln!(
                    out,
                    "{} {} {} {} {} {} c",
                    ps_num(cx1),
                    ps_num(cy1),
                    ps_num(cx2),
                    ps_num(cy2),
                    ps_num(x),
                    ps_num(y)
                );
                current = (x, y);
            }
            PathCommand::Close => {
                out.push_str("h\n");
                current = start;
            }
        }
    }
    true
}

fn pdf_shading_clipped(
    out: &mut String,
    shadings: &mut Vec<String>,
    commands: &[PathCommand],
    rule: FillRule,
    grad: &GradientPaint,
    transform: ColorTransform,
) {
    let sh_idx = shadings.len();
    shadings.push(format_shading_dict(grad, transform));
    out.push_str("q\n");
    if pdf_path(out, commands) {
        out.push_str(if rule == FillRule::EvenOdd {
            "W* n\n"
        } else {
            "W n\n"
        });
        let m = grad.matrix;
        let _ = writeln!(
            out,
            "{} {} {} {} {} {} cm\n/Sh{sh_idx} sh",
            ps_num(m[0]),
            ps_num(m[1]),
            ps_num(m[2]),
            ps_num(m[3]),
            ps_num(m[4]),
            ps_num(m[5])
        );
    }
    out.push_str("Q\n");
}

fn pdf_shape(
    out: &mut String,
    shadings: &mut Vec<String>,
    commands: &[PathCommand],
    style: PsShapeStyle<'_>,
) {
    if let Some(fg) = style.fill_gradient {
        pdf_shading_clipped(out, shadings, commands, style.rule, fg, style.transform);
    } else {
        let fill = style.transform.apply(style.fill);
        if fill[3] > 0 && pdf_path(out, commands) {
            let _ = writeln!(
                out,
                "{} {} {} rg {}",
                ps_num(f64::from(fill[0]) / 255.0),
                ps_num(f64::from(fill[1]) / 255.0),
                ps_num(f64::from(fill[2]) / 255.0),
                if style.rule == FillRule::EvenOdd {
                    "f*"
                } else {
                    "f"
                }
            );
        }
    }

    if style.stroke_width > 0.0 {
        if let Some(sg) = style.stroke_gradient {
            let outline = neoflash_core::expand_variable_width_stroke(
                commands,
                style.stroke_width,
                &neoflash_core::StrokeWidthProfile::default(),
            );
            pdf_shading_clipped(
                out,
                shadings,
                &outline,
                FillRule::NonZero,
                sg,
                style.transform,
            );
        } else {
            let stroke = style.transform.apply(style.stroke);
            if stroke[3] > 0 {
                if style.strokes_as_strokes {
                    if pdf_path(out, commands) {
                        let _ = writeln!(
                            out,
                            "{} {} {} RG {} w 1 J 1 j S",
                            ps_num(f64::from(stroke[0]) / 255.0),
                            ps_num(f64::from(stroke[1]) / 255.0),
                            ps_num(f64::from(stroke[2]) / 255.0),
                            ps_num(style.stroke_width)
                        );
                    }
                } else {
                    let outline = neoflash_core::expand_variable_width_stroke(
                        commands,
                        style.stroke_width,
                        &neoflash_core::StrokeWidthProfile::default(),
                    );
                    if pdf_path(out, &outline) {
                        let _ = writeln!(
                            out,
                            "{} {} {} rg f",
                            ps_num(f64::from(stroke[0]) / 255.0),
                            ps_num(f64::from(stroke[1]) / 255.0),
                            ps_num(f64::from(stroke[2]) / 255.0),
                        );
                    }
                }
            }
        }
    }
}

fn build_vector_pdf_bytes_with_scene(
    eval_project: &Project,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
) -> Vec<u8> {
    let mut stream_body = String::new();
    let mut shadings: Vec<String> = Vec::new();

    stream_body.push_str("q\n");
    let scale_x = f64::from(width) / f64::from(eval_project.width.max(1));
    let scale_y = f64::from(height) / f64::from(eval_project.height.max(1));
    let _ = writeln!(
        stream_body,
        "{} 0 0 -{} 0 {height} cm",
        ps_num(scale_x),
        ps_num(scale_y)
    );
    if !options.transparent_background && eval_project.background[3] == 255 {
        let bg = eval_project.background;
        let _ = writeln!(
            stream_body,
            "{} {} {} rg 0 0 {} {} re f",
            ps_num(f64::from(bg[0]) / 255.0),
            ps_num(f64::from(bg[1]) / 255.0),
            ps_num(f64::from(bg[2]) / 255.0),
            eval_project.width.max(1),
            eval_project.height.max(1)
        );
    }

    for draw in &scene.draws {
        let item = &draw.item;
        let m = item.transform;
        let _ = writeln!(
            stream_body,
            "q\n{} {} {} {} {} {} cm",
            ps_num(m[0]),
            ps_num(m[1]),
            ps_num(m[2]),
            ps_num(m[3]),
            ps_num(m[4]),
            ps_num(m[5])
        );
        match &item.kind {
            ElementKind::Rectangle {
                width,
                height,
                fill,
                stroke,
                stroke_width,
            } => {
                pdf_shape(
                    &mut stream_body,
                    &mut shadings,
                    &rectangle_path(*width, *height),
                    PsShapeStyle {
                        rule: FillRule::NonZero,
                        fill: *fill,
                        stroke: *stroke,
                        fill_gradient: None,
                        stroke_gradient: None,
                        stroke_width: *stroke_width,
                        transform: item.color_transform,
                        strokes_as_strokes: options.ai_strokes_as_strokes,
                    },
                );
            }
            ElementKind::Ellipse {
                width,
                height,
                fill,
                stroke,
                stroke_width,
            } => {
                pdf_shape(
                    &mut stream_body,
                    &mut shadings,
                    &ellipse_path(*width, *height),
                    PsShapeStyle {
                        rule: FillRule::NonZero,
                        fill: *fill,
                        stroke: *stroke,
                        fill_gradient: None,
                        stroke_gradient: None,
                        stroke_width: *stroke_width,
                        transform: item.color_transform,
                        strokes_as_strokes: options.ai_strokes_as_strokes,
                    },
                );
            }
            ElementKind::Path {
                commands,
                fill_rule,
                fill,
                stroke,
                stroke_width,
            } => {
                pdf_shape(
                    &mut stream_body,
                    &mut shadings,
                    commands,
                    PsShapeStyle {
                        rule: *fill_rule,
                        fill: *fill,
                        stroke: *stroke,
                        fill_gradient: None,
                        stroke_gradient: None,
                        stroke_width: *stroke_width,
                        transform: item.color_transform,
                        strokes_as_strokes: options.ai_strokes_as_strokes,
                    },
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
                pdf_shape(
                    &mut stream_body,
                    &mut shadings,
                    commands,
                    PsShapeStyle {
                        rule: *fill_rule,
                        fill: *fill,
                        stroke: *stroke,
                        fill_gradient: fill_gradient.as_ref(),
                        stroke_gradient: stroke_gradient.as_ref(),
                        stroke_width: *stroke_width,
                        transform: item.color_transform,
                        strokes_as_strokes: options.ai_strokes_as_strokes,
                    },
                );
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => {
                let cmds = neoflash_core::TextFieldState::static_text_to_outline_commands(
                    text,
                    font_family,
                    *font_size,
                    None,
                );
                pdf_shape(
                    &mut stream_body,
                    &mut shadings,
                    &cmds,
                    PsShapeStyle {
                        rule: FillRule::NonZero,
                        fill: *color,
                        stroke: [0; 4],
                        fill_gradient: None,
                        stroke_gradient: None,
                        stroke_width: 0.0,
                        transform: item.color_transform,
                        strokes_as_strokes: true,
                    },
                );
            }
            ElementKind::TextBox { .. } => {
                if let Some(field) = neoflash_core::TextFieldState::from_kind(&item.kind) {
                    for run in textbox_outline_runs(&field) {
                        pdf_shape(
                            &mut stream_body,
                            &mut shadings,
                            &run.commands,
                            PsShapeStyle {
                                rule: FillRule::NonZero,
                                fill: run.color,
                                stroke: if run.bold_stroke > 0.0 {
                                    run.color
                                } else {
                                    [0; 4]
                                },
                                fill_gradient: None,
                                stroke_gradient: None,
                                stroke_width: run.bold_stroke,
                                transform: item.color_transform,
                                strokes_as_strokes: true,
                            },
                        );
                    }
                }
            }
            ElementKind::Instance { .. }
            | ElementKind::Bitmap { .. }
            | ElementKind::Video { .. } => {}
        }
        stream_body.push_str("Q\n");
    }
    stream_body.push_str("Q\n");

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let total_objs = 5 + shadings.len();
    let mut offsets = vec![0usize; total_objs];

    offsets[1] = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets[2] = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets[3] = pdf.len();
    let shading_res = if shadings.is_empty() {
        String::new()
    } else {
        let mut entries = String::new();
        for idx in 0..shadings.len() {
            if idx > 0 {
                entries.push(' ');
            }
            let _ = write!(entries, "/Sh{idx} {} 0 R", 5 + idx);
        }
        format!("/Shading << {entries} >> ")
    };
    let page_obj = format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
         /Resources << {shading_res}>> /Contents 4 0 R >>\nendobj\n"
    );
    pdf.extend_from_slice(page_obj.as_bytes());

    offsets[4] = pdf.len();
    let content_obj = format!(
        "4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
        stream_body.len(),
        stream_body
    );
    pdf.extend_from_slice(content_obj.as_bytes());

    for (idx, sh_dict) in shadings.iter().enumerate() {
        let obj_num = 5 + idx;
        offsets[obj_num] = pdf.len();
        let sh_obj = format!("{obj_num} 0 obj\n{sh_dict}\nendobj\n");
        pdf.extend_from_slice(sh_obj.as_bytes());
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {total_objs}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets[1..total_objs] {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    let trailer =
        format!("trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n");
    pdf.extend_from_slice(trailer.as_bytes());
    pdf
}

fn build_pdf_bytes_from_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ExportError> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for px in rgba.as_chunks::<4>().0 {
        let a = u16::from(px[3]);
        let blend = |ch: u8| -> u8 { ((u16::from(ch) * a + 255 * (255 - a)) / 255) as u8 };
        rgb.push(blend(px[0]));
        rgb.push(blend(px[1]));
        rgb.push(blend(px[2]));
    }

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let mut offsets = [0usize; 6];
    // Obj 1: Catalog
    offsets[1] = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // Obj 2: Pages
    offsets[2] = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    // Obj 3: Page
    offsets[3] = pdf.len();
    let page_obj = format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
         /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>\nendobj\n"
    );
    pdf.extend_from_slice(page_obj.as_bytes());

    // Obj 4: Image XObject
    offsets[4] = pdf.len();
    let img_header = format!(
        "4 0 obj\n<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n",
        rgb.len()
    );
    pdf.extend_from_slice(img_header.as_bytes());
    pdf.extend_from_slice(&rgb);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");

    // Obj 5: Content stream
    offsets[5] = pdf.len();
    let stream_body = format!("q\n{width} 0 0 {height} 0 0 cm\n/Im0 Do\nQ\n");
    let content_obj = format!(
        "5 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
        stream_body.len(),
        stream_body
    );
    pdf.extend_from_slice(content_obj.as_bytes());

    let xref_offset = pdf.len();
    pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets[1..=5] {
        let line = format!("{off:010} 00000 n \n");
        pdf.extend_from_slice(line.as_bytes());
    }
    let trailer = format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n");
    pdf.extend_from_slice(trailer.as_bytes());
    Ok(pdf)
}

/// Export a project frame or frame range to PNG, JPEG, WebP, GIF, BMP, SVG, PSD, AI, or PDF (`DL-04`).
pub fn export_project_image(
    project: &Project,
    path: &Path,
    options: &ImageExportOptions,
) -> Result<ImageExportReport, ExportError> {
    export_project_image_with_progress(project, path, options, |_, _| Ok(()))
}

/// Export a project frame or frame range with a per-frame callback (`frame_1_based`, `total_frames`)
/// invoked after each frame is staged, allowing cancellation or late-frame fault injection.
pub fn export_project_image_with_progress<F>(
    project: &Project,
    path: &Path,
    options: &ImageExportOptions,
    mut on_frame: F,
) -> Result<ImageExportReport, ExportError>
where
    F: FnMut(u32, u32) -> Result<(), ExportError>,
{
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;

    let (width, height) = options.resolved_dimensions(project);
    let (start_frame, end_frame) = options
        .range
        .resolve_frames(project, options.symbol_id.as_deref());
    let total_frames = end_frame
        .saturating_sub(start_frame)
        .saturating_add(1)
        .max(1);

    let ext = options.format.extension();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let is_sequence_export =
        total_frames > 1 || !matches!(options.range, ImageExportRange::CurrentFrame(_));

    let mut single_created_dirs = Vec::new();
    let mut sequence_tx = if is_sequence_export {
        let stem = sequence_output_stem(path);
        let planned_names: Vec<String> = (0..total_frames)
            .map(|offset| {
                numbered_output_path(path, offset, total_frames, ext, true)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        ExportError::Options(format!(
                            "Invalid sequence output path '{}'.",
                            path.display()
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let tx = SequenceSetTransaction::begin_for_family(parent, stem, ext)?;
        tx.preflight(&planned_names)?;
        Some((tx, planned_names))
    } else {
        single_created_dirs = ensure_directory_chain_tracked(parent)?;
        None
    };

    let result = (|| -> Result<ImageExportReport, ExportError> {
        let mut files = Vec::with_capacity(total_frames as usize);
        let mut total_bytes = 0usize;
        let mut layer_count = 1usize;
        let mut path_count = 0usize;

        let mut eval_proj = project.clone();
        let include_guides = matches!(
            options.format,
            ImageExportFormat::Svg | ImageExportFormat::Ai
        ) && options.svg_include_hidden_guides;
        if include_guides {
            for sym in &mut eval_proj.symbols {
                for layer in &mut sym.timeline.layers {
                    if layer.properties.role == LayerRole::Guide {
                        layer.visible = true;
                    }
                }
            }
        }
        if options.symbol_id.is_none() && !eval_proj.scenes.is_empty() {
            eval_proj = flatten_scenes_with_ranges(&eval_proj).0;
        }
        let target_symbol = options
            .symbol_id
            .clone()
            .unwrap_or_else(|| eval_proj.root.clone());
        let has_timeline_video = eval_proj
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .any(|k| {
                k.video.is_some()
                    || k.elements
                        .iter()
                        .any(|e| matches!(e.kind, ElementKind::Video { .. }))
            });
        let mut playback = options.runtime_capture.then(|| {
            let mut p = neoflash_core::AuthoredPlayback::with_live_scripts(
                &eval_proj,
                &target_symbol,
                0,
                true,
            );
            for _ in 0..start_frame {
                p.tick(&eval_proj);
            }
            p
        });
        let mut bitmap_cache = HashMap::new();
        let needs_raster_video_session = has_timeline_video
            && (matches!(
                options.format,
                ImageExportFormat::Png
                    | ImageExportFormat::Jpeg
                    | ImageExportFormat::Webp
                    | ImageExportFormat::Gif
                    | ImageExportFormat::Bmp
                    | ImageExportFormat::Pdf
            ) || (options.format == ImageExportFormat::Psd && options.runtime_capture));
        let mut video_session = if needs_raster_video_session {
            Some(crate::video::SequentialVideoFrameSession::new(
                project,
                options.symbol_id.as_deref(),
                start_frame,
                width,
                height,
                options.transparent_background,
                options.runtime_capture,
                None,
            )?)
        } else {
            None
        };

        for offset in 0..total_frames {
            let frame = start_frame + offset;
            if offset > 0
                && let Some(p) = &mut playback
            {
                p.tick(&eval_proj);
            }
            let stepped_scene = if let Some(p) = &playback {
                p.scene(&eval_proj, include_guides, true)
            } else {
                evaluate_scene(&eval_proj, &target_symbol, frame, include_guides, true)
            };
            let mut render_rgba =
                |bitmap_cache: &mut HashMap<String, _>| -> Result<Vec<u8>, ExportError> {
                    if let Some(sess) = &mut video_session {
                        sess.render_frame(frame)
                    } else {
                        render_authored_scene_rgba(
                            &eval_proj,
                            &target_symbol,
                            frame,
                            &stepped_scene,
                            width,
                            height,
                            options.transparent_background,
                            bitmap_cache,
                        )
                    }
                };

            let out_path =
                numbered_output_path(path, offset, total_frames, ext, is_sequence_export);
            let bytes = match options.format {
                ImageExportFormat::Png => {
                    let rgba = render_rgba(&mut bitmap_cache)?;
                    encode_png_bytes(&rgba, width, height)?
                }
                ImageExportFormat::Jpeg => {
                    let rgba = render_rgba(&mut bitmap_cache)?;
                    encode_jpeg_bytes(
                        &rgba,
                        width,
                        height,
                        options.jpeg_quality,
                        eval_proj.background,
                    )?
                }
                ImageExportFormat::Webp => {
                    let rgba = render_rgba(&mut bitmap_cache)?;
                    encode_webp_bytes(&rgba, width, height)?
                }
                ImageExportFormat::Gif => {
                    let rgba = render_rgba(&mut bitmap_cache)?;
                    encode_gif_bytes(&rgba, width, height)?
                }
                ImageExportFormat::Bmp => {
                    let rgba = render_rgba(&mut bitmap_cache)?;
                    encode_bmp_bytes(&rgba, width, height)?
                }
                ImageExportFormat::Svg => {
                    let svg_opts = SvgOptions {
                        symbol_id: options.symbol_id.clone(),
                        frame,
                        end_frame: None,
                        width: Some(width),
                        height: Some(height),
                        transparent_background: options.transparent_background,
                        include_guides: options.svg_include_hidden_guides,
                        text_as_outlines: options.svg_text_as_outlines,
                        precision: options.svg_precision.clamp(1, 6),
                    };
                    let (mut markup, elems, _) =
                        render_scene_svg(&eval_proj, &stepped_scene, &svg_opts)?;
                    path_count += elems;
                    let prefix = safe_svg_id_prefix(&options.svg_id_prefix);
                    if prefix != "nf-" {
                        markup = markup.replace("id=\"nf-", &format!("id=\"{prefix}"));
                        markup = markup
                            .replace("url(#nf-", &format!("url(#{prefix}"))
                            .replace("href=\"#nf-", &format!("href=\"#{prefix}"));
                    }
                    if options.svg_minify {
                        markup = markup
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<_>>()
                            .join("");
                    }
                    markup.into_bytes()
                }
                ImageExportFormat::Psd => {
                    let pre_composite = options
                        .runtime_capture
                        .then(|| render_rgba(&mut bitmap_cache))
                        .transpose()?;
                    let (psd, layers) = build_psd_bytes_with_composite(
                        project,
                        options.symbol_id.as_deref(),
                        frame,
                        width,
                        height,
                        options,
                        pre_composite.as_deref(),
                    )?;
                    layer_count = layers;
                    psd
                }
                ImageExportFormat::Ai => {
                    let (ai, layers, paths) = build_ai_bytes_with_scene(
                        &eval_proj,
                        &project.name,
                        &target_symbol,
                        frame,
                        &stepped_scene,
                        width,
                        height,
                        options,
                    )?;
                    layer_count = layers;
                    path_count += paths;
                    ai
                }
                ImageExportFormat::Pdf => {
                    if !has_timeline_video
                        && ps_scene_supported(&eval_proj, &stepped_scene, options)
                    {
                        path_count += stepped_scene.draws.len();
                        build_vector_pdf_bytes_with_scene(
                            &eval_proj,
                            &stepped_scene,
                            width,
                            height,
                            options,
                        )
                    } else {
                        let rgba = render_rgba(&mut bitmap_cache)?;
                        build_pdf_bytes_from_rgba(&rgba, width, height)?
                    }
                }
            };
            total_bytes += bytes.len();
            if let Some((tx, planned_names)) = &mut sequence_tx {
                tx.stage_file(&planned_names[offset as usize], &bytes)?;
                on_frame(offset + 1, total_frames)?;
            } else {
                on_frame(offset + 1, total_frames)?;
                atomic_write(&out_path, &bytes)?;
                files.push(out_path);
            }
        }

        if let Some((tx, _)) = sequence_tx.take() {
            files = tx.commit()?;
        }

        Ok(ImageExportReport {
            format: options.format,
            files,
            width,
            height,
            frame_count: total_frames,
            total_bytes,
            layer_count,
            path_count,
        })
    })();

    if result.is_err() {
        drop(sequence_tx);
        remove_empty_dirs_reverse(&single_created_dirs);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SEQUENCE_STAGE_PREFIX;
    use neoflash_core::{
        Element, ElementKind, Keyframe, Layer, LayerProperties, ResourceUnit, Symbol,
        TimelineVideoSpan, Transform, new_id,
    };
    use std::fs;

    #[test]
    fn current_scene_range_uses_zero_based_inclusive_scene_bounds() {
        let mut project = Project::blank();
        let root_id = project.root.clone();
        project.symbol_mut(&root_id).unwrap().timeline.frame_count = 3;
        let mut second_scene: Symbol = project.symbol(&root_id).unwrap().clone();
        second_scene.id = new_id("scene");
        second_scene.name = "Second scene".to_owned();
        second_scene.timeline.frame_count = 2;
        project.symbols.push(second_scene.clone());
        project.scenes = vec![root_id.clone(), second_scene.id.clone()];

        assert_eq!(
            ImageExportRange::CurrentScene { scene_id: root_id }.resolve_frames(&project, None),
            (0, 2)
        );
        assert_eq!(
            ImageExportRange::CurrentScene {
                scene_id: second_scene.id.clone(),
            }
            .resolve_frames(&project, None),
            (3, 4)
        );
        assert_eq!(
            ImageExportRange::CurrentScene {
                scene_id: second_scene.id.clone(),
            }
            .resolve_frames(&project, Some(&second_scene.id)),
            (0, 1)
        );
    }

    #[test]
    fn exports_png_svg_psd_ai_and_pdf_images() {
        let project = Project::demo();
        let dir =
            std::env::temp_dir().join(format!("nf2-image-export-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");

        for format in ImageExportFormat::ALL {
            let mut opts = ImageExportOptions::for_project(&project, 0);
            opts.format = format;
            opts.width = 320;
            opts.height = 200;
            let out_path = dir.join(format!("sample.{}", format.extension()));
            let report = export_project_image(&project, &out_path, &opts)
                .unwrap_or_else(|e| panic!("{format:?} export failed: {e}"));
            assert_eq!(report.format, format);
            assert_eq!(report.files.len(), 1);
            assert!(report.files[0].exists());
            assert!(report.total_bytes > 64);

            let bytes = fs::read(&report.files[0]).expect("read exported file");
            match format {
                ImageExportFormat::Png => assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
                ImageExportFormat::Svg => {
                    let s = String::from_utf8_lossy(&bytes);
                    assert!(s.contains("<svg"));
                }
                ImageExportFormat::Psd => assert!(bytes.starts_with(b"8BPS")),
                ImageExportFormat::Ai => {
                    let postscript = String::from_utf8_lossy(&bytes);
                    assert!(postscript.starts_with("%!PS-Adobe-3.0 EPSF-3.0"));
                    assert!(postscript.ends_with("%%EOF\n"));
                    assert!(!postscript.contains("<svg"));
                    assert!(postscript.contains("moveto") || postscript.contains("colorimage"));
                }
                ImageExportFormat::Pdf => {
                    assert!(bytes.starts_with(b"%PDF-1.4"));
                    assert!(bytes.ends_with(b"%%EOF\n"));
                }
                ImageExportFormat::Jpeg => assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF])),
                ImageExportFormat::Webp => assert!(bytes.starts_with(b"RIFF")),
                ImageExportFormat::Gif => assert!(bytes.starts_with(b"GIF8")),
                ImageExportFormat::Bmp => assert!(bytes.starts_with(b"BM")),
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn layered_psd_streaming_preserves_records_and_rejects_oversized_output() {
        fn solid_layer(name: &str, color: [u8; 4]) -> Layer {
            Layer {
                id: new_id("layer"),
                name: name.to_owned(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: new_id("element"),
                        name: name.to_owned(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 16.0,
                            height: 8.0,
                            fill: color,
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: LayerProperties::default(),
            }
        }

        let mut project = Project::blank();
        project.width = 16;
        project.height = 8;
        let root_id = project.root.clone();
        project.symbol_mut(&root_id).unwrap().timeline.layers = vec![
            solid_layer("Top", [0, 255, 0, 255]),
            solid_layer("Bottom", [255, 0, 0, 255]),
        ];
        let mut options = ImageExportOptions::for_project(&project, 0);
        options.psd_keep_layers = true;
        options.transparent_background = true;

        let (bytes, layer_count) =
            build_psd_bytes(&project, None, 0, 16, 8, &options).expect("stream layered PSD");
        let (header, layers, composite) =
            neoflash_import::parse_psd_document(&bytes).expect("parse streamed PSD");
        assert_eq!((header.width, header.height, header.version), (16, 8, 1));
        assert_eq!(layer_count, 2);
        assert_eq!(
            layers
                .iter()
                .map(|layer| layer.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Bottom", "Top"],
            "PSD records keep their established bottom-to-top order"
        );
        assert!(layers.iter().all(|layer| {
            layer
                .rgba_pixels
                .as_ref()
                .is_some_and(|pixels| pixels.len() == 16 * 8 * 4)
        }));
        assert_eq!(composite.as_ref().map(Vec::len), Some(16 * 8 * 4));

        let mut scene_project = Project::blank();
        scene_project.width = 16;
        scene_project.height = 8;
        let scene_root = scene_project.root.clone();
        {
            let root = scene_project.symbol_mut(&scene_root).unwrap();
            root.name = "Scene One".into();
            root.timeline.frame_count = 1;
            root.timeline.layers = vec![solid_layer("Scene One layer", [255, 0, 0, 255])];
        }
        let mut second_scene: Symbol = scene_project.symbol(&scene_root).unwrap().clone();
        second_scene.id = new_id("scene");
        second_scene.name = "Scene Two".into();
        second_scene.timeline.layers = vec![solid_layer("Scene Two layer", [0, 0, 255, 255])];
        let second_scene_id = second_scene.id.clone();
        scene_project.symbols.push(second_scene);
        scene_project.scenes = vec![scene_root, second_scene_id];
        scene_project.format_version = 6;

        let (scene_bytes, _) = build_psd_bytes(&scene_project, None, 1, 16, 8, &options)
            .expect("stream flattened second-scene PSD");
        let (_, scene_layers, _) = neoflash_import::parse_psd_document(&scene_bytes)
            .expect("parse second-scene layered PSD");
        let second_pixels = scene_layers
            .iter()
            .find(|layer| layer.name == "Scene Two layer")
            .and_then(|layer| layer.rgba_pixels.as_ref())
            .expect("flattened second-scene layer record");
        let center = (4 * 16 + 8) * 4;
        assert_eq!(
            &second_pixels[center..center + 4],
            &[0, 0, 255, 255],
            "whole-movie PSD layers sample the flattened global scene frame"
        );

        options.psd_keep_layers = false;
        let error = build_psd_bytes(&project, None, 0, 8192, 8192, &options)
            .expect_err("oversized PSD is rejected before allocating a frame");
        assert!(error.to_string().contains("bounded export limit"));
    }

    #[test]
    fn ai_export_emits_vector_eps_and_falls_back_for_inherited_alpha() {
        let mut project = Project::blank();
        let root_id = project.root.clone();
        let element = Element {
            id: new_id("element"),
            name: "EPS rectangle".to_owned(),
            transform: Transform {
                x: 7.0,
                y: 9.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0, 0, 0, 255],
                stroke_width: 2.0,
            },
            ..Element::default()
        };
        project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements =
            vec![element];

        let mut options = ImageExportOptions::for_project(&project, 0);
        options.transparent_background = true;
        let (vector, _, _) = build_ai_bytes(&project, 0, 320, 200, &options).unwrap();
        let vector = String::from_utf8(vector).unwrap();
        assert!(!vector.contains("NeoFlashRasterFallback"));
        assert!(vector.contains("[1 0 0 1 7 9] concat"));
        assert!(vector.contains("lineto"));

        options.ai_strokes_as_strokes = false;
        let (outlined, _, _) = build_ai_bytes(&project, 0, 320, 200, &options).unwrap();
        let outlined = String::from_utf8(outlined).unwrap();
        assert!(!outlined.contains("NeoFlashRasterFallback"));
        assert!(!outlined.contains("setlinewidth"));

        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.layers[0].keyframes[0].elements[0]
            .transform
            .alpha = 0.5;
        let (alpha, _, _) = build_ai_bytes(&project, 0, 320, 200, &options).unwrap();
        let alpha = String::from_utf8(alpha).unwrap();
        assert!(alpha.contains("NeoFlashRasterFallback"));
        assert!(alpha.contains("colorimage"));
        assert!(!alpha.contains("<svg"));
    }

    #[test]
    fn svg_image_export_includes_only_guides_and_sanitizes_custom_id_prefix() {
        fn rectangle_layer(name: &str, role: LayerRole, color: [u8; 4]) -> Layer {
            Layer {
                id: new_id("layer"),
                name: name.to_owned(),
                visible: false,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: new_id("element"),
                        name: name.to_owned(),
                        transform: Transform::default(),
                        kind: ElementKind::Rectangle {
                            width: 20.0,
                            height: 20.0,
                            fill: color,
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
                properties: LayerProperties {
                    role,
                    ..LayerProperties::default()
                },
            }
        }

        let mut project = Project::blank();
        let root_id = project.root.clone();
        project.symbol_mut(&root_id).unwrap().timeline.layers = vec![
            rectangle_layer("hidden artwork", LayerRole::Normal, [255, 0, 0, 255]),
            rectangle_layer("hidden guide", LayerRole::Guide, [0, 255, 0, 255]),
        ];
        project.format_version = 4;

        let dir =
            std::env::temp_dir().join(format!("nf2-svg-guide-export-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut options = ImageExportOptions::for_project(&project, 0);
        options.format = ImageExportFormat::Svg;
        options.transparent_background = true;
        options.svg_include_hidden_guides = true;
        options.svg_id_prefix = "9 unsafe\" onload=\"alert(1)".to_owned();
        let report = export_project_image(&project, &dir.join("guides.svg"), &options)
            .expect("SVG guide export");
        let svg = fs::read_to_string(&report.files[0]).expect("read SVG guide export");
        assert!(
            svg.contains("#00ff00"),
            "requested hidden guide was omitted"
        );
        assert!(
            !svg.contains("#ff0000"),
            "including guides must not reveal ordinary hidden artwork"
        );
        assert!(!svg.contains("onload=\"alert"));
        assert_eq!(safe_svg_id_prefix("9 unsafe\"id"), "nf-9_unsafe_id");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ai_frame_range_is_transactional_when_later_video_decode_fails() {
        let mut project = Project::blank();
        project.width = 32;
        project.height = 32;
        project.resources.push(
            ResourceUnit::from_bytes("bad", "bad.mp4", "video/mp4", b"not-a-valid-mp4").unwrap(),
        );
        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 2;
        root.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "safe".into(),
                    kind: ElementKind::Rectangle {
                        width: 16.0,
                        height: 16.0,
                        fill: [0, 120, 255, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![Element {
                    id: "video".into(),
                    kind: ElementKind::Video {
                        width: 32.0,
                        height: 32.0,
                    },
                    ..Element::default()
                }],
                video: Some(TimelineVideoSpan::embedded("bad")),
                ..Keyframe::default()
            },
        ];
        project.format_version = 5;
        project.upgrade_format_for_features();

        let dir = std::env::temp_dir().join(format!(
            "neoflash-ai-range-transaction-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let base = dir.join("range.ai");
        let mut options = ImageExportOptions::for_project(&project, 0);
        options.format = ImageExportFormat::Ai;
        options.range = ImageExportRange::Frames { start: 0, end: 1 };
        let error = export_project_image(&project, &base, &options)
            .expect_err("later corrupt video must fail the complete AI range");
        assert!(error.to_string().contains("bad.mp4"), "got: {error}");
        assert!(!dir.join("range_0001.ai").exists());
        assert!(!dir.join("range_0002.ai").exists());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(SEQUENCE_STAGE_PREFIX))
        }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ai_range_publish_restores_existing_file_when_later_target_fails() {
        let mut project = Project::blank();
        project.width = 16;
        project.height = 16;
        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 2;
        root.timeline.layers[0].keyframes[0].elements = vec![Element {
            id: "shape".into(),
            kind: ElementKind::Rectangle {
                width: 8.0,
                height: 8.0,
                fill: [10, 20, 30, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..Element::default()
        }];

        let dir =
            std::env::temp_dir().join(format!("neoflash-ai-range-rollback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("range_0001.ai");
        let second = dir.join("range_0002.ai");

        // 1. Unowned pre-existing file is refused cleanly before any mutation.
        let unowned = b"pre-existing unowned AI file";
        fs::write(&first, unowned).unwrap();
        let mut options = ImageExportOptions::for_project(&project, 0);
        options.format = ImageExportFormat::Ai;
        options.range = ImageExportRange::Frames { start: 0, end: 1 };
        let unowned_err = export_project_image(&project, &dir.join("range.ai"), &options)
            .expect_err("unowned range_0001.ai must be refused by preflight");
        assert!(
            matches!(unowned_err, ExportError::Destination(_)),
            "got: {unowned_err}"
        );
        assert_eq!(fs::read(&first).unwrap(), unowned);
        assert!(!second.exists());
        fs::remove_file(&first).unwrap();

        // 2. Owned prior sequence is preserved byte-for-byte when a blocking directory appears at a later frame.
        export_project_image(&project, &dir.join("range.ai"), &options)
            .expect("initial two-frame AI sequence succeeds");
        let owned_first = fs::read(&first).unwrap();
        let owned_second = fs::read(&second).unwrap();

        project.symbol_mut(&root_id).unwrap().timeline.frame_count = 3;
        let third = dir.join("range_0003.ai");
        fs::create_dir(&third).unwrap();
        options.range = ImageExportRange::Frames { start: 0, end: 2 };
        let error = export_project_image(&project, &dir.join("range.ai"), &options)
            .expect_err("directory at the third target must fail the range transaction");
        assert!(
            error.to_string().contains("directory") || error.to_string().contains("Directory"),
            "got: {error}"
        );
        assert_eq!(fs::read(&first).unwrap(), owned_first);
        assert_eq!(fs::read(&second).unwrap(), owned_second);
        assert!(third.is_dir());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(SEQUENCE_STAGE_PREFIX))
        }));

        fs::remove_dir(&third).unwrap();
        let report = export_project_image(&project, &dir.join("range.ai"), &options)
            .expect("three-frame range publishes after the blocking directory is removed");
        assert_eq!(
            report.files,
            vec![first.clone(), second.clone(), third.clone()]
        );
        assert!(first.is_file() && second.is_file() && third.is_file());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(SEQUENCE_STAGE_PREFIX))
        }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ai_range_staging_enforces_checked_byte_and_file_budgets() {
        let dir =
            std::env::temp_dir().join(format!("neoflash-ai-range-budget-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        {
            let mut transaction =
                SequenceSetTransaction::begin_with_limits(&dir, "a", 4, 2).unwrap();
            transaction.stage_file("a_0001.ai", b"1234").unwrap();
            let error = transaction
                .stage_file("a_0002.ai", b"5")
                .expect_err("aggregate staging bytes must be bounded");
            assert!(error.to_string().contains("staging limit"));
        }
        {
            let mut transaction =
                SequenceSetTransaction::begin_with_limits(&dir, "a", 100, 1).unwrap();
            transaction.stage_file("a_0001.ai", b"1").unwrap();
            let error = transaction
                .stage_file("a_0002.ai", b"2")
                .expect_err("staged file count must be bounded");
            assert!(error.to_string().contains("staging limit"));
        }
        assert!(!dir.join("a_0001.ai").exists());
        assert!(!dir.join("a_0002.ai").exists());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(SEQUENCE_STAGE_PREFIX))
        }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ai_and_pdf_export_preserve_linear_and_radial_vector_shadings_and_round_trip() {
        use neoflash_core::GradientStop;

        let mut project = Project::blank();
        project.width = 200;
        project.height = 120;
        let root_id = project.root.clone();

        let linear_paint = GradientPaint {
            kind: GradientKind::Linear,
            matrix: [100.0 / 1638.4, 0.0, 0.0, 100.0 / 1638.4, 60.0, 40.0],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    ratio: 128,
                    color: [0, 255, 0, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [0, 0, 255, 255],
                },
            ],
            ..GradientPaint::default()
        };
        let radial_paint = GradientPaint {
            kind: GradientKind::Radial,
            matrix: [30.0 / 819.2, 0.0, 0.0, 30.0 / 819.2, 150.0, 60.0],
            stops: vec![
                GradientStop {
                    ratio: 0,
                    color: [255, 255, 0, 255],
                },
                GradientStop {
                    ratio: 255,
                    color: [128, 0, 255, 255],
                },
            ],
            focal_point_ratio: 0.35,
            ..GradientPaint::default()
        };

        project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: new_id("element"),
                name: "Linear box".into(),
                transform: Transform::default(),
                kind: ElementKind::PaintedPath {
                    commands: rectangle_path(100.0, 60.0)
                        .into_iter()
                        .map(|cmd| match cmd {
                            PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
                                x: x + 10.0,
                                y: y + 10.0,
                            },
                            PathCommand::LineTo { x, y } => PathCommand::LineTo {
                                x: x + 10.0,
                                y: y + 10.0,
                            },
                            other => other,
                        })
                        .collect(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [0; 4],
                    fill_gradient: Some(linear_paint),
                    stroke_gradient: None,
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
            Element {
                id: new_id("element"),
                name: "Radial orb".into(),
                transform: Transform::default(),
                kind: ElementKind::PaintedPath {
                    commands: rectangle_path(60.0, 60.0)
                        .into_iter()
                        .map(|cmd| match cmd {
                            PathCommand::MoveTo { x, y } => PathCommand::MoveTo {
                                x: x + 120.0,
                                y: y + 30.0,
                            },
                            PathCommand::LineTo { x, y } => PathCommand::LineTo {
                                x: x + 120.0,
                                y: y + 30.0,
                            },
                            other => other,
                        })
                        .collect(),
                    fill_rule: FillRule::NonZero,
                    fill: [0; 4],
                    stroke: [0; 4],
                    fill_gradient: Some(radial_paint),
                    stroke_gradient: None,
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
        ];
        project.format_version = 5;
        project.upgrade_format_for_features();

        let dir = std::env::temp_dir().join(format!(
            "neoflash-shading-export-roundtrip-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // 1) AI / EPS vector shading (`shfill`, LanguageLevel 3)
        let mut ai_opts = ImageExportOptions::for_project(&project, 0);
        ai_opts.format = ImageExportFormat::Ai;
        ai_opts.transparent_background = true;
        let ai_report = export_project_image(&project, &dir.join("shadings.ai"), &ai_opts).unwrap();
        let ai_text = fs::read_to_string(&ai_report.files[0]).unwrap();
        assert!(!ai_text.contains("NeoFlashRasterFallback"));
        assert!(ai_text.contains("%%LanguageLevel: 3"));
        assert!(ai_text.contains("/ShadingType 2"));
        assert!(ai_text.contains("/ShadingType 3"));
        assert!(ai_text.contains("/FunctionType 3"));
        assert!(ai_text.contains("shfill"));

        // 2) PDF vector shading (`/ShadingType 2`, `/ShadingType 3`, `/FunctionType 3`, `sh`) + round-trip import
        let mut pdf_opts = ImageExportOptions::for_project(&project, 0);
        pdf_opts.format = ImageExportFormat::Pdf;
        pdf_opts.transparent_background = true;
        let pdf_report =
            export_project_image(&project, &dir.join("shadings.pdf"), &pdf_opts).unwrap();
        let pdf_bytes = fs::read(&pdf_report.files[0]).unwrap();
        let pdf_text = String::from_utf8_lossy(&pdf_bytes);
        assert!(
            !pdf_text.contains("/Im0 Do"),
            "expected vector PDF, not raster fallback"
        );
        assert!(pdf_text.contains("/ShadingType 2"));
        assert!(pdf_text.contains("/ShadingType 3"));
        assert!(pdf_text.contains("/Sh0 sh"));
        assert!(pdf_text.contains("/Sh1 sh"));

        let (reimported, _) = neoflash_import::import_pdf_ai_with_options(
            &pdf_bytes,
            "shadings.pdf",
            &neoflash_import::VectorImportOptions::default(),
        )
        .expect("re-import exported vector PDF");
        reimported.validate().unwrap();
        let imported_grads: Vec<&GradientPaint> = reimported
            .symbol(&reimported.root)
            .unwrap()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .filter_map(|e| match &e.kind {
                ElementKind::PaintedPath {
                    fill_gradient: Some(g),
                    ..
                } => Some(g),
                _ => None,
            })
            .collect();
        assert_eq!(imported_grads.len(), 2);
        let imp_lin = imported_grads
            .iter()
            .find(|g| g.kind == GradientKind::Linear)
            .expect("re-imported linear shading");
        assert_eq!(imp_lin.stops.len(), 3);
        assert_eq!(imp_lin.stops[1].ratio, 128);
        assert_eq!(imp_lin.stops[1].color, [0, 255, 0, 255]);
        assert_eq!(imp_lin.sample(10.0, 40.0), [255, 0, 0, 255]);
        assert_eq!(
            imp_lin.sample(10.0 + 100.0 * (128.0 / 255.0), 40.0),
            [0, 255, 0, 255]
        );
        assert_eq!(imp_lin.sample(110.0, 40.0), [0, 0, 255, 255]);

        let imp_rad = imported_grads
            .iter()
            .find(|g| g.kind == GradientKind::Radial)
            .expect("re-imported radial shading");
        assert!(
            (imp_rad.focal_point_ratio - 0.35).abs() < 0.02,
            "focal ratio round-trip: {}",
            imp_rad.focal_point_ratio
        );
        assert_eq!(imp_rad.stops.first().unwrap().color, [255, 255, 0, 255]);
        assert_eq!(imp_rad.stops.last().unwrap().color, [128, 0, 255, 255]);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn t414_ai_pdf_text_matrix_textbox_runs_psd_runtime_capture_and_shading_stops() {
        // 1) All-at-1.0 stop pairs in `normalized_shading_stops` hold first color up to 0.9999
        let all_end_paint = GradientPaint {
            kind: GradientKind::Linear,
            stops: vec![
                neoflash_core::GradientStop {
                    ratio: 255,
                    color: [255, 0, 0, 255],
                },
                neoflash_core::GradientStop {
                    ratio: 255,
                    color: [0, 0, 255, 255],
                },
            ],
            ..Default::default()
        };
        let all_end_stops = normalized_shading_stops(&all_end_paint, ColorTransform::default());
        assert_eq!(all_end_stops.len(), 3);
        assert_eq!(all_end_stops[0], (0.0, [1.0, 0.0, 0.0]));
        assert!((all_end_stops[1].0 - 0.9999).abs() < 1e-6);
        assert_eq!(all_end_stops[1].1, [1.0, 0.0, 0.0]);
        assert_eq!(all_end_stops[2], (1.0, [0.0, 0.0, 1.0]));

        // 2) Default AI export (`ai_text_as_outlines == false`) flips Y font matrix (`[size 0 0 -size 0 0] makefont`),
        // splits `\n` lines, accepts `stroke_width == 0` semi-transparent `stroke_gradient`, and renders multi-run TextBox!
        let mut project = Project::blank();
        project.name = "T414 AI PDF PSD".into();
        project.width = 240;
        project.height = 160;
        let mut tb_state = neoflash_core::TextFieldState {
            text: "Red Blue".into(),
            word_wrap: true,
            multiline: true,
            alpha: 255,
            format: neoflash_core::TextFormat {
                font: "Helvetica".into(),
                size: 18.0,
                color: 0xff0000,
                align: neoflash_core::TextAlign::Center,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut blue_fmt = tb_state.format.clone();
        blue_fmt.color = 0x0000ff;
        blue_fmt.bold = true;
        tb_state.format_runs = vec![
            neoflash_core::TextFormatRun {
                start: 0,
                end: 4,
                format: tb_state.format.clone(),
            },
            neoflash_core::TextFormatRun {
                start: 4,
                end: 8,
                format: blue_fmt,
            },
        ];
        let mut tb_el = Element {
            id: new_id("element"),
            name: String::new(),
            transform: Transform {
                x: 10.0,
                y: 50.0,
                ..Transform::default()
            },
            kind: ElementKind::TextBox {
                text: "Red Blue".into(),
                width: 180.0,
                height: 40.0,
                format: tb_state.format.clone(),
                word_wrap: true,
                multiline: true,
            },
            ..Element::default()
        };
        tb_state.write_to_kind(&mut tb_el.kind);

        let semi_stroke_grad = GradientPaint {
            kind: GradientKind::Linear,
            stops: vec![
                neoflash_core::GradientStop {
                    ratio: 0,
                    color: [255, 0, 0, 128],
                },
                neoflash_core::GradientStop {
                    ratio: 255,
                    color: [0, 255, 0, 128],
                },
            ],
            matrix: [0.05, 0.0, 0.0, 0.05, 20.0, 20.0],
            ..Default::default()
        };
        let root_id = project.root.clone();
        project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements = vec![
            Element {
                id: new_id("element"),
                name: "Label".into(),
                transform: Transform {
                    x: 12.0,
                    y: 10.0,
                    ..Transform::default()
                },
                kind: ElementKind::Text {
                    text: "Top\nBottom".into(),
                    font_family: "Courier".into(),
                    font_size: 20.0,
                    color: [20, 40, 60, 255],
                },
                ..Element::default()
            },
            tb_el,
            Element {
                id: new_id("element"),
                name: "ZeroStrokeGrad".into(),
                transform: Transform::default(),
                kind: ElementKind::PaintedPath {
                    commands: rectangle_path(30.0, 20.0).to_vec(),
                    fill_rule: FillRule::NonZero,
                    fill: [100, 150, 200, 255],
                    stroke: [0, 0, 0, 0],
                    fill_gradient: None,
                    stroke_gradient: Some(semi_stroke_grad),
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
        ];
        project.format_version = 5;
        project.upgrade_format_for_features();

        let dir = std::env::temp_dir().join(format!("neoflash-t414-img-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut ai_opts = ImageExportOptions::for_project(&project, 0);
        ai_opts.format = ImageExportFormat::Ai;
        ai_opts.transparent_background = true;
        assert!(!ai_opts.ai_text_as_outlines);
        let ai_rep = export_project_image(&project, &dir.join("t414.ai"), &ai_opts).unwrap();
        let ai_str = fs::read_to_string(&ai_rep.files[0]).unwrap();
        assert!(
            !ai_str.contains("NeoFlashRasterFallback"),
            "opaque AI scene with stroke_width==0 stroke_gradient must stay vector"
        );
        assert!(
            ai_str.contains("/Courier findfont [20 0 0 -20 0 0] makefont setfont"),
            "ps_text must flip font Y-axis in flipped AI coordinate space:\n{ai_str}"
        );
        assert!(ai_str.contains("(Top) show") && ai_str.contains("(Bottom) show"));
        // Both Red (`1 0 0 setrgbcolor`) and Blue (`0 0 1 setrgbcolor`) runs from TextBox must be emitted!
        assert!(ai_str.contains("1 0 0 setrgbcolor") && ai_str.contains("0 0 1 setrgbcolor"));

        // Opaque multi-run TextBox in PDF stays vector and preserves run colors (`1 0 0 rg` & `0 0 1 rg`)!
        let mut pdf_opts = ImageExportOptions::for_project(&project, 0);
        pdf_opts.format = ImageExportFormat::Pdf;
        pdf_opts.transparent_background = true;
        let pdf_rep = export_project_image(&project, &dir.join("t414.pdf"), &pdf_opts).unwrap();
        let pdf_str = String::from_utf8_lossy(&fs::read(&pdf_rep.files[0]).unwrap()).into_owned();
        assert!(
            !pdf_str.contains("/Im0 Do"),
            "opaque multi-run TextBox in PDF must remain vector"
        );
        assert!(pdf_str.contains("1 0 0 rg") && pdf_str.contains("0 0 1 rg"));

        // Semi-transparent TextFieldState (`field.alpha = 128`) propagates alpha into `textbox_outline_runs`,
        // and `ps_draw_supported` rejects semi-transparent text so AI/PDF fall back to raster instead of painting opaque!
        tb_state.alpha = 128;
        let direct_runs = textbox_outline_runs(&tb_state);
        assert!(direct_runs.iter().all(|r| r.color[3] == 128));
        project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].elements[1]
            .transform
            .alpha = 128.0 / 255.0;
        let ai_semi_rep =
            export_project_image(&project, &dir.join("t414_semi.ai"), &ai_opts).unwrap();
        let ai_semi_str = fs::read_to_string(&ai_semi_rep.files[0]).unwrap();
        assert!(ai_semi_str.contains("NeoFlashRasterFallback"));

        // 3) PSD `runtime_capture == true` vs `false` on scripted MovieClip movement
        let mut psd_proj = Project::blank();
        psd_proj.name = "T414 PSD Runtime".into();
        psd_proj.width = 100;
        psd_proj.height = 40;
        let box_sym_id = new_id("symbol");
        psd_proj.symbols.push(neoflash_core::Symbol {
            id: box_sym_id.clone(),
            name: "RedBox".into(),
            folder: String::new(),
            kind: neoflash_core::SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: neoflash_core::Timeline {
                frame_count: 1,
                layers: vec![neoflash_core::Layer {
                    id: new_id("layer"),
                    name: "Shape".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![neoflash_core::Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: new_id("element"),
                            name: String::new(),
                            transform: Transform::default(),
                            kind: ElementKind::Rectangle {
                                width: 20.0,
                                height: 20.0,
                                fill: [255, 0, 0, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Default::default()
                    }],
                    properties: neoflash_core::LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
        });
        let psd_root = psd_proj.root.clone();
        psd_proj.symbol_mut(&psd_root).unwrap().timeline.layers[0].keyframes[0].elements =
            vec![Element {
                id: new_id("element"),
                name: "mover".into(),
                transform: Transform {
                    x: 5.0,
                    y: 10.0,
                    ..Transform::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: box_sym_id,
                    playback: neoflash_core::InstancePlayback::Independent,
                },
                ..Element::default()
            }];
        psd_proj.symbol_mut(&psd_root).unwrap().timeline.scripts =
            vec![neoflash_core::FrameScript {
                layer: None,
                frame: 0,
                script: neoflash_core::Script {
                    language: neoflash_core::ScriptLanguage::ActionScript2,
                    source: "mover._x = 70;".into(),
                },
            }];
        psd_proj.upgrade_format_for_features();

        let mut psd_static = ImageExportOptions::for_project(&psd_proj, 0);
        psd_static.format = ImageExportFormat::Psd;
        psd_static.runtime_capture = false;
        let psd_static_rep =
            export_project_image(&psd_proj, &dir.join("static.psd"), &psd_static).unwrap();
        let psd_static_bytes = fs::read(&psd_static_rep.files[0]).unwrap();

        let mut psd_rt = psd_static.clone();
        psd_rt.runtime_capture = true;
        let psd_rt_rep = export_project_image(&psd_proj, &dir.join("rt.psd"), &psd_rt).unwrap();
        let psd_rt_bytes = fs::read(&psd_rt_rep.files[0]).unwrap();
        assert_ne!(
            psd_static_bytes, psd_rt_bytes,
            "PSD export must honor options.runtime_capture when scripts move stage instances"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
