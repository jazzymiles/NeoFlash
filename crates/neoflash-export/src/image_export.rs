//! Multi-format still and sequence image export (`DL-04`, `W3.1`, `W3.2`):
//! PNG, SVG, PSD (layered Photoshop v1), AI (Illustrator vector), and PDF 1.4.

use crate::{
    ExportError, SvgOptions, atomic_write, render_frame_svg, render_project_frame_rgba,
    video_scene_ranges,
};
use image::{ImageFormat, RgbaImage};
use neoflash_core::{
    AuthoredDraw, AuthoredScene, Color, ColorTransform, ElementKind, FillRule, LayerRole,
    PathCommand, Project, evaluate_scene, flatten_scenes_with_ranges, layer_effectively_visible,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{Cursor, Write as IoWrite},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_AI_RANGE_STAGING_BYTES: usize = 1024 * 1024 * 1024;
const MAX_AI_RANGE_STAGING_FILES: usize = 10_000;
static AI_RANGE_TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(1);

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

struct StagedAiRangeFile {
    final_path: PathBuf,
    staged_path: PathBuf,
    backup_path: PathBuf,
}

struct AiRangeTransaction {
    directory: PathBuf,
    files: Vec<StagedAiRangeFile>,
    staged_bytes: usize,
    max_bytes: usize,
    max_files: usize,
    cleanup_on_drop: bool,
}

enum AiDestinationState {
    ExistingBackup(PathBuf),
    NewPlaceholder,
}

impl AiRangeTransaction {
    fn new(parent: &Path) -> std::io::Result<Self> {
        Self::with_limits(
            parent,
            MAX_AI_RANGE_STAGING_BYTES,
            MAX_AI_RANGE_STAGING_FILES,
        )
    }

    fn with_limits(parent: &Path, max_bytes: usize, max_files: usize) -> std::io::Result<Self> {
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        let epoch_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..64 {
            let sequence = AI_RANGE_TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
            let directory = parent.join(format!(
                ".neoflash-ai-range-{}-{epoch_nanos}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt as _;
                        if let Err(error) =
                            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                        {
                            let _ = fs::remove_dir(&directory);
                            return Err(error);
                        }
                    }
                    return Ok(Self {
                        directory,
                        files: Vec::new(),
                        staged_bytes: 0,
                        max_bytes,
                        max_files,
                        cleanup_on_drop: true,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique AI range staging directory",
        ))
    }

    fn stage(&mut self, final_path: PathBuf, bytes: &[u8]) -> Result<(), ExportError> {
        if self.files.len() >= self.max_files {
            return Err(ExportError::Options(format!(
                "AI frame range exceeds the {}-file transactional staging limit.",
                self.max_files
            )));
        }
        let next_total = self.staged_bytes.checked_add(bytes.len()).ok_or_else(|| {
            ExportError::Options("AI frame-range staging byte size overflow.".into())
        })?;
        if next_total > self.max_bytes {
            return Err(ExportError::Options(format!(
                "AI frame range requires {next_total} staged bytes, exceeding the {} MiB transactional staging limit.",
                self.max_bytes / (1024 * 1024)
            )));
        }

        let index = self.files.len();
        let staged_path = self.directory.join(format!("stage-{index:08}.ai"));
        let backup_path = self.directory.join(format!("backup-{index:08}.ai"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        self.staged_bytes = next_total;
        self.files.push(StagedAiRangeFile {
            final_path,
            staged_path,
            backup_path,
        });
        Ok(())
    }

    fn reserve_destination(
        &mut self,
        final_path: &Path,
        backup_path: &Path,
    ) -> std::io::Result<AiDestinationState> {
        loop {
            match fs::rename(final_path, backup_path) {
                Ok(()) => {
                    let metadata = match fs::symlink_metadata(backup_path) {
                        Ok(metadata) => metadata,
                        Err(error) => {
                            if fs::rename(backup_path, final_path).is_err() {
                                self.cleanup_on_drop = false;
                            }
                            return Err(error);
                        }
                    };
                    if metadata.file_type().is_dir() {
                        if let Err(error) = fs::rename(backup_path, final_path) {
                            self.cleanup_on_drop = false;
                            return Err(error);
                        }
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::IsADirectory,
                            format!("AI output '{}' is a directory", final_path.display()),
                        ));
                    }
                    return Ok(AiDestinationState::ExistingBackup(
                        backup_path.to_path_buf(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(final_path)
                    {
                        Ok(file) => {
                            file.sync_all()?;
                            return Ok(AiDestinationState::NewPlaceholder);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn rollback(published: &[(PathBuf, Option<PathBuf>)]) -> std::io::Result<()> {
        let mut first_error = None;
        for (final_path, backup_path) in published.iter().rev() {
            if let Err(error) = fs::remove_file(final_path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                first_error.get_or_insert(error);
                continue;
            }
            if let Some(backup_path) = backup_path
                && let Err(error) = fs::rename(backup_path, final_path)
            {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn rollback_error(
        &mut self,
        published: &[(PathBuf, Option<PathBuf>)],
        operation_error: std::io::Error,
    ) -> ExportError {
        match Self::rollback(published) {
            Ok(()) if self.cleanup_on_drop => ExportError::Io(operation_error),
            Ok(()) => ExportError::Options(format!(
                "AI range publish failed ({operation_error}); a destination backup could not be restored and was preserved at '{}'.",
                self.directory.display()
            )),
            Err(rollback_error) => {
                self.cleanup_on_drop = false;
                ExportError::Options(format!(
                    "AI range publish failed ({operation_error}) and rollback also failed ({rollback_error}); recoverable backups were preserved at '{}'.",
                    self.directory.display()
                ))
            }
        }
    }

    fn commit(mut self) -> Result<Vec<PathBuf>, ExportError> {
        let mut published = Vec::<(PathBuf, Option<PathBuf>)>::new();
        for index in 0..self.files.len() {
            let final_path = self.files[index].final_path.clone();
            let staged_path = self.files[index].staged_path.clone();
            let backup_path = self.files[index].backup_path.clone();
            let destination = match self.reserve_destination(&final_path, &backup_path) {
                Ok(destination) => destination,
                Err(error) => {
                    return Err(self.rollback_error(&published, error));
                }
            };
            let backup = match &destination {
                AiDestinationState::ExistingBackup(path) => Some(path.clone()),
                AiDestinationState::NewPlaceholder => None,
            };
            if let Err(error) = fs::rename(&staged_path, &final_path) {
                published.push((final_path, backup));
                return Err(self.rollback_error(&published, error));
            }
            published.push((final_path, backup));
        }

        for (_, backup_path) in &published {
            if let Some(backup_path) = backup_path {
                // The replacement is already complete. Cleanup failure must not turn a committed
                // range into a reported failure with valid outputs; `Drop` retries the private
                // staging-directory cleanup.
                let _ = fs::remove_file(backup_path);
            }
        }
        Ok(published
            .into_iter()
            .map(|(final_path, _)| final_path)
            .collect())
    }
}

impl Drop for AiRangeTransaction {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = fs::remove_dir_all(&self.directory);
        }
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

const MAX_PSD_EXPORT_BYTES: usize = 256 * 1024 * 1024;

fn build_psd_bytes(
    project: &Project,
    symbol_id: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    options: &ImageExportOptions,
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
            let scene = evaluate_scene(&single_proj, &sym_id, frame, false, true);
            let rgba = crate::render_authored_scene_rgba(
                &single_proj,
                &sym_id,
                frame,
                &scene,
                width,
                height,
                true,
                &mut bitmap_cache,
            )?;
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
    let composite = render_project_frame_rgba(
        project,
        symbol_id,
        frame,
        width,
        height,
        options.transparent_background,
        false,
    )?;
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

fn ps_draw_supported(draw: &AuthoredDraw, options: &ImageExportOptions) -> bool {
    let transform = draw.item.color_transform;
    let opaque = |color: Color| matches!(transform.apply(color)[3], 0 | 255);
    if !draw.clip_ids.is_empty()
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
            ..
        } => {
            fill_gradient.is_none() && stroke_gradient.is_none() && opaque(*fill) && opaque(*stroke)
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
        ElementKind::TextBox { text, format, .. } => {
            let outlines = options.ai_text_as_outlines || !text.is_ascii();
            opaque([
                ((format.color >> 16) & 0xff) as u8,
                ((format.color >> 8) & 0xff) as u8,
                (format.color & 0xff) as u8,
                255,
            ]) && (!outlines
                || text.trim().is_empty()
                || !neoflash_core::TextFieldState::static_text_to_outline_commands(
                    text,
                    &format.font,
                    format.size,
                    None,
                )
                .is_empty())
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
struct PsShapeStyle {
    rule: FillRule,
    fill: Color,
    stroke: Color,
    stroke_width: f64,
    transform: ColorTransform,
    strokes_as_strokes: bool,
}

fn ps_shape(out: &mut String, commands: &[PathCommand], style: PsShapeStyle) {
    if ps_path(out, commands) {
        ps_paint(
            out,
            style.rule,
            style.fill,
            if style.strokes_as_strokes {
                style.stroke
            } else {
                [0; 4]
            },
            if style.strokes_as_strokes {
                style.stroke_width
            } else {
                0.0
            },
            style.transform,
        );
    }
    if !style.strokes_as_strokes
        && style.stroke_width > 0.0
        && style.transform.apply(style.stroke)[3] > 0
    {
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

fn ps_text(
    out: &mut String,
    text: &str,
    font: &str,
    size: f64,
    color: Color,
    transform: ColorTransform,
    outlines: bool,
) {
    if outlines || !text.is_ascii() {
        let commands =
            neoflash_core::TextFieldState::static_text_to_outline_commands(text, font, size, None);
        if ps_path(out, &commands) {
            ps_paint(out, FillRule::NonZero, color, [0; 4], 0.0, transform);
        }
    } else {
        ps_rgb(out, transform.apply(color));
        let _ = writeln!(
            out,
            "/Helvetica findfont {} scalefont setfont",
            ps_num(size)
        );
        let _ = writeln!(out, "0 {} moveto ({}) show", ps_num(size), ps_string(text));
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
        }
        | ElementKind::PaintedPath {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            ps_shape(
                out,
                commands,
                PsShapeStyle {
                    rule: *fill_rule,
                    fill: *fill,
                    stroke: *stroke,
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
        ElementKind::TextBox { text, format, .. } => {
            let color = [
                ((format.color >> 16) & 0xff) as u8,
                ((format.color >> 8) & 0xff) as u8,
                (format.color & 0xff) as u8,
                255,
            ];
            ps_text(
                out,
                text,
                &format.font,
                format.size,
                color,
                item.color_transform,
                options.ai_text_as_outlines,
            );
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
    let layer_count = eval_project
        .symbol(target)
        .map(|symbol| symbol.timeline.layers.len().max(1))
        .unwrap_or(1);
    let path_count = scene.draws.len();
    let mut content = format!(
        "%!PS-Adobe-3.0 EPSF-3.0\n\
         %%Creator: NeoFlash ({})\n\
         %%Title: {}\n\
         %%BoundingBox: 0 0 {} {}\n\
         %%HiResBoundingBox: 0.0 0.0 {}.0 {}.0\n\
         %%LanguageLevel: 2\n\
         %%Pages: 1\n\
         %AI5_FileFormat 12.0\n\
         %%EndComments\n\
         %%Page: 1 1\n",
        ps_comment(&options.ai_version),
        ps_comment(&project.name),
        width,
        height,
        width,
        height,
    );
    if !ps_scene_supported(&eval_project, &scene, options) {
        let bytes = ps_raster_fallback(
            &eval_project,
            target,
            frame,
            &scene,
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
    // A range of AI files is one user operation. Render every frame before publishing any path so
    // a decoder/validation failure on a later frame cannot leave an earlier numbered file behind.
    let defer_ai_range_publish = options.format == ImageExportFormat::Ai && total_frames > 1;
    let mut ai_range_transaction = if defer_ai_range_publish {
        Some(AiRangeTransaction::new(
            path.parent().unwrap_or_else(|| Path::new(".")),
        )?)
    } else {
        None
    };

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
                            if layer.properties.role == LayerRole::Guide {
                                layer.visible = true;
                            }
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
                    include_guides: options.svg_include_hidden_guides,
                    text_as_outlines: options.svg_text_as_outlines,
                    precision: options.svg_precision.clamp(1, 6),
                };
                let (mut markup, elems, _) = render_frame_svg(&eval_proj, &svg_opts)?;
                path_count += elems;
                let prefix = safe_svg_id_prefix(&options.svg_id_prefix);
                if prefix != "nf-" {
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
        if let Some(transaction) = &mut ai_range_transaction {
            transaction.stage(out_path, &bytes)?;
        } else {
            atomic_write(&out_path, &bytes)?;
            files.push(out_path);
        }
    }

    if let Some(transaction) = ai_range_transaction {
        files = transaction.commit()?;
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
    use neoflash_core::{
        Element, ElementKind, Keyframe, Layer, LayerProperties, ResourceUnit, Symbol,
        TimelineVideoSpan, Transform, new_id,
    };

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
                .is_some_and(|name| name.starts_with(".neoflash-ai-range-"))
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
        let original = b"pre-existing first AI file";
        fs::write(&first, original).unwrap();
        fs::create_dir(&second).unwrap();

        let mut options = ImageExportOptions::for_project(&project, 0);
        options.format = ImageExportFormat::Ai;
        options.range = ImageExportRange::Frames { start: 0, end: 1 };
        let error = export_project_image(&project, &dir.join("range.ai"), &options)
            .expect_err("directory at the second target must fail the range transaction");
        assert!(
            error.to_string().contains("directory") || error.to_string().contains("Directory"),
            "got: {error}"
        );
        assert_eq!(fs::read(&first).unwrap(), original);
        assert!(second.is_dir());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(".neoflash-ai-range-"))
        }));
        fs::remove_dir(&second).unwrap();
        let report = export_project_image(&project, &dir.join("range.ai"), &options)
            .expect("the same two-frame range publishes after the target is repaired");
        assert_eq!(report.files, vec![first.clone(), second.clone()]);
        assert!(first.is_file());
        assert!(second.is_file());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(".neoflash-ai-range-"))
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
            let mut transaction = AiRangeTransaction::with_limits(&dir, 4, 2).unwrap();
            transaction.stage(dir.join("a.ai"), b"1234").unwrap();
            let error = transaction
                .stage(dir.join("b.ai"), b"5")
                .expect_err("aggregate staging bytes must be bounded");
            assert!(error.to_string().contains("staging limit"));
        }
        {
            let mut transaction = AiRangeTransaction::with_limits(&dir, 100, 1).unwrap();
            transaction.stage(dir.join("a.ai"), b"1").unwrap();
            let error = transaction
                .stage(dir.join("b.ai"), b"2")
                .expect_err("staged file count must be bounded");
            assert!(error.to_string().contains("1-file"));
        }
        assert!(!dir.join("a.ai").exists());
        assert!(!dir.join("b.ai").exists());
        assert!(!fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(".neoflash-ai-range-"))
        }));
        let _ = fs::remove_dir_all(dir);
    }
}
