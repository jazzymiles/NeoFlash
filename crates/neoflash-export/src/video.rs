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
    ElementKind, FillRule, GradientKind, GradientPaint, InstancePlayback, MaskCoverage,
    PathCommand, Project, SoundCommand, SymbolKind, TextAlign, TextFormat, evaluate_scene,
    flatten_scenes_with_ranges, oval_path_commands, rectangle_path_commands, scene_ranges,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs,
    io::{BufWriter, Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, OnceLock},
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

/// Locate an `ffmpeg` binary on the host (`NEOFLASH_FFMPEG`, `PATH`, or standard macOS/Unix paths).
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
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("ffmpeg");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for fallback in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ] {
        let p = PathBuf::from(fallback);
        if p.is_file() {
            return Some(p);
        }
    }
    None
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
    let opts = VideoOptions {
        symbol: symbol.map(str::to_owned),
        width,
        height,
        transparent_bg,
        runtime_capture,
        ..VideoOptions::default()
    };
    let prepared = prepare_timeline(project, &opts)?;
    let target_frame = frame % prepared.authored_span.max(1);
    let scene = if runtime_capture {
        let mut playback = AuthoredPlayback::new(&prepared.eval_project, &prepared.root_id, 0);
        for _ in 0..target_frame {
            playback.tick(&prepared.eval_project);
        }
        playback.scene(&prepared.eval_project, false, true)
    } else {
        evaluate_scene(
            &prepared.eval_project,
            &prepared.root_id,
            target_frame,
            false,
            true,
        )
    };
    let mut bitmap_cache = HashMap::new();
    Ok(render_authored_scene_rgba(
        &prepared.eval_project,
        &scene,
        width.max(1),
        height.max(1),
        transparent_bg,
        &mut bitmap_cache,
    ))
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
pub fn render_authored_scene_rgba(
    project: &Project,
    scene: &AuthoredScene,
    width: u32,
    height: u32,
    transparent_bg: bool,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
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

fn draw_authored_item(
    pixmap: &mut Pixmap,
    project: &Project,
    draw: &AuthoredDraw,
    stage_scale: SkiaTransform,
    mask: Option<&SkiaMask>,
    bitmap_cache: &mut HashMap<String, Option<Arc<DecodedImage>>>,
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
            if *width > 0.0 && *height > 0.0 {
                let commands = rectangle_path_commands(*width, *height, Default::default());
                draw_vector_commands(
                    pixmap,
                    &commands,
                    FillRule::NonZero,
                    ([28, 32, 38, 255], None),
                    ([90, 100, 115, 255], None, 1.0),
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

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let mut playback = AuthoredPlayback::new(
        &prepared.eval_project,
        &prepared.root_id,
        prepared.start_frame,
    );

    let mut bitmap_cache = HashMap::new();
    let mut rendered_frames: Vec<Vec<u8>> = Vec::with_capacity(prepared.frame_count as usize);
    let mut sound_events: Vec<(u32, Vec<SoundCommand>)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for idx in 0..prepared.frame_count {
        if idx > 0 {
            if idx % prepared.authored_span == 0 && prepared.start_frame > 0 {
                // Re-seek to start_frame when looping a sub-range under StopMode::AfterSeconds.
                playback = AuthoredPlayback::new(
                    &prepared.eval_project,
                    &prepared.root_id,
                    prepared.start_frame,
                );
            } else {
                playback.tick(&prepared.eval_project);
            }
        }
        let cmds = playback.drain_sound_commands();
        if !cmds.is_empty() {
            sound_events.push((idx, cmds));
        }

        let scene = if options.runtime_capture {
            playback.scene(&prepared.eval_project, false, true)
        } else {
            let f = prepared.start_frame + (idx % prepared.authored_span.max(1));
            evaluate_scene(&prepared.eval_project, &prepared.root_id, f, false, true)
        };

        let rgba = render_authored_scene_rgba(
            &prepared.eval_project,
            &scene,
            width,
            height,
            transparent_bg,
            &mut bitmap_cache,
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
fn encode_with_ffmpeg(
    output_path: &Path,
    options: &VideoOptions,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    transparent_bg: bool,
    wav_audio: Option<&[u8]>,
    warnings: Vec<String>,
) -> Result<VideoExportReport, ExportError> {
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        ExportError::Options(
            "FFmpeg was not found. Install ffmpeg (e.g. `brew install ffmpeg`), set NEOFLASH_FFMPEG, or export as GIF / PNG seq."
                .into(),
        )
    })?;

    let mut temp_wav_path: Option<PathBuf> = None;
    if let Some(wav) = wav_audio {
        let wav_file = std::env::temp_dir().join(format!(
            "neoflash-video-audio-{}-{}.wav",
            std::process::id(),
            frames.len()
        ));
        fs::write(&wav_file, wav)?;
        temp_wav_path = Some(wav_file);
    }

    let mut cmd = Command::new(&ffmpeg);
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

    if let Some(wav_file) = &temp_wav_path {
        cmd.arg("-i").arg(wav_file);
    }

    let codec = options.resolved_codec();
    let q = i32::from(options.quality.clamp(1, 100));
    let crf_h264 = (51 - (q * 35) / 100).clamp(14, 45);
    let crf_vp9 = (63 - (q * 48) / 100).clamp(15, 55);

    match (options.format, codec) {
        (VideoFormat::Mp4, VideoCodec::H265) | (VideoFormat::Mov, VideoCodec::H265) => {
            cmd.arg("-c:v")
                .arg("libx265")
                .arg("-crf")
                .arg(crf_h264.to_string())
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-tag:v")
                .arg("hvc1");
        }
        (VideoFormat::Mp4, _) | (VideoFormat::Mov, VideoCodec::H264) => {
            cmd.arg("-c:v")
                .arg("libx264")
                .arg("-preset")
                .arg("medium")
                .arg("-crf")
                .arg(crf_h264.to_string())
                .arg("-pix_fmt")
                .arg("yuv420p");
            if options.format == VideoFormat::Mp4 {
                cmd.arg("-movflags").arg("+faststart");
            }
        }
        (VideoFormat::Mov, VideoCodec::ProRes) => {
            cmd.arg("-c:v")
                .arg("prores_ks")
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
                .arg("libsvtav1")
                .arg("-crf")
                .arg(crf_vp9.to_string())
                .arg("-pix_fmt")
                .arg("yuv420p");
        }
        (VideoFormat::WebM, _) => {
            cmd.arg("-c:v")
                .arg("libvpx-vp9")
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

    if temp_wav_path.is_some() {
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

    if let Some(wav_file) = temp_wav_path {
        let _ = fs::remove_file(wav_file);
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ExportError::Options(format!(
            "ffmpeg encoding failed: {}",
            stderr.trim()
        )));
    }
    if let Some(e) = write_err {
        return Err(ExportError::Io(e));
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
}
