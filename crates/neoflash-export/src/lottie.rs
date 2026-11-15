//! Bodymovin / Lottie (`v: "5.7.14"`) JSON animation exporter.
//!
//! Converts NeoFlash authoring timelines, classic/motion/shape tweens, ease curves, motion guides,
//! nested symbols (`Graphic`, `MovieClip`, `Button`), timeline-synchronized layer crops (`T-038`),
//! instance `First frame` offsets, layer/element blend modes (`T-036`), masks, gradients, bitmaps,
//! and text into a self-contained Lottie JSON document.
//!
//! Features that Lottie players cannot execute (such as ActionScript / JavaScript code, timeline
//! audio, video viewports, or Flash-only filter/blend modes) produce actionable diagnostics in
//! [`LottieExportReport::warnings`] instead of failing or silently dropping artwork.

use crate::ExportError;
use base64::Engine as _;
use image::{ExtendedColorType, ImageEncoder as _};
use neoflash_core::{
    AppearancePaintEntry, BitmapCompression, BlendMode, ClassicRotateDirection, Color,
    ColorTransform, Easing, Element, ElementKind, FillRule, GradientKind, GradientPaint,
    GradientSpread, InstancePlayback, Keyframe, Layer, LayerRole, MaskMode, MotionPathSource,
    PathCommand, PatternBrushKind, Project, Symbol, SymbolKind, TextAlign, TextFieldType, Timeline,
    Transform, TweenKind, TweenProperty, camera_layer_matrix, flatten_scenes_with_ranges,
    layer_effective_blend_mode, layer_effectively_synced, layer_effectively_visible, motion_span,
    placement_start, sample_layer_in_at_fps, sample_timeline_camera,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Cursor, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

/// Bodymovin schema version targeted by the exporter.
pub const LOTTIE_VERSION: &str = "5.7.14";

/// Output container format (`R-043`: `JSON | dotLottie (.lottie)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieFormat {
    #[default]
    Json,
    DotLottie,
}

impl LottieFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::DotLottie => "dotLottie (.lottie)",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::DotLottie => "lottie",
        }
    }
}

/// Nested MovieClip handling (`R-043`: `Movie clips: Nested | First frame`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieMovieClipMode {
    #[default]
    Nested,
    FirstFrame,
}

impl LottieMovieClipMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nested => "Nested",
            Self::FirstFrame => "First frame",
        }
    }
}

/// Text handling (`R-043`: `Text: Shapes | Text layers`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieTextMode {
    Shapes,
    #[default]
    TextLayers,
}

impl LottieTextMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shapes => "Shapes",
            Self::TextLayers => "Text layers",
        }
    }
}

/// Bitmap image asset packaging (`R-043`: `Images: Embed | Folder`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieImageMode {
    #[default]
    Embed,
    Folder,
}

impl LottieImageMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Embed => "Embed",
            Self::Folder => "Folder",
        }
    }
}

/// Bitmap image format (`R-043`: `Image format: Original | PNG | JPEG`, plus programmatic/CLI `WebP`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieImageFormat {
    #[default]
    Original,
    Png,
    Jpeg,
    Webp,
}

impl LottieImageFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
        }
    }
}

/// Target Lottie player profile (`R-043`: `Player: lottie-web | Mobile | Skottie`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieTargetPlayer {
    #[default]
    LottieWeb,
    Mobile,
    Skottie,
}

impl LottieTargetPlayer {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LottieWeb => "lottie-web",
            Self::Mobile => "Mobile",
            Self::Skottie => "Skottie",
        }
    }
}

/// Category of a structured compatibility note (`R-043`: `Missing / Approximated / Not exported`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LottieNoteKind {
    Missing,
    Approximated,
    NotExported,
}

impl LottieNoteKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Missing => "Missing",
            Self::Approximated => "Approximated",
            Self::NotExported => "Not exported",
        }
    }
}

/// Structured compatibility note emitted during Lottie export (`R-043`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LottieNote {
    pub kind: LottieNoteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub location: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
}

/// Progress snapshot passed to [`export_lottie_with_progress`] (`R-043`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LottieProgress {
    pub completed_steps: usize,
    pub total_steps: usize,
    pub current_stage: String,
}

/// Options controlling Lottie JSON / dotLottie generation (`R-043`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LottieOptions {
    /// Output container format (`JSON` or `dotLottie (.lottie)`).
    pub format: LottieFormat,
    /// Export a specific symbol by ID or name instead of the flattened root scene timeline.
    pub symbol: Option<String>,
    /// Export a specific scene by ID or name instead of all scenes.
    pub scene: Option<String>,
    /// Optional 1-based inclusive frame range `(from_frame, to_frame)`.
    pub frame_range: Option<(u32, u32)>,
    /// Optional output width override (before `scale`).
    pub width: Option<u32>,
    /// Optional output height override (before `scale`).
    pub height: Option<u32>,
    /// Uniform output scale multiplier (`0.5`, `1.0`, `2.0`, etc.).
    pub scale: f64,
    /// Optional frame-rate override (`None` uses `project.fps`).
    pub fps: Option<u32>,
    /// Emit a bottom solid layer (`ty: 1`) matching `project.background` when its alpha is > 0.
    pub include_background: bool,
    /// Include layers whose authoring visibility is turned off.
    pub include_hidden_layers: bool,
    /// Emit frame labels and scene boundaries as Lottie `markers`.
    pub include_markers: bool,
    /// Export nested MovieClips as animated precomps (`Nested`) or frozen on frame 1 (`FirstFrame`).
    pub movie_clip_mode: LottieMovieClipMode,
    /// Export text as vector glyph outlines (`Shapes`) or Lottie `"ty": 5` text layers (`TextLayers`).
    pub text_mode: LottieTextMode,
    /// Embed bitmaps as base64 Data-URIs (`Embed`) or write external files into `images/` (`Folder`).
    pub image_mode: LottieImageMode,
    /// Keep original bitmap encoding or transcode (`Original`, `Png`, `Jpeg`).
    pub image_format: LottieImageFormat,
    /// Format the JSON output with indentation (`!pretty` = `Minify` switch in UI).
    pub pretty: bool,
    /// Coordinate rounding precision in decimal places (`0..=4`, default `4`).
    pub precision: u8,
    /// Target Lottie player profile for compatibility notes (`lottie-web`, `Mobile`, `Skottie`).
    pub player: LottieTargetPlayer,
    /// Also write an interactive `<stem>.preview.html` file next to the exported file.
    pub html_preview: bool,
}

impl Default for LottieOptions {
    fn default() -> Self {
        Self {
            format: LottieFormat::Json,
            symbol: None,
            scene: None,
            frame_range: None,
            width: None,
            height: None,
            scale: 1.0,
            fps: None,
            include_background: true,
            include_hidden_layers: false,
            include_markers: true,
            movie_clip_mode: LottieMovieClipMode::Nested,
            text_mode: LottieTextMode::TextLayers,
            image_mode: LottieImageMode::Embed,
            image_format: LottieImageFormat::Original,
            pretty: false,
            precision: 4,
            player: LottieTargetPlayer::LottieWeb,
            html_preview: false,
        }
    }
}

/// Summary and structured diagnostics produced by a Lottie export (`R-043`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LottieExportReport {
    pub path: Option<PathBuf>,
    pub html_preview_path: Option<PathBuf>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub in_point: u32,
    pub frame_count: u32,
    pub layer_count: usize,
    pub asset_count: usize,
    pub marker_count: usize,
    pub bytes: usize,
    pub exported_count: usize,
    pub approximated_count: usize,
    pub not_exported_count: usize,
    pub notes: Vec<LottieNote>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
struct ExternalImageFile {
    relative_path: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetectedRaster {
    Png,
    Jpeg,
    Webp,
    Gif,
    Other,
}

fn detect_raster_magic(bytes: &[u8]) -> DetectedRaster {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        DetectedRaster::Png
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        DetectedRaster::Jpeg
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        DetectedRaster::Webp
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        DetectedRaster::Gif
    } else {
        DetectedRaster::Other
    }
}

fn encode_png_rgba(img: &neoflash_core::DecodedImage) -> Result<Vec<u8>, ExportError> {
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(&img.rgba, img.width, img.height, ExtendedColorType::Rgba8)
        .map_err(|e| ExportError::Options(format!("Failed to encode PNG bitmap: {e}")))?;
    Ok(buf)
}

fn encode_webp_rgba(img: &neoflash_core::DecodedImage) -> Result<Vec<u8>, ExportError> {
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .encode(&img.rgba, img.width, img.height, ExtendedColorType::Rgba8)
        .map_err(|e| ExportError::Options(format!("Failed to encode WebP bitmap: {e}")))?;
    Ok(buf)
}

fn encode_jpeg_composited(
    img: &neoflash_core::DecodedImage,
    bg: Color,
    quality: u8,
) -> Result<(Vec<u8>, bool), ExportError> {
    let has_alpha = img.rgba.as_chunks::<4>().0.iter().any(|px| px[3] < 255);
    let mut rgb = Vec::with_capacity((img.width as usize) * (img.height as usize) * 3);
    for px in img.rgba.as_chunks::<4>().0 {
        let a = px[3] as u16;
        let inv = 255 - a;
        rgb.push(((px[0] as u16 * a + bg[0] as u16 * inv + 127) / 255) as u8);
        rgb.push(((px[1] as u16 * a + bg[1] as u16 * inv + 127) / 255) as u8);
        rgb.push(((px[2] as u16 * a + bg[2] as u16 * inv + 127) / 255) as u8);
    }
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100))
        .write_image(&rgb, img.width, img.height, ExtendedColorType::Rgb8)
        .map_err(|e| ExportError::Options(format!("Failed to encode JPEG bitmap: {e}")))?;
    Ok((buf, has_alpha))
}

struct LottieBuilder<'a> {
    project: &'a Project,
    options: &'a LottieOptions,
    cancel: Option<&'a AtomicBool>,
    source_fps: f64,
    fps: f64,
    comp_w: u32,
    comp_h: u32,
    package_external_images: bool,
    assets: Vec<Value>,
    external_images: Vec<ExternalImageFile>,
    precomp_id_by_symbol: BTreeMap<String, Option<String>>,
    image_id_by_resource: BTreeMap<String, Option<String>>,
    matte_precomps: BTreeMap<String, Option<String>>,
    used_asset_ids: BTreeSet<String>,
    fonts: BTreeMap<String, Value>,
    emitted_text_layer: bool,
    exported_count: usize,
    notes: Vec<LottieNote>,
    warnings: Vec<String>,
    warning_set: BTreeSet<String>,
}

impl<'a> LottieBuilder<'a> {
    fn check_cancelled(&self) -> Result<(), ExportError> {
        if self.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(ExportError::Options("Export cancelled.".into()));
        }
        Ok(())
    }

    fn alloc_unique_asset_id(&mut self, prefix: &str, raw: &str) -> String {
        let slug = sanitize_id(raw);
        let base = if slug.is_empty() {
            format!("{prefix}asset")
        } else {
            format!("{prefix}{slug}")
        };
        if self.used_asset_ids.insert(base.clone()) {
            return base;
        }
        let mut suffix = 2u32;
        loop {
            let candidate = format!("{base}_{suffix}");
            if self.used_asset_ids.insert(candidate.clone()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_note_coded(
        &mut self,
        kind: LottieNoteKind,
        code: Option<&str>,
        location: impl Into<String>,
        message: impl Into<String>,
        symbol_id: Option<String>,
        layer_id: Option<String>,
        frame: Option<u32>,
    ) {
        let location = location.into();
        let message = message.into();
        let combined = if location.is_empty() {
            message.clone()
        } else {
            format!("{location}: {message}")
        };
        if self.warning_set.insert(combined.clone()) {
            self.warnings.push(combined);
            self.notes.push(LottieNote {
                kind,
                code: code.map(str::to_owned),
                location,
                message,
                symbol_id,
                layer_id,
                frame,
            });
        }
    }

    fn register_font(&mut self, family: &str, bold: bool, italic: bool) -> String {
        let clean_family = if family.trim().is_empty() {
            "sans-serif"
        } else {
            family.trim()
        };
        let style = match (bold, italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            (false, false) => "Regular",
        };
        let f_name = if bold || italic {
            format!("{clean_family}-{}", style.replace(' ', ""))
        } else {
            clean_family.to_owned()
        };
        self.fonts.entry(f_name.clone()).or_insert_with(|| {
            json!({
                "fName": f_name,
                "fFamily": clean_family,
                "fStyle": style,
                "ascent": 75.0
            })
        });
        f_name
    }

    fn map_blend_mode(
        &mut self,
        mode: BlendMode,
        context: &str,
        symbol_id: Option<&str>,
        layer_id: Option<&str>,
        frame: Option<u32>,
    ) -> u32 {
        let bm = match mode {
            BlendMode::Normal | BlendMode::Layer => 0,
            BlendMode::Multiply => 1,
            BlendMode::Screen => 2,
            BlendMode::Overlay => 3,
            BlendMode::Darken => 4,
            BlendMode::Lighten => 5,
            BlendMode::Hardlight => 8,
            BlendMode::Difference => 10,
            BlendMode::Add => 12,
            BlendMode::Subtract | BlendMode::Invert | BlendMode::Alpha | BlendMode::Erase => {
                self.add_note_coded(
                    LottieNoteKind::NotExported,
                    Some("LOTTIE_BLEND_MODE_UNSUPPORTED"),
                    context,
                    format!(
                        "Blend mode '{}' is not supported by Lottie v{LOTTIE_VERSION}; exported as 'normal'.",
                        mode.as_str()
                    ),
                    symbol_id.map(str::to_owned),
                    layer_id.map(str::to_owned),
                    frame,
                );
                0
            }
        };
        if bm != 0 && self.options.player != LottieTargetPlayer::LottieWeb {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_BLEND_MODE_PLAYER"),
                context,
                format!(
                    "Blend mode '{}' may render as 'normal' on {} player.",
                    mode.as_str(),
                    self.options.player.label()
                ),
                symbol_id.map(str::to_owned),
                layer_id.map(str::to_owned),
                frame,
            );
        }
        bm
    }

    fn ensure_image_asset(
        &mut self,
        resource_id: &str,
        fallback_w: u32,
        fallback_h: u32,
        symbol_id: Option<&str>,
        layer_id: Option<&str>,
        frame: Option<u32>,
    ) -> Result<Option<String>, ExportError> {
        self.check_cancelled()?;
        if let Some(cached) = self.image_id_by_resource.get(resource_id) {
            return Ok(cached.clone());
        }

        let Some(unit) = self.project.resources.iter().find(|r| r.id == resource_id) else {
            self.add_note_coded(
                LottieNoteKind::Missing,
                Some("LOTTIE_MISSING_BITMAP_RESOURCE"),
                format!("Resource '{resource_id}'"),
                format!("Bitmap resource '{resource_id}' is missing from project resources."),
                symbol_id.map(str::to_owned),
                layer_id.map(str::to_owned),
                frame,
            );
            self.image_id_by_resource
                .insert(resource_id.to_owned(), None);
            return Ok(None);
        };

        let raw_bytes = unit.decoded_bytes().ok().filter(|b| !b.is_empty());
        let decoded = unit.decode_image().ok();
        self.check_cancelled()?;

        let Some(img) = decoded else {
            self.add_note_coded(
                LottieNoteKind::Missing,
                Some("LOTTIE_INVALID_BITMAP_RESOURCE"),
                format!("Resource '{}'", unit.id),
                format!(
                    "Bitmap resource '{}' ('{}') could not be decoded as a valid raster image.",
                    unit.id, unit.path
                ),
                symbol_id.map(str::to_owned),
                layer_id.map(str::to_owned),
                frame,
            );
            self.image_id_by_resource
                .insert(resource_id.to_owned(), None);
            return Ok(None);
        };

        let w = if img.width > 0 {
            img.width
        } else {
            fallback_w.max(1)
        };
        let h = if img.height > 0 {
            img.height
        } else {
            fallback_h.max(1)
        };
        let detected = raw_bytes
            .as_deref()
            .map_or(DetectedRaster::Other, detect_raster_magic);
        let default_jpeg_quality = unit
            .bitmap_export
            .as_ref()
            .map(|s| s.jpeg_quality.clamp(1, 100))
            .unwrap_or(92);

        let (ext, mime_type, encoded_bytes) = match self.options.image_format {
            LottieImageFormat::Original => {
                if let Some(settings) = unit.bitmap_export.as_ref() {
                    match settings.compression {
                        BitmapCompression::PhotoJpeg => {
                            let (bytes, has_alpha) = encode_jpeg_composited(
                                &img,
                                self.project.background,
                                settings.jpeg_quality,
                            )?;
                            if has_alpha {
                                self.add_note_coded(
                                    LottieNoteKind::Approximated,
                                    Some("LOTTIE_JPEG_ALPHA_COMPOSITED"),
                                    format!("Resource '{}'", unit.id),
                                    format!(
                                        "Bitmap '{}' has transparency, which JPEG does not support; composited over stage background.",
                                        unit.path
                                    ),
                                    symbol_id.map(str::to_owned),
                                    layer_id.map(str::to_owned),
                                    frame,
                                );
                            }
                            ("jpg", "image/jpeg", bytes)
                        }
                        BitmapCompression::LosslessPng => {
                            let bytes = if detected == DetectedRaster::Png {
                                raw_bytes.unwrap()
                            } else {
                                encode_png_rgba(&img)?
                            };
                            ("png", "image/png", bytes)
                        }
                        BitmapCompression::Webp => {
                            let bytes = if detected == DetectedRaster::Webp {
                                raw_bytes.unwrap()
                            } else {
                                encode_webp_rgba(&img)?
                            };
                            ("webp", "image/webp", bytes)
                        }
                    }
                } else {
                    match detected {
                        DetectedRaster::Png => ("png", "image/png", raw_bytes.unwrap()),
                        DetectedRaster::Jpeg => ("jpg", "image/jpeg", raw_bytes.unwrap()),
                        DetectedRaster::Webp => ("webp", "image/webp", raw_bytes.unwrap()),
                        DetectedRaster::Gif => ("gif", "image/gif", raw_bytes.unwrap()),
                        DetectedRaster::Other => {
                            let png = encode_png_rgba(&img)?;
                            self.add_note_coded(
                                LottieNoteKind::Approximated,
                                Some("LOTTIE_BITMAP_TRANSCODED"),
                                format!("Resource '{}'", unit.id),
                                format!(
                                    "Bitmap '{}' ('{}') was transcoded to lossless PNG for Lottie compatibility.",
                                    unit.path, unit.media_type
                                ),
                                symbol_id.map(str::to_owned),
                                layer_id.map(str::to_owned),
                                frame,
                            );
                            ("png", "image/png", png)
                        }
                    }
                }
            }
            LottieImageFormat::Png => {
                let bytes = if detected == DetectedRaster::Png {
                    raw_bytes.unwrap()
                } else {
                    encode_png_rgba(&img)?
                };
                ("png", "image/png", bytes)
            }
            LottieImageFormat::Jpeg => {
                let bytes = if detected == DetectedRaster::Jpeg {
                    raw_bytes.unwrap()
                } else {
                    let (jpg, has_alpha) = encode_jpeg_composited(
                        &img,
                        self.project.background,
                        default_jpeg_quality,
                    )?;
                    if has_alpha {
                        self.add_note_coded(
                            LottieNoteKind::Approximated,
                            Some("LOTTIE_JPEG_ALPHA_COMPOSITED"),
                            format!("Resource '{}'", unit.id),
                            format!(
                                "Bitmap '{}' has transparency, which JPEG does not support; composited over stage background.",
                                unit.path
                            ),
                            symbol_id.map(str::to_owned),
                            layer_id.map(str::to_owned),
                            frame,
                        );
                    }
                    jpg
                };
                ("jpg", "image/jpeg", bytes)
            }
            LottieImageFormat::Webp => {
                let bytes = if detected == DetectedRaster::Webp {
                    raw_bytes.unwrap()
                } else {
                    encode_webp_rgba(&img)?
                };
                ("webp", "image/webp", bytes)
            }
        };

        self.check_cancelled()?;

        let asset_id = self.alloc_unique_asset_id("img_", resource_id);
        if self.package_external_images {
            let filename = format!("{asset_id}.{ext}");
            self.external_images.push(ExternalImageFile {
                relative_path: format!("images/{filename}"),
                bytes: encoded_bytes,
            });
            self.assets.push(json!({
                "id": asset_id,
                "w": w,
                "h": h,
                "u": "images/",
                "p": filename,
                "e": 0
            }));
        } else {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&encoded_bytes);
            let data_uri = format!("data:{mime_type};base64,{b64}");
            self.assets.push(json!({
                "id": asset_id,
                "w": w,
                "h": h,
                "u": "",
                "p": data_uri,
                "e": 1
            }));
        }
        self.exported_count += 1;
        self.image_id_by_resource
            .insert(resource_id.to_owned(), Some(asset_id.clone()));
        Ok(Some(asset_id))
    }

    fn ensure_symbol_precomp(
        &mut self,
        symbol_id: &str,
        caller_symbol_id: &str,
        caller_layer_id: Option<&str>,
        caller_frame: Option<u32>,
        stack: &mut Vec<String>,
    ) -> Result<Option<String>, ExportError> {
        self.check_cancelled()?;
        if stack.iter().any(|s| s == symbol_id) {
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_SYMBOL_CYCLE"),
                format!("Symbol '{symbol_id}'"),
                format!("Recursive symbol reference to '{symbol_id}' was skipped."),
                Some(caller_symbol_id.to_owned()),
                caller_layer_id.map(str::to_owned),
                caller_frame,
            );
            return Ok(None);
        }
        if let Some(cached) = self.precomp_id_by_symbol.get(symbol_id) {
            return Ok(cached.clone());
        }
        let Some(symbol) = self.project.symbol(symbol_id) else {
            self.add_note_coded(
                LottieNoteKind::Missing,
                Some("LOTTIE_MISSING_SYMBOL"),
                format!("Symbol '{symbol_id}'"),
                format!("Referenced symbol '{symbol_id}' is missing from project library."),
                Some(caller_symbol_id.to_owned()),
                caller_layer_id.map(str::to_owned),
                caller_frame,
            );
            self.precomp_id_by_symbol.insert(symbol_id.to_owned(), None);
            return Ok(None);
        };

        let asset_id = self.alloc_unique_asset_id("comp_", symbol_id);
        self.precomp_id_by_symbol
            .insert(symbol_id.to_owned(), Some(asset_id.clone()));
        stack.push(symbol_id.to_owned());

        let precomp_w = self.comp_w.max(1) * 2;
        let precomp_h = self.comp_h.max(1) * 2;
        let origin = [self.comp_w as f64, self.comp_h as f64];

        let layers = if symbol.kind == SymbolKind::Button {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_BUTTON_INTERACTIVE"),
                format!("Button symbol '{}'", symbol.name),
                format!(
                    "Button symbol '{}' interactive Over/Down/Hit states are not supported by Lottie players; exported static Up state.",
                    symbol.name
                ),
                Some(symbol.id.clone()),
                caller_layer_id.map(str::to_owned),
                caller_frame,
            );
            let up_timeline = build_button_up_timeline(&symbol.timeline, self.source_fps);
            self.build_timeline_layers(
                &symbol.id,
                &symbol.name,
                &up_timeline,
                origin,
                false,
                stack,
            )?
        } else {
            self.build_timeline_layers(
                &symbol.id,
                &symbol.name,
                &symbol.timeline,
                origin,
                false,
                stack,
            )?
        };
        stack.pop();

        self.assets.push(json!({
            "id": asset_id,
            "nm": symbol.name,
            "fr": round4(self.fps),
            "w": precomp_w,
            "h": precomp_h,
            "layers": layers
        }));
        Ok(Some(asset_id))
    }

    fn ensure_mask_matte_precomp(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        mask_layer: &Layer,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
    ) -> Result<Option<String>, ExportError> {
        self.check_cancelled()?;
        let cache_key = format!("{symbol_id}::{}", mask_layer.id);
        if let Some(cached) = self.matte_precomps.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut matte_layer = mask_layer.clone();
        matte_layer.properties.role = LayerRole::Normal;
        matte_layer.properties.mask = None;
        matte_layer.visible = true;
        let mut matte_layers_vec = Vec::new();
        for l in &timeline.layers {
            if l.properties.role == LayerRole::Guide {
                matte_layers_vec.push(l.clone());
            }
        }
        matte_layers_vec.push(matte_layer);
        let matte_timeline = Timeline {
            frame_count: timeline.frame_count.max(1),
            layers: matte_layers_vec,
            scripts: Vec::new(),
            guides: Vec::new(),
        };

        let asset_id =
            self.alloc_unique_asset_id("comp_matte_", &format!("{symbol_id}_{}", mask_layer.id));
        self.matte_precomps
            .insert(cache_key.clone(), Some(asset_id.clone()));

        let layers = self.build_timeline_layers(
            symbol_id,
            &format!("Matte · {}", mask_layer.name),
            &matte_timeline,
            origin_offset,
            false,
            stack,
        )?;
        if layers.is_empty() {
            self.matte_precomps.insert(cache_key, None);
            return Ok(None);
        }

        self.assets.push(json!({
            "id": asset_id,
            "nm": format!("Matte · {}", mask_layer.name),
            "fr": round4(self.fps),
            "w": self.comp_w.max(1),
            "h": self.comp_h.max(1),
            "layers": layers
        }));
        Ok(Some(asset_id))
    }

    fn build_timeline_layers(
        &mut self,
        symbol_id: &str,
        symbol_name: &str,
        timeline: &Timeline,
        origin_offset: [f64; 2],
        include_bg: bool,
        stack: &mut Vec<String>,
    ) -> Result<Vec<Value>, ExportError> {
        self.check_cancelled()?;
        if !timeline.scripts.is_empty() {
            let first_script_frame = timeline.scripts.first().map(|s| s.frame);
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_FRAME_SCRIPT"),
                format!("Timeline '{symbol_name}'"),
                format!(
                    "{} frame script(s) are not executed by Lottie players; timeline animation was exported.",
                    timeline.scripts.len()
                ),
                Some(symbol_id.to_owned()),
                None,
                first_script_frame,
            );
        }

        let total_frames = timeline.frame_count.max(1);
        let timeline_has_camera = timeline
            .layers
            .iter()
            .any(|l| l.keyframes.iter().any(|k| k.camera.is_some()));
        let mut lottie_layers = Vec::new();
        let mut next_ind: u32 = 1;

        // Collect mask layers by ID so masked layers can attach `masksProperties` or track mattes.
        let mut mask_layers: BTreeMap<&str, &Layer> = BTreeMap::new();
        for layer in &timeline.layers {
            if layer.properties.role == LayerRole::Mask {
                mask_layers.insert(layer.id.as_str(), layer);
            }
        }

        // In NeoFlash, `timeline.layers[0]` is the bottom layer and `layers.last()` is the top
        // layer. In Lottie, `layers[0]` is rendered on top, so iterate in reverse order.
        for layer in timeline.layers.iter().rev() {
            self.check_cancelled()?;
            for kf in &layer.keyframes {
                if kf.sound.is_some() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_TIMELINE_SOUND"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        format!(
                            "timeline sound is not supported in Lottie v{LOTTIE_VERSION} export."
                        ),
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if kf.video.is_some() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_TIMELINE_VIDEO"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        format!(
                            "timeline video span is not supported in Lottie v{LOTTIE_VERSION} export."
                        ),
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
            }
            if matches!(
                layer.properties.role,
                LayerRole::Guide | LayerRole::Folder | LayerRole::Mask
            ) {
                continue;
            }
            if !self.options.include_hidden_layers && !layer_effectively_visible(timeline, layer) {
                continue;
            }

            if !layer.properties.filters.is_empty() {
                self.add_note_coded(
                    LottieNoteKind::NotExported,
                    Some("LOTTIE_DISPLAY_FILTER"),
                    format!("Layer '{}'", layer.name),
                    format!(
                        "Layer display filter(s) are not supported in core Lottie v{LOTTIE_VERSION}."
                    ),
                    Some(symbol_id.to_owned()),
                    Some(layer.id.clone()),
                    Some(0),
                );
            }
            if layer.properties.scale9_grid.is_some() {
                self.add_note_coded(
                    LottieNoteKind::Approximated,
                    Some("LOTTIE_SCALE9_GRID"),
                    format!("Layer '{}'", layer.name),
                    "9-slice scaling guides are flattened into standard 2D transforms in Lottie export.",
                    Some(symbol_id.to_owned()),
                    Some(layer.id.clone()),
                    Some(0),
                );
            }
            for kf in &layer.keyframes {
                if !kf.filters.is_empty() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_DISPLAY_FILTER"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        format!(
                            "Keyframe display filter(s) are not supported in core Lottie v{LOTTIE_VERSION}."
                        ),
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if !kf.bitmap_fills.is_empty() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_BITMAP_FILL"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "Tiled/affine bitmap shape fills are not supported by core Lottie shapes; fallback solid/gradient paint exported.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if kf.perspective_grid.is_some() {
                    self.add_note_coded(
                        LottieNoteKind::NotExported,
                        Some("LOTTIE_PERSPECTIVE_3D"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "3D perspective grid planes are not supported in 2D Lottie export.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                if !kf.instance_overrides.is_empty() {
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_INSTANCE_OVERRIDE"),
                        format!("Layer '{}' (frame {})", layer.name, kf.frame + 1),
                        "Per-instance child property overrides share the base symbol precomp in Lottie export.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(kf.frame),
                    );
                }
                for vfx in &kf.vector_effects {
                    if vfx.freeform_gradient.is_some() || vfx.gradient_mesh.is_some() {
                        self.add_note_coded(
                            LottieNoteKind::NotExported,
                            Some("LOTTIE_VECTOR_MESH_GRADIENT"),
                            format!(
                                "Layer '{}' element '{}' (frame {})",
                                layer.name,
                                vfx.element_id,
                                kf.frame + 1
                            ),
                            "Freeform/mesh gradients cannot be represented as Lottie linear/radial gradients; base vector paint exported.",
                            Some(symbol_id.to_owned()),
                            Some(layer.id.clone()),
                            Some(kf.frame),
                        );
                    } else if !vfx.procedural_effects.is_empty()
                        || vfx.envelope_warp.is_some()
                        || vfx.vector_blend.is_some()
                    {
                        self.add_note_coded(
                            LottieNoteKind::Approximated,
                            Some("LOTTIE_VECTOR_EFFECT_BAKED"),
                            format!(
                                "Layer '{}' element '{}' (frame {})",
                                layer.name,
                                vfx.element_id,
                                kf.frame + 1
                            ),
                            "Procedural vector effects / envelope warps were baked into evaluated Bézier contours.",
                            Some(symbol_id.to_owned()),
                            Some(layer.id.clone()),
                            Some(kf.frame),
                        );
                    }
                }
            }

            let layer_end = layer
                .properties
                .end_frame
                .unwrap_or(total_frames)
                .min(total_frames);
            if layer_end == 0 || layer.keyframes.is_empty() {
                continue;
            }

            let layer_bm = self.map_blend_mode(
                layer_effective_blend_mode(timeline, layer),
                &format!("Layer '{}'", layer.name),
                Some(symbol_id),
                Some(&layer.id),
                Some(0),
            );
            let synced = layer_effectively_synced(timeline, layer);
            let mask_layer = layer
                .properties
                .mask
                .as_deref()
                .and_then(|id| mask_layers.get(id).copied());

            let spans = collect_layer_spans(layer, layer_end);
            for span in spans.into_iter().rev() {
                self.check_cancelled()?;
                let emitted = self.emit_span_layers(
                    symbol_id,
                    timeline,
                    timeline_has_camera,
                    layer,
                    &span,
                    total_frames,
                    layer_bm,
                    synced,
                    mask_layer,
                    origin_offset,
                    stack,
                    &mut next_ind,
                )?;
                lottie_layers.extend(emitted);
            }
        }

        if include_bg && self.project.background[3] > 0 {
            let [r, g, b, a] = self.project.background;
            let hex = format!("#{r:02x}{g:02x}{b:02x}");
            let mut bg_ks = static_ks(
                Transform {
                    alpha: f64::from(a) / 255.0,
                    ..Transform::default()
                },
                [0.0, 0.0],
                [0.0, 0.0],
            );
            if a < 255
                && let Some(obj) = bg_ks.as_object_mut()
            {
                obj.insert(
                    "o".into(),
                    json!({"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)}),
                );
            }
            lottie_layers.push(json!({
                "ddd": 0,
                "ind": next_ind,
                "ty": 1,
                "nm": "Stage Background",
                "sr": 1,
                "ks": bg_ks,
                "ao": 0,
                "sw": self.comp_w,
                "sh": self.comp_h,
                "sc": hex,
                "ip": 0,
                "op": total_frames,
                "st": 0,
                "bm": 0
            }));
        }

        Ok(lottie_layers)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_span_layers(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        timeline_has_camera: bool,
        layer: &Layer,
        span: &LayerSpan,
        total_frames: u32,
        layer_bm: u32,
        layer_synced: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        self.check_cancelled()?;
        let stage_center = [self.comp_w as f64 * 0.5, self.comp_h as f64 * 0.5];
        let sample_filtered_frame = |f: u32| -> Vec<Element> {
            let controllers = layer
                .key_at(f)
                .map(|k| k.controller_elements.as_slice())
                .unwrap_or(&[]);
            let cam_opt = if timeline_has_camera && !layer.properties.attach_to_camera {
                sample_timeline_camera(timeline, f)
            } else {
                None
            };
            let mut out_els = Vec::new();
            for mut el in sample_layer_in_at_fps(timeline, layer, f, self.source_fps) {
                if !controllers.is_empty() && controllers.iter().any(|cid| cid == &el.id) {
                    continue;
                }
                if let Some(cam) = &cam_opt {
                    let cam_m = camera_layer_matrix(cam, layer.properties.z_depth, stage_center);
                    let world_m = neoflash_core::affine_multiply(
                        cam_m,
                        neoflash_core::transform_matrix(el.transform),
                    );
                    if let Some(cam_t) = Transform::from_matrix(world_m) {
                        el.transform.x = cam_t.x;
                        el.transform.y = cam_t.y;
                        el.transform.scale_x = cam_t.scale_x;
                        el.transform.scale_y = cam_t.scale_y;
                        el.transform.rotation = cam_t.rotation;
                        el.transform.skew = cam_t.skew;
                    }
                    el.transform.color_transform =
                        cam.color_transform.then(el.transform.color_transform);
                }
                out_els.push(el);
            }
            out_els
        };

        let first_elements = sample_filtered_frame(span.start_frame);
        if first_elements.is_empty() {
            return Ok(Vec::new());
        }

        // Check if this contiguous non-empty span is a shape tween so we can emit morphed Lottie layers.
        let has_shape_tween = layer.keyframes.iter().any(|k| {
            k.frame >= span.start_frame
                && k.frame < span.end_frame
                && k.tween.is_some()
                && k.tween_kind == TweenKind::Shape
        });

        if has_shape_tween {
            return self.emit_sampled_shape_span(
                symbol_id,
                timeline,
                timeline_has_camera,
                layer,
                span,
                layer_bm,
                mask_layer,
                origin_offset,
                stack,
                next_ind,
            );
        }

        // Group elements by their ID across the span, rendered top-to-bottom (reverse of
        // `elements` slice order, since `elements.last()` is on top in NeoFlash).
        let dense = timeline_has_camera
            || layer.properties.motion_guide.is_some()
            || layer.keyframes.iter().any(|k| {
                !k.property_expressions.is_empty()
                    || !k.ik_armatures.is_empty()
                    || !k.asset_warps.is_empty()
                    || k.motion_path.is_some()
                    || k.orient_to_path
                    || k.roving
                    || (k.frame > span.start_frame
                        && k.frame + 1 < span.end_frame
                        && (k.keyed_properties.is_some() || k.motion_keys.is_some()))
                    || has_asymmetric_pair_ease(&k.property_eases)
                    || matches!(k.tween, Some(Easing::Bounce | Easing::Elastic))
                    || k.property_eases
                        .iter()
                        .any(|(_, e)| matches!(e, Easing::Bounce | Easing::Elastic))
                    || k.motion_tracks.iter().any(|track| {
                        track.motion_path.is_some()
                            || track.orient_to_path == Some(true)
                            || matches!(track.easing, Some(Easing::Bounce | Easing::Elastic))
                            || track.property_eases.as_ref().is_some_and(|eases| {
                                has_asymmetric_pair_ease(eases)
                                    || eases.iter().any(|(_, ease)| {
                                        matches!(ease, Easing::Bounce | Easing::Elastic)
                                    })
                            })
                    })
                    || k.motion_keys
                        .as_ref()
                        .is_some_and(|keys| keys.iter().any(|key| key.roving))
            });
        let sample_frames: Vec<u32> = if dense {
            (span.start_frame..span.end_frame).collect()
        } else {
            let mut frames = BTreeSet::new();
            frames.insert(span.start_frame);
            frames.insert(span.end_frame.saturating_sub(1).max(span.start_frame));
            for kf in &layer.keyframes {
                if kf.frame >= span.start_frame && kf.frame < span.end_frame {
                    frames.insert(kf.frame);
                    frames.insert(kf.frame.saturating_sub(1).max(span.start_frame));
                }
            }
            frames.into_iter().collect()
        };

        let mut tracks: Vec<String> = Vec::new();
        let mut track_samples: BTreeMap<String, Vec<(u32, Element)>> = BTreeMap::new();
        for f in &sample_frames {
            self.check_cancelled()?;
            for el in sample_filtered_frame(*f) {
                if !track_samples.contains_key(&el.id) {
                    tracks.push(el.id.clone());
                    track_samples.insert(el.id.clone(), Vec::new());
                }
                if el.visible
                    && let Some(vec) = track_samples.get_mut(&el.id)
                {
                    vec.push((*f, el));
                }
            }
        }

        // Pre-detect consistent intra-layer `element_parents` links (`child_id -> parent_id`)
        // across this span when no camera warp is active, so children can emit native Lottie
        // `"parent": parent_ind` links.
        let mut stable_parent_of: BTreeMap<String, String> = BTreeMap::new();
        if !timeline_has_camera {
            for child_id in &tracks {
                let mut candidate: Option<String> = None;
                let mut consistent = true;
                for &f in &sample_frames {
                    let pid = layer
                        .key_at(f)
                        .and_then(|k| k.parent_of(child_id))
                        .map(str::to_owned);
                    match (&candidate, pid) {
                        (None, Some(p)) if f == sample_frames[0] => candidate = Some(p),
                        (Some(expected), Some(p)) if expected == &p => {}
                        (None, None) => {}
                        _ => {
                            consistent = false;
                            break;
                        }
                    }
                }
                if consistent
                    && let Some(pid) = candidate
                    && track_samples.contains_key(&pid)
                {
                    stable_parent_of.insert(child_id.clone(), pid);
                }
            }
        }

        let mut parent_world_tracks = BTreeMap::new();
        for pid in stable_parent_of.values() {
            if parent_world_tracks.contains_key(pid) {
                continue;
            }
            if let Some(psamples) = track_samples.get(pid)
                && !psamples.is_empty()
            {
                let anchor = if matches!(psamples[0].1.kind, ElementKind::Instance { .. }) {
                    [self.comp_w as f64, self.comp_h as f64]
                } else {
                    [0.0, 0.0]
                };
                let mut map = BTreeMap::new();
                let mut invertible = true;
                for (f, pel) in psamples {
                    let m = neoflash_core::transform_matrix(pel.transform);
                    if invert_affine(m).is_none() {
                        invertible = false;
                        break;
                    }
                    map.insert(*f, m);
                }
                if invertible {
                    parent_world_tracks.insert(pid.clone(), (map, anchor));
                }
            }
        }

        let mut emitted_ind_by_element: BTreeMap<String, u32> = BTreeMap::new();
        let mut pending_parent_patch: Vec<(usize, String)> = Vec::new();
        let mut out = Vec::new();
        for element_id in tracks.into_iter().rev() {
            self.check_cancelled()?;
            let Some(mut samples) = track_samples.remove(&element_id) else {
                continue;
            };
            let mut effective_origin = origin_offset;
            let mut parent_target_id: Option<String> = None;
            let world_samples_for_mask: Vec<(u32, Transform)> =
                samples.iter().map(|(f, e)| (*f, e.transform)).collect();

            if let Some(pid) = stable_parent_of.get(&element_id)
                && let Some((pmap, panchor)) = parent_world_tracks.get(pid)
                && samples.iter().all(|(f, _)| pmap.contains_key(f))
            {
                let mut converted_ok = true;
                let mut local_samples = Vec::with_capacity(samples.len());
                for (f, mut el) in samples.iter().cloned() {
                    let pm = pmap[&f];
                    let Some(inv_pm) = invert_affine(pm) else {
                        converted_ok = false;
                        break;
                    };
                    let local_m = neoflash_core::affine_multiply(
                        inv_pm,
                        neoflash_core::transform_matrix(el.transform),
                    );
                    let Some(lt) = Transform::from_matrix(local_m) else {
                        converted_ok = false;
                        break;
                    };
                    el.transform.x = lt.x;
                    el.transform.y = lt.y;
                    el.transform.scale_x = lt.scale_x;
                    el.transform.scale_y = lt.scale_y;
                    el.transform.rotation = lt.rotation;
                    el.transform.skew = lt.skew;
                    local_samples.push((f, el));
                }
                if converted_ok {
                    samples = local_samples;
                    effective_origin = *panchor;
                    parent_target_id = Some(pid.clone());
                }
            }

            let emitted_layers = self.emit_element_track(
                symbol_id,
                timeline,
                layer,
                samples,
                &world_samples_for_mask,
                dense,
                total_frames,
                layer_bm,
                layer_synced,
                mask_layer,
                effective_origin,
                origin_offset,
                stack,
                next_ind,
            )?;
            if !emitted_layers.is_empty() {
                let content_idx_in_out = out.len() + emitted_layers.len() - 1;
                if let Some(ind_u64) = emitted_layers.last().and_then(|v| v["ind"].as_u64()) {
                    emitted_ind_by_element.insert(element_id.clone(), ind_u64 as u32);
                }
                if let Some(pid) = parent_target_id {
                    pending_parent_patch.push((content_idx_in_out, pid));
                }
                out.extend(emitted_layers);
            }
        }

        for (layer_idx, pid) in pending_parent_patch {
            if let Some(&p_ind) = emitted_ind_by_element.get(&pid)
                && let Some(obj) = out.get_mut(layer_idx).and_then(Value::as_object_mut)
            {
                obj.insert("parent".into(), json!(p_ind));
            }
        }

        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_sampled_shape_span(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        timeline_has_camera: bool,
        layer: &Layer,
        span: &LayerSpan,
        layer_bm: u32,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        let stage_center = [self.comp_w as f64 * 0.5, self.comp_h as f64 * 0.5];
        let apply_cam = |mut els: Vec<Element>, f: u32| -> Vec<Element> {
            let controllers = layer
                .key_at(f)
                .map(|k| k.controller_elements.as_slice())
                .unwrap_or(&[]);
            els.retain(|e| e.visible && !controllers.iter().any(|cid| cid == &e.id));
            if timeline_has_camera
                && !layer.properties.attach_to_camera
                && let Some(cam) = sample_timeline_camera(timeline, f)
            {
                let cam_m = camera_layer_matrix(&cam, layer.properties.z_depth, stage_center);
                for el in &mut els {
                    let world_m = neoflash_core::affine_multiply(
                        cam_m,
                        neoflash_core::transform_matrix(el.transform),
                    );
                    if let Some(cam_t) = Transform::from_matrix(world_m) {
                        el.transform.x = cam_t.x;
                        el.transform.y = cam_t.y;
                        el.transform.scale_x = cam_t.scale_x;
                        el.transform.scale_y = cam_t.scale_y;
                        el.transform.rotation = cam_t.rotation;
                        el.transform.skew = cam_t.skew;
                    }
                    el.transform.color_transform =
                        cam.color_transform.then(el.transform.color_transform);
                }
            }
            els
        };

        let mut frame_shapes: Vec<(u32, Element)> = Vec::new();
        let mut single_track = true;
        for f in span.start_frame..span.end_frame {
            self.check_cancelled()?;
            let mut els = apply_cam(
                sample_shape_tween_frame(timeline, layer, f, self.source_fps),
                f,
            );
            if els.len() == 1 && els[0].to_path_commands().is_some() {
                frame_shapes.push((f, els.pop().unwrap()));
            } else {
                single_track = false;
                break;
            }
        }

        if single_track
            && !frame_shapes.is_empty()
            && let Some(layers) = self.try_build_morphed_shape_layer(
                symbol_id,
                timeline,
                layer,
                span,
                &frame_shapes,
                layer_bm,
                mask_layer,
                origin_offset,
                stack,
                next_ind,
            )?
        {
            return Ok(layers);
        }

        // Fallback: emit 1-frame slices for arbitrary multi-shape topology changes.
        let mut out = Vec::new();
        for f in (span.start_frame..span.end_frame).rev() {
            self.check_cancelled()?;
            let els = apply_cam(
                sample_layer_in_at_fps(timeline, layer, f, self.source_fps),
                f,
            );
            for el in els.into_iter().rev() {
                if let Some(shape_group) = self.element_to_shape_group(
                    &el,
                    [0.0, 0.0],
                    true,
                    None,
                    Some(symbol_id),
                    Some(&layer.id),
                    Some(f),
                ) {
                    let ind = *next_ind;
                    *next_ind += 1;
                    let bm = if el.blend_mode.is_normal() {
                        layer_bm
                    } else {
                        self.map_blend_mode(
                            el.blend_mode,
                            &format!("Element '{}'", el.id),
                            Some(symbol_id),
                            Some(&layer.id),
                            Some(f),
                        )
                    };
                    let obj = json!({
                        "ddd": 0,
                        "ind": ind,
                        "ty": 4,
                        "nm": layer_element_name(layer, &el),
                        "sr": 1,
                        "ks": static_ks(el.transform, [0.0, 0.0], origin_offset),
                        "ao": 0,
                        "shapes": [shape_group],
                        "ip": f,
                        "op": f + 1,
                        "st": 0,
                        "bm": bm
                    });
                    let world_mask_samples = [(f, el.transform)];
                    let wrapped = self.attach_mask_or_matte(
                        symbol_id,
                        timeline,
                        mask_layer,
                        obj,
                        f,
                        f + 1,
                        &world_mask_samples,
                        origin_offset,
                        stack,
                        next_ind,
                    )?;
                    out.extend(wrapped);
                }
            }
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn try_build_morphed_shape_layer(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        span: &LayerSpan,
        frame_shapes: &[(u32, Element)],
        layer_bm: u32,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Option<Vec<Value>>, ExportError> {
        struct MorphFrameSample {
            frame: u32,
            contours: Vec<LottieBezier>,
            fill: Color,
            stroke: Color,
            fill_grad: Option<GradientPaint>,
            stroke_grad: Option<GradientPaint>,
            stroke_width: f64,
            fill_rule: FillRule,
        }

        let mut samples_per_frame: Vec<MorphFrameSample> = Vec::with_capacity(frame_shapes.len());
        for (f, el) in frame_shapes {
            self.check_cancelled()?;
            let resolved_el = resolve_parametric_stroke_element(el);
            let target_el = resolved_el.as_ref().unwrap_or(el);
            let Some((cmds, fill_rule)) = target_el.to_path_commands() else {
                return Ok(None);
            };
            let m = neoflash_core::transform_matrix(Transform {
                x: el.transform.x + origin_offset[0],
                y: el.transform.y + origin_offset[1],
                ..el.transform
            });
            let tx_cmds = transform_path_commands(&cmds, m);
            let contours = path_commands_to_lottie_beziers(&tx_cmds);
            if contours.is_empty() {
                return Ok(None);
            }
            let (raw_fill, raw_stroke, stroke_width) = extract_element_paint(target_el);
            let ct = el.transform.color_transform;
            let fill = ct.apply(raw_fill);
            let stroke = ct.apply(raw_stroke);
            let (fill_grad, stroke_grad) = match &target_el.kind {
                ElementKind::PaintedPath {
                    fill_gradient,
                    stroke_gradient,
                    ..
                } => (
                    fill_gradient
                        .as_ref()
                        .map(|g| transform_gradient_paint(g, m, ct, true)),
                    stroke_gradient
                        .as_ref()
                        .map(|g| transform_gradient_paint(g, m, ct, true)),
                ),
                _ => (None, None),
            };
            samples_per_frame.push(MorphFrameSample {
                frame: *f,
                contours,
                fill,
                stroke,
                fill_grad,
                stroke_grad,
                stroke_width,
                fill_rule,
            });
        }

        let contour_count = samples_per_frame[0].contours.len();
        let vertex_counts: Vec<usize> = samples_per_frame[0]
            .contours
            .iter()
            .map(|c| c.v.len())
            .collect();
        if samples_per_frame.iter().any(|s| {
            s.contours.len() != contour_count
                || s.contours
                    .iter()
                    .zip(&vertex_counts)
                    .any(|(c, &expected)| c.v.len() != expected)
        }) {
            return Ok(None);
        }

        let any_fill_grad = samples_per_frame.iter().any(|s| s.fill_grad.is_some());
        let any_stroke_grad = samples_per_frame.iter().any(|s| s.stroke_grad.is_some());
        let normalized_fill_grads: Option<Vec<(u32, GradientPaint)>> = if any_fill_grad {
            let Some(template) = samples_per_frame.iter().find_map(|s| s.fill_grad.as_ref()) else {
                return Ok(None);
            };
            let kind = template.kind;
            let stop_len = template.stops.len();
            if stop_len == 0 {
                return Ok(None);
            }
            let mut grads = Vec::with_capacity(samples_per_frame.len());
            for s in &samples_per_frame {
                if let Some(g) = &s.fill_grad {
                    if g.kind != kind || g.stops.len() != stop_len {
                        return Ok(None);
                    }
                    grads.push((s.frame, g.clone()));
                } else {
                    grads.push((s.frame, solid_as_matching_gradient(s.fill, template)));
                }
            }
            Some(grads)
        } else {
            None
        };

        let normalized_stroke_grads: Option<Vec<(u32, GradientPaint)>> = if any_stroke_grad {
            let Some(template) = samples_per_frame
                .iter()
                .find_map(|s| s.stroke_grad.as_ref())
            else {
                return Ok(None);
            };
            let kind = template.kind;
            let stop_len = template.stops.len();
            if stop_len == 0 {
                return Ok(None);
            }
            let mut grads = Vec::with_capacity(samples_per_frame.len());
            for s in &samples_per_frame {
                if let Some(g) = &s.stroke_grad {
                    if g.kind != kind || g.stops.len() != stop_len {
                        return Ok(None);
                    }
                    grads.push((s.frame, g.clone()));
                } else {
                    grads.push((s.frame, solid_as_matching_gradient(s.stroke, template)));
                }
            }
            Some(grads)
        } else {
            None
        };

        let mut items: Vec<Value> = Vec::new();
        for c_idx in 0..contour_count {
            let mut kfs = Vec::new();
            for (idx, s) in samples_per_frame.iter().enumerate() {
                let bez = s.contours[c_idx].to_json();
                if idx + 1 < samples_per_frame.len() {
                    kfs.push(json!({
                        "t": s.frame,
                        "s": [bez],
                        "i": {"x": [1.0], "y": [1.0]},
                        "o": {"x": [0.0], "y": [0.0]}
                    }));
                } else {
                    kfs.push(json!({
                        "t": s.frame,
                        "s": [bez]
                    }));
                }
            }
            let dir = samples_per_frame[0].contours[c_idx].lottie_direction();
            items.push(json!({
                "ty": "sh",
                "d": dir,
                "nm": format!("Path {}", c_idx + 1),
                "ks": {
                    "a": if kfs.len() > 1 { 1 } else { 0 },
                    "k": if kfs.len() > 1 { Value::Array(kfs) } else { samples_per_frame[0].contours[c_idx].to_json() }
                }
            }));
        }

        let fill_rule0 = samples_per_frame[0].fill_rule;
        let fill0 = samples_per_frame[0].fill;
        let stroke0 = samples_per_frame[0].stroke;
        let sw0 = samples_per_frame[0].stroke_width;

        if let Some(grads) = &normalized_fill_grads {
            self.note_gradient_approximations(&grads[0].1, symbol_id, &layer.id, span.start_frame);
            items.push(animated_gradient_fill_item(grads, fill_rule0));
        } else if samples_per_frame.iter().any(|s| s.fill[3] > 0) {
            let fill_rgb_varies = samples_per_frame
                .iter()
                .any(|s| s.fill[0..3] != fill0[0..3]);
            let fill_alpha_varies = samples_per_frame.iter().any(|s| s.fill[3] != fill0[3]);
            let c_val = if fill_rgb_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let [r, g, b, _] = s.fill;
                        let c = [
                            round4(f64::from(r) / 255.0),
                            round4(f64::from(g) / 255.0),
                            round4(f64::from(b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": c, "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = fill0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if fill_alpha_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let o = round4(f64::from(s.fill[3]) / 255.0 * 100.0);
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": [o], "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": [o]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(fill0[3]) / 255.0 * 100.0)})
            };
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": c_val,
                "o": o_val,
                "r": if fill_rule0 == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }

        if let Some(grads) = &normalized_stroke_grads
            && samples_per_frame.iter().any(|s| s.stroke_width > 0.0)
        {
            self.note_gradient_approximations(&grads[0].1, symbol_id, &layer.id, span.start_frame);
            let widths: Vec<(u32, f64)> = samples_per_frame
                .iter()
                .map(|s| (s.frame, s.stroke_width))
                .collect();
            items.push(animated_gradient_stroke_item(grads, &widths));
        } else if samples_per_frame
            .iter()
            .any(|s| s.stroke[3] > 0 && s.stroke_width > 0.0)
        {
            let stroke_rgb_varies = samples_per_frame
                .iter()
                .any(|s| s.stroke[0..3] != stroke0[0..3]);
            let stroke_alpha_varies = samples_per_frame.iter().any(|s| s.stroke[3] != stroke0[3]);
            let sw_varies = samples_per_frame
                .iter()
                .any(|s| (s.stroke_width - sw0).abs() > 1e-4);
            let c_val = if stroke_rgb_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let [r, g, b, _] = s.stroke;
                        let c = [
                            round4(f64::from(r) / 255.0),
                            round4(f64::from(g) / 255.0),
                            round4(f64::from(b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": c, "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = stroke0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if stroke_alpha_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let o = round4(f64::from(s.stroke[3]) / 255.0 * 100.0);
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": [o], "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": [o]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(stroke0[3]) / 255.0 * 100.0)})
            };
            let w_val = if sw_varies && samples_per_frame.len() > 1 {
                let kfs: Vec<Value> = samples_per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, s)| {
                        let w = round4(s.stroke_width);
                        if idx + 1 < samples_per_frame.len() {
                            json!({"t": s.frame, "s": [w], "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
                        } else {
                            json!({"t": s.frame, "s": [w]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(sw0)})
            };
            items.push(json!({
                "ty": "st",
                "nm": "Stroke",
                "c": c_val,
                "o": o_val,
                "w": w_val,
                "lc": 2,
                "lj": 2,
                "ml": 4,
                "bm": 0
            }));
        }
        items.push(identity_group_tr());

        let ind = *next_ind;
        *next_ind += 1;
        let first_el = &frame_shapes[0].1;
        let obj = json!({
            "ddd": 0,
            "ind": ind,
            "ty": 4,
            "nm": layer_element_name(layer, first_el),
            "sr": 1,
            "ks": static_ks(Transform::default(), [0.0, 0.0], [0.0, 0.0]),
            "ao": 0,
            "shapes": [{
                "ty": "gr",
                "nm": "Shape Group",
                "it": items,
                "bm": 0
            }],
            "ip": span.start_frame,
            "op": span.end_frame,
            "st": 0,
            "bm": layer_bm
        });
        let identity_mask_samples: Vec<(u32, Transform)> = frame_shapes
            .iter()
            .map(|(f, _)| {
                (
                    *f,
                    Transform {
                        x: -origin_offset[0],
                        y: -origin_offset[1],
                        ..Transform::default()
                    },
                )
            })
            .collect();
        let wrapped = self.attach_mask_or_matte(
            symbol_id,
            timeline,
            mask_layer,
            obj,
            span.start_frame,
            span.end_frame,
            &identity_mask_samples,
            origin_offset,
            stack,
            next_ind,
        )?;
        self.exported_count += 1;
        Ok(Some(wrapped))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_element_track(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        layer: &Layer,
        samples: Vec<(u32, Element)>,
        world_samples_for_mask: &[(u32, Transform)],
        force_dense_ks: bool,
        _total_frames: u32,
        layer_bm: u32,
        layer_synced: bool,
        mask_layer: Option<&Layer>,
        origin_offset: [f64; 2],
        comp_origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        self.check_cancelled()?;
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        let first_frame = samples.first().unwrap().0;
        let last_frame = samples.last().unwrap().0 + 1;
        let representative = &samples[0].1;

        if !representative.scripts.is_empty() {
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_OBJECT_SCRIPT"),
                format!(
                    "Layer '{}' element '{}'",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                "object script(s) are not executed by Lottie players.",
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }
        if samples.iter().any(|(_, e)| !e.filters.is_empty()) {
            self.add_note_coded(
                LottieNoteKind::NotExported,
                Some("LOTTIE_DISPLAY_FILTER"),
                format!(
                    "Layer '{}' element '{}'",
                    layer.name,
                    layer_element_name(layer, representative)
                ),
                format!("display filter(s) are not exported to Lottie v{LOTTIE_VERSION}."),
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }
        if representative.scale9_grid().is_some()
            && samples
                .iter()
                .any(|(_, e)| (e.transform.scale_x - e.transform.scale_y).abs() > 1e-3)
        {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_SCALE9_GRID"),
                format!("Layer '{}' · {}", layer.name, representative.id),
                "Element 9-slice grid is approximated with uniform/affine layer scaling in Lottie.",
                Some(symbol_id.to_owned()),
                Some(layer.id.clone()),
                Some(first_frame),
            );
        }

        let bm = if representative.blend_mode.is_normal() {
            layer_bm
        } else {
            self.map_blend_mode(
                representative.blend_mode,
                &format!("Layer '{}' element '{}'", layer.name, representative.id),
                Some(symbol_id),
                Some(&layer.id),
                Some(first_frame),
            )
        };

        let orient_auto = layer.keyframes.iter().any(|k| {
            k.frame >= first_frame
                && k.frame < last_frame
                && k.motion_orient_to_path_for(&representative.id)
        });

        match &representative.kind {
            ElementKind::Instance {
                symbol_id: child_symbol_id,
                playback,
            } => {
                let Some(ref_id) = self.ensure_symbol_precomp(
                    child_symbol_id,
                    symbol_id,
                    Some(&layer.id),
                    Some(first_frame),
                    stack,
                )?
                else {
                    return Ok(Vec::new());
                };
                if samples
                    .iter()
                    .any(|(_, e)| has_non_alpha_color_transform(e.transform.color_transform))
                {
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_COLOR_TRANSFORM_APPROX"),
                        format!(
                            "Layer '{}' · {}",
                            layer.name,
                            layer_element_name(layer, representative)
                        ),
                        "RGB tint/color-offset transform on symbol instance cannot be represented losslessly on a core Lottie precomp layer; alpha channel was preserved.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(first_frame),
                    );
                }
                let precomp_anchor = [self.comp_w as f64, self.comp_h as f64];
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    precomp_anchor,
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let target_sym = self.project.symbol(child_symbol_id);
                let is_button = target_sym.is_some_and(|s| s.kind == SymbolKind::Button);
                let effective_playback = if is_button {
                    InstancePlayback::SingleFrame(0)
                } else if self.options.movie_clip_mode == LottieMovieClipMode::FirstFrame
                    && target_sym.is_some_and(|s| s.kind == SymbolKind::MovieClip)
                {
                    if target_sym.is_some_and(|s| s.timeline.frame_count > 1) {
                        self.add_note_coded(
                            LottieNoteKind::Approximated,
                            Some("LOTTIE_MOVIECLIP_FIRST_FRAME"),
                            format!("Layer '{}' · {}", layer.name, representative.name),
                            format!(
                                "MovieClip '{}' exported as first frame only (Movie clips: First frame).",
                                child_symbol_id
                            ),
                            Some(child_symbol_id.clone()),
                            Some(layer.id.clone()),
                            Some(first_frame),
                        );
                    }
                    InstancePlayback::SingleFrame(0)
                } else {
                    *playback
                };
                let mut obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 0,
                    "nm": layer_element_name(layer, representative),
                    "refId": ref_id,
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "w": self.comp_w.max(1) * 2,
                    "h": self.comp_h.max(1) * 2,
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                if let Some(tm) = self.build_instance_time_remap(
                    timeline,
                    layer,
                    &representative.id,
                    child_symbol_id,
                    effective_playback,
                    is_button,
                    layer_synced,
                    first_frame,
                    last_frame,
                ) {
                    obj.as_object_mut().unwrap().insert("tm".into(), tm);
                }
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
            ElementKind::Bitmap {
                resource_id,
                width,
                height,
                ..
            } => {
                let Some(ref_id) = self.ensure_image_asset(
                    resource_id,
                    *width,
                    *height,
                    Some(symbol_id),
                    Some(&layer.id),
                    Some(first_frame),
                )?
                else {
                    return Ok(Vec::new());
                };
                if samples
                    .iter()
                    .any(|(_, e)| has_non_alpha_color_transform(e.transform.color_transform))
                {
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_COLOR_TRANSFORM_APPROX"),
                        format!(
                            "Layer '{}' · {}",
                            layer.name,
                            layer_element_name(layer, representative)
                        ),
                        "RGB tint/color-offset transform on bitmap instance cannot be represented losslessly on a core Lottie image layer; alpha channel was preserved.",
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(first_frame),
                    );
                }
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    [0.0, 0.0],
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 2,
                    "nm": layer_element_name(layer, representative),
                    "refId": ref_id,
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => {
                self.emitted_text_layer = true;
                let f_name = self.register_font(font_family, false, false);
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    [0.0, 0.0],
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let [r, g, b, _] = representative.transform.color_transform.apply(*color);
                let obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 5,
                    "nm": layer_element_name(layer, representative),
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "t": {
                        "d": {
                            "k": [{
                                "t": 0,
                                "s": {
                                    "s": round4(*font_size),
                                    "f": f_name,
                                    "t": text.replace('\n', "\r"),
                                    "j": 0,
                                    "tr": 0,
                                    "lh": round4(*font_size * 1.2),
                                    "ls": 0,
                                    "fc": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0)]
                                }
                            }]
                        },
                        "p": {},
                        "m": {"g": 1, "a": {"a": 0, "k": [0, 0]}},
                        "a": []
                    },
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                ..
            } => {
                self.emitted_text_layer = true;
                if let Some(tf_state) =
                    neoflash_core::TextFieldState::from_kind(&representative.kind)
                {
                    let kind_label = match tf_state.field_type {
                        TextFieldType::Input => "Input",
                        TextFieldType::Dynamic => "Dynamic",
                    };
                    self.add_note_coded(
                        LottieNoteKind::Approximated,
                        Some("LOTTIE_DYNAMIC_INPUT_TEXT"),
                        format!(
                            "Layer '{}' · {}",
                            layer.name,
                            layer_element_name(layer, representative)
                        ),
                        format!(
                            "{kind_label} text field interactivity/runtime binding is not supported by Lottie players; static text content was exported."
                        ),
                        Some(symbol_id.to_owned()),
                        Some(layer.id.clone()),
                        Some(first_frame),
                    );
                }
                let f_name = self.register_font(&format.font, format.bold, format.italic);
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    [0.0, 0.0],
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let fmt_rgba = [
                    ((format.color >> 16) & 0xFF) as u8,
                    ((format.color >> 8) & 0xFF) as u8,
                    (format.color & 0xFF) as u8,
                    255,
                ];
                let [r, g, b, _] = representative.transform.color_transform.apply(fmt_rgba);
                let justify = match format.effective_align_mode() {
                    "right" | "end" => 1,
                    "center" => 2,
                    "justify" | "justify_all" => 3,
                    _ => match format.align {
                        TextAlign::Left => 0,
                        TextAlign::Right => 1,
                        TextAlign::Center => 2,
                    },
                };
                let tracking = if format.size > 1e-3 {
                    round4((format.letter_spacing / format.size) * 1000.0)
                } else {
                    0.0
                };
                let obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 5,
                    "nm": layer_element_name(layer, representative),
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "t": {
                        "d": {
                            "k": [{
                                "t": 0,
                                "s": {
                                    "s": round4(format.size),
                                    "f": f_name,
                                    "t": text.replace('\n', "\r"),
                                    "j": justify,
                                    "tr": tracking,
                                    "lh": round4(format.size * 1.2 + format.leading),
                                    "ls": 0,
                                    "fc": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0)],
                                    "sz": [round4(*width), round4(*height)],
                                    "ps": [0.0, 0.0]
                                }
                            }]
                        },
                        "p": {},
                        "m": {"g": 1, "a": {"a": 0, "k": [0, 0]}},
                        "a": []
                    },
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
            ElementKind::Video { .. } => {
                self.add_note_coded(
                    LottieNoteKind::NotExported,
                    Some("LOTTIE_VIDEO_ELEMENT"),
                    format!(
                        "Layer '{}' element '{}'",
                        layer.name,
                        layer_element_name(layer, representative)
                    ),
                    format!("video elements are not supported by Lottie v{LOTTIE_VERSION}."),
                    Some(symbol_id.to_owned()),
                    Some(layer.id.clone()),
                    Some(first_frame),
                );
                Ok(Vec::new())
            }
            ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. } => {
                let dash_override =
                    resolve_native_dash_stroke(layer, first_frame, &representative.id);
                let Some(shape_group) = self.build_shape_group_for_samples(
                    layer,
                    &samples,
                    force_dense_ks,
                    dash_override.as_ref(),
                    symbol_id,
                ) else {
                    return Ok(Vec::new());
                };
                let ks = self.build_animated_ks(
                    timeline,
                    layer,
                    &samples,
                    [0.0, 0.0],
                    origin_offset,
                    force_dense_ks,
                    orient_auto,
                );
                let ind = *next_ind;
                *next_ind += 1;
                let obj = json!({
                    "ddd": 0,
                    "ind": ind,
                    "ty": 4,
                    "nm": layer_element_name(layer, representative),
                    "sr": 1,
                    "ks": ks,
                    "ao": if orient_auto { 1 } else { 0 },
                    "shapes": [shape_group],
                    "ip": first_frame,
                    "op": last_frame,
                    "st": first_frame,
                    "bm": bm
                });
                let wrapped = self.attach_mask_or_matte(
                    symbol_id,
                    timeline,
                    mask_layer,
                    obj,
                    first_frame,
                    last_frame,
                    world_samples_for_mask,
                    comp_origin_offset,
                    stack,
                    next_ind,
                )?;
                self.exported_count += 1;
                Ok(wrapped)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_instance_time_remap(
        &self,
        timeline: &Timeline,
        layer: &Layer,
        element_id: &str,
        symbol_id: &str,
        playback: InstancePlayback,
        is_button: bool,
        layer_synced: bool,
        first_frame: u32,
        last_frame: u32,
    ) -> Option<Value> {
        let child_sym = self.project.symbol(symbol_id)?;
        let child_len = if is_button {
            1
        } else {
            child_sym.timeline.frame_count.max(1)
        };
        if child_len <= 1 {
            if last_frame > first_frame + 1 {
                return Some(json!({
                    "a": 0,
                    "k": 0.0
                }));
            }
            return None;
        }
        let span_start = placement_start(layer, first_frame, element_id);
        let child_frame_at = |parent_frame: u32| -> u32 {
            let age = if layer_synced {
                parent_frame
            } else {
                parent_frame.saturating_sub(span_start)
            };
            match playback {
                InstancePlayback::SingleFrame(f) => f.min(child_len - 1),
                InstancePlayback::PlayOnce => age.min(child_len - 1),
                InstancePlayback::PlayOnceFrom(start) => {
                    start.saturating_add(age).min(child_len - 1)
                }
                InstancePlayback::Loop => age % child_len,
                InstancePlayback::LoopFrom(start) => start.saturating_add(age) % child_len,
                InstancePlayback::Independent => {
                    if layer_synced {
                        parent_frame % child_len
                    } else {
                        age % child_len
                    }
                }
            }
        };

        if let InstancePlayback::SingleFrame(f) = playback {
            return Some(json!({
                "a": 0,
                "k": round4(f.min(child_len - 1) as f64 / self.fps)
            }));
        }

        let _ = timeline;
        let matches_default =
            (first_frame..last_frame).all(|f| child_frame_at(f) == f.saturating_sub(first_frame));
        if matches_default {
            return None;
        }

        if last_frame <= first_frame + 1 {
            return Some(json!({
                "a": 0,
                "k": round4(child_frame_at(first_frame) as f64 / self.fps)
            }));
        }

        let mut kfs = Vec::with_capacity((last_frame - first_frame) as usize);
        for f in first_frame..last_frame {
            let cf = child_frame_at(f);
            let sec = round4(cf as f64 / self.fps);
            kfs.push(json!({
                "t": f,
                "s": [sec],
                "h": 1
            }));
        }
        Some(json!({
            "a": 1,
            "k": kfs
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_animated_ks(
        &self,
        timeline: &Timeline,
        layer: &Layer,
        samples: &[(u32, Element)],
        anchor: [f64; 2],
        origin_offset: [f64; 2],
        force_dense: bool,
        orient_auto: bool,
    ) -> Value {
        let first_t = samples[0].1.transform;
        let element_id = samples[0].1.id.as_str();
        let first_f = samples[0].0;
        let last_f = samples.last().unwrap().0;
        let has_classic_spin = layer.keyframes.iter().any(|k| {
            k.frame >= first_f
                && k.frame < last_f
                && k.tween.is_some()
                && k.tween_kind == TweenKind::Classic
                && (k.classic_rotate_turns > 0
                    || !matches!(k.classic_rotate_dir, ClassicRotateDirection::Auto))
        });

        let is_static = !has_classic_spin
            && !orient_auto
            && samples
                .iter()
                .all(|(_, e)| transforms_approx_eq(e.transform, first_t));
        if is_static {
            return static_ks(first_t, anchor, origin_offset);
        }

        let dense = force_dense
            || layer.properties.motion_guide.is_some()
            || layer.keyframes.iter().any(|k| {
                !k.property_expressions.is_empty()
                    || !matches!(k.motion_path_for(element_id), MotionPathSource::None)
                    || k.motion_orient_to_path_for(element_id)
                    || k.motion_key_roving_for(element_id)
                    || matches!(
                        k.motion_easing_for(element_id),
                        Easing::Bounce | Easing::Elastic
                    )
                    || TweenProperty::ALL.into_iter().any(|property| {
                        matches!(
                            k.motion_property_ease_for(element_id, property),
                            Easing::Bounce | Easing::Elastic
                        )
                    })
            });

        let key_frames: Vec<(u32, Transform, Option<Easing>)> = if dense {
            samples
                .iter()
                .enumerate()
                .map(|(idx, (f, el))| {
                    let ease = (idx + 1 < samples.len()).then_some(Easing::Linear);
                    (*f, el.transform, ease)
                })
                .collect()
        } else {
            collect_transform_keyframes(timeline, layer, samples, element_id)
        };

        let unwrapped_rotations = compute_unwrapped_rotations(layer, &key_frames, dense);
        let auto_orient_offsets = if orient_auto {
            compute_lottie_auto_orient_deg(&key_frames)
        } else {
            vec![0.0; key_frames.len()]
        };

        let p_kfs = build_vec3_keyframes(&key_frames, |t, f| {
            let ease = if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::X).or(t.2)
            };
            (
                [
                    round4(t.1.x + origin_offset[0]),
                    round4(t.1.y + origin_offset[1]),
                    0.0,
                ],
                ease,
            )
        });
        let s_kfs = build_vec3_keyframes(&key_frames, |t, f| {
            let hold_scale = !dense
                && motion_span(layer, f).is_none()
                && layer.key_at(f).is_some_and(|k| {
                    k.tween.is_some() && k.tween_kind == TweenKind::Classic && !k.tween_scale
                });
            let ease = if hold_scale {
                None
            } else if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::ScaleX).or(t.2)
            };
            let sk_cos = lottie_skew_cos(t.1.skew);
            (
                [
                    round4(t.1.scale_x * 100.0),
                    round4(t.1.scale_y * sk_cos * 100.0),
                    100.0,
                ],
                ease,
            )
        });
        let r_kfs = build_indexed_scalar_keyframes(&key_frames, |idx, t, f| {
            let (rot_deg, hold_rot) = unwrapped_rotations[idx];
            let compensated = rot_deg - auto_orient_offsets[idx];
            let ease = if hold_rot {
                None
            } else if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::Rotation).or(t.2)
            };
            (round4(compensated), ease)
        });
        let o_kfs = build_scalar_keyframes(&key_frames, |t, f| {
            let ease = if dense {
                t.2
            } else {
                ease_for_prop(layer, f, element_id, TweenProperty::Alpha).or(t.2)
            };
            (round4(effective_opacity_percent(t.1)), ease)
        });

        let mut ks = Map::new();
        ks.insert("o".into(), o_kfs);
        ks.insert("r".into(), r_kfs);
        ks.insert("p".into(), p_kfs);
        ks.insert(
            "a".into(),
            json!({"a": 0, "k": [round4(anchor[0]), round4(anchor[1]), 0.0]}),
        );
        ks.insert("s".into(), s_kfs);
        if samples.iter().any(|(_, e)| e.transform.skew.abs() > 1e-4) {
            let sk_kfs = build_scalar_keyframes(&key_frames, |t, f| {
                let ease = if dense {
                    t.2
                } else {
                    ease_for_prop(layer, f, element_id, TweenProperty::Skew).or(t.2)
                };
                (round4(lottie_skew_deg(t.1.skew)), ease)
            });
            ks.insert("sk".into(), sk_kfs);
            ks.insert("sa".into(), json!({"a": 0, "k": 0.0}));
        }
        Value::Object(ks)
    }

    fn note_gradient_approximations(
        &mut self,
        grad: &GradientPaint,
        symbol_id: &str,
        layer_id: &str,
        frame: u32,
    ) {
        if grad.spread != GradientSpread::Extend {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_GRADIENT_SPREAD"),
                format!("Layer '{layer_id}' (frame {})", frame + 1),
                format!(
                    "Gradient spread mode '{:?}' is approximated as pad/extend in Lottie.",
                    grad.spread
                ),
                Some(symbol_id.to_owned()),
                Some(layer_id.to_owned()),
                Some(frame),
            );
        }
        if grad.linear_rgb {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_GRADIENT_LINEAR_RGB"),
                format!("Layer '{layer_id}' (frame {})", frame + 1),
                "Linear-RGB gradient interpolation is approximated in standard sRGB in Lottie.",
                Some(symbol_id.to_owned()),
                Some(layer_id.to_owned()),
                Some(frame),
            );
        }
    }

    fn build_shape_group_for_samples(
        &mut self,
        layer: &Layer,
        samples: &[(u32, Element)],
        force_dense: bool,
        dash_override: Option<&NativeDashStroke>,
        symbol_id: &str,
    ) -> Option<Value> {
        let first_el = &samples[0].1;
        let first_f = samples[0].0;
        let shape_static = dash_override.is_some()
            || samples.iter().all(|(_, e)| {
                e.kind == first_el.kind
                    && e.transform.primitive == first_el.transform.primitive
                    && apply_rgb_only(e.transform.color_transform, [255, 255, 255, 255], false)
                        == apply_rgb_only(
                            first_el.transform.color_transform,
                            [255, 255, 255, 255],
                            false,
                        )
            });

        if shape_static {
            return self.element_to_shape_group(
                first_el,
                [0.0, 0.0],
                false,
                dash_override,
                Some(symbol_id),
                Some(&layer.id),
                Some(first_f),
            );
        }

        let key_samples: Vec<(u32, &Element, Option<Easing>)> = if force_dense {
            samples
                .iter()
                .enumerate()
                .map(|(idx, (f, el))| {
                    let ease = (idx + 1 < samples.len()).then_some(Easing::Linear);
                    (*f, el, ease)
                })
                .collect()
        } else {
            let sample_map: BTreeMap<u32, &Element> =
                samples.iter().map(|(f, e)| (*f, e)).collect();
            let mut key_times = BTreeSet::new();
            key_times.insert(samples[0].0);
            key_times.insert(samples.last().unwrap().0);
            for kf in &layer.keyframes {
                if kf.frame >= samples[0].0
                    && kf.frame <= samples.last().unwrap().0
                    && sample_map.contains_key(&kf.frame)
                {
                    key_times.insert(kf.frame);
                }
            }
            key_times
                .into_iter()
                .map(|f| (f, sample_map[&f], layer_ease_at(layer, f, &first_el.id)))
                .collect()
        };

        let mut per_key = Vec::with_capacity(key_samples.len());
        for (f, el, ease) in &key_samples {
            let resolved = resolve_parametric_stroke_element(el);
            let target = resolved.as_ref().unwrap_or(el);
            if matches!(
                target.kind,
                ElementKind::PaintedPath {
                    fill_gradient: Some(_),
                    ..
                } | ElementKind::PaintedPath {
                    stroke_gradient: Some(_),
                    ..
                }
            ) {
                return self.element_to_shape_group(
                    first_el,
                    [0.0, 0.0],
                    false,
                    None,
                    Some(symbol_id),
                    Some(&layer.id),
                    Some(first_f),
                );
            }
            let (cmds, fill_rule) = target.to_path_commands()?;
            let contours = path_commands_to_lottie_beziers(&cmds);
            if contours.is_empty() {
                return None;
            }
            let (raw_fill, raw_stroke, sw) = extract_element_paint(target);
            let ct = el.transform.color_transform;
            let fill = apply_rgb_only(ct, raw_fill, false);
            let stroke = apply_rgb_only(ct, raw_stroke, false);
            per_key.push((*f, *ease, contours, fill, stroke, sw, fill_rule));
        }

        let c_count = per_key[0].2.len();
        let v_counts: Vec<usize> = per_key[0].2.iter().map(|c| c.v.len()).collect();
        let compatible = per_key.iter().all(|(_, _, cs, _, _, _, _)| {
            cs.len() == c_count
                && cs
                    .iter()
                    .zip(&v_counts)
                    .all(|(c, &expected)| c.v.len() == expected)
        });
        if !compatible {
            return self.element_to_shape_group(
                first_el,
                [0.0, 0.0],
                false,
                None,
                Some(symbol_id),
                Some(&layer.id),
                Some(first_f),
            );
        }

        let mut items: Vec<Value> = Vec::new();
        for c_idx in 0..c_count {
            let first_json = per_key[0].2[c_idx].to_json();
            let dir = per_key[0].2[c_idx].lottie_direction();
            let geom_varies = per_key
                .iter()
                .any(|(_, _, cs, _, _, _, _)| cs[c_idx].to_json() != first_json);
            if geom_varies && per_key.len() > 1 {
                let mut kfs = Vec::with_capacity(per_key.len());
                for (idx, (f, ease, cs, _, _, _, _)) in per_key.iter().enumerate() {
                    let bez = cs[c_idx].to_json();
                    if idx + 1 < per_key.len() {
                        if let Some(e) = ease {
                            let (o, i) = easing_to_lottie_tangents(*e);
                            kfs.push(json!({
                                "t": *f,
                                "s": [bez],
                                "o": {"x": [o[0]], "y": [o[1]]},
                                "i": {"x": [i[0]], "y": [i[1]]}
                            }));
                        } else {
                            kfs.push(json!({"t": *f, "s": [bez], "h": 1}));
                        }
                    } else {
                        kfs.push(json!({"t": *f, "s": [bez]}));
                    }
                }
                items.push(json!({
                    "ty": "sh",
                    "d": dir,
                    "nm": format!("Path {}", c_idx + 1),
                    "ks": {"a": 1, "k": kfs}
                }));
            } else {
                items.push(json!({
                    "ty": "sh",
                    "d": dir,
                    "nm": format!("Path {}", c_idx + 1),
                    "ks": {"a": 0, "k": first_json}
                }));
            }
        }

        let fill0 = per_key[0].3;
        let stroke0 = per_key[0].4;
        let sw0 = per_key[0].5;
        let fill_rule0 = per_key[0].6;

        if per_key.iter().any(|(_, _, _, f, _, _, _)| f[3] > 0) {
            let rgb_varies = per_key
                .iter()
                .any(|(_, _, _, f, _, _, _)| f[0..3] != fill0[0..3]);
            let a_varies = per_key.iter().any(|(_, _, _, f, _, _, _)| f[3] != fill0[3]);
            let c_val = if rgb_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, [r, g, b, _], _, _, _))| {
                        let c = [
                            round4(f64::from(*r) / 255.0),
                            round4(f64::from(*g) / 255.0),
                            round4(f64::from(*b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": c, "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": c, "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = fill0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if a_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, [_, _, _, a], _, _, _))| {
                        let o_pct = round4(f64::from(*a) / 255.0 * 100.0);
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": [o_pct], "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": [o_pct], "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": [o_pct]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(fill0[3]) / 255.0 * 100.0)})
            };
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": c_val,
                "o": o_val,
                "r": if fill_rule0 == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }

        if per_key
            .iter()
            .any(|(_, _, _, _, s, sw, _)| s[3] > 0 && *sw > 0.0)
        {
            let rgb_varies = per_key
                .iter()
                .any(|(_, _, _, _, s, _, _)| s[0..3] != stroke0[0..3]);
            let a_varies = per_key
                .iter()
                .any(|(_, _, _, _, s, _, _)| s[3] != stroke0[3]);
            let sw_varies = per_key
                .iter()
                .any(|(_, _, _, _, _, sw, _)| (*sw - sw0).abs() > 1e-4);
            let c_val = if rgb_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, _, [r, g, b, _], _, _))| {
                        let c = [
                            round4(f64::from(*r) / 255.0),
                            round4(f64::from(*g) / 255.0),
                            round4(f64::from(*b) / 255.0),
                            1.0,
                        ];
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": c, "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": c, "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": c})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                let [r, g, b, _] = stroke0;
                json!({"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]})
            };
            let o_val = if a_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, _, [_, _, _, a], _, _))| {
                        let o_pct = round4(f64::from(*a) / 255.0 * 100.0);
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": [o_pct], "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": [o_pct], "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": [o_pct]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(f64::from(stroke0[3]) / 255.0 * 100.0)})
            };
            let w_val = if sw_varies && per_key.len() > 1 {
                let kfs: Vec<Value> = per_key
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, ease, _, _, _, sw, _))| {
                        let w = round4(*sw);
                        if idx + 1 < per_key.len() {
                            if let Some(e) = ease {
                                let (o, i) = easing_to_lottie_tangents(*e);
                                json!({"t": *f, "s": [w], "o": {"x": [o[0]], "y": [o[1]]}, "i": {"x": [i[0]], "y": [i[1]]}})
                            } else {
                                json!({"t": *f, "s": [w], "h": 1})
                            }
                        } else {
                            json!({"t": *f, "s": [w]})
                        }
                    })
                    .collect();
                json!({"a": 1, "k": kfs})
            } else {
                json!({"a": 0, "k": round4(sw0)})
            };
            items.push(json!({
                "ty": "st",
                "nm": "Stroke",
                "c": c_val,
                "o": o_val,
                "w": w_val,
                "lc": 2,
                "lj": 2,
                "ml": 4,
                "bm": 0
            }));
        }

        items.push(identity_group_tr());
        Some(json!({
            "ty": "gr",
            "nm": if first_el.name.is_empty() { "Vector Shape" } else { &first_el.name },
            "it": items,
            "bm": 0
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn element_to_shape_group(
        &mut self,
        element: &Element,
        offset: [f64; 2],
        apply_color_tx_alpha: bool,
        dash_override: Option<&NativeDashStroke>,
        symbol_id: Option<&str>,
        layer_id: Option<&str>,
        frame: Option<u32>,
    ) -> Option<Value> {
        let resolved = if dash_override.is_none() {
            resolve_parametric_stroke_element(element)
        } else {
            None
        };
        let target = resolved.as_ref().unwrap_or(element);

        let (mut commands, fill_rule) = if let Some(d_ov) = dash_override {
            (d_ov.commands.clone(), FillRule::NonZero)
        } else {
            target.to_path_commands()?
        };
        if offset != [0.0, 0.0] {
            commands =
                transform_path_commands(&commands, [1.0, 0.0, 0.0, 1.0, offset[0], offset[1]]);
        }
        let contours = path_commands_to_lottie_beziers(&commands);
        if contours.is_empty() {
            return None;
        }
        let mut items: Vec<Value> = contours
            .into_iter()
            .enumerate()
            .map(|(i, bez)| {
                let dir = bez.lottie_direction();
                json!({
                    "ty": "sh",
                    "d": dir,
                    "nm": format!("Path {}", i + 1),
                    "ks": {
                        "a": 0,
                        "k": bez.to_json()
                    }
                })
            })
            .collect();

        let ct = element.transform.color_transform;
        if let Some(d_ov) = dash_override {
            let stroke = apply_rgb_only(ct, d_ov.color, apply_color_tx_alpha);
            if d_ov.width > 0.0 {
                let dash_json = build_lottie_dash_array(d_ov.dash, d_ov.gap);
                if let Some(grad) = &d_ov.gradient
                    && !grad.stops.is_empty()
                {
                    if let (Some(sid), Some(lid), Some(fr)) = (symbol_id, layer_id, frame) {
                        self.note_gradient_approximations(grad, sid, lid, fr);
                    }
                    let tinted = tint_gradient_paint(grad, ct, apply_color_tx_alpha);
                    let mut gs = gradient_stroke_item(&tinted, d_ov.width);
                    gs.as_object_mut()
                        .unwrap()
                        .insert("d".into(), Value::Array(dash_json));
                    items.push(gs);
                } else if stroke[3] > 0 {
                    let [r, g, b, a] = stroke;
                    items.push(json!({
                        "ty": "st",
                        "nm": "Stroke",
                        "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                        "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                        "w": {"a": 0, "k": round4(d_ov.width)},
                        "lc": 2,
                        "lj": 2,
                        "ml": 4,
                        "d": dash_json,
                        "bm": 0
                    }));
                }
            }
            items.push(identity_group_tr());
            return Some(json!({
                "ty": "gr",
                "nm": if element.name.is_empty() { "Vector Shape" } else { &element.name },
                "it": items,
                "bm": 0
            }));
        }

        let (raw_fill, raw_stroke, stroke_width) = extract_element_paint(target);
        let fill = apply_rgb_only(ct, raw_fill, apply_color_tx_alpha);
        let stroke = apply_rgb_only(ct, raw_stroke, apply_color_tx_alpha);

        let (fill_grad, stroke_grad) = match &target.kind {
            ElementKind::PaintedPath {
                fill_gradient,
                stroke_gradient,
                ..
            } => (fill_gradient.as_ref(), stroke_gradient.as_ref()),
            _ => (None, None),
        };

        if let Some(grad) = fill_grad
            && !grad.stops.is_empty()
        {
            if let (Some(sid), Some(lid), Some(fr)) = (symbol_id, layer_id, frame) {
                self.note_gradient_approximations(grad, sid, lid, fr);
            }
            let tinted = tint_gradient_paint(grad, ct, apply_color_tx_alpha);
            items.push(gradient_fill_item(&tinted, fill_rule));
        } else if fill[3] > 0 {
            let [r, g, b, a] = fill;
            items.push(json!({
                "ty": "fl",
                "nm": "Fill",
                "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
                "bm": 0
            }));
        }

        if stroke_width > 0.0 {
            if let Some(grad) = stroke_grad
                && !grad.stops.is_empty()
            {
                if let (Some(sid), Some(lid), Some(fr)) = (symbol_id, layer_id, frame) {
                    self.note_gradient_approximations(grad, sid, lid, fr);
                }
                let tinted = tint_gradient_paint(grad, ct, apply_color_tx_alpha);
                items.push(gradient_stroke_item(&tinted, stroke_width));
            } else if stroke[3] > 0 {
                let [r, g, b, a] = stroke;
                items.push(json!({
                    "ty": "st",
                    "nm": "Stroke",
                    "c": {"a": 0, "k": [round4(f64::from(r) / 255.0), round4(f64::from(g) / 255.0), round4(f64::from(b) / 255.0), 1.0]},
                    "o": {"a": 0, "k": round4(f64::from(a) / 255.0 * 100.0)},
                    "w": {"a": 0, "k": round4(stroke_width)},
                    "lc": 2,
                    "lj": 2,
                    "ml": 4,
                    "bm": 0
                }));
            }
        }

        items.push(identity_group_tr());
        Some(json!({
            "ty": "gr",
            "nm": if element.name.is_empty() { "Vector Shape" } else { &element.name },
            "it": items,
            "bm": 0
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn attach_mask_or_matte(
        &mut self,
        symbol_id: &str,
        timeline: &Timeline,
        mask_layer: Option<&Layer>,
        mut layer_obj: Value,
        first_frame: u32,
        last_frame: u32,
        world_samples: &[(u32, Transform)],
        comp_origin_offset: [f64; 2],
        stack: &mut Vec<String>,
        next_ind: &mut u32,
    ) -> Result<Vec<Value>, ExportError> {
        let Some(mask_layer) = mask_layer else {
            return Ok(vec![layer_obj]);
        };
        if !layer_effectively_visible(timeline, mask_layer) {
            return Ok(vec![layer_obj]);
        }

        let mask_has_non_vector = (first_frame..last_frame).any(|f| {
            sample_layer_in_at_fps(timeline, mask_layer, f, self.source_fps)
                .iter()
                .any(|el| el.visible && el.to_path_commands().is_none())
        });

        let use_track_matte =
            mask_layer.properties.mask_mode != MaskMode::Geometry || mask_has_non_vector;

        if !use_track_matte
            && self.try_attach_vector_masks(
                &mut layer_obj,
                timeline,
                mask_layer,
                first_frame,
                last_frame,
                world_samples,
            )
        {
            return Ok(vec![layer_obj]);
        }

        let Some(matte_ref_id) = self.ensure_mask_matte_precomp(
            symbol_id,
            timeline,
            mask_layer,
            comp_origin_offset,
            stack,
        )?
        else {
            return Ok(vec![layer_obj]);
        };

        let tt_code = match (
            mask_layer.properties.mask_mode,
            mask_layer.properties.mask_invert,
        ) {
            (MaskMode::Luminance, false) => 3,
            (MaskMode::Luminance, true) => 4,
            (_, false) => 1,
            (_, true) => 2,
        };

        if self.options.player != LottieTargetPlayer::LottieWeb && matches!(tt_code, 3 | 4) {
            self.add_note_coded(
                LottieNoteKind::Approximated,
                Some("LOTTIE_LUMA_MATTE_PLAYER"),
                format!("Mask layer '{}'", mask_layer.name),
                format!(
                    "Luminance track matte (`tt: {tt_code}`) may have limited support on {} player.",
                    self.options.player.label()
                ),
                Some(symbol_id.to_owned()),
                Some(mask_layer.id.clone()),
                Some(first_frame),
            );
        }

        let matte_ind = *next_ind;
        *next_ind += 1;
        let matte_layer_json = json!({
            "ddd": 0,
            "ind": matte_ind,
            "ty": 0,
            "nm": format!("Matte · {}", mask_layer.name),
            "refId": matte_ref_id,
            "td": 1,
            "sr": 1,
            "ks": static_ks(Transform::default(), [0.0, 0.0], [0.0, 0.0]),
            "ao": 0,
            "w": self.comp_w.max(1),
            "h": self.comp_h.max(1),
            "ip": first_frame,
            "op": last_frame,
            "st": 0,
            "bm": 0
        });
        if let Some(map) = layer_obj.as_object_mut() {
            map.insert("tt".into(), json!(tt_code));
        }
        Ok(vec![matte_layer_json, layer_obj])
    }

    fn try_attach_vector_masks(
        &mut self,
        layer_obj: &mut Value,
        timeline: &Timeline,
        mask_layer: &Layer,
        first_frame: u32,
        last_frame: u32,
        world_samples: &[(u32, Transform)],
    ) -> bool {
        let fps = self.source_fps;
        let fallback_target = world_samples.first().map(|(_, t)| *t).unwrap_or_default();
        let target_at = |frame: u32| -> Transform {
            match world_samples.binary_search_by_key(&frame, |(f, _)| *f) {
                Ok(i) => world_samples[i].1,
                Err(0) => fallback_target,
                Err(i) if i >= world_samples.len() => world_samples.last().unwrap().1,
                Err(i) => {
                    let (f0, t0) = world_samples[i - 1];
                    let (f1, t1) = world_samples[i];
                    let u = if f1 > f0 {
                        (frame - f0) as f64 / (f1 - f0) as f64
                    } else {
                        0.0
                    };
                    Transform {
                        x: t0.x + (t1.x - t0.x) * u,
                        y: t0.y + (t1.y - t0.y) * u,
                        scale_x: t0.scale_x + (t1.scale_x - t0.scale_x) * u,
                        scale_y: t0.scale_y + (t1.scale_y - t0.scale_y) * u,
                        rotation: t0.rotation + (t1.rotation - t0.rotation) * u,
                        skew: t0.skew + (t1.skew - t0.skew) * u,
                        ..t0
                    }
                }
            }
        };

        let mask_contours_at = |frame: u32| -> Vec<LottieBezier> {
            let inv_target = invert_affine(neoflash_core::transform_matrix(target_at(frame)));
            let mut out = Vec::new();
            for el in sample_layer_in_at_fps(timeline, mask_layer, frame, fps) {
                if !el.visible {
                    continue;
                }
                let Some((cmds, _)) = el.to_path_commands() else {
                    continue;
                };
                let mask_m = neoflash_core::transform_matrix(el.transform);
                let combined = match inv_target {
                    Some(inv) => neoflash_core::affine_multiply(inv, mask_m),
                    None => mask_m,
                };
                let local_cmds = transform_path_commands(&cmds, combined);
                out.extend(path_commands_to_lottie_beziers(&local_cmds));
            }
            out
        };

        let first_contours = mask_contours_at(first_frame);
        if first_contours.is_empty() {
            return true;
        }

        let target_moves = world_samples
            .iter()
            .any(|(_, t)| !transforms_approx_eq(*t, fallback_target));
        let mask_animated = last_frame > first_frame + 1
            && (target_moves
                || mask_layer.keyframes.len() > 1
                || mask_layer
                    .keyframes
                    .iter()
                    .any(|k| k.tween.is_some() || !k.property_expressions.is_empty()));
        let mask_invert = mask_layer.properties.mask_invert;
        let mask_mode_for_index = |idx: usize| -> (bool, &'static str) {
            if mask_invert {
                if idx == 0 { (true, "a") } else { (false, "s") }
            } else {
                (false, "a")
            }
        };
        let mut masks_props = Vec::new();
        if mask_animated {
            let per_frame: Vec<(u32, Vec<LottieBezier>)> = (first_frame..last_frame)
                .map(|f| (f, mask_contours_at(f)))
                .collect();
            let count = first_contours.len();
            let v_counts: Vec<usize> = first_contours.iter().map(|c| c.v.len()).collect();
            let compatible = per_frame.iter().all(|(_, cs)| {
                cs.len() == count
                    && cs
                        .iter()
                        .zip(&v_counts)
                        .all(|(c, &expected)| c.v.len() == expected)
            });
            if !compatible {
                return false;
            }
            for c_idx in 0..count {
                let (inv, mode) = mask_mode_for_index(c_idx);
                let kfs: Vec<Value> = per_frame
                    .iter()
                    .enumerate()
                    .map(|(idx, (f, cs))| {
                        let bez = cs[c_idx].to_json();
                        if idx + 1 < per_frame.len() {
                            json!({
                                "t": *f,
                                "s": [bez],
                                "i": {"x": [1.0], "y": [1.0]},
                                "o": {"x": [0.0], "y": [0.0]}
                            })
                        } else {
                            json!({"t": *f, "s": [bez]})
                        }
                    })
                    .collect();
                masks_props.push(json!({
                    "inv": inv,
                    "mode": mode,
                    "pt": {"a": 1, "k": kfs},
                    "o": {"a": 0, "k": 100},
                    "x": {"a": 0, "k": 0},
                    "nm": mask_layer.name
                }));
            }
        } else {
            for (c_idx, bez) in first_contours.into_iter().enumerate() {
                let (inv, mode) = mask_mode_for_index(c_idx);
                masks_props.push(json!({
                    "inv": inv,
                    "mode": mode,
                    "pt": {"a": 0, "k": bez.to_json()},
                    "o": {"a": 0, "k": 100},
                    "x": {"a": 0, "k": 0},
                    "nm": mask_layer.name
                }));
            }
        }
        if !masks_props.is_empty()
            && let Some(map) = layer_obj.as_object_mut()
        {
            map.insert("hasMask".into(), Value::Bool(true));
            map.insert("masksProperties".into(), Value::Array(masks_props));
        }
        true
    }
}

#[derive(Clone, Debug)]
struct NativeDashStroke {
    commands: Vec<PathCommand>,
    color: Color,
    gradient: Option<GradientPaint>,
    width: f64,
    dash: f64,
    gap: f64,
}

fn resolve_native_dash_stroke(
    layer: &Layer,
    frame: u32,
    element_id: &str,
) -> Option<NativeDashStroke> {
    let kf = layer.key_at(frame)?;
    let (base_id, entry_idx) = if kf.appearance(element_id).is_some() {
        (element_id, 0usize)
    } else if let Some((prefix, idx_str)) = element_id.rsplit_once("-app-")
        && let Ok(idx) = idx_str.parse::<usize>()
        && kf.appearance(prefix).is_some()
    {
        (prefix, idx)
    } else {
        return None;
    };
    let app = kf.appearance(base_id)?;
    if !app.modifiers.is_empty() || app.text_binding.is_some() {
        return None;
    }
    let entry = app.entries.get(entry_idx)?;
    let AppearancePaintEntry::Stroke {
        color,
        gradient,
        width,
        offset_distance,
        pattern_brush: Some(PatternBrushKind::DashPattern { dash, gap }),
        ..
    } = entry
    else {
        return None;
    };
    if *width <= 0.0 || *dash <= 0.0 || *gap < 0.0 || offset_distance.abs() > 1e-6 {
        return None;
    }
    let base_el = kf.elements.iter().find(|e| e.id == base_id)?;
    let (commands, _) = base_el.to_path_commands()?;
    Some(NativeDashStroke {
        commands,
        color: *color,
        gradient: gradient.clone(),
        width: *width,
        dash: *dash,
        gap: *gap,
    })
}

fn build_lottie_dash_array(dash: f64, gap: f64) -> Vec<Value> {
    vec![
        json!({"n": "d", "nm": "dash", "v": {"a": 0, "k": round4(dash.max(0.01))}}),
        json!({"n": "g", "nm": "gap", "v": {"a": 0, "k": round4(gap.max(0.0))}}),
        json!({"n": "o", "nm": "offset", "v": {"a": 0, "k": 0.0}}),
    ]
}

fn resolve_parametric_stroke_element(element: &Element) -> Option<Element> {
    if !element.stroke_width_profile().is_default()
        && let Some(kind) = element.parametric_path_kind()
    {
        let mut expanded = element.clone();
        expanded.kind = kind;
        expanded.transform.primitive = None;
        return Some(expanded);
    }
    None
}

fn build_button_up_timeline(timeline: &Timeline, fps: f64) -> Timeline {
    let mut layers = Vec::with_capacity(timeline.layers.len());
    for layer in &timeline.layers {
        let mut up_layer = layer.clone();
        let up_elements = sample_layer_in_at_fps(timeline, layer, 0, fps);
        let any_sound = layer.keyframes.iter().find_map(|k| k.sound.clone());
        let any_video = layer.keyframes.iter().find_map(|k| k.video.clone());
        let all_filters: Vec<_> = layer
            .keyframes
            .iter()
            .flat_map(|k| k.filters.iter().cloned())
            .collect();
        if up_elements.is_empty()
            && any_sound.is_none()
            && any_video.is_none()
            && all_filters.is_empty()
        {
            up_layer.keyframes = Vec::new();
        } else {
            up_layer.keyframes = vec![Keyframe {
                frame: 0,
                elements: up_elements,
                sound: any_sound,
                video: any_video,
                filters: all_filters,
                ..Keyframe::default()
            }];
        }
        up_layer.properties.end_frame = None;
        layers.push(up_layer);
    }
    Timeline {
        frame_count: 1,
        layers,
        scripts: timeline.scripts.clone(),
        guides: Vec::new(),
    }
}

fn has_asymmetric_pair_ease(eases: &[(TweenProperty, Easing)]) -> bool {
    let lookup = |prop: TweenProperty| eases.iter().find(|(p, _)| *p == prop).map(|(_, e)| *e);
    let x_ease = lookup(TweenProperty::X);
    let y_ease = lookup(TweenProperty::Y);
    if x_ease != y_ease && (x_ease.is_some() || y_ease.is_some()) {
        return true;
    }
    let sx_ease = lookup(TweenProperty::ScaleX);
    let sy_ease = lookup(TweenProperty::ScaleY);
    sx_ease != sy_ease && (sx_ease.is_some() || sy_ease.is_some())
}

fn has_non_alpha_color_transform(ct: ColorTransform) -> bool {
    (ct.multipliers[0] - 1.0).abs() > 1e-4
        || (ct.multipliers[1] - 1.0).abs() > 1e-4
        || (ct.multipliers[2] - 1.0).abs() > 1e-4
        || ct.offsets[0].abs() > 0.5
        || ct.offsets[1].abs() > 0.5
        || ct.offsets[2].abs() > 0.5
}

fn lottie_skew_deg(skew: f64) -> f64 {
    if !skew.is_finite() {
        return 0.0;
    }
    skew.clamp(-85.0, 85.0)
}

fn lottie_skew_cos(skew: f64) -> f64 {
    if skew.abs() <= 1e-6 || !skew.is_finite() {
        return 1.0;
    }
    skew.clamp(-85.0, 85.0).to_radians().cos()
}

fn classic_span_rotation_delta(kf: &Keyframe, a_rot: f64, b_rot: f64) -> (f64, bool) {
    let turns = (kf.classic_rotate_turns.min(Keyframe::MAX_ROTATE_TURNS) as f64) * 360.0;
    match kf.classic_rotate_dir {
        ClassicRotateDirection::Auto => {
            if turns == 0.0 {
                (b_rot - a_rot, false)
            } else {
                let delta = b_rot - a_rot;
                let sign = if delta < 0.0 { -1.0 } else { 1.0 };
                (delta + sign * turns, false)
            }
        }
        ClassicRotateDirection::None => (b_rot - a_rot, true),
        ClassicRotateDirection::Cw => {
            let cw_delta = (b_rot - a_rot).rem_euclid(360.0) + turns;
            (cw_delta, false)
        }
        ClassicRotateDirection::Ccw => {
            let ccw_delta = (a_rot - b_rot).rem_euclid(360.0) + turns;
            (-ccw_delta, false)
        }
    }
}

fn compute_lottie_auto_orient_deg(key_frames: &[(u32, Transform, Option<Easing>)]) -> Vec<f64> {
    let n = key_frames.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let chord_deg = |a: usize, b: usize| -> Option<f64> {
        let dx = key_frames[b].1.x - key_frames[a].1.x;
        let dy = key_frames[b].1.y - key_frames[a].1.y;
        (dx.hypot(dy) > 1e-6).then(|| dy.atan2(dx).to_degrees())
    };
    let mut raw = vec![0.0; n];
    for (idx, slot) in raw.iter_mut().enumerate() {
        if let Some(deg) = (idx + 1 < n).then(|| chord_deg(idx, idx + 1)).flatten() {
            *slot = deg;
        } else if let Some(deg) = (idx > 0).then(|| chord_deg(idx - 1, idx)).flatten() {
            *slot = deg;
        } else {
            for dist in 1..n {
                if idx + dist < n
                    && let Some(deg) = chord_deg(idx, idx + dist)
                {
                    *slot = deg;
                    break;
                }
                if idx >= dist
                    && let Some(deg) = chord_deg(idx - dist, idx)
                {
                    *slot = deg;
                    break;
                }
            }
        }
    }
    raw
}

fn compute_unwrapped_rotations(
    layer: &Layer,
    keys: &[(u32, Transform, Option<Easing>)],
    dense: bool,
) -> Vec<(f64, bool)> {
    if keys.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(keys.len());
    if !dense {
        let mut turn_offset = 0.0f64;
        for (idx, (f, t, _)) in keys.iter().enumerate() {
            let curr_unwrapped = t.rotation + turn_offset;
            let mut hold = false;
            if idx + 1 < keys.len() {
                let next_f = keys[idx + 1].0;
                let next_raw_rot = keys[idx + 1].1.rotation;
                if motion_span(layer, *f).is_none()
                    && let Some(kf) = layer.key_at(*f)
                    && kf.frame == *f
                    && layer.keyframes.iter().any(|nk| nk.frame == next_f)
                {
                    if kf.tween.is_some() && kf.tween_kind == TweenKind::Classic {
                        let (delta, is_hold) =
                            classic_span_rotation_delta(kf, t.rotation, next_raw_rot);
                        hold = is_hold;
                        if !is_hold {
                            let target_end = curr_unwrapped + delta;
                            turn_offset = target_end - next_raw_rot;
                        }
                    } else if kf.tween.is_none() {
                        hold = true;
                    }
                }
            }
            out.push((curr_unwrapped, hold));
        }
    } else {
        let mut prev_unwrapped = keys[0].1.rotation;
        let first_hold = motion_span(layer, keys[0].0).is_none()
            && layer.key_at(keys[0].0).is_some_and(|k| {
                k.tween.is_none()
                    || (k.tween_kind == TweenKind::Classic
                        && k.classic_rotate_dir == ClassicRotateDirection::None)
            });
        out.push((prev_unwrapped, first_hold));
        for idx in 1..keys.len() {
            let (curr_f, curr_t, _) = keys[idx];
            let prev_f = keys[idx - 1].0;
            let prev_hold = out[idx - 1].1;
            let curr_unwrapped = if prev_hold {
                curr_t.rotation
            } else if motion_span(layer, prev_f).is_none()
                && layer.keyframes.iter().any(|k| k.frame == curr_f)
                && let Some(start_kf) = layer.key_at(prev_f)
                && start_kf.tween.is_some()
                && start_kf.tween_kind == TweenKind::Classic
            {
                // Exact end keyframe of a classic spin span: unwrap nearest to `prev_unwrapped`
                // while respecting multi-turn direction if step >= 180 deg.
                let k = ((prev_unwrapped - curr_t.rotation) / 360.0).round();
                curr_t.rotation + k * 360.0
            } else {
                let k = ((prev_unwrapped - curr_t.rotation) / 360.0).round();
                curr_t.rotation + k * 360.0
            };
            let hold = motion_span(layer, curr_f).is_none()
                && layer.key_at(curr_f).is_some_and(|k| {
                    k.tween.is_none()
                        || (k.tween_kind == TweenKind::Classic
                            && k.classic_rotate_dir == ClassicRotateDirection::None)
                });
            prev_unwrapped = curr_unwrapped;
            out.push((curr_unwrapped, hold));
        }
    }
    out
}

#[derive(Clone, Debug)]
struct LayerSpan {
    start_frame: u32,
    end_frame: u32,
}

fn collect_layer_spans(layer: &Layer, layer_end: u32) -> Vec<LayerSpan> {
    let mut spans = Vec::new();
    let mut current_start: Option<u32> = None;
    for f in 0..layer_end {
        let non_empty = layer.key_at(f).is_some_and(|k| !k.elements.is_empty());
        match (current_start, non_empty) {
            (None, true) => current_start = Some(f),
            (Some(start), false) => {
                spans.push(LayerSpan {
                    start_frame: start,
                    end_frame: f,
                });
                current_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = current_start {
        spans.push(LayerSpan {
            start_frame: start,
            end_frame: layer_end,
        });
    }
    spans
}

fn sample_shape_tween_frame(
    timeline: &Timeline,
    layer: &Layer,
    frame: u32,
    fps: f64,
) -> Vec<Element> {
    fn has_post_shape_evaluation(key: &Keyframe) -> bool {
        !key.property_expressions.is_empty()
            || !key.ik_armatures.is_empty()
            || !key.element_parents.is_empty()
            || !key.asset_warps.is_empty()
            || !key.vector_effects.is_empty()
            || !key.appearances.is_empty()
    }

    // Plain endpoint keys keep the shape tween's resampled topology. That lets Lottie encode a
    // single animated path even when the authored endpoint primitives have different command
    // counts. Advanced keys must go through the shared evaluator so expressions, rigs, effects,
    // and appearances are not silently dropped at exact keyframes.
    if let Some((idx, key)) = layer
        .keyframes
        .iter()
        .enumerate()
        .find(|(_, key)| key.frame == frame)
    {
        if key.tween.is_some()
            && key.tween_kind == TweenKind::Shape
            && let Some(next) = layer.keyframes.get(idx + 1)
            && !has_post_shape_evaluation(key)
            && !has_post_shape_evaluation(next)
        {
            return neoflash_core::shape_tween_elements_with_hints(
                &key.elements,
                &next.elements,
                &key.shape_hints,
                0.0,
            );
        }
        if idx > 0 {
            let previous = &layer.keyframes[idx - 1];
            if previous.tween.is_some()
                && previous.tween_kind == TweenKind::Shape
                && !has_post_shape_evaluation(previous)
                && !has_post_shape_evaluation(key)
            {
                return neoflash_core::shape_tween_elements_with_hints(
                    &previous.elements,
                    &key.elements,
                    &previous.shape_hints,
                    1.0,
                );
            }
        }
    }
    sample_layer_in_at_fps(timeline, layer, frame, fps)
}

fn collect_transform_keyframes(
    timeline: &Timeline,
    layer: &Layer,
    samples: &[(u32, Element)],
    element_id: &str,
) -> Vec<(u32, Transform, Option<Easing>)> {
    let first_f = samples.first().unwrap().0;
    let last_f = samples.last().unwrap().0;
    let sample_map: BTreeMap<u32, Transform> =
        samples.iter().map(|(f, e)| (*f, e.transform)).collect();

    let mut key_times: BTreeSet<u32> = BTreeSet::new();
    key_times.insert(first_f);
    key_times.insert(last_f);
    for kf in &layer.keyframes {
        let relevant =
            kf.keyed_properties.is_none() || !kf.motion_key_properties_for(element_id).is_empty();
        if relevant
            && kf.frame >= first_f
            && kf.frame <= last_f
            && sample_map.contains_key(&kf.frame)
        {
            key_times.insert(kf.frame);
            // If a hold keyframe transitions at `next_kf.frame`, include `next_kf.frame - 1` or
            // mark the keyframe as hold (`None`).
        }
    }
    let _ = timeline;

    let times: Vec<u32> = key_times.into_iter().collect();
    let mut out = Vec::with_capacity(times.len());
    for &f in &times {
        let t = sample_map[&f];
        let ease = layer_ease_at(layer, f, element_id);
        out.push((f, t, ease));
    }
    out
}

fn layer_ease_at(layer: &Layer, frame: u32, element_id: &str) -> Option<Easing> {
    if let Some((start, _, _)) = motion_span(layer, frame) {
        return Some(start.motion_easing_for(element_id));
    }
    layer.key_at(frame).and_then(|k| k.tween)
}

fn ease_for_prop(
    layer: &Layer,
    frame: u32,
    element_id: &str,
    prop: TweenProperty,
) -> Option<Easing> {
    if let Some((start, _, _)) = motion_span(layer, frame) {
        return Some(start.motion_property_ease_for(element_id, prop));
    }
    layer.key_at(frame).and_then(|k| k.tween)
}

fn easing_to_lottie_tangents(ease: Easing) -> ([f64; 2], [f64; 2]) {
    match ease {
        Easing::Linear => ([0.0, 0.0], [1.0, 1.0]),
        Easing::EaseIn => ([0.3333, 0.0], [0.6667, 0.3333]),
        Easing::EaseOut => ([0.3333, 0.6667], [0.6667, 1.0]),
        Easing::EaseInOut => ([0.42, 0.0], [0.58, 1.0]),
        Easing::CubicBezier { x1, y1, x2, y2 } => (
            [round4(x1.clamp(0.0, 1.0)), round4(y1)],
            [round4(x2.clamp(0.0, 1.0)), round4(y2)],
        ),
        Easing::Flash { acceleration } => {
            let a = (acceleration / 100.0).clamp(-1.0, 1.0);
            if a < 0.0 {
                let w = -a;
                (
                    [0.3333, round4(0.3333 * (1.0 - w))],
                    [0.6667, round4(0.6667 * (1.0 - w) + 0.3333 * w)],
                )
            } else {
                let w = a;
                (
                    [0.3333, round4(0.3333 * (1.0 - w) + 0.6667 * w)],
                    [0.6667, round4(0.6667 * (1.0 - w) + 1.0 * w)],
                )
            }
        }
        Easing::Bounce | Easing::Elastic => ([0.0, 0.0], [1.0, 1.0]),
    }
}

fn build_vec3_keyframes(
    keys: &[(u32, Transform, Option<Easing>)],
    extract: impl Fn(&(u32, Transform, Option<Easing>), u32) -> ([f64; 3], Option<Easing>),
) -> Value {
    let first = extract(&keys[0], keys[0].0).0;
    if keys.iter().all(|k| extract(k, k.0).0 == first) {
        return json!({"a": 0, "k": first});
    }
    let mut arr = Vec::with_capacity(keys.len());
    for (idx, k) in keys.iter().enumerate() {
        let (val, ease) = extract(k, k.0);
        if idx + 1 < keys.len() {
            if let Some(e) = ease {
                let (o, i) = easing_to_lottie_tangents(e);
                arr.push(json!({
                    "t": k.0,
                    "s": val,
                    "o": {"x": [o[0]], "y": [o[1]]},
                    "i": {"x": [i[0]], "y": [i[1]]}
                }));
            } else {
                arr.push(json!({
                    "t": k.0,
                    "s": val,
                    "h": 1
                }));
            }
        } else {
            arr.push(json!({
                "t": k.0,
                "s": val
            }));
        }
    }
    json!({"a": 1, "k": arr})
}

fn build_scalar_keyframes(
    keys: &[(u32, Transform, Option<Easing>)],
    extract: impl Fn(&(u32, Transform, Option<Easing>), u32) -> (f64, Option<Easing>),
) -> Value {
    let first = extract(&keys[0], keys[0].0).0;
    if keys
        .iter()
        .all(|k| (extract(k, k.0).0 - first).abs() < 1e-4)
    {
        return json!({"a": 0, "k": first});
    }
    let mut arr = Vec::with_capacity(keys.len());
    for (idx, k) in keys.iter().enumerate() {
        let (val, ease) = extract(k, k.0);
        if idx + 1 < keys.len() {
            if let Some(e) = ease {
                let (o, i) = easing_to_lottie_tangents(e);
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "o": {"x": [o[0]], "y": [o[1]]},
                    "i": {"x": [i[0]], "y": [i[1]]}
                }));
            } else {
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "h": 1
                }));
            }
        } else {
            arr.push(json!({
                "t": k.0,
                "s": [val]
            }));
        }
    }
    json!({"a": 1, "k": arr})
}

fn build_indexed_scalar_keyframes(
    keys: &[(u32, Transform, Option<Easing>)],
    extract: impl Fn(usize, &(u32, Transform, Option<Easing>), u32) -> (f64, Option<Easing>),
) -> Value {
    let first = extract(0, &keys[0], keys[0].0).0;
    if keys
        .iter()
        .enumerate()
        .all(|(idx, k)| (extract(idx, k, k.0).0 - first).abs() < 1e-4)
    {
        return json!({"a": 0, "k": first});
    }
    let mut arr = Vec::with_capacity(keys.len());
    for (idx, k) in keys.iter().enumerate() {
        let (val, ease) = extract(idx, k, k.0);
        if idx + 1 < keys.len() {
            if let Some(e) = ease {
                let (o, i) = easing_to_lottie_tangents(e);
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "o": {"x": [o[0]], "y": [o[1]]},
                    "i": {"x": [i[0]], "y": [i[1]]}
                }));
            } else {
                arr.push(json!({
                    "t": k.0,
                    "s": [val],
                    "h": 1
                }));
            }
        } else {
            arr.push(json!({
                "t": k.0,
                "s": [val]
            }));
        }
    }
    json!({"a": 1, "k": arr})
}

fn static_ks(t: Transform, anchor: [f64; 2], origin_offset: [f64; 2]) -> Value {
    let sk_cos = lottie_skew_cos(t.skew);
    let mut ks = Map::new();
    ks.insert(
        "o".into(),
        json!({"a": 0, "k": round4(effective_opacity_percent(t))}),
    );
    ks.insert("r".into(), json!({"a": 0, "k": round4(t.rotation)}));
    ks.insert(
        "p".into(),
        json!({"a": 0, "k": [round4(t.x + origin_offset[0]), round4(t.y + origin_offset[1]), 0.0]}),
    );
    ks.insert(
        "a".into(),
        json!({"a": 0, "k": [round4(anchor[0]), round4(anchor[1]), 0.0]}),
    );
    ks.insert(
        "s".into(),
        json!({"a": 0, "k": [round4(t.scale_x * 100.0), round4(t.scale_y * sk_cos * 100.0), 100.0]}),
    );
    if t.skew.abs() > 1e-4 {
        ks.insert(
            "sk".into(),
            json!({"a": 0, "k": round4(lottie_skew_deg(t.skew))}),
        );
        ks.insert("sa".into(), json!({"a": 0, "k": 0.0}));
    }
    Value::Object(ks)
}

fn identity_group_tr() -> Value {
    json!({
        "ty": "tr",
        "p": {"a": 0, "k": [0, 0]},
        "a": {"a": 0, "k": [0, 0]},
        "s": {"a": 0, "k": [100, 100]},
        "r": {"a": 0, "k": 0},
        "o": {"a": 0, "k": 100},
        "sk": {"a": 0, "k": 0},
        "sa": {"a": 0, "k": 0},
        "nm": "Transform"
    })
}

fn effective_opacity_percent(t: Transform) -> f64 {
    let ct_alpha =
        (t.color_transform.multipliers[3] + t.color_transform.offsets[3] / 255.0).clamp(0.0, 1.0);
    (t.alpha.clamp(0.0, 1.0) * ct_alpha * 100.0).clamp(0.0, 100.0)
}

fn apply_rgb_only(ct: ColorTransform, color: Color, include_alpha: bool) -> Color {
    let mut out = ct.apply(color);
    if !include_alpha {
        out[3] = color[3];
    }
    out
}

fn extract_element_paint(element: &Element) -> (Color, Color, f64) {
    match &element.kind {
        ElementKind::Rectangle {
            fill,
            stroke,
            stroke_width,
            ..
        } => (*fill, *stroke, *stroke_width),
        ElementKind::Ellipse {
            fill,
            stroke,
            stroke_width,
            ..
        } => {
            let fill = if element.oval_options().close_path {
                *fill
            } else {
                [0, 0, 0, 0]
            };
            (fill, *stroke, *stroke_width)
        }
        ElementKind::Path {
            fill,
            stroke,
            stroke_width,
            ..
        }
        | ElementKind::PaintedPath {
            fill,
            stroke,
            stroke_width,
            ..
        } => (*fill, *stroke, *stroke_width),
        _ => ([0; 4], [0; 4], 0.0),
    }
}

fn tint_gradient_paint(
    grad: &GradientPaint,
    ct: ColorTransform,
    include_alpha: bool,
) -> GradientPaint {
    let mut out = grad.clone();
    for stop in &mut out.stops {
        stop.color = apply_rgb_only(ct, stop.color, include_alpha);
    }
    out
}

fn transform_gradient_paint(
    grad: &GradientPaint,
    m: [f64; 6],
    ct: ColorTransform,
    include_alpha: bool,
) -> GradientPaint {
    let mut out = tint_gradient_paint(grad, ct, include_alpha);
    out.matrix = neoflash_core::affine_multiply(m, grad.matrix);
    out
}

fn solid_as_matching_gradient(color: Color, template: &GradientPaint) -> GradientPaint {
    let mut out = template.clone();
    for stop in &mut out.stops {
        stop.color = color;
    }
    out
}

fn gradient_endpoints(grad: &GradientPaint) -> ([f64; 2], [f64; 2], u8) {
    let [a, b, _c, _d, tx, ty] = grad.matrix;
    match grad.kind {
        GradientKind::Linear => (
            [round4(tx - 819.2 * a), round4(ty - 819.2 * b)],
            [round4(tx + 819.2 * a), round4(ty + 819.2 * b)],
            1,
        ),
        GradientKind::Radial => (
            [round4(tx), round4(ty)],
            [round4(tx + 819.2 * a), round4(ty + 819.2 * b)],
            2,
        ),
    }
}

fn gradient_radial_highlight(grad: &GradientPaint) -> Option<(f64, f64)> {
    if grad.kind != GradientKind::Radial
        || !grad.focal_point_ratio.is_finite()
        || grad.focal_point_ratio.abs() <= 1e-4
    {
        return None;
    }
    let r = grad.focal_point_ratio.clamp(-0.999, 0.999);
    let h = round4(r.abs() * 100.0);
    let a = if r < 0.0 { 180.0 } else { 0.0 };
    Some((h, a))
}

fn normalized_lottie_stops(
    grad: &GradientPaint,
) -> std::borrow::Cow<'_, [neoflash_core::GradientStop]> {
    if grad.stops.is_empty() {
        std::borrow::Cow::Owned(vec![
            neoflash_core::GradientStop {
                ratio: 0,
                color: [0, 0, 0, 0],
            },
            neoflash_core::GradientStop {
                ratio: 255,
                color: [0, 0, 0, 0],
            },
        ])
    } else if grad.stops.len() == 1 {
        let c = grad.stops[0].color;
        std::borrow::Cow::Owned(vec![
            neoflash_core::GradientStop { ratio: 0, color: c },
            neoflash_core::GradientStop {
                ratio: 255,
                color: c,
            },
        ])
    } else {
        std::borrow::Cow::Borrowed(&grad.stops)
    }
}

fn gradient_stops_array_with_opacity(grad: &GradientPaint, include_opacity: bool) -> Vec<f64> {
    let stops = normalized_lottie_stops(grad);
    let mut arr = Vec::with_capacity(stops.len() * 6);
    for stop in stops.iter() {
        arr.push(round4(f64::from(stop.ratio) / 255.0));
        arr.push(round4(f64::from(stop.color[0]) / 255.0));
        arr.push(round4(f64::from(stop.color[1]) / 255.0));
        arr.push(round4(f64::from(stop.color[2]) / 255.0));
    }
    if include_opacity {
        for stop in stops.iter() {
            arr.push(round4(f64::from(stop.ratio) / 255.0));
            arr.push(round4(f64::from(stop.color[3]) / 255.0));
        }
    }
    arr
}

fn gradient_stops_array(grad: &GradientPaint) -> Vec<f64> {
    let stops = normalized_lottie_stops(grad);
    let has_alpha = stops.iter().any(|s| s.color[3] < 255);
    gradient_stops_array_with_opacity(grad, has_alpha)
}

fn gradient_fill_item(grad: &GradientPaint, fill_rule: FillRule) -> Value {
    let (s, e, t) = gradient_endpoints(grad);
    let p_count = normalized_lottie_stops(grad).len();
    let k = gradient_stops_array(grad);
    let mut obj = json!({
        "ty": "gf",
        "nm": "Gradient Fill",
        "o": {"a": 0, "k": 100},
        "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
        "bm": 0,
        "g": {
            "p": p_count,
            "k": {"a": 0, "k": k}
        },
        "s": {"a": 0, "k": s},
        "e": {"a": 0, "k": e},
        "t": t
    });
    if let Some((h, a)) = gradient_radial_highlight(grad)
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), json!({"a": 0, "k": h}));
        map.insert("a".into(), json!({"a": 0, "k": a}));
    }
    obj
}

fn gradient_stroke_item(grad: &GradientPaint, stroke_width: f64) -> Value {
    let (s, e, t) = gradient_endpoints(grad);
    let p_count = normalized_lottie_stops(grad).len();
    let k = gradient_stops_array(grad);
    let mut obj = json!({
        "ty": "gs",
        "nm": "Gradient Stroke",
        "o": {"a": 0, "k": 100},
        "w": {"a": 0, "k": round4(stroke_width)},
        "lc": 2,
        "lj": 2,
        "ml": 4,
        "bm": 0,
        "g": {
            "p": p_count,
            "k": {"a": 0, "k": k}
        },
        "s": {"a": 0, "k": s},
        "e": {"a": 0, "k": e},
        "t": t
    });
    if let Some((h, a)) = gradient_radial_highlight(grad)
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), json!({"a": 0, "k": h}));
        map.insert("a".into(), json!({"a": 0, "k": a}));
    }
    obj
}

fn build_linear_vec_prop<const N: usize>(samples: &[(u32, [f64; N])]) -> Value {
    let first = samples[0].1;
    if samples.iter().all(|(_, v)| *v == first) {
        return json!({"a": 0, "k": first.as_slice()});
    }
    let kfs: Vec<Value> = samples
        .iter()
        .enumerate()
        .map(|(idx, (f, v))| {
            if idx + 1 < samples.len() {
                json!({"t": *f, "s": v.as_slice(), "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
            } else {
                json!({"t": *f, "s": v.as_slice()})
            }
        })
        .collect();
    json!({"a": 1, "k": kfs})
}

fn build_linear_f64_slice_prop(samples: &[(u32, Vec<f64>)]) -> Value {
    let first = &samples[0].1;
    if samples.iter().all(|(_, v)| v == first) {
        return json!({"a": 0, "k": first});
    }
    let kfs: Vec<Value> = samples
        .iter()
        .enumerate()
        .map(|(idx, (f, v))| {
            if idx + 1 < samples.len() {
                json!({"t": *f, "s": v, "i": {"x": [1.0], "y": [1.0]}, "o": {"x": [0.0], "y": [0.0]}})
            } else {
                json!({"t": *f, "s": v})
            }
        })
        .collect();
    json!({"a": 1, "k": kfs})
}

fn animated_gradient_common(
    grads: &[(u32, GradientPaint)],
) -> (usize, u8, Value, Value, Value, Option<(Value, Value)>) {
    let p_count = normalized_lottie_stops(&grads[0].1).len();
    let (_, _, t_code) = gradient_endpoints(&grads[0].1);
    let include_opacity = grads
        .iter()
        .any(|(_, g)| normalized_lottie_stops(g).iter().any(|s| s.color[3] < 255));
    let k_samples: Vec<(u32, Vec<f64>)> = grads
        .iter()
        .map(|(f, g)| (*f, gradient_stops_array_with_opacity(g, include_opacity)))
        .collect();
    let s_samples: Vec<(u32, [f64; 2])> = grads
        .iter()
        .map(|(f, g)| (*f, gradient_endpoints(g).0))
        .collect();
    let e_samples: Vec<(u32, [f64; 2])> = grads
        .iter()
        .map(|(f, g)| (*f, gradient_endpoints(g).1))
        .collect();
    let highlight = if t_code == 2
        && grads
            .iter()
            .any(|(_, g)| g.focal_point_ratio.is_finite() && g.focal_point_ratio.abs() > 1e-4)
    {
        let h_samples: Vec<(u32, [f64; 1])> = grads
            .iter()
            .map(|(f, g)| {
                let r = if g.focal_point_ratio.is_finite() {
                    g.focal_point_ratio.clamp(-0.999, 0.999)
                } else {
                    0.0
                };
                (*f, [round4(r.abs() * 100.0)])
            })
            .collect();
        let a_samples: Vec<(u32, [f64; 1])> = grads
            .iter()
            .map(|(f, g)| {
                let neg = g.focal_point_ratio.is_finite() && g.focal_point_ratio < -1e-4;
                (*f, [if neg { 180.0 } else { 0.0 }])
            })
            .collect();
        let h_val = if h_samples.iter().all(|(_, v)| v[0] == h_samples[0].1[0]) {
            json!({"a": 0, "k": h_samples[0].1[0]})
        } else {
            build_linear_vec_prop(&h_samples)
        };
        let a_val = if a_samples.iter().all(|(_, v)| v[0] == a_samples[0].1[0]) {
            json!({"a": 0, "k": a_samples[0].1[0]})
        } else {
            build_linear_vec_prop(&a_samples)
        };
        Some((h_val, a_val))
    } else {
        None
    };
    (
        p_count,
        t_code,
        build_linear_f64_slice_prop(&k_samples),
        build_linear_vec_prop(&s_samples),
        build_linear_vec_prop(&e_samples),
        highlight,
    )
}

fn animated_gradient_fill_item(grads: &[(u32, GradientPaint)], fill_rule: FillRule) -> Value {
    let (p_count, t_code, k_val, s_val, e_val, highlight) = animated_gradient_common(grads);
    let mut obj = json!({
        "ty": "gf",
        "nm": "Gradient Fill",
        "o": {"a": 0, "k": 100},
        "r": if fill_rule == FillRule::EvenOdd { 2 } else { 1 },
        "bm": 0,
        "g": {
            "p": p_count,
            "k": k_val
        },
        "s": s_val,
        "e": e_val,
        "t": t_code
    });
    if let Some((h_val, a_val)) = highlight
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), h_val);
        map.insert("a".into(), a_val);
    }
    obj
}

fn animated_gradient_stroke_item(grads: &[(u32, GradientPaint)], widths: &[(u32, f64)]) -> Value {
    let (p_count, t_code, k_val, s_val, e_val, highlight) = animated_gradient_common(grads);
    let w_samples: Vec<(u32, [f64; 1])> = widths.iter().map(|(f, w)| (*f, [round4(*w)])).collect();
    let w_val = if w_samples
        .iter()
        .all(|(_, v)| (v[0] - w_samples[0].1[0]).abs() < 1e-4)
    {
        json!({"a": 0, "k": w_samples[0].1[0]})
    } else {
        build_linear_vec_prop(&w_samples)
    };
    let mut obj = json!({
        "ty": "gs",
        "nm": "Gradient Stroke",
        "o": {"a": 0, "k": 100},
        "w": w_val,
        "lc": 2,
        "lj": 2,
        "ml": 4,
        "bm": 0,
        "g": {
            "p": p_count,
            "k": k_val
        },
        "s": s_val,
        "e": e_val,
        "t": t_code
    });
    if let Some((h_val, a_val)) = highlight
        && let Some(map) = obj.as_object_mut()
    {
        map.insert("h".into(), h_val);
        map.insert("a".into(), a_val);
    }
    obj
}

#[derive(Clone, Debug)]
struct LottieBezier {
    c: bool,
    v: Vec<[f64; 2]>,
    i: Vec<[f64; 2]>,
    o: Vec<[f64; 2]>,
}

impl LottieBezier {
    fn to_json(&self) -> Value {
        json!({
            "c": self.c,
            "v": self.v.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>(),
            "i": self.i.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>(),
            "o": self.o.iter().map(|p| [round4(p[0]), round4(p[1])]).collect::<Vec<_>>()
        })
    }

    /// Screen-space (Y-down) contour winding direction for Bodymovin `"ty": "sh"` (`1` = CW, `3` = CCW).
    fn lottie_direction(&self) -> u8 {
        if !self.c || self.v.len() < 2 {
            return 1;
        }
        let n = self.v.len();
        let mut pts = Vec::with_capacity(n * 4);
        for idx in 0..n {
            let next = (idx + 1) % n;
            let p0 = self.v[idx];
            let p1 = [p0[0] + self.o[idx][0], p0[1] + self.o[idx][1]];
            let p3 = self.v[next];
            let p2 = [p3[0] + self.i[next][0], p3[1] + self.i[next][1]];
            for step in 0..4 {
                let u = f64::from(step) / 4.0;
                let mu = 1.0 - u;
                let x = mu * mu * mu * p0[0]
                    + 3.0 * mu * mu * u * p1[0]
                    + 3.0 * mu * u * u * p2[0]
                    + u * u * u * p3[0];
                let y = mu * mu * mu * p0[1]
                    + 3.0 * mu * mu * u * p1[1]
                    + 3.0 * mu * u * u * p2[1]
                    + u * u * u * p3[1];
                pts.push([x, y]);
            }
        }
        let mut area2 = 0.0;
        for idx in 0..pts.len() {
            let a = pts[idx];
            let b = pts[(idx + 1) % pts.len()];
            area2 += a[0] * b[1] - b[0] * a[1];
        }
        if area2 < -1e-6 { 3 } else { 1 }
    }
}

fn path_commands_to_lottie_beziers(commands: &[PathCommand]) -> Vec<LottieBezier> {
    let mut contours = Vec::new();
    let mut current: Option<LottieBezier> = None;

    let flush = |current: &mut Option<LottieBezier>, contours: &mut Vec<LottieBezier>| {
        if let Some(mut bez) = current.take() {
            if bez.c && bez.v.len() > 1 {
                let first = bez.v[0];
                let last = *bez.v.last().unwrap();
                if (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-4 {
                    let last_in = bez.i.pop().unwrap_or([0.0, 0.0]);
                    bez.v.pop();
                    bez.o.pop();
                    bez.i[0] = last_in;
                }
            }
            if bez.v.len() >= 2 {
                contours.push(bez);
            }
        }
    };

    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                flush(&mut current, &mut contours);
                current = Some(LottieBezier {
                    c: false,
                    v: vec![[x, y]],
                    i: vec![[0.0, 0.0]],
                    o: vec![[0.0, 0.0]],
                });
            }
            PathCommand::LineTo { x, y } => {
                if let Some(bez) = current.as_mut() {
                    bez.v.push([x, y]);
                    bez.i.push([0.0, 0.0]);
                    bez.o.push([0.0, 0.0]);
                }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                if let Some(bez) = current.as_mut()
                    && let Some(&[x0, y0]) = bez.v.last()
                {
                    // Exact degree elevation from quadratic to cubic Bézier:
                    // cp1 = p0 + 2/3 * (q - p0), cp2 = p2 + 2/3 * (q - p2)
                    if let Some(last_o) = bez.o.last_mut() {
                        *last_o = [(2.0 / 3.0) * (cx - x0), (2.0 / 3.0) * (cy - y0)];
                    }
                    let in_tan = [(2.0 / 3.0) * (cx - x), (2.0 / 3.0) * (cy - y)];
                    bez.v.push([x, y]);
                    bez.i.push(in_tan);
                    bez.o.push([0.0, 0.0]);
                }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                if let Some(bez) = current.as_mut()
                    && let Some(&[x0, y0]) = bez.v.last()
                {
                    if let Some(last_o) = bez.o.last_mut() {
                        *last_o = [cx1 - x0, cy1 - y0];
                    }
                    let in_tan = [cx2 - x, cy2 - y];
                    bez.v.push([x, y]);
                    bez.i.push(in_tan);
                    bez.o.push([0.0, 0.0]);
                }
            }
            PathCommand::Close => {
                if let Some(bez) = current.as_mut() {
                    bez.c = true;
                }
                flush(&mut current, &mut contours);
            }
        }
    }
    flush(&mut current, &mut contours);
    contours
}

fn transform_path_commands(commands: &[PathCommand], m: [f64; 6]) -> Vec<PathCommand> {
    let pt =
        |x: f64, y: f64| -> (f64, f64) { (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) };
    commands
        .iter()
        .map(|cmd| match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (x, y) = pt(x, y);
                PathCommand::MoveTo { x, y }
            }
            PathCommand::LineTo { x, y } => {
                let (x, y) = pt(x, y);
                PathCommand::LineTo { x, y }
            }
            PathCommand::QuadraticTo { cx, cy, x, y } => {
                let (cx, cy) = pt(cx, cy);
                let (x, y) = pt(x, y);
                PathCommand::QuadraticTo { cx, cy, x, y }
            }
            PathCommand::CubicTo {
                cx1,
                cy1,
                cx2,
                cy2,
                x,
                y,
            } => {
                let (cx1, cy1) = pt(cx1, cy1);
                let (cx2, cy2) = pt(cx2, cy2);
                let (x, y) = pt(x, y);
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                }
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn invert_affine(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m[3] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[2] * inv_det;
    let d = m[0] * inv_det;
    let tx = -(a * m[4] + c * m[5]);
    let ty = -(b * m[4] + d * m[5]);
    Some([a, b, c, d, tx, ty])
}

fn transforms_approx_eq(a: Transform, b: Transform) -> bool {
    (a.x - b.x).abs() < 1e-4
        && (a.y - b.y).abs() < 1e-4
        && (a.scale_x - b.scale_x).abs() < 1e-4
        && (a.scale_y - b.scale_y).abs() < 1e-4
        && (a.rotation - b.rotation).abs() < 1e-4
        && (a.alpha - b.alpha).abs() < 1e-4
        && (a.skew - b.skew).abs() < 1e-4
        && a.color_transform == b.color_transform
}

fn layer_element_name(layer: &Layer, el: &Element) -> String {
    if el.name.is_empty() {
        layer.name.clone()
    } else {
        format!("{} · {}", layer.name, el.name)
    }
}

fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn round4(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 10_000.0).round() / 10_000.0
}

fn resolve_target_symbol<'a>(project: &'a Project, query: &str) -> Option<&'a Symbol> {
    project
        .symbol(query)
        .or_else(|| project.symbols.iter().find(|s| s.name == query))
}

fn build_markers(timeline: &Timeline, scene_ranges: &[neoflash_core::SceneRange]) -> Vec<Value> {
    let mut markers = Vec::new();
    if scene_ranges.len() > 1 {
        for scene in scene_ranges {
            markers.push(json!({
                "tm": scene.start,
                "dr": scene.frames,
                "cm": scene.name
            }));
        }
    }
    for layer in &timeline.layers {
        for (idx, kf) in layer.keyframes.iter().enumerate() {
            if !kf.label.trim().is_empty() {
                let next_f = layer
                    .keyframes
                    .get(idx + 1)
                    .map(|n| n.frame)
                    .unwrap_or(timeline.frame_count);
                markers.push(json!({
                    "tm": kf.frame,
                    "dr": next_f.saturating_sub(kf.frame),
                    "cm": kf.label
                }));
            }
        }
    }
    markers
}

/// Build a Lottie (`v: "5.7.14"`) JSON [`Value`] and [`LottieExportReport`] from `project`.
pub fn export_lottie(
    project: &Project,
    options: &LottieOptions,
) -> Result<(Value, LottieExportReport), ExportError> {
    export_lottie_value(project, options)
}

/// Build a Lottie (`v: "5.7.14"`) JSON [`Value`] and [`LottieExportReport`] from `project`.
pub fn export_lottie_value(
    project: &Project,
    options: &LottieOptions,
) -> Result<(Value, LottieExportReport), ExportError> {
    export_lottie_with_progress(project, options, |_| true)
}

/// Build a Lottie (`v: "5.7.14"`) JSON [`Value`] and [`LottieExportReport`] from `project`,
/// reporting progress and supporting cancellation (`on_progress` returning `false`).
pub fn export_lottie_with_progress<F>(
    project: &Project,
    options: &LottieOptions,
    on_progress: F,
) -> Result<(Value, LottieExportReport), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let cancel = AtomicBool::new(false);
    export_lottie_with_cancel(project, options, &cancel, on_progress)
}

/// Build a Lottie (`v: "5.7.14"`) JSON [`Value`] and [`LottieExportReport`] from `project`,
/// supporting both an [`AtomicBool`] cancellation token and a progress callback.
pub fn export_lottie_with_cancel<F>(
    project: &Project,
    options: &LottieOptions,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<(Value, LottieExportReport), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let (value, report, _) =
        export_lottie_internal(project, options, Some(cancel), &mut on_progress)?;
    if cancel.load(Ordering::Relaxed)
        || !on_progress(&LottieProgress {
            completed_steps: 4,
            total_steps: 4,
            current_stage: "Completed".into(),
        })
    {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    Ok((value, report))
}

fn check_cancel_flag(cancel: Option<&AtomicBool>) -> Result<(), ExportError> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    Ok(())
}

fn export_lottie_internal<F>(
    project: &Project,
    options: &LottieOptions,
    cancel: Option<&AtomicBool>,
    mut on_progress: F,
) -> Result<(Value, LottieExportReport, Vec<ExternalImageFile>), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    check_cancel_flag(cancel)?;
    project
        .validate()
        .map_err(|e| ExportError::InvalidProject(e.to_string()))?;

    if !on_progress(&LottieProgress {
        completed_steps: 0,
        total_steps: 4,
        current_stage: "Preparing timeline".into(),
    }) {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    check_cancel_flag(cancel)?;

    let (mut working_project, scene_ranges, target_symbol_id, comp_name, include_bg) =
        if let Some(sym_query) = options.symbol.as_deref() {
            let sym = resolve_target_symbol(project, sym_query).ok_or_else(|| {
                ExportError::Options(format!(
                    "Symbol '{sym_query}' was not found in the project."
                ))
            })?;
            let is_scene = project.is_scene(&sym.id) || sym.id == project.root;
            (
                project.clone(),
                Vec::new(),
                sym.id.clone(),
                sym.name.clone(),
                options.include_background && is_scene,
            )
        } else if let Some(scene_query) = options.scene.as_deref() {
            let sym = resolve_target_symbol(project, scene_query).ok_or_else(|| {
                ExportError::Options(format!(
                    "Scene '{scene_query}' was not found in the project."
                ))
            })?;
            (
                project.clone(),
                Vec::new(),
                sym.id.clone(),
                sym.name.clone(),
                options.include_background,
            )
        } else {
            let (flattened, ranges) = flatten_scenes_with_ranges(project);
            let root_id = flattened.root.clone();
            (
                flattened,
                ranges,
                root_id,
                project.name.clone(),
                options.include_background,
            )
        };

    check_cancel_flag(cancel)?;
    let mut text_outlined_count = 0usize;
    if options.text_mode == LottieTextMode::Shapes {
        text_outlined_count = crate::fonts::convert_project_text_to_outlines(&mut working_project);
    }

    if !on_progress(&LottieProgress {
        completed_steps: 1,
        total_steps: 4,
        current_stage: "Building layers and precomps".into(),
    }) {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    check_cancel_flag(cancel)?;

    let target_symbol = working_project.symbol(&target_symbol_id).ok_or_else(|| {
        ExportError::InvalidProject(format!("Target symbol '{target_symbol_id}' is missing."))
    })?;

    let scale = if options.scale.is_finite() && options.scale > 0.0 {
        options.scale.clamp(0.1, 16.0)
    } else {
        1.0
    };
    let base_w = options.width.unwrap_or(working_project.width).max(1);
    let base_h = options.height.unwrap_or(working_project.height).max(1);
    let comp_w = ((base_w as f64) * scale).round().max(1.0) as u32;
    let comp_h = ((base_h as f64) * scale).round().max(1.0) as u32;
    let fps_u32 = options.fps.unwrap_or(working_project.fps).clamp(1, 240);

    let package_external_images =
        options.format == LottieFormat::DotLottie || options.image_mode == LottieImageMode::Folder;
    let mut builder = LottieBuilder {
        project: &working_project,
        options,
        cancel,
        source_fps: f64::from(working_project.fps.max(1)),
        fps: f64::from(fps_u32),
        comp_w: working_project.width.max(1),
        comp_h: working_project.height.max(1),
        package_external_images,
        assets: Vec::new(),
        external_images: Vec::new(),
        precomp_id_by_symbol: BTreeMap::new(),
        image_id_by_resource: BTreeMap::new(),
        matte_precomps: BTreeMap::new(),
        used_asset_ids: BTreeSet::new(),
        fonts: BTreeMap::new(),
        emitted_text_layer: false,
        exported_count: 0,
        notes: Vec::new(),
        warnings: Vec::new(),
        warning_set: BTreeSet::new(),
    };

    if text_outlined_count > 0 {
        builder.add_note_coded(
            LottieNoteKind::Approximated,
            Some("LOTTIE_TEXT_OUTLINED"),
            "Document · Text",
            format!(
                "Converted {text_outlined_count} text element(s) to vector shape outlines (Text: Shapes)."
            ),
            None,
            None,
            None,
        );
    }

    if working_project.has_program() {
        builder.add_note_coded(
            LottieNoteKind::NotExported,
            Some("LOTTIE_PROJECT_SCRIPTS"),
            "Project · Scripts",
            "Project scripts/classes (ActionScript/JavaScript) are not executed by Lottie players; timeline animation was exported.",
            None,
            None,
            None,
        );
    }

    let is_scene = project.is_scene(&target_symbol.id) || target_symbol.id == working_project.root;
    let root_offset = if is_scene {
        [0.0, 0.0]
    } else {
        [
            working_project.width as f64 / 2.0,
            working_project.height as f64 / 2.0,
        ]
    };

    let mut stack = vec![target_symbol_id.clone()];
    let mut layers = builder.build_timeline_layers(
        &target_symbol_id,
        &comp_name,
        &target_symbol.timeline,
        root_offset,
        include_bg,
        &mut stack,
    )?;

    if builder.emitted_text_layer {
        builder.add_note_coded(
            LottieNoteKind::Approximated,
            Some("LOTTIE_TEXT_FONT_DEPENDENCY"),
            "Document · Text",
            format!(
                "Text layers on {} require runtime font availability matching `fonts.list`; switch Text to 'Shapes' for portable glyph outlines.",
                options.player.label()
            ),
            Some(target_symbol_id.clone()),
            None,
            None,
        );
    }

    check_cancel_flag(cancel)?;
    if !on_progress(&LottieProgress {
        completed_steps: 2,
        total_steps: 4,
        current_stage: "Applying scale, precision and frame range".into(),
    }) {
        return Err(ExportError::Options("Export cancelled.".into()));
    }

    let markers = if options.include_markers {
        build_markers(&target_symbol.timeline, &scene_ranges)
    } else {
        Vec::new()
    };
    let total_frames = target_symbol.timeline.frame_count.max(1);
    let (ip, op) = if let Some((from_1, to_1)) = options.frame_range {
        let from0 = from_1.saturating_sub(1).min(total_frames.saturating_sub(1));
        let to0 = to_1.max(from_1).min(total_frames);
        if from0 >= to0 {
            return Err(ExportError::Options(format!(
                "Invalid frame range {from_1}..={to_1} (document has {total_frames} frames)."
            )));
        }
        (from0, to0)
    } else {
        (0, total_frames)
    };
    let exported_frames = op.saturating_sub(ip).max(1);
    if exported_frames == 1 {
        builder.add_note_coded(
            LottieNoteKind::Approximated,
            Some("LOTTIE_SINGLE_FRAME_COMP"),
            comp_name.clone(),
            "Composition has 1 frame and will play as a still image.",
            Some(target_symbol_id.clone()),
            None,
            Some(ip),
        );
    }

    let sx = comp_w as f64 / working_project.width.max(1) as f64;
    let sy = comp_h as f64 / working_project.height.max(1) as f64;
    if (sx - 1.0).abs() > 1e-4 || (sy - 1.0).abs() > 1e-4 {
        let parent_ind = (layers.len() as u32) + 1000;
        for l in &mut layers {
            if let Some(obj) = l.as_object_mut()
                && !obj.contains_key("parent")
            {
                obj.insert("parent".into(), json!(parent_ind));
            }
        }
        layers.push(json!({
            "ddd": 0,
            "ind": parent_ind,
            "ty": 3,
            "nm": "Stage Scale",
            "sr": 1,
            "ks": {
                "o": {"a": 0, "k": 0},
                "r": {"a": 0, "k": 0},
                "p": {"a": 0, "k": [0, 0, 0]},
                "a": {"a": 0, "k": [0, 0, 0]},
                "s": {"a": 0, "k": [round4(sx * 100.0), round4(sy * 100.0), 100.0]}
            },
            "ao": 0,
            "ip": ip,
            "op": op,
            "st": 0,
            "bm": 0
        }));
    }

    let mut root = json!({
        "v": LOTTIE_VERSION,
        "fr": round4(builder.fps),
        "ip": ip,
        "op": op,
        "w": comp_w,
        "h": comp_h,
        "nm": comp_name,
        "ddd": 0,
        "assets": builder.assets,
        "layers": layers,
        "markers": markers
    });
    if !builder.fonts.is_empty()
        && let Some(root_map) = root.as_object_mut()
    {
        let font_list: Vec<Value> = builder.fonts.values().cloned().collect();
        root_map.insert("fonts".into(), json!({ "list": font_list }));
    }

    let precision = options.precision.min(4);
    if precision < 4 {
        round_json_numbers(&mut root, precision);
    }

    check_cancel_flag(cancel)?;

    let approximated_count = builder
        .notes
        .iter()
        .filter(|n| n.kind == LottieNoteKind::Approximated)
        .count();
    let not_exported_count = builder
        .notes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                LottieNoteKind::NotExported | LottieNoteKind::Missing
            )
        })
        .count();
    let layer_count = root["layers"].as_array().map_or(0, Vec::len);
    let asset_count = root["assets"].as_array().map_or(0, Vec::len);
    let marker_count = root["markers"].as_array().map_or(0, Vec::len);

    let report = LottieExportReport {
        path: None,
        html_preview_path: None,
        name: comp_name,
        width: comp_w,
        height: comp_h,
        fps: fps_u32,
        in_point: ip,
        frame_count: exported_frames,
        layer_count,
        asset_count,
        marker_count,
        bytes: 0,
        exported_count: builder.exported_count.max(layer_count),
        approximated_count,
        not_exported_count,
        notes: builder.notes,
        warnings: builder.warnings,
    };

    Ok((root, report, builder.external_images))
}

fn round_json_numbers(val: &mut Value, decimals: u8) {
    let factor = 10_f64.powi(i32::from(decimals));
    match val {
        Value::Number(n) => {
            if let Some(f) = n.as_f64()
                && n.as_i64().is_none()
                && n.as_u64().is_none()
            {
                let rounded = (f * factor).round() / factor;
                if let Some(num) = serde_json::Number::from_f64(rounded) {
                    *n = num;
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                round_json_numbers(item, decimals);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                round_json_numbers(v, decimals);
            }
        }
        _ => {}
    }
}

/// Serialize `project` into a Lottie JSON string along with [`LottieExportReport`].
pub fn export_lottie_json(
    project: &Project,
    options: &LottieOptions,
) -> Result<(String, LottieExportReport), ExportError> {
    export_lottie_json_with_progress(project, options, |_| true)
}

/// Serialize `project` into a Lottie JSON string along with [`LottieExportReport`],
/// reporting progress and supporting cancellation (`on_progress` returning `false`).
pub fn export_lottie_json_with_progress<F>(
    project: &Project,
    options: &LottieOptions,
    on_progress: F,
) -> Result<(String, LottieExportReport), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let cancel = AtomicBool::new(false);
    export_lottie_json_with_cancel(project, options, &cancel, on_progress)
}

/// Serialize `project` into a Lottie JSON string along with [`LottieExportReport`],
/// supporting both an [`AtomicBool`] cancellation token and a progress callback.
pub fn export_lottie_json_with_cancel<F>(
    project: &Project,
    options: &LottieOptions,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<(String, LottieExportReport), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let (value, mut report, external_images) =
        export_lottie_internal(project, options, Some(cancel), &mut on_progress)?;
    check_cancel_flag(Some(cancel))?;
    let json = if options.pretty {
        serde_json::to_string_pretty(&value)?
    } else {
        serde_json::to_string(&value)?
    };
    check_cancel_flag(Some(cancel))?;
    if options.format == LottieFormat::DotLottie {
        let zip_bytes = build_dotlottie_zip(json.as_bytes(), &external_images, Some(cancel))?;
        report.bytes = zip_bytes.len();
    } else {
        report.bytes = json.len();
    }
    if cancel.load(Ordering::Relaxed)
        || !on_progress(&LottieProgress {
            completed_steps: 4,
            total_steps: 4,
            current_stage: "Completed".into(),
        })
    {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    Ok((json, report))
}

/// Build an in-memory `.lottie` ZIP archive and [`LottieExportReport`] from `project`.
pub fn export_dotlottie_bytes(
    project: &Project,
    options: &LottieOptions,
) -> Result<(Vec<u8>, LottieExportReport), ExportError> {
    let cancel = AtomicBool::new(false);
    export_dotlottie_bytes_with_cancel(project, options, &cancel, |_| true)
}

/// Build an in-memory `.lottie` ZIP archive with [`AtomicBool`] cancellation and progress reporting.
pub fn export_dotlottie_bytes_with_cancel<F>(
    project: &Project,
    options: &LottieOptions,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<(Vec<u8>, LottieExportReport), ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let dot_opts = LottieOptions {
        format: LottieFormat::DotLottie,
        ..options.clone()
    };
    let (value, mut report, external_images) =
        export_lottie_internal(project, &dot_opts, Some(cancel), &mut on_progress)?;
    check_cancel_flag(Some(cancel))?;
    let json = if dot_opts.pretty {
        serde_json::to_string_pretty(&value)?
    } else {
        serde_json::to_string(&value)?
    };
    let zip_bytes = build_dotlottie_zip(json.as_bytes(), &external_images, Some(cancel))?;
    validate_dotlottie_bytes(&zip_bytes)?;
    report.bytes = zip_bytes.len();
    if cancel.load(Ordering::Relaxed)
        || !on_progress(&LottieProgress {
            completed_steps: 4,
            total_steps: 4,
            current_stage: "Completed".into(),
        })
    {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    Ok((zip_bytes, report))
}

/// Write a dotLottie (`.lottie`) ZIP package atomically to `output_file`.
pub fn export_dotlottie(
    project: &Project,
    output_file: &Path,
    options: &LottieOptions,
) -> Result<LottieExportReport, ExportError> {
    let dot_opts = LottieOptions {
        format: LottieFormat::DotLottie,
        ..options.clone()
    };
    export_project_lottie(project, output_file, &dot_opts)
}

/// Write a Lottie `.json` or `.lottie` file to `output_file` and return the resulting [`LottieExportReport`].
pub fn export_project_lottie(
    project: &Project,
    output_file: &Path,
    options: &LottieOptions,
) -> Result<LottieExportReport, ExportError> {
    export_project_lottie_with_progress(project, output_file, options, |_| true)
}

/// Write a Lottie `.json` or `.lottie` file to `output_file` with progress and cancellation support.
pub fn export_project_lottie_with_progress<F>(
    project: &Project,
    output_file: &Path,
    options: &LottieOptions,
    on_progress: F,
) -> Result<LottieExportReport, ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    let cancel = AtomicBool::new(false);
    export_project_lottie_with_cancel(project, output_file, options, &cancel, on_progress)
}

/// Write a Lottie `.json` or `.lottie` file atomically to `output_file` with [`AtomicBool`]
/// cancellation and progress reporting. If cancelled or errored at any point, zero partial or
/// temporary files are left on disk and any pre-existing file at `output_file` remains untouched.
pub fn export_project_lottie_with_cancel<F>(
    project: &Project,
    output_file: &Path,
    options: &LottieOptions,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<LottieExportReport, ExportError>
where
    F: FnMut(&LottieProgress) -> bool,
{
    check_cancel_flag(Some(cancel))?;
    let is_dotlottie = options.format == LottieFormat::DotLottie
        || output_file
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("lottie"));

    let effective_options = if is_dotlottie && options.format != LottieFormat::DotLottie {
        LottieOptions {
            format: LottieFormat::DotLottie,
            ..options.clone()
        }
    } else {
        options.clone()
    };

    let (value, mut report, external_images) =
        export_lottie_internal(project, &effective_options, Some(cancel), &mut on_progress)?;
    check_cancel_flag(Some(cancel))?;

    if !on_progress(&LottieProgress {
        completed_steps: 3,
        total_steps: 4,
        current_stage: "Packaging assets and staging files".into(),
    }) {
        return Err(ExportError::Options("Export cancelled.".into()));
    }
    check_cancel_flag(Some(cancel))?;

    let json = if effective_options.pretty {
        serde_json::to_string_pretty(&value)?
    } else {
        serde_json::to_string(&value)?
    };
    check_cancel_flag(Some(cancel))?;

    let mut staged = StagedFileBatch::new();

    if is_dotlottie {
        let zip_bytes = build_dotlottie_zip(json.as_bytes(), &external_images, Some(cancel))?;
        validate_dotlottie_bytes(&zip_bytes)?;
        staged.stage_file(output_file, &zip_bytes, Some(cancel))?;
        report.bytes = zip_bytes.len();
    } else {
        staged.stage_file(output_file, json.as_bytes(), Some(cancel))?;
        report.bytes = json.len();
    }

    let write_external_folder_images = !external_images.is_empty()
        && (!is_dotlottie
            || effective_options.image_mode == LottieImageMode::Folder
            || effective_options.html_preview);
    if write_external_folder_images {
        let base_dir = output_file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        for img in &external_images {
            validate_zip_entry_path(&img.relative_path)?;
            let img_path = base_dir.join(&img.relative_path);
            staged.stage_file(&img_path, &img.bytes, Some(cancel))?;
        }
    }

    if effective_options.html_preview {
        let preview_path = output_file.with_extension("preview.html");
        let html = build_lottie_html_preview(&report.name, report.width, report.height, &json);
        staged.stage_file(&preview_path, html.as_bytes(), Some(cancel))?;
        report.html_preview_path = Some(preview_path);
    }

    if cancel.load(Ordering::Relaxed)
        || !on_progress(&LottieProgress {
            completed_steps: 4,
            total_steps: 4,
            current_stage: "Completed".into(),
        })
    {
        return Err(ExportError::Options("Export cancelled.".into()));
    }

    staged.commit(Some(cancel))?;
    report.path = Some(output_file.to_path_buf());
    Ok(report)
}

struct StagedFileBatch {
    staged_files: Vec<(PathBuf, PathBuf)>,
    created_dirs: Vec<PathBuf>,
    committed: bool,
}

impl StagedFileBatch {
    fn new() -> Self {
        Self {
            staged_files: Vec::new(),
            created_dirs: Vec::new(),
            committed: false,
        }
    }

    fn ensure_dir_tracked(&mut self, dir: &Path) -> Result<(), ExportError> {
        if dir.as_os_str().is_empty() || dir.exists() {
            return Ok(());
        }
        let mut missing = Vec::new();
        let mut cur = Some(dir);
        while let Some(p) = cur {
            if p.as_os_str().is_empty() || p.exists() {
                break;
            }
            missing.push(p.to_path_buf());
            cur = p.parent();
        }
        for p in missing.into_iter().rev() {
            match fs::create_dir(&p) {
                Ok(()) => self.created_dirs.push(p),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(ExportError::Io(e)),
            }
        }
        Ok(())
    }

    fn stage_file(
        &mut self,
        final_path: &Path,
        bytes: &[u8],
        cancel: Option<&AtomicBool>,
    ) -> Result<(), ExportError> {
        check_cancel_flag(cancel)?;
        let parent = final_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        self.ensure_dir_tracked(parent)?;
        check_cancel_flag(cancel)?;

        let file_name = final_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("output");
        static NEXT_TMP_SEQ: AtomicU64 = AtomicU64::new(0);
        let tmp_path = loop {
            let seq = NEXT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".{file_name}.neoflash-tmp-{}-{seq}",
                std::process::id()
            ));
            if !candidate.exists() {
                break candidate;
            }
        };

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        self.staged_files
            .push((tmp_path.clone(), final_path.to_path_buf()));

        for chunk in bytes.chunks(64 * 1024) {
            check_cancel_flag(cancel)?;
            file.write_all(chunk)?;
        }
        file.sync_all()?;
        drop(file);
        check_cancel_flag(cancel)?;
        Ok(())
    }

    fn commit(mut self, cancel: Option<&AtomicBool>) -> Result<(), ExportError> {
        check_cancel_flag(cancel)?;
        let mut touched_parents = BTreeSet::new();
        for (tmp_path, final_path) in &self.staged_files {
            fs::rename(tmp_path, final_path)?;
            if let Some(parent) = final_path.parent()
                && !parent.as_os_str().is_empty()
            {
                touched_parents.insert(parent.to_path_buf());
            }
        }
        for parent in touched_parents {
            if let Ok(dir_file) = fs::File::open(parent) {
                let _ = dir_file.sync_all();
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedFileBatch {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (tmp_path, _) in self.staged_files.drain(..).rev() {
            let _ = fs::remove_file(tmp_path);
        }
        for dir in self.created_dirs.drain(..).rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

fn validate_zip_entry_path(name: &str) -> Result<(), ExportError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.contains(':')
        || name.chars().any(char::is_control)
    {
        return Err(ExportError::Options(format!(
            "Invalid dotLottie archive entry path '{name}'."
        )));
    }
    let parts: Vec<&str> = name.split('/').collect();
    if parts
        .iter()
        .any(|seg| seg.is_empty() || *seg == "." || *seg == "..")
    {
        return Err(ExportError::Options(format!(
            "Unsafe or non-normalized dotLottie archive entry path '{name}'."
        )));
    }
    Ok(())
}

fn build_dotlottie_zip(
    animation_json: &[u8],
    external_images: &[ExternalImageFile],
    cancel: Option<&AtomicBool>,
) -> Result<Vec<u8>, ExportError> {
    check_cancel_flag(cancel)?;
    let manifest = json!({
        "version": "1.0",
        "generator": "NeoFlash",
        "animations": [{
            "id": "animation",
            "speed": 1,
            "loop": true
        }]
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let deflate_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let stored_opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let cursor = Cursor::new(Vec::new());
    let mut zw = zip::ZipWriter::new(cursor);

    zw.start_file("manifest.json", deflate_opts)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    zw.write_all(&manifest_bytes)?;
    check_cancel_flag(cancel)?;

    zw.start_file("animations/animation.json", deflate_opts)
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    zw.write_all(animation_json)?;
    check_cancel_flag(cancel)?;

    let mut seen_paths = BTreeSet::new();
    seen_paths.insert("manifest.json".to_owned());
    seen_paths.insert("animations/animation.json".to_owned());
    for img in external_images {
        check_cancel_flag(cancel)?;
        validate_zip_entry_path(&img.relative_path)?;
        if !seen_paths.insert(img.relative_path.clone()) {
            return Err(ExportError::Options(format!(
                "Duplicate dotLottie archive entry '{}'.",
                img.relative_path
            )));
        }
        zw.start_file(&img.relative_path, stored_opts)
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
        zw.write_all(&img.bytes)?;
    }

    let finished = zw
        .finish()
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;
    Ok(finished.into_inner())
}

/// Validate a `.lottie` ZIP archive against the dotLottie container specification,
/// checking path safety, `manifest.json`, `animations/<id>.json`, and every referenced
/// `assets[]` / `images/*` entry. Returns the parsed primary animation JSON [`Value`].
pub fn validate_dotlottie_bytes(bytes: &[u8]) -> Result<Value, ExportError> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(ExportError::Options(
            "dotLottie archive does not start with ZIP local header signature.".into(),
        ));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ExportError::Options(format!("Invalid dotLottie ZIP archive: {e}")))?;
    if archive.len() > 4096 {
        return Err(ExportError::Options(
            "dotLottie archive exceeds 4096 entry limit.".into(),
        ));
    }

    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            ExportError::Options(format!("Invalid dotLottie ZIP entry #{idx}: {e}"))
        })?;
        let name = entry.name().to_owned();
        if entry.is_dir() {
            return Err(ExportError::Options(format!(
                "dotLottie archive contains unexpected directory entry '{name}'."
            )));
        }
        validate_zip_entry_path(&name)?;
        if entries.contains_key(&name) {
            return Err(ExportError::Options(format!(
                "Duplicate entry '{name}' in dotLottie archive."
            )));
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        entries.insert(name, data);
    }

    let manifest_bytes = entries.get("manifest.json").ok_or_else(|| {
        ExportError::Options("dotLottie archive is missing 'manifest.json'.".into())
    })?;
    let manifest: Value = serde_json::from_slice(manifest_bytes).map_err(|e| {
        ExportError::Options(format!("Invalid JSON in dotLottie 'manifest.json': {e}"))
    })?;
    let version = manifest["version"].as_str().unwrap_or("");
    if version.trim().is_empty() {
        return Err(ExportError::Options(
            "dotLottie 'manifest.json' is missing non-empty 'version'.".into(),
        ));
    }
    let generator = manifest["generator"].as_str().unwrap_or("");
    if generator.trim().is_empty() {
        return Err(ExportError::Options(
            "dotLottie 'manifest.json' is missing 'generator'.".into(),
        ));
    }
    let anims = manifest["animations"].as_array().ok_or_else(|| {
        ExportError::Options("dotLottie 'manifest.json' is missing 'animations' array.".into())
    })?;
    if anims.is_empty() {
        return Err(ExportError::Options(
            "dotLottie 'manifest.json' must declare at least one animation.".into(),
        ));
    }

    let mut primary_anim: Option<Value> = None;
    for anim_meta in anims {
        let anim_id = anim_meta["id"].as_str().unwrap_or("");
        if anim_id.is_empty() || anim_id.contains('/') || anim_id.contains('\\') || anim_id == ".."
        {
            return Err(ExportError::Options(format!(
                "Invalid animation id '{anim_id}' in dotLottie manifest."
            )));
        }
        let anim_entry_path = format!("animations/{anim_id}.json");
        let anim_bytes = entries.get(&anim_entry_path).ok_or_else(|| {
            ExportError::Options(format!(
                "dotLottie archive is missing animation file '{anim_entry_path}'."
            ))
        })?;
        let anim_val: Value = serde_json::from_slice(anim_bytes).map_err(|e| {
            ExportError::Options(format!("Invalid JSON in '{anim_entry_path}': {e}"))
        })?;
        validate_lottie_asset_references(&anim_val, Some(&entries))?;
        if primary_anim.is_none() {
            primary_anim = Some(anim_val);
        }
    }

    primary_anim.ok_or_else(|| ExportError::Options("No animation found in dotLottie.".into()))
}

fn validate_lottie_asset_references(
    lottie: &Value,
    archive_entries: Option<&BTreeMap<String, Vec<u8>>>,
) -> Result<(), ExportError> {
    let assets = lottie["assets"].as_array().ok_or_else(|| {
        ExportError::Options("Lottie JSON is missing root 'assets' array.".into())
    })?;
    let mut asset_ids = BTreeSet::new();
    for asset in assets {
        let id = asset["id"].as_str().unwrap_or("");
        if id.is_empty() {
            return Err(ExportError::Options(
                "Lottie asset entry is missing non-empty 'id'.".into(),
            ));
        }
        if !asset_ids.insert(id.to_owned()) {
            return Err(ExportError::Options(format!(
                "Duplicate Lottie asset id '{id}'."
            )));
        }
        if let Some(p) = asset["p"].as_str() {
            let embedded = asset["e"].as_i64().unwrap_or(0) == 1;
            if embedded {
                if !p.starts_with("data:image/") {
                    return Err(ExportError::Options(format!(
                        "Embedded Lottie image asset '{id}' does not use a valid data:image/* URI."
                    )));
                }
            } else {
                let u = asset["u"].as_str().unwrap_or("");
                let rel = format!("{u}{p}");
                validate_zip_entry_path(&rel)?;
                if let Some(entries) = archive_entries {
                    let img_bytes = entries.get(&rel).ok_or_else(|| {
                        ExportError::Options(format!(
                            "dotLottie archive is missing referenced image asset '{rel}' (id '{id}')."
                        ))
                    })?;
                    if detect_raster_magic(img_bytes) == DetectedRaster::Other {
                        return Err(ExportError::Options(format!(
                            "dotLottie image entry '{rel}' does not have valid PNG/JPEG/WebP/GIF magic bytes."
                        )));
                    }
                }
            }
        }
    }

    let check_layers = |layers: &[Value], ctx: &str| -> Result<(), ExportError> {
        for layer in layers {
            if let Some(ref_id) = layer["refId"].as_str()
                && !asset_ids.contains(ref_id)
            {
                return Err(ExportError::Options(format!(
                    "Dangling refId '{ref_id}' in {ctx} layer '{}'.",
                    layer["nm"].as_str().unwrap_or("<unnamed>")
                )));
            }
        }
        Ok(())
    };

    let root_layers = lottie["layers"].as_array().ok_or_else(|| {
        ExportError::Options("Lottie JSON is missing root 'layers' array.".into())
    })?;
    check_layers(root_layers, "root")?;
    for asset in assets {
        if let Some(comp_layers) = asset["layers"].as_array() {
            let id = asset["id"].as_str().unwrap_or("<precomp>");
            check_layers(comp_layers, id)?;
        }
    }
    Ok(())
}

fn escape_html_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\u{2028}' => out.push_str("&#x2028;"),
            '\u{2029}' => out.push_str("&#x2029;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_json_for_html_script(json: &str) -> String {
    let mut out = String::with_capacity(json.len() + 16);
    for ch in json.chars() {
        match ch {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

fn build_lottie_html_preview(title: &str, width: u32, height: u32, lottie_json: &str) -> String {
    let safe_title = escape_html_text(title);
    let safe_json = escape_json_for_html_script(lottie_json);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{safe_title} — Lottie Preview</title>
<style>
  body {{ margin: 0; background: #14171f; color: #e6e9f0; font-family: system-ui, sans-serif; display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; }}
  #player {{ width: {width}px; height: {height}px; max-width: 92vw; max-height: 80vh; background: #1e222d; border-radius: 8px; box-shadow: 0 8px 28px rgba(0,0,0,0.45); overflow: hidden; }}
  .meta {{ margin-top: 12px; font-size: 13px; color: #9aa2b2; }}
</style>
<script src="https://cdnjs.cloudflare.com/ajax/libs/lottie-web/5.12.2/lottie.min.js" async></script>
</head>
<body>
  <div id="player"></div>
  <div class="meta">{safe_title} · {width} × {height} · Bodymovin v{LOTTIE_VERSION}</div>
  <script>
    (function() {{
      const rootEl = document.documentElement;
      const container = document.getElementById('player');
      const animationData = {safe_json};
      window.__lottieAnimationData = animationData;

      function mountPlayer() {{
        if (window.lottie && !container.dataset.mounted) {{
          container.dataset.mounted = 'lottie-web';
          container.textContent = '';
          window.lottie.loadAnimation({{
            container: container,
            renderer: 'svg',
            loop: true,
            autoplay: true,
            animationData: animationData
          }});
          return true;
        }}
        return false;
      }}

      if (!mountPlayer()) {{
        const ns = 'http://www.w3.org/2000/svg';
        const svg = document.createElementNS(ns, 'svg');
        svg.setAttribute('viewBox', '0 0 ' + (animationData.w || {width}) + ' ' + (animationData.h || {height}));
        svg.setAttribute('width', '100%');
        svg.setAttribute('height', '100%');
        const layers = Array.isArray(animationData.layers) ? animationData.layers : [];
        for (let i = layers.length - 1; i >= 0; i--) {{
          const layer = layers[i];
          if (layer && layer.ty === 1 && typeof layer.sc === 'string') {{
            const rect = document.createElementNS(ns, 'rect');
            rect.setAttribute('width', String(layer.sw || animationData.w || {width}));
            rect.setAttribute('height', String(layer.sh || animationData.h || {height}));
            rect.setAttribute('fill', layer.sc);
            svg.appendChild(rect);
          }}
        }}
        container.appendChild(svg);
        window.addEventListener('load', mountPlayer);
      }}

      const assets = Array.isArray(animationData.assets) ? animationData.assets : [];
      const assetMap = new Map();
      const imageAssets = [];
      for (const asset of assets) {{
        if (asset && typeof asset.id === 'string') {{
          assetMap.set(asset.id, asset);
          if (typeof asset.p === 'string' && asset.p.length > 0) {{
            imageAssets.push(asset);
          }}
        }}
      }}

      const dangling = [];
      function checkLayerRefs(list) {{
        if (!Array.isArray(list)) return;
        for (const layer of list) {{
          if (layer && typeof layer.refId === 'string' && !assetMap.has(layer.refId)) {{
            dangling.push(layer.refId);
          }}
        }}
      }}
      checkLayerRefs(animationData.layers);
      for (const asset of assets) {{
        if (asset && Array.isArray(asset.layers)) {{
          checkLayerRefs(asset.layers);
        }}
      }}

      rootEl.setAttribute('data-lottie-xss', String(Boolean(window.__xss || window.__t051_marker)));
      rootEl.setAttribute('data-lottie-dangling-refs', String(dangling.length));
      rootEl.setAttribute('data-lottie-images-expected', String(imageAssets.length));

      if (imageAssets.length === 0) {{
        rootEl.setAttribute('data-lottie-images-loaded', '0');
        rootEl.setAttribute('data-lottie-images-failed', '0');
        rootEl.setAttribute('data-lottie-images-verified', String(dangling.length === 0));
        rootEl.setAttribute('data-lottie-ready', 'true');
      }} else {{
        let loaded = 0;
        let failed = 0;
        const finishOne = () => {{
          if (loaded + failed === imageAssets.length) {{
            rootEl.setAttribute('data-lottie-xss', String(Boolean(window.__xss || window.__t051_marker)));
            rootEl.setAttribute('data-lottie-images-loaded', String(loaded));
            rootEl.setAttribute('data-lottie-images-failed', String(failed));
            rootEl.setAttribute(
              'data-lottie-images-verified',
              String(dangling.length === 0 && failed === 0 && loaded === imageAssets.length)
            );
            rootEl.setAttribute('data-lottie-ready', 'true');
          }}
        }};
        for (const imgAsset of imageAssets) {{
          const probe = new Image();
          probe.onload = () => {{ loaded += 1; finishOne(); }};
          probe.onerror = () => {{ failed += 1; finishOne(); }};
          probe.src = imgAsset.e === 1 ? imgAsset.p : ((imgAsset.u || '') + imgAsset.p);
        }}
      }}
    }})();
  </script>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        FrameScript, GradientStop, Keyframe, LayerProperties, MotionPathOverride,
        MotionPropertyKey, MotionTrackOverride, OvalOptions, PrimitiveOptions, PropertyExpression,
        RectangleOptions, Script, ScriptLanguage, SymbolKind, TweenProperty,
    };

    struct TestOutputDir(PathBuf);

    impl TestOutputDir {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "neoflash-lottie-{label}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test output {}: {error}", path.display()),
                }
            }
        }
    }

    impl Drop for TestOutputDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn object_scoped_motion_tracks_export_independent_paths_eases_and_orientation() {
        let rect = |id: &str, x: f64, y: f64, alpha: f64| Element {
            id: id.into(),
            name: id.into(),
            transform: Transform {
                x,
                y,
                alpha,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let path = |start_x, control_y, end_x| MotionPathOverride::Path {
            commands: vec![
                PathCommand::MoveTo { x: start_x, y: 0.0 },
                PathCommand::CubicTo {
                    cx1: start_x,
                    cy1: control_y,
                    cx2: end_x,
                    cy2: control_y,
                    x: end_x,
                    y: 0.0,
                },
            ],
        };
        let mut a_track = MotionTrackOverride::new("a");
        a_track.easing = Some(Easing::EaseIn);
        a_track.property_eases = Some(vec![
            (TweenProperty::X, Easing::Linear),
            (TweenProperty::Y, Easing::Linear),
        ]);
        a_track.orient_to_path = Some(true);
        a_track.motion_path = Some(path(0.0, 100.0, 100.0));
        let mut b_track = MotionTrackOverride::new("b");
        b_track.easing = Some(Easing::EaseOut);
        b_track.property_eases = Some(vec![
            (TweenProperty::X, Easing::Linear),
            (TweenProperty::Y, Easing::Linear),
        ]);
        b_track.orient_to_path = Some(false);
        b_track.motion_path = Some(path(200.0, -100.0, 300.0));

        let mut project = Project::blank();
        project.format_version = 10;
        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 21;
        root.timeline.layers[0].name = "Motion".into();
        root.timeline.layers[0].keyframes = vec![
            Keyframe {
                elements: vec![rect("a", 0.0, 0.0, 1.0), rect("b", 200.0, 0.0, 1.0)],
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Motion,
                motion_tracks: vec![a_track, b_track],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 20,
                elements: vec![rect("a", 100.0, 0.0, 0.0), rect("b", 300.0, 0.0, 0.0)],
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Motion,
                keyed_properties: Some(vec![
                    TweenProperty::X,
                    TweenProperty::Y,
                    TweenProperty::Alpha,
                ]),
                motion_keys: Some(vec![
                    MotionPropertyKey {
                        element_id: "a".into(),
                        properties: vec![TweenProperty::X, TweenProperty::Y, TweenProperty::Alpha],
                        roving: false,
                    },
                    MotionPropertyKey {
                        element_id: "b".into(),
                        properties: vec![TweenProperty::X, TweenProperty::Y, TweenProperty::Alpha],
                        roving: false,
                    },
                ]),
                ..Keyframe::default()
            },
        ];
        project
            .validate()
            .expect("format-10 motion project validates");

        let (value, _) = export_lottie_value(
            &project,
            &LottieOptions {
                include_background: false,
                ..LottieOptions::default()
            },
        )
        .expect("export object-scoped motion");
        let layers = value["layers"].as_array().unwrap();
        let layer = |name: &str| {
            layers
                .iter()
                .find(|layer| layer["nm"] == format!("Motion · {name}"))
                .unwrap_or_else(|| panic!("missing Lottie track for {name}"))
        };
        let a = layer("a");
        let b = layer("b");
        fn key_at<'a>(property: &str, layer: &'a Value, frame: u32) -> &'a Value {
            layer["ks"][property]["k"]
                .as_array()
                .unwrap()
                .iter()
                .find(|key| key["t"] == frame)
                .unwrap_or_else(|| panic!("missing {property} key at frame {frame}"))
        }
        let a_position = &key_at("p", a, 10)["s"];
        let b_position = &key_at("p", b, 10)["s"];
        assert!((a_position[0].as_f64().unwrap() - 50.0).abs() < 0.05);
        assert!((a_position[1].as_f64().unwrap() - 75.0).abs() < 0.05);
        assert!((b_position[0].as_f64().unwrap() - 250.0).abs() < 0.05);
        assert!((b_position[1].as_f64().unwrap() + 75.0).abs() < 0.05);
        assert!((key_at("o", a, 10)["s"][0].as_f64().unwrap() - 75.0).abs() < 0.05);
        assert!((key_at("o", b, 10)["s"][0].as_f64().unwrap() - 25.0).abs() < 0.05);
        assert_eq!(a["ao"], 1);
        assert_eq!(b["ao"], 0);
    }

    fn heart_puff_commands(cx: f64, cy: f64, size: f64) -> Vec<PathCommand> {
        vec![
            PathCommand::MoveTo {
                x: cx,
                y: cy + size * 0.85,
            },
            PathCommand::CubicTo {
                cx1: cx - size * 1.15,
                cy1: cy + size * 0.2,
                cx2: cx - size * 0.95,
                cy2: cy - size * 0.85,
                x: cx,
                y: cy - size * 0.3,
            },
            PathCommand::CubicTo {
                cx1: cx + size * 0.95,
                cy1: cy - size * 0.85,
                cx2: cx + size * 1.15,
                cy2: cy + size * 0.2,
                x: cx,
                y: cy + size * 0.85,
            },
            PathCommand::Close,
        ]
    }

    #[test]
    fn property_expression_sampling_uses_source_document_fps() {
        let mut project = Project::blank();
        project.fps = 30;
        project.format_version = 9;
        let root_id = project.root.clone();
        let element = Element {
            id: "expression-shape".into(),
            transform: Transform::default(),
            kind: ElementKind::Rectangle {
                width: 10.0,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 61;
        root.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![element.clone()],
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Motion,
                property_expressions: vec![PropertyExpression {
                    property: TweenProperty::X,
                    expression: "value + time".into(),
                    ..PropertyExpression::default()
                }],
                ..Keyframe::default()
            },
            Keyframe {
                frame: 60,
                elements: vec![element],
                ..Keyframe::default()
            },
        ];
        project.validate().expect("expression project validates");

        let position_at_30 = |fps| {
            let mut project = project.clone();
            project.fps = fps;
            let (value, _) = export_lottie_value(
                &project,
                &LottieOptions {
                    include_background: false,
                    ..LottieOptions::default()
                },
            )
            .unwrap();
            value["layers"][0]["ks"]["p"]["k"]
                .as_array()
                .unwrap()
                .iter()
                .find(|key| key["t"] == 30)
                .unwrap()["s"][0]
                .as_f64()
                .unwrap()
        };
        assert!((position_at_30(30) - 1.0).abs() < 1e-9);
        assert!((position_at_30(60) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn exact_shape_keys_use_the_shared_effect_aware_sampler() {
        use neoflash_core::{
            ElementAppearance, ElementVectorEffects, FreeformGradient, FreeformGradientPoint,
            VectorModifier, sample_layer_in_at_fps,
        };

        let rectangle = |width| Element {
            id: "shape".into(),
            kind: ElementKind::Rectangle {
                width,
                height: 10.0,
                fill: [255, 0, 0, 255],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            ..Element::default()
        };
        let start = Keyframe {
            frame: 0,
            elements: vec![rectangle(10.0)],
            tween: Some(Easing::Linear),
            tween_kind: TweenKind::Shape,
            property_expressions: vec![PropertyExpression {
                property: TweenProperty::X,
                expression: "value + 7".into(),
                ..PropertyExpression::default()
            }],
            vector_effects: vec![ElementVectorEffects {
                element_id: "shape".into(),
                freeform_gradient: Some(FreeformGradient {
                    points: vec![FreeformGradientPoint {
                        id: "green".into(),
                        position: [0.0, 0.0],
                        color: [0, 255, 0, 255],
                        radius: 10.0,
                        weight: 1.0,
                    }],
                    lines: Vec::new(),
                }),
                ..ElementVectorEffects::default()
            }],
            appearances: vec![ElementAppearance {
                element_id: "shape".into(),
                modifiers: vec![VectorModifier::Repeater {
                    copies: 2,
                    offset_x: 20.0,
                    offset_y: 0.0,
                    scale_step: 1.0,
                    rotation_step: 0.0,
                }],
                ..ElementAppearance::default()
            }],
            ..Keyframe::default()
        };
        let end = Keyframe {
            frame: 10,
            elements: vec![rectangle(20.0)],
            property_expressions: vec![PropertyExpression {
                property: TweenProperty::Y,
                expression: "value + 3".into(),
                ..PropertyExpression::default()
            }],
            ..Keyframe::default()
        };
        let layer = Layer {
            id: "shape-layer".into(),
            name: "Shape".into(),
            visible: true,
            locked: false,
            keyframes: vec![start, end],
            properties: LayerProperties::default(),
        };
        let timeline = Timeline {
            frame_count: 11,
            layers: vec![layer],
            scripts: Vec::new(),
            guides: Vec::new(),
        };

        for frame in [0, 10] {
            let exported = sample_shape_tween_frame(&timeline, &timeline.layers[0], frame, 30.0);
            let shared = sample_layer_in_at_fps(&timeline, &timeline.layers[0], frame, 30.0);
            assert_eq!(exported, shared);
        }
        assert_eq!(
            sample_shape_tween_frame(&timeline, &timeline.layers[0], 0, 30.0)[0]
                .transform
                .x,
            7.0
        );
        assert_eq!(
            sample_shape_tween_frame(&timeline, &timeline.layers[0], 10, 30.0)[0]
                .transform
                .y,
            3.0
        );
    }

    #[test]
    fn simple_shape_tween_keeps_one_animated_shape_track() {
        let mut project = Project::blank();
        project.fps = 30;
        project.format_version = 6;
        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 11;
        root.timeline.layers[0].keyframes = vec![
            Keyframe {
                frame: 0,
                elements: vec![Element {
                    id: "morph".into(),
                    kind: ElementKind::Rectangle {
                        width: 40.0,
                        height: 20.0,
                        fill: [255, 0, 0, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                tween: Some(Easing::Linear),
                tween_kind: TweenKind::Shape,
                ..Keyframe::default()
            },
            Keyframe {
                frame: 10,
                elements: vec![Element {
                    id: "morph".into(),
                    kind: ElementKind::Ellipse {
                        width: 50.0,
                        height: 30.0,
                        fill: [0, 0, 255, 255],
                        stroke: [0; 4],
                        stroke_width: 0.0,
                    },
                    ..Element::default()
                }],
                ..Keyframe::default()
            },
        ];

        let (value, _) = export_lottie_value(
            &project,
            &LottieOptions {
                include_background: false,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        let layers = value["layers"].as_array().unwrap();
        assert_eq!(
            layers.len(),
            1,
            "simple morph should not become frame slices"
        );
        assert_eq!(layers[0]["ty"], 4);
        let items = layers[0]["shapes"][0]["it"].as_array().unwrap();
        let path = items.iter().find(|item| item["ty"] == "sh").unwrap();
        assert_eq!(path["ks"]["a"], 1, "morph path should remain animated");
    }

    #[test]
    fn demo_and_tween_features_export_valid_bodymovin_lottie_json() {
        let mut project = Project::blank();
        project.name = "Hero Banner".into();
        project.width = 640;
        project.height = 360;
        project.fps = 30;
        project.background = [24, 28, 36, 255];
        project.format_version = 8;

        // Add a nested Graphic symbol ("Star") with 4 frames so we can test Precomps + T-038
        // `sync_timeline` and `LoopFrom` time remapping!
        let star_id = "symbol-star".to_owned();
        project.symbols.push(Symbol {
            id: star_id.clone(),
            name: "Star".into(),
            folder: String::new(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                guides: vec![],
                frame_count: 4,
                scripts: vec![],
                layers: vec![Layer {
                    id: "star-layer".into(),
                    name: "Star Shape".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: (0..4)
                        .map(|f| Keyframe {
                            frame: f,
                            elements: vec![Element {
                                id: "star-rect".into(),
                                name: "Box".into(),
                                transform: Transform {
                                    x: f64::from(f * 10),
                                    y: 0.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 40.0,
                                    height: 40.0,
                                    fill: [255, 200, 40, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        })
                        .collect(),
                }],
            },
        });

        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 10;
        // Layer 0 (bottom): Classic-tweened rounded rectangle with Screen blend mode + label
        root.timeline.layers[0] = Layer {
            id: "layer-bg-card".into(),
            name: "Card".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                blend_mode: BlendMode::Screen,
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    label: "intro".into(),
                    tween: Some(Easing::EaseInOut),
                    tween_kind: TweenKind::Classic,
                    elements: vec![Element {
                        id: "card-el".into(),
                        name: "Card".into(),
                        transform: Transform {
                            x: 40.0,
                            y: 50.0,
                            primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                                corner_radii: [8.0; 4],
                                corners_locked: true,
                            })),
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 120.0,
                            height: 80.0,
                            fill: [60, 140, 255, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 9,
                    elements: vec![Element {
                        id: "card-el".into(),
                        name: "Card".into(),
                        transform: Transform {
                            x: 240.0,
                            y: 110.0,
                            alpha: 0.5,
                            primitive: Some(PrimitiveOptions::Rectangle(RectangleOptions {
                                corner_radii: [8.0; 4],
                                corners_locked: true,
                            })),
                            ..Transform::default()
                        },
                        kind: ElementKind::Rectangle {
                            width: 120.0,
                            height: 80.0,
                            fill: [60, 140, 255, 255],
                            stroke: [0; 4],
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        };

        // Layer 1 (top): Timeline-synced layer (`sync_timeline = true`) with frames 0..6 empty
        // and the `Star` instance appearing at frame 6 (1-based frame 7).
        root.timeline.layers.push(Layer {
            id: "layer-synced-star".into(),
            name: "Synced Star".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                sync_timeline: true,
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    elements: vec![],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 6,
                    elements: vec![Element {
                        id: "star-inst".into(),
                        name: "StarInstance".into(),
                        transform: Transform {
                            x: 320.0,
                            y: 180.0,
                            ..Transform::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: star_id.clone(),
                            playback: InstancePlayback::Loop,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        });

        let (lottie, report) = export_lottie_value(&project, &LottieOptions::default()).unwrap();
        assert_eq!(lottie["v"], LOTTIE_VERSION);
        assert_eq!(lottie["w"], 640);
        assert_eq!(lottie["h"], 360);
        assert_eq!(lottie["fr"], 30.0);
        assert_eq!(lottie["ip"], 0);
        assert_eq!(lottie["op"], 10);
        assert_eq!(report.asset_count, 1, "Star precomp asset");
        assert_eq!(
            report.layer_count, 3,
            "Synced Star precomp + Card shape + Stage Background solid"
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        // Top layer in Lottie (`layers[0]`) is "Synced Star · StarInstance" (`ty: 0`, `ip: 6`, `op: 10`)
        // with time remapping `tm` starting at frame 6 (`6 % 4 == 2`, so `2.0 / 30.0` seconds).
        let top = &lottie["layers"][0];
        assert_eq!(top["ty"], 0);
        assert_eq!(top["ip"], 6);
        assert_eq!(top["op"], 10);
        assert_eq!(top["tm"]["a"], 1);
        assert_eq!(top["tm"]["k"][0]["t"], 6);
        assert_eq!(top["tm"]["k"][0]["s"][0], round4(2.0 / 30.0));

        // Second layer (`layers[1]`) is "Card · Card" (`ty: 4`, Screen blend mode `bm: 2`, animated `p` and `o`).
        let card = &lottie["layers"][1];
        assert_eq!(card["ty"], 4);
        assert_eq!(card["bm"], 2);
        assert_eq!(card["ks"]["p"]["a"], 1);
        assert_eq!(card["ks"]["o"]["a"], 1);

        // Marker "intro" exported at frame 0.
        assert_eq!(lottie["markers"][0]["cm"], "intro");
        assert_eq!(lottie["markers"][0]["tm"], 0);

        // Also test symbol-only export (`options.symbol = Some("Star")`).
        let (star_json, star_report) = export_lottie_json(
            &project,
            &LottieOptions {
                symbol: Some("Star".into()),
                include_background: false,
                include_hidden_layers: false,
                pretty: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        assert_eq!(star_report.name, "Star");
        assert_eq!(star_report.frame_count, 4);
        assert!(star_json.contains("\"nm\": \"Star\""));

        // Test progress callback + cancellation (`export_lottie_with_progress`), frame_range, scale,
        // dotLottie `.lottie` ZIP packaging, and HTML preview output.
        let mut seen_steps = Vec::new();
        let err = export_lottie_with_progress(&project, &LottieOptions::default(), |p| {
            seen_steps.push(p.completed_steps);
            p.completed_steps < 1
        })
        .unwrap_err();
        assert!(err.to_string().contains("cancelled"), "{err}");
        assert_eq!(seen_steps, vec![0, 1]);

        let output = TestOutputDir::new("dotlottie");
        let dir = &output.0;
        let dot_path = dir.join("banner.lottie");
        let dot_report = export_project_lottie_with_progress(
            &project,
            &dot_path,
            &LottieOptions {
                format: LottieFormat::DotLottie,
                frame_range: Some((2, 6)),
                scale: 2.0,
                precision: 2,
                html_preview: true,
                ..LottieOptions::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(dot_report.width, 1280);
        assert_eq!(dot_report.height, 720);
        assert_eq!(dot_report.in_point, 1);
        assert_eq!(dot_report.frame_count, 5);
        assert!(dot_report.bytes > 64);
        assert!(dot_report.html_preview_path.as_ref().unwrap().exists());
        let zip_bytes = fs::read(&dot_path).unwrap();
        assert_eq!(&zip_bytes[0..4], b"PK\x03\x04");
    }

    #[test]
    fn shape_tween_gradients_oval_and_script_warnings_export_cleanly() {
        let mut project = Project::blank();
        project.format_version = 8;
        let root_id = project.root.clone();
        let root = project.symbol_mut(&root_id).unwrap();
        root.timeline.frame_count = 5;
        root.timeline.scripts.push(FrameScript {
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "stop();".into(),
            },
        });
        root.timeline.layers[0] = Layer {
            id: "morph-layer".into(),
            name: "Morph".into(),
            visible: true,
            locked: false,
            properties: LayerProperties {
                blend_mode: BlendMode::Invert,
                ..LayerProperties::default()
            },
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Shape,
                    elements: vec![Element {
                        id: "shape-a".into(),
                        name: "Oval".into(),
                        transform: Transform {
                            primitive: Some(PrimitiveOptions::Oval(OvalOptions {
                                start_angle: 0.0,
                                end_angle: 270.0,
                                inner_radius: 25.0,
                                close_path: true,
                            })),
                            ..Transform::default()
                        },
                        kind: ElementKind::Ellipse {
                            width: 100.0,
                            height: 100.0,
                            fill: [255, 80, 80, 255],
                            stroke: [255, 255, 255, 255],
                            stroke_width: 3.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
                Keyframe {
                    frame: 4,
                    elements: vec![Element {
                        id: "shape-b".into(),
                        name: "GradientPath".into(),
                        transform: Transform::default(),
                        kind: ElementKind::PaintedPath {
                            commands: vec![
                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                PathCommand::LineTo { x: 120.0, y: 0.0 },
                                PathCommand::QuadraticTo {
                                    cx: 120.0,
                                    cy: 120.0,
                                    x: 0.0,
                                    y: 120.0,
                                },
                                PathCommand::Close,
                            ],
                            fill_rule: FillRule::EvenOdd,
                            fill: [0, 200, 120, 255],
                            stroke: [0; 4],
                            fill_gradient: Some(GradientPaint {
                                kind: GradientKind::Linear,
                                matrix: [0.1, 0.0, 0.0, 0.1, 60.0, 60.0],
                                stops: vec![
                                    GradientStop {
                                        ratio: 0,
                                        color: [255, 0, 128, 255],
                                    },
                                    GradientStop {
                                        ratio: 255,
                                        color: [0, 200, 255, 180],
                                    },
                                ],
                                focal_point_ratio: 0.0,
                                ..Default::default()
                            }),
                            stroke_gradient: None,
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                },
            ],
        };

        let (lottie, report) = export_lottie_value(&project, &LottieOptions::default()).unwrap();
        assert_eq!(lottie["v"], LOTTIE_VERSION);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("frame script(s) are not executed by Lottie players")),
            "expected script diagnostic: {:?}",
            report.warnings
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Blend mode 'invert' is not supported by Lottie")),
            "expected blend mode diagnostic: {:?}",
            report.warnings
        );
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.kind == LottieNoteKind::NotExported
                    && n.message.contains("Blend mode 'invert'")),
            "expected structured NotExported note: {:?}",
            report.notes
        );
        assert!(report.not_exported_count >= 2);
    }

    #[test]
    fn html_preview_escapes_script_tags_and_image_formats_transcode_and_dotlottie_preview_writes_folder_assets()
     {
        let mut project = Project::blank();
        project.format_version = 8;
        project.name =
            "Literal </script><script>globalThis.__t051_marker=true</script> name".into();

        // Create a valid 2x2 translucent PNG resource
        let mut png_bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png_bytes)
            .write_image(
                &[
                    255, 0, 0, 255, // opaque red
                    0, 255, 0, 128, // translucent green
                    0, 0, 255, 255, // opaque blue
                    255, 255, 0, 255,
                ],
                2,
                2,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes("red", "red.png", "image/png", &png_bytes)
                .unwrap(),
        );
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "bitmap".into(),
                name: "red".into(),
                kind: ElementKind::Bitmap {
                    resource_id: "red".into(),
                    width: 2,
                    height: 2,
                    smoothing: false,
                },
                ..Element::default()
            });
        project.validate().unwrap();

        let output = TestOutputDir::new("images");
        let dir = &output.0;

        // T-057: HTML preview escapes `</script>` and title HTML entities
        let title_out = dir.join("title.json");
        let title_report = export_project_lottie(
            &project,
            &title_out,
            &LottieOptions {
                html_preview: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        let html = fs::read_to_string(title_report.html_preview_path.unwrap()).unwrap();
        assert!(
            !html.contains("</script><script>"),
            "HTML preview must not contain unescaped closing </script> inside title or script data"
        );
        assert!(
            html.contains(
                "&lt;/script&gt;&lt;script&gt;globalThis.__t051_marker=true&lt;/script&gt;"
            ),
            "Title and meta should HTML-escape angle brackets"
        );
        assert!(
            html.contains("\\u003c/script\\u003e\\u003cscript\\u003e"),
            "Inline script JSON should escape < and > as \\u003c and \\u003e"
        );

        // T-058: PNG -> JPEG in Folder and Embed modes produces real JPEG bytes (\xFF\xD8\xFF)
        let folder_jpeg_out = dir.join("folder-jpeg.json");
        let folder_jpeg_report = export_project_lottie(
            &project,
            &folder_jpeg_out,
            &LottieOptions {
                image_mode: LottieImageMode::Folder,
                image_format: LottieImageFormat::Jpeg,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        let written_jpg = fs::read(dir.join("images/img_red.jpg")).unwrap();
        assert!(
            written_jpg.starts_with(b"\xFF\xD8\xFF"),
            "Folder JPEG output must have JPEG magic bytes, got: {:?}",
            &written_jpg[..written_jpg.len().min(8)]
        );
        assert!(
            folder_jpeg_report
                .notes
                .iter()
                .any(|n| n.kind == LottieNoteKind::Approximated
                    && n.message.contains("JPEG does not support")),
            "Translucent PNG exported as JPEG should record an Approximated note"
        );

        let (embed_jpeg_val, _) = export_lottie_value(
            &project,
            &LottieOptions {
                image_mode: LottieImageMode::Embed,
                image_format: LottieImageFormat::Jpeg,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        let data_uri = embed_jpeg_val["assets"][0]["p"].as_str().unwrap();
        assert!(
            data_uri.starts_with("data:image/jpeg;base64,"),
            "Embed JPEG must use data:image/jpeg;base64, prefix, got: {data_uri}"
        );
        let decoded_embed = base64::engine::general_purpose::STANDARD
            .decode(data_uri.trim_start_matches("data:image/jpeg;base64,"))
            .unwrap();
        assert!(
            decoded_embed.starts_with(b"\xFF\xD8\xFF"),
            "Embed JPEG base64 payload must decode to JPEG magic bytes"
        );

        // T-059: dotLottie + Images Folder + HTML preview writes external images alongside .preview.html
        let dot_dir = dir.join("dotlottie-sub");
        let dot_out = dot_dir.join("folder.lottie");
        let dot_report = export_project_lottie(
            &project,
            &dot_out,
            &LottieOptions {
                format: LottieFormat::DotLottie,
                image_mode: LottieImageMode::Folder,
                image_format: LottieImageFormat::Original,
                html_preview: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        assert!(dot_report.html_preview_path.unwrap().exists());
        let loose_png = dot_dir.join("images/img_red.png");
        assert!(
            loose_png.exists(),
            "dotLottie with Folder images + HTML preview must also write images/img_red.png next to .preview.html"
        );
        assert!(
            fs::read(&loose_png)
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
    }

    #[test]
    fn large_static_timeline_estimate_is_fast_and_cancellable() {
        let mut p = Project::blank();
        p.name = "Lottie UI estimate 50s 150 shapes".into();
        p.format_version = 8;
        let id = p.root.clone();
        let t = &mut p.symbol_mut(&id).unwrap().timeline;
        t.frame_count = 1200;
        t.layers[0].keyframes[0].elements = (0..150)
            .map(|i| Element {
                id: format!("shape-{i}"),
                name: format!("Rectangle {i}"),
                kind: ElementKind::Rectangle {
                    width: 10.0,
                    height: 10.0,
                    fill: [255, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            })
            .collect();
        p.validate().unwrap();

        let start = std::time::Instant::now();
        let (json, report) = export_lottie_json(&p, &LottieOptions::default()).unwrap();
        let elapsed_ms = start.elapsed().as_millis();
        assert_eq!(report.frame_count, 1200);
        assert_eq!(report.layer_count, 151); // 150 shapes + 1 background
        assert!(json.len() > 100_000);
        assert!(
            elapsed_ms < 250,
            "1200 frames x 150 static rectangles should export in <250ms even in debug build, took {elapsed_ms}ms"
        );

        let err = export_lottie_json_with_progress(&p, &LottieOptions::default(), |prog| {
            prog.completed_steps < 1
        })
        .unwrap_err();
        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn midnight_express_multi_scene_cartoon_stress_test() {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder};
        use neoflash_core::{
            AuthoredPlayback, BlendMode, Easing, Element, ElementKind, FillRule, FilterSpec,
            FrameRange, FrameSound, GradientKind, GradientPaint, GradientStop, InstancePlayback,
            Keyframe, Layer, LayerProperties, LayerRole, PathCommand, PrimitiveOptions, Project,
            RectangleOptions, ResourceUnit, ShapeHint, SoundEffect, SoundSync, Symbol, SymbolKind,
            Timeline, TimelineEdit, Transform, TweenKind, TweenProperty, edit_frames, evaluate,
            flatten_scenes_with_ranges,
        };

        // 1. Build a valid 8 kHz mono 16-bit PCM WAV whistle/chug audio resource (0.5s = 4000 samples).
        let sample_rate = 8000u32;
        let num_samples = 4000u32;
        let data_bytes = num_samples * 2;
        let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let chord = ((t * 440.0 * std::f64::consts::TAU).sin()
                + 0.6 * (t * 554.37 * std::f64::consts::TAU).sin())
                * 0.4;
            let s = (chord * 32767.0).clamp(-32768.0, 32767.0) as i16;
            wav.extend_from_slice(&s.to_le_bytes());
        }

        // 2. Build a 16x16 neon billboard PNG resource.
        let mut png_bytes = Vec::new();
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for y in 0..16u8 {
            for x in 0..16u8 {
                let border = !(2..14).contains(&x) || !(2..14).contains(&y);
                if border {
                    rgba.extend_from_slice(&[0, 240, 255, 255]);
                } else {
                    rgba.extend_from_slice(&[24, 18, 56, 220]);
                }
            }
        }
        PngEncoder::new(&mut png_bytes)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .unwrap();

        let mut p = Project::blank();
        p.format_version = 9;
        p.name = "Midnight Express — Classic Flash Cartoon".into();
        p.width = 960;
        p.height = 540;
        p.fps = 24;
        p.background = [12, 16, 34, 255];
        p.resources.push(
            ResourceUnit::from_bytes("res-whistle", "audio/whistle.wav", "audio/wav", &wav)
                .unwrap(),
        );
        p.resources.push(
            ResourceUnit::from_bytes(
                "res-billboard",
                "images/billboard.png",
                "image/png",
                &png_bytes,
            )
            .unwrap(),
        );

        // 3. Nested Graphic symbol: `sym-train` (6 frames, looping locomotive with pulsing headlight).
        let train_symbol = Symbol {
            id: "sym-train".into(),
            name: "Locomotive".into(),
            folder: "Vehicles".into(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 6,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "train-body-layer".into(),
                    name: "Train Body".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: (0..6)
                        .map(|f| Keyframe {
                            frame: f,
                            elements: vec![
                                Element {
                                    id: "cabin".into(),
                                    name: "Cabin".into(),
                                    transform: Transform {
                                        x: -60.0,
                                        y: -24.0,
                                        primitive: Some(PrimitiveOptions::Rectangle(
                                            RectangleOptions {
                                                corner_radii: [8.0, 8.0, 4.0, 4.0],
                                                corners_locked: false,
                                            },
                                        )),
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 120.0,
                                        height: 44.0,
                                        fill: [220, 52, 74, 255],
                                        stroke: [255, 215, 110, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "headlight".into(),
                                    name: "Headlight".into(),
                                    transform: Transform {
                                        x: 52.0 + f as f64 * 4.0,
                                        y: -14.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Ellipse {
                                        width: 18.0 + f as f64 * 2.0,
                                        height: 18.0 + f as f64 * 2.0,
                                        fill: [255, 244, 160, 255],
                                        stroke: [255, 180, 40, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        })
                        .collect(),
                }],
            },
        };

        // 4. Nested MovieClip symbol: `sym-drone` (4 frames, glowing escort drone).
        let drone_symbol = Symbol {
            id: "sym-drone".into(),
            name: "Escort Drone".into(),
            folder: "Vehicles".into(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 4,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "drone-layer".into(),
                    name: "Drone".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            elements: vec![Element {
                                id: "drone-hull".into(),
                                name: "Hull".into(),
                                filters: vec![FilterSpec::Glow {
                                    color: 0x00F0FF,
                                    alpha: 0.85,
                                    blur_x: 12.0,
                                    blur_y: 12.0,
                                    strength: 2.0,
                                    quality: 2,
                                    inner: false,
                                    knockout: false,
                                }],
                                kind: ElementKind::Ellipse {
                                    width: 48.0,
                                    height: 22.0,
                                    fill: [40, 225, 255, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 3,
                            elements: vec![Element {
                                id: "drone-hull".into(),
                                name: "Hull".into(),
                                transform: Transform {
                                    y: -8.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 48.0,
                                    height: 22.0,
                                    fill: [40, 225, 255, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                }],
            },
        };

        p.library_folders = vec!["Vehicles".into()];
        p.symbols.push(train_symbol);
        p.symbols.push(drone_symbol);

        // 5. Scene 1 (`p.root`): "Night Station" (24 frames).
        let scene1_id = p.root.clone();
        {
            let scene1 = p.symbol_mut(&scene1_id).unwrap();
            scene1.name = "Night Station".into();
            scene1.timeline.frame_count = 24;
            scene1.timeline.layers = vec![
                // Layer 0: Night Sky Gradient Backdrop + Moon + Platform
                Layer {
                    id: "s1-backdrop".into(),
                    name: "Night Sky Backdrop".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "station_start".into(),
                        elements: vec![
                            Element {
                                id: "sky-grad".into(),
                                name: "Sky".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 540.0 },
                                        PathCommand::LineTo { x: 0.0, y: 540.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [16, 22, 48, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.3, 0.0, 0.0, 0.3, 480.0, 270.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [10, 14, 36, 255],
                                            },
                                            GradientStop {
                                                ratio: 165,
                                                color: [28, 42, 88, 255],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [62, 38, 82, 255],
                                            },
                                        ],
                                        focal_point_ratio: 0.0,
                                        ..Default::default()
                                    }),
                                    stroke_gradient: None,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "full-moon".into(),
                                name: "Moon".into(),
                                transform: Transform {
                                    x: 760.0,
                                    y: 64.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 84.0,
                                    height: 84.0,
                                    fill: [255, 246, 200, 255],
                                    stroke: [255, 225, 130, 180],
                                    stroke_width: 4.0,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "platform-deck".into(),
                                name: "Platform".into(),
                                transform: Transform {
                                    x: 0.0,
                                    y: 440.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 960.0,
                                    height: 100.0,
                                    fill: [24, 28, 44, 255],
                                    stroke: [90, 110, 160, 255],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    }],
                },
                // Layer 1: Masked Clouds (revealed by the animated Searchlight Mask above it)
                Layer {
                    id: "s1-lit-clouds".into(),
                    name: "Searchlight Clouds".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        mask: Some("s1-searchlight-mask".into()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "cloud-band".into(),
                            name: "Illuminated Clouds".into(),
                            transform: Transform {
                                x: 120.0,
                                y: 90.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 680.0,
                                height: 140.0,
                                fill: [110, 220, 255, 220],
                                stroke: [255, 255, 255, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 2: Animated Searchlight Mask (sweeps across the sky)
                Layer {
                    id: "s1-searchlight-mask".into(),
                    name: "Searchlight Mask".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            elements: vec![Element {
                                id: "beam-cone".into(),
                                name: "Beam".into(),
                                transform: Transform {
                                    x: 120.0,
                                    y: 80.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 180.0,
                                    height: 160.0,
                                    fill: [255, 255, 255, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 23,
                            elements: vec![Element {
                                id: "beam-cone".into(),
                                name: "Beam".into(),
                                transform: Transform {
                                    x: 560.0,
                                    y: 80.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 180.0,
                                    height: 160.0,
                                    fill: [255, 255, 255, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 3: Curved Railway Motion Guide
                Layer {
                    id: "s1-track-guide".into(),
                    name: "Track Guide".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Guide,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "guide-rail".into(),
                            name: "Rail".into(),
                            kind: ElementKind::Path {
                                commands: vec![
                                    PathCommand::MoveTo { x: 80.0, y: 410.0 },
                                    PathCommand::QuadraticTo {
                                        cx: 360.0,
                                        cy: 320.0,
                                        x: 540.0,
                                        y: 380.0,
                                    },
                                    PathCommand::QuadraticTo {
                                        cx: 720.0,
                                        cy: 430.0,
                                        x: 880.0,
                                        y: 360.0,
                                    },
                                ],
                                fill_rule: FillRule::NonZero,
                                fill: [0, 0, 0, 0],
                                stroke: [120, 200, 255, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 4: Guided Express Train (`orient_to_path` + `snap_to_guide` + Graphic `Loop`)
                Layer {
                    id: "s1-express-train".into(),
                    name: "Express Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        motion_guide: Some("s1-track-guide".into()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseIn),
                            orient_to_path: true,
                            snap_to_guide: true,
                            elements: vec![Element {
                                id: "train-inst".into(),
                                name: "Train".into(),
                                transform: Transform {
                                    x: 80.0,
                                    y: 410.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 23,
                            elements: vec![Element {
                                id: "train-inst".into(),
                                name: "Train".into(),
                                transform: Transform {
                                    x: 880.0,
                                    y: 360.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 5: Shape Tween Steam Puff with ShapeHints + Color/Alpha Morph
                Layer {
                    id: "s1-steam-morph".into(),
                    name: "Steam Puff Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [0.0, 0.5],
                                end_uv: [0.0, 0.5],
                            }],
                            elements: vec![Element {
                                id: "steam-puff".into(),
                                name: "Steam".into(),
                                transform: Transform {
                                    x: 160.0,
                                    y: 330.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 36.0,
                                    height: 28.0,
                                    fill: [240, 248, 255, 230],
                                    stroke: [180, 220, 255, 200],
                                    stroke_width: 2.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 23,
                            elements: vec![Element {
                                id: "steam-puff".into(),
                                name: "Steam".into(),
                                transform: Transform {
                                    x: 520.0,
                                    y: 190.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 140.0,
                                    height: 54.0,
                                    fill: [130, 170, 230, 90],
                                    stroke: [120, 180, 255, 60],
                                    stroke_width: 5.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 6: Motion Tween Neon Station Sign with ◆ property keyframe + Screen Blend Mode
                Layer {
                    id: "s1-neon-sign".into(),
                    name: "Neon Sign Glow".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        blend_mode: BlendMode::Screen,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Motion,
                            motion_path: None,
                            elements: vec![Element {
                                id: "neon-banner".into(),
                                name: "Neon Banner".into(),
                                transform: Transform {
                                    x: 240.0,
                                    y: 56.0,
                                    alpha: 0.75,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "SCENE 1 · MIDNIGHT STATION".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 26.0,
                                    color: [255, 90, 200, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 12,
                            keyed_properties: Some(vec![
                                TweenProperty::X,
                                TweenProperty::Y,
                                TweenProperty::Alpha,
                            ]),
                            elements: vec![Element {
                                id: "neon-banner".into(),
                                name: "Neon Banner".into(),
                                transform: Transform {
                                    x: 280.0,
                                    y: 48.0,
                                    alpha: 1.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "SCENE 1 · MIDNIGHT STATION".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 26.0,
                                    color: [255, 90, 200, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 7: Sound Layer (Stream whistle + stereo envelope)
                Layer {
                    id: "s1-audio".into(),
                    name: "Whistle Audio".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        sound: Some(FrameSound {
                            resource: "res-whistle".into(),
                            sync: SoundSync::Stream,
                            loops: 1,
                            loop_forever: false,
                            effect: SoundEffect::Custom,
                            in_point: Some(0.0),
                            out_point: None,
                            envelope: Some(vec![[0.0, 0.3, 0.9], [0.4, 0.9, 0.4]]),
                        }),
                        ..Keyframe::default()
                    }],
                },
            ];
        }

        // 6. Scene 2: "Neon Bridge Chase" (24 frames), exercising T-055 (synced layer in Scene 2)
        // and T-056 (reversed Flash easing curve).
        let scene2 = Symbol {
            id: "scene-bridge".into(),
            name: "Neon Bridge Chase".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 24,
                scripts: vec![],
                guides: vec![],
                layers: vec![
                    Layer {
                        id: "s2-bridge".into(),
                        name: "Bridge Horizon".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            label: "bridge_chase".into(),
                            elements: vec![
                                Element {
                                    id: "bridge-deck".into(),
                                    name: "Bridge Deck".into(),
                                    transform: Transform {
                                        x: 0.0,
                                        y: 400.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 960.0,
                                        height: 36.0,
                                        fill: [50, 62, 96, 255],
                                        stroke: [0, 230, 255, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "neon-billboard".into(),
                                    name: "Billboard".into(),
                                    transform: Transform {
                                        x: 740.0,
                                        y: 180.0,
                                        scale_x: 6.0,
                                        scale_y: 6.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Bitmap {
                                        resource_id: "res-billboard".into(),
                                        width: 16,
                                        height: 16,
                                        smoothing: true,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        }],
                    },
                    // Synced layer in Scene 2 with delayed placement at scene-local frame 4 (T-055!)
                    Layer {
                        id: "s2-synced-train".into(),
                        name: "Synced Bridge Train".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            sync_timeline: true,
                            ..LayerProperties::default()
                        },
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                elements: vec![],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 4,
                                tween: Some(Easing::Linear),
                                elements: vec![Element {
                                    id: "s2-train-inst".into(),
                                    name: "Bridge Train".into(),
                                    transform: Transform {
                                        x: 140.0,
                                        y: 388.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 23,
                                elements: vec![Element {
                                    id: "s2-train-inst".into(),
                                    name: "Bridge Train".into(),
                                    transform: Transform {
                                        x: 820.0,
                                        y: 388.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Drone Escort layer whose tween we reverse via TimelineEdit::Reverse (T-056!)
                    Layer {
                        id: "s2-drone-escort".into(),
                        name: "Drone Escort".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::Flash { acceleration: 70.0 }),
                                elements: vec![Element {
                                    id: "s2-drone".into(),
                                    name: "Drone".into(),
                                    transform: Transform {
                                        x: 120.0,
                                        y: 160.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-drone".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 23,
                                elements: vec![Element {
                                    id: "s2-drone".into(),
                                    name: "Drone".into(),
                                    transform: Transform {
                                        x: 780.0,
                                        y: 140.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-drone".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                ],
            },
        };
        p.symbols.push(scene2);
        p.scenes = vec![scene1_id.clone(), "scene-bridge".into()];
        p.upgrade_format_for_features();
        p.validate().unwrap();

        // Exercise TimelineEdit::Reverse on Scene 2's Drone Escort layer and verify T-056 easing inversion!
        edit_frames(
            &mut p,
            "scene-bridge",
            TimelineEdit::Reverse {
                range: FrameRange {
                    layers: vec!["s2-drone-escort".into()],
                    start: 0,
                    end: 23,
                },
            },
        )
        .unwrap();
        assert_eq!(
            p.symbol("scene-bridge").unwrap().timeline.layers[2].keyframes[0].tween,
            Some(Easing::Flash {
                acceleration: -70.0
            })
        );

        // Verify Scene 1 and Scene 2 flattened evaluation matches authored scene-local evaluation on every frame (T-055).
        let (flat, ranges) = flatten_scenes_with_ranges(&p);
        flat.validate().unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[1].start, 24);
        for f in 0..24 {
            let s1_authored = evaluate(&p, &scene1_id, f);
            let s1_flat = evaluate(&flat, &flat.root, ranges[0].start + f);
            assert_eq!(s1_authored.len(), s1_flat.len(), "Scene 1 frame {f}");
            let s2_authored = evaluate(&p, "scene-bridge", f);
            let s2_flat = evaluate(&flat, &flat.root, ranges[1].start + f);
            assert_eq!(s2_authored.len(), s2_flat.len(), "Scene 2 frame {f}");
            for (a, b) in s2_authored.iter().zip(s2_flat.iter()) {
                assert_eq!(
                    a.transform, b.transform,
                    "Scene 2 frame {f} item {} transform mismatch",
                    a.element_id
                );
            }
        }

        // Verify sound playback emits the Stream whistle command on frame 0.
        let mut playback = AuthoredPlayback::new(&flat, &flat.root, 0);
        let initial_sounds = playback.drain_sound_commands();
        assert_eq!(initial_sounds.len(), 1);

        // Normal regressions own temporary outputs and never rewrite authored examples.
        // Set NEOFLASH_EXPORT_CARTOON_HTML5=1 for real-runtime publication artifacts.
        let output = TestOutputDir::new("midnight-express");
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let export_html5 = std::env::var_os("NEOFLASH_EXPORT_CARTOON_HTML5").is_some();
        let qa_dir = if export_html5 {
            repo_root.join("target/artifacts/generated-examples/midnight-express")
        } else {
            output.0.clone()
        };
        fs::create_dir_all(&qa_dir).unwrap();
        let lottie_json_path = qa_dir.join("midnight-express.lottie.json");
        let json_report = export_project_lottie(
            &p,
            &lottie_json_path,
            &LottieOptions {
                format: LottieFormat::Json,
                image_mode: LottieImageMode::Folder,
                image_format: LottieImageFormat::Jpeg,
                html_preview: true,
                scale: 1.5,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        assert_eq!(json_report.width, 1440);
        assert_eq!(json_report.height, 810);
        assert_eq!(json_report.frame_count, 48);
        assert!(lottie_json_path.exists());
        assert!(json_report.html_preview_path.unwrap().exists());
        assert!(
            qa_dir.join("images/img_res-billboard.jpg").exists(),
            "Billboard PNG should be transcoded to JPEG in Folder mode"
        );
        let dotlottie_path = qa_dir.join("midnight-express.lottie");
        let dot_report = export_project_lottie(
            &p,
            &dotlottie_path,
            &LottieOptions {
                format: LottieFormat::DotLottie,
                image_mode: LottieImageMode::Embed,
                image_format: LottieImageFormat::Png,
                html_preview: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        assert!(dotlottie_path.exists());
        assert!(dot_report.html_preview_path.unwrap().exists());
        let web_runtime = repo_root.join("target/web-runtime");
        if export_html5 {
            assert!(
                web_runtime.join("neoflash_web_bg.wasm").is_file(),
                "NEOFLASH_EXPORT_CARTOON_HTML5 requires scripts/build-web.sh first"
            );
            let html5_dir = qa_dir.join("html5");
            crate::export_project_with_options(
                &p,
                &html5_dir,
                &web_runtime,
                &crate::PublishOptions::default(),
            )
            .unwrap();
            assert!(html5_dir.join("index.html").exists());
        }
        let parsed_lottie: Value =
            serde_json::from_str(&fs::read_to_string(&lottie_json_path).unwrap()).unwrap();
        let bg_layer = parsed_lottie["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["nm"] == "Stage Background")
            .unwrap();
        assert_eq!(
            bg_layer["sw"], 960,
            "Stage Background must use unscaled stage width because it is parented to Stage Scale (Bug C)"
        );
        let masked_clouds = parsed_lottie["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["nm"] == "Searchlight Clouds · Illuminated Clouds")
            .unwrap();
        assert_eq!(
            masked_clouds["masksProperties"][0]["pt"]["a"], 1,
            "Animated Searchlight Mask must export animated mask path keyframes (Bug B)"
        );
        let steam_layer = parsed_lottie["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["nm"] == "Steam Puff Morph · Steam")
            .unwrap();
        let steam_items = steam_layer["shapes"][0]["it"].as_array().unwrap();
        let steam_fill = steam_items.iter().find(|it| it["ty"] == "fl").unwrap();
        assert_eq!(
            steam_fill["c"]["a"], 1,
            "Shape tween with changing fill color must animate Lottie fill color (Bug D)"
        );
        assert_eq!(
            steam_fill["o"]["a"], 1,
            "Shape tween with changing fill alpha must animate Lottie fill opacity (Bug D)"
        );
    }

    #[test]
    fn midnight_express_episode_3_starlight_morph_cartoon() {
        use crate::video::{VideoCodec, VideoFormat, VideoOptions, export_project_video};
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder};
        use neoflash_core::{
            AuthoredPlayback, BlendMode, Easing, Element, ElementKind, FillRule, FilterSpec,
            FrameSound, GradientKind, GradientPaint, GradientStop, InstancePlayback, Keyframe,
            Layer, LayerProperties, LayerRole, PathCommand, PrimitiveOptions, Project,
            RectangleOptions, ResourceUnit, ShapeHint, SoundEffect, SoundSync, Symbol, SymbolKind,
            Timeline, Transform, TweenKind, TweenProperty, evaluate, flatten_scenes_with_ranges,
        };
        use std::f64::consts::TAU;

        // Helper 1: Crisp Crescent Moon contour around (cx, cy) with radius r.
        fn crescent_moon_commands(cx: f64, cy: f64, r: f64) -> Vec<PathCommand> {
            vec![
                PathCommand::MoveTo {
                    x: cx - r * 0.15,
                    y: cy - r,
                },
                PathCommand::QuadraticTo {
                    cx: cx + r * 0.95,
                    cy: cy - r * 0.88,
                    x: cx + r * 0.95,
                    y: cy,
                },
                PathCommand::QuadraticTo {
                    cx: cx + r * 0.95,
                    cy: cy + r * 0.88,
                    x: cx - r * 0.15,
                    y: cy + r,
                },
                PathCommand::QuadraticTo {
                    cx: cx + r * 0.42,
                    cy: cy + r * 0.52,
                    x: cx + r * 0.42,
                    y: cy,
                },
                PathCommand::QuadraticTo {
                    cx: cx + r * 0.42,
                    cy: cy - r * 0.52,
                    x: cx - r * 0.15,
                    y: cy - r,
                },
                PathCommand::Close,
            ]
        }

        // Helper 2: N-pointed Star Beacon contour around (cx, cy).
        fn star_beacon_commands(
            cx: f64,
            cy: f64,
            outer_r: f64,
            inner_r: f64,
            points: usize,
        ) -> Vec<PathCommand> {
            let mut cmds = Vec::with_capacity(points * 2 + 2);
            let total = points * 2;
            for i in 0..total {
                let angle = -std::f64::consts::FRAC_PI_2 + (i as f64) * TAU / (total as f64);
                let rad = if i % 2 == 0 { outer_r } else { inner_r };
                let x = cx + rad * angle.cos();
                let y = cy + rad * angle.sin();
                if i == 0 {
                    cmds.push(PathCommand::MoveTo { x, y });
                } else {
                    cmds.push(PathCommand::LineTo { x, y });
                }
            }
            cmds.push(PathCommand::Close);
            cmds
        }

        // Helper 3: Winged Comet Crest contour around (cx, cy).
        fn winged_comet_commands(cx: f64, cy: f64, s: f64) -> Vec<PathCommand> {
            vec![
                PathCommand::MoveTo {
                    x: cx + 78.0 * s,
                    y: cy,
                },
                PathCommand::QuadraticTo {
                    cx: cx + 44.0 * s,
                    cy: cy - 38.0 * s,
                    x: cx - 12.0 * s,
                    y: cy - 52.0 * s,
                },
                PathCommand::QuadraticTo {
                    cx: cx + 6.0 * s,
                    cy: cy - 18.0 * s,
                    x: cx - 88.0 * s,
                    y: cy - 22.0 * s,
                },
                PathCommand::QuadraticTo {
                    cx: cx - 42.0 * s,
                    cy,
                    x: cx - 96.0 * s,
                    y: cy + 8.0 * s,
                },
                PathCommand::QuadraticTo {
                    cx: cx + 4.0 * s,
                    cy: cy + 16.0 * s,
                    x: cx - 14.0 * s,
                    y: cy + 50.0 * s,
                },
                PathCommand::QuadraticTo {
                    cx: cx + 44.0 * s,
                    cy: cy + 36.0 * s,
                    x: cx + 78.0 * s,
                    y: cy,
                },
                PathCommand::Close,
            ]
        }

        // Helper 4: Fluffy Cloud Puff contour around (cx, cy).
        fn cloud_puff_commands(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathCommand> {
            vec![
                PathCommand::MoveTo {
                    x: cx - rx,
                    y: cy + ry * 0.2,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx * 0.95,
                    cy: cy - ry * 0.65,
                    x: cx - rx * 0.35,
                    y: cy - ry * 0.6,
                },
                PathCommand::QuadraticTo {
                    cx,
                    cy: cy - ry * 1.25,
                    x: cx + rx * 0.4,
                    y: cy - ry * 0.6,
                },
                PathCommand::QuadraticTo {
                    cx: cx + rx * 1.0,
                    cy: cy - ry * 0.55,
                    x: cx + rx,
                    y: cy + ry * 0.15,
                },
                PathCommand::QuadraticTo {
                    cx: cx + rx * 0.9,
                    cy: cy + ry * 0.85,
                    x: cx + rx * 0.25,
                    y: cy + ry * 0.75,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx * 0.3,
                    cy: cy + ry * 0.95,
                    x: cx - rx,
                    y: cy + ry * 0.2,
                },
                PathCommand::Close,
            ]
        }

        // Helper 5: Celestial Sky-Whale Spirit contour around (cx, cy).
        fn sky_whale_commands(cx: f64, cy: f64, s: f64) -> Vec<PathCommand> {
            vec![
                // Snout tip (right)
                PathCommand::MoveTo {
                    x: cx + 115.0 * s,
                    y: cy + 6.0 * s,
                },
                // Forehead & arched dorsal back
                PathCommand::QuadraticTo {
                    cx: cx + 105.0 * s,
                    cy: cy - 46.0 * s,
                    x: cx + 22.0 * s,
                    y: cy - 38.0 * s,
                },
                // Dorsal fin
                PathCommand::QuadraticTo {
                    cx: cx - 6.0 * s,
                    cy: cy - 68.0 * s,
                    x: cx - 22.0 * s,
                    y: cy - 30.0 * s,
                },
                // Tail stock to upper fluke
                PathCommand::QuadraticTo {
                    cx: cx - 68.0 * s,
                    cy: cy - 16.0 * s,
                    x: cx - 118.0 * s,
                    y: cy - 48.0 * s,
                },
                // Tail notch
                PathCommand::QuadraticTo {
                    cx: cx - 94.0 * s,
                    cy: cy - 6.0 * s,
                    x: cx - 122.0 * s,
                    y: cy + 32.0 * s,
                },
                // Lower tail stock & belly
                PathCommand::QuadraticTo {
                    cx: cx - 64.0 * s,
                    cy: cy + 8.0 * s,
                    x: cx - 10.0 * s,
                    y: cy + 28.0 * s,
                },
                // Flipper fin
                PathCommand::QuadraticTo {
                    cx: cx + 14.0 * s,
                    cy: cy + 64.0 * s,
                    x: cx + 38.0 * s,
                    y: cy + 28.0 * s,
                },
                // Chin back to snout
                PathCommand::QuadraticTo {
                    cx: cx + 88.0 * s,
                    cy: cy + 30.0 * s,
                    x: cx + 115.0 * s,
                    y: cy + 6.0 * s,
                },
                PathCommand::Close,
            ]
        }

        // Helper 6: Starlight Portal Arch contour around (cx, cy).
        fn portal_arch_commands(cx: f64, cy: f64, w: f64, h: f64) -> Vec<PathCommand> {
            let hw = w * 0.5;
            let hh = h * 0.5;
            let inner_w = hw * 0.58;
            let inner_h = hh * 0.72;
            vec![
                PathCommand::MoveTo {
                    x: cx - hw,
                    y: cy + hh,
                },
                PathCommand::LineTo {
                    x: cx - hw,
                    y: cy - hh * 0.15,
                },
                PathCommand::QuadraticTo {
                    cx: cx - hw,
                    cy: cy - hh * 1.05,
                    x: cx,
                    y: cy - hh,
                },
                PathCommand::QuadraticTo {
                    cx: cx + hw,
                    cy: cy - hh * 1.05,
                    x: cx + hw,
                    y: cy - hh * 0.15,
                },
                PathCommand::LineTo {
                    x: cx + hw,
                    y: cy + hh,
                },
                PathCommand::LineTo {
                    x: cx + inner_w,
                    y: cy + hh,
                },
                PathCommand::LineTo {
                    x: cx + inner_w,
                    y: cy - hh * 0.1,
                },
                PathCommand::QuadraticTo {
                    cx,
                    cy: cy - inner_h * 1.15,
                    x: cx - inner_w,
                    y: cy - hh * 0.1,
                },
                PathCommand::LineTo {
                    x: cx - inner_w,
                    y: cy + hh,
                },
                PathCommand::Close,
            ]
        }

        // Helper 7: Aurora Ribbon Wave across the sky.
        fn aurora_ribbon_commands(
            y_left: f64,
            y_mid: f64,
            y_right: f64,
            crest: f64,
        ) -> Vec<PathCommand> {
            vec![
                PathCommand::MoveTo { x: 40.0, y: y_left },
                PathCommand::CubicTo {
                    cx1: 260.0,
                    cy1: y_mid - crest,
                    cx2: 420.0,
                    cy2: y_mid + crest * 0.8,
                    x: 480.0,
                    y: y_mid,
                },
                PathCommand::CubicTo {
                    cx1: 560.0,
                    cy1: y_mid - crest * 0.9,
                    cx2: 740.0,
                    cy2: y_right - crest * 0.5,
                    x: 920.0,
                    y: y_right,
                },
                PathCommand::LineTo {
                    x: 920.0,
                    y: y_right + 46.0,
                },
                PathCommand::CubicTo {
                    cx1: 740.0,
                    cy1: y_right + 46.0 + crest * 0.4,
                    cx2: 560.0,
                    cy2: y_mid + 52.0 - crest * 0.6,
                    x: 480.0,
                    y: y_mid + 52.0,
                },
                PathCommand::CubicTo {
                    cx1: 420.0,
                    cy1: y_mid + 52.0 + crest * 0.7,
                    cx2: 260.0,
                    cy2: y_left + 44.0 - crest * 0.8,
                    x: 40.0,
                    y: y_left + 44.0,
                },
                PathCommand::Close,
            ]
        }

        // Helper 8: Paper Lantern Diamond -> Origami Bird -> Star.
        fn lantern_diamond_commands(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<PathCommand> {
            vec![
                PathCommand::MoveTo { x: cx, y: cy - ry },
                PathCommand::QuadraticTo {
                    cx: cx + rx * 0.65,
                    cy: cy - ry * 0.35,
                    x: cx + rx,
                    y: cy,
                },
                PathCommand::QuadraticTo {
                    cx: cx + rx * 0.65,
                    cy: cy + ry * 0.35,
                    x: cx,
                    y: cy + ry,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx * 0.65,
                    cy: cy + ry * 0.35,
                    x: cx - rx,
                    y: cy,
                },
                PathCommand::QuadraticTo {
                    cx: cx - rx * 0.65,
                    cy: cy - ry * 0.35,
                    x: cx,
                    y: cy - ry,
                },
                PathCommand::Close,
            ]
        }

        fn origami_bird_commands(cx: f64, cy: f64, s: f64) -> Vec<PathCommand> {
            vec![
                // Beak (right)
                PathCommand::MoveTo {
                    x: cx + 48.0 * s,
                    y: cy - 4.0 * s,
                },
                // Head to upper wing tip
                PathCommand::LineTo {
                    x: cx + 14.0 * s,
                    y: cy - 14.0 * s,
                },
                PathCommand::LineTo {
                    x: cx - 12.0 * s,
                    y: cy - 52.0 * s,
                },
                // Wing trailing edge to tail tip
                PathCommand::LineTo {
                    x: cx - 18.0 * s,
                    y: cy - 10.0 * s,
                },
                PathCommand::LineTo {
                    x: cx - 48.0 * s,
                    y: cy - 22.0 * s,
                },
                // Lower tail & lower wing
                PathCommand::LineTo {
                    x: cx - 24.0 * s,
                    y: cy + 8.0 * s,
                },
                PathCommand::LineTo {
                    x: cx - 6.0 * s,
                    y: cy + 44.0 * s,
                },
                PathCommand::LineTo {
                    x: cx + 18.0 * s,
                    y: cy + 10.0 * s,
                },
                PathCommand::Close,
            ]
        }

        // 1. Build a rich 8 kHz mono 16-bit PCM WAV nocturnal chime + train whistle (1.5s = 12000 samples).
        let sample_rate = 8000u32;
        let num_samples = 12000u32;
        let data_bytes = num_samples * 2;
        let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let env = (1.0 - (t / 1.5) * 0.45).clamp(0.0, 1.0);
            // Dreamy major-9th nocturnal bell + train whistle harmonic chord
            let f1 = (t * 440.0 * TAU).sin();
            let f2 = (t * 554.37 * TAU).sin() * 0.65;
            let f3 = (t * 659.25 * TAU).sin() * 0.55;
            let f4 = (t * 987.77 * TAU).sin() * 0.35 * (t * 6.0 * TAU).sin().abs();
            let sample =
                ((f1 + f2 + f3 + f4) * 0.24 * env * 32767.0).clamp(-32768.0, 32767.0) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }

        // 2. Build a 16x16 Starlight Station neon emblem PNG resource.
        let mut png_bytes = Vec::new();
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for y in 0..16u8 {
            for x in 0..16u8 {
                let border = !(2..14).contains(&x) || !(2..14).contains(&y);
                let center_diamond = (x as i32 - 8).abs() + (y as i32 - 8).abs() <= 4;
                if border {
                    rgba.extend_from_slice(&[90, 245, 255, 255]);
                } else if center_diamond {
                    rgba.extend_from_slice(&[255, 220, 110, 255]);
                } else {
                    rgba.extend_from_slice(&[24, 18, 56, 230]);
                }
            }
        }
        PngEncoder::new(&mut png_bytes)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .unwrap();

        let mut p = Project::blank();
        p.format_version = 9;
        p.name = "Midnight Express 3 — The Starlight Morph".into();
        p.width = 960;
        p.height = 540;
        p.fps = 24;
        p.background = [12, 16, 36, 255];
        p.resources.push(
            ResourceUnit::from_bytes(
                "res-starlight-whistle",
                "audio/starlight_whistle.wav",
                "audio/wav",
                &wav,
            )
            .unwrap(),
        );
        p.resources.push(
            ResourceUnit::from_bytes(
                "res-billboard",
                "images/starlight_sign.png",
                "image/png",
                &png_bytes,
            )
            .unwrap(),
        );

        // 3. Nested Graphic symbol: `sym-train` (8 frames, detailed Midnight Express locomotive
        //    with cabin, boiler, smokestack, warm passenger windows, wheels, and pulsing headlight).
        let train_symbol = Symbol {
            id: "sym-train".into(),
            name: "Midnight Locomotive".into(),
            folder: "Vehicles".into(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 8,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "train-body-layer".into(),
                    name: "Locomotive Hull".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: (0..8)
                        .map(|f| {
                            let pulse = (f as f64 * TAU / 8.0).sin() * 0.5 + 0.5;
                            let bounce = if f % 2 == 0 { 0.0 } else { -1.5 };
                            Keyframe {
                                frame: f,
                                elements: vec![
                                    // Chassis undercarriage
                                    Element {
                                        id: "chassis".into(),
                                        name: "Chassis".into(),
                                        transform: Transform {
                                            x: -74.0,
                                            y: 12.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 148.0,
                                            height: 10.0,
                                            fill: [38, 48, 78, 255],
                                            stroke: [255, 210, 105, 255],
                                            stroke_width: 2.0,
                                        },
                                        ..Element::default()
                                    },
                                    // Main Cabin & Boiler Body
                                    Element {
                                        id: "cabin".into(),
                                        name: "Cabin".into(),
                                        transform: Transform {
                                            x: -68.0,
                                            y: -28.0 + bounce,
                                            primitive: Some(PrimitiveOptions::Rectangle(
                                                RectangleOptions {
                                                    corner_radii: [10.0, 10.0, 4.0, 4.0],
                                                    corners_locked: false,
                                                },
                                            )),
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 132.0,
                                            height: 44.0,
                                            fill: [215, 50, 78, 255],
                                            stroke: [255, 218, 115, 255],
                                            stroke_width: 3.0,
                                        },
                                        ..Element::default()
                                    },
                                    // Smokestack Chimney
                                    Element {
                                        id: "chimney".into(),
                                        name: "Chimney".into(),
                                        transform: Transform {
                                            x: 34.0,
                                            y: -48.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 18.0,
                                            height: 22.0,
                                            fill: [52, 74, 122, 255],
                                            stroke: [255, 218, 115, 255],
                                            stroke_width: 2.0,
                                        },
                                        ..Element::default()
                                    },
                                    // Cozy Warm Lit Passenger Window #1
                                    Element {
                                        id: "win-1".into(),
                                        name: "Window 1".into(),
                                        transform: Transform {
                                            x: -52.0,
                                            y: -18.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 26.0,
                                            height: 18.0,
                                            fill: [255, 238, 155, 255],
                                            stroke: [90, 50, 40, 255],
                                            stroke_width: 1.5,
                                        },
                                        ..Element::default()
                                    },
                                    // Cozy Warm Lit Passenger Window #2
                                    Element {
                                        id: "win-2".into(),
                                        name: "Window 2".into(),
                                        transform: Transform {
                                            x: -16.0,
                                            y: -18.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 26.0,
                                            height: 18.0,
                                            fill: [255, 238, 155, 255],
                                            stroke: [90, 50, 40, 255],
                                            stroke_width: 1.5,
                                        },
                                        ..Element::default()
                                    },
                                    // Rear Wheel
                                    Element {
                                        id: "wheel-rear".into(),
                                        name: "Rear Wheel".into(),
                                        transform: Transform {
                                            x: -48.0,
                                            y: 14.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Ellipse {
                                            width: 22.0,
                                            height: 22.0,
                                            fill: [24, 32, 58, 255],
                                            stroke: [255, 215, 110, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                    // Front Wheel
                                    Element {
                                        id: "wheel-front".into(),
                                        name: "Front Wheel".into(),
                                        transform: Transform {
                                            x: 22.0,
                                            y: 14.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Ellipse {
                                            width: 22.0,
                                            height: 22.0,
                                            fill: [24, 32, 58, 255],
                                            stroke: [255, 215, 110, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                    // Pulsing Golden Headlight
                                    Element {
                                        id: "headlight".into(),
                                        name: "Headlight".into(),
                                        transform: Transform {
                                            x: 56.0 + pulse * 6.0,
                                            y: -16.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Ellipse {
                                            width: 20.0 + pulse * 8.0,
                                            height: 20.0 + pulse * 8.0,
                                            fill: [255, 248, 175, 245],
                                            stroke: [255, 195, 55, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            }
                        })
                        .collect(),
                }],
            },
        };

        // 4. Nested MovieClip symbol: `sym-drone` (6 frames, glowing starlight escort drone).
        let drone_symbol = Symbol {
            id: "sym-drone".into(),
            name: "Starlight Escort Drone".into(),
            folder: "Vehicles".into(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 6,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "drone-layer".into(),
                    name: "Drone".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            elements: vec![Element {
                                id: "drone-hull".into(),
                                name: "Hull".into(),
                                filters: vec![FilterSpec::Glow {
                                    color: 0x5AF5FF,
                                    alpha: 0.9,
                                    blur_x: 14.0,
                                    blur_y: 14.0,
                                    strength: 2.2,
                                    quality: 2,
                                    inner: false,
                                    knockout: false,
                                }],
                                kind: ElementKind::Ellipse {
                                    width: 52.0,
                                    height: 24.0,
                                    fill: [65, 235, 255, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 5,
                            elements: vec![Element {
                                id: "drone-hull".into(),
                                name: "Hull".into(),
                                transform: Transform {
                                    y: -10.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 52.0,
                                    height: 24.0,
                                    fill: [65, 235, 255, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                }],
            },
        };

        p.library_folders = vec!["Vehicles".into()];
        p.symbols.push(train_symbol);
        p.symbols.push(drone_symbol);

        // =========================================================================
        // 5. SCENE 1 (`p.root`): "Night Station · Starlight Morph" (48 frames)
        // =========================================================================
        let scene1_id = p.root.clone();
        {
            let scene1 = p.symbol_mut(&scene1_id).unwrap();
            scene1.name = "Night Station · Starlight Morph".into();
            scene1.timeline.frame_count = 48;
            scene1.timeline.layers = vec![
                // Layer 0: Night Sky Gradient Backdrop + Viaduct Deck
                Layer {
                    id: "s1-backdrop".into(),
                    name: "Night Sky & Viaduct".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "ep3_scene1_station".into(),
                        elements: vec![
                            Element {
                                id: "sky-grad".into(),
                                name: "Night Sky".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 540.0 },
                                        PathCommand::LineTo { x: 0.0, y: 540.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [14, 20, 46, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [8, 12, 34, 255],
                                            },
                                            GradientStop {
                                                ratio: 155,
                                                color: [26, 38, 86, 255],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [72, 42, 98, 255],
                                            },
                                        ],
                                        focal_point_ratio: 0.0,
                                        ..Default::default()
                                    }),
                                    stroke_gradient: None,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "platform-deck".into(),
                                name: "Starlight Viaduct".into(),
                                transform: Transform {
                                    x: 0.0,
                                    y: 438.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 960.0,
                                    height: 102.0,
                                    fill: [20, 26, 46, 255],
                                    stroke: [95, 130, 195, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    }],
                },
                // Layer 1: Masked Illuminated Clouds
                Layer {
                    id: "s1-lit-clouds".into(),
                    name: "Searchlight Clouds".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        mask: Some("s1-searchlight-mask".into()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "cloud-band".into(),
                            name: "Illuminated Clouds".into(),
                            transform: Transform {
                                x: 60.0,
                                y: 40.0,
                                ..Transform::default()
                            },
                            kind: ElementKind::Rectangle {
                                width: 820.0,
                                height: 220.0,
                                fill: [68, 165, 235, 130],
                                stroke: [160, 235, 255, 190],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 2: Animated Searchlight Mask
                Layer {
                    id: "s1-searchlight-mask".into(),
                    name: "Searchlight Mask".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Mask,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            elements: vec![Element {
                                id: "beam-cone".into(),
                                name: "Beam".into(),
                                transform: Transform {
                                    x: 110.0,
                                    y: 72.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 190.0,
                                    height: 165.0,
                                    fill: [255, 255, 255, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "beam-cone".into(),
                                name: "Beam".into(),
                                transform: Transform {
                                    x: 610.0,
                                    y: 72.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 190.0,
                                    height: 165.0,
                                    fill: [255, 255, 255, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 3: SHAPE TWEEN MORPH #1 — Crescent Moon -> 8-Point Star Beacon -> Winged Comet
                Layer {
                    id: "s1-celestial-morph".into(),
                    name: "Moon to Star-Comet Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![
                                ShapeHint {
                                    label: 'a',
                                    start_uv: [0.5, 0.0],
                                    end_uv: [0.5, 0.0],
                                },
                                ShapeHint {
                                    label: 'b',
                                    start_uv: [0.5, 1.0],
                                    end_uv: [0.5, 1.0],
                                },
                            ],
                            elements: vec![Element {
                                id: "celestial-morph".into(),
                                name: "Crescent Moon".into(),
                                kind: ElementKind::Path {
                                    commands: crescent_moon_commands(760.0, 125.0, 62.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 240, 165, 255],
                                    stroke: [255, 195, 80, 240],
                                    stroke_width: 3.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 24,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [1.0, 0.5],
                                end_uv: [1.0, 0.5],
                            }],
                            elements: vec![Element {
                                id: "celestial-morph".into(),
                                name: "Starlight Beacon".into(),
                                kind: ElementKind::Path {
                                    commands: star_beacon_commands(650.0, 135.0, 74.0, 30.0, 8),
                                    fill_rule: FillRule::NonZero,
                                    fill: [95, 245, 255, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 4.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "celestial-morph".into(),
                                name: "Winged Comet".into(),
                                kind: ElementKind::Path {
                                    commands: winged_comet_commands(790.0, 155.0, 0.95),
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 130, 225, 255],
                                    stroke: [255, 235, 135, 255],
                                    stroke_width: 3.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 4: Curved Railway Motion Guide
                Layer {
                    id: "s1-track-guide".into(),
                    name: "Track Guide".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Guide,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "guide-rail".into(),
                            name: "Rail".into(),
                            kind: ElementKind::Path {
                                commands: vec![
                                    PathCommand::MoveTo { x: 90.0, y: 415.0 },
                                    PathCommand::QuadraticTo {
                                        cx: 360.0,
                                        cy: 330.0,
                                        x: 540.0,
                                        y: 382.0,
                                    },
                                    PathCommand::QuadraticTo {
                                        cx: 710.0,
                                        cy: 425.0,
                                        x: 865.0,
                                        y: 355.0,
                                    },
                                ],
                                fill_rule: FillRule::NonZero,
                                fill: [0, 0, 0, 0],
                                stroke: [120, 205, 255, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 5: Guided Express Train (`orient_to_path` + `snap_to_guide` + Graphic `Loop`)
                Layer {
                    id: "s1-express-train".into(),
                    name: "Express Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        motion_guide: Some("s1-track-guide".into()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            orient_to_path: true,
                            snap_to_guide: true,
                            elements: vec![Element {
                                id: "train-inst".into(),
                                name: "Train".into(),
                                transform: Transform {
                                    x: 90.0,
                                    y: 415.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "train-inst".into(),
                                name: "Train".into(),
                                transform: Transform {
                                    x: 865.0,
                                    y: 355.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 6: SHAPE TWEEN MORPH #2 — Steam Cloud Puff -> Celestial Sky-Whale -> Starlight Portal Arch
                Layer {
                    id: "s1-steam-morph".into(),
                    name: "Steam to Sky-Whale Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![
                                ShapeHint {
                                    label: 'a',
                                    start_uv: [1.0, 0.5],
                                    end_uv: [1.0, 0.5],
                                },
                                ShapeHint {
                                    label: 'b',
                                    start_uv: [0.0, 0.5],
                                    end_uv: [0.0, 0.5],
                                },
                            ],
                            elements: vec![Element {
                                id: "steam-spirit".into(),
                                name: "Steam Spirit".into(),
                                kind: ElementKind::Path {
                                    commands: cloud_puff_commands(185.0, 315.0, 52.0, 34.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [235, 248, 255, 235],
                                    stroke: [150, 220, 255, 215],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 24,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![ShapeHint {
                                label: 'a',
                                start_uv: [0.5, 0.0],
                                end_uv: [0.5, 0.0],
                            }],
                            elements: vec![Element {
                                id: "steam-spirit".into(),
                                name: "Celestial Sky-Whale".into(),
                                kind: ElementKind::Path {
                                    commands: sky_whale_commands(465.0, 225.0, 1.05),
                                    fill_rule: FillRule::NonZero,
                                    fill: [110, 222, 255, 230],
                                    stroke: [225, 252, 255, 245],
                                    stroke_width: 3.2,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "steam-spirit".into(),
                                name: "Starlight Portal Arch".into(),
                                kind: ElementKind::Path {
                                    commands: portal_arch_commands(835.0, 325.0, 135.0, 190.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [195, 125, 255, 235],
                                    stroke: [95, 245, 255, 255],
                                    stroke_width: 4.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 7: Motion Tween Neon Episode Title with ◆ property keyframe + Screen Blend Mode
                Layer {
                    id: "s1-neon-sign".into(),
                    name: "Neon Episode Title".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        blend_mode: BlendMode::Screen,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Motion,
                            motion_path: None,
                            elements: vec![Element {
                                id: "neon-banner".into(),
                                name: "Neon Banner".into(),
                                transform: Transform {
                                    x: 72.0,
                                    y: 44.0,
                                    alpha: 0.8,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "MIDNIGHT EXPRESS 3 · THE STARLIGHT MORPH".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 24.0,
                                    color: [255, 115, 220, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 24,
                            keyed_properties: Some(vec![
                                TweenProperty::X,
                                TweenProperty::Y,
                                TweenProperty::Alpha,
                            ]),
                            elements: vec![Element {
                                id: "neon-banner".into(),
                                name: "Neon Banner".into(),
                                transform: Transform {
                                    x: 96.0,
                                    y: 38.0,
                                    alpha: 1.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "MIDNIGHT EXPRESS 3 · THE STARLIGHT MORPH".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 24.0,
                                    color: [255, 115, 220, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 8: Sound Layer (Stream whistle + stereo envelope)
                Layer {
                    id: "s1-audio".into(),
                    name: "Starlight Whistle Audio".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        sound: Some(FrameSound {
                            resource: "res-starlight-whistle".into(),
                            sync: SoundSync::Stream,
                            loops: 1,
                            loop_forever: false,
                            effect: SoundEffect::Custom,
                            in_point: Some(0.0),
                            out_point: None,
                            envelope: Some(vec![[0.0, 0.35, 0.9], [0.7, 0.95, 0.45]]),
                        }),
                        ..Keyframe::default()
                    }],
                },
            ];
        }

        // =========================================================================
        // 6. SCENE 2: "Nebula Bridge & Shape Symphony" (48 frames)
        // =========================================================================
        let scene2 = Symbol {
            id: "scene-nebula-bridge".into(),
            name: "Nebula Bridge & Shape Symphony".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 48,
                scripts: vec![],
                guides: vec![],
                layers: vec![
                    // Layer 0: Cosmic Backdrop & Bridge Deck
                    Layer {
                        id: "s2-bridge".into(),
                        name: "Nebula Sky & Bridge".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            label: "ep3_scene2_nebula".into(),
                            elements: vec![
                                Element {
                                    id: "s2-sky-grad".into(),
                                    name: "Nebula Sky".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 0.0 },
                                            PathCommand::LineTo { x: 960.0, y: 540.0 },
                                            PathCommand::LineTo { x: 0.0, y: 540.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [18, 16, 48, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [12, 10, 42, 255],
                                                },
                                                GradientStop {
                                                    ratio: 150,
                                                    color: [42, 28, 92, 255],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [22, 68, 112, 255],
                                                },
                                            ],
                                            focal_point_ratio: 0.0,
                                            ..Default::default()
                                        }),
                                        stroke_gradient: None,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "bridge-deck".into(),
                                    name: "Bridge Deck".into(),
                                    transform: Transform {
                                        x: 0.0,
                                        y: 410.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 960.0,
                                        height: 38.0,
                                        fill: [42, 56, 94, 255],
                                        stroke: [0, 235, 255, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "neon-billboard".into(),
                                    name: "Starlight Sign".into(),
                                    transform: Transform {
                                        x: 780.0,
                                        y: 250.0,
                                        scale_x: 5.5,
                                        scale_y: 5.5,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Bitmap {
                                        resource_id: "res-billboard".into(),
                                        width: 16,
                                        height: 16,
                                        smoothing: true,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        }],
                    },
                    // Layer 1: SHAPE TWEEN MORPH #3 — Aurora Ribbon Wave Morph (`PaintedPath` gradient morph)
                    Layer {
                        id: "s2-aurora-morph".into(),
                        name: "Aurora Ribbon Shape Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            blend_mode: BlendMode::Screen,
                            ..LayerProperties::default()
                        },
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![Element {
                                    id: "aurora-ribbon".into(),
                                    name: "Emerald Aurora".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: aurora_ribbon_commands(115.0, 90.0, 120.0, 55.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [0, 250, 195, 195],
                                        stroke: [140, 255, 230, 210],
                                        stroke_width: 2.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.5, 0.0, 0.0, 0.2, 480.0, 110.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [0, 245, 185, 195],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [80, 175, 255, 210],
                                                },
                                            ],
                                            focal_point_ratio: 0.0,
                                            ..Default::default()
                                        }),
                                        stroke_gradient: None,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 24,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![Element {
                                    id: "aurora-ribbon".into(),
                                    name: "Magenta Crown Aurora".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: aurora_ribbon_commands(85.0, 130.0, 80.0, -65.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 110, 225, 205],
                                        stroke: [255, 225, 130, 225],
                                        stroke_width: 3.0,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.5, 0.0, 0.0, 0.2, 480.0, 110.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [255, 105, 220, 210],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [255, 215, 100, 215],
                                                },
                                            ],
                                            focal_point_ratio: 0.0,
                                            ..Default::default()
                                        }),
                                        stroke_gradient: None,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "aurora-ribbon".into(),
                                    name: "Starlight Halo Aurora".into(),
                                    kind: ElementKind::PaintedPath {
                                        commands: aurora_ribbon_commands(105.0, 75.0, 110.0, 48.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [110, 220, 255, 200],
                                        stroke: [255, 255, 255, 220],
                                        stroke_width: 2.5,
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            matrix: [0.5, 0.0, 0.0, 0.2, 480.0, 110.0],
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [110, 235, 255, 200],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [195, 130, 255, 210],
                                                },
                                            ],
                                            focal_point_ratio: 0.0,
                                            ..Default::default()
                                        }),
                                        stroke_gradient: None,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 2: SHAPE TWEEN MORPH #4 — Paper Lanterns -> Origami Star-Birds -> 4-Point Stars
                    Layer {
                        id: "s2-lantern-birds-morph".into(),
                        name: "Lantern to Star-Bird Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                shape_hints: vec![ShapeHint {
                                    label: 'a',
                                    start_uv: [1.0, 0.5],
                                    end_uv: [1.0, 0.5],
                                }],
                                elements: vec![
                                    Element {
                                        id: "lantern-bird-1".into(),
                                        name: "Sky Lantern 1".into(),
                                        kind: ElementKind::Path {
                                            commands: lantern_diamond_commands(
                                                210.0, 240.0, 32.0, 44.0,
                                            ),
                                            fill_rule: FillRule::NonZero,
                                            fill: [255, 185, 85, 245],
                                            stroke: [255, 240, 175, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "lantern-bird-2".into(),
                                        name: "Sky Lantern 2".into(),
                                        kind: ElementKind::Path {
                                            commands: lantern_diamond_commands(
                                                350.0, 195.0, 26.0, 36.0,
                                            ),
                                            fill_rule: FillRule::NonZero,
                                            fill: [255, 150, 195, 245],
                                            stroke: [255, 235, 245, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 24,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                shape_hints: vec![ShapeHint {
                                    label: 'a',
                                    start_uv: [0.5, 0.0],
                                    end_uv: [0.5, 0.0],
                                }],
                                elements: vec![
                                    Element {
                                        id: "lantern-bird-1".into(),
                                        name: "Origami Star-Bird 1".into(),
                                        kind: ElementKind::Path {
                                            commands: origami_bird_commands(470.0, 215.0, 1.15),
                                            fill_rule: FillRule::NonZero,
                                            fill: [110, 250, 220, 245],
                                            stroke: [255, 255, 255, 255],
                                            stroke_width: 2.8,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "lantern-bird-2".into(),
                                        name: "Origami Star-Bird 2".into(),
                                        kind: ElementKind::Path {
                                            commands: origami_bird_commands(610.0, 175.0, 0.95),
                                            fill_rule: FillRule::NonZero,
                                            fill: [155, 225, 255, 245],
                                            stroke: [255, 255, 255, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![
                                    Element {
                                        id: "lantern-bird-1".into(),
                                        name: "Guiding Star 1".into(),
                                        kind: ElementKind::Path {
                                            commands: star_beacon_commands(
                                                690.0, 195.0, 44.0, 16.0, 4,
                                            ),
                                            fill_rule: FillRule::NonZero,
                                            fill: [255, 242, 155, 255],
                                            stroke: [255, 195, 85, 255],
                                            stroke_width: 3.0,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "lantern-bird-2".into(),
                                        name: "Guiding Star 2".into(),
                                        kind: ElementKind::Path {
                                            commands: star_beacon_commands(
                                                810.0, 155.0, 36.0, 13.0, 4,
                                            ),
                                            fill_rule: FillRule::NonZero,
                                            fill: [255, 210, 245, 255],
                                            stroke: [255, 255, 255, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 3: Synced Bridge Express Train
                    Layer {
                        id: "s2-synced-train".into(),
                        name: "Synced Bridge Train".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            sync_timeline: true,
                            ..LayerProperties::default()
                        },
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::Linear),
                                elements: vec![Element {
                                    id: "s2-train-inst".into(),
                                    name: "Bridge Train".into(),
                                    transform: Transform {
                                        x: 110.0,
                                        y: 382.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "s2-train-inst".into(),
                                    name: "Bridge Train".into(),
                                    transform: Transform {
                                        x: 840.0,
                                        y: 382.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 4: Escort Drone
                    Layer {
                        id: "s2-drone-escort".into(),
                        name: "Drone Escort".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::Flash { acceleration: 55.0 }),
                                elements: vec![Element {
                                    id: "s2-drone".into(),
                                    name: "Drone".into(),
                                    transform: Transform {
                                        x: 130.0,
                                        y: 165.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-drone".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "s2-drone".into(),
                                    name: "Drone".into(),
                                    transform: Transform {
                                        x: 810.0,
                                        y: 145.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-drone".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                ],
            },
        };

        // =========================================================================
        // 7. SCENE 3: "Dream Harbour Dawn" (48 frames)
        // =========================================================================
        let scene3 = Symbol {
            id: "scene-harbour-dawn".into(),
            name: "Dream Harbour Dawn".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 48,
                scripts: vec![],
                guides: vec![],
                layers: vec![
                    // Layer 0: SHAPE TWEEN MORPH #5 — Night-to-Dawn Gradient Sky Morph + Harbour Platform
                    Layer {
                        id: "s3-dawn-sky-morph".into(),
                        name: "Night-to-Dawn Sky Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                label: "ep3_scene3_dawn".into(),
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![
                                    Element {
                                        id: "s3-sky".into(),
                                        name: "Dawn Sky".into(),
                                        kind: ElementKind::PaintedPath {
                                            commands: vec![
                                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 540.0 },
                                                PathCommand::LineTo { x: 0.0, y: 540.0 },
                                                PathCommand::Close,
                                            ],
                                            fill_rule: FillRule::NonZero,
                                            fill: [24, 30, 68, 255],
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                            fill_gradient: Some(GradientPaint {
                                                kind: GradientKind::Linear,
                                                matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                                stops: vec![
                                                    GradientStop {
                                                        ratio: 0,
                                                        color: [18, 24, 58, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 155,
                                                        color: [68, 48, 108, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 255,
                                                        color: [155, 85, 125, 255],
                                                    },
                                                ],
                                                focal_point_ratio: 0.0,
                                                ..Default::default()
                                            }),
                                            stroke_gradient: None,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "s3-platform".into(),
                                        name: "Harbour Platform".into(),
                                        transform: Transform {
                                            x: 0.0,
                                            y: 432.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 960.0,
                                            height: 108.0,
                                            fill: [38, 40, 66, 255],
                                            stroke: [255, 195, 120, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![
                                    Element {
                                        id: "s3-sky".into(),
                                        name: "Dawn Sky".into(),
                                        kind: ElementKind::PaintedPath {
                                            commands: vec![
                                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 540.0 },
                                                PathCommand::LineTo { x: 0.0, y: 540.0 },
                                                PathCommand::Close,
                                            ],
                                            fill_rule: FillRule::NonZero,
                                            fill: [245, 175, 130, 255],
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                            fill_gradient: Some(GradientPaint {
                                                kind: GradientKind::Linear,
                                                matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                                stops: vec![
                                                    GradientStop {
                                                        ratio: 0,
                                                        color: [48, 72, 138, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 155,
                                                        color: [228, 122, 148, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 255,
                                                        color: [255, 212, 132, 255],
                                                    },
                                                ],
                                                focal_point_ratio: 0.0,
                                                ..Default::default()
                                            }),
                                            stroke_gradient: None,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "s3-platform".into(),
                                        name: "Harbour Platform".into(),
                                        transform: Transform {
                                            x: 0.0,
                                            y: 432.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 960.0,
                                            height: 108.0,
                                            fill: [64, 54, 78, 255],
                                            stroke: [255, 220, 145, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 1: SHAPE TWEEN MORPH #6 — Sleeping Crescent -> Dawn Star Crown -> Golden Rising Sun
                    Layer {
                        id: "s3-horizon-sun-morph".into(),
                        name: "Crescent to Sunrise Sun Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                shape_hints: vec![ShapeHint {
                                    label: 'a',
                                    start_uv: [0.5, 0.0],
                                    end_uv: [0.5, 0.0],
                                }],
                                elements: vec![Element {
                                    id: "horizon-celestial".into(),
                                    name: "Sleeping Crescent".into(),
                                    kind: ElementKind::Path {
                                        commands: crescent_moon_commands(735.0, 255.0, 56.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [215, 232, 255, 245],
                                        stroke: [140, 195, 255, 230],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "horizon-celestial".into(),
                                    name: "Rising Dawn Sun".into(),
                                    kind: ElementKind::Path {
                                        commands: star_beacon_commands(
                                            735.0, 195.0, 88.0, 66.0, 12,
                                        ),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 222, 110, 255],
                                        stroke: [255, 145, 65, 255],
                                        stroke_width: 4.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 2: Arriving Express Train (glides to a gentle stop at Dream Harbour)
                    Layer {
                        id: "s3-arrival-train".into(),
                        name: "Harbour Arrival Train".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseOut),
                                elements: vec![Element {
                                    id: "s3-train-inst".into(),
                                    name: "Harbour Train".into(),
                                    transform: Transform {
                                        x: 110.0,
                                        y: 404.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 26,
                                elements: vec![Element {
                                    id: "s3-train-inst".into(),
                                    name: "Harbour Train".into(),
                                    transform: Transform {
                                        x: 410.0,
                                        y: 404.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 3: SHAPE TWEEN MORPH #7 — Chimney Steam Puff -> Glowing Rose Heart -> Morning Star
                    Layer {
                        id: "s3-heart-steam-morph".into(),
                        name: "Chimney Heart-Steam Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseOut),
                                tween_kind: TweenKind::Shape,
                                shape_hints: vec![ShapeHint {
                                    label: 'a',
                                    start_uv: [0.5, 0.0],
                                    end_uv: [0.5, 0.0],
                                }],
                                elements: vec![Element {
                                    id: "heart-puff".into(),
                                    name: "Chimney Puff".into(),
                                    kind: ElementKind::Path {
                                        commands: cloud_puff_commands(260.0, 325.0, 32.0, 22.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [240, 248, 255, 225],
                                        stroke: [185, 225, 255, 210],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 26,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                shape_hints: vec![ShapeHint {
                                    label: 'a',
                                    start_uv: [0.5, 0.0],
                                    end_uv: [0.5, 0.0],
                                }],
                                elements: vec![Element {
                                    id: "heart-puff".into(),
                                    name: "Glowing Heart Puff".into(),
                                    kind: ElementKind::Path {
                                        commands: heart_puff_commands(445.0, 255.0, 56.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 125, 175, 245],
                                        stroke: [255, 235, 245, 255],
                                        stroke_width: 3.5,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "heart-puff".into(),
                                    name: "Morning Star Emblem".into(),
                                    kind: ElementKind::Path {
                                        commands: star_beacon_commands(455.0, 175.0, 58.0, 22.0, 6),
                                        fill_rule: FillRule::NonZero,
                                        fill: [255, 242, 150, 250],
                                        stroke: [255, 185, 95, 255],
                                        stroke_width: 3.2,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    // Layer 4: Closing Nightly Series Title
                    Layer {
                        id: "s3-closing-title".into(),
                        name: "Closing Series Title".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseOut),
                                tween_kind: TweenKind::Motion,
                                elements: vec![Element {
                                    id: "s3-title".into(),
                                    name: "Goodnight Title".into(),
                                    transform: Transform {
                                        x: 72.0,
                                        y: 52.0,
                                        alpha: 0.65,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Text {
                                        text: "DREAM HARBOUR DAWN · SEE YOU TOMORROW NIGHT".into(),
                                        font_family: "Helvetica".into(),
                                        font_size: 24.0,
                                        color: [255, 244, 195, 255],
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 24,
                                keyed_properties: Some(vec![
                                    TweenProperty::X,
                                    TweenProperty::Y,
                                    TweenProperty::Alpha,
                                ]),
                                elements: vec![Element {
                                    id: "s3-title".into(),
                                    name: "Goodnight Title".into(),
                                    transform: Transform {
                                        x: 84.0,
                                        y: 44.0,
                                        alpha: 1.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Text {
                                        text: "DREAM HARBOUR DAWN · SEE YOU TOMORROW NIGHT".into(),
                                        font_family: "Helvetica".into(),
                                        font_size: 24.0,
                                        color: [255, 244, 195, 255],
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                ],
            },
        };

        p.symbols.push(scene2);
        p.symbols.push(scene3);
        p.scenes = vec![
            scene1_id.clone(),
            "scene-nebula-bridge".into(),
            "scene-harbour-dawn".into(),
        ];
        p.upgrade_format_for_features();
        p.validate().unwrap();

        // Verify all 3 scenes flatten cleanly to 144 frames and evaluate every morphed frame!
        let (flat, ranges) = flatten_scenes_with_ranges(&p);
        flat.validate().unwrap();
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[1].start, 48);
        assert_eq!(ranges[2].start, 96);
        assert_eq!(flat.symbol(&flat.root).unwrap().timeline.frame_count, 144);

        for f in 0..144 {
            let items = evaluate(&flat, &flat.root, f);
            assert!(
                !items.is_empty(),
                "Flattened frame {f} must produce non-empty draw items"
            );
        }

        // Verify sound playback emits the Stream whistle command on frame 0.
        let mut playback = AuthoredPlayback::new(&flat, &flat.root, 0);
        let initial_sounds = playback.drain_sound_commands();
        assert_eq!(initial_sounds.len(), 1);

        // Keep Lottie coverage in the ordinary suite. Full media generation is explicit:
        // NEOFLASH_EXPORT_EPISODE3_MEDIA=1 writes MP4/GIF, and
        // NEOFLASH_EXPORT_CARTOON_HTML5=1 uses the built HTML5 runtime.
        // Neither mode rewrites examples/; requested artifacts go to target/artifacts/generated-examples/.
        let output = TestOutputDir::new("midnight-express-3");
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let export_media = std::env::var_os("NEOFLASH_EXPORT_EPISODE3_MEDIA").is_some();
        let export_html5 = std::env::var_os("NEOFLASH_EXPORT_CARTOON_HTML5").is_some();
        let out_dir = if export_media || export_html5 {
            repo_root.join("target/artifacts/generated-examples")
        } else {
            output.0.clone()
        };
        fs::create_dir_all(&out_dir).unwrap();

        let lottie_json_path = out_dir.join("midnight-express-3.lottie.json");
        let lottie_report = export_project_lottie(
            &p,
            &lottie_json_path,
            &LottieOptions {
                format: LottieFormat::Json,
                image_mode: LottieImageMode::Embed,
                image_format: LottieImageFormat::Png,
                html_preview: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        assert_eq!(lottie_report.frame_count, 144);
        assert!(lottie_json_path.exists());
        assert!(lottie_report.html_preview_path.unwrap().exists());

        // Keep multi-scene video preparation in the normal suite without encoding frames.
        let video_estimate = crate::video::estimate_video(&p, &VideoOptions::default()).unwrap();
        assert_eq!(video_estimate.frame_count, 144);
        assert_eq!(video_estimate.duration_secs, 6.0);

        let mp4_path = out_dir.join("midnight-express-3.mp4");
        let gif_path = out_dir.join("midnight-express-3.gif");
        if export_media {
            let mp4_report = export_project_video(
                &p,
                &mp4_path,
                &VideoOptions {
                    format: VideoFormat::Mp4,
                    codec: VideoCodec::H264,
                    include_audio: true,
                    ..VideoOptions::default()
                },
            )
            .unwrap();
            assert_eq!(mp4_report.frame_count, 144);
            assert!(mp4_path.exists());
        }
        if export_media {
            let gif_report = export_project_video(
                &p,
                &gif_path,
                &VideoOptions {
                    format: VideoFormat::Gif,
                    codec: VideoCodec::GifLzw,
                    include_audio: false,
                    ..VideoOptions::default()
                },
            )
            .unwrap();
            assert_eq!(gif_report.frame_count, 144);
            assert!(gif_path.exists());
        }

        let web_runtime = repo_root.join("target/web-runtime");
        if export_html5 {
            assert!(
                web_runtime.join("neoflash_web_bg.wasm").is_file(),
                "NEOFLASH_EXPORT_CARTOON_HTML5 requires scripts/build-web.sh first"
            );
            let html5_dir = out_dir.join("midnight-express-3-html5");
            crate::export_project_with_options(
                &p,
                &html5_dir,
                &web_runtime,
                &crate::PublishOptions::default(),
            )
            .unwrap();
            assert!(html5_dir.join("index.html").exists());
        }
    }

    #[test]
    fn midnight_express_episode_4_aurora_odyssey_cartoon() {
        use crate::video::{VideoCodec, VideoFormat, VideoOptions, export_project_video};
        use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
        use neoflash_core::{
            AssetWarpRig, AuthoredPlayback, BlendMode, DecoEffectKind, Display3dPose, Easing,
            Element, ElementKind, FillRule, FilterSpec, FrameScript, FrameSound, GradientKind,
            GradientPaint, GradientStop, IkArmature, IkBone, InstancePlayback, Keyframe, Layer,
            LayerProperties, LayerRole, PathCommand, PrimitiveOptions, Project, RectangleOptions,
            ResourceUnit, Script, ScriptLanguage, ShapeHint, SoundEffect, SoundSync,
            SprayBrushConfig, Symbol, SymbolKind, Timeline, Transform, TweenKind, TweenProperty,
            WarpPin, WarpPinKind, evaluate, flatten_scenes_with_ranges, generate_deco_elements,
            generate_spray_brush_along_points, project_element_3d,
        };
        use std::f64::consts::TAU;

        // High-vertex (64-point) Ice Crystal Rose contour
        fn ice_crystal_rose_64(cx: f64, cy: f64, base_r: f64) -> Vec<PathCommand> {
            let n = 64usize;
            let mut cmds = Vec::with_capacity(n + 2);
            for i in 0..n {
                let a = (i as f64 / n as f64) * TAU - std::f64::consts::FRAC_PI_2;
                let r = base_r + 22.0 * (a * 8.0).cos() + 9.0 * (a * 16.0).sin();
                let x = cx + r * a.cos();
                let y = cy + r * a.sin();
                if i == 0 {
                    cmds.push(PathCommand::MoveTo { x, y });
                } else {
                    cmds.push(PathCommand::LineTo { x, y });
                }
            }
            cmds.push(PathCommand::Close);
            cmds
        }

        // High-vertex (64-point) Northern Crown Star contour
        fn northern_crown_64(cx: f64, cy: f64, outer_r: f64, inner_r: f64) -> Vec<PathCommand> {
            let n = 64usize;
            let mut cmds = Vec::with_capacity(n + 2);
            for i in 0..n {
                let a = (i as f64 / n as f64) * TAU - std::f64::consts::FRAC_PI_2;
                let spire = if i % 4 == 0 {
                    outer_r
                } else if i % 2 == 0 {
                    (outer_r + inner_r) * 0.55
                } else {
                    inner_r
                };
                let x = cx + spire * a.cos();
                let y = cy + spire * a.sin();
                if i == 0 {
                    cmds.push(PathCommand::MoveTo { x, y });
                } else {
                    cmds.push(PathCommand::LineTo { x, y });
                }
            }
            cmds.push(PathCommand::Close);
            cmds
        }

        // Aurora Ribbon wave contour
        fn aurora_ribbon_commands(cx: f64, cy: f64, width: f64, height: f64) -> Vec<PathCommand> {
            let hw = width * 0.5;
            let hh = height * 0.5;
            vec![
                PathCommand::MoveTo {
                    x: cx - hw,
                    y: cy - hh,
                },
                PathCommand::CubicTo {
                    cx1: cx - hw * 0.35,
                    cy1: cy - hh * 2.2,
                    cx2: cx + hw * 0.35,
                    cy2: cy + hh * 1.4,
                    x: cx + hw,
                    y: cy - hh * 0.6,
                },
                PathCommand::LineTo {
                    x: cx + hw,
                    y: cy + hh,
                },
                PathCommand::CubicTo {
                    cx1: cx + hw * 0.35,
                    cy1: cy + hh * 2.4,
                    cx2: cx - hw * 0.35,
                    cy2: cy - hh * 1.2,
                    x: cx - hw,
                    y: cy + hh * 0.8,
                },
                PathCommand::Close,
            ]
        }

        // 1. Synthesize 8 kHz mono 16-bit PCM WAV Aurora Choir + Train Whistle (1.5s = 12000 samples)
        let sample_rate = 8000u32;
        let num_samples = 12000u32;
        let data_bytes = num_samples * 2;
        let mut wav = Vec::with_capacity((44 + data_bytes) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_bytes.to_le_bytes());
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let env = (1.0 - (t / 1.5) * 0.4).clamp(0.0, 1.0);
            // Lydian Aurora shimmer chord (F4 + A4 + C5 + E5 + B5)
            let f1 = (t * 349.23 * TAU).sin();
            let f2 = (t * 440.00 * TAU).sin() * 0.7;
            let f3 = (t * 523.25 * TAU).sin() * 0.6;
            let f4 = (t * 659.25 * TAU).sin() * 0.5;
            let f5 = (t * 987.77 * TAU).sin() * 0.35 * (t * 5.0 * TAU).sin().abs();
            let sample =
                ((f1 + f2 + f3 + f4 + f5) * 0.21 * env * 32767.0).clamp(-32768.0, 32767.0) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }

        // 2. Build a 16x16 Aurora Emblem PNG resource
        let mut png_bytes = Vec::new();
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for y in 0..16u8 {
            for x in 0..16u8 {
                let border = !(2..14).contains(&x) || !(2..14).contains(&y);
                let star = (x as i32 - 8).abs() + (y as i32 - 8).abs() <= 4;
                if border {
                    rgba.extend_from_slice(&[52, 211, 153, 255]);
                } else if star {
                    rgba.extend_from_slice(&[250, 204, 21, 255]);
                } else {
                    rgba.extend_from_slice(&[15, 23, 42, 235]);
                }
            }
        }
        PngEncoder::new(&mut png_bytes)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .unwrap();

        let mut p = Project::blank();
        p.format_version = 9;
        p.name = "Midnight Express 4 — The Aurora Puppet & IK Odyssey".into();
        p.width = 960;
        p.height = 540;
        p.fps = 24;
        p.background = [10, 15, 32, 255];
        p.resources.push(
            ResourceUnit::from_bytes(
                "res-aurora-whistle",
                "audio/aurora_whistle.wav",
                "audio/wav",
                &wav,
            )
            .unwrap(),
        );
        p.resources.push(
            ResourceUnit::from_bytes(
                "res-aurora-crest",
                "images/aurora_crest.png",
                "image/png",
                &png_bytes,
            )
            .unwrap(),
        );

        // 3. Nested Graphic symbol: `sym-train` (8 frames, Aurora Express locomotive)
        let train_symbol = Symbol {
            id: "sym-train".into(),
            name: "Aurora Express Locomotive".into(),
            folder: "Vehicles".into(),
            kind: SymbolKind::Graphic,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 8,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "train-body-layer".into(),
                    name: "Locomotive Hull".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: (0..8)
                        .map(|f| {
                            let pulse = (f as f64 * TAU / 8.0).sin() * 0.5 + 0.5;
                            let bounce = if f % 2 == 0 { 0.0 } else { -1.5 };
                            Keyframe {
                                frame: f,
                                elements: vec![
                                    Element {
                                        id: "chassis".into(),
                                        name: "Chassis".into(),
                                        transform: Transform {
                                            x: -74.0,
                                            y: 12.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 148.0,
                                            height: 10.0,
                                            fill: [30, 58, 95, 255],
                                            stroke: [52, 211, 153, 255],
                                            stroke_width: 2.0,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "cabin".into(),
                                        name: "Cabin".into(),
                                        transform: Transform {
                                            x: -68.0,
                                            y: -28.0 + bounce,
                                            primitive: Some(PrimitiveOptions::Rectangle(
                                                RectangleOptions {
                                                    corner_radii: [10.0, 10.0, 4.0, 4.0],
                                                    corners_locked: false,
                                                },
                                            )),
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 132.0,
                                            height: 44.0,
                                            fill: [16, 185, 129, 255],
                                            stroke: [250, 204, 21, 255],
                                            stroke_width: 3.0,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "chimney".into(),
                                        name: "Chimney".into(),
                                        transform: Transform {
                                            x: 34.0,
                                            y: -48.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 18.0,
                                            height: 22.0,
                                            fill: [14, 116, 144, 255],
                                            stroke: [250, 204, 21, 255],
                                            stroke_width: 2.0,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "win-1".into(),
                                        name: "Window 1".into(),
                                        transform: Transform {
                                            x: -52.0,
                                            y: -18.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 26.0,
                                            height: 18.0,
                                            fill: [254, 240, 138, 255],
                                            stroke: [15, 23, 42, 255],
                                            stroke_width: 1.5,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "win-2".into(),
                                        name: "Window 2".into(),
                                        transform: Transform {
                                            x: -16.0,
                                            y: -18.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 26.0,
                                            height: 18.0,
                                            fill: [254, 240, 138, 255],
                                            stroke: [15, 23, 42, 255],
                                            stroke_width: 1.5,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "wheel-rear".into(),
                                        name: "Rear Wheel".into(),
                                        transform: Transform {
                                            x: -48.0,
                                            y: 14.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Ellipse {
                                            width: 22.0,
                                            height: 22.0,
                                            fill: [15, 23, 42, 255],
                                            stroke: [52, 211, 153, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "wheel-front".into(),
                                        name: "Front Wheel".into(),
                                        transform: Transform {
                                            x: 22.0,
                                            y: 14.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Ellipse {
                                            width: 22.0,
                                            height: 22.0,
                                            fill: [15, 23, 42, 255],
                                            stroke: [52, 211, 153, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "headlight".into(),
                                        name: "Headlight".into(),
                                        transform: Transform {
                                            x: 56.0 + pulse * 6.0,
                                            y: -16.0 + bounce,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Ellipse {
                                            width: 20.0 + pulse * 8.0,
                                            height: 20.0 + pulse * 8.0,
                                            fill: [167, 243, 208, 245],
                                            stroke: [52, 211, 153, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            }
                        })
                        .collect(),
                }],
            },
        };

        // 4. Nested MovieClip symbol: `sym-drone` (6 frames, Aurora Escort Drone)
        let drone_symbol = Symbol {
            id: "sym-drone".into(),
            name: "Aurora Escort Drone".into(),
            folder: "Vehicles".into(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 6,
                scripts: vec![],
                guides: vec![],
                layers: vec![Layer {
                    id: "drone-layer".into(),
                    name: "Drone".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            elements: vec![Element {
                                id: "drone-hull".into(),
                                name: "Hull".into(),
                                filters: vec![FilterSpec::Glow {
                                    color: 0x34D399,
                                    alpha: 0.9,
                                    blur_x: 14.0,
                                    blur_y: 14.0,
                                    strength: 2.2,
                                    quality: 2,
                                    inner: false,
                                    knockout: false,
                                }],
                                kind: ElementKind::Ellipse {
                                    width: 52.0,
                                    height: 24.0,
                                    fill: [52, 211, 153, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 5,
                            elements: vec![Element {
                                id: "drone-hull".into(),
                                name: "Hull".into(),
                                transform: Transform {
                                    y: -12.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Ellipse {
                                    width: 52.0,
                                    height: 24.0,
                                    fill: [52, 211, 153, 255],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                }],
            },
        };

        p.library_folders = vec!["Vehicles".into()];
        p.symbols.push(train_symbol);
        p.symbols.push(drone_symbol);

        // =========================================================================
        // 5. SCENE 1 (`p.root`): "Aurora Glacier Depot · IK & Puppet Rig" (48 frames)
        // =========================================================================
        let scene1_id = p.root.clone();
        {
            let scene1 = p.symbol_mut(&scene1_id).unwrap();
            scene1.name = "Aurora Glacier Depot · IK & Puppet Rig".into();
            scene1.timeline.frame_count = 48;
            // Live ActionScript HUD measuring real FPS during HTML5 playback
            scene1.timeline.scripts = vec![FrameScript {
                frame: 0,
                script: Script {
                    language: ScriptLanguage::ActionScript2,
                    source: r#"
if (this.ep4Init == undefined) {
    this.ep4Init = true;
    this.targetFps = 24;
    this.measuredFps = 24.0;
    this.lastStamp = getTimer();
    this.onEnterFrame = function() {
        var now = getTimer();
        var dt = now - this.lastStamp;
        if (dt <= 0) { dt = 1; }
        this.lastStamp = now;
        var inst = 1000 / dt;
        this.measuredFps = Math.round((this.measuredFps * 0.85 + inst * 0.15) * 10) / 10;
    };
}
"#
                    .into(),
                },
            }];
            scene1.timeline.layers = vec![
                // Layer 0: Polar Night Sky + Glacier Depot Platform
                Layer {
                    id: "ep4-s1-backdrop".into(),
                    name: "Polar Sky & Glacier Depot".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        label: "ep4_scene1_aurora_depot".into(),
                        elements: vec![
                            Element {
                                id: "ep4-sky-grad".into(),
                                name: "Polar Sky".into(),
                                kind: ElementKind::PaintedPath {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 0.0 },
                                        PathCommand::LineTo { x: 960.0, y: 540.0 },
                                        PathCommand::LineTo { x: 0.0, y: 540.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [10, 18, 42, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                    fill_gradient: Some(GradientPaint {
                                        kind: GradientKind::Linear,
                                        matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                        stops: vec![
                                            GradientStop {
                                                ratio: 0,
                                                color: [6, 12, 30, 255],
                                            },
                                            GradientStop {
                                                ratio: 145,
                                                color: [16, 68, 92, 255],
                                            },
                                            GradientStop {
                                                ratio: 255,
                                                color: [42, 34, 88, 255],
                                            },
                                        ],
                                        focal_point_ratio: 0.0,
                                        ..Default::default()
                                    }),
                                    stroke_gradient: None,
                                },
                                ..Element::default()
                            },
                            Element {
                                id: "ep4-glacier-deck".into(),
                                name: "Glacier Viaduct".into(),
                                transform: Transform {
                                    x: 0.0,
                                    y: 438.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Rectangle {
                                    width: 960.0,
                                    height: 102.0,
                                    fill: [18, 32, 56, 255],
                                    stroke: [52, 211, 153, 255],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            },
                        ],
                        ..Keyframe::default()
                    }],
                },
                // Layer 1: PUPPET TOOL (Asset Warp Rig) — Waving Aurora Borealis Curtain
                Layer {
                    id: "ep4-s1-puppet-aurora".into(),
                    name: "Puppet Warped Aurora Curtain".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        blend_mode: BlendMode::Screen,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            asset_warps: vec![AssetWarpRig {
                                element_id: "aurora-curtain".into(),
                                envelop_subdivisions: 4,
                                pins: vec![
                                    WarpPin {
                                        id: "pin-west".into(),
                                        kind: WarpPinKind::Open,
                                        rest_pos: [180.0, 130.0],
                                        pos: [180.0, 118.0],
                                        rotation: 8.0,
                                        stiffness: 0.5,
                                    },
                                    WarpPin {
                                        id: "pin-center".into(),
                                        kind: WarpPinKind::Open,
                                        rest_pos: [480.0, 130.0],
                                        pos: [480.0, 152.0],
                                        rotation: -12.0,
                                        stiffness: 0.5,
                                    },
                                    WarpPin {
                                        id: "pin-east".into(),
                                        kind: WarpPinKind::Open,
                                        rest_pos: [780.0, 130.0],
                                        pos: [780.0, 112.0],
                                        rotation: 10.0,
                                        stiffness: 0.5,
                                    },
                                ],
                            }],
                            elements: vec![Element {
                                id: "aurora-curtain".into(),
                                name: "Aurora Borealis Curtain".into(),
                                kind: ElementKind::Path {
                                    commands: aurora_ribbon_commands(480.0, 130.0, 680.0, 56.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [52, 211, 153, 185],
                                    stroke: [167, 243, 208, 220],
                                    stroke_width: 2.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "aurora-curtain".into(),
                                name: "Aurora Borealis Curtain".into(),
                                kind: ElementKind::Path {
                                    commands: aurora_ribbon_commands(480.0, 116.0, 700.0, 64.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [56, 189, 248, 195],
                                    stroke: [192, 132, 252, 235],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 2: INVERSE KINEMATICS (Bone/Bind Armature) — Articulated Signal Crane Arm
                Layer {
                    id: "ep4-s1-ik-crane".into(),
                    name: "IK Articulated Signal Crane".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Classic,
                            ik_armatures: vec![IkArmature {
                                id: "ik-signal-crane".into(),
                                name: "Signal Crane Armature".into(),
                                effector_bone_id: "bone-upper".into(),
                                target_pos: [215.0, 295.0],
                                pole_pos: Some([120.0, 320.0]),
                                bones: vec![
                                    IkBone {
                                        id: "bone-lower".into(),
                                        parent_bone_id: None,
                                        element_id: "crane-lower".into(),
                                        length: 78.0,
                                        min_angle: Some((-120.0_f64).to_radians()),
                                        max_angle: Some((-15.0_f64).to_radians()),
                                        stiffness: 0.25,
                                    },
                                    IkBone {
                                        id: "bone-upper".into(),
                                        parent_bone_id: Some("bone-lower".into()),
                                        element_id: "crane-upper".into(),
                                        length: 68.0,
                                        min_angle: Some((-90.0_f64).to_radians()),
                                        max_angle: Some((45.0_f64).to_radians()),
                                        stiffness: 0.15,
                                    },
                                ],
                            }],
                            elements: vec![
                                Element {
                                    id: "crane-lower".into(),
                                    name: "Crane Lower Boom".into(),
                                    transform: Transform {
                                        x: 140.0,
                                        y: 360.0,
                                        rotation: -25.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 14.0,
                                        height: 78.0,
                                        fill: [56, 189, 248, 255],
                                        stroke: [255, 255, 255, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "crane-upper".into(),
                                    name: "Crane Signal Lantern".into(),
                                    transform: Transform {
                                        x: 175.0,
                                        y: 300.0,
                                        rotation: 15.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Ellipse {
                                        width: 28.0,
                                        height: 28.0,
                                        fill: [52, 211, 153, 255],
                                        stroke: [254, 240, 138, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![
                                Element {
                                    id: "crane-lower".into(),
                                    name: "Crane Lower Boom".into(),
                                    transform: Transform {
                                        x: 140.0,
                                        y: 350.0,
                                        rotation: 12.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 14.0,
                                        height: 78.0,
                                        fill: [56, 189, 248, 255],
                                        stroke: [255, 255, 255, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: "crane-upper".into(),
                                    name: "Crane Signal Lantern".into(),
                                    transform: Transform {
                                        x: 205.0,
                                        y: 270.0,
                                        rotation: 45.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Ellipse {
                                        width: 28.0,
                                        height: 28.0,
                                        fill: [52, 211, 153, 255],
                                        stroke: [254, 240, 138, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 3: HIGH-VERTEX SHAPE TWEEN #1 (64-vertex Ice Crystal Rose -> 64-vertex Northern Crown Star)
                Layer {
                    id: "ep4-s1-crystal-morph".into(),
                    name: "64-Vertex Ice Crystal to Northern Crown Morph".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Shape,
                            shape_hints: vec![
                                ShapeHint {
                                    label: 'a',
                                    start_uv: [0.5, 0.0],
                                    end_uv: [0.5, 0.0],
                                },
                                ShapeHint {
                                    label: 'b',
                                    start_uv: [0.5, 1.0],
                                    end_uv: [0.5, 1.0],
                                },
                            ],
                            elements: vec![Element {
                                id: "ice-crystal-64".into(),
                                name: "Ice Crystal Rose 64".into(),
                                kind: ElementKind::Path {
                                    commands: ice_crystal_rose_64(760.0, 135.0, 58.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [125, 211, 252, 240],
                                    stroke: [255, 255, 255, 255],
                                    stroke_width: 3.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "ice-crystal-64".into(),
                                name: "Northern Crown 64".into(),
                                kind: ElementKind::Path {
                                    commands: northern_crown_64(740.0, 125.0, 76.0, 32.0),
                                    fill_rule: FillRule::NonZero,
                                    fill: [250, 204, 21, 250],
                                    stroke: [52, 211, 153, 255],
                                    stroke_width: 3.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 4: Curved Glacier Track Motion Guide
                Layer {
                    id: "ep4-s1-track-guide".into(),
                    name: "Glacier Track Guide".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        role: LayerRole::Guide,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "ep4-guide-rail".into(),
                            name: "Rail".into(),
                            kind: ElementKind::Path {
                                commands: vec![
                                    PathCommand::MoveTo { x: 90.0, y: 415.0 },
                                    PathCommand::QuadraticTo {
                                        cx: 380.0,
                                        cy: 325.0,
                                        x: 560.0,
                                        y: 380.0,
                                    },
                                    PathCommand::QuadraticTo {
                                        cx: 720.0,
                                        cy: 420.0,
                                        x: 870.0,
                                        y: 350.0,
                                    },
                                ],
                                fill_rule: FillRule::NonZero,
                                fill: [0, 0, 0, 0],
                                stroke: [52, 211, 153, 255],
                                stroke_width: 2.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                },
                // Layer 5: Guided Aurora Express Train (`orient_to_path` + `snap_to_guide`)
                Layer {
                    id: "ep4-s1-express-train".into(),
                    name: "Aurora Express Train".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        motion_guide: Some("ep4-s1-track-guide".into()),
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            orient_to_path: true,
                            snap_to_guide: true,
                            elements: vec![Element {
                                id: "ep4-train-inst".into(),
                                name: "Train".into(),
                                transform: Transform {
                                    x: 90.0,
                                    y: 415.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 47,
                            elements: vec![Element {
                                id: "ep4-train-inst".into(),
                                name: "Train".into(),
                                transform: Transform {
                                    x: 870.0,
                                    y: 350.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Instance {
                                    symbol_id: "sym-train".into(),
                                    playback: InstancePlayback::Loop,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 6: Episode 4 Neon Banner
                Layer {
                    id: "ep4-s1-title".into(),
                    name: "Episode 4 Banner".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties {
                        blend_mode: BlendMode::Screen,
                        ..LayerProperties::default()
                    },
                    keyframes: vec![
                        Keyframe {
                            frame: 0,
                            tween: Some(Easing::EaseInOut),
                            tween_kind: TweenKind::Motion,
                            elements: vec![Element {
                                id: "ep4-banner".into(),
                                name: "Banner".into(),
                                transform: Transform {
                                    x: 68.0,
                                    y: 42.0,
                                    alpha: 0.85,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "MIDNIGHT EXPRESS 4 · THE AURORA PUPPET & IK ODYSSEY"
                                        .into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 23.0,
                                    color: [52, 211, 153, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                        Keyframe {
                            frame: 24,
                            keyed_properties: Some(vec![
                                TweenProperty::X,
                                TweenProperty::Y,
                                TweenProperty::Alpha,
                            ]),
                            elements: vec![Element {
                                id: "ep4-banner".into(),
                                name: "Banner".into(),
                                transform: Transform {
                                    x: 88.0,
                                    y: 36.0,
                                    alpha: 1.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "MIDNIGHT EXPRESS 4 · THE AURORA PUPPET & IK ODYSSEY"
                                        .into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 23.0,
                                    color: [52, 211, 153, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        },
                    ],
                },
                // Layer 7: Sound Layer
                Layer {
                    id: "ep4-s1-audio".into(),
                    name: "Aurora Choir & Whistle Audio".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        sound: Some(FrameSound {
                            resource: "res-aurora-whistle".into(),
                            sync: SoundSync::Stream,
                            loops: 1,
                            loop_forever: false,
                            effect: SoundEffect::Custom,
                            in_point: Some(0.0),
                            out_point: None,
                            envelope: Some(vec![[0.0, 0.4, 0.9], [0.75, 0.95, 0.5]]),
                        }),
                        ..Keyframe::default()
                    }],
                },
            ];
        }

        // =========================================================================
        // 6. SCENE 2: "Clockwork Aurora Viaduct · Deco, Spray & 3D" (48 frames)
        // =========================================================================
        let deco_elements = generate_deco_elements(
            DecoEffectKind::SymmetryBrush,
            [480.0, 185.0],
            115.0,
            28.0,
            [56, 189, 248, 220],
            [250, 204, 21, 255],
        );
        let spray_elements = generate_spray_brush_along_points(
            &[[120.0, 95.0], [310.0, 75.0], [640.0, 88.0], [830.0, 105.0]],
            &SprayBrushConfig {
                symbol_id: None,
                fill: [254, 240, 138, 235],
                scale_w_pct: 90.0,
                scale_h_pct: 90.0,
                random_scaling: true,
                rotate_symbol: false,
                random_rotation: true,
                brush_width: 48.0,
                brush_height: 48.0,
                brush_angle_deg: 0.0,
            },
        );
        let arch_3d_base = Element {
            id: "s2-3d-arch".into(),
            name: "3D Clockwork Arch".into(),
            transform: Transform {
                x: 420.0,
                y: 260.0,
                ..Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 120.0,
                height: 150.0,
                fill: [30, 58, 138, 180],
                stroke: [52, 211, 153, 255],
                stroke_width: 3.0,
            },
            ..Element::default()
        };
        let arch_3d = project_element_3d(
            &arch_3d_base,
            &Display3dPose {
                x: 420.0,
                y: 260.0,
                z: -40.0,
                scale_x: 1.0,
                scale_y: 1.0,
                scale_z: 1.0,
                rotation_x: 18.0,
                rotation_y: -24.0,
                rotation_z: 4.0,
            },
            480.0,
            [480.0, 270.0],
        );

        let mut s2_backdrop_elements = vec![
            Element {
                id: "s2-sky".into(),
                name: "Viaduct Sky".into(),
                kind: ElementKind::Rectangle {
                    width: 960.0,
                    height: 540.0,
                    fill: [12, 20, 48, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            },
            Element {
                id: "s2-bridge-deck".into(),
                name: "Bridge Deck".into(),
                transform: Transform {
                    x: 0.0,
                    y: 412.0,
                    ..Transform::default()
                },
                kind: ElementKind::Rectangle {
                    width: 960.0,
                    height: 24.0,
                    fill: [30, 58, 95, 255],
                    stroke: [250, 204, 21, 255],
                    stroke_width: 2.5,
                },
                ..Element::default()
            },
            arch_3d,
        ];
        s2_backdrop_elements.extend(deco_elements);
        s2_backdrop_elements.extend(spray_elements);

        let scene2 = Symbol {
            id: "scene-clockwork-viaduct".into(),
            name: "Clockwork Aurora Viaduct · Deco & 3D".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 48,
                scripts: vec![],
                guides: vec![],
                layers: vec![
                    Layer {
                        id: "ep4-s2-backdrop".into(),
                        name: "Deco, Spray & 3D Viaduct".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            label: "ep4_scene2_viaduct".into(),
                            elements: s2_backdrop_elements,
                            ..Keyframe::default()
                        }],
                    },
                    Layer {
                        id: "ep4-s2-morph".into(),
                        name: "Crown to Crystal Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![Element {
                                    id: "s2-crown-64".into(),
                                    name: "Crown 64".into(),
                                    kind: ElementKind::Path {
                                        commands: northern_crown_64(240.0, 185.0, 68.0, 28.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [52, 211, 153, 240],
                                        stroke: [255, 255, 255, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "s2-crown-64".into(),
                                    name: "Crystal 64".into(),
                                    kind: ElementKind::Path {
                                        commands: ice_crystal_rose_64(720.0, 185.0, 64.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [244, 114, 182, 240],
                                        stroke: [254, 240, 138, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    Layer {
                        id: "ep4-s2-train".into(),
                        name: "Viaduct Express Train".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::Linear),
                                elements: vec![Element {
                                    id: "s2-train-inst".into(),
                                    name: "Bridge Train".into(),
                                    transform: Transform {
                                        x: 110.0,
                                        y: 382.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "s2-train-inst".into(),
                                    name: "Bridge Train".into(),
                                    transform: Transform {
                                        x: 850.0,
                                        y: 382.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    Layer {
                        id: "ep4-s2-drone".into(),
                        name: "Aurora Drone Escort".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::Flash { acceleration: 50.0 }),
                                elements: vec![Element {
                                    id: "s2-drone-inst".into(),
                                    name: "Drone".into(),
                                    transform: Transform {
                                        x: 130.0,
                                        y: 150.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-drone".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "s2-drone-inst".into(),
                                    name: "Drone".into(),
                                    transform: Transform {
                                        x: 820.0,
                                        y: 135.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-drone".into(),
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                ],
            },
        };

        // =========================================================================
        // 7. SCENE 3: "Solstice Citadel Sunrise" (48 frames)
        // =========================================================================
        let scene3 = Symbol {
            id: "scene-solstice-sunrise".into(),
            name: "Solstice Citadel Sunrise".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            class_name: None,
            linkage_id: None,
            timeline: Timeline {
                frame_count: 48,
                scripts: vec![],
                guides: vec![],
                layers: vec![
                    Layer {
                        id: "ep4-s3-sky-morph".into(),
                        name: "Polar-to-Solstice Sky Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                label: "ep4_scene3_sunrise".into(),
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![
                                    Element {
                                        id: "s3-sky".into(),
                                        name: "Solstice Sky".into(),
                                        kind: ElementKind::PaintedPath {
                                            commands: vec![
                                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 540.0 },
                                                PathCommand::LineTo { x: 0.0, y: 540.0 },
                                                PathCommand::Close,
                                            ],
                                            fill_rule: FillRule::NonZero,
                                            fill: [18, 28, 64, 255],
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                            fill_gradient: Some(GradientPaint {
                                                kind: GradientKind::Linear,
                                                matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                                stops: vec![
                                                    GradientStop {
                                                        ratio: 0,
                                                        color: [14, 24, 58, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 155,
                                                        color: [45, 85, 125, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 255,
                                                        color: [135, 95, 145, 255],
                                                    },
                                                ],
                                                focal_point_ratio: 0.0,
                                                ..Default::default()
                                            }),
                                            stroke_gradient: None,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "s3-platform".into(),
                                        name: "Citadel Platform".into(),
                                        transform: Transform {
                                            x: 0.0,
                                            y: 432.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 960.0,
                                            height: 108.0,
                                            fill: [30, 42, 68, 255],
                                            stroke: [52, 211, 153, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![
                                    Element {
                                        id: "s3-sky".into(),
                                        name: "Solstice Sky".into(),
                                        kind: ElementKind::PaintedPath {
                                            commands: vec![
                                                PathCommand::MoveTo { x: 0.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 0.0 },
                                                PathCommand::LineTo { x: 960.0, y: 540.0 },
                                                PathCommand::LineTo { x: 0.0, y: 540.0 },
                                                PathCommand::Close,
                                            ],
                                            fill_rule: FillRule::NonZero,
                                            fill: [250, 185, 125, 255],
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                            fill_gradient: Some(GradientPaint {
                                                kind: GradientKind::Linear,
                                                matrix: [0.0, 0.33, -0.33, 0.0, 480.0, 270.0],
                                                stops: vec![
                                                    GradientStop {
                                                        ratio: 0,
                                                        color: [38, 92, 148, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 155,
                                                        color: [232, 128, 152, 255],
                                                    },
                                                    GradientStop {
                                                        ratio: 255,
                                                        color: [254, 218, 128, 255],
                                                    },
                                                ],
                                                focal_point_ratio: 0.0,
                                                ..Default::default()
                                            }),
                                            stroke_gradient: None,
                                        },
                                        ..Element::default()
                                    },
                                    Element {
                                        id: "s3-platform".into(),
                                        name: "Citadel Platform".into(),
                                        transform: Transform {
                                            x: 0.0,
                                            y: 432.0,
                                            ..Transform::default()
                                        },
                                        kind: ElementKind::Rectangle {
                                            width: 960.0,
                                            height: 108.0,
                                            fill: [58, 52, 78, 255],
                                            stroke: [250, 204, 21, 255],
                                            stroke_width: 2.5,
                                        },
                                        ..Element::default()
                                    },
                                ],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    Layer {
                        id: "ep4-s3-sun-morph".into(),
                        name: "64-Vertex Frost Rose to Solstice Sun Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![Element {
                                    id: "solstice-sun".into(),
                                    name: "Frost Rose".into(),
                                    kind: ElementKind::Path {
                                        commands: ice_crystal_rose_64(735.0, 250.0, 56.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [167, 243, 208, 245],
                                        stroke: [52, 211, 153, 255],
                                        stroke_width: 3.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "solstice-sun".into(),
                                    name: "Solstice Sun".into(),
                                    kind: ElementKind::Path {
                                        commands: northern_crown_64(735.0, 190.0, 90.0, 68.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [250, 204, 21, 255],
                                        stroke: [249, 115, 22, 255],
                                        stroke_width: 4.0,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    Layer {
                        id: "ep4-s3-arrival-train".into(),
                        name: "Citadel Arrival Train".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseOut),
                                elements: vec![Element {
                                    id: "s3-train-inst".into(),
                                    name: "Citadel Train".into(),
                                    transform: Transform {
                                        x: 110.0,
                                        y: 404.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 28,
                                elements: vec![Element {
                                    id: "s3-train-inst".into(),
                                    name: "Citadel Train".into(),
                                    transform: Transform {
                                        x: 420.0,
                                        y: 404.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: "sym-train".into(),
                                        playback: InstancePlayback::Loop,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    Layer {
                        id: "ep4-s3-heart-morph".into(),
                        name: "Aurora Heart Morph".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![
                            Keyframe {
                                frame: 0,
                                tween: Some(Easing::EaseInOut),
                                tween_kind: TweenKind::Shape,
                                elements: vec![Element {
                                    id: "s3-heart".into(),
                                    name: "Glowing Aurora Heart".into(),
                                    kind: ElementKind::Path {
                                        commands: heart_puff_commands(280.0, 310.0, 38.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [52, 211, 153, 235],
                                        stroke: [255, 255, 255, 255],
                                        stroke_width: 2.5,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                            Keyframe {
                                frame: 47,
                                elements: vec![Element {
                                    id: "s3-heart".into(),
                                    name: "Golden Heart".into(),
                                    kind: ElementKind::Path {
                                        commands: heart_puff_commands(455.0, 215.0, 58.0),
                                        fill_rule: FillRule::NonZero,
                                        fill: [244, 114, 182, 250],
                                        stroke: [254, 240, 138, 255],
                                        stroke_width: 3.5,
                                    },
                                    ..Element::default()
                                }],
                                ..Keyframe::default()
                            },
                        ],
                    },
                    Layer {
                        id: "ep4-s3-closing-title".into(),
                        name: "Closing Morning Title".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "s3-morning-title".into(),
                                name: "Morning Title".into(),
                                transform: Transform {
                                    x: 72.0,
                                    y: 46.0,
                                    ..Transform::default()
                                },
                                kind: ElementKind::Text {
                                    text: "SOLSTICE CITADEL SUNRISE · GOOD MORNING!".into(),
                                    font_family: "Helvetica".into(),
                                    font_size: 24.0,
                                    color: [254, 240, 138, 255],
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    },
                ],
            },
        };

        p.symbols.push(scene2);
        p.symbols.push(scene3);
        p.scenes = vec![
            scene1_id.clone(),
            "scene-clockwork-viaduct".into(),
            "scene-solstice-sunrise".into(),
        ];
        p.upgrade_format_for_features();
        p.validate().unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        if std::env::var_os("NEOFLASH_WRITE_EPISODE4_EXAMPLE").is_some() {
            let example_path = repo_root.join("examples/midnight-express-4.nflash");
            fs::write(&example_path, p.to_json().unwrap()).unwrap();
        }

        let (flat, ranges) = flatten_scenes_with_ranges(&p);
        flat.validate().unwrap();
        assert_eq!(ranges.len(), 3);
        assert_eq!(flat.symbol(&flat.root).unwrap().timeline.frame_count, 144);
        for f in 0..144 {
            let items = evaluate(&flat, &flat.root, f);
            assert!(!items.is_empty(), "frame {f} must produce draw items");
        }

        let mut playback = AuthoredPlayback::new(&flat, &flat.root, 0);
        assert_eq!(playback.drain_sound_commands().len(), 1);

        let output = TestOutputDir::new("midnight-express-4");
        let export_media = std::env::var_os("NEOFLASH_EXPORT_EPISODE4_MEDIA").is_some();
        let export_html5 = std::env::var_os("NEOFLASH_EXPORT_CARTOON_HTML5").is_some();
        let out_dir = if export_media || export_html5 {
            repo_root.join("target/artifacts/generated-examples")
        } else {
            output.0.clone()
        };
        fs::create_dir_all(&out_dir).unwrap();

        let lottie_json_path = out_dir.join("midnight-express-4.lottie.json");
        let lottie_report = export_project_lottie(
            &p,
            &lottie_json_path,
            &LottieOptions {
                format: LottieFormat::Json,
                image_mode: LottieImageMode::Embed,
                image_format: LottieImageFormat::Png,
                html_preview: true,
                ..LottieOptions::default()
            },
        )
        .unwrap();
        assert_eq!(lottie_report.frame_count, 144);
        assert!(lottie_json_path.exists());

        if export_media {
            let mp4_path = out_dir.join("midnight-express-4.mp4");
            let mp4_report = export_project_video(
                &p,
                &mp4_path,
                &VideoOptions {
                    format: VideoFormat::Mp4,
                    codec: VideoCodec::H264,
                    include_audio: true,
                    ..VideoOptions::default()
                },
            )
            .unwrap();
            assert_eq!(mp4_report.frame_count, 144);
            let gif_path = out_dir.join("midnight-express-4.gif");
            let gif_report = export_project_video(
                &p,
                &gif_path,
                &VideoOptions {
                    format: VideoFormat::Gif,
                    codec: VideoCodec::GifLzw,
                    include_audio: false,
                    ..VideoOptions::default()
                },
            )
            .unwrap();
            assert_eq!(gif_report.frame_count, 144);
        }

        let web_runtime = repo_root.join("target/web-runtime");
        if export_html5 && web_runtime.join("neoflash_web_bg.wasm").is_file() {
            let html5_dir = out_dir.join("midnight-express-4-html5");
            crate::export_project_with_options(
                &p,
                &html5_dir,
                &web_runtime,
                &crate::PublishOptions::default(),
            )
            .unwrap();
            assert!(html5_dir.join("index.html").exists());
        }
    }
}
