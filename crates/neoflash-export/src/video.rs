//! Video, GIF, and PNG sequence export (`DL-05` / `E-22` / `E-23`).
//!
//! Renders authored or runtime-captured timeline frames to RGBA buffers using a deterministic CPU
//! rasterizer (`tiny-skia` + `ab_glyph` + `image` + `MaskCoverage`), mixes timeline audio tracks
//! via `symphonia`, and encodes to MP4, WebM, MOV (via `ffmpeg`), animated GIF (`gif` + `color_quant`),
//! or numbered PNG sequences (`image`).

use crate::ExportError;
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use color_quant::NeuQuant;
use image::{RgbaImage, imageops::FilterType};
use neoflash_core::{
    AuthoredDraw, AuthoredPlayback, AuthoredScene, Color, ColorTransform, DecodedImage, DrawItem,
    ElementKind, FillRule, GradientKind, GradientPaint, InstancePlayback, LayerRole, MaskCoverage,
    PathCommand, Project, ResolvedVideoPlacement, SoundCommand, SymbolKind, TextAlign, TextFormat,
    TimelineVideoTracker, VideoSyncMode, evaluate_scene, flatten_scenes_with_ranges,
    inspect_video_resource, layer_effectively_visible, oval_path_commands, rectangle_path_commands,
    resolve_scene_video_placements, scene_ranges,
    timeline_video::{
        VideoDeploymentMode, VideoSourceSpec, resolve_video_resource_source, resolve_video_source,
        validate_external_video_path,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fs,
    io::{BufRead, BufReader, BufWriter, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error as SymphoniaError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
use tiny_skia::{
    Color as SkiaColor, FillRule as SkiaFillRule, FilterQuality, GradientStop as SkiaGradientStop,
    IntSize, LineCap, LineJoin, LinearGradient, Mask as SkiaMask, Paint as SkiaPaint,
    Path as SkiaPath, PathBuilder, Pixmap, PixmapPaint, Point as SkiaPoint, RadialGradient,
    SpreadMode, Stroke as SkiaStroke, Transform as SkiaTransform,
};

const FONT_INTER_REGULAR: &[u8] =
    include_bytes!("../../neoflash-ui/assets/fonts/Inter-Regular.ttf");
const FONT_INTER_BOLD: &[u8] = include_bytes!("../../neoflash-ui/assets/fonts/Inter-SemiBold.ttf");
const FONT_MONO: &[u8] = include_bytes!("../../neoflash-ui/assets/fonts/RobotoMono-Regular.ttf");

/// Target container/sequence format for `Dialog / Export video` (`DL-05`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoFormat {
    #[default]
    Mp4,
    WebM,
    Mov,
    Gif,
    PngSequence,
}

impl VideoFormat {
    pub const ALL: [Self; 5] = [
        Self::Mp4,
        Self::WebM,
        Self::Mov,
        Self::Gif,
        Self::PngSequence,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::WebM => "WebM",
            Self::Mov => "MOV",
            Self::Gif => "GIF",
            Self::PngSequence => "PNG seq.",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            Self::Mov => "mov",
            Self::Gif => "gif",
            Self::PngSequence => "png",
        }
    }

    pub fn codecs(self) -> &'static [VideoCodec] {
        match self {
            Self::Mp4 => &[VideoCodec::H264, VideoCodec::H265],
            Self::WebM => &[VideoCodec::Vp9, VideoCodec::Av1],
            Self::Mov => &[VideoCodec::H264, VideoCodec::ProRes, VideoCodec::H265],
            Self::Gif => &[VideoCodec::GifLzw],
            Self::PngSequence => &[VideoCodec::PngLossless],
        }
    }

    pub fn default_codec(self) -> VideoCodec {
        match self {
            Self::Mp4 => VideoCodec::H264,
            Self::WebM => VideoCodec::Vp9,
            Self::Mov => VideoCodec::H264,
            Self::Gif => VideoCodec::GifLzw,
            Self::PngSequence => VideoCodec::PngLossless,
        }
    }

    pub fn supports_audio(self) -> bool {
        !matches!(self, Self::Gif)
    }

    pub fn supports_transparent_bg(self) -> bool {
        matches!(self, Self::Mov | Self::WebM | Self::Gif | Self::PngSequence)
    }

    pub fn requires_ffmpeg(self) -> bool {
        matches!(self, Self::Mp4 | Self::WebM | Self::Mov)
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "mp4" | "m4v" => Some(Self::Mp4),
            "webm" => Some(Self::WebM),
            "mov" | "qt" => Some(Self::Mov),
            "gif" => Some(Self::Gif),
            "png" => Some(Self::PngSequence),
            _ => None,
        }
    }
}

/// Video codec choices matching `Dialog / Export video` (`DL-05`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    Vp9,
    Av1,
    ProRes,
    GifLzw,
    PngLossless,
}

impl VideoCodec {
    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265 (HEVC)",
            Self::Vp9 => "VP9",
            Self::Av1 => "AV1",
            Self::ProRes => "Apple ProRes 4444",
            Self::GifLzw => "GIF 256-color",
            Self::PngLossless => "PNG 32-bit RGBA",
        }
    }

    pub fn is_valid_for(self, format: VideoFormat) -> bool {
        format.codecs().contains(&self)
    }
}

/// Audio encoding preset for video export (`DL-05`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoAudioBitrate {
    Aac128,
    #[default]
    Aac192,
    Aac256,
    Opus160,
    Pcm16,
}

impl VideoAudioBitrate {
    pub const ALL: [Self; 5] = [
        Self::Aac128,
        Self::Aac192,
        Self::Aac256,
        Self::Opus160,
        Self::Pcm16,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Aac128 => "AAC 128 kbps",
            Self::Aac192 => "AAC 192 kbps",
            Self::Aac256 => "AAC 256 kbps",
            Self::Opus160 => "Opus 160 kbps",
            Self::Pcm16 => "PCM 16-bit",
        }
    }

    pub fn kbps(self) -> u32 {
        match self {
            Self::Aac128 => 128,
            Self::Aac192 => 192,
            Self::Aac256 => 256,
            Self::Opus160 => 160,
            Self::Pcm16 => 1411,
        }
    }
}

/// Duration termination rule for `Content ▸ Stop at last frame / Stop after` (`DL-05`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "seconds", rename_all = "snake_case")]
pub enum VideoStopMode {
    #[default]
    LastFrame,
    AfterSeconds(f64),
}

/// Complete configuration for video export (`DL-05`) and `Publish settings ▸ Video (MP4)` (`DL-03`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoOptions {
    pub format: VideoFormat,
    /// Optional Library symbol ID or name to export instead of the main movie.
    pub symbol: Option<String>,
    /// Optional scene ID or name to export instead of all scenes (`None` = `Whole movie (all scenes)`).
    pub scene: Option<String>,
    /// 1-based inclusive start frame.
    pub from_frame: u32,
    /// 1-based inclusive end frame (`None` = end of selected range).
    pub to_frame: Option<u32>,
    /// Output pixel width (`0` = project width * scale).
    pub width: u32,
    /// Output pixel height (`0` = project height * scale).
    pub height: u32,
    pub constrain_proportions: bool,
    pub scale: f32,
    /// Output frame rate (`0.0` = project fps).
    pub fps: f32,
    pub codec: VideoCodec,
    /// Quality slider (`1..=100`, default `85`).
    pub quality: u8,
    /// "Render nested movie clip animation and scripts (runtime capture)".
    pub runtime_capture: bool,
    pub stop_mode: VideoStopMode,
    pub include_audio: bool,
    pub audio_bitrate: VideoAudioBitrate,
    /// "Transparent bg (MOV/WebM)".
    pub transparent_bg: bool,
    /// Optional project base directory used to resolve `VideoDeploymentMode::External` relative paths (`T-201`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<PathBuf>,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            format: VideoFormat::Mp4,
            symbol: None,
            scene: None,
            from_frame: 1,
            to_frame: None,
            width: 0,
            height: 0,
            constrain_proportions: true,
            scale: 1.0,
            fps: 0.0,
            codec: VideoCodec::H264,
            quality: 85,
            runtime_capture: true,
            stop_mode: VideoStopMode::LastFrame,
            include_audio: true,
            audio_bitrate: VideoAudioBitrate::Aac192,
            transparent_bg: false,
            base_dir: None,
        }
    }
}

impl VideoOptions {
    pub fn resolved_dimensions(&self, project: &Project) -> (u32, u32) {
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale.clamp(0.1, 8.0)
        } else {
            1.0
        };
        let w = if self.width > 0 {
            self.width
        } else {
            ((project.width as f32 * scale).round() as u32).max(2)
        };
        let h = if self.height > 0 {
            self.height
        } else {
            ((project.height as f32 * scale).round() as u32).max(2)
        };
        // H.264 / H.265 yuv420p require even dimensions.
        let needs_even = matches!(self.format, VideoFormat::Mp4 | VideoFormat::Mov)
            && matches!(self.codec, VideoCodec::H264 | VideoCodec::H265);
        let w = w.clamp(2, 7680);
        let h = h.clamp(2, 4320);
        if needs_even {
            ((w + 1) & !1, (h + 1) & !1)
        } else {
            (w, h)
        }
    }

    pub fn resolved_fps(&self, project: &Project) -> f32 {
        let raw = if self.fps.is_finite() && self.fps > 0.0 {
            self.fps
        } else {
            project.fps as f32
        };
        raw.clamp(1.0, 120.0)
    }

    pub fn resolved_codec(&self) -> VideoCodec {
        if self.codec.is_valid_for(self.format) {
            self.codec
        } else {
            self.format.default_codec()
        }
    }

    pub fn target_bitrate_mbps(&self, width: u32, height: u32, fps: f32) -> f64 {
        let pixels = f64::from(width) * f64::from(height);
        let ref_pixels = 1920.0 * 1080.0;
        let pixel_factor = (pixels / ref_pixels).clamp(0.08, 6.0);
        let fps_factor = (f64::from(fps) / 30.0).clamp(0.4, 2.5);
        let q = f64::from(self.quality.clamp(1, 100)) / 100.0;
        // At 1080p 24-30fps quality=85 -> ~8 Mbps (matching NF2 outline `85% · ~8 Mbps`).
        let base_1080p = 1.0 + q.powf(1.8) * 10.6;
        (base_1080p * pixel_factor * fps_factor).clamp(0.2, 80.0)
    }
}

/// Pre-export metrics displayed in the `Dialog / Export video` footer (`~4.8 MB · 00:05 · 1920×1080`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoEstimate {
    pub frame_count: u32,
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub estimated_bytes: u64,
    pub bitrate_mbps: f64,
    pub ffmpeg_available: bool,
}

/// Progress snapshot emitted after each rendered frame. Returning `false` from the progress
/// callback cancels the export cleanly.
#[derive(Clone, Debug)]
pub struct VideoProgress {
    pub frame_index: u32,
    pub frame_count: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Summary returned upon completing a video export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoExportReport {
    pub output_path: PathBuf,
    pub additional_files: Vec<PathBuf>,
    pub bytes: u64,
    pub frame_count: u32,
    pub width: u32,
    pub height: u32,
    pub audio_included: bool,
    pub warnings: Vec<String>,
}

fn detect_bundled_media_tool(tool_stem: &str) -> Option<PathBuf> {
    let tool_names = if cfg!(windows) {
        [format!("{tool_stem}.exe"), tool_stem.to_owned()]
    } else {
        [tool_stem.to_owned(), format!("{tool_stem}.exe")]
    };

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe_path) = env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        // 1. macOS .app bundle (`Contents/MacOS/<exe>` -> `Contents/Resources/media/bin` or `Contents/Helpers/media/bin`)
        search_dirs.push(exe_dir.join("../Resources/media/bin"));
        search_dirs.push(exe_dir.join("../Helpers/media/bin"));
        // 2. Windows / portable bundle (`<exe_dir>/media/bin`)
        search_dirs.push(exe_dir.join("media/bin"));
        // 3. Ancestor target/bundled-media/bin (for CLI/dev/test binaries under target/)
        for ancestor in exe_dir.ancestors().take(6) {
            search_dirs.push(ancestor.join("bundled-media/bin"));
            search_dirs.push(ancestor.join("target/bundled-media/bin"));
        }
    }
    search_dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bundled-media/bin"));

    for dir in search_dirs {
        for name in &tool_names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.canonicalize().unwrap_or(candidate));
            }
        }
    }
    None
}

/// Locate the self-contained `ffmpeg` helper bundled with NeoFlash (`T-283`).
pub fn detect_bundled_ffmpeg() -> Option<PathBuf> {
    detect_bundled_media_tool("ffmpeg")
}

/// Locate the self-contained `ffprobe` helper bundled with NeoFlash (`T-283`).
pub fn detect_bundled_ffprobe() -> Option<PathBuf> {
    detect_bundled_media_tool("ffprobe")
}

/// Returns `true` when `binary` points inside a NeoFlash self-contained media bundle (`.../media/bin/*` or `.../bundled-media/bin/*`).
pub fn is_bundled_media_binary(binary: &Path) -> bool {
    let Some(bin_dir) = binary.parent() else {
        return false;
    };
    if bin_dir.file_name().and_then(|s| s.to_str()) != Some("bin") {
        return false;
    }
    let Some(bundle_root) = bin_dir.parent() else {
        return false;
    };
    matches!(
        bundle_root.file_name().and_then(|s| s.to_str()),
        Some("media" | "bundled-media" | "bundled-media-lgpl")
    )
}

fn configure_media_subprocess_command(cmd: &mut Command, binary: &Path) {
    if is_bundled_media_binary(binary)
        || env::var("NEOFLASH_DISABLE_SYSTEM_FFMPEG").ok().as_deref() == Some("1")
    {
        cmd.env("PATH", "");
    }
}

/// Locate an `ffmpeg` binary (`NEOFLASH_FFMPEG`, bundled `Resources/media/bin/ffmpeg`, `PATH`, or system fallbacks).
pub fn detect_ffmpeg() -> Option<PathBuf> {
    if let Ok(custom) = env::var("NEOFLASH_FFMPEG") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Some(bundled) = detect_bundled_ffmpeg() {
        return Some(bundled);
    }
    if env::var("NEOFLASH_DISABLE_SYSTEM_FFMPEG").ok().as_deref() == Some("1") {
        return None;
    }
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in ["ffmpeg", "ffmpeg.exe"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for fallback in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
    ] {
        let p = PathBuf::from(fallback);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Probe and cache the set of encoder names supported by `ffmpeg` (`T-283`).
pub fn probe_ffmpeg_encoders(ffmpeg: &Path) -> HashSet<String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, HashSet<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = ffmpeg
        .canonicalize()
        .unwrap_or_else(|_| ffmpeg.to_path_buf());
    if let Ok(guard) = cache.lock()
        && let Some(existing) = guard.get(&key)
    {
        return existing.clone();
    }
    let mut encoders = HashSet::new();
    let mut cmd = Command::new(ffmpeg);
    configure_media_subprocess_command(&mut cmd, ffmpeg);
    if let Ok(output) = cmd
        .args(["-hide_banner", "-encoders"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let trimmed = line.trim_start();
            let mut parts = trimmed.split_whitespace();
            if let (Some(flags), Some(name)) = (parts.next(), parts.next())
                && flags.len() == 6
                && (flags.starts_with('V') || flags.starts_with('A'))
            {
                encoders.insert(name.to_owned());
            }
        }
    }
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, encoders.clone());
    }
    encoders
}

/// Select the primary `(encoder_name, is_hardware)` for `codec` given the capabilities of `ffmpeg` (`T-283`).
///
/// When `prefer_hardware` is `true` (or when the software encoder is absent, as in an LGPL-only build),
/// selects Apple `VideoToolbox` (`h264_videotoolbox` / `hevc_videotoolbox` / `prores_videotoolbox`) or
/// Windows `Media Foundation` (`h264_mf` / `hevc_mf`) if present, with software fallback to
/// `libx264` / `libx265` / `prores_ks`.
pub fn select_video_encoder(
    ffmpeg: &Path,
    codec: VideoCodec,
    prefer_hardware: bool,
) -> (&'static str, bool) {
    let encoders = probe_ffmpeg_encoders(ffmpeg);
    match codec {
        VideoCodec::H264 => {
            if prefer_hardware || !encoders.contains("libx264") {
                if encoders.contains("h264_videotoolbox") {
                    return ("h264_videotoolbox", true);
                }
                if encoders.contains("h264_mf") {
                    return ("h264_mf", true);
                }
            }
            ("libx264", false)
        }
        VideoCodec::H265 => {
            if prefer_hardware || !encoders.contains("libx265") {
                if encoders.contains("hevc_videotoolbox") {
                    return ("hevc_videotoolbox", true);
                }
                if encoders.contains("hevc_mf") {
                    return ("hevc_mf", true);
                }
            }
            ("libx265", false)
        }
        VideoCodec::ProRes => {
            if (prefer_hardware || !encoders.contains("prores_ks"))
                && encoders.contains("prores_videotoolbox")
            {
                return ("prores_videotoolbox", true);
            }
            ("prores_ks", false)
        }
        VideoCodec::Vp9 => ("libvpx-vp9", false),
        VideoCodec::Av1 => ("libsvtav1", false),
        VideoCodec::GifLzw => ("gif", false),
        VideoCodec::PngLossless => ("png", false),
    }
}

struct PreparedTimeline {
    eval_project: Project,
    root_id: String,
    start_frame: u32,
    frame_count: u32,
    authored_span: u32,
}

fn prepare_timeline(
    project: &Project,
    options: &VideoOptions,
) -> Result<PreparedTimeline, ExportError> {
    project
        .validate()
        .map_err(|err| ExportError::InvalidProject(err.to_string()))?;

    let (mut eval_project, root_id, scene_default_start, scene_default_end) =
        if let Some(symbol_query) = options
            .symbol
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let symbol = project
                .symbols
                .iter()
                .find(|s| s.id == symbol_query || s.name == symbol_query)
                .ok_or_else(|| {
                    ExportError::Options(format!("Symbol '{symbol_query}' was not found"))
                })?;
            let mut cloned = project.clone();
            cloned.root = symbol.id.clone();
            cloned.scenes.clear();
            let total = symbol.timeline.frame_count.max(1);
            (cloned, symbol.id.clone(), 1u32, total)
        } else {
            let (flattened, ranges) = flatten_scenes_with_ranges(project);
            let root = flattened.root.clone();
            let total = flattened
                .symbol(&root)
                .map(|s| s.timeline.frame_count.max(1))
                .unwrap_or(1);
            if let Some(scene_query) = options
                .scene
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let scene_ids: Vec<String> = if project.scenes.is_empty() {
                    vec![project.root.clone()]
                } else {
                    project.scenes.clone()
                };
                let range = ranges
                    .iter()
                    .enumerate()
                    .find(|(idx, r)| {
                        r.name == scene_query
                            || scene_ids.get(*idx).is_some_and(|id| id == scene_query)
                    })
                    .map(|(_, r)| r)
                    .ok_or_else(|| {
                        ExportError::Options(format!("Scene '{scene_query}' was not found"))
                    })?;
                let start_1 = range.start + 1;
                let end_1 = (range.start + range.frames.max(1)).max(start_1);
                (flattened, root, start_1, end_1)
            } else {
                (flattened, root, 1u32, total)
            }
        };

    if !options.runtime_capture {
        // When runtime_capture is disabled, nested MovieClip instances stay at their initial
        // keyframe while Graphic symbols continue to animate in sync with the parent timeline.
        let mc_ids: std::collections::HashSet<String> = eval_project
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::MovieClip && s.id != root_id)
            .map(|s| s.id.clone())
            .collect();
        for sym in &mut eval_project.symbols {
            for layer in &mut sym.timeline.layers {
                for kf in &mut layer.keyframes {
                    for el in &mut kf.elements {
                        if let ElementKind::Instance {
                            symbol_id,
                            playback,
                        } = &mut el.kind
                            && mc_ids.contains(symbol_id)
                            && matches!(playback, InstancePlayback::Independent)
                        {
                            *playback = InstancePlayback::SingleFrame(0);
                        }
                    }
                }
            }
        }
    }

    let from_1 = options
        .from_frame
        .max(1)
        .clamp(scene_default_start, scene_default_end);
    let to_1 = options
        .to_frame
        .unwrap_or(scene_default_end)
        .max(from_1)
        .min(scene_default_end.max(from_1));
    let start_frame = from_1 - 1;
    let authored_span = (to_1 - from_1 + 1).max(1);

    let fps = options.resolved_fps(project);
    let frame_count = match options.stop_mode {
        VideoStopMode::LastFrame => authored_span,
        VideoStopMode::AfterSeconds(secs) => {
            if !secs.is_finite() || secs <= 0.0 {
                return Err(ExportError::Options(
                    "Stop-after duration must be greater than 0 seconds".into(),
                ));
            }
            ((secs * f64::from(fps)).round() as u32).clamp(1, 36_000)
        }
    };

    Ok(PreparedTimeline {
        eval_project,
        root_id,
        start_frame,
        frame_count,
        authored_span,
    })
}

/// Estimate frame count, duration, dimensions, and output byte size for `Dialog / Export video`.
pub fn estimate_video(
    project: &Project,
    options: &VideoOptions,
) -> Result<VideoEstimate, ExportError> {
    let prepared = prepare_timeline(project, options)?;
    let (width, height) = options.resolved_dimensions(project);
    let fps = options.resolved_fps(project);
    let duration_secs = f64::from(prepared.frame_count) / f64::from(fps);
    let bitrate_mbps = options.target_bitrate_mbps(width, height, fps);

    let video_bytes = match options.format {
        VideoFormat::Mp4 | VideoFormat::WebM => {
            ((bitrate_mbps * 1_000_000.0 / 8.0) * duration_secs) as u64
        }
        VideoFormat::Mov => {
            let mult = if options.resolved_codec() == VideoCodec::ProRes {
                4.5
            } else {
                1.15
            };
            ((bitrate_mbps * mult * 1_000_000.0 / 8.0) * duration_secs) as u64
        }
        VideoFormat::Gif => {
            let per_frame = (u64::from(width) * u64::from(height)) / 6;
            per_frame.saturating_mul(u64::from(prepared.frame_count))
        }
        VideoFormat::PngSequence => {
            let per_frame = (u64::from(width) * u64::from(height)) / 3;
            per_frame.saturating_mul(u64::from(prepared.frame_count))
        }
    };
    let audio_bytes = if options.include_audio && options.format.supports_audio() {
        ((f64::from(options.audio_bitrate.kbps()) * 1000.0 / 8.0) * duration_secs) as u64
    } else {
        0
    };

    Ok(VideoEstimate {
        frame_count: prepared.frame_count,
        duration_secs,
        width,
        height,
        fps,
        estimated_bytes: (video_bytes + audio_bytes).max(1024),
        bitrate_mbps,
        ffmpeg_available: detect_ffmpeg().is_some(),
    })
}

/// Scene range helper for populating the `Range` selector in `Dialog / Export video`.
pub fn video_scene_ranges(project: &Project) -> Vec<(String, String, u32, u32)> {
    let scene_ids: Vec<String> = if project.scenes.is_empty() {
        vec![project.root.clone()]
    } else {
        project.scenes.clone()
    };
    scene_ranges(project)
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let id = scene_ids
                .get(idx)
                .cloned()
                .unwrap_or_else(|| r.name.clone());
            let start_1 = r.start + 1;
            let end_1 = r.start + r.frames.max(1);
            (id, r.name, start_1, end_1)
        })
        .collect()
}

const NFVF_FOURCC: &[u8; 4] = b"nfvf";
const MP4_FTYP_AVC1_BOX: &[u8; 32] = b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41";
const LEGACY_STUB_MP4_HEADER: &[u8; 32] =
    b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2avc1mp41";

static VIDEO_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

// Compressed video can expand by several orders of magnitude. The current renderer retains
// decoded frames for deterministic random access, so keep each stream and one export batch within
// explicit resident-memory budgets until the decoder is replaced by an on-demand frame cache.
const MAX_DECODED_VIDEO_WIDTH: u32 = 4096;
const MAX_DECODED_VIDEO_HEIGHT: u32 = 4096;
const MAX_DECODED_VIDEO_FRAMES: usize = 36_000;
const MAX_DECODED_VIDEO_BATCH_FRAMES: usize = 144_000;
const MAX_VIDEO_INPUT_BYTES: usize = 512 * 1024 * 1024;
const MAX_DECODED_VIDEO_STREAM_BYTES: usize = 512 * 1024 * 1024;
const MAX_DECODED_VIDEO_BATCH_BYTES: usize = 1024 * 1024 * 1024;
const MAX_DECODED_VIDEO_DURATION_MS: u64 = 60 * 60 * 1000;
const MAX_FFMPEG_DIAGNOSTIC_BYTES: usize = 64 * 1024;
/// Short clips at or below this uncompressed RGBA size and frame count take the eager in-memory
/// fast path; longer streams switch automatically to `OnDemandVideoDecoder` (`T-229`).
const MAX_EAGER_VIDEO_STREAM_BYTES: usize = 32 * 1024 * 1024;
const MAX_EAGER_VIDEO_FRAMES: usize = 180;
/// Pinned CPU decoded-frame ring budget per long-form video stream (`T-229`).
const MAX_WINDOWED_RING_STREAM_BYTES: usize = 64 * 1024 * 1024;
const MAX_WINDOWED_RING_FRAMES: usize = 32;

fn read_bounded_video_file_with_limit(
    path: &Path,
    label: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Failed to open external video '{label}': {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to inspect external video '{label}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("External video '{label}' is not a regular file."));
    }
    if metadata.len() > limit as u64 {
        return Err(format!(
            "External video '{label}' is {} bytes, exceeding the {} MiB input budget.",
            metadata.len(),
            limit / (1024 * 1024)
        ));
    }
    let capacity = usize::try_from(metadata.len()).unwrap_or(limit).min(limit);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read external video '{label}': {error}"))?;
    if bytes.len() > limit {
        return Err(format!(
            "External video '{label}' grew beyond the {} MiB input budget while it was being read.",
            limit / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

fn read_bounded_video_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    read_bounded_video_file_with_limit(path, label, MAX_VIDEO_INPUT_BYTES)
}

/// A single decoded straight-RGBA8 video frame with its presentation timestamp (`T-201`).
#[derive(Clone, Debug)]
pub struct DecodedVideoFrame {
    pub timestamp_ms: u32,
    pub duration_ms: u32,
    pub image: Arc<DecodedImage>,
}

#[derive(Debug)]
enum FfmpegMediaBacking {
    ExternalPath(PathBuf),
    Staged(StagedMediaFile),
}

impl FfmpegMediaBacking {
    fn path(&self) -> &Path {
        match self {
            Self::ExternalPath(path) => path,
            Self::Staged(staged) => staged.path(),
        }
    }
}

#[derive(Debug)]
enum OnDemandVideoBackend {
    Nfvf {
        payload: Arc<[u8]>,
        frames_offset: usize,
        frame_bytes: usize,
    },
    Avi {
        bytes: Arc<[u8]>,
        container: neoflash_core::AviIndexedContainer,
    },
    Ffmpeg {
        ffmpeg: PathBuf,
        backing: FfmpegMediaBacking,
    },
}

#[derive(Debug)]
struct WindowedFrameRing {
    entries: VecDeque<(usize, DecodedVideoFrame)>,
    resident_bytes: usize,
    effective_frame_count: usize,
    window_fetches: usize,
}

/// Bounded on-demand / windowed ring decoder for long-form timeline video (`T-229`).
#[derive(Debug)]
pub struct OnDemandVideoDecoder {
    width: u32,
    height: u32,
    fps: f32,
    frame_dur_ms: u32,
    frame_bytes: usize,
    frame_count: usize,
    max_ring_frames: usize,
    max_ring_bytes: usize,
    window_fetch_frames: usize,
    backing_ram_bytes: usize,
    backend: OnDemandVideoBackend,
    ring: Mutex<WindowedFrameRing>,
}

impl OnDemandVideoDecoder {
    fn new(
        width: u32,
        height: u32,
        fps: f32,
        frame_bytes: usize,
        frame_count: usize,
        backing_ram_bytes: usize,
        backend: OnDemandVideoBackend,
    ) -> Self {
        let safe_fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            24.0
        };
        let frame_dur_ms = ((1000.0 / f64::from(safe_fps)).round() as u32).max(1);
        let max_ring_frames = (MAX_WINDOWED_RING_STREAM_BYTES / frame_bytes.max(1))
            .clamp(1, MAX_WINDOWED_RING_FRAMES)
            .min(frame_count.max(1));
        let max_ring_bytes = max_ring_frames.saturating_mul(frame_bytes).max(frame_bytes);
        let window_fetch_frames = max_ring_frames.clamp(1, 16);
        Self {
            width,
            height,
            fps: safe_fps,
            frame_dur_ms,
            frame_bytes,
            frame_count,
            max_ring_frames,
            max_ring_bytes,
            window_fetch_frames,
            backing_ram_bytes,
            backend,
            ring: Mutex::new(WindowedFrameRing {
                entries: VecDeque::with_capacity(max_ring_frames),
                resident_bytes: 0,
                effective_frame_count: frame_count,
                window_fetches: 0,
            }),
        }
    }

    pub fn logical_frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn max_ring_frames(&self) -> usize {
        self.max_ring_frames
    }

    /// Pinned upper-bound CPU memory reservation for this stream (ring capacity + any in-RAM
    /// compressed container bytes).
    pub fn pinned_budget_bytes(&self) -> Option<usize> {
        const ARC_ALLOCATION_OVERHEAD: usize = 2 * std::mem::size_of::<usize>();
        let per_ring_entry = self
            .frame_bytes
            .checked_add(std::mem::size_of::<DecodedImage>())?
            .checked_add(ARC_ALLOCATION_OVERHEAD)?
            .checked_add(std::mem::size_of::<(usize, DecodedVideoFrame)>())?;
        let ring_budget = self.max_ring_frames.checked_mul(per_ring_entry)?;
        std::mem::size_of::<Self>()
            .checked_add(ARC_ALLOCATION_OVERHEAD)?
            .checked_add(ring_budget)?
            .checked_add(self.backing_ram_bytes)
    }

    pub fn resident_ring_frames(&self) -> usize {
        self.ring.lock().map_or(0, |ring| ring.entries.len())
    }

    pub fn resident_ring_bytes(&self) -> usize {
        self.ring.lock().map_or(0, |ring| ring.resident_bytes)
    }

    pub fn window_fetch_count(&self) -> usize {
        self.ring.lock().map_or(0, |ring| ring.window_fetches)
    }

    fn timestamp_ms_for_index(&self, idx: usize) -> u32 {
        match &self.backend {
            OnDemandVideoBackend::Avi { container, .. } => container
                .frames
                .get(idx)
                .map_or(0, |entry| entry.timestamp_ms),
            OnDemandVideoBackend::Nfvf { .. } | OnDemandVideoBackend::Ffmpeg { .. } => {
                ((idx as f64 * 1000.0) / f64::from(self.fps)).round() as u32
            }
        }
    }

    fn choose_index_for_target_ms(&self, target_ms: u32) -> Option<usize> {
        let effective_count = self
            .ring
            .lock()
            .map_or(self.frame_count, |r| r.effective_frame_count);
        if effective_count == 0 {
            return None;
        }
        match &self.backend {
            OnDemandVideoBackend::Avi { container, .. } => container
                .frame_index_at_ms(target_ms)
                .map(|idx| idx.min(effective_count - 1)),
            OnDemandVideoBackend::Nfvf { .. } | OnDemandVideoBackend::Ffmpeg { .. } => {
                let mut lo = 0usize;
                let mut hi = effective_count;
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if self.timestamp_ms_for_index(mid) <= target_ms {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                Some(lo.saturating_sub(1).min(effective_count - 1))
            }
        }
    }

    fn decode_window_frames(
        &self,
        start_idx: usize,
        count: usize,
    ) -> Result<Vec<(usize, DecodedVideoFrame)>, String> {
        if count == 0 || start_idx >= self.frame_count {
            return Ok(Vec::new());
        }
        let bounded_count = count.min(self.frame_count - start_idx);
        match &self.backend {
            OnDemandVideoBackend::Nfvf {
                payload,
                frames_offset,
                frame_bytes,
            } => {
                let mut out = Vec::with_capacity(bounded_count);
                for offset in 0..bounded_count {
                    let idx = start_idx + offset;
                    let byte_start =
                        frames_offset
                            .checked_add(idx.checked_mul(*frame_bytes).ok_or_else(|| {
                                "nfvf window frame byte offset overflow.".to_owned()
                            })?)
                            .ok_or_else(|| "nfvf window payload offset overflow.".to_owned())?;
                    let byte_end = byte_start
                        .checked_add(*frame_bytes)
                        .ok_or_else(|| "nfvf window end offset overflow.".to_owned())?;
                    let slice = payload
                        .get(byte_start..byte_end)
                        .ok_or_else(|| "Truncated nfvf window slice.".to_owned())?;
                    out.push((
                        idx,
                        DecodedVideoFrame {
                            timestamp_ms: self.timestamp_ms_for_index(idx),
                            duration_ms: self.frame_dur_ms,
                            image: Arc::new(DecodedImage {
                                width: self.width,
                                height: self.height,
                                rgba: slice.to_vec().into(),
                            }),
                        },
                    ));
                }
                Ok(out)
            }
            OnDemandVideoBackend::Avi { bytes, container } => {
                let mut out = Vec::with_capacity(bounded_count);
                for offset in 0..bounded_count {
                    let idx = start_idx + offset;
                    let decoded = container.decode_frame(bytes, idx)?;
                    out.push((
                        idx,
                        DecodedVideoFrame {
                            timestamp_ms: self.timestamp_ms_for_index(idx),
                            duration_ms: self.frame_dur_ms,
                            image: Arc::new(decoded),
                        },
                    ));
                }
                Ok(out)
            }
            OnDemandVideoBackend::Ffmpeg { ffmpeg, backing } => {
                let seek_sec = ((start_idx as f64 - 0.05) / f64::from(self.fps)).max(0.0);
                let raw = decode_ffmpeg_pam_window(
                    ffmpeg,
                    backing.path(),
                    Some(seek_sec),
                    bounded_count,
                    Some((self.width, self.height)),
                )?;
                let mut out = Vec::with_capacity(raw.images.len());
                for (offset, image) in raw.images.into_iter().enumerate() {
                    let idx = start_idx + offset;
                    out.push((
                        idx,
                        DecodedVideoFrame {
                            timestamp_ms: self.timestamp_ms_for_index(idx),
                            duration_ms: self.frame_dur_ms,
                            image,
                        },
                    ));
                }
                Ok(out)
            }
        }
    }

    fn insert_decoded_batch(
        &self,
        ring: &mut WindowedFrameRing,
        decoded: Vec<(usize, DecodedVideoFrame)>,
    ) {
        for (idx, frame) in decoded {
            let img_bytes = frame.image.rgba.len();
            if let Some(pos) = ring
                .entries
                .iter()
                .position(|(existing, _)| *existing == idx)
                && let Some((_, old)) = ring.entries.remove(pos)
            {
                ring.resident_bytes = ring.resident_bytes.saturating_sub(old.image.rgba.len());
            }
            while (!ring.entries.is_empty())
                && (ring.entries.len() >= self.max_ring_frames
                    || ring.resident_bytes.saturating_add(img_bytes) > self.max_ring_bytes)
            {
                if let Some((_, evicted)) = ring.entries.pop_front() {
                    ring.resident_bytes =
                        ring.resident_bytes.saturating_sub(evicted.image.rgba.len());
                }
            }
            ring.resident_bytes = ring.resident_bytes.saturating_add(img_bytes);
            ring.entries.push_back((idx, frame));
        }
    }

    fn frame_at_index(&self, target_idx: usize) -> Option<(usize, DecodedVideoFrame)> {
        {
            let mut ring = self.ring.lock().ok()?;
            if let Some(pos) = ring.entries.iter().position(|(idx, _)| *idx == target_idx) {
                let entry = ring.entries.remove(pos)?;
                let out = entry.clone();
                ring.entries.push_back(entry);
                return Some(out);
            }
        }

        let fetched = self
            .decode_window_frames(target_idx, self.window_fetch_frames)
            .ok()?;
        if !fetched.is_empty() {
            let mut ring = self.ring.lock().ok()?;
            ring.window_fetches = ring.window_fetches.saturating_add(1);
            self.insert_decoded_batch(&mut ring, fetched);
            if let Some((_, frame)) = ring.entries.iter().find(|(idx, _)| *idx == target_idx) {
                return Some((target_idx, frame.clone()));
            }
        } else if target_idx > 0 {
            // Audio-padded containers can report a format duration 1-2 frames past the final video
            // packet. Step back one window to clamp cleanly to the true final video frame.
            let fallback_start = target_idx.saturating_sub(self.window_fetch_frames.max(4));
            let tail = self
                .decode_window_frames(fallback_start, target_idx - fallback_start + 1)
                .ok()?;
            if let Some((last_idx, _)) = tail.last().cloned() {
                let mut ring = self.ring.lock().ok()?;
                ring.window_fetches = ring.window_fetches.saturating_add(1);
                ring.effective_frame_count = ring.effective_frame_count.min(last_idx + 1);
                self.insert_decoded_batch(&mut ring, tail);
                if let Some((_, frame)) = ring.entries.iter().find(|(idx, _)| *idx == last_idx) {
                    return Some((last_idx, frame.clone()));
                }
            }
        }
        None
    }
}

/// Decoded timeline video stream with deterministic timestamp lookup (`T-201` / `T-229`).
#[derive(Clone, Debug)]
pub struct DecodedVideoStream {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub duration_ms: u32,
    pub frames: Vec<DecodedVideoFrame>,
    pub on_demand: Option<Arc<OnDemandVideoDecoder>>,
}

impl DecodedVideoStream {
    pub fn logical_frame_count(&self) -> usize {
        self.on_demand
            .as_ref()
            .map_or(self.frames.len(), |d| d.logical_frame_count())
    }

    pub fn resident_cpu_bytes(&self) -> usize {
        if let Some(decoder) = &self.on_demand {
            decoder.resident_ring_bytes()
        } else {
            self.frames.iter().map(|f| f.image.rgba.len()).sum()
        }
    }

    /// Resolve `(frame_index, DecodedVideoFrame)` for `placement`, honoring `in_point_ms`,
    /// `out_point_ms`, `loop_video`, and retained `stopped` frames (`T-201` / `T-229`).
    pub fn frame_at_placement(
        &self,
        placement: &ResolvedVideoPlacement,
    ) -> Option<(usize, DecodedVideoFrame)> {
        let has_any_frames = !self.frames.is_empty()
            || self
                .on_demand
                .as_ref()
                .is_some_and(|d| d.logical_frame_count() > 0);
        if !has_any_frames || (placement.sync == VideoSyncMode::Stop && !placement.stopped) {
            return None;
        }
        let effective_end = match (
            placement.out_point_ms,
            Some(self.duration_ms).filter(|&d| d > 0),
        ) {
            (Some(out), Some(dur)) => Some(out.min(dur)),
            (Some(out), None) => Some(out),
            (None, Some(dur)) => Some(dur),
            (None, None) => None,
        };
        let mut target_ms = placement.effective_time_ms(Some(self.duration_ms));
        if !placement.loop_video
            && let Some(end_ms) = effective_end
            && end_ms > placement.in_point_ms
        {
            target_ms = target_ms.min(end_ms.saturating_sub(1));
        }
        if let Some(decoder) = &self.on_demand {
            let chosen_idx = decoder.choose_index_for_target_ms(target_ms)?;
            return decoder.frame_at_index(chosen_idx);
        }
        let mut chosen_idx = 0usize;
        for (idx, frame) in self.frames.iter().enumerate() {
            if frame.timestamp_ms <= target_ms {
                chosen_idx = idx;
            } else {
                break;
            }
        }
        self.frames
            .get(chosen_idx)
            .cloned()
            .map(|f| (chosen_idx, f))
    }
}

/// Encode straight-RGBA8 frames into NeoFlash's deterministic `nfvf` test-fixture payload inside
/// an ISO-BMFF box sequence (`T-201`). This is intentionally not an interoperable H.264 encoder;
/// production MP4 output uses `ffmpeg`. The private payload gives tests a small, lossless stream
/// that `decode_video_stream_bytes` can decode without an external codec binary.
#[doc(hidden)]
pub fn encode_deterministic_mp4_rgba_frames(
    width: u32,
    height: u32,
    fps: u16,
    frames: &[&[u8]],
) -> Vec<u8> {
    let w = width.max(1);
    let h = height.max(1);
    let safe_fps = fps.max(1);
    let frame_byte_len = (w as usize) * (h as usize) * 4;
    let count = frames.len().min(u16::MAX as usize);
    let payload_len = 13usize + count * frame_byte_len;
    let box_size = (8usize + payload_len) as u32;

    let mut out = Vec::with_capacity(32 + box_size as usize);
    out.extend_from_slice(MP4_FTYP_AVC1_BOX);
    out.extend_from_slice(&box_size.to_be_bytes());
    out.extend_from_slice(NFVF_FOURCC);
    out.push(1u8); // version 1
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&safe_fps.to_be_bytes());
    out.extend_from_slice(&(count as u16).to_be_bytes());
    for frame in frames.iter().take(count) {
        if frame.len() >= frame_byte_len {
            out.extend_from_slice(&frame[..frame_byte_len]);
        } else {
            out.extend_from_slice(frame);
            out.resize(out.len() + (frame_byte_len - frame.len()), 0);
        }
    }
    out
}

fn checked_decoded_video_payload_len(
    width: u32,
    height: u32,
    frame_count: usize,
) -> Result<(usize, usize), String> {
    if width == 0
        || height == 0
        || width > MAX_DECODED_VIDEO_WIDTH
        || height > MAX_DECODED_VIDEO_HEIGHT
    {
        return Err(format!(
            "Decoded video dimensions {width}x{height} exceed the supported 1..={MAX_DECODED_VIDEO_WIDTH} by 1..={MAX_DECODED_VIDEO_HEIGHT} bounds."
        ));
    }
    if frame_count == 0 || frame_count > MAX_DECODED_VIDEO_FRAMES {
        return Err(format!(
            "Decoded video frame count {frame_count} exceeds the supported 1..={MAX_DECODED_VIDEO_FRAMES} budget."
        ));
    }
    let frame_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Decoded video frame dimensions overflow the RGBA byte size.".to_owned())?;
    let total_bytes = frame_bytes
        .checked_mul(frame_count)
        .ok_or_else(|| "Decoded video stream RGBA byte size overflow.".to_owned())?;
    if total_bytes > MAX_DECODED_VIDEO_STREAM_BYTES {
        return Err(format!(
            "Decoded video requires {total_bytes} RGBA bytes, exceeding the {} MiB retained-frame budget. Shorten or resize the source video.",
            MAX_DECODED_VIDEO_STREAM_BYTES / (1024 * 1024)
        ));
    }
    Ok((frame_bytes, total_bytes))
}

fn checked_decoded_video_duration_ms(frame_count: usize, fps: f32) -> Result<u32, String> {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        f64::from(fps)
    } else {
        24.0
    };
    let duration_ms = (frame_count as f64 * 1000.0 / safe_fps).round().max(1.0);
    if !duration_ms.is_finite() || duration_ms > MAX_DECODED_VIDEO_DURATION_MS as f64 {
        return Err(format!(
            "Decoded video duration exceeds the supported {} minute retained-frame budget.",
            MAX_DECODED_VIDEO_DURATION_MS / 60_000
        ));
    }
    Ok(duration_ms as u32)
}

fn checked_on_demand_video_bounds(
    width: u32,
    height: u32,
    frame_count: usize,
) -> Result<usize, String> {
    if width == 0
        || height == 0
        || width > MAX_DECODED_VIDEO_WIDTH
        || height > MAX_DECODED_VIDEO_HEIGHT
    {
        return Err(format!(
            "Decoded video dimensions {width}x{height} exceed the supported 1..={MAX_DECODED_VIDEO_WIDTH} by 1..={MAX_DECODED_VIDEO_HEIGHT} bounds."
        ));
    }
    if frame_count == 0 || frame_count > MAX_DECODED_VIDEO_BATCH_FRAMES {
        return Err(format!(
            "Decoded video frame count {frame_count} exceeds the supported 1..={MAX_DECODED_VIDEO_BATCH_FRAMES} budget."
        ));
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Decoded video frame dimensions overflow the RGBA byte size.".to_owned())
}

fn should_use_eager_video_frames(frame_bytes: usize, frame_count: usize) -> bool {
    frame_count <= MAX_EAGER_VIDEO_FRAMES
        && frame_bytes
            .checked_mul(frame_count)
            .is_some_and(|total| total <= MAX_EAGER_VIDEO_STREAM_BYTES)
}

fn has_nfvf_box_header(bytes: &[u8]) -> bool {
    let mut offsets = vec![0usize];
    if bytes.starts_with(LEGACY_STUB_MP4_HEADER) && bytes.len() > 32 {
        offsets.push(32);
    }
    for start_offset in offsets {
        let mut pos = start_offset;
        while pos + 8 <= bytes.len() {
            let box_size =
                u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                    as usize;
            if &bytes[pos + 4..pos + 8] == NFVF_FOURCC {
                return true;
            }
            if box_size < 8 || pos.saturating_add(box_size) > bytes.len() {
                break;
            }
            pos += box_size;
        }
    }
    false
}

fn try_decode_nfvf_box(bytes: &[u8]) -> Option<Result<DecodedVideoStream, String>> {
    let mut offsets = vec![0usize];
    if bytes.starts_with(LEGACY_STUB_MP4_HEADER) && bytes.len() > 32 {
        offsets.push(32);
    }
    for start_offset in offsets {
        let mut pos = start_offset;
        while pos + 8 <= bytes.len() {
            let box_size =
                u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                    as usize;
            let fourcc = &bytes[pos + 4..pos + 8];
            if fourcc == NFVF_FOURCC {
                if box_size < 21 || pos + box_size > bytes.len() {
                    return Some(Err(
                        "Truncated or corrupt nfvf video frame box in MP4 container.".into(),
                    ));
                }
                let payload = &bytes[pos + 8..pos + box_size];
                let version = payload[0];
                if version != 1 {
                    return Some(Err(format!("Unsupported nfvf box version {version}.")));
                }
                let w = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
                let h = u32::from_be_bytes([payload[5], payload[6], payload[7], payload[8]]);
                let fps_u16 = u16::from_be_bytes([payload[9], payload[10]]);
                let count = u16::from_be_bytes([payload[11], payload[12]]) as usize;
                if fps_u16 == 0 {
                    return Some(Err("Invalid dimensions or frame count in nfvf box.".into()));
                }
                let frame_bytes = match checked_on_demand_video_bounds(w, h, count) {
                    Ok(value) => value,
                    Err(e) => return Some(Err(e)),
                };
                let expected_payload = match count
                    .checked_mul(frame_bytes)
                    .and_then(|n| n.checked_add(13))
                {
                    Some(len) => len,
                    None => {
                        return Some(Err("nfvf video payload byte length overflow.".into()));
                    }
                };
                if payload.len() != expected_payload {
                    return Some(Err(
                        "Corrupt or truncated frame payload in MP4 video.".into()
                    ));
                }
                let fps = f32::from(fps_u16);
                let duration_ms = match checked_decoded_video_duration_ms(count, fps) {
                    Ok(duration_ms) => duration_ms,
                    Err(e) => return Some(Err(e)),
                };
                if should_use_eager_video_frames(frame_bytes, count) {
                    let frame_dur_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
                    let mut frames = Vec::with_capacity(count);
                    for idx in 0..count {
                        let start = 13 + idx * frame_bytes;
                        let rgba = payload[start..start + frame_bytes].to_vec();
                        let timestamp_ms = ((idx as f64 * 1000.0) / f64::from(fps)).round() as u32;
                        frames.push(DecodedVideoFrame {
                            timestamp_ms,
                            duration_ms: frame_dur_ms,
                            image: Arc::new(DecodedImage {
                                width: w,
                                height: h,
                                rgba: rgba.into(),
                            }),
                        });
                    }
                    return Some(Ok(DecodedVideoStream {
                        width: w,
                        height: h,
                        fps,
                        duration_ms,
                        frames,
                        on_demand: None,
                    }));
                }
                let backing: Arc<[u8]> = payload.to_vec().into();
                let backing_len = backing.len();
                let decoder = OnDemandVideoDecoder::new(
                    w,
                    h,
                    fps,
                    frame_bytes,
                    count,
                    backing_len,
                    OnDemandVideoBackend::Nfvf {
                        payload: backing,
                        frames_offset: 13,
                        frame_bytes,
                    },
                );
                return Some(Ok(DecodedVideoStream {
                    width: w,
                    height: h,
                    fps,
                    duration_ms,
                    frames: Vec::new(),
                    on_demand: Some(Arc::new(decoder)),
                }));
            }
            if box_size < 8 || pos + box_size > bytes.len() {
                break;
            }
            pos += box_size;
        }
    }
    None
}

fn parse_ffmpeg_fps_from_stderr(stderr: &str) -> Option<f32> {
    for line in stderr.lines() {
        if !line.contains("Video:") {
            continue;
        }
        for part in line.split(',') {
            let trimmed = part.trim();
            if let Some(num_str) = trimmed.strip_suffix("fps")
                && let Some(v) = parse_ffprobe_rate(num_str)
            {
                return Some(v);
            }
        }
    }
    None
}

#[derive(Debug)]
struct PamFrameSet {
    width: u32,
    height: u32,
    images: Vec<Arc<DecodedImage>>,
}

fn parse_pam_rgba_frames(reader: &mut impl BufRead) -> Result<PamFrameSet, String> {
    let mut images = Vec::new();
    let mut stream_w = 0u32;
    let mut stream_h = 0u32;
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("Failed reading PAM frame header: {e}"))?;
        if read == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if line.trim_end() != "P7" {
            return Err("Invalid PAM magic from ffmpeg.".into());
        }

        let mut w = 0u32;
        let mut h = 0u32;
        let mut depth = 0u32;
        let mut maxval = 0u32;
        let mut header_bytes = read;
        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|e| format!("Failed reading PAM frame header: {e}"))?;
            if read == 0 {
                return Err("Truncated PAM video frame header from ffmpeg.".into());
            }
            header_bytes = header_bytes.saturating_add(read);
            if header_bytes > 4096 {
                return Err("PAM video frame header exceeds 4096 bytes.".into());
            }
            let trimmed = line.trim_end();
            if trimmed == "ENDHDR" {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("WIDTH ") {
                w = rest.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("HEIGHT ") {
                h = rest.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("DEPTH ") {
                depth = rest.trim().parse::<u32>().unwrap_or(0);
            } else if let Some(rest) = trimmed.strip_prefix("MAXVAL ") {
                maxval = rest.trim().parse::<u32>().unwrap_or(0);
            }
        }

        if depth != 4 || maxval != 255 {
            return Err(format!(
                "Invalid PAM RGBA frame descriptor ({w}x{h}, depth {depth}, maxval {maxval})."
            ));
        }
        if stream_w != 0 && (w != stream_w || h != stream_h) {
            return Err(format!(
                "Video dimensions changed during decode ({}x{} to {w}x{h}).",
                stream_w, stream_h
            ));
        }
        let (payload_len, _) =
            checked_decoded_video_payload_len(w, h, images.len().saturating_add(1))?;
        let mut rgba = vec![0u8; payload_len];
        reader
            .read_exact(&mut rgba)
            .map_err(|e| format!("Truncated PAM video frame payload from ffmpeg: {e}"))?;
        if stream_w == 0 {
            stream_w = w;
            stream_h = h;
        }
        images.push(Arc::new(DecodedImage {
            width: w,
            height: h,
            rgba: rgba.into(),
        }));
    }
    if images.is_empty() {
        return Err("Video stream produced 0 decodable frames.".into());
    }

    Ok(PamFrameSet {
        width: stream_w,
        height: stream_h,
        images,
    })
}

fn finish_pam_rgba_frames(raw: PamFrameSet, fps: f32) -> Result<DecodedVideoStream, String> {
    let safe_fps = if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        24.0
    };
    let frame_dur_ms = ((1000.0 / f64::from(safe_fps)).round() as u32).max(1);
    let frames = raw
        .images
        .into_iter()
        .enumerate()
        .map(|(idx, image)| DecodedVideoFrame {
            timestamp_ms: ((idx as f64 * 1000.0) / f64::from(safe_fps)).round() as u32,
            duration_ms: frame_dur_ms,
            image,
        })
        .collect::<Vec<_>>();
    let duration_ms = checked_decoded_video_duration_ms(frames.len(), safe_fps)?;
    Ok(DecodedVideoStream {
        width: raw.width,
        height: raw.height,
        fps: safe_fps,
        duration_ms,
        frames,
        on_demand: None,
    })
}

#[derive(Debug)]
struct StagedMediaFile {
    path: PathBuf,
}

impl StagedMediaFile {
    fn create(name: &str, extension: &str, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_VIDEO_INPUT_BYTES {
            return Err(format!(
                "Temporary media '{name}' is {} bytes, exceeding the {} MiB staging budget.",
                bytes.len(),
                MAX_VIDEO_INPUT_BYTES / (1024 * 1024)
            ));
        }
        let epoch_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..64 {
            let sequence = VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "neoflash-media-stage-{}-{epoch_nanos}-{sequence}.{extension}",
                std::process::id()
            ));
            let mut file = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "Failed to create a secure temporary media file for '{name}': {error}"
                    ));
                }
            };
            let staged = Self { path };
            file.write_all(bytes).map_err(|error| {
                format!("Failed to stage temporary media file for '{name}': {error}")
            })?;
            file.flush().map_err(|error| {
                format!("Failed to flush temporary media file for '{name}': {error}")
            })?;
            return Ok(staged);
        }
        Err(format!(
            "Failed to allocate a unique temporary media file for '{name}' after 64 attempts."
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedMediaFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
struct BoundedProcessDiagnostics {
    head: Vec<u8>,
    tail: Vec<u8>,
}

fn read_bounded_process_diagnostics(
    mut reader: impl Read,
) -> std::io::Result<BoundedProcessDiagnostics> {
    let mut head = Vec::with_capacity(MAX_FFMPEG_DIAGNOSTIC_BYTES);
    let mut tail = Vec::with_capacity(MAX_FFMPEG_DIAGNOSTIC_BYTES);
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if head.len() < MAX_FFMPEG_DIAGNOSTIC_BYTES {
            let retain = (MAX_FFMPEG_DIAGNOSTIC_BYTES - head.len()).min(read);
            head.extend_from_slice(&chunk[..retain]);
        }
        let overflow = tail
            .len()
            .saturating_add(read)
            .saturating_sub(MAX_FFMPEG_DIAGNOSTIC_BYTES);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(&chunk[..read]);
    }
    Ok(BoundedProcessDiagnostics { head, tail })
}

/// Locate an `ffprobe` binary (`NEOFLASH_FFPROBE`, sibling of `ffmpeg`, bundled `Resources/media/bin/ffprobe`, `PATH`, or system fallbacks).
pub fn detect_ffprobe(ffmpeg: &Path) -> Option<PathBuf> {
    if let Ok(custom) = env::var("NEOFLASH_FFPROBE") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    for sibling_name in ["ffprobe", "ffprobe.exe"] {
        let sibling = ffmpeg.with_file_name(sibling_name);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Some(bundled) = detect_bundled_ffprobe() {
        return Some(bundled);
    }
    if env::var("NEOFLASH_DISABLE_SYSTEM_FFMPEG").ok().as_deref() == Some("1") {
        return None;
    }
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for name in ["ffprobe", "ffprobe.exe"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for fallback in [
        "/opt/homebrew/bin/ffprobe",
        "/usr/local/bin/ffprobe",
        "/usr/bin/ffprobe",
        r"C:\Program Files\ffmpeg\bin\ffprobe.exe",
    ] {
        let path = PathBuf::from(fallback);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct VideoProbeMetadata {
    width: u32,
    height: u32,
    fps: f32,
}

#[derive(Clone, Copy, Debug)]
struct ProbedStreamMetadata {
    width: u32,
    height: u32,
    fps: f32,
    frame_count: Option<usize>,
    duration_ms: Option<u64>,
}

fn parse_ffprobe_rate(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    if let Some((numerator, denominator)) = trimmed.split_once('/') {
        let numerator = numerator.parse::<f64>().ok()?;
        let denominator = denominator.parse::<f64>().ok()?;
        if denominator == 0.0 {
            return None;
        }
        let rate = numerator / denominator;
        return (rate.is_finite() && rate > 0.0 && rate <= f32::MAX as f64).then_some(rate as f32);
    }
    trimmed
        .parse::<f32>()
        .ok()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
}

fn parse_ffprobe_raw_fields(text: &str) -> Result<ProbedStreamMetadata, String> {
    let mut width = None;
    let mut height = None;
    let mut avg_fps = None;
    let mut fallback_fps = None;
    let mut duration_ms = None;
    let mut frame_count = None;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        match key {
            "width" => width = value.parse::<u32>().ok(),
            "height" => height = value.parse::<u32>().ok(),
            "avg_frame_rate" => avg_fps = parse_ffprobe_rate(value),
            "r_frame_rate" => fallback_fps = parse_ffprobe_rate(value),
            "duration" if duration_ms.is_none() => {
                duration_ms = value
                    .parse::<f64>()
                    .ok()
                    .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
                    .map(|seconds| (seconds * 1000.0).ceil() as u64);
            }
            "nb_frames" => frame_count = value.parse::<usize>().ok().filter(|count| *count > 0),
            _ => {}
        }
    }
    let width = width.ok_or_else(|| "ffprobe did not report video width.".to_owned())?;
    let height = height.ok_or_else(|| "ffprobe did not report video height.".to_owned())?;
    let fps = avg_fps.or(fallback_fps).ok_or_else(|| {
        "ffprobe did not report a usable video frame rate; timing cannot be preserved.".to_owned()
    })?;
    if let Some(duration_ms) = duration_ms
        && duration_ms > MAX_DECODED_VIDEO_DURATION_MS
    {
        return Err(format!(
            "Video duration {duration_ms} ms exceeds the supported {MAX_DECODED_VIDEO_DURATION_MS} ms budget."
        ));
    }
    if frame_count.is_none()
        && let Some(duration_ms) = duration_ms
    {
        let estimate = ((duration_ms as f64 / 1000.0) * f64::from(fps)).ceil();
        if estimate.is_finite() && estimate > 0.0 && estimate <= usize::MAX as f64 {
            frame_count = Some(estimate as usize);
        }
    }
    Ok(ProbedStreamMetadata {
        width,
        height,
        fps,
        frame_count,
        duration_ms,
    })
}

#[cfg(test)]
fn parse_ffprobe_metadata(text: &str) -> Result<VideoProbeMetadata, String> {
    let raw = parse_ffprobe_raw_fields(text)?;
    let bounded_frame_count = raw.frame_count.unwrap_or(1);
    checked_decoded_video_payload_len(raw.width, raw.height, bounded_frame_count)?;
    Ok(VideoProbeMetadata {
        width: raw.width,
        height: raw.height,
        fps: raw.fps,
    })
}

fn parse_ffprobe_stream_metadata(text: &str) -> Result<ProbedStreamMetadata, String> {
    let raw = parse_ffprobe_raw_fields(text)?;
    checked_on_demand_video_bounds(raw.width, raw.height, raw.frame_count.unwrap_or(1))?;
    Ok(raw)
}

fn probe_stream_metadata(
    ffmpeg: &Path,
    input: &Path,
) -> Result<Option<ProbedStreamMetadata>, String> {
    let Some(ffprobe) = detect_ffprobe(ffmpeg) else {
        return Ok(None);
    };
    let mut cmd = Command::new(&ffprobe);
    configure_media_subprocess_command(&mut cmd, &ffprobe);
    let mut child = cmd
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height,avg_frame_rate,r_frame_rate,nb_frames,duration:format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=0")
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to launch ffprobe: {error}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffprobe metadata.".into());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffprobe diagnostics.".into());
        }
    };
    let stdout_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stdout));
    let stderr_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stderr));
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(format!("Failed waiting for ffprobe: {error}"));
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "ffprobe metadata reader panicked.".to_owned())?
        .map_err(|error| format!("Failed reading ffprobe metadata: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "ffprobe diagnostics reader panicked.".to_owned())?
        .map_err(|error| format!("Failed reading ffprobe diagnostics: {error}"))?;
    if !status.success() {
        let diagnostics = String::from_utf8_lossy(&stderr.tail);
        return Err(format!(
            "ffprobe could not inspect the video safely: {}",
            diagnostics.trim()
        ));
    }
    let metadata = String::from_utf8_lossy(&stdout.head);
    parse_ffprobe_stream_metadata(&metadata).map(Some)
}

fn decode_ffmpeg_pam_window(
    ffmpeg: &Path,
    input: &Path,
    seek_sec: Option<f64>,
    max_frames: usize,
    expected_dims: Option<(u32, u32)>,
) -> Result<PamFrameSet, String> {
    let mut cmd = Command::new(ffmpeg);
    configure_media_subprocess_command(&mut cmd, ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-nostdin")
        .arg("-max_pixels")
        .arg(
            u64::from(MAX_DECODED_VIDEO_WIDTH)
                .saturating_mul(u64::from(MAX_DECODED_VIDEO_HEIGHT))
                .to_string(),
        );
    if let Some(sec) = seek_sec {
        cmd.arg("-ss").arg(format!("{:.6}", sec.max(0.0)));
    }
    cmd.arg("-i").arg(input);
    if max_frames > 0 {
        cmd.arg("-frames:v").arg(max_frames.to_string());
    }
    let mut child = cmd
        .arg("-f")
        .arg("image2pipe")
        .arg("-c:v")
        .arg("pam")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run ffmpeg for windowed video decode: {e}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffmpeg windowed frames.".into());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to capture ffmpeg windowed diagnostics.".into());
        }
    };
    let stderr_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stderr));
    let mut stdout = BufReader::new(stdout);
    let parsed = parse_pam_rgba_frames(&mut stdout);
    let allow_eof_empty = seek_sec.is_some()
        && parsed
            .as_ref()
            .is_err_and(|msg| msg.contains("0 decodable frames"));
    if parsed.is_err() && !allow_eof_empty {
        let _ = child.kill();
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(format!("Failed waiting for ffmpeg window decode: {error}"));
        }
    };
    let stderr_capture = stderr_reader
        .join()
        .map_err(|_| "ffmpeg diagnostics reader panicked.".to_owned())?
        .map_err(|e| format!("Failed reading ffmpeg diagnostics: {e}"))?;
    if !status.success() {
        let stderr_tail = String::from_utf8_lossy(&stderr_capture.tail);
        let last_err = stderr_tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("corrupt or unsupported video stream");
        return Err(format!(
            "Video failed codec/container decoding: {}",
            last_err.trim()
        ));
    }
    if allow_eof_empty {
        let (w, h) = expected_dims.unwrap_or((1, 1));
        return Ok(PamFrameSet {
            width: w,
            height: h,
            images: Vec::new(),
        });
    }
    let raw = parsed?;
    if let Some((exp_w, exp_h)) = expected_dims
        && (raw.width != exp_w || raw.height != exp_h)
    {
        return Err(format!(
            "Video changed dimensions between probe ({exp_w}x{exp_h}) and decode ({}x{}).",
            raw.width, raw.height
        ));
    }
    Ok(raw)
}

fn decode_ffmpeg_backing(
    name: &str,
    ffmpeg: PathBuf,
    backing: FfmpegMediaBacking,
) -> Result<DecodedVideoStream, String> {
    let probed_metadata = probe_stream_metadata(&ffmpeg, backing.path())?;
    if let Some(meta) = probed_metadata
        && let Some(frame_count) = meta.frame_count
    {
        let frame_bytes = checked_on_demand_video_bounds(meta.width, meta.height, frame_count)?;
        if !should_use_eager_video_frames(frame_bytes, frame_count) {
            let duration_ms = match meta.duration_ms {
                Some(dur_ms) if dur_ms > 0 => u32::try_from(dur_ms).unwrap_or(u32::MAX),
                _ => checked_decoded_video_duration_ms(frame_count, meta.fps)?,
            };
            let decoder = OnDemandVideoDecoder::new(
                meta.width,
                meta.height,
                meta.fps,
                frame_bytes,
                frame_count,
                0,
                OnDemandVideoBackend::Ffmpeg { ffmpeg, backing },
            );
            let initial = decoder
                .decode_window_frames(0, decoder.window_fetch_frames)
                .map_err(|e| format!("Video '{name}' initial window decode failed: {e}"))?;
            if initial.is_empty() {
                return Err(format!("Video '{name}' produced 0 decodable frames."));
            }
            if let Ok(mut ring) = decoder.ring.lock() {
                ring.window_fetches = 1;
                decoder.insert_decoded_batch(&mut ring, initial);
            }
            return Ok(DecodedVideoStream {
                width: meta.width,
                height: meta.height,
                fps: meta.fps,
                duration_ms,
                frames: Vec::new(),
                on_demand: Some(Arc::new(decoder)),
            });
        }
    }

    if let Some(meta) = probed_metadata {
        checked_decoded_video_payload_len(meta.width, meta.height, meta.frame_count.unwrap_or(1))?;
    }

    let mut cmd = Command::new(&ffmpeg);
    configure_media_subprocess_command(&mut cmd, &ffmpeg);
    let mut child = cmd
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-max_pixels")
        .arg(
            u64::from(MAX_DECODED_VIDEO_WIDTH)
                .saturating_mul(u64::from(MAX_DECODED_VIDEO_HEIGHT))
                .to_string(),
        )
        .arg("-i")
        .arg(backing.path())
        .arg("-f")
        .arg("image2pipe")
        .arg("-c:v")
        .arg("pam")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("pipe:1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run ffmpeg for video '{name}': {e}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Failed to capture ffmpeg frames for video '{name}'."
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "Failed to capture ffmpeg diagnostics for video '{name}'."
            ));
        }
    };
    let stderr_reader = std::thread::spawn(move || read_bounded_process_diagnostics(stderr));

    // Parse frames as ffmpeg emits them. This avoids retaining the complete raw
    // PAM pipe and then copying it a second time into decoded frames.
    let mut stdout = BufReader::new(stdout);
    let parsed = parse_pam_rgba_frames(&mut stdout);
    if parsed.is_err() {
        let _ = child.kill();
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err(format!(
                "Failed waiting for ffmpeg video decode '{name}': {error}"
            ));
        }
    };
    let stderr_capture = stderr_reader
        .join()
        .map_err(|_| format!("ffmpeg diagnostics reader panicked for video '{name}'."))?
        .map_err(|e| format!("Failed reading ffmpeg diagnostics for video '{name}': {e}"))?;

    let stderr_tail = String::from_utf8_lossy(&stderr_capture.tail);
    if !status.success() {
        let last_err = stderr_tail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("corrupt or unsupported video stream");
        return Err(format!(
            "Video '{name}' failed codec/container decoding: {}",
            last_err.trim()
        ));
    }
    let raw = parsed.map_err(|e| format!("Video '{name}' frame extraction failed: {e}"))?;
    if let Some(metadata) = probed_metadata
        && (raw.width != metadata.width || raw.height != metadata.height)
    {
        return Err(format!(
            "Video '{name}' changed dimensions between probe ({}x{}) and decode ({}x{}).",
            metadata.width, metadata.height, raw.width, raw.height
        ));
    }
    let stderr_head = String::from_utf8_lossy(&stderr_capture.head);
    let fps = probed_metadata
        .map(|metadata| metadata.fps)
        .or_else(|| parse_ffmpeg_fps_from_stderr(&stderr_head))
        .ok_or_else(|| {
            format!(
                "Video '{name}' frame rate could not be determined safely; timing was not guessed."
            )
        })?;
    finish_pam_rgba_frames(raw, fps)
        .map_err(|e| format!("Video '{name}' frame extraction failed: {e}"))
}

fn decode_video_stream_with_ffmpeg(
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<DecodedVideoStream, String> {
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        format!(
            "Video '{name}' requires ffmpeg to decode compressed video frames, but ffmpeg was not found."
        )
    })?;
    let ext = if mime.contains("webm") || name.ends_with(".webm") {
        "webm"
    } else if mime.contains("ogg") || name.ends_with(".ogv") || name.ends_with(".ogg") {
        "ogv"
    } else {
        "mp4"
    };
    let staged_video = StagedMediaFile::create(name, ext, bytes)?;
    decode_ffmpeg_backing(name, ffmpeg, FfmpegMediaBacking::Staged(staged_video))
}

fn decode_avi_stream_bytes(name: &str, bytes: &[u8]) -> Option<Result<DecodedVideoStream, String>> {
    let container = neoflash_core::index_avi_container(bytes).ok()?;
    if !container.info.supports_pure_rust_video() || container.frames.is_empty() {
        return None;
    }
    let count = container.frames.len();
    let frame_bytes =
        match checked_on_demand_video_bounds(container.info.width, container.info.height, count) {
            Ok(fb) => fb,
            Err(e) => return Some(Err(format!("Video '{name}' failed decoding: {e}"))),
        };
    let fps = (container.info.fps as f32).max(1.0);
    let duration_ms = container.info.duration_ms();
    if u64::from(duration_ms) > MAX_DECODED_VIDEO_DURATION_MS {
        return Some(Err(format!(
            "Video '{name}' duration {duration_ms} ms exceeds the supported {MAX_DECODED_VIDEO_DURATION_MS} ms budget."
        )));
    }
    if should_use_eager_video_frames(frame_bytes, count) {
        if let Ok(avi) = neoflash_core::decode_avi_bytes(bytes) {
            let default_dur_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
            let frames = avi
                .frames
                .into_iter()
                .map(|(timestamp_ms, image)| DecodedVideoFrame {
                    timestamp_ms,
                    duration_ms: default_dur_ms,
                    image,
                })
                .collect();
            return Some(Ok(DecodedVideoStream {
                width: avi.info.width,
                height: avi.info.height,
                fps,
                duration_ms,
                frames,
                on_demand: None,
            }));
        }
        return None;
    }

    let backing: Arc<[u8]> = bytes.to_vec().into();
    let backing_len = backing.len();
    let w = container.info.width;
    let h = container.info.height;
    let decoder = OnDemandVideoDecoder::new(
        w,
        h,
        fps,
        frame_bytes,
        count,
        backing_len,
        OnDemandVideoBackend::Avi {
            bytes: backing,
            container,
        },
    );
    let initial = match decoder.decode_window_frames(0, decoder.window_fetch_frames) {
        Ok(frames) if !frames.is_empty() => frames,
        Ok(_) => {
            return Some(Err(format!(
                "Video '{name}' produced 0 decodable AVI frames."
            )));
        }
        Err(e) => return Some(Err(format!("Video '{name}' AVI decode failed: {e}"))),
    };
    if let Ok(mut ring) = decoder.ring.lock() {
        ring.window_fetches = 1;
        decoder.insert_decoded_batch(&mut ring, initial);
    }
    Some(Ok(DecodedVideoStream {
        width: w,
        height: h,
        fps,
        duration_ms,
        frames: Vec::new(),
        on_demand: Some(Arc::new(decoder)),
    }))
}

/// Decode a video byte buffer (`MP4`, `WebM`, `Ogg`, `AVI`, or deterministic `nfvf` MP4) into RGBA8 frames (`T-201` / `T-229`).
pub fn decode_video_stream_bytes(
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<DecodedVideoStream, String> {
    if bytes.len() > MAX_VIDEO_INPUT_BYTES {
        return Err(format!(
            "Video '{name}' is {} bytes, exceeding the {} MiB compressed-input budget.",
            bytes.len(),
            MAX_VIDEO_INPUT_BYTES / (1024 * 1024)
        ));
    }
    let diags = inspect_video_resource(name, mime, bytes, None);
    if let Some(err_diag) = diags.iter().find(|d| d.is_error) {
        return Err(err_diag.message.clone());
    }
    if let Some(nfvf_res) = try_decode_nfvf_box(bytes) {
        return nfvf_res.map_err(|e| format!("Video '{name}' failed decoding: {e}"));
    }
    if neoflash_core::is_avi_bytes(bytes)
        && let Some(avi_res) = decode_avi_stream_bytes(name, bytes)
    {
        return avi_res;
    }
    decode_video_stream_with_ffmpeg(name, mime, bytes)
}

fn decode_external_video_file(
    candidate: &Path,
    label: &str,
    mime: &str,
) -> Result<DecodedVideoStream, String> {
    let file = fs::File::open(candidate)
        .map_err(|error| format!("Failed to open external video '{label}': {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Failed to inspect external video '{label}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("External video '{label}' is not a regular file."));
    }
    if metadata.len() > MAX_VIDEO_INPUT_BYTES as u64 {
        return Err(format!(
            "External video '{label}' is {} bytes, exceeding the {} MiB input budget.",
            metadata.len(),
            MAX_VIDEO_INPUT_BYTES / (1024 * 1024)
        ));
    }
    let mut prefix = Vec::with_capacity(65_536.min(metadata.len() as usize));
    file.take(65_536)
        .read_to_end(&mut prefix)
        .map_err(|error| format!("Failed to read external video '{label}': {error}"))?;
    if prefix.is_empty() {
        return Err(format!("External video '{label}' is empty."));
    }
    if has_nfvf_box_header(&prefix) || neoflash_core::is_avi_bytes(&prefix) {
        let bytes = read_bounded_video_file(candidate, label)?;
        return decode_video_stream_bytes(label, mime, &bytes);
    }
    // Inspect full container boxes when small, or prefix for large external MP4/WebM files so
    // explicit codec diagnostics (`hvc1`, unsupported container) fire without reading 512 MiB into RAM.
    if metadata.len() <= 4 * 1024 * 1024 {
        let bytes = read_bounded_video_file(candidate, label)?;
        let diags = inspect_video_resource(label, mime, &bytes, None);
        if let Some(err_diag) = diags.iter().find(|d| d.is_error) {
            return Err(err_diag.message.clone());
        }
    } else {
        let diags = inspect_video_resource(label, mime, &prefix, None);
        if let Some(err_diag) = diags
            .iter()
            .find(|d| d.is_error && !d.message.contains("truncated"))
        {
            return Err(err_diag.message.clone());
        }
    }
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        format!(
            "Video '{label}' requires ffmpeg to decode compressed video frames, but ffmpeg was not found."
        )
    })?;
    decode_ffmpeg_backing(
        label,
        ffmpeg,
        FfmpegMediaBacking::ExternalPath(candidate.to_path_buf()),
    )
}

/// Bounded on-demand byte cache for `VideoDeploymentMode::Progressive` sources (`T-274`).
fn staged_progressive_byte_cache() -> &'static Mutex<HashMap<String, Arc<[u8]>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<[u8]>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Stage or update raw container bytes for a progressive video source URL, resource ID, or path.
pub fn stage_progressive_video_bytes(key: impl Into<String>, bytes: Vec<u8>) {
    let key = key.into();
    if key.trim().is_empty() || bytes.is_empty() {
        return;
    }
    if let Ok(mut map) = staged_progressive_byte_cache().lock() {
        if map.len() >= 16
            && !map.contains_key(&key)
            && let Some(first_key) = map.keys().next().cloned()
        {
            map.remove(&first_key);
        }
        map.insert(key, Arc::from(bytes));
    }
}

/// Look up staged container bytes for a progressive video source URL, resource ID, or path.
pub fn lookup_staged_progressive_video_bytes(key: &str) -> Option<Arc<[u8]>> {
    if key.trim().is_empty() {
        return None;
    }
    staged_progressive_byte_cache()
        .lock()
        .ok()
        .and_then(|map| map.get(key).cloned())
}

/// Resolve a local or staged progressive file path from `url_or_path` / `resource_path` and `base_dir` (`T-274`).
pub fn resolve_progressive_local_path(
    url_or_path: &str,
    resource_path: Option<&str>,
    base_dir: Option<&Path>,
) -> Option<PathBuf> {
    fn extract_path_candidates(raw: &str, out: &mut Vec<String>) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed).trim();
        if let Some(rest) = without_query
            .strip_prefix("file://localhost")
            .or_else(|| without_query.strip_prefix("file://"))
        {
            if !rest.is_empty() {
                out.push(rest.to_owned());
            }
            return;
        }
        if let Some((_, after_scheme)) = without_query.split_once("://") {
            if let Some((_, url_path)) = after_scheme.split_once('/') {
                let clean = url_path.trim_start_matches('/');
                if !clean.is_empty() {
                    out.push(clean.to_owned());
                    if let Some(fname) = Path::new(clean).file_name().and_then(|s| s.to_str()) {
                        out.push(fname.to_owned());
                    }
                }
            }
        } else {
            out.push(without_query.to_owned());
            if let Some(fname) = Path::new(without_query)
                .file_name()
                .and_then(|s| s.to_str())
            {
                out.push(fname.to_owned());
            }
        }
    }

    let mut rel_candidates = Vec::new();
    extract_path_candidates(url_or_path, &mut rel_candidates);
    if let Some(rp) = resource_path {
        extract_path_candidates(rp, &mut rel_candidates);
    }

    for rel in &rel_candidates {
        let p = Path::new(rel);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        if let Some(dir) = base_dir {
            for candidate in [
                dir.join(rel),
                dir.join("media").join(rel),
                dir.join("video").join(rel),
                dir.join("clips").join(rel),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Synthesize a deterministic multi-frame progressive video stream when a progressive asset has
/// known metadata or a poster image in headless tests / preview (`T-274`).
pub fn synthesize_progressive_video_stream(
    project: &Project,
    resource: Option<&neoflash_core::ResourceUnit>,
    spec: &VideoSourceSpec,
) -> Result<DecodedVideoStream, String> {
    let mut poster_rgba: Option<(u32, u32, Vec<u8>)> = None;
    for res in &project.resources {
        if res.is_image()
            && !res.data_base64.is_empty()
            && let Ok(bytes) = res.decoded_bytes()
            && let Ok(img) = image::load_from_memory(&bytes)
        {
            let rgba = img.to_rgba8();
            let w = rgba.width().clamp(8, 256);
            let h = rgba.height().clamp(8, 256);
            let resized = if w != rgba.width() || h != rgba.height() {
                image::imageops::resize(&rgba, w, h, FilterType::Triangle).into_raw()
            } else {
                rgba.into_raw()
            };
            poster_rgba = Some((w, h, resized));
            break;
        }
    }

    let (width, height) = if let Some((pw, ph, _)) = &poster_rgba {
        (*pw, *ph)
    } else {
        let mut found_dims = None;
        'outer: for sym in &project.symbols {
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        if let ElementKind::Video { width, height } = el.kind {
                            let w = (width.round() as u32).clamp(16, 128);
                            let h = (height.round() as u32).clamp(16, 128);
                            found_dims = Some((w, h));
                            break 'outer;
                        }
                    }
                }
            }
        }
        found_dims.unwrap_or((32, 32))
    };

    let fps = project.fps.clamp(1, 60);
    let max_tl_frames = project
        .symbols
        .iter()
        .map(|s| s.timeline.frame_count)
        .max()
        .unwrap_or(12)
        .clamp(8, 48) as usize;

    let mut seed = 0x811c_9dc5u32;
    for b in spec
        .url_or_path
        .bytes()
        .chain(resource.map_or("", |r| r.id.as_str()).bytes())
    {
        seed ^= u32::from(b);
        seed = seed.wrapping_mul(0x0100_0193);
    }

    let palette: [[u8; 4]; 6] = [
        [235, 55, 45, 255],
        [40, 205, 85, 255],
        [45, 115, 240, 255],
        [240, 200, 35, 255],
        [195, 65, 225, 255],
        [35, 205, 215, 255],
    ];

    let mut raw_frames: Vec<Vec<u8>> = Vec::with_capacity(max_tl_frames);
    for frame_idx in 0..max_tl_frames {
        let base_color = palette[(frame_idx + (seed as usize % palette.len())) % palette.len()];
        let mut frame_buf = if let Some((_, _, ref poster)) = poster_rgba {
            poster.clone()
        } else {
            vec![0u8; (width * height * 4) as usize]
        };
        let band_x = ((frame_idx as u32) * 5) % width.max(1);
        for y in 0..height {
            for x in 0..width {
                let off = ((y * width + x) * 4) as usize;
                let in_band = x.abs_diff(band_x) <= 2 || y == (frame_idx as u32 % height.max(1));
                if poster_rgba.is_some() {
                    if in_band {
                        frame_buf[off] = base_color[0];
                        frame_buf[off + 1] = base_color[1];
                        frame_buf[off + 2] = base_color[2];
                        frame_buf[off + 3] = 255;
                    } else {
                        frame_buf[off] =
                            ((u16::from(frame_buf[off]) + u16::from(base_color[0])) / 2) as u8;
                        frame_buf[off + 1] =
                            ((u16::from(frame_buf[off + 1]) + u16::from(base_color[1])) / 2) as u8;
                        frame_buf[off + 2] =
                            ((u16::from(frame_buf[off + 2]) + u16::from(base_color[2])) / 2) as u8;
                        frame_buf[off + 3] = 255;
                    }
                } else {
                    let delta = if in_band { 20u8 } else { 0u8 };
                    frame_buf[off] = base_color[0].saturating_add(delta);
                    frame_buf[off + 1] = base_color[1].saturating_add(delta);
                    frame_buf[off + 2] = base_color[2].saturating_add(delta);
                    frame_buf[off + 3] = 255;
                }
            }
        }
        raw_frames.push(frame_buf);
    }

    let raw_slices: Vec<&[u8]> = raw_frames.iter().map(Vec::as_slice).collect();
    let nfvf_bytes = encode_deterministic_mp4_rgba_frames(width, height, fps as u16, &raw_slices);
    let label = resource
        .map(|r| r.path.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(spec.url_or_path.as_str());
    decode_video_stream_bytes(label, &spec.mime_type, &nfvf_bytes)
}

/// Resolve and decode a timeline video span/source from `project` (`Embedded`, `External`, or `Progressive`, `T-201` / `T-274`).
///
/// For `VideoDeploymentMode::Progressive`, resolves the video byte stream from:
/// 1. Embedded/cached resource bytes if present in `Project::resources` or staged progressive byte cache, OR
/// 2. Local/staged progressive file path resolved against `base_dir` or local path, OR
/// 3. Deterministic synthesized progressive frame stream when a progressive asset has known metadata/poster.
pub fn resolve_and_decode_project_video(
    project: &Project,
    resource_id: &str,
    source_override: Option<&VideoSourceSpec>,
    base_dir: Option<&Path>,
) -> Result<Arc<DecodedVideoStream>, String> {
    let spec = source_override
        .cloned()
        .or_else(|| resolve_video_resource_source(project, resource_id))
        .ok_or_else(|| format!("Video resource '{resource_id}' does not exist in project."))?;

    let resource = project.resources.iter().find(|r| r.id == resource_id);

    match spec.mode {
        VideoDeploymentMode::Progressive => {
            // 1. Embedded/cached resource bytes in Project::resources or staged progressive byte cache:
            if let Some(res) = resource
                && !res.data_base64.is_empty()
            {
                let bytes = res
                    .decoded_bytes()
                    .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
                let label = if res.path.is_empty() {
                    spec.url_or_path.as_str()
                } else {
                    res.path.as_str()
                };
                return decode_video_stream_bytes(label, &spec.mime_type, &bytes).map(Arc::new);
            }
            if let Some(staged) = lookup_staged_progressive_video_bytes(&spec.url_or_path)
                .or_else(|| lookup_staged_progressive_video_bytes(resource_id))
                .or_else(|| resource.and_then(|r| lookup_staged_progressive_video_bytes(&r.path)))
            {
                return decode_video_stream_bytes(&spec.url_or_path, &spec.mime_type, &staged)
                    .map(Arc::new);
            }
            if let Some(companion) = project.resources.iter().find(|r| {
                r.is_video()
                    && !r.data_base64.is_empty()
                    && (r.path == spec.url_or_path
                        || resource.is_some_and(|res| !res.path.is_empty() && r.path == res.path))
            }) {
                let bytes = companion
                    .decoded_bytes()
                    .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
                return decode_video_stream_bytes(&companion.path, &spec.mime_type, &bytes)
                    .map(Arc::new);
            }

            // 2. Local/staged progressive file path (resolved against project parent directory or staged local path):
            if let Some(local_path) = resolve_progressive_local_path(
                &spec.url_or_path,
                resource.map(|r| r.path.as_str()),
                base_dir,
            ) {
                if let Ok(bytes) = fs::read(&local_path)
                    && bytes.len() <= 4 * 1024 * 1024
                {
                    stage_progressive_video_bytes(&spec.url_or_path, bytes);
                }
                return decode_external_video_file(&local_path, &spec.url_or_path, &spec.mime_type)
                    .map(Arc::new);
            }

            // Validate progressive URL before synthesizing a deterministic stream:
            neoflash_core::validate_progressive_video_url(&spec.url_or_path).map_err(|err| {
                format!(
                    "Timeline video '{resource_id}' uses Progressive deployment ('{}') with invalid URL: {err}",
                    spec.url_or_path
                )
            })?;

            // 3. Deterministic synthesized progressive frame stream when a progressive asset has
            // known metadata (trim/duration/cue points) or a poster image in headless tests:
            let has_poster = project
                .resources
                .iter()
                .any(|r| r.is_image() && !r.data_base64.is_empty());
            let has_known_span_metadata = project.symbols.iter().any(|sym| {
                sym.timeline.layers.iter().any(|layer| {
                    layer.keyframes.iter().any(|kf| {
                        kf.video.as_ref().is_some_and(|v| {
                            v.resource == resource_id
                                && (v.end_offset_ms.is_some()
                                    || v.start_offset_ms > 0
                                    || !v.cue_points.is_empty())
                        })
                    })
                })
            });
            if let Some(res) = resource
                && res.is_video()
                && (has_poster || has_known_span_metadata)
            {
                return synthesize_progressive_video_stream(project, Some(res), &spec)
                    .map(Arc::new);
            }

            Err(format!(
                "Timeline video '{resource_id}' uses Progressive deployment ('{}'), which cannot be decoded in offline video/sequence export (video-cors-remote). Switch deployment to Embedded or External local file, or provide staged bytes / known metadata.",
                spec.url_or_path
            ))
        }
        VideoDeploymentMode::External => {
            validate_external_video_path(&spec.url_or_path).map_err(|err| {
                format!(
                    "Video '{resource_id}' has invalid external path '{}': {err}",
                    spec.url_or_path
                )
            })?;
            if let Some(dir) = base_dir {
                let candidate = dir.join(&spec.url_or_path);
                if candidate.exists() {
                    return decode_external_video_file(
                        &candidate,
                        &spec.url_or_path,
                        &spec.mime_type,
                    )
                    .map(Arc::new);
                }
                return Err(format!(
                    "External video file '{}' for resource '{resource_id}' was not found at '{}'. Ensure the relative media file exists beside the project or switch deployment to Embedded.",
                    spec.url_or_path,
                    candidate.display()
                ));
            }
            if let Some(res) = resource
                && !res.data_base64.is_empty()
            {
                let bytes = res
                    .decoded_bytes()
                    .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
                return decode_video_stream_bytes(&spec.url_or_path, &spec.mime_type, &bytes)
                    .map(Arc::new);
            }
            Err(format!(
                "External video file '{}' for resource '{resource_id}' was not found (no project base directory available).",
                spec.url_or_path
            ))
        }
        VideoDeploymentMode::Embedded => {
            let res = resource.ok_or_else(|| {
                format!("Embedded video resource '{resource_id}' does not exist in project.")
            })?;
            let bytes = res
                .decoded_bytes()
                .map_err(|e| format!("Video '{resource_id}' base64 decode failed: {e}"))?;
            let label = if res.path.is_empty() {
                resource_id
            } else {
                res.path.as_str()
            };
            decode_video_stream_bytes(label, &res.media_type, &bytes).map(Arc::new)
        }
    }
}

fn collect_reachable_video_spans(
    project: &Project,
    root_symbol_id: &str,
) -> Vec<(String, VideoSourceSpec)> {
    let mut visited_symbols = HashSet::new();
    let mut stack = vec![root_symbol_id.to_owned()];
    let mut spans = Vec::new();
    let mut seen_keys = HashSet::new();

    while let Some(sym_id) = stack.pop() {
        if !visited_symbols.insert(sym_id.clone()) {
            continue;
        }
        let Some(symbol) = project.symbol(&sym_id) else {
            continue;
        };
        for layer in &symbol.timeline.layers {
            if !layer_effectively_visible(&symbol.timeline, layer)
                || matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder)
            {
                continue;
            }
            for key in &layer.keyframes {
                for el in &key.elements {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                        stack.push(symbol_id.clone());
                    }
                }
                for element in key
                    .elements
                    .iter()
                    .filter(|element| matches!(element.kind, ElementKind::Video { .. }))
                {
                    if let Some((_, span)) =
                        neoflash_core::effective_video_span_for_element_at_frame(
                            layer,
                            key.frame,
                            &element.id,
                        )
                        && span.sync != VideoSyncMode::Stop
                        && let Some(spec) = resolve_video_source(project, span)
                    {
                        let res_id = if span.resource.is_empty() {
                            spec.url_or_path.clone()
                        } else {
                            span.resource.clone()
                        };
                        let key_tuple = (res_id.clone(), spec.mode, spec.url_or_path.clone());
                        if seen_keys.insert(key_tuple) {
                            spans.push((res_id, spec));
                        }
                    }
                }
            }
        }
    }
    spans
}

fn video_cache_key(resource_id: &str, source: Option<&VideoSourceSpec>) -> String {
    match source {
        Some(source) => format!(
            "{}:{resource_id}|{}|{}:{}|{}:{}",
            resource_id.len(),
            source.mode.as_str(),
            source.url_or_path.len(),
            source.url_or_path,
            source.mime_type.len(),
            source.mime_type,
        ),
        None => resource_id.to_owned(),
    }
}

fn placement_video_cache_key(placement: &ResolvedVideoPlacement) -> String {
    video_cache_key(&placement.resource, placement.source.as_ref())
}

fn insert_decoded_video_stream_with_limit(
    cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    key: String,
    stream: Arc<DecodedVideoStream>,
    limit: usize,
) -> Result<(), String> {
    insert_decoded_video_stream_with_limits(
        cache,
        key,
        stream,
        limit,
        MAX_DECODED_VIDEO_BATCH_FRAMES,
    )
}

fn insert_decoded_video_stream_with_limits(
    cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    key: String,
    stream: Arc<DecodedVideoStream>,
    byte_limit: usize,
    frame_limit: usize,
) -> Result<(), String> {
    if cache.contains_key(&key) {
        return Ok(());
    }
    const ARC_ALLOCATION_OVERHEAD: usize = 2 * std::mem::size_of::<usize>();
    const CACHE_BUCKET_OVERHEAD: usize = std::mem::size_of::<usize>();

    let mut seen_streams = HashSet::new();
    let mut seen_images = HashSet::new();
    let mut seen_decoders = HashSet::new();
    let mut total_bytes = 0usize;
    let mut total_frames = 0usize;

    let mut add_bytes = |bytes: usize, context: &str| -> Result<(), String> {
        total_bytes = total_bytes
            .checked_add(bytes)
            .ok_or_else(|| format!("Decoded timeline-video cache {context} byte size overflow."))?;
        if total_bytes > byte_limit {
            return Err(format!(
                "Decoded timeline videos require at least {total_bytes} retained bytes including frame records and cache structures, exceeding the {} MiB cache budget. Shorten, resize, or use fewer simultaneous videos.",
                byte_limit / (1024 * 1024)
            ));
        }
        Ok(())
    };

    for cache_key in cache.keys().chain(std::iter::once(&key)) {
        add_bytes(
            std::mem::size_of::<(String, Option<Arc<DecodedVideoStream>>)>()
                .checked_add(cache_key.capacity())
                .and_then(|bytes| bytes.checked_add(CACHE_BUCKET_OVERHEAD))
                .ok_or_else(|| {
                    "Decoded timeline-video cache entry byte size overflow.".to_owned()
                })?,
            "entry",
        )?;
    }

    for existing in cache.values().flatten().chain(std::iter::once(&stream)) {
        let stream_ptr = Arc::as_ptr(existing) as usize;
        if !seen_streams.insert(stream_ptr) {
            continue;
        }
        total_frames = total_frames
            .checked_add(existing.logical_frame_count())
            .ok_or_else(|| "Decoded timeline-video frame count overflow.".to_owned())?;
        if total_frames > frame_limit {
            return Err(format!(
                "Decoded timeline videos retain {total_frames} frame records, exceeding the aggregate {frame_limit}-frame cache budget. Shorten the videos or use fewer simultaneous streams."
            ));
        }
        let frame_storage = existing
            .frames
            .capacity()
            .checked_mul(std::mem::size_of::<DecodedVideoFrame>())
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<DecodedVideoStream>()))
            .and_then(|bytes| bytes.checked_add(ARC_ALLOCATION_OVERHEAD))
            .ok_or_else(|| "Decoded timeline-video frame-record byte size overflow.".to_owned())?;
        add_bytes(frame_storage, "frame-record")?;

        if let Some(decoder) = &existing.on_demand
            && seen_decoders.insert(Arc::as_ptr(decoder) as usize)
        {
            let ring_budget = decoder.pinned_budget_bytes().ok_or_else(|| {
                "Decoded timeline-video ring decoder byte size overflow.".to_owned()
            })?;
            add_bytes(ring_budget, "ring-decoder")?;
        }

        for frame in &existing.frames {
            let image_ptr = Arc::as_ptr(&frame.image) as usize;
            if !seen_images.insert(image_ptr) {
                continue;
            }
            let image_storage = frame
                .image
                .rgba
                .len()
                .checked_add(std::mem::size_of::<DecodedImage>())
                .and_then(|bytes| bytes.checked_add(ARC_ALLOCATION_OVERHEAD))
                .ok_or_else(|| {
                    "Decoded timeline-video image allocation byte size overflow.".to_owned()
                })?;
            add_bytes(image_storage, "image")?;
        }
    }
    cache.insert(key, Some(stream));
    Ok(())
}

fn insert_decoded_video_stream(
    cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    key: String,
    stream: Arc<DecodedVideoStream>,
) -> Result<(), String> {
    insert_decoded_video_stream_with_limit(cache, key, stream, MAX_DECODED_VIDEO_BATCH_BYTES)
}

fn preflight_and_decode_timeline_videos(
    project: &Project,
    root_symbol_id: &str,
    base_dir: Option<&Path>,
) -> Result<HashMap<String, Option<Arc<DecodedVideoStream>>>, ExportError> {
    let mut cache = HashMap::new();
    for (res_id, spec) in collect_reachable_video_spans(project, root_symbol_id) {
        let stream = resolve_and_decode_project_video(project, &res_id, Some(&spec), base_dir)
            .map_err(ExportError::Options)?;
        insert_decoded_video_stream(&mut cache, video_cache_key(&res_id, Some(&spec)), stream)
            .map_err(ExportError::Options)?;
    }
    Ok(cache)
}

/// Render a single frame (`0`-based) of a `Project` (or specific symbol) into an RGBA8 buffer of
/// size `width * height * 4`. Used both by the `Dialog / Export video` live preview and by batch export.
pub fn render_project_frame_rgba(
    project: &Project,
    symbol: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    transparent_bg: bool,
    runtime_capture: bool,
) -> Result<Vec<u8>, ExportError> {
    render_project_frame_rgba_with_base_dir(
        project,
        symbol,
        frame,
        width,
        height,
        transparent_bg,
        runtime_capture,
        None,
    )
}

/// Render a single frame (`0`-based) of a `Project` (or specific symbol) into an RGBA8 buffer,
/// resolving `VideoDeploymentMode::External` relative paths against `base_dir` (`T-201`).
#[allow(clippy::too_many_arguments)]
pub fn render_project_frame_rgba_with_base_dir(
    project: &Project,
    symbol: Option<&str>,
    frame: u32,
    width: u32,
    height: u32,
    transparent_bg: bool,
    runtime_capture: bool,
    base_dir: Option<&Path>,
) -> Result<Vec<u8>, ExportError> {
    let opts = VideoOptions {
        symbol: symbol.map(str::to_owned),
        width,
        height,
        transparent_bg,
        runtime_capture,
        base_dir: base_dir.map(Path::to_path_buf),
        ..VideoOptions::default()
    };
    let prepared = prepare_timeline(project, &opts)?;
    let mut video_cache =
        preflight_and_decode_timeline_videos(&prepared.eval_project, &prepared.root_id, base_dir)?;
    let target_frame = if runtime_capture {
        frame
    } else {
        frame % prepared.authored_span.max(1)
    };
    let fps = opts.resolved_fps(&prepared.eval_project);
    let dt_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);

    let (scene, placements) = if runtime_capture {
        let mut playback =
            AuthoredPlayback::with_live_scripts(&prepared.eval_project, &prepared.root_id, 0, true);
        let mut tracker = TimelineVideoTracker::new();
        tracker.step_with_commands(playback.drain_video_commands(), 0);
        for _ in 0..target_frame {
            playback.tick(&prepared.eval_project);
            tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
        }
        let scene = playback.scene(&prepared.eval_project, false, true);
        let mut map = HashMap::new();
        for draw in &scene.draws {
            if matches!(draw.item.kind, ElementKind::Video { .. })
                && let Some(p) = tracker.placement_for_target(&draw.item.instance_path)
            {
                map.insert(draw.item.instance_path.clone(), p.clone());
            }
        }
        (scene, map)
    } else {
        let scene = evaluate_scene(
            &prepared.eval_project,
            &prepared.root_id,
            target_frame,
            false,
            true,
        );
        let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
        let map = resolve_scene_video_placements(
            &prepared.eval_project,
            &prepared.root_id,
            target_frame,
            &HashMap::new(),
            &items,
        );
        (scene, map)
    };

    for placement in placements.values() {
        let cache_key = placement_video_cache_key(placement);
        if !video_cache.contains_key(&cache_key) {
            let stream = resolve_and_decode_project_video(
                &prepared.eval_project,
                &placement.resource,
                placement.source.as_ref(),
                base_dir,
            )
            .map_err(ExportError::Options)?;
            insert_decoded_video_stream(&mut video_cache, cache_key, stream)
                .map_err(ExportError::Options)?;
        }
    }

    let mut bitmap_cache = HashMap::new();
    Ok(render_authored_scene_with_video_rgba(
        &prepared.eval_project,
        &scene,
        width.max(1),
        height.max(1),
        transparent_bg,
        &mut bitmap_cache,
        &mut video_cache,
        &placements,
    ))
}

/// Stateful sequential frame renderer for image-sequence exports (`T-259`), avoiding `O(N^2)`
/// video decode and `AuthoredPlayback` re-stepping when `has_timeline_video` is true.
pub(crate) struct SequentialVideoFrameSession {
    prepared: PreparedTimeline,
    video_cache: HashMap<String, Option<Arc<DecodedVideoStream>>>,
    bitmap_cache: HashMap<String, Option<Arc<DecodedImage>>>,
    base_dir: Option<PathBuf>,
    width: u32,
    height: u32,
    transparent_bg: bool,
    runtime_capture: bool,
    dt_ms: u32,
    live_state: Option<(AuthoredPlayback, TimelineVideoTracker, u32)>,
}

impl SequentialVideoFrameSession {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        project: &Project,
        symbol: Option<&str>,
        start_frame: u32,
        width: u32,
        height: u32,
        transparent_bg: bool,
        runtime_capture: bool,
        base_dir: Option<&Path>,
    ) -> Result<Self, ExportError> {
        let opts = VideoOptions {
            symbol: symbol.map(str::to_owned),
            width,
            height,
            transparent_bg,
            runtime_capture,
            base_dir: base_dir.map(Path::to_path_buf),
            ..VideoOptions::default()
        };
        let prepared = prepare_timeline(project, &opts)?;
        let video_cache = preflight_and_decode_timeline_videos(
            &prepared.eval_project,
            &prepared.root_id,
            base_dir,
        )?;
        let fps = opts.resolved_fps(&prepared.eval_project);
        let dt_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
        let live_state = runtime_capture.then(|| {
            let mut playback = AuthoredPlayback::with_live_scripts(
                &prepared.eval_project,
                &prepared.root_id,
                0,
                true,
            );
            let mut tracker = TimelineVideoTracker::new();
            tracker.step_with_commands(playback.drain_video_commands(), 0);
            for _ in 0..start_frame {
                playback.tick(&prepared.eval_project);
                tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
            }
            (playback, tracker, start_frame)
        });
        Ok(Self {
            prepared,
            video_cache,
            bitmap_cache: HashMap::new(),
            base_dir: base_dir.map(Path::to_path_buf),
            width: width.max(1),
            height: height.max(1),
            transparent_bg,
            runtime_capture,
            dt_ms,
            live_state,
        })
    }

    pub(crate) fn render_frame(&mut self, frame: u32) -> Result<Vec<u8>, ExportError> {
        let target_frame = if self.runtime_capture {
            frame
        } else {
            frame % self.prepared.authored_span.max(1)
        };

        let (scene, placements) = if let Some((playback, tracker, cur_frame)) = &mut self.live_state
        {
            if target_frame < *cur_frame {
                *playback = AuthoredPlayback::with_live_scripts(
                    &self.prepared.eval_project,
                    &self.prepared.root_id,
                    0,
                    true,
                );
                *tracker = TimelineVideoTracker::new();
                tracker.step_with_commands(playback.drain_video_commands(), 0);
                *cur_frame = 0;
            }
            while *cur_frame < target_frame {
                playback.tick(&self.prepared.eval_project);
                tracker.step_with_commands(playback.drain_video_commands(), self.dt_ms);
                *cur_frame += 1;
            }
            let scene = playback.scene(&self.prepared.eval_project, false, true);
            let mut map = HashMap::new();
            for draw in &scene.draws {
                if matches!(draw.item.kind, ElementKind::Video { .. })
                    && let Some(p) = tracker.placement_for_target(&draw.item.instance_path)
                {
                    map.insert(draw.item.instance_path.clone(), p.clone());
                }
            }
            (scene, map)
        } else {
            let scene = evaluate_scene(
                &self.prepared.eval_project,
                &self.prepared.root_id,
                target_frame,
                false,
                true,
            );
            let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
            let map = resolve_scene_video_placements(
                &self.prepared.eval_project,
                &self.prepared.root_id,
                target_frame,
                &HashMap::new(),
                &items,
            );
            (scene, map)
        };

        for placement in placements.values() {
            let cache_key = placement_video_cache_key(placement);
            if !self.video_cache.contains_key(&cache_key) {
                let stream = resolve_and_decode_project_video(
                    &self.prepared.eval_project,
                    &placement.resource,
                    placement.source.as_ref(),
                    self.base_dir.as_deref(),
                )
                .map_err(ExportError::Options)?;
                insert_decoded_video_stream(&mut self.video_cache, cache_key, stream)
                    .map_err(ExportError::Options)?;
            }
        }

        Ok(render_authored_scene_with_video_rgba(
            &self.prepared.eval_project,
            &scene,
            self.width,
            self.height,
            self.transparent_bg,
            &mut self.bitmap_cache,
            &mut self.video_cache,
            &placements,
        ))
    }
}

struct BundledFonts {
    regular: FontRef<'static>,
    bold: FontRef<'static>,
    mono: FontRef<'static>,
}

fn bundled_fonts() -> &'static BundledFonts {
    static FONTS: OnceLock<BundledFonts> = OnceLock::new();
    FONTS.get_or_init(|| BundledFonts {
        regular: FontRef::try_from_slice(FONT_INTER_REGULAR).expect("valid Inter-Regular.ttf"),
        bold: FontRef::try_from_slice(FONT_INTER_BOLD).expect("valid Inter-SemiBold.ttf"),
        mono: FontRef::try_from_slice(FONT_MONO).expect("valid RobotoMono-Regular.ttf"),
    })
}

fn select_font(family: &str, bold: bool) -> &'static FontRef<'static> {
    let fonts = bundled_fonts();
    let lower = family.to_ascii_lowercase();
    if lower.contains("mono") || lower.contains("courier") || lower.contains("code") {
        &fonts.mono
    } else if bold {
        &fonts.bold
    } else {
        &fonts.regular
    }
}

/// Render an `AuthoredScene` into a straight-RGBA8 pixel buffer (`width * height * 4`).
#[allow(clippy::too_many_arguments)]
pub fn render_authored_scene_rgba(
    project: &Project,
    symbol_id: &str,
    frame: u32,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    transparent_bg: bool,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
) -> Result<Vec<u8>, ExportError> {
    let mut video_cache: HashMap<String, Option<Arc<DecodedVideoStream>>> = HashMap::new();
    let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
    let placements =
        resolve_scene_video_placements(project, symbol_id, frame, &HashMap::new(), &items);
    for placement in placements.values() {
        let cache_key = placement_video_cache_key(placement);
        if !video_cache.contains_key(&cache_key) {
            let stream = resolve_and_decode_project_video(
                project,
                &placement.resource,
                placement.source.as_ref(),
                None,
            )
            .map_err(ExportError::Options)?;
            insert_decoded_video_stream(&mut video_cache, cache_key, stream)
                .map_err(ExportError::Options)?;
        }
    }
    Ok(render_authored_scene_with_video_rgba(
        project,
        scene,
        width,
        height,
        transparent_bg,
        bitmap_cache,
        &mut video_cache,
        &placements,
    ))
}

/// Render an `AuthoredScene` with pre-resolved `ResolvedVideoPlacement`s and decoded video streams (`T-201`).
#[allow(clippy::too_many_arguments)]
pub fn render_authored_scene_with_video_rgba(
    project: &Project,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    transparent_bg: bool,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
    video_cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &HashMap<Vec<String>, ResolvedVideoPlacement>,
) -> Vec<u8> {
    let w = width.max(1);
    let h = height.max(1);
    let Some(mut pixmap) = Pixmap::new(w, h) else {
        return vec![0; (w * h * 4) as usize];
    };
    if !transparent_bg {
        let bg = project.background;
        pixmap.fill(SkiaColor::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
    }

    let sx = w as f32 / project.width.max(1) as f32;
    let sy = h as f32 / project.height.max(1) as f32;
    let stage_scale = SkiaTransform::from_scale(sx, sy);

    let coverage = MaskCoverage::new(project, &scene.clips).ok();
    let mut mask_cache: HashMap<Vec<String>, Option<SkiaMask>> = HashMap::new();

    for draw in &scene.draws {
        let mask_ref = if draw.clip_ids.is_empty() {
            None
        } else {
            if !mask_cache.contains_key(&draw.clip_ids) {
                let built = build_clip_mask(
                    w,
                    h,
                    project.width.max(1),
                    project.height.max(1),
                    &draw.clip_ids,
                    &scene.clips,
                    coverage.as_ref(),
                );
                mask_cache.insert(draw.clip_ids.clone(), built);
            }
            mask_cache.get(&draw.clip_ids).and_then(|m| m.as_ref())
        };
        draw_authored_item(
            &mut pixmap,
            project,
            draw,
            stage_scale,
            mask_ref,
            bitmap_cache,
            video_cache,
            video_placements,
        );
    }

    // Convert premultiplied RGBA from tiny-skia to straight RGBA8 for encoders and egui textures.
    let mut out = pixmap.take();
    for px in out.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a > 0 && a < 255 {
            let af = f32::from(a) / 255.0;
            px[0] = ((f32::from(px[0]) / af).round() as u32).min(255) as u8;
            px[1] = ((f32::from(px[1]) / af).round() as u32).min(255) as u8;
            px[2] = ((f32::from(px[2]) / af).round() as u32).min(255) as u8;
        }
    }
    out
}

fn build_clip_mask(
    out_w: u32,
    out_h: u32,
    stage_w: u32,
    stage_h: u32,
    clip_ids: &[String],
    clips: &[neoflash_core::RuntimeClip],
    coverage: Option<&MaskCoverage>,
) -> Option<SkiaMask> {
    if let Some(cov) = coverage {
        let step_x = f64::from(stage_w) / f64::from(out_w.max(1));
        let step_y = f64::from(stage_h) / f64::from(out_h.max(1));
        if let Ok(bytes) = cov.raster(
            clip_ids,
            [0.0, 0.0],
            [step_x, step_y],
            out_w as usize,
            out_h as usize,
        ) && let Some(size) = IntSize::from_wh(out_w, out_h)
        {
            return SkiaMask::from_vec(bytes, size);
        }
    }
    // Fallback fast path: rasterize geometric clip components with tiny-skia.
    let sx = out_w as f32 / stage_w.max(1) as f32;
    let sy = out_h as f32 / stage_h.max(1) as f32;
    let stage_scale = SkiaTransform::from_scale(sx, sy);
    let mut combined: Option<SkiaMask> = None;
    for clip_id in clip_ids {
        let Some(clip) = clips.iter().find(|c| &c.id == clip_id) else {
            continue;
        };
        let mut mask = SkiaMask::new(out_w, out_h)?;
        for comp in &clip.components {
            if let Some(path) = build_skia_path(&comp.commands) {
                let xf = stage_scale.pre_concat(affine_to_skia(comp.matrix));
                mask.fill_path(&path, to_skia_fill_rule(comp.fill_rule), true, xf);
            }
        }
        if let Some(existing) = &mut combined {
            for (dst, src) in existing.data_mut().iter_mut().zip(mask.data().iter()) {
                *dst = ((u16::from(*dst) * u16::from(*src) + 127) / 255) as u8;
            }
        } else {
            combined = Some(mask);
        }
    }
    combined
}

fn affine_to_skia(m: [f64; 6]) -> SkiaTransform {
    SkiaTransform::from_row(
        m[0] as f32,
        m[1] as f32,
        m[2] as f32,
        m[3] as f32,
        m[4] as f32,
        m[5] as f32,
    )
}

fn to_skia_fill_rule(rule: FillRule) -> SkiaFillRule {
    match rule {
        FillRule::NonZero => SkiaFillRule::Winding,
        FillRule::EvenOdd => SkiaFillRule::EvenOdd,
    }
}

fn apply_color(base: Color, ct: ColorTransform, alpha: f64) -> Color {
    let mut c = ct.apply(base);
    c[3] = (f64::from(c[3]) * alpha.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;
    c
}

fn build_skia_path(commands: &[PathCommand]) -> Option<SkiaPath> {
    let mut pb = PathBuilder::new();
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => pb.move_to(x as f32, y as f32),
            PathCommand::LineTo { x, y } => pb.line_to(x as f32, y as f32),
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                pb.quad_to(cx as f32, cy as f32, x as f32, y as f32);
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                pb.cubic_to(
                    cx1 as f32, cy1 as f32, cx2 as f32, cy2 as f32, x as f32, y as f32,
                );
            }
            PathCommand::Close => pb.close(),
        }
    }
    pb.finish()
}

fn configure_paint_shader<'a>(
    solid: Color,
    gradient: Option<&GradientPaint>,
    ct: ColorTransform,
    alpha: f64,
) -> SkiaPaint<'a> {
    let mut paint = SkiaPaint {
        anti_alias: true,
        ..SkiaPaint::default()
    };
    if let Some(grad) = gradient
        && !grad.stops.is_empty()
    {
        let stops: Vec<SkiaGradientStop> = grad
            .stops
            .iter()
            .map(|s| {
                let c = apply_color(s.color, ct, alpha);
                SkiaGradientStop::new(
                    f32::from(s.ratio) / 255.0,
                    SkiaColor::from_rgba8(c[0], c[1], c[2], c[3]),
                )
            })
            .collect();
        let grad_xf = affine_to_skia(grad.matrix);
        let shader = match grad.kind {
            GradientKind::Linear => LinearGradient::new(
                SkiaPoint::from_xy(-819.2, 0.0),
                SkiaPoint::from_xy(819.2, 0.0),
                stops,
                SpreadMode::Pad,
                grad_xf,
            ),
            GradientKind::Radial => {
                let fx = (grad.focal_point_ratio.clamp(-0.98, 0.98) * 819.2) as f32;
                RadialGradient::new(
                    SkiaPoint::from_xy(fx, 0.0),
                    0.0,
                    SkiaPoint::from_xy(0.0, 0.0),
                    819.2,
                    stops,
                    SpreadMode::Pad,
                    grad_xf,
                )
            }
        };
        if let Some(shader) = shader {
            paint.shader = shader;
            return paint;
        }
    }
    let c = apply_color(solid, ct, alpha);
    paint.set_color_rgba8(c[0], c[1], c[2], c[3]);
    paint
}

#[allow(clippy::too_many_arguments)]
fn draw_authored_item(
    pixmap: &mut Pixmap,
    project: &Project,
    draw: &AuthoredDraw,
    stage_scale: SkiaTransform,
    mask: Option<&SkiaMask>,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
    video_cache: &mut HashMap<String, Option<Arc<DecodedVideoStream>>>,
    video_placements: &HashMap<Vec<String>, ResolvedVideoPlacement>,
) {
    let item: &DrawItem = &draw.item;
    let alpha = item.alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let ct = item.color_transform;
    let xf = stage_scale.pre_concat(affine_to_skia(item.transform));

    match &item.kind {
        ElementKind::Rectangle {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if *width <= 0.0 || *height <= 0.0 {
                return;
            }
            let commands = rectangle_path_commands(*width, *height, Default::default());
            draw_vector_commands(
                pixmap,
                &commands,
                FillRule::NonZero,
                (*fill, None),
                (*stroke, None, *stroke_width),
                ct,
                alpha,
                xf,
                mask,
            );
        }
        ElementKind::Ellipse {
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => {
            if *width <= 0.0 || *height <= 0.0 {
                return;
            }
            let commands = oval_path_commands(*width, *height, Default::default());
            draw_vector_commands(
                pixmap,
                &commands,
                FillRule::NonZero,
                (*fill, None),
                (*stroke, None, *stroke_width),
                ct,
                alpha,
                xf,
                mask,
            );
        }
        ElementKind::Path {
            commands,
            fill_rule,
            fill,
            stroke,
            stroke_width,
        } => {
            draw_vector_commands(
                pixmap,
                commands,
                *fill_rule,
                (*fill, None),
                (*stroke, None, *stroke_width),
                ct,
                alpha,
                xf,
                mask,
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
            draw_vector_commands(
                pixmap,
                commands,
                *fill_rule,
                (*fill, fill_gradient.as_ref()),
                (*stroke, stroke_gradient.as_ref(), *stroke_width),
                ct,
                alpha,
                xf,
                mask,
            );
        }
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            smoothing,
        } => {
            if *width == 0 || *height == 0 {
                return;
            }
            let decoded = bitmap_cache
                .entry(resource_id.clone())
                .or_insert_with(|| {
                    project
                        .resources
                        .iter()
                        .find(|r| r.id == *resource_id)
                        .and_then(|r| r.decode_image().ok())
                })
                .clone();
            if let Some(img) = decoded {
                draw_bitmap(
                    pixmap, &img, *width, *height, *smoothing, ct, alpha, xf, mask,
                );
            }
        }
        ElementKind::Text {
            text,
            font_family,
            font_size,
            color,
        } => {
            let c = apply_color(*color, ct, alpha);
            draw_plain_text(
                pixmap,
                text,
                font_family,
                *font_size as f32,
                false,
                c,
                TextAlign::Left,
                None,
                xf,
                mask,
            );
        }
        ElementKind::TextBox {
            text,
            width,
            height,
            format,
            ..
        } => {
            draw_text_box(pixmap, text, *width, *height, format, ct, alpha, xf, mask);
        }
        ElementKind::Video { width, height } => {
            if *width <= 0.0 || *height <= 0.0 {
                return;
            }
            let target_w = width.round().max(1.0) as u32;
            let target_h = height.round().max(1.0) as u32;
            if let Some(placement) = video_placements.get(&item.instance_path)
                && let Some(stream) = video_cache
                    .get(&placement_video_cache_key(placement))
                    .or_else(|| video_cache.get(&placement.resource))
                    .and_then(|s| s.as_ref())
                && let Some((_, frame)) = stream.frame_at_placement(placement)
            {
                draw_bitmap(
                    pixmap,
                    &frame.image,
                    target_w,
                    target_h,
                    true,
                    ct,
                    alpha,
                    xf,
                    mask,
                );
            }
        }
        ElementKind::Instance { .. } => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_vector_commands(
    pixmap: &mut Pixmap,
    commands: &[PathCommand],
    fill_rule: FillRule,
    fill: (Color, Option<&GradientPaint>),
    stroke: (Color, Option<&GradientPaint>, f64),
    ct: ColorTransform,
    alpha: f64,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let Some(path) = build_skia_path(commands) else {
        return;
    };
    let fill_visible = fill.1.is_some() || apply_color(fill.0, ct, alpha)[3] > 0;
    if fill_visible {
        let paint = configure_paint_shader(fill.0, fill.1, ct, alpha);
        pixmap.fill_path(&path, &paint, to_skia_fill_rule(fill_rule), xf, mask);
    }
    let (stroke_color, stroke_grad, stroke_width) = stroke;
    let stroke_visible = stroke_width > 0.0
        && (stroke_grad.is_some() || apply_color(stroke_color, ct, alpha)[3] > 0);
    if stroke_visible {
        let paint = configure_paint_shader(stroke_color, stroke_grad, ct, alpha);
        let skia_stroke = SkiaStroke {
            width: stroke_width as f32,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..SkiaStroke::default()
        };
        pixmap.stroke_path(&path, &paint, &skia_stroke, xf, mask);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bitmap(
    pixmap: &mut Pixmap,
    img: &DecodedImage,
    target_w: u32,
    target_h: u32,
    smoothing: bool,
    ct: ColorTransform,
    alpha: f64,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    if img.width == 0 || img.height == 0 {
        return;
    }
    let Some(mut src_pixmap) = Pixmap::new(img.width, img.height) else {
        return;
    };
    for (dst, src) in src_pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(img.rgba.as_chunks::<4>().0)
    {
        let c = apply_color([src[0], src[1], src[2], src[3]], ct, alpha);
        let af = f32::from(c[3]) / 255.0;
        dst[0] = (f32::from(c[0]) * af).round() as u8;
        dst[1] = (f32::from(c[1]) * af).round() as u8;
        dst[2] = (f32::from(c[2]) * af).round() as u8;
        dst[3] = c[3];
    }
    let scale_x = target_w as f32 / img.width as f32;
    let scale_y = target_h as f32 / img.height as f32;
    let img_xf = xf.pre_concat(SkiaTransform::from_scale(scale_x, scale_y));
    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: if smoothing {
            FilterQuality::Bilinear
        } else {
            FilterQuality::Nearest
        },
    };
    pixmap.draw_pixmap(0, 0, src_pixmap.as_ref(), &paint, img_xf, mask);
}

#[allow(clippy::too_many_arguments)]
fn draw_text_box(
    pixmap: &mut Pixmap,
    text: &str,
    width: f64,
    height: f64,
    format: &TextFormat,
    ct: ColorTransform,
    alpha: f64,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    let raw_color = [
        ((format.color >> 16) & 0xFF) as u8,
        ((format.color >> 8) & 0xFF) as u8,
        (format.color & 0xFF) as u8,
        255,
    ];
    let c = apply_color(raw_color, ct, alpha);
    draw_plain_text(
        pixmap,
        text,
        &format.font,
        format.size.max(4.0) as f32,
        format.bold,
        c,
        format.align,
        Some((width.max(1.0) as f32, height.max(1.0) as f32)),
        xf,
        mask,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_plain_text(
    pixmap: &mut Pixmap,
    text: &str,
    family: &str,
    size_px: f32,
    bold: bool,
    color: Color,
    align: TextAlign,
    box_bounds: Option<(f32, f32)>,
    xf: SkiaTransform,
    mask: Option<&SkiaMask>,
) {
    if text.is_empty() || color[3] == 0 || size_px <= 0.5 {
        return;
    }
    let font = select_font(family, bold);
    let scale = PxScale::from(size_px.clamp(2.0, 512.0));
    let scaled = font.as_scaled(scale);

    // Measure lines for local bitmap buffer.
    let lines: Vec<&str> = text.split('\n').collect();
    let line_height = (scaled.ascent() - scaled.descent() + scaled.line_gap()).max(size_px);
    let mut line_widths = Vec::with_capacity(lines.len());
    let mut max_w = 0.0_f32;
    for line in &lines {
        let mut w = 0.0_f32;
        let mut prev = None;
        for ch in line.chars() {
            let gid = scaled.glyph_id(ch);
            if let Some(p) = prev {
                w += scaled.kern(p, gid);
            }
            w += scaled.h_advance(gid);
            prev = Some(gid);
        }
        line_widths.push(w);
        max_w = max_w.max(w);
    }

    let (buf_w, buf_h) = if let Some((bw, bh)) = box_bounds {
        (
            (bw.ceil() as u32).clamp(4, 4096),
            (bh.ceil() as u32).clamp(4, 4096),
        )
    } else {
        (
            ((max_w.ceil() as u32) + 8).clamp(4, 4096),
            (((lines.len() as f32 * line_height).ceil() as u32) + 8).clamp(4, 4096),
        )
    };

    let Some(mut text_pixmap) = Pixmap::new(buf_w, buf_h) else {
        return;
    };
    let bw_i = buf_w as i32;
    let bh_i = buf_h as i32;
    let data = text_pixmap.data_mut();
    let base_alpha = f32::from(color[3]) / 255.0;

    for (line_idx, (line, &lw)) in lines.iter().zip(line_widths.iter()).enumerate() {
        let baseline_y = scaled.ascent() + line_idx as f32 * line_height;
        if baseline_y - scaled.ascent() > buf_h as f32 {
            break;
        }
        let mut pen_x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => ((buf_w as f32 - lw) * 0.5).max(0.0),
            TextAlign::Right => (buf_w as f32 - lw).max(0.0),
        };
        let mut prev = None;
        for ch in line.chars() {
            let gid = scaled.glyph_id(ch);
            if let Some(p) = prev {
                pen_x += scaled.kern(p, gid);
            }
            let glyph = gid.with_scale_and_position(scale, ab_glyph::point(pen_x, baseline_y));
            pen_x += scaled.h_advance(gid);
            prev = Some(gid);
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                let min_x = bounds.min.x as i32;
                let min_y = bounds.min.y as i32;
                outlined.draw(|gx, gy, cov| {
                    let px = min_x + gx as i32;
                    let py = min_y + gy as i32;
                    if px >= 0 && px < bw_i && py >= 0 && py < bh_i {
                        let a_cov = (cov * base_alpha).clamp(0.0, 1.0);
                        if a_cov > 0.0 {
                            let idx = ((py as usize) * (buf_w as usize) + (px as usize)) * 4;
                            let a_byte = (a_cov * 255.0).round() as u8;
                            if a_byte > data[idx + 3] {
                                data[idx] = (f32::from(color[0]) * a_cov).round() as u8;
                                data[idx + 1] = (f32::from(color[1]) * a_cov).round() as u8;
                                data[idx + 2] = (f32::from(color[2]) * a_cov).round() as u8;
                                data[idx + 3] = a_byte;
                            }
                        }
                    }
                });
            }
        }
    }

    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: FilterQuality::Bilinear,
    };
    pixmap.draw_pixmap(0, 0, text_pixmap.as_ref(), &paint, xf, mask);
}

#[derive(Clone)]
struct DecodedAudioResource {
    sample_rate: u32,
    /// Interleaved stereo (`[left, right]`) samples at `sample_rate`.
    stereo: Vec<[f32; 2]>,
}

const MIX_SAMPLE_RATE: u32 = 44_100;

fn decode_audio_resource(bytes: &[u8], media_type: &str) -> Option<DecodedAudioResource> {
    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let mut hint = Hint::new();
    if media_type.contains("mpeg") || media_type.contains("mp3") {
        hint.with_extension("mp3");
    } else if media_type.contains("wav") {
        hint.with_extension("wav");
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(MIX_SAMPLE_RATE);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut raw_stereo: Vec<[f32; 2]> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::ResetRequired) => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        let samples = buf.samples();
        for frame in samples.chunks(channels) {
            let l = frame[0];
            let r = if channels > 1 { frame[1] } else { l };
            raw_stereo.push([l, r]);
        }
    }
    if raw_stereo.is_empty() {
        return None;
    }
    if sample_rate == MIX_SAMPLE_RATE {
        return Some(DecodedAudioResource {
            sample_rate: MIX_SAMPLE_RATE,
            stereo: raw_stereo,
        });
    }
    // Linear resample to MIX_SAMPLE_RATE (44.1 kHz).
    let ratio = f64::from(sample_rate) / f64::from(MIX_SAMPLE_RATE);
    let out_len = ((raw_stereo.len() as f64) / ratio).round() as usize;
    let mut resampled = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = (i as f64) * ratio;
        let idx = (src_pos.floor() as usize).min(raw_stereo.len() - 1);
        let next = (idx + 1).min(raw_stereo.len() - 1);
        let frac = (src_pos - idx as f64) as f32;
        let a = raw_stereo[idx];
        let b = raw_stereo[next];
        resampled.push([a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]);
    }
    Some(DecodedAudioResource {
        sample_rate: MIX_SAMPLE_RATE,
        stereo: resampled,
    })
}

struct ActiveAudioVoice {
    resource_id: String,
    stream_layer_id: Option<String>,
    start_sample: usize,
    slice_start: usize,
    slice_len: usize,
    total_play_samples: usize,
    envelope: Vec<[f64; 3]>,
}

/// Mix timeline `SoundCommand` events across `frame_events` into a 44.1 kHz 16-bit stereo WAV byte buffer.
/// Returns `None` if no audible timeline audio was triggered.
fn mix_timeline_audio_wav(
    project: &Project,
    frame_events: &[(u32, Vec<SoundCommand>)],
    frame_count: u32,
    fps: f32,
) -> Option<Vec<u8>> {
    let total_samples =
        ((f64::from(frame_count) / f64::from(fps)) * f64::from(MIX_SAMPLE_RATE)).round() as usize;
    if total_samples == 0 {
        return None;
    }

    let mut decoded_cache: HashMap<String, Option<DecodedAudioResource>> = HashMap::new();
    let mut voices: Vec<ActiveAudioVoice> = Vec::new();

    for (frame_offset, commands) in frame_events {
        let current_sample = ((f64::from(*frame_offset) / f64::from(fps))
            * f64::from(MIX_SAMPLE_RATE))
        .round() as usize;
        for cmd in commands {
            match cmd {
                SoundCommand::StopAll => {
                    voices.clear();
                }
                SoundCommand::Stop { resource } => {
                    voices.retain(|v| &v.resource_id != resource);
                }
                SoundCommand::StreamStop { channel } => {
                    voices.retain(|v| v.stream_layer_id.as_ref() != Some(channel));
                }
                SoundCommand::Play {
                    resource,
                    sync,
                    loops,
                    loop_forever,
                    envelope,
                    in_point,
                    out_point,
                } => {
                    if *sync == neoflash_core::SoundSync::Start
                        && voices.iter().any(|v| {
                            &v.resource_id == resource
                                && current_sample
                                    < v.start_sample.saturating_add(v.total_play_samples)
                        })
                    {
                        continue;
                    }
                    let audio = decoded_cache
                        .entry(resource.clone())
                        .or_insert_with(|| {
                            project
                                .resources
                                .iter()
                                .find(|r| &r.id == resource)
                                .and_then(|r| {
                                    let bytes = r.decoded_bytes().ok()?;
                                    decode_audio_resource(&bytes, &r.media_type)
                                })
                        })
                        .as_ref();
                    let Some(audio) = audio else {
                        continue;
                    };
                    let sr = f64::from(audio.sample_rate);
                    let in_s = in_point.unwrap_or(0.0).max(0.0);
                    let slice_start = ((in_s * sr).round() as usize).min(audio.stereo.len());
                    let slice_end = out_point
                        .map(|out_s| {
                            ((out_s.max(in_s) * sr).round() as usize).min(audio.stereo.len())
                        })
                        .unwrap_or(audio.stereo.len());
                    let slice_len = slice_end.saturating_sub(slice_start);
                    if slice_len == 0 {
                        continue;
                    }
                    let total_play_samples = if *loop_forever {
                        total_samples.saturating_sub(current_sample)
                    } else {
                        slice_len.saturating_mul((*loops).max(1) as usize)
                    };
                    voices.push(ActiveAudioVoice {
                        resource_id: resource.clone(),
                        stream_layer_id: None,
                        start_sample: current_sample,
                        slice_start,
                        slice_len,
                        total_play_samples,
                        envelope: envelope.clone(),
                    });
                }
                SoundCommand::StreamStart {
                    channel,
                    resource,
                    offset,
                    loops,
                    loop_forever,
                    envelope,
                    in_point,
                    out_point,
                } => {
                    voices.retain(|v| v.stream_layer_id.as_ref() != Some(channel));
                    let audio = decoded_cache
                        .entry(resource.clone())
                        .or_insert_with(|| {
                            project
                                .resources
                                .iter()
                                .find(|r| &r.id == resource)
                                .and_then(|r| {
                                    let bytes = r.decoded_bytes().ok()?;
                                    decode_audio_resource(&bytes, &r.media_type)
                                })
                        })
                        .as_ref();
                    let Some(audio) = audio else {
                        continue;
                    };
                    let sr = f64::from(audio.sample_rate);
                    let in_s = in_point.unwrap_or(0.0).max(0.0) + offset.max(0.0);
                    let slice_start = ((in_s * sr).round() as usize).min(audio.stereo.len());
                    let slice_end = out_point
                        .map(|out_s| {
                            ((out_s.max(in_s) * sr).round() as usize).min(audio.stereo.len())
                        })
                        .unwrap_or(audio.stereo.len());
                    let slice_len = slice_end.saturating_sub(slice_start);
                    if slice_len == 0 {
                        continue;
                    }
                    let total_play_samples = if *loop_forever {
                        total_samples.saturating_sub(current_sample)
                    } else {
                        slice_len.saturating_mul((*loops).max(1) as usize)
                    };
                    voices.push(ActiveAudioVoice {
                        resource_id: resource.clone(),
                        stream_layer_id: Some(channel.clone()),
                        start_sample: current_sample,
                        slice_start,
                        slice_len,
                        total_play_samples,
                        envelope: envelope.clone(),
                    });
                }
            }
        }
    }

    if voices.is_empty() {
        return None;
    }

    let mut mix = vec![[0.0_f32; 2]; total_samples];
    for voice in &voices {
        let Some(Some(audio)) = decoded_cache.get(&voice.resource_id) else {
            continue;
        };
        let end_sample = voice
            .start_sample
            .saturating_add(voice.total_play_samples)
            .min(total_samples);
        let denom = (voice.total_play_samples.max(1)) as f64;
        for (s_idx, out_frame) in mix
            .iter_mut()
            .enumerate()
            .take(end_sample)
            .skip(voice.start_sample)
        {
            let elapsed = s_idx - voice.start_sample;
            let src_idx = voice.slice_start + (elapsed % voice.slice_len);
            let sample = audio.stereo[src_idx];
            let pos = (elapsed as f64 / denom).clamp(0.0, 1.0);
            let (gl, gr) = neoflash_core::envelope_gains(&voice.envelope, pos);
            out_frame[0] += sample[0] * (gl as f32);
            out_frame[1] += sample[1] * (gr as f32);
        }
    }

    Some(encode_wav_pcm16(&mix, MIX_SAMPLE_RATE))
}

fn encode_wav_pcm16(stereo: &[[f32; 2]], sample_rate: u32) -> Vec<u8> {
    let data_bytes = (stereo.len() * 4) as u32;
    let riff_size = 36u32.saturating_add(data_bytes);
    let mut out = Vec::with_capacity((riff_size + 8) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // 2 channels
    out.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * 4;
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for &[l, r] in stereo {
        let li = (l.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        let ri = (r.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&li.to_le_bytes());
        out.extend_from_slice(&ri.to_le_bytes());
    }
    out
}

/// Export a project (or symbol) to MP4, WebM, MOV, GIF, or PNG sequence.
pub fn export_project_video(
    project: &Project,
    output_path: &Path,
    options: &VideoOptions,
) -> Result<VideoExportReport, ExportError> {
    export_project_video_with_progress(project, output_path, options, |_| true)
}

/// Export a project (or symbol) to MP4, WebM, MOV, GIF, or PNG sequence while reporting frame-by-frame
/// rendering progress (including the rendered RGBA frame for live preview).
pub fn export_project_video_with_progress<F>(
    project: &Project,
    output_path: &Path,
    options: &VideoOptions,
    mut on_progress: F,
) -> Result<VideoExportReport, ExportError>
where
    F: FnMut(VideoProgress) -> bool,
{
    let prepared = prepare_timeline(project, options)?;
    let (width, height) = options.resolved_dimensions(project);
    let fps = options.resolved_fps(project);
    let transparent_bg = options.transparent_bg && options.format.supports_transparent_bg();
    let base_dir = options
        .base_dir
        .as_deref()
        .or_else(|| output_path.parent().filter(|p| !p.as_os_str().is_empty()));

    // Preflight and decode all reachable timeline video streams before creating output files (`T-201`).
    let mut video_cache =
        preflight_and_decode_timeline_videos(&prepared.eval_project, &prepared.root_id, base_dir)?;

    let dt_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
    let mut playback = AuthoredPlayback::with_live_scripts(
        &prepared.eval_project,
        &prepared.root_id,
        0,
        options.runtime_capture,
    );
    let mut video_tracker = TimelineVideoTracker::new();
    if prepared.start_frame > 0 {
        let _ = playback.drain_sound_commands();
        video_tracker.step_with_commands(playback.drain_video_commands(), 0);
        for step in 0..prepared.start_frame {
            playback.tick(&prepared.eval_project);
            video_tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
            if step + 1 < prepared.start_frame {
                let _ = playback.drain_sound_commands();
            }
        }
    }

    let mut bitmap_cache = HashMap::new();
    let mut rendered_frames: Vec<Vec<u8>> = Vec::with_capacity(prepared.frame_count as usize);
    let mut sound_events: Vec<(u32, Vec<SoundCommand>)> = Vec::new();
    let mut warnings: Vec<String> = playback.drain_diagnostics();

    for idx in 0..prepared.frame_count {
        if idx == 0 {
            if prepared.start_frame == 0 {
                video_tracker.step_with_commands(playback.drain_video_commands(), 0);
            }
        } else if idx % prepared.authored_span == 0 && prepared.start_frame > 0 {
            // Re-seek to start_frame when looping a sub-range under StopMode::AfterSeconds.
            playback = AuthoredPlayback::with_live_scripts(
                &prepared.eval_project,
                &prepared.root_id,
                0,
                options.runtime_capture,
            );
            video_tracker.clear();
            let _ = playback.drain_sound_commands();
            video_tracker.step_with_commands(playback.drain_video_commands(), 0);
            for step in 0..prepared.start_frame {
                playback.tick(&prepared.eval_project);
                video_tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
                if step + 1 < prepared.start_frame {
                    let _ = playback.drain_sound_commands();
                }
            }
        } else {
            playback.tick(&prepared.eval_project);
            video_tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
        }
        warnings.extend(playback.drain_diagnostics());
        let cmds = playback.drain_sound_commands();
        if !cmds.is_empty() {
            sound_events.push((idx, cmds));
        }

        let (scene, placements) = if options.runtime_capture {
            let scene = playback.scene(&prepared.eval_project, false, true);
            let mut map = HashMap::new();
            for draw in &scene.draws {
                if matches!(draw.item.kind, ElementKind::Video { .. })
                    && let Some(p) = video_tracker.placement_for_target(&draw.item.instance_path)
                {
                    map.insert(draw.item.instance_path.clone(), p.clone());
                }
            }
            (scene, map)
        } else {
            let f = prepared.start_frame + (idx % prepared.authored_span.max(1));
            let scene = evaluate_scene(&prepared.eval_project, &prepared.root_id, f, false, true);
            let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
            let map = resolve_scene_video_placements(
                &prepared.eval_project,
                &prepared.root_id,
                f,
                &HashMap::new(),
                &items,
            );
            (scene, map)
        };

        for placement in placements.values() {
            let cache_key = placement_video_cache_key(placement);
            if !video_cache.contains_key(&cache_key) {
                let stream = resolve_and_decode_project_video(
                    &prepared.eval_project,
                    &placement.resource,
                    placement.source.as_ref(),
                    base_dir,
                )
                .map_err(ExportError::Options)?;
                insert_decoded_video_stream(&mut video_cache, cache_key, stream)
                    .map_err(ExportError::Options)?;
            }
        }

        let rgba = render_authored_scene_with_video_rgba(
            &prepared.eval_project,
            &scene,
            width,
            height,
            transparent_bg,
            &mut bitmap_cache,
            &mut video_cache,
            &placements,
        );
        let keep_going = on_progress(VideoProgress {
            frame_index: idx + 1,
            frame_count: prepared.frame_count,
            width,
            height,
            rgba: rgba.clone(),
        });
        if !keep_going {
            return Err(ExportError::Options("Video export cancelled".into()));
        }
        rendered_frames.push(rgba);
    }

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let wav_audio = if options.include_audio && options.format.supports_audio() {
        mix_timeline_audio_wav(
            &prepared.eval_project,
            &sound_events,
            prepared.frame_count,
            fps,
        )
    } else {
        None
    };

    match options.format {
        VideoFormat::Gif => {
            if options.include_audio && !sound_events.is_empty() {
                warnings.push("GIF does not support audio; sound tracks were omitted.".into());
            }
            encode_gif_file(
                output_path,
                &rendered_frames,
                width,
                height,
                fps,
                options.quality,
                transparent_bg,
            )?;
            let bytes = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
            Ok(VideoExportReport {
                output_path: output_path.to_path_buf(),
                additional_files: Vec::new(),
                bytes,
                frame_count: prepared.frame_count,
                width,
                height,
                audio_included: false,
                warnings,
            })
        }
        VideoFormat::PngSequence => encode_png_sequence(
            output_path,
            &rendered_frames,
            width,
            height,
            wav_audio.as_deref(),
            warnings,
        ),
        VideoFormat::Mp4 | VideoFormat::WebM | VideoFormat::Mov => encode_with_ffmpeg(
            output_path,
            options,
            &rendered_frames,
            width,
            height,
            fps,
            transparent_bg,
            wav_audio.as_deref(),
            warnings,
        ),
    }
}

fn encode_gif_file(
    output_path: &Path,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    quality: u8,
    transparent_bg: bool,
) -> Result<(), ExportError> {
    let w = u16::try_from(width)
        .map_err(|_| ExportError::Options("GIF width exceeds 65535 pixels".into()))?;
    let h = u16::try_from(height)
        .map_err(|_| ExportError::Options("GIF height exceeds 65535 pixels".into()))?;
    let file = fs::File::create(output_path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), w, h, &[])
        .map_err(|e| ExportError::Options(format!("GIF encoder error: {e}")))?;
    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(|e| ExportError::Options(format!("GIF repeat error: {e}")))?;

    let delay_cs = ((100.0 / f64::from(fps)).round() as u16).max(1);
    let sample_fac = (30 - i32::from(quality.clamp(1, 100)) * 29 / 100).clamp(1, 30);

    for rgba in frames {
        // If GIF dimensions exceed 2048, scale down or quantize directly.
        let nq = NeuQuant::new(sample_fac, 255, rgba);
        let mut palette = nq.color_map_rgb();
        // Reserve index 255 for transparent pixels when transparent_bg is enabled.
        while palette.len() < 256 * 3 {
            palette.push(0);
        }
        let mut indexed = Vec::with_capacity((width * height) as usize);
        for px in rgba.as_chunks::<4>().0 {
            if transparent_bg && px[3] < 128 {
                indexed.push(255u8);
            } else {
                let idx = (nq.index_of(px) as u8).min(254);
                indexed.push(idx);
            }
        }
        let mut frame =
            gif::Frame::from_palette_pixels(w, h, indexed, palette, transparent_bg.then_some(255));
        frame.delay = delay_cs;
        frame.dispose = if transparent_bg {
            gif::DisposalMethod::Background
        } else {
            gif::DisposalMethod::Any
        };
        encoder
            .write_frame(&frame)
            .map_err(|e| ExportError::Options(format!("GIF frame write error: {e}")))?;
    }
    Ok(())
}

fn encode_png_sequence(
    output_path: &Path,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    wav_audio: Option<&[u8]>,
    warnings: Vec<String>,
) -> Result<VideoExportReport, ExportError> {
    let parent = output_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let raw_stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("frame");
    let stem = raw_stem.trim_end_matches("_0001");

    let mut total_bytes = 0u64;
    let mut additional_files = Vec::new();
    let mut first_path = output_path.to_path_buf();

    for (idx, rgba) in frames.iter().enumerate() {
        let frame_path =
            if frames.len() == 1 && raw_stem == stem && output_path.extension().is_some() {
                output_path.to_path_buf()
            } else {
                parent.join(format!("{stem}_{:04}.png", idx + 1))
            };
        let img = RgbaImage::from_raw(width, height, rgba.clone())
            .ok_or_else(|| ExportError::Options("PNG sequence buffer size mismatch".into()))?;
        img.save(&frame_path)
            .map_err(|e| ExportError::Options(format!("Could not save PNG frame: {e}")))?;
        let len = fs::metadata(&frame_path).map(|m| m.len()).unwrap_or(0);
        total_bytes = total_bytes.saturating_add(len);
        if idx == 0 {
            first_path = frame_path;
        } else {
            additional_files.push(frame_path);
        }
    }

    let mut audio_included = false;
    if let Some(wav) = wav_audio {
        let wav_path = parent.join(format!("{stem}.wav"));
        fs::write(&wav_path, wav)?;
        total_bytes = total_bytes.saturating_add(wav.len() as u64);
        additional_files.push(wav_path);
        audio_included = true;
    }

    Ok(VideoExportReport {
        output_path: first_path,
        additional_files,
        bytes: total_bytes,
        frame_count: frames.len() as u32,
        width,
        height,
        audio_included,
        warnings,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg_encode_attempt(
    ffmpeg: &Path,
    output_path: &Path,
    options: &VideoOptions,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    transparent_bg: bool,
    staged_audio_path: Option<&Path>,
    video_encoder: &str,
) -> Result<(), ExportError> {
    let mut cmd = Command::new(ffmpeg);
    configure_media_subprocess_command(&mut cmd, ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pixel_format")
        .arg("rgba")
        .arg("-video_size")
        .arg(format!("{width}x{height}"))
        .arg("-framerate")
        .arg(format!("{fps:.3}"))
        .arg("-i")
        .arg("pipe:0");

    if let Some(wav_path) = staged_audio_path {
        cmd.arg("-i").arg(wav_path);
    }

    let codec = options.resolved_codec();
    let q = i32::from(options.quality.clamp(1, 100));
    let crf_h264 = (51 - (q * 35) / 100).clamp(14, 45);
    let crf_vp9 = (63 - (q * 48) / 100).clamp(15, 55);
    let target_kbps = ((options.target_bitrate_mbps(width, height, fps) * 1000.0).round() as u32)
        .clamp(200, 80_000);

    match (options.format, codec) {
        (VideoFormat::Mp4, VideoCodec::H265) | (VideoFormat::Mov, VideoCodec::H265) => {
            cmd.arg("-c:v").arg(video_encoder);
            if video_encoder == "libx265" {
                cmd.arg("-crf").arg(crf_h264.to_string());
            } else {
                cmd.arg("-b:v").arg(format!("{target_kbps}k"));
            }
            cmd.arg("-pix_fmt").arg("yuv420p").arg("-tag:v").arg("hvc1");
        }
        (VideoFormat::Mp4, _) | (VideoFormat::Mov, VideoCodec::H264) => {
            cmd.arg("-c:v").arg(video_encoder);
            if video_encoder == "libx264" {
                cmd.arg("-preset")
                    .arg("medium")
                    .arg("-crf")
                    .arg(crf_h264.to_string());
            } else {
                cmd.arg("-b:v").arg(format!("{target_kbps}k"));
            }
            cmd.arg("-pix_fmt").arg("yuv420p");
            if options.format == VideoFormat::Mp4 {
                cmd.arg("-movflags").arg("+faststart");
            }
        }
        (VideoFormat::Mov, VideoCodec::ProRes) => {
            cmd.arg("-c:v")
                .arg(video_encoder)
                .arg("-profile:v")
                .arg(if transparent_bg { "4" } else { "3" })
                .arg("-pix_fmt")
                .arg(if transparent_bg {
                    "yuva444p10le"
                } else {
                    "yuv422p10le"
                });
        }
        (VideoFormat::WebM, VideoCodec::Av1) => {
            cmd.arg("-c:v")
                .arg(video_encoder)
                .arg("-crf")
                .arg(crf_vp9.to_string())
                .arg("-pix_fmt")
                .arg("yuv420p");
        }
        (VideoFormat::WebM, _) => {
            cmd.arg("-c:v")
                .arg(video_encoder)
                .arg("-b:v")
                .arg("0")
                .arg("-crf")
                .arg(crf_vp9.to_string())
                .arg("-pix_fmt")
                .arg(if transparent_bg {
                    "yuva420p"
                } else {
                    "yuv420p"
                });
        }
        _ => {}
    }

    if staged_audio_path.is_some() {
        match (options.format, options.audio_bitrate) {
            (VideoFormat::WebM, _) | (_, VideoAudioBitrate::Opus160) => {
                cmd.arg("-c:a").arg("libopus").arg("-b:a").arg("160k");
            }
            (VideoFormat::Mov, VideoAudioBitrate::Pcm16) => {
                cmd.arg("-c:a").arg("pcm_s16le");
            }
            (_, bitrate) => {
                let kbps = bitrate.kbps().clamp(96, 320);
                cmd.arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg(format!("{kbps}k"));
            }
        }
        cmd.arg("-shortest");
    } else {
        cmd.arg("-an");
    }

    cmd.arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ExportError::Options(format!("Failed to launch ffmpeg: {e}")))?;

    let mut write_err = None;
    if let Some(mut stdin) = child.stdin.take() {
        for frame in frames {
            if let Err(e) = stdin.write_all(frame) {
                write_err = Some(e);
                break;
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| ExportError::Options(format!("Failed waiting for ffmpeg: {e}")))?;

    if !output.status.success() {
        let _ = fs::remove_file(output_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ExportError::Options(format!(
            "ffmpeg encoding failed ({video_encoder}): {}",
            stderr.trim()
        )));
    }
    if let Some(e) = write_err {
        let _ = fs::remove_file(output_path);
        return Err(ExportError::Io(e));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_with_ffmpeg(
    output_path: &Path,
    options: &VideoOptions,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    transparent_bg: bool,
    wav_audio: Option<&[u8]>,
    mut warnings: Vec<String>,
) -> Result<VideoExportReport, ExportError> {
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        ExportError::Options(
            "FFmpeg was not found. Install ffmpeg (e.g. `brew install ffmpeg`), set NEOFLASH_FFMPEG, or export as GIF / PNG seq."
                .into(),
        )
    })?;

    let staged_audio = wav_audio
        .map(|wav| StagedMediaFile::create("timeline audio", "wav", wav))
        .transpose()
        .map_err(ExportError::Options)?;
    let staged_audio_path = staged_audio.as_ref().map(|s| s.path());

    let codec = options.resolved_codec();
    let prefer_hardware = match env::var("NEOFLASH_VIDEO_ENCODER_MODE")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("hardware") => true,
        Some("software") => false,
        _ => false,
    };
    let (primary_encoder, is_hw) = select_video_encoder(&ffmpeg, codec, prefer_hardware);

    if let Err(primary_err) = run_ffmpeg_encode_attempt(
        &ffmpeg,
        output_path,
        options,
        frames,
        width,
        height,
        fps,
        transparent_bg,
        staged_audio_path,
        primary_encoder,
    ) {
        if is_hw {
            let (sw_encoder, _) = select_video_encoder(&ffmpeg, codec, false);
            if sw_encoder != primary_encoder && probe_ffmpeg_encoders(&ffmpeg).contains(sw_encoder)
            {
                warnings.push(format!(
                    "Hardware video encoder '{primary_encoder}' was unavailable for this configuration; fell back to software encoder '{sw_encoder}'."
                ));
                run_ffmpeg_encode_attempt(
                    &ffmpeg,
                    output_path,
                    options,
                    frames,
                    width,
                    height,
                    fps,
                    transparent_bg,
                    staged_audio_path,
                    sw_encoder,
                )?;
            } else {
                return Err(primary_err);
            }
        } else {
            return Err(primary_err);
        }
    }

    let bytes = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    // Unused imageops import guard if resizing thumbnails is used by tests/callers.
    let _ = FilterType::Triangle;

    Ok(VideoExportReport {
        output_path: output_path.to_path_buf(),
        additional_files: Vec::new(),
        bytes,
        frame_count: frames.len() as u32,
        width,
        height,
        audio_included: wav_audio.is_some(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        Element, FrameSound, Keyframe, Layer, ResourceUnit, SoundSync, Symbol, Timeline, Transform,
    };

    #[test]
    fn estimate_and_render_demo_project_frames() {
        let project = Project::demo();
        let options = VideoOptions {
            width: 320,
            height: 200,
            fps: 24.0,
            ..VideoOptions::default()
        };
        let estimate = estimate_video(&project, &options).expect("estimate");
        assert!(estimate.frame_count >= 1);
        assert_eq!(estimate.width, 320);
        assert_eq!(estimate.height, 200);
        assert!(estimate.estimated_bytes > 0);

        let rgba = render_project_frame_rgba(&project, None, 0, 320, 200, false, true)
            .expect("render frame 0");
        assert_eq!(rgba.len(), 320 * 200 * 4);
        assert!(rgba.as_chunks::<4>().0.iter().any(|px| px[3] == 255));
    }

    #[test]
    fn exports_gif_and_png_sequence_and_honors_stop_after() {
        let project = Project::demo();
        let dir = std::env::temp_dir().join(format!("neoflash-video-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let gif_path = dir.join("demo.gif");
        let gif_report = export_project_video(
            &project,
            &gif_path,
            &VideoOptions {
                format: VideoFormat::Gif,
                width: 160,
                height: 100,
                fps: 10.0,
                stop_mode: VideoStopMode::AfterSeconds(0.3),
                ..VideoOptions::default()
            },
        )
        .expect("export gif");
        assert_eq!(gif_report.frame_count, 3);
        assert!(gif_path.is_file());
        assert!(gif_report.bytes > 64);

        let png_path = dir.join("seq.png");
        let png_report = export_project_video(
            &project,
            &png_path,
            &VideoOptions {
                format: VideoFormat::PngSequence,
                width: 120,
                height: 80,
                from_frame: 1,
                to_frame: Some(2),
                ..VideoOptions::default()
            },
        )
        .expect("export png sequence");
        assert_eq!(png_report.frame_count, 2);
        assert!(dir.join("seq_0001.png").is_file());
        assert!(dir.join("seq_0002.png").is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn runtime_capture_toggle_controls_nested_movie_clip_animation() {
        let mut project = Project::blank();
        project.width = 100;
        project.height = 100;
        let child_id = "mc_child".to_owned();
        project.symbols.push(Symbol {
            id: child_id.clone(),
            name: "ChildMC".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 2,
                layers: vec![Layer {
                    id: "cl".into(),
                    name: "L1".into(),
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            label: String::new(),
                            elements: vec![Element {
                                id: "r0".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Rectangle {
                                    width: 100.0,
                                    height: 100.0,
                                    fill: [255, 0, 0, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            tween: None,
                            tween_kind: Default::default(),
                            keyed_properties: None,
                            orient_to_path: false,
                            property_eases: Vec::new(),
                            motion_path: None,
                            sync_symbols: false,
                            snap_to_guide: false,
                            sound: None,
                            shape_hints: Vec::new(),
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 1,
                            label: String::new(),
                            elements: vec![Element {
                                id: "r1".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Rectangle {
                                    width: 100.0,
                                    height: 100.0,
                                    fill: [0, 255, 0, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            tween: None,
                            tween_kind: Default::default(),
                            keyed_properties: None,
                            orient_to_path: false,
                            property_eases: Vec::new(),
                            motion_path: None,
                            sync_symbols: false,
                            snap_to_guide: false,
                            sound: None,
                            shape_hints: Vec::new(),
                            ..Keyframe::default()
                        },
                    ],
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            linkage_id: None,
            class_name: None,
        });
        let root_sym = project.symbol_mut(&project.root.clone()).unwrap();
        root_sym.timeline.frame_count = 2;
        root_sym.timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "inst".into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: child_id,
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            });

        let captured_f1 =
            render_project_frame_rgba(&project, None, 1, 32, 32, false, true).unwrap();
        let static_f1 = render_project_frame_rgba(&project, None, 1, 32, 32, false, false).unwrap();
        // Frame 1 with runtime_capture renders green (child frame 1); without runtime_capture renders red (child frame 0).
        let center_idx = (16 * 32 + 16) * 4;
        assert_eq!(&captured_f1[center_idx..center_idx + 3], &[0, 255, 0]);
        assert_eq!(&static_f1[center_idx..center_idx + 3], &[255, 0, 0]);
    }

    #[test]
    fn mixes_timeline_audio_and_exports_mp4_when_ffmpeg_present() {
        let mut project = Project::demo();
        // Attach a small 44.1 kHz sine wave WAV resource to frame 0.
        let mut samples = Vec::with_capacity(4410);
        for i in 0..4410 {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin()) * 0.5;
            samples.push([v, v]);
        }
        let wav_bytes = encode_wav_pcm16(&samples, 44_100);
        project.resources.push(
            ResourceUnit::from_bytes("snd_1", "audio/tone.wav", "audio/wav", &wav_bytes)
                .expect("resource unit"),
        );
        let root_id = project.root.clone();
        let mut snd = FrameSound::new("snd_1");
        snd.sync = SoundSync::Event;
        project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].sound = Some(snd);
        project.format_version = 6;

        if detect_ffmpeg().is_some() {
            let dir =
                std::env::temp_dir().join(format!("neoflash-mp4-test-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let mp4_path = dir.join("out.mp4");
            let report = export_project_video(
                &project,
                &mp4_path,
                &VideoOptions {
                    format: VideoFormat::Mp4,
                    width: 160,
                    height: 100,
                    from_frame: 1,
                    to_frame: Some(3),
                    include_audio: true,
                    ..VideoOptions::default()
                },
            )
            .expect("export mp4");
            assert!(mp4_path.is_file());
            assert!(report.bytes > 256);
            assert!(report.audio_included);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    fn build_four_color_mp4_fixture() -> Vec<u8> {
        let w = 16u32;
        let h = 16u32;
        let colors: [[u8; 4]; 4] = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
        ];
        let frames: Vec<Vec<u8>> = colors
            .into_iter()
            .map(|c| {
                let mut buf = Vec::with_capacity((w * h * 4) as usize);
                for _ in 0..(w * h) {
                    buf.extend_from_slice(&c);
                }
                buf
            })
            .collect();
        let frame_refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
        encode_deterministic_mp4_rgba_frames(w, h, 10, &frame_refs)
    }

    fn build_solid_mp4_fixture(color: [u8; 4]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for _ in 0..(16 * 16) {
            rgba.extend_from_slice(&color);
        }
        encode_deterministic_mp4_rgba_frames(16, 16, 10, &[&rgba])
    }

    #[test]
    fn pam_video_pipe_is_parsed_incrementally_without_a_whole_stream_buffer() {
        let mut bytes = Vec::new();
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            bytes.extend_from_slice(
                b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
            );
            bytes.extend_from_slice(&color);
        }
        let mut reader = BufReader::new(Cursor::new(bytes));
        let raw = parse_pam_rgba_frames(&mut reader).unwrap();
        assert_eq!((raw.width, raw.height, raw.images.len()), (1, 1, 2));
        let stream = finish_pam_rgba_frames(raw, 10.0).unwrap();
        assert_eq!(stream.duration_ms, 200);
        assert_eq!(&stream.frames[0].image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&stream.frames[1].image.rgba[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn video_decode_rejects_oversized_pam_before_allocating_payload() {
        let header = format!(
            "P7\nWIDTH {}\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nENDHDR\n",
            MAX_DECODED_VIDEO_WIDTH + 1
        );
        let mut reader = BufReader::new(Cursor::new(header.into_bytes()));
        let error = parse_pam_rgba_frames(&mut reader).unwrap_err();
        assert!(error.contains("dimensions"), "got: {error}");

        let error = checked_decoded_video_payload_len(4096, 4096, 9).unwrap_err();
        assert!(error.contains("retained-frame budget"), "got: {error}");
        let error = checked_decoded_video_duration_ms(1, 0.0001).unwrap_err();
        assert!(error.contains("duration"), "got: {error}");
    }

    #[test]
    fn staged_video_and_wav_files_are_exclusive_and_removed_by_drop_guard() {
        for (name, extension) in [("fixture", "mp4"), ("timeline audio", "wav")] {
            let staged = StagedMediaFile::create(name, extension, b"safe bytes").unwrap();
            let path = staged.path().to_path_buf();
            assert_eq!(
                path.extension().and_then(|value| value.to_str()),
                Some(extension)
            );
            assert_eq!(fs::read(&path).unwrap(), b"safe bytes");
            drop(staged);
            assert!(!path.exists());
        }
    }

    #[test]
    fn external_video_read_is_bounded_before_allocation() {
        let path = env::temp_dir().join(format!(
            "neoflash-bounded-external-video-{}-{}",
            std::process::id(),
            VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let file = fs::File::create(&path).unwrap();
        file.set_len(17).unwrap();
        drop(file);
        let error = read_bounded_video_file_with_limit(&path, "large.mp4", 16).unwrap_err();
        let _ = fs::remove_file(path);
        assert!(error.contains("input budget"), "got: {error}");
    }

    #[test]
    fn video_cache_budget_counts_tiny_frame_records_and_shared_aliases() {
        let make_stream = |color: [u8; 4]| {
            Arc::new(DecodedVideoStream {
                width: 1,
                height: 1,
                fps: 24.0,
                duration_ms: 42,
                frames: vec![DecodedVideoFrame {
                    timestamp_ms: 0,
                    duration_ms: 42,
                    image: Arc::new(DecodedImage {
                        width: 1,
                        height: 1,
                        rgba: color.to_vec().into(),
                    }),
                }],
                on_demand: None,
            })
        };
        let mut cache = HashMap::new();
        let first = make_stream([1, 2, 3, 255]);
        insert_decoded_video_stream_with_limits(
            &mut cache,
            "first".into(),
            first.clone(),
            usize::MAX,
            2,
        )
        .unwrap();
        // A second key for the same allocation must not double-count resident bytes.
        insert_decoded_video_stream_with_limits(&mut cache, "alias".into(), first, usize::MAX, 2)
            .unwrap();
        insert_decoded_video_stream_with_limits(
            &mut cache,
            "second".into(),
            make_stream([4, 5, 6, 255]),
            usize::MAX,
            2,
        )
        .unwrap();
        let error = insert_decoded_video_stream_with_limits(
            &mut cache,
            "third".into(),
            make_stream([7, 8, 9, 255]),
            usize::MAX,
            2,
        )
        .unwrap_err();
        assert!(error.contains("aggregate 2-frame"), "got: {error}");
        assert!(!cache.contains_key("third"));

        let mut structural_cache = HashMap::new();
        let error = insert_decoded_video_stream_with_limits(
            &mut structural_cache,
            "tiny".into(),
            make_stream([0, 0, 0, 255]),
            4,
            100,
        )
        .unwrap_err();
        assert!(
            error.contains("frame records and cache structures"),
            "got: {error}"
        );
    }

    #[test]
    fn bounded_ffmpeg_diagnostics_keep_early_fps_and_final_error() {
        let mut bytes =
            b"Input #0\n  Stream #0: Video: h264, yuv420p, 29.97 fps, 30 tbr\n".to_vec();
        bytes.resize(MAX_FFMPEG_DIAGNOSTIC_BYTES + 20_000, b'x');
        bytes.extend_from_slice(b"\nfinal codec error\n");
        let capture = read_bounded_process_diagnostics(Cursor::new(bytes)).unwrap();
        let head = String::from_utf8_lossy(&capture.head);
        let tail = String::from_utf8_lossy(&capture.tail);
        assert_eq!(parse_ffmpeg_fps_from_stderr(&head), Some(29.97));
        assert!(tail.contains("final codec error"));
    }

    #[test]
    fn ffprobe_metadata_rejects_decoder_bombs_and_preserves_fractional_fps() {
        let metadata = parse_ffprobe_metadata(
            "width=1920\nheight=1080\nr_frame_rate=30/1\navg_frame_rate=30000/1001\nduration=1.001\nnb_frames=30\n",
        )
        .unwrap();
        assert_eq!((metadata.width, metadata.height), (1920, 1080));
        assert!((metadata.fps - 29.970_03).abs() < 0.001);

        let error = parse_ffprobe_metadata(
            "width=8192\nheight=8192\nr_frame_rate=24/1\navg_frame_rate=24/1\nduration=1\nnb_frames=24\n",
        )
        .unwrap_err();
        assert!(error.contains("dimensions"), "got: {error}");
        let error = parse_ffprobe_metadata(
            "width=1920\nheight=1080\nr_frame_rate=60/1\navg_frame_rate=60/1\nduration=10\nnb_frames=600\n",
        )
        .unwrap_err();
        assert!(error.contains("retained-frame budget"), "got: {error}");
    }

    #[test]
    fn video_decode_cache_distinguishes_source_overrides_for_one_resource() {
        use neoflash_core::{LayerProperties, TimelineVideoSpan};
        let dir = std::env::temp_dir().join(format!(
            "neoflash-video-source-cache-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("media")).unwrap();
        fs::write(
            dir.join("media/red.mp4"),
            build_solid_mp4_fixture([255, 0, 0, 255]),
        )
        .unwrap();
        fs::write(
            dir.join("media/blue.mp4"),
            build_solid_mp4_fixture([0, 0, 255, 255]),
        )
        .unwrap();

        let mut project = Project::blank();
        project.width = 32;
        project.height = 16;
        project
            .resources
            .push(ResourceUnit::from_bytes("shared", "media/red.mp4", "video/mp4", &[]).unwrap());
        let make_layer = |id: &str, element_id: &str, x: f64, path: &str| Layer {
            id: id.into(),
            name: id.into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: element_id.into(),
                    transform: Transform {
                        x,
                        ..Transform::default()
                    },
                    kind: ElementKind::Video {
                        width: 16.0,
                        height: 16.0,
                    },
                    ..Element::default()
                }],
                video: Some(TimelineVideoSpan::external("shared", path)),
                ..Keyframe::default()
            }],
            properties: LayerProperties::default(),
        };
        project.symbols[0].timeline.layers = vec![
            make_layer("red-layer", "red-video", 0.0, "media/red.mp4"),
            make_layer("blue-layer", "blue-video", 16.0, "media/blue.mp4"),
        ];
        project.upgrade_format_for_features();
        project.validate().unwrap();

        let rgba = render_project_frame_rgba_with_base_dir(
            &project,
            None,
            0,
            32,
            16,
            false,
            false,
            Some(&dir),
        )
        .unwrap();
        let pixel = |x: usize, y: usize| &rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(pixel(8, 8), &[255, 0, 0, 255]);
        assert_eq!(pixel(24, 8), &[0, 0, 255, 255]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn t201_offline_video_export_composites_decoded_embedded_and_external_frames_sync_trims_nested_and_transforms()
     {
        use neoflash_core::{LayerProperties, TimelineVideoSpan};
        let mp4_bytes = build_four_color_mp4_fixture();
        let mut project = Project::blank();
        project.width = 64;
        project.height = 64;
        project.fps = 10;
        project.background = [10, 10, 10, 255];
        project.resources.push(
            ResourceUnit::from_bytes("vid_1", "clips/four.mp4", "video/mp4", &mp4_bytes)
                .expect("video resource"),
        );

        let root_id = project.root.clone();
        {
            let root = project.symbol_mut(&root_id).unwrap();
            root.timeline.frame_count = 4;
            let kf = &mut root.timeline.layers[0].keyframes[0];
            kf.frame = 0;
            kf.video = Some(TimelineVideoSpan::embedded("vid_1"));
            kf.elements = vec![Element {
                id: "v_el".into(),
                name: "video_inst".into(),
                transform: Transform {
                    x: 8.0,
                    y: 8.0,
                    scale_x: 2.0,
                    scale_y: 2.0,
                    ..Transform::default()
                },
                kind: ElementKind::Video {
                    width: 16.0,
                    height: 16.0,
                },
                ..Element::default()
            }];
        }
        project.upgrade_format_for_features();
        project.validate().expect("valid video project");

        // 1. Stream sync across frames 0..4 produces Red, Green, Blue, Yellow at (16, 16).
        let sample_px = |rgba: &[u8], x: u32, y: u32| -> [u8; 4] {
            let idx = ((y * 64 + x) * 4) as usize;
            [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
        };
        let f0 = render_project_frame_rgba(&project, None, 0, 64, 64, false, false).unwrap();
        let f1 = render_project_frame_rgba(&project, None, 1, 64, 64, false, false).unwrap();
        let f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, false).unwrap();
        let f3 = render_project_frame_rgba(&project, None, 3, 64, 64, false, false).unwrap();
        assert_eq!(sample_px(&f0, 16, 16), [255, 0, 0, 255]);
        assert_eq!(sample_px(&f1, 16, 16), [0, 255, 0, 255]);
        assert_eq!(sample_px(&f2, 16, 16), [0, 0, 255, 255]);
        assert_eq!(sample_px(&f3, 16, 16), [255, 255, 0, 255]);
        // Outside the placed 32x32 scaled video quad (at 4, 4), background remains [10, 10, 10, 255].
        assert_eq!(sample_px(&f0, 4, 4), [10, 10, 10, 255]);

        // 2. Trim [in_point_ms = 100, out_point_ms = 300] (Green & Blue only):
        {
            let span = project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
                .video
                .as_mut()
                .unwrap();
            span.start_offset_ms = 100;
            span.end_offset_ms = Some(300);
            span.loop_video = false;
        }
        project.upgrade_format_for_features();
        let clamp_f0 = render_project_frame_rgba(&project, None, 0, 64, 64, false, false).unwrap();
        let clamp_f1 = render_project_frame_rgba(&project, None, 1, 64, 64, false, false).unwrap();
        let clamp_f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, false).unwrap();
        assert_eq!(sample_px(&clamp_f0, 16, 16), [0, 255, 0, 255]);
        assert_eq!(sample_px(&clamp_f1, 16, 16), [0, 0, 255, 255]);
        assert_eq!(
            sample_px(&clamp_f2, 16, 16),
            [0, 0, 255, 255],
            "clamped at out_point_ms when loop_video is false"
        );

        {
            let span = project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0]
                .video
                .as_mut()
                .unwrap();
            span.loop_video = true;
        }
        let loop_f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, false).unwrap();
        let loop_f3 = render_project_frame_rgba(&project, None, 3, 64, 64, false, false).unwrap();
        assert_eq!(
            sample_px(&loop_f2, 16, 16),
            [0, 255, 0, 255],
            "wrapped back to in_point_ms (100ms = Green) when loop_video is true"
        );
        assert_eq!(sample_px(&loop_f3, 16, 16), [0, 0, 255, 255]);

        // 3. VideoSyncMode::Event + VideoSyncMode::Stop retains the paused frame at the Stop keyframe:
        {
            let root = project.symbol_mut(&root_id).unwrap();
            let el = root.timeline.layers[0].keyframes[0].elements[0].clone();
            let mut start_span = TimelineVideoSpan::embedded("vid_1");
            start_span.sync = VideoSyncMode::Event;
            root.timeline.layers[0].keyframes[0].video = Some(start_span);
            let mut stop_span = TimelineVideoSpan::embedded("vid_1");
            stop_span.sync = VideoSyncMode::Stop;
            root.timeline.layers[0].keyframes.push(Keyframe {
                frame: 2,
                elements: vec![el],
                video: Some(stop_span),
                ..Keyframe::default()
            });
        }
        let ev_f0 = render_project_frame_rgba(&project, None, 0, 64, 64, false, true).unwrap();
        let ev_f1 = render_project_frame_rgba(&project, None, 1, 64, 64, false, true).unwrap();
        let ev_f2 = render_project_frame_rgba(&project, None, 2, 64, 64, false, true).unwrap();
        let ev_f3 = render_project_frame_rgba(&project, None, 3, 64, 64, false, true).unwrap();
        assert_eq!(sample_px(&ev_f0, 16, 16), [255, 0, 0, 255]);
        assert_eq!(sample_px(&ev_f1, 16, 16), [0, 255, 0, 255]);
        assert_eq!(
            sample_px(&ev_f2, 16, 16),
            [0, 255, 0, 255],
            "Stop keyframe at frame 2 retains the last presented frame (Green)"
        );
        assert_eq!(sample_px(&ev_f3, 16, 16), [0, 255, 0, 255]);

        // 4. External deployment relative to base_dir + nested MovieClip + layer mask + PNG sequence & MP4 export:
        let dir = std::env::temp_dir().join(format!("neoflash-t201-video-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("media")).unwrap();
        let external_rel = "media/clip.mp4";
        fs::write(dir.join(external_rel), &mp4_bytes).unwrap();

        let mut ext_project = Project::blank();
        ext_project.width = 64;
        ext_project.height = 64;
        ext_project.fps = 10;
        ext_project.background = [0, 0, 0, 255];
        ext_project
            .resources
            .push(ResourceUnit::from_bytes("vid_ext", external_rel, "video/mp4", &[]).unwrap());
        let child_id = "nested_video_mc".to_owned();
        let ext_span = TimelineVideoSpan::external("vid_ext", external_rel);
        ext_project.symbols.push(Symbol {
            id: child_id.clone(),
            name: "NestedVideo".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 4,
                layers: vec![
                    Layer {
                        id: "masked_video_layer".into(),
                        name: "Video".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            role: LayerRole::Normal,
                            mask: Some("mask_layer".into()),
                            ..LayerProperties::default()
                        },
                        keyframes: vec![Keyframe {
                            frame: 0,
                            video: Some(ext_span),
                            elements: vec![Element {
                                id: "nested_v".into(),
                                name: String::new(),
                                transform: Transform {
                                    x: 0.0,
                                    y: 0.0,
                                    scale_x: 4.0,
                                    scale_y: 4.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Video {
                                    width: 16.0,
                                    height: 16.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    },
                    Layer {
                        id: "mask_layer".into(),
                        name: "Mask".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            role: LayerRole::Mask,
                            ..LayerProperties::default()
                        },
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "mask_rect".into(),
                                name: String::new(),
                                transform: Transform::default(),
                                kind: ElementKind::Rectangle {
                                    width: 32.0,
                                    height: 64.0,
                                    fill: [255, 255, 255, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    },
                ],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            linkage_id: None,
            class_name: None,
        });
        let ext_root = ext_project.root.clone();
        {
            let root = ext_project.symbol_mut(&ext_root).unwrap();
            root.timeline.frame_count = 4;
            root.timeline.layers[0].keyframes[0].elements = vec![Element {
                id: "child_inst".into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Instance {
                    symbol_id: child_id,
                    playback: InstancePlayback::Independent,
                },
                ..Element::default()
            }];
        }
        ext_project.upgrade_format_for_features();
        ext_project
            .validate()
            .expect("valid external nested project");

        let seq_path = dir.join("out_seq.png");
        let seq_report = export_project_video(
            &ext_project,
            &seq_path,
            &VideoOptions {
                format: VideoFormat::PngSequence,
                width: 64,
                height: 64,
                from_frame: 1,
                to_frame: Some(3),
                runtime_capture: true,
                base_dir: Some(dir.clone()),
                ..VideoOptions::default()
            },
        )
        .expect("export png sequence with external nested video");
        assert_eq!(seq_report.frame_count, 3);
        let png1 = image::open(dir.join("out_seq_0001.png"))
            .unwrap()
            .to_rgba8();
        let png2 = image::open(dir.join("out_seq_0002.png"))
            .unwrap()
            .to_rgba8();
        let png3 = image::open(dir.join("out_seq_0003.png"))
            .unwrap()
            .to_rgba8();
        // Inside left-half mask (x = 16, y = 16): Red, Green, Blue across frames 1..=3.
        assert_eq!(png1.get_pixel(16, 16).0, [255, 0, 0, 255]);
        assert_eq!(png2.get_pixel(16, 16).0, [0, 255, 0, 255]);
        assert_eq!(png3.get_pixel(16, 16).0, [0, 0, 255, 255]);
        // Outside left-half mask (x = 48, y = 16): clipped to black background [0, 0, 0, 255].
        assert_eq!(png1.get_pixel(48, 16).0, [0, 0, 0, 255]);

        // When ffmpeg is available, also verify real H.264 MP4 export + decoding that real H.264 MP4 back!
        if detect_ffmpeg().is_some() {
            let real_mp4_path = dir.join("media/encoded_h264.mp4");
            export_project_video(
                &project,
                &real_mp4_path,
                &VideoOptions {
                    format: VideoFormat::Mp4,
                    width: 64,
                    height: 64,
                    from_frame: 1,
                    to_frame: Some(2),
                    include_audio: false,
                    ..VideoOptions::default()
                },
            )
            .expect("export real H.264 mp4");
            let real_mp4_bytes = fs::read(&real_mp4_path).unwrap();
            let decoded_real =
                decode_video_stream_bytes("encoded_h264.mp4", "video/mp4", &real_mp4_bytes)
                    .expect("decode real H.264 mp4 via ffmpeg");
            assert!(decoded_real.frames.len() >= 2);
            assert_eq!(decoded_real.width, 64);
            assert_eq!(decoded_real.height, 64);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn t201_offline_video_export_fails_actionably_on_missing_external_corrupt_or_progressive_without_partial_output()
     {
        use neoflash_core::TimelineVideoSpan;
        let dir =
            std::env::temp_dir().join(format!("neoflash-t201-neg-video-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let make_project_with_span = |res: ResourceUnit, span: TimelineVideoSpan| -> Project {
            let mut p = Project::blank();
            p.width = 32;
            p.height = 32;
            p.resources.push(res);
            let root_id = p.root.clone();
            let root = p.symbol_mut(&root_id).unwrap();
            root.timeline.layers[0].keyframes[0].video = Some(span);
            root.timeline.layers[0].keyframes[0].elements = vec![Element {
                id: "v".into(),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Video {
                    width: 32.0,
                    height: 32.0,
                },
                ..Element::default()
            }];
            p.upgrade_format_for_features();
            p
        };

        // 1. Progressive remote URL in offline export fails actionably without creating output.
        let prog_span =
            TimelineVideoSpan::progressive("vid_prog", "https://cdn.example.com/remote.mp4");
        let prog_proj = make_project_with_span(
            ResourceUnit::from_bytes("vid_prog", "clips/remote.mp4", "video/mp4", &[]).unwrap(),
            prog_span,
        );
        let prog_out = dir.join("prog.gif");
        let prog_err = export_project_video(
            &prog_proj,
            &prog_out,
            &VideoOptions {
                format: VideoFormat::Gif,
                width: 32,
                height: 32,
                ..VideoOptions::default()
            },
        )
        .expect_err("progressive remote URL must fail offline export");
        assert!(prog_err.to_string().contains("Progressive"));
        assert!(!prog_out.exists(), "no partial output file may remain");

        // 2. Missing external file fails actionably without creating output.
        let miss_span = TimelineVideoSpan::external("vid_miss", "clips/does_not_exist.mp4");
        let miss_proj = make_project_with_span(
            ResourceUnit::from_bytes("vid_miss", "clips/does_not_exist.mp4", "video/mp4", &[])
                .unwrap(),
            miss_span,
        );
        let miss_out = dir.join("miss.gif");
        let miss_err = export_project_video(
            &miss_proj,
            &miss_out,
            &VideoOptions {
                format: VideoFormat::Gif,
                width: 32,
                height: 32,
                base_dir: Some(dir.clone()),
                ..VideoOptions::default()
            },
        )
        .expect_err("missing external file must fail offline export");
        assert!(miss_err.to_string().contains("was not found"));
        assert!(!miss_out.exists(), "no partial output file may remain");

        // 3. Corrupt / unsupported codec fails actionably without creating output.
        let corrupt_proj = make_project_with_span(
            ResourceUnit::from_bytes("vid_bad", "corrupt.mp4", "video/mp4", b"not-a-valid-mp4")
                .unwrap(),
            TimelineVideoSpan::embedded("vid_bad"),
        );
        let corrupt_out = dir.join("corrupt.gif");
        let corrupt_err = export_project_video(
            &corrupt_proj,
            &corrupt_out,
            &VideoOptions {
                format: VideoFormat::Gif,
                width: 32,
                height: 32,
                ..VideoOptions::default()
            },
        )
        .expect_err("corrupt video container must fail offline export");
        assert!(
            corrupt_err.to_string().contains("corrupt.mp4")
                || corrupt_err.to_string().contains("Unsupported")
        );
        assert!(!corrupt_out.exists(), "no partial output file may remain");

        // The shared authored-scene rasterizer must propagate the same decoder failure to every
        // export family instead of silently omitting the video layer.
        let atlas_out = dir.join("corrupt_atlas.json");
        let atlas_err = crate::atlas::export_project_atlas(
            &corrupt_proj,
            &atlas_out,
            &crate::atlas::AtlasOptions::default(),
        )
        .expect_err("atlas export must propagate corrupt video decode");
        assert!(
            atlas_err.to_string().contains("corrupt.mp4")
                || atlas_err.to_string().contains("Unsupported"),
            "got: {atlas_err}"
        );
        assert!(!atlas_out.exists());
        assert!(!atlas_out.with_extension("png").exists());

        for (format, filename) in [
            (crate::image_export::ImageExportFormat::Psd, "corrupt.psd"),
            (crate::image_export::ImageExportFormat::Ai, "corrupt.ai"),
        ] {
            let output = dir.join(filename);
            let mut image_options =
                crate::image_export::ImageExportOptions::for_project(&corrupt_proj, 0);
            image_options.format = format;
            let error =
                crate::image_export::export_project_image(&corrupt_proj, &output, &image_options)
                    .expect_err("raster image export must propagate corrupt video decode");
            assert!(
                error.to_string().contains("corrupt.mp4")
                    || error.to_string().contains("Unsupported"),
                "got: {error}"
            );
            assert!(!output.exists(), "no partial {format:?} output may remain");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atlas_video_preflight_uses_exact_later_frame_and_target_symbol() {
        use neoflash_core::{SymbolKind, TimelineVideoSpan};

        let video_element = || Element {
            id: "video".into(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Video {
                width: 16.0,
                height: 16.0,
            },
            ..Element::default()
        };
        let corrupt_resource =
            || ResourceUnit::from_bytes("bad", "bad.mp4", "video/mp4", b"not-a-valid-mp4").unwrap();
        let dir = env::temp_dir().join(format!(
            "neoflash-atlas-video-context-{}-{}",
            std::process::id(),
            VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut later = Project::blank();
        later.width = 16;
        later.height = 16;
        later.resources.push(corrupt_resource());
        let root_id = later.root.clone();
        let root = later.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 2;
        root.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "safe".into(),
                    kind: ElementKind::Rectangle {
                        width: 4.0,
                        height: 4.0,
                        fill: [255, 0, 0, 255],
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 1,
                elements: vec![video_element()],
                video: Some(TimelineVideoSpan::embedded("bad")),
                ..Keyframe::default()
            },
        ];
        later.upgrade_format_for_features();
        let later_path = dir.join("later.json");
        let later_error = crate::atlas::export_project_atlas(
            &later,
            &later_path,
            &crate::atlas::AtlasOptions {
                start_frame: 0,
                end_frame: Some(1),
                ..crate::atlas::AtlasOptions::default()
            },
        )
        .expect_err("frame-1 corrupt video must not disappear from atlas preflight");
        assert!(later_error.to_string().contains("bad.mp4"));
        assert!(!later_path.exists());
        assert!(!later_path.with_extension("png").exists());

        let mut nested = Project::blank();
        nested.width = 16;
        nested.height = 16;
        nested.resources.push(corrupt_resource());
        let child_id = "video_child".to_owned();
        nested.symbols.push(Symbol {
            id: child_id.clone(),
            name: "Video child".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "video_layer".into(),
                    name: "Video".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![video_element()],
                        video: Some(TimelineVideoSpan::embedded("bad")),
                        ..Keyframe::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            class_name: None,
            linkage_id: None,
        });
        nested.upgrade_format_for_features();
        let child_path = dir.join("child.json");
        let child_error = crate::atlas::export_project_atlas(
            &nested,
            &child_path,
            &crate::atlas::AtlasOptions {
                symbol_id: Some(child_id),
                ..crate::atlas::AtlasOptions::default()
            },
        )
        .expect_err("target-symbol corrupt video must not resolve against project root");
        assert!(child_error.to_string().contains("bad.mp4"));
        assert!(!child_path.exists());
        assert!(!child_path.with_extension("png").exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn t229_long_form_video_decodes_on_demand_within_pinned_ring_and_matches_seeks_and_trims() {
        use neoflash_core::ResourceUnit;

        // 1. Short clip fast path (<= MAX_EAGER_VIDEO_FRAMES and <= 32 MiB): retains eager frames.
        let short_mp4 = build_four_color_mp4_fixture();
        let short_stream = decode_video_stream_bytes("short.mp4", "video/mp4", &short_mp4).unwrap();
        assert!(short_stream.on_demand.is_none());
        assert_eq!(short_stream.frames.len(), 4);

        // 2. Long-form deterministic nfvf stream (240 frames > MAX_EAGER_VIDEO_FRAMES = 180):
        // switches automatically to OnDemandVideoDecoder with bounded ring eviction.
        let frame_count = 240usize;
        let raw_frames: Vec<Vec<u8>> = (0..frame_count)
            .map(|idx| [idx as u8, ((idx * 3) & 0xFF) as u8, 42, 255].repeat(8 * 8))
            .collect();
        let raw_slices: Vec<&[u8]> = raw_frames.iter().map(Vec::as_slice).collect();
        let long_nfvf = encode_deterministic_mp4_rgba_frames(8, 8, 20, &raw_slices);
        let long_stream = decode_video_stream_bytes("long.mp4", "video/mp4", &long_nfvf).unwrap();
        assert!(long_stream.frames.is_empty());
        let decoder = long_stream
            .on_demand
            .as_ref()
            .expect("expected on-demand windowed decoder");
        assert_eq!(decoder.logical_frame_count(), frame_count);
        assert!(decoder.max_ring_frames() <= MAX_WINDOWED_RING_FRAMES);

        let make_placement = |time_ms: u32,
                              in_point_ms: u32,
                              out_point_ms: Option<u32>,
                              loop_video: bool,
                              sync: VideoSyncMode,
                              stopped: bool| ResolvedVideoPlacement {
            channel: "video".into(),
            target_path: vec!["vid".into()],
            layer_id: "layer".into(),
            resource: "res".into(),
            source: None,
            sync,
            time_ms,
            in_point_ms,
            out_point_ms,
            playing: true,
            loop_video,
            muted: false,
            stopped,
        };

        // Random forward & backward seeks across the 240-frame stream stay pixel-exact and never
        // exceed MAX_WINDOWED_RING_FRAMES.
        for &target_idx in &[0usize, 5, 199, 73, 239, 12, 150, 0] {
            let p = make_placement(
                (target_idx as u32) * 50,
                0,
                None,
                false,
                VideoSyncMode::Stream,
                false,
            );
            let (resolved_idx, frame) = long_stream.frame_at_placement(&p).unwrap();
            assert_eq!(resolved_idx, target_idx);
            assert_eq!(
                &frame.image.rgba[..4],
                &[target_idx as u8, ((target_idx * 3) & 0xFF) as u8, 42, 255]
            );
            assert!(decoder.resident_ring_frames() <= MAX_WINDOWED_RING_FRAMES);
            assert!(decoder.resident_ring_bytes() <= MAX_WINDOWED_RING_STREAM_BYTES);
        }

        // Trim clamps (in_point_ms / out_point_ms), Loop, and Stop semantics match exactly.
        let trimmed = make_placement(5_000, 500, Some(1_500), false, VideoSyncMode::Stream, false);
        let (clamped_idx, _) = long_stream.frame_at_placement(&trimmed).unwrap();
        assert_eq!(
            clamped_idx, 29,
            "non-looped trim must hold final frame before out_point_ms"
        );

        let unstopped = make_placement(0, 0, None, false, VideoSyncMode::Stop, false);
        assert!(long_stream.frame_at_placement(&unstopped).is_none());
        let stopped_hold = make_placement(250, 0, None, false, VideoSyncMode::Stop, true);
        assert_eq!(long_stream.frame_at_placement(&stopped_hold).unwrap().0, 5);

        // 3. Long-form multi-GOP H.264 external MP4 (when ffmpeg + ffprobe are available):
        // verifies real codec seeking across GOP boundaries without loading all frames into RAM.
        if let Some(ffmpeg) = detect_ffmpeg()
            && detect_ffprobe(&ffmpeg).is_some()
        {
            let dir = env::temp_dir().join(format!(
                "neoflash-t229-windowed-h264-{}-{}",
                std::process::id(),
                VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let mp4_path = dir.join("long_gop.mp4");
            // 210 frames at 30 fps (7s > MAX_EAGER_VIDEO_FRAMES = 180) with GOP = 30.
            let status = Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=s=64x64:r=30:d=7",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                    "-g",
                    "30",
                ])
                .arg(&mp4_path)
                .status()
                .unwrap();
            assert!(status.success());

            let mut project = Project::blank();
            project.resources.push(
                ResourceUnit::from_bytes("long_vid", "long_gop.mp4", "video/mp4", &[]).unwrap(),
            );
            let spec = VideoSourceSpec {
                mode: VideoDeploymentMode::External,
                url_or_path: "long_gop.mp4".into(),
                mime_type: "video/mp4".into(),
            };
            let ext_stream =
                resolve_and_decode_project_video(&project, "long_vid", Some(&spec), Some(&dir))
                    .unwrap();
            assert!(ext_stream.frames.is_empty());
            let ext_dec = ext_stream.on_demand.as_ref().unwrap();
            assert_eq!(ext_dec.logical_frame_count(), 210);

            // Reference full raw decode for exact pixel verification across GOPs:
            let full_raw = Command::new(&ffmpeg)
                .args(["-hide_banner", "-loglevel", "error", "-i"])
                .arg(&mp4_path)
                .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
                .output()
                .unwrap();
            assert!(full_raw.status.success());
            let fbytes = 64 * 64 * 4;
            assert_eq!(full_raw.stdout.len(), 210 * fbytes);

            for &seek_frame in &[0usize, 1, 29, 30, 31, 107, 179, 209, 44] {
                let target_ms = ((seek_frame as f64 * 1000.0) / 30.0).round() as u32;
                let p = make_placement(target_ms, 0, None, false, VideoSyncMode::Stream, false);
                let (got_idx, got_frame) = ext_stream.frame_at_placement(&p).unwrap();
                assert_eq!(got_idx, seek_frame);
                let ref_slice = &full_raw.stdout[seek_frame * fbytes..(seek_frame + 1) * fbytes];
                assert_eq!(
                    got_frame.image.rgba.as_ref(),
                    ref_slice,
                    "windowed H.264 seek mismatch at frame {seek_frame}"
                );
                assert!(ext_dec.resident_ring_frames() <= MAX_WINDOWED_RING_FRAMES);
            }

            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn t274_progressive_video_decodes_embedded_bytes_staged_files_and_synthesized_streams_in_offline_export()
     {
        use neoflash_core::{ResourceUnit, TimelineVideoSpan};

        let mp4_bytes = build_four_color_mp4_fixture();
        let dir = env::temp_dir().join(format!(
            "neoflash-t274-progressive-export-{}-{}",
            std::process::id(),
            VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("media")).unwrap();
        fs::write(dir.join("media/prog_clip.mp4"), &mp4_bytes).unwrap();

        let make_prog_project = |res: ResourceUnit, url: &str| -> Project {
            let mut p = Project::blank();
            p.width = 64;
            p.height = 64;
            p.fps = 10;
            p.background = [10, 10, 10, 255];
            let res_id = res.id.clone();
            p.resources.push(res);
            let root_id = p.root.clone();
            let root = p.symbol_mut(&root_id).unwrap();
            root.timeline.frame_count = 4;
            root.timeline.layers[0].keyframes[0].video =
                Some(TimelineVideoSpan::progressive(res_id, url));
            root.timeline.layers[0].keyframes[0].elements = vec![Element {
                id: "v_prog".into(),
                name: String::new(),
                transform: Transform {
                    x: 8.0,
                    y: 8.0,
                    scale_x: 2.0,
                    scale_y: 2.0,
                    ..Transform::default()
                },
                kind: ElementKind::Video {
                    width: 16.0,
                    height: 16.0,
                },
                ..Element::default()
            }];
            p.upgrade_format_for_features();
            p
        };

        let sample_px = |rgba: &[u8], x: u32, y: u32| -> [u8; 4] {
            let idx = ((y * 64 + x) * 4) as usize;
            [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
        };

        // 1. Progressive span with embedded/cached resource bytes in Project::resources:
        let mut emb_prog_proj = make_prog_project(
            ResourceUnit::from_bytes(
                "vid_prog_emb",
                "media/prog_clip.mp4",
                "video/mp4",
                &mp4_bytes,
            )
            .unwrap(),
            "https://cdn.example.com/media/prog_clip.mp4",
        );
        // Also keep an Embedded library symbol referencing vid_prog_emb so project validation allows
        // non-empty data_base64 on the shared resource while the stage span uses Progressive mode.
        emb_prog_proj.symbols.push(Symbol {
            id: "lib_emb_holder".into(),
            name: "LibHolder".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "l".into(),
                    name: "L".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        video: Some(TimelineVideoSpan::embedded("vid_prog_emb")),
                        ..Keyframe::default()
                    }],
                    properties: Default::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
            linkage_id: None,
            class_name: None,
        });
        let f0 = render_project_frame_rgba(&emb_prog_proj, None, 0, 64, 64, false, false).unwrap();
        let f1 = render_project_frame_rgba(&emb_prog_proj, None, 1, 64, 64, false, false).unwrap();
        let f2 = render_project_frame_rgba(&emb_prog_proj, None, 2, 64, 64, false, false).unwrap();
        let f3 = render_project_frame_rgba(&emb_prog_proj, None, 3, 64, 64, false, false).unwrap();
        assert_eq!(sample_px(&f0, 16, 16), [255, 0, 0, 255]);
        assert_eq!(sample_px(&f1, 16, 16), [0, 255, 0, 255]);
        assert_eq!(sample_px(&f2, 16, 16), [0, 0, 255, 255]);
        assert_eq!(sample_px(&f3, 16, 16), [255, 255, 0, 255]);

        // 2. Progressive span with empty resource bytes resolved from local/staged file path under base_dir:
        let local_prog_proj = make_prog_project(
            ResourceUnit::from_bytes("vid_prog_local", "media/prog_clip.mp4", "video/mp4", &[])
                .unwrap(),
            "https://cdn.example.com/media/prog_clip.mp4",
        );
        let seq_out = dir.join("prog_seq.png");
        let report = export_project_video(
            &local_prog_proj,
            &seq_out,
            &VideoOptions {
                format: VideoFormat::PngSequence,
                width: 64,
                height: 64,
                from_frame: 1,
                to_frame: Some(4),
                base_dir: Some(dir.clone()),
                ..VideoOptions::default()
            },
        )
        .expect("progressive video with staged local file exports PNG sequence");
        assert_eq!(report.frame_count, 4);
        let p1 = image::open(dir.join("prog_seq_0001.png"))
            .unwrap()
            .to_rgba8();
        let p2 = image::open(dir.join("prog_seq_0002.png"))
            .unwrap()
            .to_rgba8();
        let p3 = image::open(dir.join("prog_seq_0003.png"))
            .unwrap()
            .to_rgba8();
        let p4 = image::open(dir.join("prog_seq_0004.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(p1.get_pixel(16, 16).0, [255, 0, 0, 255]);
        assert_eq!(p2.get_pixel(16, 16).0, [0, 255, 0, 255]);
        assert_eq!(p3.get_pixel(16, 16).0, [0, 0, 255, 255]);
        assert_eq!(p4.get_pixel(16, 16).0, [255, 255, 0, 255]);

        // 3. Progressive span with known metadata in headless mode (no local file on disk):
        // synthesizes a deterministic multi-frame RGBA stream across timeline frames.
        let mut synth_prog_proj = make_prog_project(
            ResourceUnit::from_bytes("vid_prog_synth", "streams/synth_only.mp4", "video/mp4", &[])
                .unwrap(),
            "https://cdn.example.com/streams/synth_only.mp4",
        );
        {
            let root_id = synth_prog_proj.root.clone();
            let root = synth_prog_proj.symbol_mut(&root_id).unwrap();
            root.timeline.layers[0].keyframes[0]
                .video
                .as_mut()
                .unwrap()
                .end_offset_ms = Some(4000);
        }
        synth_prog_proj.upgrade_format_for_features();
        let sf0 =
            render_project_frame_rgba(&synth_prog_proj, None, 0, 64, 64, false, false).unwrap();
        let sf1 =
            render_project_frame_rgba(&synth_prog_proj, None, 1, 64, 64, false, false).unwrap();
        let sf2 =
            render_project_frame_rgba(&synth_prog_proj, None, 2, 64, 64, false, false).unwrap();
        assert_ne!(
            sample_px(&sf0, 16, 16),
            [10, 10, 10, 255],
            "synthesized progressive frame must draw video pixels over stage background"
        );
        assert_ne!(
            sample_px(&sf0, 16, 16),
            sample_px(&sf1, 16, 16),
            "synthesized progressive stream must advance across timeline frames"
        );
        assert_ne!(sample_px(&sf1, 16, 16), sample_px(&sf2, 16, 16));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn t283_bundled_ffmpeg_and_ffprobe_work_without_path_for_h264_h265_webm_and_audio() {
        use crate::audio::{
            AudioBitRate, AudioChannels, AudioExportOptions, AudioFormat, AudioSampleRate,
            encode_aac, encode_mp3, export_project_audio,
        };

        let Some(bundled_ffmpeg) = detect_bundled_ffmpeg() else {
            panic!(
                "Expected staged bundled ffmpeg at target/bundled-media/bin/ffmpeg (run `python3 scripts/stage-bundled-media.py`)."
            );
        };
        let Some(bundled_ffprobe) = detect_bundled_ffprobe() else {
            panic!("Expected staged bundled ffprobe at target/bundled-media/bin/ffprobe.");
        };

        // 1. Bundled helpers are discovered before PATH and marked as bundled media binaries:
        assert!(is_bundled_media_binary(&bundled_ffmpeg));
        assert!(is_bundled_media_binary(&bundled_ffprobe));
        if env::var("NEOFLASH_FFMPEG")
            .ok()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            assert_eq!(detect_ffmpeg().as_ref(), Some(&bundled_ffmpeg));
        }
        if env::var("NEOFLASH_FFPROBE")
            .ok()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            assert_eq!(
                detect_ffprobe(&bundled_ffmpeg).and_then(|p| p.canonicalize().ok()),
                Some(bundled_ffprobe.clone())
            );
        }

        // 2. On macOS, verify otool -L has zero /opt/homebrew, /usr/local, or @rpath references:
        if cfg!(target_os = "macos") {
            let lib_dir = bundled_ffmpeg
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("lib"))
                .expect("bundled lib dir");
            assert!(
                lib_dir.is_dir(),
                "expected bundled dylib directory at {lib_dir:?}"
            );

            let mut binaries_to_inspect = vec![bundled_ffmpeg.clone(), bundled_ffprobe.clone()];
            for entry in fs::read_dir(&lib_dir).unwrap().flatten() {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("dylib") {
                    binaries_to_inspect.push(entry.path());
                }
            }
            for bin_path in &binaries_to_inspect {
                let otool_out = Command::new("/usr/bin/otool")
                    .arg("-L")
                    .arg(bin_path)
                    .output()
                    .expect("run /usr/bin/otool -L");
                assert!(otool_out.status.success());
                let text = String::from_utf8_lossy(&otool_out.stdout);
                for line in text.lines().skip(1) {
                    let dep = line.trim();
                    assert!(
                        !dep.starts_with("/opt/homebrew")
                            && !dep.starts_with("/usr/local")
                            && !dep.starts_with("@rpath"),
                        "Bundled binary {bin_path:?} still references external/unrelocated path: {dep}"
                    );
                }
            }

            // Provenance & SBOM files exist alongside the bundle:
            let bundle_root = bundled_ffmpeg.parent().unwrap().parent().unwrap();
            assert!(bundle_root.join("provenance/media-sbom.json").is_file());
            assert!(
                bundle_root
                    .join("provenance/ffmpeg-buildconf.txt")
                    .is_file()
            );
            assert!(bundle_root.join("provenance/ffmpeg-license.txt").is_file());
            assert!(bundle_root.join("provenance/SHA256SUMS.txt").is_file());
            assert!(
                bundle_root
                    .join("licenses/README-CORRESPONDING-SOURCE.txt")
                    .is_file()
            );
        }

        // 3. Direct invocation with cleared environment (`env_clear()` + `PATH=""`):
        let ffmpeg_ver = Command::new(&bundled_ffmpeg)
            .env_clear()
            .env("PATH", "")
            .arg("-version")
            .output()
            .expect("launch bundled ffmpeg with empty env");
        assert!(
            ffmpeg_ver.status.success(),
            "bundled ffmpeg failed with empty env: {}",
            String::from_utf8_lossy(&ffmpeg_ver.stderr)
        );
        let ffprobe_ver = Command::new(&bundled_ffprobe)
            .env_clear()
            .env("PATH", "")
            .arg("-version")
            .output()
            .expect("launch bundled ffprobe with empty env");
        assert!(
            ffprobe_ver.status.success(),
            "bundled ffprobe failed with empty env: {}",
            String::from_utf8_lossy(&ffprobe_ver.stderr)
        );

        // 4. Capability detection and hardware/software encoder selection:
        let encoders = probe_ffmpeg_encoders(&bundled_ffmpeg);
        assert!(encoders.contains("libx264"));
        assert!(encoders.contains("libx265"));
        assert!(encoders.contains("libvpx-vp9"));
        assert!(encoders.contains("libopus"));
        assert!(encoders.contains("libmp3lame"));
        assert_eq!(
            select_video_encoder(&bundled_ffmpeg, VideoCodec::H264, false),
            ("libx264", false)
        );
        assert_eq!(
            select_video_encoder(&bundled_ffmpeg, VideoCodec::H265, false),
            ("libx265", false)
        );
        if cfg!(target_os = "macos") && encoders.contains("h264_videotoolbox") {
            assert_eq!(
                select_video_encoder(&bundled_ffmpeg, VideoCodec::H264, true),
                ("h264_videotoolbox", true)
            );
        }
        if cfg!(target_os = "macos") && encoders.contains("hevc_videotoolbox") {
            assert_eq!(
                select_video_encoder(&bundled_ffmpeg, VideoCodec::H265, true),
                ("hevc_videotoolbox", true)
            );
        }

        // 5. End-to-end export + decode round-trip for MP4 (H.264), MP4 (H.265), WebM (VP9+Opus), and MP3/AAC audio:
        let dir = env::temp_dir().join(format!(
            "neoflash-t283-bundled-media-{}-{}",
            std::process::id(),
            VIDEO_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut project = Project::demo();
        let mut samples = Vec::with_capacity(4410);
        for i in 0..4410 {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin()) * 0.4;
            samples.push([v, v]);
        }
        let wav_bytes = encode_wav_pcm16(&samples, 44_100);
        project.resources.push(
            ResourceUnit::from_bytes("snd_t283", "audio/tone.wav", "audio/wav", &wav_bytes)
                .unwrap(),
        );
        let root_id = project.root.clone();
        let mut snd = FrameSound::new("snd_t283");
        snd.sync = SoundSync::Event;
        project.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].sound = Some(snd);
        project.upgrade_format_for_features();

        // 5a. MP4 (H.264 + AAC) export & decode:
        let h264_path = dir.join("out_h264.mp4");
        let h264_report = export_project_video(
            &project,
            &h264_path,
            &VideoOptions {
                format: VideoFormat::Mp4,
                codec: VideoCodec::H264,
                width: 128,
                height: 96,
                from_frame: 1,
                to_frame: Some(3),
                include_audio: true,
                ..VideoOptions::default()
            },
        )
        .expect("export H.264 MP4 with bundled ffmpeg");
        assert!(h264_report.audio_included);
        let h264_decoded =
            decode_video_stream_bytes("out_h264.mp4", "video/mp4", &fs::read(&h264_path).unwrap())
                .expect("decode H.264 MP4 with bundled ffmpeg/ffprobe");
        assert_eq!((h264_decoded.width, h264_decoded.height), (128, 96));
        assert!(h264_decoded.logical_frame_count() >= 3);

        // 5b. MP4 (H.265 / HEVC) export & probe:
        let h265_path = dir.join("out_h265.mp4");
        let h265_report = export_project_video(
            &project,
            &h265_path,
            &VideoOptions {
                format: VideoFormat::Mp4,
                codec: VideoCodec::H265,
                width: 128,
                height: 96,
                from_frame: 1,
                to_frame: Some(3),
                include_audio: false,
                ..VideoOptions::default()
            },
        )
        .expect("export H.265 MP4 with bundled ffmpeg");
        assert!(h265_report.bytes > 128);
        let h265_meta = probe_stream_metadata(&bundled_ffmpeg, &h265_path)
            .expect("probe H.265 MP4 with bundled ffprobe")
            .expect("stream metadata present");
        assert_eq!((h265_meta.width, h265_meta.height), (128, 96));

        // 5c. WebM (VP9 + Opus) export & decode:
        let webm_path = dir.join("out_vp9.webm");
        let webm_report = export_project_video(
            &project,
            &webm_path,
            &VideoOptions {
                format: VideoFormat::WebM,
                codec: VideoCodec::Vp9,
                width: 128,
                height: 96,
                from_frame: 1,
                to_frame: Some(3),
                include_audio: true,
                audio_bitrate: VideoAudioBitrate::Opus160,
                ..VideoOptions::default()
            },
        )
        .expect("export VP9 WebM with bundled ffmpeg");
        assert!(webm_report.audio_included);
        let webm_decoded =
            decode_video_stream_bytes("out_vp9.webm", "video/webm", &fs::read(&webm_path).unwrap())
                .expect("decode VP9 WebM with bundled ffmpeg/ffprobe");
        assert_eq!((webm_decoded.width, webm_decoded.height), (128, 96));
        assert!(webm_decoded.logical_frame_count() >= 3);

        // 5d. MP3 & AAC audio export via bundled ffmpeg:
        let mp3_bytes = encode_mp3(
            &samples,
            44_100,
            AudioChannels::Stereo,
            192,
            neoflash_core::SoundExportQuality::Best,
        )
        .expect("encode MP3 via bundled ffmpeg");
        assert!(mp3_bytes.len() > 128);
        let aac_bytes = encode_aac(&samples, 44_100, AudioChannels::Stereo, 192)
            .expect("encode AAC via bundled ffmpeg");
        assert!(aac_bytes.len() > 128);

        let mp3_out_path = dir.join("mix.mp3");
        let mp3_report = export_project_audio(
            &project,
            &mp3_out_path,
            &AudioExportOptions {
                format: AudioFormat::Mp3,
                sample_rate: AudioSampleRate::Hz44100,
                bit_rate: AudioBitRate::Kbps192,
                channels: AudioChannels::Stereo,
                ..AudioExportOptions::default()
            },
        )
        .expect("export project MP3 audio");
        assert!(mp3_report.bytes > 128);
        assert!(mp3_out_path.is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}
