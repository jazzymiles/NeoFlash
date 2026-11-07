//! Multi-format still and sequence image export (`DL-04`, `W3.1`, `W3.2`):
//! PNG, SVG, PSD (layered Photoshop v1), AI (Illustrator vector), and PDF 1.4.

use crate::{
    ExportError, SvgOptions, render_frame_svg, render_project_frame_rgba, video_scene_ranges,
};
use image::{ImageFormat, RgbaImage};
use neoflash_core::{
    LayerRole, Project, evaluate_scene, flatten_scenes_with_ranges, layer_effectively_visible,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

/// Output format selected in `Dialog / Export image` (`DL-04`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageExportFormat {
    #[default]
    Png,
    Svg,
    Psd,
    Ai,
    Pdf,
}

impl ImageExportFormat {
    pub const ALL: [Self; 5] = [Self::Png, Self::Svg, Self::Psd, Self::Ai, Self::Pdf];

    pub const fn index(self) -> usize {
        match self {
            Self::Png => 0,
            Self::Svg => 1,
            Self::Psd => 2,
            Self::Ai => 3,
            Self::Pdf => 4,
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
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
            Self::Psd => "psd",
            Self::Ai => "ai",
            Self::Pdf => "pdf",
        }
    }

    pub const fn button_label(self) -> &'static str {
        match self {
            Self::Png => "Export PNG",
            Self::Svg => "Export SVG",
            Self::Psd => "Export PSD",
            Self::Ai => "Export AI",
            Self::Pdf => "Export PDF",
        }
    }
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
                for (sym_id, _name, start, frames) in video_scene_ranges(project) {
                    if &sym_id == scene_id {
                        let s = start.min(last);
                        let e = start.saturating_add(frames.saturating_sub(1)).min(last);
                        return (s, e.max(s));
                    }
                }
                (0, last)
            }
            Self::Frames { start, end } => {
                let s = (*start).min(last);
                let e = (*end).min(last).max(s);
                (s, e)
            }
        }
    }
}

/// Full configuration for `Dialog / Export image` (`DL-04`).
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
        ImageExportFormat::Png => (w as usize * h as usize / 3).max(1024) * frames,
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
        ImageExportFormat::Png | ImageExportFormat::Pdf => {
            if frames > 1 {
                format!("{size_str} · {frames} frames")
            } else {
                format!("{size_str} · {w} × {h}")
            }
        }
    }
}

fn numbered_output_path(base: &Path, frame_offset: u32, total_frames: u32, ext: &str) -> PathBuf {
    if total_frames <= 1 {
        return base.with_extension(ext);
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("export");
    parent.join(format!("{stem}_{:04}.{ext}", frame_offset + 1))
}

fn encode_png_bytes(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ExportError> {
    let img = RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        ExportError::InvalidProject(format!(
            "Invalid RGBA buffer dimensions {width}x{height} ({} bytes)",
            rgba.len()
        ))
    })?;
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| ExportError::InvalidProject(format!("PNG encode failed: {e}")))?;
    Ok(cursor.into_inner())
}

fn build_psd_bytes(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
) -> Result<(Vec<u8>, usize), ExportError> {
    let composite = render_project_frame_rgba(
        project,
        symbol_id,
        frame,
        width,
        height,
        options.transparent_background,
        false,
    )?;

    // Collect per-layer RGBA buffers when psd_keep_layers is true.
    let mut layer_records: Vec<(String, Vec<u8>)> = Vec::new();
    if options.psd_keep_layers {
        let sym_id = symbol_id
            .map(str::to_owned)
            .unwrap_or_else(|| project.root.clone());
        if let Some(sym) = project.symbol(&sym_id) {
            let mut bitmap_cache = HashMap::new();
            for (idx, layer) in sym.timeline.layers.iter().enumerate().rev() {
                if matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder) {
                    continue;
                }
                if !layer_effectively_visible(&sym.timeline, layer) {
                    continue;
                }
                let mut single_proj = project.clone();
                if let Some(target_sym) = single_proj.symbol_mut(&sym_id) {
                    for (other_idx, other_layer) in
                        target_sym.timeline.layers.iter_mut().enumerate()
                    {
                        if other_idx != idx {
                            other_layer.visible = false;
                        }
                    }
                }
                let scene = evaluate_scene(&single_proj, &sym_id, frame, false, true);
                let rgba = crate::render_authored_scene_rgba(
                    &single_proj,
                    &scene,
                    width,
                    height,
                    true,
                    &mut bitmap_cache,
                );
                layer_records.push((layer.name.clone(), rgba));
            }
        }
    }

    let mut out = Vec::new();
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
    let pixel_count = (width as usize) * (height as usize);
    let channel_data_len = 2 + pixel_count; // 2-byte compression=0 + raw plane
    if layer_records.is_empty() {
        out.extend_from_slice(&0u32.to_be_bytes());
    } else {
        let mut layer_info = Vec::new();
        layer_info.extend_from_slice(&(layer_records.len() as u16).to_be_bytes());
        for (name, _) in &layer_records {
            layer_info.extend_from_slice(&0i32.to_be_bytes()); // top
            layer_info.extend_from_slice(&0i32.to_be_bytes()); // left
            layer_info.extend_from_slice(&(height as i32).to_be_bytes()); // bottom
            layer_info.extend_from_slice(&(width as i32).to_be_bytes()); // right
            layer_info.extend_from_slice(&4u16.to_be_bytes()); // 4 channels
            for ch_id in [0i16, 1, 2, -1] {
                layer_info.extend_from_slice(&ch_id.to_be_bytes());
                layer_info.extend_from_slice(&(channel_data_len as u32).to_be_bytes());
            }
            layer_info.extend_from_slice(b"8BIM");
            layer_info.extend_from_slice(b"norm");
            layer_info.push(255); // opacity
            layer_info.push(0); // clipping
            layer_info.push(0); // flags (visible)
            layer_info.push(0); // filler

            // Extra data field: mask (0 bytes) + blending ranges (0 bytes) + Pascal name padded to 4 bytes
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len().min(255);
            let pascal_total = (1 + name_len + 3) & !3;
            let extra_len = 4 + 4 + pascal_total;
            layer_info.extend_from_slice(&(extra_len as u32).to_be_bytes());
            layer_info.extend_from_slice(&0u32.to_be_bytes()); // layer mask data length
            layer_info.extend_from_slice(&0u32.to_be_bytes()); // layer blending ranges length
            layer_info.push(name_len as u8);
            layer_info.extend_from_slice(&name_bytes[..name_len]);
            for _ in (1 + name_len)..pascal_total {
                layer_info.push(0);
            }
        }
        // Channel image data for each layer (R, G, B, A planes)
        for (_, rgba) in &layer_records {
            for c_idx in [0usize, 1, 2, 3] {
                layer_info.extend_from_slice(&0u16.to_be_bytes()); // raw compression
                for px in rgba.chunks_exact(4) {
                    layer_info.push(px[c_idx]);
                }
            }
        }
        if layer_info.len() % 2 != 0 {
            layer_info.push(0);
        }
        let mut section = Vec::new();
        section.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
        section.extend_from_slice(&layer_info);
        section.extend_from_slice(&0u32.to_be_bytes()); // global layer mask info length = 0
        out.extend_from_slice(&(section.len() as u32).to_be_bytes());
        out.extend_from_slice(&section);
    }

    // 5. Composite Image Data Section (Raw planar R, G, B, A)
    out.extend_from_slice(&0u16.to_be_bytes()); // compression = 0 (Raw)
    for c_idx in [0usize, 1, 2, 3] {
        for px in composite.chunks_exact(4) {
            out.push(px[c_idx]);
        }
    }

    Ok((out, layer_records.len().max(1)))
}

fn build_ai_bytes(
    project: &Project,
    frame: u32,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
) -> Result<(Vec<u8>, usize, usize), ExportError> {
    let mut svg_project = project.clone();
    if options.svg_include_hidden_guides {
        for sym in &mut svg_project.symbols {
            for layer in &mut sym.timeline.layers {
                layer.visible = true;
            }
        }
    }
    let svg_opts = SvgOptions {
        symbol_id: options.symbol_id.clone(),
        frame,
        end_frame: None,
        width: Some(width),
        height: Some(height),
        transparent_background: options.transparent_background,
        text_as_outlines: options.ai_text_as_outlines,
        precision: options.svg_precision.clamp(1, 6),
    };
    let (svg_markup, path_count, _) = render_frame_svg(&svg_project, &svg_opts)?;
    let layer_count = project
        .symbol(&project.root)
        .map(|s| s.timeline.layers.len().max(1))
        .unwrap_or(1);
    let content = format!(
        "%!PS-Adobe-3.0\n\
         %%Creator: NeoFlash ({})\n\
         %%Title: {}\n\
         %%BoundingBox: 0 0 {} {}\n\
         %%HiResBoundingBox: 0.0 0.0 {}.0 {}.0\n\
         %AI5_FileFormat 12.0\n\
         %%EndComments\n\
         % NeoFlash Embedded SVG Vector Stream ({} layers, {} paths)\n\
         {}\n\
         %%EOF\n",
        options.ai_version,
        project.name,
        width,
        height,
        width,
        height,
        layer_count,
        path_count,
        svg_markup
    );
    Ok((content.into_bytes(), layer_count, path_count))
}

fn build_pdf_bytes(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    transparent_bg: bool,
) -> Result<Vec<u8>, ExportError> {
    let rgba = render_project_frame_rgba(
        project,
        symbol_id,
        frame,
        width,
        height,
        transparent_bg,
        false,
    )?;
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for px in rgba.chunks_exact(4) {
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

/// Export a project frame or frame range to PNG, SVG, PSD, AI, or PDF (`DL-04`).
pub fn export_project_image(
    project: &Project,
    path: &Path,
    options: &ImageExportOptions,
) -> Result<ImageExportReport, ExportError> {
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

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let ext = options.format.extension();
    let mut files = Vec::with_capacity(total_frames as usize);
    let mut total_bytes = 0usize;
    let mut layer_count = 1usize;
    let mut path_count = 0usize;

    for offset in 0..total_frames {
        let frame = start_frame + offset;
        let out_path = numbered_output_path(path, offset, total_frames, ext);
        let bytes = match options.format {
            ImageExportFormat::Png => {
                let rgba = render_project_frame_rgba(
                    project,
                    options.symbol_id.as_deref(),
                    frame,
                    width,
                    height,
                    options.transparent_background,
                    false,
                )?;
                encode_png_bytes(&rgba, width, height)?
            }
            ImageExportFormat::Svg => {
                let mut eval_proj = project.clone();
                if options.svg_include_hidden_guides {
                    for sym in &mut eval_proj.symbols {
                        for layer in &mut sym.timeline.layers {
                            layer.visible = true;
                        }
                    }
                }
                let svg_opts = SvgOptions {
                    symbol_id: options.symbol_id.clone(),
                    frame,
                    end_frame: None,
                    width: Some(width),
                    height: Some(height),
                    transparent_background: options.transparent_background,
                    text_as_outlines: options.svg_text_as_outlines,
                    precision: options.svg_precision.clamp(1, 6),
                };
                let (mut markup, elems, _) = render_frame_svg(&eval_proj, &svg_opts)?;
                path_count += elems;
                if !options.svg_id_prefix.trim().is_empty() && options.svg_id_prefix != "nf-" {
                    let prefix = options.svg_id_prefix.trim();
                    markup = markup.replace("id=\"nf-", &format!("id=\"{prefix}"));
                    markup = markup.replace("url(#nf-", &format!("url(#{prefix}"));
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
                let (psd, layers) = build_psd_bytes(
                    project,
                    options.symbol_id.as_deref(),
                    frame,
                    width,
                    height,
                    options,
                )?;
                layer_count = layers;
                psd
            }
            ImageExportFormat::Ai => {
                let (ai, layers, paths) = build_ai_bytes(project, frame, width, height, options)?;
                layer_count = layers;
                path_count += paths;
                ai
            }
            ImageExportFormat::Pdf => build_pdf_bytes(
                project,
                options.symbol_id.as_deref(),
                frame,
                width,
                height,
                options.transparent_background,
            )?,
        };
        total_bytes += bytes.len();
        fs::write(&out_path, &bytes)?;
        files.push(out_path);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
                ImageExportFormat::Ai => assert!(bytes.starts_with(b"%!PS-Adobe-3.0")),
                ImageExportFormat::Pdf => {
                    assert!(bytes.starts_with(b"%PDF-1.4"));
                    assert!(bytes.ends_with(b"%%EOF\n"));
                }
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
