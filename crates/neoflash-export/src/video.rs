//! Video, GIF, and PNG sequence export (`DL-05` / `E-22` / `E-23`).
//!
//! Renders authored or runtime-captured timeline frames to RGBA buffers using a deterministic CPU
//! rasterizer (`tiny-skia` + `ab_glyph` + `image` + `MaskCoverage`), mixes timeline audio tracks
//! via `symphonia`, and encodes to MP4, WebM, MOV (via `ffmpeg`), animated GIF (`gif` + `color_quant`),
//! or numbered PNG sequences (`image`).

use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock, atomic::AtomicU64},
    time::Duration,
};

use crate::ExportError;
use neoflash_core::{
    ElementKind, InstancePlayback, Project, SymbolKind, flatten_scenes_with_ranges, scene_ranges,
};
use serde::{Deserialize, Serialize};

mod decoder;
mod encode;
mod ffprobe_ffmpeg;
mod progressive;
mod render;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_progressive;

pub(crate) use self::{
    decoder::has_nfvf_box_header,
    render::{SequentialVideoFrameSession, render_local_brush},
};
pub use self::{
    decoder::{
        DecodedVideoFrame, DecodedVideoStream, OnDemandVideoDecoder,
        encode_deterministic_mp4_rgba_frames,
    },
    encode::{export_project_video, export_project_video_with_progress},
    ffprobe_ffmpeg::{decode_video_stream_bytes, detect_ffprobe},
    progressive::{
        discard_staged_progressive_video_bytes, fetch_progressive_video_bytes,
        fetch_progressive_video_bytes_allowing_private_for_tests,
        lookup_staged_progressive_video_bytes, resolve_and_decode_project_video,
        resolve_progressive_local_path, stage_downloaded_progressive_video_bytes,
        stage_progressive_video_bytes,
    },
    render::{
        render_authored_scene_rgba, render_authored_scene_with_video_rgba,
        render_project_frame_rgba, render_project_frame_rgba_with_base_dir,
    },
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
    if let Ok(exe_path) = env::current_exe() {
        let canonical = exe_path.canonicalize().ok();
        for candidate_exe in std::iter::once(&exe_path).chain(canonical.iter()) {
            let Some(exe_dir) = candidate_exe.parent() else {
                continue;
            };
            // 1. macOS .app bundle (`Contents/MacOS/<exe>` -> `Contents/Resources/media/bin` or `Contents/Helpers/media/bin`)
            search_dirs.push(exe_dir.join("../Resources/media/bin"));
            search_dirs.push(exe_dir.join("../Helpers/media/bin"));
            // 2. Windows / portable bundle (`<exe_dir>/media/bin`)
            search_dirs.push(exe_dir.join("media/bin"));
            // 3. Ancestor target/bundled-media/bin (for CLI/dev/test binaries under target/)
            for ancestor in exe_dir.ancestors().take(8) {
                search_dirs.push(ancestor.join("bundled-media/bin"));
                search_dirs.push(ancestor.join("target/bundled-media/bin"));
            }
        }
    }
    #[cfg(debug_assertions)]
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

/// Apply the execution environment required by a resolved NeoFlash media helper.
///
/// Bundled helpers must remain self-contained: clearing `PATH` here prevents an apparently
/// portable package from succeeding only because another `ffmpeg` installation is available on
/// the host. Callers outside the video exporter (for example the editor's Extract audio command)
/// should use this after [`detect_ffmpeg`] instead of recreating the bundle rules.
pub fn configure_media_subprocess_command(cmd: &mut Command, binary: &Path) {
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
const MAX_PROGRESSIVE_VIDEO_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROGRESSIVE_VIDEO_REDIRECTS: usize = 5;
const MAX_DOWNLOADED_PROGRESSIVE_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_DOWNLOADED_PROGRESSIVE_CACHE_ENTRIES: usize = 8;
const DOWNLOADED_PROGRESSIVE_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_FFMPEG_DIAGNOSTIC_BYTES: usize = 64 * 1024;
/// Short clips at or below this uncompressed RGBA size and frame count take the eager in-memory
/// fast path; longer streams switch automatically to `OnDemandVideoDecoder` (`T-229`).
const MAX_EAGER_VIDEO_STREAM_BYTES: usize = 32 * 1024 * 1024;
const MAX_EAGER_VIDEO_FRAMES: usize = 180;
/// Pinned CPU decoded-frame ring budget per long-form video stream (`T-229`).
const MAX_WINDOWED_RING_STREAM_BYTES: usize = 64 * 1024 * 1024;
const MAX_WINDOWED_RING_FRAMES: usize = 32;
