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

mod assets;
mod dotlottie;
mod preview;
mod shapes;
#[cfg(test)]
mod tests;
mod tracks;

pub use dotlottie::{
    export_dotlottie, export_dotlottie_bytes, export_dotlottie_bytes_with_cancel,
    validate_dotlottie_bytes,
};

use self::{
    assets::ExternalImageFile,
    dotlottie::{build_dotlottie_zip, validate_zip_entry_path},
    preview::build_lottie_html_preview,
};
use crate::ExportError;
use neoflash_core::{BlendMode, Project, Symbol, Timeline, flatten_scenes_with_ranges};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write as _,
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
        crate::ensure_export_dir_tracked(dir, &mut self.created_dirs).map_err(ExportError::Io)
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
        crate::commit_staged_files_atomically(&self.staged_files)?;
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
        crate::cleanup_created_export_dirs(&mut self.created_dirs);
    }
}
