//! One atomic graphic import shared by the File menu and native file drops.
use crate::{
    app::{Editor, reaches},
    selection::{Selection, SelectionTarget},
    stage_mapping::StageMapping,
};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use std::{collections::HashMap, path::PathBuf};

pub(crate) fn is_supported_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "m4v" | "mov" | "webm" | "ogv" | "avi"
            )
        })
}

pub(crate) fn is_known_unsupported_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "flv" | "mkv" | "wmv"
            )
        })
}

pub(crate) fn is_video_drop_path(path: &std::path::Path) -> bool {
    is_supported_video_path(path) || is_known_unsupported_video_path(path)
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineDropZone {
    pub panel_rect: Rect,
    pub rows_inner_rect: Rect,
    pub grid_clip: Rect,
    pub grid_origin_x: f32,
    pub cell_width: f32,
    pub row_height: f32,
    pub list_origin_y: f32,
    pub visible_layer_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TimelineDropPlacement {
    pub frame: u32,
    pub hovered_layer: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MediaPlacementMode {
    LibraryOnly,
    Stage { point: Option<[f64; 2]> },
    Timeline(TimelineDropPlacement),
}

impl MediaPlacementMode {
    fn places_on_timeline_or_stage(self) -> bool {
        !matches!(self, Self::LibraryOnly)
    }

    fn stage_point(self) -> Option<[f64; 2]> {
        match self {
            Self::Stage { point } => point,
            Self::LibraryOnly | Self::Timeline(_) => None,
        }
    }
}

impl TimelineDropZone {
    pub(crate) fn resolve(
        &self,
        pos: Pos2,
        current_frame: u32,
        is_button: bool,
    ) -> TimelineDropPlacement {
        let max_frames = if is_button { 4 } else { 1_000_000 };
        let frame = if self.grid_clip.width() > 0.0 && pos.x >= self.grid_clip.left() {
            crate::timeline_paint::frame_at_x(
                pos.x,
                self.grid_origin_x,
                self.cell_width,
                max_frames,
            )
        } else if is_button {
            current_frame.min(3)
        } else {
            current_frame.min(999_999)
        };
        let hovered_layer = if pos.y >= self.rows_inner_rect.top()
            && pos.y <= self.rows_inner_rect.bottom()
            && pos.y >= self.list_origin_y
            && self.row_height > 0.0
        {
            let row_idx = ((pos.y - self.list_origin_y) / self.row_height).floor() as usize;
            self.visible_layer_indices.get(row_idx).copied()
        } else {
            None
        };
        TimelineDropPlacement {
            frame,
            hovered_layer,
        }
    }

    pub(crate) fn highlight_rect(&self, pos: Pos2, target_frame: u32) -> Rect {
        if pos.y >= self.rows_inner_rect.top()
            && pos.y <= self.rows_inner_rect.bottom()
            && pos.y >= self.list_origin_y
            && self.row_height > 0.0
        {
            let row_idx = ((pos.y - self.list_origin_y) / self.row_height).floor() as usize;
            if row_idx < self.visible_layer_indices.len() {
                let y0 = self.list_origin_y + row_idx as f32 * self.row_height;
                let y1 = y0 + self.row_height;
                if self.grid_clip.width() > 0.0 && pos.x >= self.grid_clip.left() {
                    let x0 = self.grid_origin_x + target_frame as f32 * self.cell_width;
                    let cell = Rect::from_min_size(
                        Pos2::new(x0, y0),
                        egui::vec2(self.cell_width.max(4.0), self.row_height),
                    )
                    .intersect(self.grid_clip);
                    if cell.is_positive() {
                        return cell;
                    }
                }
                let row_strip = Rect::from_min_max(
                    Pos2::new(self.panel_rect.left(), y0),
                    Pos2::new(self.panel_rect.right(), y1),
                )
                .intersect(self.rows_inner_rect);
                if row_strip.is_positive() {
                    return row_strip;
                }
            }
        }
        self.panel_rect.shrink(neoflash_ui::tokens::space::s1())
    }
}

#[derive(Clone)]
pub(crate) struct PreparedGraphic {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub elements: Vec<Element>,
    pub vector_effects: Vec<ElementVectorEffects>,
    pub resources: Vec<ResourceUnit>,
    pub notice: Option<String>,
}

#[derive(Clone)]
struct PreparedImportGraphic {
    pub graphic: PreparedGraphic,
    pub timeline_override: Option<Timeline>,
    pub symbol_kind: SymbolKind,
    pub playback: InstancePlayback,
    pub helper_symbols: Vec<Symbol>,
}

#[derive(Clone)]
enum PreparedGraphicBatch {
    None,
    Symbols(Vec<PreparedImportGraphic>),
    Keyframes(Box<PreparedImportGraphic>),
}

impl PreparedGraphicBatch {
    fn is_empty(&self) -> bool {
        matches!(self, Self::None)
    }

    fn is_keyframes(&self) -> bool {
        matches!(self, Self::Keyframes(_))
    }
}

#[derive(Clone)]
struct PreparedAudioFile {
    path: PathBuf,
    stem: String,
    unit: ResourceUnit,
    duration_sec: Option<f64>,
}

fn import_audio_duration(unit: &ResourceUnit) -> Option<f64> {
    crate::timeline_sound::decode_unit(unit)
        .ok()
        .map(|audio| audio.duration())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
}

pub(crate) fn layer_is_dedicated_script_layer(timeline: &Timeline, index: usize) -> bool {
    let Some(layer) = timeline.layers.get(index) else {
        return false;
    };
    let has_no_media_or_elements = layer
        .keyframes
        .iter()
        .all(|k| k.elements.is_empty() && k.sound.is_none() && k.video.is_none());
    if !has_no_media_or_elements {
        return false;
    }
    let named_actions = matches!(
        layer.name.trim().to_ascii_lowercase().as_str(),
        "actions" | "action" | "scripts" | "script"
    );
    named_actions || timeline.layer_owns_any_frame_script(index)
}

fn sole_untouched_blank_layer(timeline: &Timeline) -> Option<usize> {
    if timeline.layers.len() != 1 || layer_is_dedicated_script_layer(timeline, 0) {
        return None;
    }
    let layer = &timeline.layers[0];
    let untouched = layer.properties == neoflash_core::LayerProperties::default()
        && layer_effective_outline_color(timeline, layer).is_none()
        && layer_effectively_visible(timeline, layer)
        && !layer_effectively_locked(timeline, layer)
        && layer.keyframes.len() <= 1
        && layer.keyframes.iter().all(|k| {
            k.frame == 0
                && k.elements.is_empty()
                && k.sound.is_none()
                && k.video.is_none()
                && k.camera.is_none()
                && k.filters.is_empty()
                && k.bitmap_fills.is_empty()
                && k.appearances.is_empty()
                && k.vector_effects.is_empty()
                && k.tween.is_none()
                && k.label.is_empty()
        });
    untouched.then_some(0)
}

fn compatible_audio_layer(
    timeline: &Timeline,
    candidate_layer: Option<usize>,
    target_frame: u32,
) -> Option<usize> {
    candidate_layer.filter(|&index| {
        timeline.layers.get(index).is_some_and(|layer| {
            layer.properties.role == LayerRole::Normal
                && layer.properties.mask.is_none()
                && layer_effective_outline_color(timeline, layer).is_none()
                && layer_effectively_visible(timeline, layer)
                && !layer_effectively_locked(timeline, layer)
                && !layer_is_dedicated_script_layer(timeline, index)
                && layer
                    .keyframes
                    .iter()
                    .all(|key| key.elements.is_empty() && key.video.is_none())
                && layer
                    .keyframes
                    .iter()
                    .find(|key| key.frame == target_frame)
                    .and_then(|key| key.sound.as_ref())
                    .is_none()
        })
    })
}

fn compatible_video_layer(timeline: &Timeline, candidate_layer: Option<usize>) -> Option<usize> {
    candidate_layer.filter(|&index| {
        timeline.layers.get(index).is_some_and(|l| {
            l.properties.role == LayerRole::Normal
                && l.properties.mask.is_none()
                && layer_effective_outline_color(timeline, l).is_none()
                && layer_effectively_visible(timeline, l)
                && !layer_effectively_locked(timeline, l)
                && !layer_is_dedicated_script_layer(timeline, index)
                && l.keyframes
                    .iter()
                    .all(|k| k.sound.is_none() && k.video.is_none())
                && !l
                    .keyframes
                    .iter()
                    .flat_map(|k| &k.elements)
                    .any(|e| matches!(e.kind, ElementKind::Video { .. }))
        })
    })
}

fn compatible_graphic_layer(timeline: &Timeline, candidate_layer: Option<usize>) -> Option<usize> {
    candidate_layer.filter(|&index| {
        timeline.layers.get(index).is_some_and(|layer| {
            layer.properties.role == LayerRole::Normal
                && layer.properties.mask.is_none()
                && layer_effective_outline_color(timeline, layer).is_none()
                && layer_effectively_visible(timeline, layer)
                && !layer_effectively_locked(timeline, layer)
                && !layer_is_dedicated_script_layer(timeline, index)
                && layer
                    .keyframes
                    .iter()
                    .all(|k| k.sound.is_none() && k.video.is_none())
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn stage_audio_attachment(
    symbol: &mut Symbol,
    existing_layer: Option<usize>,
    frame: u32,
    fps: f64,
    duration_sec: Option<f64>,
    resource: Id,
    layer_name: &str,
    rename_default_layer: bool,
) -> (usize, u32) {
    let timeline = &mut symbol.timeline;
    let place_frame = if existing_layer.is_some() { frame } else { 0 };
    let layer_index = existing_layer.unwrap_or_else(|| {
        timeline.layers.push(Layer {
            id: new_id("layer"),
            name: layer_name.to_owned(),
            visible: true,
            locked: false,
            properties: Default::default(),
            keyframes: Vec::new(),
        });
        timeline.layers.len() - 1
    });
    let audio_frames = duration_sec
        .map(|seconds| ((seconds * fps).ceil() as u32).clamp(1, 36_000))
        .unwrap_or(1);
    let needed_end = if symbol.kind == SymbolKind::Button {
        timeline.frame_count.max(place_frame + 1).min(4)
    } else {
        place_frame.saturating_add(audio_frames)
    };
    timeline.frame_count = timeline.frame_count.max(needed_end);
    let layer = &mut timeline.layers[layer_index];
    if rename_default_layer && existing_layer == Some(0) && layer.name == "Layer 1" {
        layer.name = layer_name.to_owned();
    }
    if let Some(end) = &mut layer.properties.end_frame {
        *end = (*end).max(needed_end);
    }
    let key_index = match layer
        .keyframes
        .binary_search_by_key(&place_frame, |key| key.frame)
    {
        Ok(index) => index,
        Err(index) => {
            let elements = sample_layer_authored_pose_at_fps(layer, place_frame, fps);
            layer.keyframes.insert(
                index,
                Keyframe {
                    frame: place_frame,
                    elements,
                    ..Keyframe::default()
                },
            );
            index
        }
    };
    let key = &mut layer.keyframes[key_index];
    key.keyed_properties = None;
    key.motion_keys = None;
    key.roving = false;
    key.sound = Some(FrameSound::new(resource));
    layer.normalize_motion_keys();
    (layer_index, place_frame)
}

#[derive(Clone)]
struct PreparedFileImport {
    swcs: Vec<(PathBuf, neoflash_import::SwcLibrary)>,
    fonts: Vec<ResourceUnit>,
    audio: Vec<PreparedAudioFile>,
    videos: Vec<crate::panels::import_video::PreparedVideoFile>,
    swatches: Vec<GlobalSwatch>,
    palette_file_count: usize,
    graphics: PreparedGraphicBatch,
}

impl PreparedFileImport {
    fn category_count(&self) -> usize {
        usize::from(!self.swcs.is_empty())
            + usize::from(!self.fonts.is_empty())
            + usize::from(!self.audio.is_empty())
            + usize::from(!self.videos.is_empty())
            + usize::from(self.palette_file_count != 0)
            + usize::from(!self.graphics.is_empty())
    }
}

struct StagedSwcReport {
    path: PathBuf,
    library_name: String,
    report: neoflash_import::SwcLinkReport,
}

struct StagedGraphicPlacement {
    layer: Option<usize>,
    placed_frame: u32,
    selection: Option<Selection>,
    library_selected: Option<Id>,
    message: String,
    stops_playback: bool,
}

struct FileImportPlan {
    project: Project,
    swc_reports: Vec<StagedSwcReport>,
    audio_source_paths: Vec<(Id, PathBuf)>,
    resource_selected: Option<Id>,
    audio_attached_count: usize,
    video_imported_paths: Vec<String>,
    target_layer: Option<usize>,
    target_frame: Option<u32>,
    selection: Option<Selection>,
    library_selected: Option<Id>,
    graphic_message: Option<String>,
    stops_playback: bool,
}

impl From<PreparedGraphic> for PreparedImportGraphic {
    fn from(graphic: PreparedGraphic) -> Self {
        Self {
            graphic,
            timeline_override: None,
            symbol_kind: SymbolKind::Graphic,
            playback: InstancePlayback::SingleFrame(0),
            helper_symbols: Vec::new(),
        }
    }
}

const IMPORT_GRAPHICS_EXTENSIONS: &[&str] = &[
    "svg", "png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff", "psd", "psb", "fig", "figma",
    "json", "swf", "swc", "pdf", "ai", "eps", "fxg", "act", "clr", "ase", "mp3", "wav", "ogg",
    "flac", "aac", "m4a", "aif", "aiff", "caf", "opus", "mp4", "m4v", "mov", "webm", "ogv", "avi",
    "ttf", "otf", "ttc", "otc", "woff", "woff2",
];

fn is_figma_graphic_path(path: &std::path::Path, ext: &str) -> bool {
    if matches!(ext, "fig" | "figma") {
        return true;
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".figma.json"))
}

fn path_ext_lower(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_palette_ext(ext: &str) -> bool {
    matches!(ext, "act" | "clr" | "ase")
}

fn is_sequence_candidate_image_path(path: &std::path::Path) -> bool {
    matches!(
        path_ext_lower(path).as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff"
    )
}

/// Reject numbered image sequences where two candidate files resolve to the same frame number
/// (for example `walk_1.png` and `walk_01.png` alongside `walk_2.png`) BEFORE decoding bytes or
/// mutating the destination [`Project`] (`T-496C`, Proposal #2).
pub(crate) fn validate_unique_sequence_frame_numbers<P: AsRef<std::path::Path>>(
    inputs: &[P],
    expand_single_seed: bool,
) -> Result<(), String> {
    use std::collections::BTreeMap;

    if inputs.is_empty() {
        return Ok(());
    }

    let first = inputs[0].as_ref();
    let explicit_directory = inputs.len() == 1 && first.is_dir();
    let mut explicit_seed = false;

    let candidates: Vec<PathBuf> = if explicit_directory {
        let mut found = Vec::new();
        let entries = std::fs::read_dir(first).map_err(|e| format!("{}: {e}", first.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_file() && is_sequence_candidate_image_path(&path) {
                found.push(path);
            }
        }
        found.sort();
        found
    } else if inputs.len() == 1 && expand_single_seed && is_sequence_candidate_image_path(first) {
        let Some(file_name) = first.file_name().and_then(|s| s.to_str()) else {
            return Ok(());
        };
        let Some(seed) = neoflash_import::parse_numbered_image_filename(file_name, 0) else {
            return Ok(());
        };
        explicit_seed = true;
        let parent = first
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut found = Vec::new();
        let entries =
            std::fs::read_dir(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if !path.is_file() || !is_sequence_candidate_image_path(&path) {
                continue;
            }
            let Some(cand_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Some(item) = neoflash_import::parse_numbered_image_filename(cand_name, 0)
                && item.prefix.eq_ignore_ascii_case(&seed.prefix)
                && item.extension.eq_ignore_ascii_case(&seed.extension)
            {
                found.push(path);
            }
        }
        found.sort();
        found
    } else {
        if inputs.len() < 2
            || inputs
                .iter()
                .any(|p| !is_sequence_candidate_image_path(p.as_ref()))
        {
            return Ok(());
        }
        inputs.iter().map(|p| p.as_ref().to_path_buf()).collect()
    };

    if candidates.len() < 2 {
        return Ok(());
    }

    let mut groups: BTreeMap<(String, String), Vec<(u32, PathBuf)>> = BTreeMap::new();
    let mut unnumbered_count = 0usize;
    for (idx, path) in candidates.iter().enumerate() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if let Some(item) = neoflash_import::parse_numbered_image_filename(name, idx) {
            groups
                .entry((
                    item.prefix.to_ascii_lowercase(),
                    item.extension.to_ascii_lowercase(),
                ))
                .or_default()
                .push((item.number, path.clone()));
        } else {
            unnumbered_count += 1;
        }
    }

    if groups.is_empty() {
        return Ok(());
    }

    // Multi-file explicit lists only form a sequence when all inputs belong to one numbered family
    // (otherwise editor/CLI falls back to importing independent images such as `hero_1.png` + `villain_1.png`).
    if !explicit_directory && !explicit_seed && (unnumbered_count > 0 || groups.len() > 1) {
        return Ok(());
    }

    let Some((_, dominant)) = groups
        .into_iter()
        .max_by_key(|((p, e), items)| (items.len(), p.len(), e.len()))
    else {
        return Ok(());
    };
    if dominant.len() < 2 {
        return Ok(());
    }

    let mut seen_by_number: BTreeMap<u32, PathBuf> = BTreeMap::new();
    for (number, path) in dominant {
        if let Some(existing) = seen_by_number.get(&number) {
            return Err(format!(
                "Duplicate image sequence frame number {number}: '{}' and '{}' both resolve to frame {number}. Rename or remove one of the conflicting files before importing.",
                existing.display(),
                path.display(),
            ));
        }
        seen_by_number.insert(number, path);
    }
    Ok(())
}

fn format_import_diagnostics(report: &neoflash_import::ImportReport) -> Option<String> {
    let items: Vec<String> = report
        .diagnostics
        .iter()
        .map(|d| format!("[{}] {}: {}", d.code, d.location, d.message))
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items.join(" | "))
    }
}

/// Convert a native Stage drop into the edited symbol's local coordinates.
/// Kept pure so rotated/skewed placement math is covered without a platform drag source.
pub(crate) fn mapped_drop_point(
    mapping: StageMapping,
    origin: Pos2,
    zoom: f32,
    position: Pos2,
) -> [f64; 2] {
    let position = mapping.screen_to_edit(origin, zoom, position);
    [f64::from(position.x), f64::from(position.y)]
}

impl Editor {
    pub(crate) fn import_graphics_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Graphics, Audio & Libraries", IMPORT_GRAPHICS_EXTENSIONS)
            .pick_files()
        {
            let (figma_paths, other_paths): (Vec<PathBuf>, Vec<PathBuf>) = paths
                .into_iter()
                .partition(|p| crate::panels::figma_import::is_figma_file_path(p));
            if !other_paths.is_empty()
                && let Err(error) = self.import_graphics_paths(&other_paths, None)
            {
                self.notify_with_output(
                    neoflash_ui::components::ToastType::Error,
                    format!("Import failed: {error}"),
                );
            }
            if !figma_paths.is_empty() {
                self.enqueue_figma_import_dialog(
                    figma_paths,
                    crate::panels::figma_import::FigmaImportTarget::Stage { point: None },
                );
            }
        }
    }

    pub(crate) fn import_to_library_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Graphics, Audio & Libraries", IMPORT_GRAPHICS_EXTENSIONS)
            .pick_files()
        {
            let (figma_paths, other_paths): (Vec<PathBuf>, Vec<PathBuf>) = paths
                .into_iter()
                .partition(|p| crate::panels::figma_import::is_figma_file_path(p));
            if !other_paths.is_empty()
                && let Err(error) = self.import_graphics_to_library_paths(&other_paths)
            {
                self.notify_with_output(
                    neoflash_ui::components::ToastType::Error,
                    format!("Library import failed: {error}"),
                );
            }
            if !figma_paths.is_empty() {
                self.enqueue_figma_import_dialog(
                    figma_paths,
                    crate::panels::figma_import::FigmaImportTarget::Library,
                );
            }
        }
    }

    pub(crate) fn stage_file_drop(
        &mut self,
        ui: &egui::Ui,
        area: Rect,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        if self.beta_feedback.open {
            self.graphic_drop_position = None;
            return;
        }
        let ctx = ui.ctx();
        let pointer = Self::live_viewport_drop_pointer(ctx);
        let (hovering, dropped) = ctx.input(|input| {
            (
                !input.raw.hovered_files.is_empty(),
                input.raw.dropped_files.clone(),
            )
        });
        if hovering {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            if let Some(position) = pointer {
                self.graphic_drop_position = Some(position);
            }
            let over_other_zone = self.graphic_drop_position.is_some_and(|p| {
                self.timeline_drop_zones
                    .get(&ctx.viewport_id())
                    .is_some_and(|z| z.panel_rect.contains(p))
                    || self
                        .library_drop_rects
                        .get(&ctx.viewport_id())
                        .is_some_and(|r| r.contains(p))
            });
            if !over_other_zone {
                let hot = self.graphic_drop_position.is_some_and(|p| area.contains(p));
                let color = ui.visuals().selection.bg_fill;
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("graphic-file-drop"),
                ));
                painter.rect_filled(
                    area.shrink(neoflash_ui::tokens::space::s1()),
                    neoflash_ui::tokens::radius::sm(),
                    color.gamma_multiply(if hot { 0.3 } else { 0.12 }),
                );
                painter.text(
                    area.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drop images, video or audio onto the Stage",
                    egui::FontId::proportional(crate::theme::PANEL_FONT),
                    ui.visuals().text_color(),
                );
            }
        }
        if dropped.is_empty() {
            if !hovering {
                self.graphic_drop_position = None;
            }
            return;
        }
        // Consume once, even when a floating panel causes another viewport pass.
        ctx.input_mut(|input| input.raw.dropped_files.clear());
        let position = pointer
            .or(self.graphic_drop_position.take())
            .unwrap_or(area.center());
        let paths: Option<Vec<_>> = dropped.iter().map(|file| file.path.clone()).collect();
        let Some(paths) = paths else {
            if !area.contains(position) {
                self.message("Drop media onto the Stage, Timeline, or Library.");
            } else {
                self.message("This drop has no local file path. Save the file first, then drag that file onto the Stage.");
            }
            return;
        };
        let (as_paths, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .into_iter()
            .partition(|path| Self::is_actionscript_source_path(path));
        self.deferred_stage_script_drops.extend(as_paths);
        if paths.is_empty() {
            return;
        }
        if area.contains(position) {
            let point = mapped_drop_point(mapping, origin, self.zoom, position);
            let _ = self.import_dropped_media_into_active_document(
                ctx,
                paths,
                MediaPlacementMode::Stage { point: Some(point) },
            );
        } else if ctx.viewport_id() == egui::ViewportId::ROOT
            && position.y <= self.root_top_chrome_bottom()
        {
            // Stale cursor over top chrome (or media dropped on tab strip): place at Stage center.
            let _ = self.import_dropped_media_into_active_document(
                ctx,
                paths,
                MediaPlacementMode::Stage { point: None },
            );
        } else {
            self.message("Drop media onto the Stage, Timeline, or Library.");
        }
    }

    pub(crate) fn timeline_file_drop(&mut self, ui: &egui::Ui, zone: &TimelineDropZone) {
        if self.beta_feedback.open || self.has_modal_dialog_in(ui.ctx()) {
            return;
        }
        let ctx = ui.ctx();
        let mut pointer = Self::live_viewport_drop_pointer(ctx);
        let (hovering, dropped) = ctx.input(|input| {
            (
                !input.raw.hovered_files.is_empty(),
                input.raw.dropped_files.clone(),
            )
        });
        if !hovering && dropped.is_empty() {
            return;
        }
        let in_floating = ctx.viewport_id() != egui::ViewportId::ROOT
            || self.detached.rendering_floating_window.is_some();
        let embedded_native_drop = self
            .detached
            .rendering_floating_window
            .is_some_and(|(_, embedded_drained)| embedded_drained);
        if in_floating
            && (pointer.is_none()
                || (embedded_native_drop && !zone.panel_rect.contains(pointer.unwrap_or_default())))
        {
            pointer = Some(zone.panel_rect.center());
        }
        let Some(pos) = pointer else {
            return;
        };
        let in_zone = zone.panel_rect.contains(pos)
            || (in_floating
                && pos.x >= zone.panel_rect.left()
                && pos.x <= zone.panel_rect.right()
                && pos.y >= 0.0
                && pos.y <= zone.panel_rect.bottom());
        if !in_zone || (!embedded_native_drop && !drop_window_is_front(ctx, pos)) {
            return;
        }
        let is_button = self.symbol().kind == SymbolKind::Button;
        let placement = zone.resolve(pos, self.frame, is_button);
        if hovering {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            let color = ui.visuals().selection.bg_fill;
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new(("timeline-file-drop", ctx.viewport_id())),
            ));
            let highlight = if is_button {
                zone.highlight_rect(pos, placement.frame)
            } else {
                zone.panel_rect.shrink(neoflash_ui::tokens::space::s1())
            };
            painter.rect_filled(
                highlight,
                neoflash_ui::tokens::radius::sm(),
                color.gamma_multiply(0.22),
            );
            painter.text(
                zone.panel_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop audio, video or images onto Timeline",
                egui::FontId::proportional(crate::theme::PANEL_FONT),
                ui.visuals().text_color(),
            );
        }
        if dropped.is_empty() {
            return;
        }
        ctx.input_mut(|input| input.raw.dropped_files.clear());
        let paths: Option<Vec<_>> = dropped.iter().map(|file| file.path.clone()).collect();
        let Some(paths) = paths else {
            self.message(
                "This drop has no local file path. Save the file first, then drag it onto the Timeline.",
            );
            return;
        };
        let (as_paths, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .into_iter()
            .partition(|path| Self::is_actionscript_source_path(path));
        self.deferred_stage_script_drops.extend(as_paths);
        if paths.is_empty() {
            return;
        }
        let _ = self.import_dropped_media_into_active_document(
            ctx,
            paths,
            MediaPlacementMode::Timeline(placement),
        );
    }

    pub(crate) fn library_file_drop(&mut self, ui: &egui::Ui, area: Rect) {
        if self.beta_feedback.open || self.has_modal_dialog_in(ui.ctx()) {
            return;
        }
        let ctx = ui.ctx();
        let mut pointer = Self::live_viewport_drop_pointer(ctx);
        let (hovering, dropped) = ctx.input(|input| {
            (
                !input.raw.hovered_files.is_empty(),
                input.raw.dropped_files.clone(),
            )
        });
        if !hovering && dropped.is_empty() {
            return;
        }
        let in_floating = ctx.viewport_id() != egui::ViewportId::ROOT
            || self.detached.rendering_floating_window.is_some();
        let embedded_native_drop = self
            .detached
            .rendering_floating_window
            .is_some_and(|(_, embedded_drained)| embedded_drained);
        if in_floating
            && (pointer.is_none()
                || (embedded_native_drop && !area.contains(pointer.unwrap_or_default())))
        {
            pointer = Some(area.center());
        }
        let Some(pos) = pointer else {
            return;
        };
        let in_area = area.contains(pos)
            || (in_floating
                && pos.x >= area.left()
                && pos.x <= area.right()
                && pos.y >= 0.0
                && pos.y <= area.bottom());
        if !in_area || (!embedded_native_drop && !drop_window_is_front(ctx, pos)) {
            return;
        }
        if hovering {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            let color = ui.visuals().selection.bg_fill;
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new(("library-file-drop", ctx.viewport_id())),
            ));
            painter.rect_filled(
                area.shrink(neoflash_ui::tokens::space::s1()),
                neoflash_ui::tokens::radius::sm(),
                color.gamma_multiply(0.22),
            );
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Drop audio, video or images into Library",
                egui::FontId::proportional(crate::theme::PANEL_FONT),
                ui.visuals().text_color(),
            );
        }
        if dropped.is_empty() {
            return;
        }
        ctx.input_mut(|input| input.raw.dropped_files.clear());
        let paths: Option<Vec<_>> = dropped.iter().map(|file| file.path.clone()).collect();
        let Some(paths) = paths else {
            self.message(
                "This drop has no local file path. Save the file first, then drag it into the Library.",
            );
            return;
        };
        let (as_paths, paths): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .into_iter()
            .partition(|path| Self::is_actionscript_source_path(path));
        self.deferred_stage_script_drops.extend(as_paths);
        if paths.is_empty() {
            return;
        }
        let _ = self.import_dropped_media_into_active_document(
            ctx,
            paths,
            MediaPlacementMode::LibraryOnly,
        );
    }

    pub(crate) fn import_dropped_media_into_active_document(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        mode: MediaPlacementMode,
    ) -> bool {
        if paths.is_empty() {
            return false;
        }
        if paths
            .iter()
            .any(|path| is_known_unsupported_video_path(path))
        {
            self.message(
                "Video import supports H.264 MP4/M4V/MOV, VP9 WebM, Ogg and AVI video. Transcode this file, then drop it again.",
            );
            return false;
        }
        let (figma_paths, other_paths): (Vec<PathBuf>, Vec<PathBuf>) = match mode {
            MediaPlacementMode::Stage { .. } | MediaPlacementMode::LibraryOnly => paths
                .into_iter()
                .partition(|path| crate::panels::figma_import::is_figma_file_path(path)),
            MediaPlacementMode::Timeline(_) => (Vec::new(), paths),
        };
        let mut succeeded = false;
        if !other_paths.is_empty() {
            let only_videos = other_paths.iter().all(|p| is_supported_video_path(p));
            match self.import_media_paths(&other_paths, mode) {
                Ok(()) => succeeded = true,
                Err(error) => {
                    if only_videos {
                        self.message(format!("Video import failed: {error}"));
                    } else {
                        self.notify_with_output(
                            neoflash_ui::components::ToastType::Error,
                            format!("Import failed: {error}"),
                        );
                    }
                }
            }
        }
        if !figma_paths.is_empty() {
            let target = match mode {
                MediaPlacementMode::Stage { point } => {
                    crate::panels::figma_import::FigmaImportTarget::Stage { point }
                }
                MediaPlacementMode::LibraryOnly | MediaPlacementMode::Timeline(_) => {
                    // `timeline_file_drop` partitions `.fig` paths into `Stage { point: None }`;
                    // any non-Stage mode stays side-effect-free on Stage/Timeline.
                    crate::panels::figma_import::FigmaImportTarget::Library
                }
            };
            self.open_figma_import_dialog(ctx, figma_paths, target);
            succeeded = true;
        }
        succeeded
    }

    pub(crate) fn stage_library_drop(
        &mut self,
        ui: &egui::Ui,
        area: Rect,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        if self.library_panel.drag.is_none() {
            return;
        }
        let ctx = ui.ctx();
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.library_panel.drag = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let pointer = crate::native_screens::pointer_state()
            .and_then(|(global, down)| {
                ctx.input(|input| input.viewport().inner_rect)
                    .map(|inner| (global / ctx.zoom_factor() - inner.min.to_vec2(), down))
            })
            .or_else(|| {
                ctx.input(|input| {
                    input
                        .pointer
                        .latest_pos()
                        .map(|p| (p, input.pointer.primary_down()))
                })
            });
        let Some((position, down)) = pointer else {
            return;
        };
        let alt = ctx.input(|input| input.modifiers.alt);
        let hit = area.contains(position) && drop_window_is_front(ctx, position);
        if hit {
            match &self.library_panel.drag {
                Some(crate::library::LibraryDrag::Symbol(sym_id)) => {
                    let sym_id = sym_id.clone();
                    let new_name = self
                        .project
                        .symbol(&sym_id)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| sym_id.clone());
                    let swap_target = alt
                        .then(|| self.stage_swap_target_info(ui, area, origin, mapping, &sym_id))
                        .flatten();
                    if let Some((_, target_rect)) = &swap_target
                        && target_rect.is_finite()
                        && target_rect.is_positive()
                    {
                        neoflash_ui::components::canvas::paint_selection_box(
                            &ui.painter_at(area),
                            ctx,
                            *target_rect,
                            neoflash_ui::components::canvas::SelectionKind::Symbol,
                        );
                    }
                    let label = if let Some((old_name, _)) = swap_target {
                        format!("Replace “{old_name}” with “{new_name}”")
                    } else {
                        format!("Place “{new_name}”")
                    };
                    let ghost_painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Tooltip,
                        egui::Id::new("library-symbol-stage-drag-ghost"),
                    ));
                    neoflash_ui::components::drag_ghost_at(
                        &ghost_painter,
                        ctx,
                        position + egui::vec2(12., 12.),
                        &label,
                    );
                }
                Some(crate::library::LibraryDrag::Resource(_)) => {
                    ui.painter_at(area).rect_stroke(
                        area.shrink(3.),
                        2.,
                        egui::Stroke::new(2.0_f32, ui.visuals().selection.stroke.color),
                        egui::StrokeKind::Inside,
                    );
                    ctx.set_cursor_icon(egui::CursorIcon::Copy);
                }
                Some(crate::library::LibraryDrag::Folder(_)) | None => {}
            }
        }
        if down {
            return;
        }
        if !hit {
            let floating_timeline_open = self.workspace.floating.iter().any(|w| {
                w.panels().contains(&crate::workspace::PanelId::Timeline)
                    && self
                        .workspace
                        .is_panel_visible(crate::workspace::PanelId::Timeline)
            });
            if !floating_timeline_open
                && !(alt
                    && matches!(
                        self.library_panel.drag,
                        Some(crate::library::LibraryDrag::Symbol(_))
                    ))
            {
                self.library_panel.drag = None;
            }
            return;
        }
        let payload = self.library_panel.drag.take().unwrap();
        self.cancel_selection_gesture();
        if alt
            && let crate::library::LibraryDrag::Symbol(ref id) = payload
            && (self.can_swap_instance_symbol(id) || self.can_swap_layer_symbol(self.layer, id))
        {
            self.swap_layer_or_selected_symbol(self.layer, id);
            return;
        }
        let point = Some(mapped_drop_point(mapping, origin, self.zoom, position));
        let result = match payload {
            crate::library::LibraryDrag::Symbol(id) => self.place_library_symbol(ctx, &id, point),
            crate::library::LibraryDrag::Resource(id) => {
                self.place_library_resource(&id, MediaPlacementMode::Stage { point })
            }
            crate::library::LibraryDrag::Folder(_) => Ok(()),
        };
        if let Err(error) = result {
            self.message(format!("Cannot place on Stage: {error}"));
        }
    }

    pub(crate) fn timeline_library_drop(
        &mut self,
        ui: &egui::Ui,
        zone: &TimelineDropZone,
        p: &neoflash_ui::tokens::Palette,
    ) {
        let Some(drag_payload) = self.library_panel.drag.clone() else {
            return;
        };
        if matches!(drag_payload, crate::library::LibraryDrag::Folder(_)) {
            return;
        }
        let ctx = ui.ctx();
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.library_panel.drag = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let pointer = crate::native_screens::pointer_state()
            .and_then(|(global, down)| {
                ctx.input(|input| input.viewport().inner_rect)
                    .map(|inner| (global / ctx.zoom_factor() - inner.min.to_vec2(), down))
            })
            .or_else(|| {
                ctx.input(|input| {
                    let mut released = None;
                    for event in &input.events {
                        if let egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            ..
                        } = event
                        {
                            released = Some(*pos);
                        }
                    }
                    input
                        .pointer
                        .latest_pos()
                        .or(released)
                        .map(|pt| (pt, input.pointer.primary_down()))
                })
            });
        let Some((pos, down)) = pointer else {
            return;
        };
        let hit = zone.panel_rect.contains(pos) && drop_window_is_front(ctx, pos);
        if !hit {
            return;
        }
        let alt = ctx.input(|input| input.modifiers.alt);
        let is_button = self.symbol().kind == SymbolKind::Button;
        let placement = zone.resolve(pos, self.frame, is_button);

        if alt
            && let crate::library::LibraryDrag::Symbol(ref sym_id) = drag_payload
            && (zone.rows_inner_rect.contains(pos) || zone.grid_clip.contains(pos))
            && let Some(target_layer_idx) = placement.hovered_layer
            && self.can_swap_layer_symbol(target_layer_idx, sym_id)
        {
            if let Some(row_idx) = zone
                .visible_layer_indices
                .iter()
                .position(|&idx| idx == target_layer_idx)
            {
                let highlight = Rect::from_min_max(
                    Pos2::new(
                        zone.rows_inner_rect.left(),
                        zone.list_origin_y + row_idx as f32 * zone.row_height,
                    ),
                    Pos2::new(
                        zone.rows_inner_rect.right(),
                        zone.list_origin_y + (row_idx + 1) as f32 * zone.row_height,
                    ),
                )
                .intersect(zone.rows_inner_rect);
                let hp = ui.painter_at(zone.rows_inner_rect);
                hp.rect_filled(highlight, 0., p.timeline_row_selected);
                hp.rect_stroke(
                    highlight,
                    0.,
                    egui::Stroke::new(1.0_f32, p.accent_default),
                    egui::StrokeKind::Inside,
                );
            }
            let layer_name = self.symbol().timeline.layers[target_layer_idx].name.clone();
            let ghost_painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("library-symbol-timeline-swap-ghost"),
            ));
            neoflash_ui::components::drag_ghost_at(
                &ghost_painter,
                ctx,
                pos + egui::vec2(12., 12.),
                &format!("Replace on all keyframes of “{layer_name}”"),
            );
            if !down {
                self.library_panel.drag = None;
                let previous_layer = self.layer;
                self.layer = target_layer_idx;
                if self.layer != previous_layer {
                    self.clear_timeline_frame_selection();
                }
                self.swap_layer_or_selected_symbol(target_layer_idx, sym_id);
            }
            return;
        }

        let item_name = match &drag_payload {
            crate::library::LibraryDrag::Symbol(sym_id) => self
                .project
                .symbol(sym_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| sym_id.clone()),
            crate::library::LibraryDrag::Resource(res_id) => self
                .project
                .resources
                .iter()
                .find(|r| &r.id == res_id)
                .map(|r| {
                    std::path::Path::new(&r.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(r.path.as_str())
                        .to_owned()
                })
                .unwrap_or_else(|| res_id.clone()),
            crate::library::LibraryDrag::Folder(_) => String::new(),
        };
        let will_reuse_row = match &drag_payload {
            crate::library::LibraryDrag::Symbol(_) => {
                compatible_graphic_layer(&self.symbol().timeline, placement.hovered_layer).is_some()
            }
            crate::library::LibraryDrag::Resource(res_id) => self
                .project
                .resources
                .iter()
                .find(|r| &r.id == res_id)
                .is_some_and(|unit| {
                    if unit.is_audio() {
                        compatible_audio_layer(
                            &self.symbol().timeline,
                            placement.hovered_layer,
                            placement.frame,
                        )
                        .is_some()
                    } else if unit.is_video() {
                        compatible_video_layer(&self.symbol().timeline, placement.hovered_layer)
                            .is_some()
                    } else {
                        compatible_graphic_layer(&self.symbol().timeline, placement.hovered_layer)
                            .is_some()
                    }
                }),
            crate::library::LibraryDrag::Folder(_) => false,
        };
        let preview_frame = if will_reuse_row { placement.frame } else { 0 };
        let highlight = if will_reuse_row {
            zone.highlight_rect(pos, preview_frame)
        } else {
            zone.panel_rect.shrink(neoflash_ui::tokens::space::s1())
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new(("library-timeline-drop", ctx.viewport_id())),
        ));
        painter.rect_filled(
            highlight,
            neoflash_ui::tokens::radius::sm(),
            ui.visuals().selection.bg_fill.gamma_multiply(0.22),
        );
        painter.rect_stroke(
            highlight,
            neoflash_ui::tokens::radius::sm(),
            egui::Stroke::new(1.0_f32, p.accent_default),
            egui::StrokeKind::Inside,
        );
        neoflash_ui::components::drag_ghost_at(
            &painter,
            ctx,
            pos + egui::vec2(12., 12.),
            &format!("Place “{item_name}” at frame {}", preview_frame + 1),
        );
        ctx.set_cursor_icon(egui::CursorIcon::Copy);

        if down {
            return;
        }
        let payload = self.library_panel.drag.take().unwrap();
        self.cancel_selection_gesture();
        let result = match payload {
            crate::library::LibraryDrag::Symbol(id) => self.place_library_symbol_with_mode(
                ctx,
                &id,
                MediaPlacementMode::Timeline(placement),
            ),
            crate::library::LibraryDrag::Resource(id) => {
                self.place_library_resource(&id, MediaPlacementMode::Timeline(placement))
            }
            crate::library::LibraryDrag::Folder(_) => Ok(()),
        };
        if let Err(error) = result {
            self.message(format!("Cannot place on Timeline: {error}"));
        }
    }

    fn stage_swap_target_info(
        &self,
        ui: &egui::Ui,
        area: Rect,
        origin: Pos2,
        mapping: StageMapping,
        new_symbol_id: &str,
    ) -> Option<(String, Rect)> {
        let mut target_el_id: Option<Id> = None;
        let mut old_sym_id: Option<Id> = None;
        if self.can_swap_instance_symbol(new_symbol_id) {
            for (target, el) in self.sampled_targets() {
                if !self.target_editable(&target) {
                    continue;
                }
                if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                    target_el_id = Some(el.id);
                    old_sym_id = Some(symbol_id.clone());
                    break;
                }
            }
        }
        if old_sym_id.is_none() && self.can_swap_layer_symbol(self.layer, new_symbol_id) {
            let timeline = &self.symbol().timeline;
            if let Some(layer) = timeline.layers.get(self.layer) {
                for el in
                    sample_layer_in_at_fps(timeline, layer, self.frame, f64::from(self.project.fps))
                {
                    if let ElementKind::Instance { symbol_id, .. } = &el.kind {
                        target_el_id = Some(el.id);
                        old_sym_id = Some(symbol_id.clone());
                        break;
                    }
                }
                if old_sym_id.is_none() {
                    old_sym_id = layer
                        .keyframes
                        .iter()
                        .flat_map(|k| &k.elements)
                        .find_map(|el| match &el.kind {
                            ElementKind::Instance { symbol_id, .. } => Some(symbol_id.clone()),
                            _ => None,
                        });
                }
            }
        }
        let old_sym_id = old_sym_id?;
        let old_name = self
            .project
            .symbol(&old_sym_id)
            .map(|s| s.name.clone())
            .unwrap_or(old_sym_id);
        let mut screen_rect = Rect::NOTHING;
        if let Some(el_id) = target_el_id {
            let painter = ui.painter_at(area);
            let scene = self.presented_authored_scene(self.show_layer_masking(ui.ctx()));
            let context_bounds = scene
                .draws
                .iter()
                .filter(|d| d.item.instance_path.first() == Some(&el_id))
                .fold(Rect::NOTHING, |acc, d| {
                    acc.union(crate::stage_selection::mapped_world_bounds(
                        &d.item, &painter, mapping,
                    ))
                });
            if context_bounds.is_finite() && context_bounds.is_positive() {
                screen_rect = Rect::from_min_max(
                    origin + context_bounds.min.to_vec2() * self.zoom,
                    origin + context_bounds.max.to_vec2() * self.zoom,
                );
            }
        }
        Some((old_name, screen_rect))
    }

    pub(crate) fn import_graphics_paths(
        &mut self,
        paths: &[PathBuf],
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        self.import_media_paths(paths, MediaPlacementMode::Stage { point })
    }

    pub(crate) fn import_graphics_to_library_paths(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<(), String> {
        self.import_media_paths(paths, MediaPlacementMode::LibraryOnly)
    }

    pub(crate) fn import_media_paths(
        &mut self,
        paths: &[PathBuf],
        mode: MediaPlacementMode,
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        if paths.len() > 64 {
            return Err("Import at most 64 graphics at once.".into());
        }
        if paths
            .iter()
            .any(|path| is_known_unsupported_video_path(path))
        {
            return Err(
                "Video import supports H.264 MP4/M4V/MOV, VP9 WebM, Ogg and AVI video. Transcode this file, then drop it again."
                    .into(),
            );
        }

        let place_on_stage = mode.places_on_timeline_or_stage();

        // Partition SWC libraries, audio files, video files, font files, colour palettes, and graphic/interchange files.
        let mut swc_paths = Vec::new();
        let mut audio_paths = Vec::new();
        let mut video_paths = Vec::new();
        let mut font_paths = Vec::new();
        let mut palette_paths = Vec::new();
        let mut graphic_paths = Vec::new();
        for path in paths {
            let ext = path_ext_lower(path);
            if ext == "swc" {
                swc_paths.push(path.clone());
            } else if crate::resources::is_audio_import_path(path) {
                audio_paths.push(path.clone());
            } else if is_supported_video_path(path) {
                video_paths.push(path.clone());
            } else if crate::resources::is_font_import_path(path) {
                font_paths.push(path.clone());
            } else if is_palette_ext(&ext) {
                palette_paths.push(path.clone());
            } else {
                graphic_paths.push(path.clone());
            }
        }

        // Decode every external input before mutating editor or document state.
        let mut parsed_swatches = Vec::new();
        for pal_path in &palette_paths {
            let display = pal_path.display().to_string();
            let bytes = std::fs::read(pal_path).map_err(|e| format!("{display}: {e}"))?;
            let mut swatches = Self::parse_palette_file_bytes(&bytes, &display)?;
            parsed_swatches.append(&mut swatches);
        }

        let used = self
            .project
            .resources
            .iter()
            .try_fold(0usize, |total, resource| {
                total.checked_add(resource.byte_len())
            })
            .ok_or("Project resource byte count overflowed.")?;
        let mut remaining = MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used);

        let mut items = Vec::with_capacity(graphic_paths.len());
        let seq_opts = self.workspace.sequence_import_options.clone();
        let prepared_graphics = if graphic_paths.is_empty() {
            PreparedGraphicBatch::None
        } else {
            let sequence_request =
                graphic_paths.len() >= 2 || (graphic_paths.len() == 1 && graphic_paths[0].is_dir());
            let imported_sequence = if sequence_request {
                validate_unique_sequence_frame_numbers(&graphic_paths, false)
                    .map_err(|error| format!("Image sequence import failed: {error}"))?;
                neoflash_import::import_numbered_image_sequence_paths_with_options(
                    &graphic_paths,
                    false,
                    &seq_opts,
                )
                .map_err(|error| format!("Image sequence import failed: {error}"))?
            } else {
                None
            };

            if let Some(imported_sequence) = imported_sequence {
                if place_on_stage
                    && seq_opts.placement
                        == neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer
                {
                    PreparedGraphicBatch::Keyframes(Box::new(prepare_sequence_graphic(
                        imported_sequence,
                        neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer,
                        &mut remaining,
                    )?))
                } else {
                    let seq_item = prepare_sequence_graphic(
                        imported_sequence,
                        seq_opts.placement,
                        &mut remaining,
                    )?;
                    items.push(seq_item);
                    PreparedGraphicBatch::Symbols(items)
                }
            } else {
                for path in &graphic_paths {
                    let ext = path_ext_lower(path);
                    let item = if is_figma_graphic_path(path, &ext) {
                        prepare_figma_graphic(path, &mut remaining)?
                    } else {
                        match ext.as_str() {
                            "psd" | "psb" => prepare_psd_graphic(path, &mut remaining)?,
                            "swf" => prepare_swf_graphic(path, &mut remaining)?,
                            "pdf" | "ai" | "eps" => prepare_pdf_ai_graphic(path, &mut remaining)?,
                            "fxg" => prepare_fxg_graphic(path)?,
                            _ => {
                                let unit = crate::resources::read_resource(path, remaining)?;
                                remaining = remaining.saturating_sub(unit.byte_len());
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Graphic")
                                    .to_owned();
                                PreparedImportGraphic::from(prepare_graphic(unit, name)?)
                            }
                        }
                    };
                    items.push(item);
                }
                PreparedGraphicBatch::Symbols(items)
            }
        };

        let mut loaded_swcs = Vec::with_capacity(swc_paths.len());
        for swc_path in &swc_paths {
            let library = neoflash_import::import_swc(swc_path)
                .map_err(|error| format!("{}: {error}", swc_path.display()))?;
            loaded_swcs.push((swc_path.clone(), library));
        }

        let mut fonts = Vec::with_capacity(font_paths.len());
        for font_path in &font_paths {
            let unit = crate::resources::read_font_resource(font_path, remaining)?;
            remaining = remaining.saturating_sub(unit.byte_len());
            fonts.push(unit);
        }
        let mut audio = Vec::with_capacity(audio_paths.len());
        for audio_path in &audio_paths {
            let unit = crate::resources::read_audio_resource(audio_path, remaining)?;
            remaining = remaining.saturating_sub(unit.byte_len());
            let duration_sec = if place_on_stage {
                import_audio_duration(&unit)
            } else {
                None
            };
            let stem = audio_path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Sound")
                .to_owned();
            audio.push(PreparedAudioFile {
                path: audio_path.clone(),
                stem,
                unit,
                duration_sec,
            });
        }
        let mut videos = Vec::with_capacity(video_paths.len());
        for video_path in &video_paths {
            let prepared_video = self.prepare_video_file_for_import(video_path, &mut remaining)?;
            videos.push(prepared_video);
        }

        let prepared = PreparedFileImport {
            swcs: loaded_swcs,
            fonts,
            audio,
            videos,
            swatches: parsed_swatches,
            palette_file_count: palette_paths.len(),
            graphics: prepared_graphics,
        };

        let baseline = self.project.clone();
        let mut plan = self.build_file_import_plan(baseline.clone(), &prepared, mode)?;
        self.finish_edits();
        if self.project != baseline {
            drop(plan);
            plan = self.build_file_import_plan(self.project.clone(), &prepared, mode)?;
        }

        let label = self.file_import_history_label(&prepared, place_on_stage);
        let FileImportPlan {
            project,
            swc_reports,
            audio_source_paths,
            resource_selected,
            audio_attached_count,
            video_imported_paths,
            target_layer,
            target_frame,
            selection,
            library_selected,
            graphic_message,
            stops_playback,
        } = plan;
        if stops_playback {
            self.set_playing(false);
        }
        let clears_timeline_range = target_layer.is_some() || target_frame.is_some();
        self.transact_named(label, |editor| {
            editor.project = project;
            if let Some(layer) = target_layer {
                editor.layer = layer;
            }
            if let Some(frame) = target_frame {
                let last = editor.symbol().timeline.frame_count.saturating_sub(1);
                editor.frame = frame.min(last);
            }
            if clears_timeline_range {
                editor.clear_timeline_frame_selection();
            }
            if let Some(selection) = selection {
                editor.selection = selection;
            }
            if let Some(symbol) = library_selected {
                editor.library_selected = Some(symbol);
            }
        });

        for (id, path) in audio_source_paths {
            self.timeline_audio.source_paths.insert(id, path);
        }
        if resource_selected.is_some() {
            self.resource_panel.selected = resource_selected;
            self.resource_panel.baseline = None;
        }

        self.emit_swc_import_reports(&swc_reports);
        if !prepared.fonts.is_empty() {
            self.show_panel(crate::workspace::PanelId::Library);
            self.message(format!(
                "Imported {} font file(s) into the Library.",
                prepared.fonts.len()
            ));
        }
        if !prepared.audio.is_empty() {
            if audio_attached_count > 0 {
                if prepared.graphics.is_empty() && prepared.videos.is_empty() {
                    self.show_frame_sound();
                } else {
                    self.show_panel(crate::workspace::PanelId::Properties);
                }
                self.message(format!(
                    "Imported {} audio file(s) and attached sound to the timeline.",
                    prepared.audio.len()
                ));
            } else {
                self.show_panel(crate::workspace::PanelId::Library);
                self.message(format!(
                    "Imported {} audio file(s) into the Library.",
                    prepared.audio.len()
                ));
            }
        }
        if !video_imported_paths.is_empty() {
            if place_on_stage {
                if video_imported_paths.len() == 1
                    && prepared.graphics.is_empty()
                    && prepared.audio.is_empty()
                {
                    self.message(format!("Imported video {}.", video_imported_paths[0]));
                } else {
                    self.message(format!(
                        "Imported {} video file(s).",
                        video_imported_paths.len()
                    ));
                }
            } else {
                self.show_panel(crate::workspace::PanelId::Library);
                self.message(format!(
                    "Imported {} video file(s) into the Library.",
                    video_imported_paths.len()
                ));
            }
        }
        if prepared.graphics.is_empty()
            && prepared.videos.is_empty()
            && prepared.palette_file_count != 0
        {
            self.message(format!(
                "Imported {} swatch(es) from {} palette file(s).",
                prepared.swatches.len(),
                prepared.palette_file_count
            ));
        }
        if let Some(message) = graphic_message {
            self.message(message);
        }
        Ok(())
    }

    fn build_file_import_plan(
        &self,
        mut candidate: Project,
        prepared: &PreparedFileImport,
        mode: MediaPlacementMode,
    ) -> Result<FileImportPlan, String> {
        let place_on_stage = mode.places_on_timeline_or_stage();
        let mut swc_reports = Vec::with_capacity(prepared.swcs.len());
        for (path, library) in &prepared.swcs {
            let report = neoflash_import::link_swc_into_project(
                &mut candidate,
                library,
                neoflash_import::SwcConflictPolicy::PreferNewer,
            )
            .map_err(|error| format!("{}: {error}", path.display()))?;
            swc_reports.push(StagedSwcReport {
                path: path.clone(),
                library_name: library.name.clone(),
                report,
            });
        }

        let mut resource_selected = None;
        if !prepared.fonts.is_empty() {
            if prepared.fonts.len() + candidate.resources.len() > neoflash_core::MAX_RESOURCES {
                return Err("The project supports at most 1024 resource files.".into());
            }
            candidate.format_version = candidate.format_version.max(3);
            for prepared_unit in &prepared.fonts {
                let mut unit = prepared_unit.clone();
                unit.path = crate::resources::unique_imported_resource_path(
                    &candidate.resources,
                    &unit.path,
                );
                resource_selected = Some(unit.id.clone());
                candidate.resources.push(unit.clone());
                crate::resources::upsert_imported_font_embedding_metadata(
                    &mut candidate.resources,
                    &unit,
                )?;
            }
        }

        if !prepared.audio.is_empty() {
            if prepared.audio.len() + candidate.resources.len() > neoflash_core::MAX_RESOURCES {
                return Err("The project supports at most 1024 resource files.".into());
            }
            candidate.format_version = candidate.format_version.max(3);
            for audio_item in &prepared.audio {
                let mut unit = audio_item.unit.clone();
                unit.path = crate::resources::unique_imported_resource_path(
                    &candidate.resources,
                    &unit.path,
                );
                resource_selected = Some(unit.id.clone());
                candidate.resources.push(unit);
            }
        }

        let mut staged_videos = Vec::with_capacity(prepared.videos.len());
        let mut video_imported_paths = Vec::with_capacity(prepared.videos.len());
        for video in &prepared.videos {
            let (res_id, rel_path) = Self::upsert_prepared_video_resource(&mut candidate, video)?;
            resource_selected = Some(res_id.clone());
            video_imported_paths.push(rel_path);
            staged_videos.push((res_id, video));
        }

        if !prepared.swatches.is_empty() {
            Self::merge_swatches_into_project(&mut candidate, &prepared.swatches);
        }

        if !place_on_stage {
            let graphic_placement = match &prepared.graphics {
                PreparedGraphicBatch::None => None,
                PreparedGraphicBatch::Symbols(items) => Some(self.stage_prepared_import_graphics(
                    &mut candidate,
                    items.clone(),
                    None,
                    false,
                )?),
                PreparedGraphicBatch::Keyframes(item) => {
                    Some(self.stage_prepared_import_graphics(
                        &mut candidate,
                        vec![item.as_ref().clone()],
                        None,
                        false,
                    )?)
                }
            };
            let (library_selected, graphic_message) = match graphic_placement {
                Some(staged) => (staged.library_selected, Some(staged.message)),
                None => (None, None),
            };
            candidate.pin_displaced_frame_script_layers(&self.project);
            crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
            candidate.validate().map_err(|error| error.to_string())?;
            let audio_source_paths = prepared
                .audio
                .iter()
                .map(|a| (a.unit.id.clone(), a.path.clone()))
                .collect();
            return Ok(FileImportPlan {
                project: candidate,
                swc_reports,
                audio_source_paths,
                resource_selected,
                audio_attached_count: 0,
                video_imported_paths,
                target_layer: None,
                target_frame: None,
                selection: None,
                library_selected,
                graphic_message,
                stops_playback: false,
            });
        }

        let before_timeline_plan = candidate.clone();
        let symbol_id = self.symbol_id().to_owned();
        let is_button = candidate
            .symbol(&symbol_id)
            .is_some_and(|s| s.kind == SymbolKind::Button);
        let is_timeline_drop = matches!(mode, MediaPlacementMode::Timeline(_));
        let separate_graphic_layers =
            is_timeline_drop || !staged_videos.is_empty() || prepared.audio.len() > 1;
        let graphic_track_count = match &prepared.graphics {
            PreparedGraphicBatch::None => 0,
            PreparedGraphicBatch::Keyframes(_) => 1,
            PreparedGraphicBatch::Symbols(items) => {
                if separate_graphic_layers {
                    items.len()
                } else {
                    usize::from(!items.is_empty())
                }
            }
        };
        let total_tracks = prepared.audio.len() + staged_videos.len() + graphic_track_count;
        let single_track = total_tracks == 1;
        let (base_frame, candidate_layer_idx, rename_blank_timeline_layer) = match mode {
            MediaPlacementMode::Stage { .. } | MediaPlacementMode::LibraryOnly => (
                if is_button {
                    self.frame.min(3)
                } else {
                    self.frame.min(999_999)
                },
                Some(self.layer),
                false,
            ),
            MediaPlacementMode::Timeline(tl) => {
                if is_button {
                    if single_track {
                        (tl.frame.min(3), tl.hovered_layer, false)
                    } else {
                        (0, None, false)
                    }
                } else {
                    // After Effects–style Finder drop onto Timeline: imported tracks always start
                    // at Frame 1 (`frame = 0`) without leading blank frames; reuse the sole
                    // untouched empty `Layer 1` on a brand-new document, otherwise create
                    // dedicated stem-named tracks above existing layers.
                    let blank_sole = single_track
                        .then(|| {
                            candidate
                                .symbol(&symbol_id)
                                .and_then(|sym| sole_untouched_blank_layer(&sym.timeline))
                        })
                        .flatten();
                    (0, blank_sole, blank_sole.is_some())
                }
            }
        };
        let stage_shared_empty_layer = !is_timeline_drop
            && staged_videos.is_empty()
            && prepared.audio.len() == 1
            && graphic_track_count == 1
            && candidate_layer_idx.is_some_and(|idx| {
                candidate.symbol(&symbol_id).is_some_and(|sym| {
                    let tl = &sym.timeline;
                    !layer_is_dedicated_script_layer(tl, idx)
                        && tl.layers.get(idx).is_some_and(|l| {
                            l.properties.role == LayerRole::Normal
                                && l.properties.mask.is_none()
                                && layer_effective_outline_color(tl, l).is_none()
                                && layer_effectively_visible(tl, l)
                                && !layer_effectively_locked(tl, l)
                                && l.keyframes.iter().all(|k| {
                                    k.elements.is_empty() && k.video.is_none() && k.sound.is_none()
                                })
                        })
                })
            });

        let mut final_layer: Option<usize> = None;
        let mut resolved_frame: Option<u32> = None;
        let mut selection_targets: Vec<SelectionTarget> = Vec::new();
        let mut stops_playback = false;
        let mut last_symbol: Option<Id> = None;
        let mut graphic_message: Option<String> = None;
        let mut audio_attached_count = 0usize;

        if stage_shared_empty_layer {
            match &prepared.graphics {
                PreparedGraphicBatch::None => {}
                PreparedGraphicBatch::Symbols(items) => {
                    let staged = self.stage_prepared_import_graphics_at(
                        &mut candidate,
                        items.clone(),
                        mode.stage_point(),
                        true,
                        base_frame,
                        candidate_layer_idx,
                        false,
                        false,
                    )?;
                    if let Some(layer) = staged.layer {
                        final_layer = Some(layer);
                    }
                    resolved_frame = Some(staged.placed_frame);
                    if let Some(sel) = staged.selection {
                        selection_targets.extend(sel.targets);
                    }
                    last_symbol = staged.library_selected;
                    graphic_message = Some(staged.message);
                    stops_playback |= staged.stops_playback;
                }
                PreparedGraphicBatch::Keyframes(item) => {
                    let staged = self.stage_sequence_keyframes_at(
                        &mut candidate,
                        item.as_ref().clone(),
                        mode.stage_point(),
                        base_frame,
                        candidate_layer_idx,
                        false,
                    )?;
                    if let Some(layer) = staged.layer {
                        final_layer = Some(layer);
                    }
                    resolved_frame = Some(staged.placed_frame);
                    if let Some(sel) = staged.selection {
                        selection_targets.extend(sel.targets);
                    }
                    last_symbol = staged.library_selected;
                    graphic_message = Some(staged.message);
                    stops_playback |= staged.stops_playback;
                }
            }
        }

        // 1. Place audio track(s).
        for audio_item in &prepared.audio {
            let fps = f64::from(candidate.fps.max(1));
            let symbol = candidate
                .symbol_mut(&symbol_id)
                .ok_or("The current symbol was removed.")?;
            let existing_layer = if stage_shared_empty_layer {
                candidate_layer_idx
            } else if single_track {
                compatible_audio_layer(&symbol.timeline, candidate_layer_idx, base_frame)
            } else {
                None
            };
            let place_at = resolved_frame.unwrap_or(base_frame);
            let (layer, placed_frame) = stage_audio_attachment(
                symbol,
                existing_layer,
                place_at,
                fps,
                audio_item.duration_sec,
                audio_item.unit.id.clone(),
                &audio_item.stem,
                rename_blank_timeline_layer,
            );
            final_layer = Some(layer);
            resolved_frame = Some(placed_frame);
            audio_attached_count += 1;
        }

        // 2. Place video track(s).
        if !staged_videos.is_empty() {
            let base_center = mode.stage_point().unwrap_or_else(|| self.paste_center());
            let fps = f64::from(candidate.fps.max(1));
            for (v_idx, (res_id, video)) in staged_videos.iter().enumerate() {
                let timeline = &mut candidate
                    .symbol_mut(&symbol_id)
                    .ok_or("The current symbol was removed.")?
                    .timeline;
                let existing_layer = if single_track {
                    compatible_video_layer(timeline, candidate_layer_idx)
                } else {
                    None
                };
                let place_frame = if existing_layer.is_some() {
                    base_frame
                } else {
                    0
                };
                let layer_idx = existing_layer.unwrap_or_else(|| {
                    timeline.layers.push(Layer {
                        id: new_id("layer"),
                        name: video.stem.clone(),
                        visible: true,
                        locked: false,
                        properties: Default::default(),
                        keyframes: Vec::new(),
                    });
                    timeline.layers.len() - 1
                });
                let video_frames = ((video.duration_sec * fps).ceil() as u32).max(1);
                let needed_end = if is_button {
                    timeline.frame_count.max(place_frame + 1).min(4)
                } else {
                    place_frame.saturating_add(video_frames)
                };
                timeline.frame_count = timeline.frame_count.max(needed_end);
                let max_tl = timeline.frame_count;
                let layer = &mut timeline.layers[layer_idx];
                if rename_blank_timeline_layer
                    && existing_layer == Some(0)
                    && layer.name == "Layer 1"
                {
                    layer.name = video.stem.clone();
                }
                if let Some(end) = &mut layer.properties.end_frame
                    && *end < needed_end.min(max_tl)
                {
                    *end = needed_end.min(max_tl);
                }
                if !layer.keyframes.iter().any(|k| k.frame == place_frame) {
                    let elems = sample_layer_authored_pose_at_fps(layer, place_frame, fps);
                    layer.keyframes.push(Keyframe {
                        frame: place_frame,
                        elements: elems,
                        ..Keyframe::default()
                    });
                    layer.keyframes.sort_by_key(|k| k.frame);
                }
                let layer_id = layer.id.clone();
                let key = layer
                    .keyframes
                    .iter_mut()
                    .find(|k| k.frame == place_frame)
                    .ok_or("Could not create a video import keyframe.")?;
                key.keyed_properties = None;
                key.motion_keys = None;
                key.roving = false;
                key.video = Some(TimelineVideoSpan {
                    resource: res_id.clone(),
                    sync: VideoSyncMode::Stream,
                    deployment: VideoDeploymentMode::Embedded,
                    external_path: None,
                    stream_url: None,
                    start_offset_ms: 0,
                    end_offset_ms: None,
                    loop_video: false,
                    muted: false,
                    cue_points: Vec::new(),
                });
                let center = [
                    base_center[0] + v_idx as f64 * 16.0,
                    base_center[1] + v_idx as f64 * 16.0,
                ];
                let el_id = new_id("video");
                key.elements.push(Element {
                    id: el_id.clone(),
                    name: video.file_name.clone(),
                    transform: Transform {
                        x: center[0] - video.width * 0.5,
                        y: center[1] - video.height * 0.5,
                        ..Transform::default()
                    },
                    kind: ElementKind::Video {
                        width: video.width,
                        height: video.height,
                    },
                    ..Element::default()
                });
                layer.normalize_motion_keys();
                selection_targets.push(SelectionTarget {
                    layer: layer_id,
                    element: el_id,
                });
                final_layer = Some(layer_idx);
                resolved_frame = Some(place_frame);
                stops_playback = true;
            }
        }

        // 3. Place graphic(s) / image sequence.
        if !stage_shared_empty_layer {
            match &prepared.graphics {
                PreparedGraphicBatch::None => {}
                PreparedGraphicBatch::Symbols(items) => {
                    let staged = self.stage_prepared_import_graphics_at(
                        &mut candidate,
                        items.clone(),
                        mode.stage_point(),
                        true,
                        base_frame,
                        if single_track {
                            candidate_layer_idx
                        } else {
                            None
                        },
                        separate_graphic_layers && !single_track,
                        rename_blank_timeline_layer,
                    )?;
                    if let Some(layer) = staged.layer {
                        final_layer = Some(layer);
                    }
                    resolved_frame = Some(staged.placed_frame);
                    if let Some(sel) = staged.selection {
                        selection_targets.extend(sel.targets);
                    }
                    last_symbol = staged.library_selected;
                    graphic_message = Some(staged.message);
                    stops_playback |= staged.stops_playback;
                }
                PreparedGraphicBatch::Keyframes(item) => {
                    let staged = self.stage_sequence_keyframes_at(
                        &mut candidate,
                        item.as_ref().clone(),
                        mode.stage_point(),
                        base_frame,
                        if single_track {
                            candidate_layer_idx
                        } else {
                            None
                        },
                        rename_blank_timeline_layer,
                    )?;
                    if let Some(layer) = staged.layer {
                        final_layer = Some(layer);
                    }
                    resolved_frame = Some(staged.placed_frame);
                    if let Some(sel) = staged.selection {
                        selection_targets.extend(sel.targets);
                    }
                    last_symbol = staged.library_selected;
                    graphic_message = Some(staged.message);
                    stops_playback |= staged.stops_playback;
                }
            }
        }

        let selection = (!selection_targets.is_empty()).then_some(Selection {
            symbol: symbol_id,
            targets: selection_targets,
        });
        let target_frame = if is_timeline_drop {
            Some(resolved_frame.unwrap_or(0))
        } else {
            resolved_frame.filter(|&f| f != self.frame)
        };

        candidate.pin_displaced_frame_script_layers(&before_timeline_plan);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|error| error.to_string())?;
        let audio_source_paths = prepared
            .audio
            .iter()
            .map(|a| (a.unit.id.clone(), a.path.clone()))
            .collect();
        Ok(FileImportPlan {
            project: candidate,
            swc_reports,
            audio_source_paths,
            resource_selected,
            audio_attached_count,
            video_imported_paths,
            target_layer: final_layer,
            target_frame,
            selection,
            library_selected: last_symbol,
            graphic_message,
            stops_playback,
        })
    }

    fn file_import_history_label(
        &self,
        prepared: &PreparedFileImport,
        place_on_stage: bool,
    ) -> &'static str {
        if prepared.category_count() > 1 {
            "Import files"
        } else if !prepared.swcs.is_empty() {
            "Link SWC"
        } else if !prepared.fonts.is_empty() {
            "Import font"
        } else if !prepared.audio.is_empty() {
            "Import audio"
        } else if !prepared.videos.is_empty() {
            "Import video"
        } else if prepared.palette_file_count != 0 {
            "Import colour palette"
        } else if prepared.graphics.is_keyframes() {
            "Import Image Sequence"
        } else if place_on_stage {
            "Edit"
        } else {
            "Import to Library"
        }
    }

    fn emit_swc_import_reports(&mut self, reports: &[StagedSwcReport]) {
        if reports.is_empty() {
            return;
        }
        let mut total_symbols = 0usize;
        let mut total_sources = 0usize;
        let mut total_resources = 0usize;
        for staged in reports {
            let StagedSwcReport {
                path,
                library_name,
                report,
            } = staged;
            total_symbols += report.linked_symbols;
            total_sources += report.linked_sources;
            total_resources += report.linked_resources;
            let file_loc = crate::output_pipeline::OutputLocation::for_file(path.clone());
            self.push_output_coalesced(
                crate::output_pipeline::OutputEvent::new(
                    crate::output_pipeline::OutputSeverity::Info,
                    crate::output_pipeline::OutputSource::Import,
                    format!(
                        "Linked SWC '{library_name}': {} symbol(s), {} source(s), {} resource(s), {} definition(s).",
                        report.linked_symbols,
                        report.linked_sources,
                        report.linked_resources,
                        report.linked_definitions.len()
                    ),
                )
                .with_location(file_loc.clone()),
            );
            if !report.unresolved_dependencies.is_empty() {
                self.push_output_coalesced(
                    crate::output_pipeline::OutputEvent::new(
                        crate::output_pipeline::OutputSeverity::Warning,
                        crate::output_pipeline::OutputSource::Import,
                        format!(
                            "SWC '{library_name}' has {} unresolved external dependency(ies): {}",
                            report.unresolved_dependencies.len(),
                            report.unresolved_dependencies.join(", ")
                        ),
                    )
                    .with_location(file_loc.clone()),
                );
            }
            for conflict in &report.conflicts {
                self.push_output_coalesced(
                    crate::output_pipeline::OutputEvent::new(
                        crate::output_pipeline::OutputSeverity::Warning,
                        crate::output_pipeline::OutputSource::Import,
                        format!(
                            "SWC conflict on '{}': '{}' vs '{}' (selected '{}').",
                            conflict.definition,
                            conflict.existing_library,
                            conflict.incoming_library,
                            conflict.winner
                        ),
                    )
                    .with_location(file_loc.clone()),
                );
            }
        }
        self.show_panel(crate::workspace::PanelId::Library);
        self.message(format!(
            "Linked {} SWC library(ies) ({total_symbols} symbol(s), {total_sources} source(s), {total_resources} resource(s)).",
            reports.len()
        ));
    }

    pub(crate) fn place_graphic_resource(
        &mut self,
        resource_id: &str,
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        self.place_library_resource(resource_id, MediaPlacementMode::Stage { point })
    }

    pub(crate) fn place_library_resource(
        &mut self,
        resource_id: &str,
        mode: MediaPlacementMode,
    ) -> Result<(), String> {
        let unit = self
            .project
            .resources
            .iter()
            .find(|unit| unit.id == resource_id)
            .ok_or("This library resource no longer exists.")?
            .clone();
        let stem = std::path::Path::new(&unit.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Media")
            .to_owned();

        if unit.is_audio() {
            self.finish_edits();
            let mut candidate = self.project.clone();
            let before_pin = candidate.clone();
            let symbol_id = self.symbol_id().to_owned();
            let is_button = candidate
                .symbol(&symbol_id)
                .is_some_and(|s| s.kind == SymbolKind::Button);
            let target_frame = match mode {
                MediaPlacementMode::Timeline(tl) => {
                    if is_button {
                        tl.frame.min(3)
                    } else {
                        tl.frame.min(999_999)
                    }
                }
                _ => {
                    if is_button {
                        self.frame.min(3)
                    } else {
                        self.frame.min(999_999)
                    }
                }
            };
            let candidate_layer = match mode {
                MediaPlacementMode::Timeline(tl) => tl.hovered_layer,
                _ => Some(self.layer),
            };
            let fps = f64::from(candidate.fps.max(1));
            let duration_sec = import_audio_duration(&unit);
            let symbol = candidate
                .symbol_mut(&symbol_id)
                .ok_or("The current symbol was removed.")?;
            let existing_layer =
                compatible_audio_layer(&symbol.timeline, candidate_layer, target_frame);
            let (layer_idx, placed_frame) = stage_audio_attachment(
                symbol,
                existing_layer,
                target_frame,
                fps,
                duration_sec,
                unit.id.clone(),
                &stem,
                false,
            );
            candidate.pin_displaced_frame_script_layers(&before_pin);
            crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
            candidate.validate().map_err(|e| e.to_string())?;
            let set_frame =
                if matches!(mode, MediaPlacementMode::Timeline(_)) || existing_layer.is_none() {
                    Some(placed_frame)
                } else {
                    None
                };
            self.transact_named("Attach sound", |editor| {
                editor.project = candidate;
                editor.layer = layer_idx;
                if let Some(f) = set_frame {
                    editor.frame = f.min(editor.symbol().timeline.frame_count.saturating_sub(1));
                }
                editor.clear_timeline_frame_selection();
            });
            self.show_frame_sound();
            self.message(format!(
                "Attached sound '{}' at frame {}.",
                unit.path,
                placed_frame + 1
            ));
            return Ok(());
        }

        if unit.is_video() {
            self.finish_edits();
            let (duration_sec, vid_w, vid_h, video_span) = self.inspect_existing_video_unit(&unit);
            let mut candidate = self.project.clone();
            let before_pin = candidate.clone();
            let symbol_id = self.symbol_id().to_owned();
            let is_button = candidate
                .symbol(&symbol_id)
                .is_some_and(|s| s.kind == SymbolKind::Button);
            let target_frame = match mode {
                MediaPlacementMode::Timeline(tl) => {
                    if is_button {
                        tl.frame.min(3)
                    } else {
                        tl.frame.min(999_999)
                    }
                }
                _ => {
                    if is_button {
                        self.frame.min(3)
                    } else {
                        self.frame.min(999_999)
                    }
                }
            };
            let candidate_layer = match mode {
                MediaPlacementMode::Timeline(tl) => tl.hovered_layer,
                _ => Some(self.layer),
            };
            let fps = f64::from(candidate.fps.max(1));
            let video_frames = ((duration_sec * fps).ceil() as u32).max(1);
            let center = mode.stage_point().unwrap_or_else(|| self.paste_center());
            let timeline = &mut candidate
                .symbol_mut(&symbol_id)
                .ok_or("The current symbol was removed.")?
                .timeline;
            let existing_layer = compatible_video_layer(timeline, candidate_layer);
            let placed_frame = if existing_layer.is_some() {
                target_frame
            } else {
                0
            };
            let layer_idx = existing_layer.unwrap_or_else(|| {
                timeline.layers.push(Layer {
                    id: new_id("layer"),
                    name: stem.clone(),
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes: Vec::new(),
                });
                timeline.layers.len() - 1
            });
            let needed_end = if is_button {
                timeline.frame_count.max(placed_frame + 1).min(4)
            } else {
                placed_frame.saturating_add(video_frames)
            };
            timeline.frame_count = timeline.frame_count.max(needed_end);
            let max_tl = timeline.frame_count;
            let layer = &mut timeline.layers[layer_idx];
            if let Some(end) = &mut layer.properties.end_frame
                && *end < needed_end.min(max_tl)
            {
                *end = needed_end.min(max_tl);
            }
            if !layer.keyframes.iter().any(|k| k.frame == placed_frame) {
                let elems = sample_layer_authored_pose_at_fps(layer, placed_frame, fps);
                layer.keyframes.push(Keyframe {
                    frame: placed_frame,
                    elements: elems,
                    ..Keyframe::default()
                });
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            let layer_id = layer.id.clone();
            let key = layer
                .keyframes
                .iter_mut()
                .find(|k| k.frame == placed_frame)
                .ok_or("Could not create a video keyframe.")?;
            key.keyed_properties = None;
            key.motion_keys = None;
            key.roving = false;
            key.video = Some(video_span);
            let el_id = new_id("video");
            let file_name = std::path::Path::new(&unit.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&stem)
                .to_owned();
            key.elements.push(Element {
                id: el_id.clone(),
                name: file_name,
                transform: Transform {
                    x: center[0] - vid_w * 0.5,
                    y: center[1] - vid_h * 0.5,
                    ..Transform::default()
                },
                kind: ElementKind::Video {
                    width: vid_w,
                    height: vid_h,
                },
                ..Element::default()
            });
            layer.normalize_motion_keys();
            candidate.pin_displaced_frame_script_layers(&before_pin);
            crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
            candidate.validate().map_err(|e| e.to_string())?;
            self.set_playing(false);
            let selection = Selection {
                symbol: symbol_id,
                targets: vec![SelectionTarget {
                    layer: layer_id,
                    element: el_id,
                }],
            };
            let set_frame =
                if matches!(mode, MediaPlacementMode::Timeline(_)) || existing_layer.is_none() {
                    Some(placed_frame)
                } else {
                    None
                };
            self.transact_named("Place video", |editor| {
                editor.project = candidate;
                editor.layer = layer_idx;
                if let Some(f) = set_frame {
                    editor.frame = f.min(editor.symbol().timeline.frame_count.saturating_sub(1));
                }
                editor.clear_timeline_frame_selection();
                editor.selection = selection;
            });
            self.message(format!(
                "Placed video '{}' at frame {}.",
                unit.path,
                placed_frame + 1
            ));
            return Ok(());
        }

        let graphic = prepare_graphic(unit, stem)?;
        let items = vec![PreparedImportGraphic::from(graphic)];
        self.place_prepared_import_graphics_with_mode(items, Vec::new(), mode)
    }

    fn place_prepared_import_graphics_with_mode(
        &mut self,
        items: Vec<PreparedImportGraphic>,
        swatches: Vec<GlobalSwatch>,
        mode: MediaPlacementMode,
    ) -> Result<(), String> {
        let place_on_stage = mode.places_on_timeline_or_stage();
        self.finish_edits();
        let mut candidate = self.project.clone();
        let before_pin = candidate.clone();
        candidate.format_version = candidate.format_version.max(3);
        if !swatches.is_empty() {
            Self::merge_swatches_into_project(&mut candidate, &swatches);
        }
        let is_button = self.symbol().kind == SymbolKind::Button;
        let (target_frame, candidate_layer) = match mode {
            MediaPlacementMode::Timeline(tl) => (
                if is_button {
                    tl.frame.min(3)
                } else {
                    tl.frame.min(999_999)
                },
                tl.hovered_layer,
            ),
            _ => (
                if is_button {
                    self.frame.min(3)
                } else {
                    self.frame.min(999_999)
                },
                Some(self.layer),
            ),
        };
        let staged = self.stage_prepared_import_graphics_at(
            &mut candidate,
            items,
            mode.stage_point(),
            place_on_stage,
            target_frame,
            candidate_layer,
            false,
            false,
        )?;
        candidate.pin_displaced_frame_script_layers(&before_pin);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|error| error.to_string())?;
        if staged.stops_playback {
            self.set_playing(false);
        }
        let StagedGraphicPlacement {
            layer,
            placed_frame,
            selection,
            library_selected,
            message,
            ..
        } = staged;
        let label = if place_on_stage {
            "Edit"
        } else {
            "Import to Library"
        };
        let set_frame = (matches!(mode, MediaPlacementMode::Timeline(_))
            || (place_on_stage && placed_frame != self.frame))
            .then_some(placed_frame);
        let clears_timeline_range = layer.is_some() || set_frame.is_some();
        self.transact_named(label, |editor| {
            editor.project = candidate;
            if let Some(layer) = layer {
                editor.layer = layer;
            }
            if let Some(f) = set_frame {
                editor.frame = f.min(editor.symbol().timeline.frame_count.saturating_sub(1));
            }
            if clears_timeline_range {
                editor.clear_timeline_frame_selection();
            }
            if let Some(selection) = selection {
                editor.selection = selection;
            }
            if let Some(symbol) = library_selected {
                editor.library_selected = Some(symbol);
            }
        });
        self.message(message);
        Ok(())
    }

    fn stage_prepared_import_graphics(
        &self,
        candidate: &mut Project,
        items: Vec<PreparedImportGraphic>,
        point: Option<[f64; 2]>,
        place_on_stage: bool,
    ) -> Result<StagedGraphicPlacement, String> {
        let is_button = self.symbol().kind == SymbolKind::Button;
        let target_frame = if is_button {
            self.frame.min(3)
        } else {
            self.frame.min(999_999)
        };
        self.stage_prepared_import_graphics_at(
            candidate,
            items,
            point,
            place_on_stage,
            target_frame,
            Some(self.layer),
            false,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_prepared_import_graphics_at(
        &self,
        candidate: &mut Project,
        items: Vec<PreparedImportGraphic>,
        point: Option<[f64; 2]>,
        place_on_stage: bool,
        target_frame: u32,
        candidate_layer_idx: Option<usize>,
        one_layer_per_item: bool,
        rename_default_layer: bool,
    ) -> Result<StagedGraphicPlacement, String> {
        candidate.format_version = candidate.format_version.max(3);
        let folder = self.library_panel.selected_folder.clone();
        candidate.add_library_folder(&folder)?;
        let mut placed_tracks: Vec<(String, u32, Element)> = Vec::with_capacity(items.len());
        let mut notices = Vec::new();
        let mut last_symbol = None;
        let mut max_imported_frames = 1u32;
        let exact_position = point.is_some() || self.editing_in_place();
        let at = point.unwrap_or_else(|| self.paste_center());
        for (index, mut item) in items.into_iter().enumerate() {
            merge_import_graphic_resources(candidate, &mut item)?;
            for mut helper in item.helper_symbols.drain(..) {
                helper.folder = folder.clone();
                if let Some(link) = helper.linkage_id.as_deref()
                    && candidate
                        .symbols
                        .iter()
                        .any(|s| s.linkage_id.as_deref() == Some(link))
                {
                    notices.push(format!(
                        "Cleared duplicate linkage identifier '{link}' on imported symbol '{}'.",
                        helper.name
                    ));
                    helper.linkage_id = None;
                }
                if let Some(cls) = helper.class_name.as_deref()
                    && candidate
                        .symbols
                        .iter()
                        .any(|s| s.class_name.as_deref() == Some(cls))
                {
                    notices.push(format!(
                        "Cleared duplicate class binding '{cls}' on imported symbol '{}'.",
                        helper.name
                    ));
                    helper.class_name = None;
                }
                let base_name = helper.name.clone();
                let mut suffix = 2;
                while candidate
                    .symbols
                    .iter()
                    .any(|s| s.name == helper.name && s.folder == folder)
                {
                    helper.name = format!("{base_name} {suffix}");
                    suffix += 1;
                }
                candidate.symbols.push(helper);
            }
            let graphic = &mut item.graphic;
            let symbol_id = new_id("symbol");
            let mut name = graphic.name.clone();
            let mut suffix = 2;
            while candidate
                .symbols
                .iter()
                .any(|s| s.name == name && s.folder == folder)
            {
                name = format!("{} {suffix}", graphic.name);
                suffix += 1;
            }
            let width = graphic.width.max(1.0);
            let height = graphic.height.max(1.0);
            let scale = (candidate.width as f64 * 0.8 / width)
                .min(candidate.height as f64 * 0.8 / height)
                .min(1.);
            let half = [width * scale / 2., height * scale / 2.];
            let page_min = if self.edit_context_is_isolated() {
                [
                    -(candidate.width as f64) / 2.,
                    -(candidate.height as f64) / 2.,
                ]
            } else {
                [0., 0.]
            };
            let requested = [at[0] + index as f64 * 16., at[1] + index as f64 * 16.];
            let center = if exact_position {
                requested
            } else {
                [
                    requested[0].clamp(
                        page_min[0] + half[0],
                        page_min[0] + candidate.width as f64 - half[0],
                    ),
                    requested[1].clamp(
                        page_min[1] + half[1],
                        page_min[1] + candidate.height as f64 - half[1],
                    ),
                ]
            };
            let timeline = item.timeline_override.unwrap_or_else(|| Timeline {
                guides: vec![],
                frame_count: 1,
                layers: vec![Layer {
                    properties: Default::default(),
                    id: new_id("layer"),
                    name: "Artwork".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: std::mem::take(&mut graphic.elements),
                        vector_effects: std::mem::take(&mut graphic.vector_effects),
                        tween: None,
                        label: String::new(),
                        ..Default::default()
                    }],
                }],
                scripts: vec![],
            });
            let sym_frames = timeline.frame_count.max(1);
            max_imported_frames = max_imported_frames.max(sym_frames);
            if place_on_stage {
                placed_tracks.push((
                    name.clone(),
                    sym_frames,
                    Element {
                        id: new_id("object"),
                        name: name.clone(),
                        transform: Transform {
                            x: center[0] - half[0],
                            y: center[1] - half[1],
                            scale_x: scale,
                            scale_y: scale,
                            ..Default::default()
                        },
                        kind: ElementKind::Instance {
                            symbol_id: symbol_id.clone(),
                            playback: item.playback,
                        },
                        scripts: vec![],
                        accessibility: None,
                        ..Element::default()
                    },
                ));
            }
            candidate.symbols.push(Symbol {
                id: symbol_id.clone(),
                name,
                folder: folder.clone(),
                kind: item.symbol_kind,
                class_name: None,
                linkage_id: None,
                timeline,
            });
            if let Some(notice) = graphic.notice.take() {
                notices.push(notice);
            }
            last_symbol = Some(symbol_id);
        }
        if !place_on_stage {
            let suffix = if notices.is_empty() {
                String::new()
            } else {
                format!(" {}", notices.join(" "))
            };
            return Ok(StagedGraphicPlacement {
                layer: None,
                placed_frame: 0,
                selection: None,
                library_selected: last_symbol,
                message: format!("Imported symbol(s) into Library.{suffix}"),
                stops_playback: false,
            });
        }
        if one_layer_per_item && !placed_tracks.is_empty() {
            let symbol_id = self.symbol_id().to_owned();
            let is_button = candidate
                .symbol(&symbol_id)
                .is_some_and(|s| s.kind == SymbolKind::Button);
            let timeline = &mut candidate
                .symbol_mut(&symbol_id)
                .ok_or("The current symbol was removed.")?
                .timeline;
            let count = placed_tracks.len();
            let mut targets = Vec::with_capacity(count);
            let mut top_layer = 0usize;
            for (layer_name, span, element) in placed_tracks {
                let needed_end = if is_button {
                    timeline.frame_count.clamp(1, 4)
                } else {
                    span.max(1)
                };
                timeline.frame_count = timeline.frame_count.max(needed_end);
                let layer_id = new_id("layer");
                targets.push(SelectionTarget {
                    layer: layer_id.clone(),
                    element: element.id.clone(),
                });
                let keyframes = vec![Keyframe {
                    frame: 0,
                    elements: vec![element],
                    ..Keyframe::default()
                }];
                timeline.layers.push(Layer {
                    id: layer_id,
                    name: layer_name,
                    visible: true,
                    locked: false,
                    properties: Default::default(),
                    keyframes,
                });
                top_layer = timeline.layers.len() - 1;
            }
            let suffix = if notices.is_empty() {
                String::new()
            } else {
                format!(" {}", notices.join(" "))
            };
            return Ok(StagedGraphicPlacement {
                layer: Some(top_layer),
                placed_frame: 0,
                selection: Some(Selection {
                    symbol: symbol_id,
                    targets,
                }),
                library_selected: last_symbol,
                message: format!("Placed {count} graphic(s) on Timeline.{suffix}"),
                stops_playback: true,
            });
        }
        let fallback_layer_name = if placed_tracks.len() == 1 {
            placed_tracks[0].0.clone()
        } else {
            "Imported graphics".into()
        };
        let placed: Vec<Element> = placed_tracks.into_iter().map(|(_, _, el)| el).collect();
        self.stage_graphic_placements_with_span_at(
            candidate,
            placed,
            last_symbol,
            notices,
            max_imported_frames,
            target_frame,
            candidate_layer_idx,
            &fallback_layer_name,
            rename_default_layer,
        )
    }

    pub(crate) fn place_library_symbol(
        &mut self,
        ctx: &egui::Context,
        symbol: &str,
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        self.place_library_symbol_with_mode(ctx, symbol, MediaPlacementMode::Stage { point })
    }

    pub(crate) fn place_library_symbol_with_mode(
        &mut self,
        ctx: &egui::Context,
        symbol: &str,
        mode: MediaPlacementMode,
    ) -> Result<(), String> {
        if reaches(
            &self.project,
            symbol,
            self.symbol_id(),
            &mut std::collections::HashSet::new(),
        ) {
            return Err("A symbol cannot contain itself or an ancestor.".into());
        }
        self.finish_edits();
        let source = self
            .project
            .symbol(symbol)
            .ok_or("This library symbol no longer exists.")?;
        let sym_name = source.name.clone();
        let sym_frames = source.timeline.frame_count.max(1);
        let painter = ctx.layer_painter(egui::LayerId::background());
        let bounds = evaluate_scene(&self.project, symbol, 0, false, true)
            .draws
            .iter()
            .fold(Rect::NOTHING, |rect, draw| {
                rect.union(crate::stage::world_bounds(&draw.item, &painter))
            });
        let point = mode.stage_point();
        let exact_position = point.is_some() || self.editing_in_place();
        let mut at = point.unwrap_or_else(|| self.paste_center());
        let center = if bounds.is_finite() {
            bounds.center()
        } else {
            Pos2::ZERO
        };
        let page_size = [self.project.width as f64, self.project.height as f64];
        let page_min = if self.edit_context_is_isolated() {
            [-page_size[0] / 2., -page_size[1] / 2.]
        } else {
            [0., 0.]
        };
        let half = if bounds.is_finite() {
            [bounds.width() as f64 / 2., bounds.height() as f64 / 2.]
        } else {
            [0., 0.]
        };
        if !exact_position {
            for axis in 0..2 {
                let inset = half[axis].min(page_size[axis] / 2.);
                at[axis] = at[axis].clamp(
                    page_min[axis] + inset,
                    page_min[axis] + page_size[axis] - inset,
                );
            }
        }
        let element = Element {
            id: new_id("object"),
            name: String::new(),
            transform: Transform {
                x: at[0] - center.x as f64,
                y: at[1] - center.y as f64,
                ..Default::default()
            },
            kind: ElementKind::Instance {
                symbol_id: source.id.clone(),
                playback: match source.kind {
                    SymbolKind::MovieClip => InstancePlayback::Independent,
                    SymbolKind::Graphic => InstancePlayback::Loop,
                    SymbolKind::Button => InstancePlayback::SingleFrame(0),
                },
            },
            scripts: vec![],
            accessibility: None,
            ..Element::default()
        };
        let is_button = self.symbol().kind == SymbolKind::Button;
        let (target_frame, candidate_layer, min_span) = match mode {
            MediaPlacementMode::Timeline(tl) => (
                if is_button {
                    tl.frame.min(3)
                } else {
                    tl.frame.min(999_999)
                },
                tl.hovered_layer,
                sym_frames,
            ),
            _ => (
                if is_button {
                    self.frame.min(3)
                } else {
                    self.frame.min(999_999)
                },
                Some(self.layer),
                1,
            ),
        };
        let mut candidate = self.project.clone();
        let before_pin = candidate.clone();
        let staged = self.stage_graphic_placements_with_span_at(
            &mut candidate,
            vec![element],
            Some(symbol.into()),
            vec![],
            min_span,
            target_frame,
            candidate_layer,
            &sym_name,
            false,
        )?;
        candidate.pin_displaced_frame_script_layers(&before_pin);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|error| error.to_string())?;
        if staged.stops_playback {
            self.set_playing(false);
        }
        let StagedGraphicPlacement {
            layer,
            placed_frame,
            selection,
            library_selected,
            message,
            ..
        } = staged;
        let set_frame =
            if matches!(mode, MediaPlacementMode::Timeline(_)) || placed_frame != self.frame {
                Some(placed_frame)
            } else {
                None
            };
        let clears_timeline_range = layer.is_some() || set_frame.is_some();
        self.transact(|editor| {
            editor.project = candidate;
            if let Some(layer) = layer {
                editor.layer = layer;
            }
            if let Some(f) = set_frame {
                editor.frame = f.min(editor.symbol().timeline.frame_count.saturating_sub(1));
            }
            if clears_timeline_range {
                editor.clear_timeline_frame_selection();
            }
            if let Some(selection) = selection {
                editor.selection = selection;
            }
            editor.library_selected = library_selected;
        });
        self.message(message);
        Ok(())
    }

    #[allow(dead_code)]
    fn commit_graphic_placements(
        &mut self,
        candidate: Project,
        placed: Vec<Element>,
        last_symbol: Option<String>,
        notices: Vec<String>,
    ) -> Result<(), String> {
        self.commit_graphic_placements_with_span(candidate, placed, last_symbol, notices, 1)
    }

    fn commit_graphic_placements_with_span(
        &mut self,
        mut candidate: Project,
        placed: Vec<Element>,
        last_symbol: Option<String>,
        notices: Vec<String>,
        min_frame_span: u32,
    ) -> Result<(), String> {
        let before_pin = candidate.clone();
        let staged = self.stage_graphic_placements_with_span(
            &mut candidate,
            placed,
            last_symbol,
            notices,
            min_frame_span,
        )?;
        candidate.pin_displaced_frame_script_layers(&before_pin);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|error| error.to_string())?;
        if staged.stops_playback {
            self.set_playing(false);
        }
        let StagedGraphicPlacement {
            layer,
            placed_frame,
            selection,
            library_selected,
            message,
            ..
        } = staged;
        self.transact(|editor| {
            editor.project = candidate;
            if let Some(layer) = layer {
                editor.layer = layer;
                editor.clear_timeline_frame_selection();
            }
            if placed_frame != editor.frame {
                editor.frame =
                    placed_frame.min(editor.symbol().timeline.frame_count.saturating_sub(1));
                editor.clear_timeline_frame_selection();
            }
            if let Some(selection) = selection {
                editor.selection = selection;
            }
            editor.library_selected = library_selected;
        });
        self.message(message);
        Ok(())
    }

    fn stage_graphic_placements_with_span(
        &self,
        candidate: &mut Project,
        placed: Vec<Element>,
        last_symbol: Option<String>,
        notices: Vec<String>,
        min_frame_span: u32,
    ) -> Result<StagedGraphicPlacement, String> {
        let is_button = self.symbol().kind == SymbolKind::Button;
        let target_frame = if is_button {
            self.frame.min(3)
        } else {
            self.frame.min(999_999)
        };
        self.stage_graphic_placements_with_span_at(
            candidate,
            placed,
            last_symbol,
            notices,
            min_frame_span,
            target_frame,
            Some(self.layer),
            "Imported graphics",
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_graphic_placements_with_span_at(
        &self,
        candidate: &mut Project,
        placed: Vec<Element>,
        last_symbol: Option<String>,
        notices: Vec<String>,
        min_frame_span: u32,
        target_frame: u32,
        candidate_layer_idx: Option<usize>,
        fallback_layer_name: &str,
        rename_default_layer: bool,
    ) -> Result<StagedGraphicPlacement, String> {
        let count = placed.len();
        let symbol_id = self.symbol_id().to_owned();
        let fps = f64::from(candidate.fps.max(1));
        let symbol = candidate
            .symbol_mut(&symbol_id)
            .ok_or("The current symbol was removed.")?;
        let is_button = symbol.kind == SymbolKind::Button;
        let timeline = &mut symbol.timeline;
        // Import must remain visible even when a folder, mask, guide, hidden, locked,
        // dedicated script/Actions, or sound/video layer is selected. Add a normal top layer
        // starting at Frame 1 (`frame = 0`) without altering the existing layer.
        let existing_layer = compatible_graphic_layer(timeline, candidate_layer_idx);
        let use_current = existing_layer.is_some();
        let placed_frame = if use_current { target_frame } else { 0 };
        let target_layer = existing_layer.unwrap_or_else(|| {
            timeline.layers.push(Layer {
                id: new_id("layer"),
                name: fallback_layer_name.into(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: Vec::new(),
            });
            timeline.layers.len() - 1
        });
        let needed_end = if is_button {
            timeline.frame_count.max(placed_frame + 1).min(4)
        } else {
            placed_frame.saturating_add(min_frame_span.max(1))
        };
        timeline.frame_count = timeline.frame_count.max(needed_end);
        let max_tl = timeline.frame_count;
        let layer = &mut timeline.layers[target_layer];
        if rename_default_layer && existing_layer == Some(0) && layer.name == "Layer 1" {
            layer.name = fallback_layer_name.into();
        }
        if let Some(end) = &mut layer.properties.end_frame
            && *end < needed_end.min(max_tl)
        {
            *end = needed_end.min(max_tl);
        }
        let layer_id = layer.id.clone();
        if !layer.keyframes.iter().any(|key| key.frame == placed_frame) {
            let elems = sample_layer_authored_pose_at_fps(layer, placed_frame, fps);
            layer.keyframes.push(Keyframe {
                frame: placed_frame,
                elements: elems,
                ..Keyframe::default()
            });
            layer.keyframes.sort_by_key(|key| key.frame);
        }
        let targets = placed
            .iter()
            .map(|element| SelectionTarget {
                layer: layer_id.clone(),
                element: element.id.clone(),
            })
            .collect();
        let key = layer
            .keyframes
            .iter_mut()
            .find(|k| k.frame == placed_frame)
            .ok_or("Could not create an import keyframe.")?;
        key.keyed_properties = None;
        key.motion_keys = None;
        key.roving = false;
        key.elements.extend(placed);
        layer.normalize_motion_keys();
        let selection = Selection {
            symbol: symbol_id,
            targets,
        };
        let suffix = if notices.is_empty() {
            String::new()
        } else {
            format!(" {}", notices.join(" "))
        };
        let target_note = if use_current {
            ""
        } else {
            " Created a visible layer."
        };
        Ok(StagedGraphicPlacement {
            layer: Some(target_layer),
            placed_frame,
            selection: Some(selection),
            library_selected: last_symbol,
            message: format!("Placed {count} graphic(s) on Stage.{target_note}{suffix}"),
            stops_playback: true,
        })
    }

    fn stage_sequence_keyframes_at(
        &self,
        candidate: &mut Project,
        mut seq_item: PreparedImportGraphic,
        point: Option<[f64; 2]>,
        base_frame: u32,
        candidate_layer_idx: Option<usize>,
        rename_default_layer: bool,
    ) -> Result<StagedGraphicPlacement, String> {
        candidate.format_version = candidate.format_version.max(3);
        merge_import_graphic_resources(candidate, &mut seq_item)?;

        let seq_timeline = seq_item
            .timeline_override
            .take()
            .ok_or_else(|| "Image sequence import produced no timeline.".to_owned())?;
        let seq_layer = seq_timeline
            .layers
            .into_iter()
            .next()
            .ok_or_else(|| "Image sequence import produced no frames.".to_owned())?;
        let width = seq_item.graphic.width.max(1.0);
        let height = seq_item.graphic.height.max(1.0);
        let scale = (candidate.width as f64 * 0.8 / width)
            .min(candidate.height as f64 * 0.8 / height)
            .min(1.0);
        let half = [width * scale / 2.0, height * scale / 2.0];
        let page_min = if self.edit_context_is_isolated() {
            [
                -(candidate.width as f64) / 2.0,
                -(candidate.height as f64) / 2.0,
            ]
        } else {
            [0.0, 0.0]
        };
        let exact_position = point.is_some() || self.editing_in_place();
        let requested = point.unwrap_or_else(|| self.paste_center());
        let center = if exact_position {
            requested
        } else {
            [
                requested[0].clamp(
                    page_min[0] + half[0],
                    page_min[0] + candidate.width as f64 - half[0],
                ),
                requested[1].clamp(
                    page_min[1] + half[1],
                    page_min[1] + candidate.height as f64 - half[1],
                ),
            ]
        };
        let offset_x = center[0] - half[0];
        let offset_y = center[1] - half[1];

        let symbol_id = self.symbol_id().to_owned();
        let symbol = candidate
            .symbol_mut(&symbol_id)
            .ok_or("The current symbol was removed.")?;
        let is_button = symbol.kind == SymbolKind::Button;
        let timeline = &mut symbol.timeline;
        let existing_layer = compatible_graphic_layer(timeline, candidate_layer_idx);
        let placed_frame = if existing_layer.is_some() {
            base_frame
        } else {
            0
        };
        let target_layer = existing_layer.unwrap_or_else(|| {
            timeline.layers.push(Layer {
                id: new_id("layer"),
                name: seq_item.graphic.name.clone(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: Vec::new(),
            });
            timeline.layers.len() - 1
        });
        let raw_needed_end = placed_frame.saturating_add(seq_timeline.frame_count.max(1));
        let needed_end = if is_button {
            timeline.frame_count.max(raw_needed_end.min(4)).min(4)
        } else {
            raw_needed_end
        };
        timeline.frame_count = timeline.frame_count.max(needed_end);
        let max_tl = timeline.frame_count;
        let layer = &mut timeline.layers[target_layer];
        if rename_default_layer && existing_layer == Some(0) && layer.name == "Layer 1" {
            layer.name = seq_item.graphic.name.clone();
        }
        if let Some(end) = &mut layer.properties.end_frame
            && *end < needed_end.min(max_tl)
        {
            *end = needed_end.min(max_tl);
        }
        let layer_id = layer.id.clone();
        let mut first_targets = Vec::new();
        let mut keyframe_count = 0usize;
        for (idx, mut kf) in seq_layer.keyframes.into_iter().enumerate() {
            let dest_frame = placed_frame.saturating_add(kf.frame);
            if is_button && dest_frame >= max_tl {
                continue;
            }
            keyframe_count += 1;
            kf.frame = dest_frame;
            kf.keyed_properties = None;
            kf.motion_keys = None;
            kf.roving = false;
            for el in &mut kf.elements {
                el.id = new_id("object");
                el.transform.x = offset_x + el.transform.x * scale;
                el.transform.y = offset_y + el.transform.y * scale;
                el.transform.scale_x *= scale;
                el.transform.scale_y *= scale;
                if idx == 0 {
                    first_targets.push(SelectionTarget {
                        layer: layer_id.clone(),
                        element: el.id.clone(),
                    });
                }
            }
            if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == dest_frame) {
                existing.keyed_properties = None;
                existing.motion_keys = None;
                existing.roving = false;
                existing.elements.extend(kf.elements);
            } else {
                layer.keyframes.push(kf);
            }
        }
        layer.normalize_motion_keys();
        let notice = seq_item.graphic.notice.take();
        let selection = Selection {
            symbol: symbol_id,
            targets: first_targets,
        };
        let suffix = notice.map(|n| format!(" {n}")).unwrap_or_default();
        Ok(StagedGraphicPlacement {
            layer: Some(target_layer),
            placed_frame,
            selection: Some(selection),
            library_selected: None,
            message: format!("Placed {keyframe_count} sequence keyframe(s) on timeline.{suffix}"),
            stops_playback: true,
        })
    }

    pub(crate) fn commit_figma_as_movie_clip_symbol(
        &mut self,
        path: &std::path::Path,
        imported: Project,
        report: &neoflash_import::ImportReport,
        point: Option<[f64; 2]>,
        place_on_stage: bool,
    ) -> Result<(), String> {
        let used = self
            .project
            .resources
            .iter()
            .try_fold(0usize, |total, r| total.checked_add(r.byte_len()))
            .ok_or_else(|| "Project resource byte count overflowed.".to_owned())?;
        let mut remaining = MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used);
        let item = prepare_imported_figma_project(path, imported, report, &mut remaining)?;
        self.finish_edits();
        let mut candidate = self.project.clone();
        let before_pin = candidate.clone();
        candidate.format_version = candidate.format_version.max(3);
        let staged =
            self.stage_prepared_import_graphics(&mut candidate, vec![item], point, place_on_stage)?;
        candidate.pin_displaced_frame_script_layers(&before_pin);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|e| e.to_string())?;
        if staged.stops_playback {
            self.set_playing(false);
        }
        let StagedGraphicPlacement {
            layer,
            placed_frame,
            selection,
            library_selected,
            message,
            ..
        } = staged;
        let label = if place_on_stage {
            "Import Figma"
        } else {
            "Import Figma to Library"
        };
        self.transact_named(label, |editor| {
            editor.project = candidate;
            if let Some(layer) = layer {
                editor.layer = layer;
                editor.clear_timeline_frame_selection();
            }
            if place_on_stage && placed_frame != editor.frame {
                editor.frame =
                    placed_frame.min(editor.symbol().timeline.frame_count.saturating_sub(1));
                editor.clear_timeline_frame_selection();
            }
            if let Some(selection) = selection {
                editor.selection = selection;
            }
            if let Some(symbol) = library_selected {
                editor.library_selected = Some(symbol);
            }
        });
        if !place_on_stage {
            self.show_panel(crate::workspace::PanelId::Library);
        }
        self.message(message);
        Ok(())
    }

    pub(crate) fn commit_figma_pages_as_separate_library_symbols(
        &mut self,
        path: &std::path::Path,
        imported: Project,
        report: &neoflash_import::ImportReport,
    ) -> Result<(), String> {
        let used = self
            .project
            .resources
            .iter()
            .try_fold(0usize, |total, r| total.checked_add(r.byte_len()))
            .ok_or_else(|| "Project resource byte count overflowed.".to_owned())?;
        let mut remaining = MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used);
        let mut base_item = prepare_imported_figma_project(path, imported, report, &mut remaining)?;
        let page_layers = base_item
            .timeline_override
            .take()
            .map(|tl| tl.layers)
            .unwrap_or_default();
        if page_layers.is_empty() {
            return Err("Selected Figma pages contained no layers to import.".into());
        }

        let mut items = Vec::with_capacity(page_layers.len());
        let shared_resources = std::mem::take(&mut base_item.graphic.resources);
        let shared_helpers = std::mem::take(&mut base_item.helper_symbols);
        let shared_notice = base_item.graphic.notice.take();
        let (w, h) = (base_item.graphic.width, base_item.graphic.height);

        for (idx, page_layer) in page_layers.into_iter().enumerate() {
            let sym_name = if page_layer.name.trim().is_empty() {
                format!("{} Page {}", base_item.graphic.name, idx + 1)
            } else {
                page_layer.name.clone()
            };
            items.push(PreparedImportGraphic {
                graphic: PreparedGraphic {
                    name: sym_name,
                    width: w,
                    height: h,
                    elements: Vec::new(),
                    vector_effects: Vec::new(),
                    resources: shared_resources.clone(),
                    notice: if idx == 0 {
                        shared_notice.clone()
                    } else {
                        None
                    },
                },
                timeline_override: Some(Timeline {
                    frame_count: 1,
                    layers: vec![page_layer],
                    scripts: Vec::new(),
                    guides: Vec::new(),
                }),
                symbol_kind: SymbolKind::MovieClip,
                playback: InstancePlayback::Independent,
                helper_symbols: if idx == 0 {
                    shared_helpers.clone()
                } else {
                    Vec::new()
                },
            });
        }

        self.finish_edits();
        let mut candidate = self.project.clone();
        let before_pin = candidate.clone();
        candidate.format_version = candidate.format_version.max(3);
        let staged = self.stage_prepared_import_graphics(&mut candidate, items, None, false)?;
        candidate.pin_displaced_frame_script_layers(&before_pin);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|e| e.to_string())?;
        let StagedGraphicPlacement {
            library_selected,
            message,
            ..
        } = staged;
        self.transact_named("Import Figma to Library", |editor| {
            editor.project = candidate;
            if let Some(symbol) = library_selected {
                editor.library_selected = Some(symbol);
            }
        });
        self.show_panel(crate::workspace::PanelId::Library);
        self.message(message);
        Ok(())
    }

    pub(crate) fn commit_figma_pages_directly_as_layers(
        &mut self,
        path: &std::path::Path,
        imported: Project,
        report: &neoflash_import::ImportReport,
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        let used = self
            .project
            .resources
            .iter()
            .try_fold(0usize, |total, r| total.checked_add(r.byte_len()))
            .ok_or_else(|| "Project resource byte count overflowed.".to_owned())?;
        let mut remaining = MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used);
        let mut item = prepare_imported_figma_project(path, imported, report, &mut remaining)?;

        self.finish_edits();
        let mut candidate = self.project.clone();
        let before_pin = candidate.clone();
        candidate.format_version = candidate.format_version.max(3);
        let folder = self.library_panel.selected_folder.clone();
        candidate.add_library_folder(&folder)?;

        merge_import_graphic_resources(&mut candidate, &mut item)?;
        let mut notices = Vec::new();
        for mut helper in item.helper_symbols.drain(..) {
            helper.folder = folder.clone();
            if let Some(link) = helper.linkage_id.as_deref()
                && candidate
                    .symbols
                    .iter()
                    .any(|s| s.linkage_id.as_deref() == Some(link))
            {
                notices.push(format!(
                    "Cleared duplicate linkage identifier '{link}' on imported symbol '{}'.",
                    helper.name
                ));
                helper.linkage_id = None;
            }
            if let Some(cls) = helper.class_name.as_deref()
                && candidate
                    .symbols
                    .iter()
                    .any(|s| s.class_name.as_deref() == Some(cls))
            {
                notices.push(format!(
                    "Cleared duplicate class binding '{cls}' on imported symbol '{}'.",
                    helper.name
                ));
                helper.class_name = None;
            }
            let base_name = helper.name.clone();
            let mut suffix = 2;
            while candidate
                .symbols
                .iter()
                .any(|s| s.name == helper.name && s.folder == folder)
            {
                helper.name = format!("{base_name} {suffix}");
                suffix += 1;
            }
            candidate.symbols.push(helper);
        }
        if let Some(n) = item.graphic.notice.take() {
            notices.push(n);
        }

        let width = item.graphic.width.max(1.0);
        let height = item.graphic.height.max(1.0);
        let half = [width * 0.5, height * 0.5];
        let page_min = if self.edit_context_is_isolated() {
            [
                -(candidate.width as f64) * 0.5,
                -(candidate.height as f64) * 0.5,
            ]
        } else {
            [0.0, 0.0]
        };
        let exact_position = point.is_some() || self.editing_in_place();
        let requested = point.unwrap_or_else(|| self.paste_center());
        let clamp_axis = |req: f64, pmin: f64, span: f64, h: f64| -> f64 {
            if exact_position {
                return req;
            }
            let lo = pmin + h;
            let hi = pmin + span - h;
            if lo <= hi {
                req.clamp(lo, hi)
            } else {
                pmin + span * 0.5
            }
        };
        let center_x = clamp_axis(requested[0], page_min[0], candidate.width as f64, half[0]);
        let center_y = clamp_axis(requested[1], page_min[1], candidate.height as f64, half[1]);
        let offset_x = center_x - half[0];
        let offset_y = center_y - half[1];

        let raw_layers = item
            .timeline_override
            .take()
            .map(|tl| tl.layers)
            .unwrap_or_default();
        if raw_layers.is_empty() {
            return Err("Selected Figma pages contained no layers to import.".into());
        }

        let symbol_id = self.symbol_id().to_owned();
        let is_button = candidate
            .symbol(&symbol_id)
            .is_some_and(|s| s.kind == SymbolKind::Button);
        let mut selection_targets = Vec::new();
        let mut prepared_layers = Vec::with_capacity(raw_layers.len());
        let mut max_required_frames = 1u32;

        for mut page_layer in raw_layers {
            if is_button {
                page_layer.keyframes.retain(|kf| kf.frame < 4);
                if let Some(end) = &mut page_layer.properties.end_frame {
                    *end = (*end).min(4);
                }
                page_layer.normalize_motion_keys();
            }
            let lid = page_layer.id.clone();
            for kf in &mut page_layer.keyframes {
                max_required_frames = max_required_frames.max(kf.frame.saturating_add(1));
                for el in &mut kf.elements {
                    el.transform.x = (el.transform.x + offset_x).clamp(-100_000.0, 100_000.0);
                    el.transform.y = (el.transform.y + offset_y).clamp(-100_000.0, 100_000.0);
                    if kf.frame == 0 {
                        selection_targets.push(SelectionTarget {
                            layer: lid.clone(),
                            element: el.id.clone(),
                        });
                    }
                }
            }
            if page_layer.properties.role != LayerRole::Folder
                && !page_layer.keyframes.iter().any(|k| k.frame == 0)
            {
                page_layer.keyframes.insert(0, Keyframe::default());
            }
            page_layer.keyframes.sort_by_key(|k| k.frame);
            prepared_layers.push(page_layer);
        }

        let imported_layer_count = prepared_layers.len();
        let timeline = &mut candidate
            .symbol_mut(&symbol_id)
            .ok_or_else(|| "The current symbol was removed.".to_owned())?
            .timeline;
        if is_button {
            timeline.frame_count = timeline.frame_count.max(max_required_frames).min(4);
        } else {
            timeline.frame_count = timeline.frame_count.max(max_required_frames);
        }

        if crate::panels::figma_import::active_timeline_is_single_blank_layer(timeline, 0) {
            timeline.layers = prepared_layers;
        } else {
            timeline.layers.extend(prepared_layers);
        }
        let target_layer_idx = timeline.layers.len().saturating_sub(1);

        candidate.pin_displaced_frame_script_layers(&before_pin);
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
        candidate.validate().map_err(|e| e.to_string())?;

        self.set_playing(false);
        let selection = Selection {
            symbol: symbol_id,
            targets: selection_targets,
        };
        self.transact_named("Import Figma", |editor| {
            editor.project = candidate;
            editor.layer = target_layer_idx;
            editor.frame = 0;
            editor.clear_timeline_frame_selection();
            editor.selection = selection;
        });

        let suffix = if notices.is_empty() {
            String::new()
        } else {
            format!(" {}", notices.join(" "))
        };
        self.message(format!(
            "Imported {imported_layer_count} Figma page layer(s) onto timeline.{suffix}"
        ));
        Ok(())
    }
}

pub(crate) fn merge_graphic_resources(
    project: &mut Project,
    graphic: &mut PreparedGraphic,
) -> Result<(), String> {
    let mut item = PreparedImportGraphic::from(PreparedGraphic {
        name: std::mem::take(&mut graphic.name),
        width: graphic.width,
        height: graphic.height,
        elements: std::mem::take(&mut graphic.elements),
        vector_effects: std::mem::take(&mut graphic.vector_effects),
        resources: std::mem::take(&mut graphic.resources),
        notice: graphic.notice.take(),
    });
    merge_import_graphic_resources(project, &mut item)?;
    *graphic = item.graphic;
    Ok(())
}

fn remap_timeline_bitmap_resources(
    timeline: &mut Timeline,
    ids: &HashMap<String, String>,
) -> Result<(), String> {
    for layer in &mut timeline.layers {
        for key in &mut layer.keyframes {
            if let Some(sound) = &mut key.sound
                && let Some(next_id) = ids.get(&sound.resource)
            {
                sound.resource = next_id.clone();
            }
            if let Some(video) = &mut key.video
                && let Some(next_id) = ids.get(&video.resource)
            {
                video.resource = next_id.clone();
            }
            for fill in &mut key.bitmap_fills {
                fill.resource_id = ids
                    .get(&fill.resource_id)
                    .ok_or("Missing imported image bytes.")?
                    .clone();
            }
            for element in &mut key.elements {
                if let ElementKind::Bitmap { resource_id, .. } = &mut element.kind {
                    *resource_id = ids
                        .get(resource_id)
                        .ok_or("Missing imported image bytes.")?
                        .clone();
                }
            }
        }
    }
    Ok(())
}

fn merge_import_graphic_resources(
    project: &mut Project,
    item: &mut PreparedImportGraphic,
) -> Result<(), String> {
    let graphic = &mut item.graphic;
    let mut ids = HashMap::new();
    for mut unit in std::mem::take(&mut graphic.resources) {
        if let Some(existing) = project
            .resources
            .iter()
            .find(|old| old.media_type == unit.media_type && old.data_base64 == unit.data_base64)
        {
            ids.insert(unit.id, existing.id.clone());
            continue;
        }
        let old_id = unit.id.clone();
        if project.resources.iter().any(|old| old.id == unit.id) {
            unit.id = new_id("res");
        }
        let original = unit.path.clone();
        let (stem, extension) = original
            .rsplit_once('.')
            .map(|(s, e)| (s, format!(".{e}")))
            .unwrap_or((&original, String::new()));
        let mut suffix = 2;
        while project
            .resources
            .iter()
            .any(|old| old.path.eq_ignore_ascii_case(&unit.path))
        {
            unit.path = format!("{stem}-{suffix}{extension}");
            suffix += 1;
        }
        ids.insert(old_id, unit.id.clone());
        project.resources.push(unit);
    }
    for element in &mut graphic.elements {
        if let ElementKind::Bitmap { resource_id, .. } = &mut element.kind {
            *resource_id = ids
                .get(resource_id)
                .ok_or("Missing imported image bytes.")?
                .clone();
        }
    }
    if let Some(timeline) = &mut item.timeline_override {
        remap_timeline_bitmap_resources(timeline, &ids)?;
        timeline.remap_resource_ids(&ids);
    }
    for helper in &mut item.helper_symbols {
        remap_timeline_bitmap_resources(&mut helper.timeline, &ids)?;
        helper.timeline.remap_resource_ids(&ids);
    }
    Ok(())
}

fn prepare_psd_graphic(
    path: &std::path::Path,
    remaining: &mut usize,
) -> Result<PreparedImportGraphic, String> {
    let display = path.display().to_string();
    let (mut imported, report) =
        neoflash_import::import_psd_path(path, &neoflash_import::PsdImportOptions::default())
            .map_err(|e| format!("{display} [psd-import]: {e}"))?;
    // The PSD parser bounds source bytes itself. Only its portable pixels consume
    // the destination resource budget; large Photoshop containers often have small composites.
    let resource_bytes = imported.resources.iter().map(ResourceUnit::byte_len).sum();
    *remaining = remaining.checked_sub(resource_bytes).ok_or_else(|| {
        format!("{display}: imported PSD pixels exceed the remaining resource budget.")
    })?;
    let root_id = imported.root.clone();
    let root_symbol = imported
        .symbols
        .iter()
        .find(|s| s.id == root_id)
        .cloned()
        .ok_or_else(|| format!("{display}: PSD import produced no root symbol"))?;
    let helper_symbols: Vec<Symbol> = imported
        .symbols
        .into_iter()
        .filter(|s| s.id != root_id)
        .collect();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("PSD")
        .to_owned();
    Ok(PreparedImportGraphic {
        graphic: PreparedGraphic {
            name,
            width: (imported.width as f64).max(1.0),
            height: (imported.height as f64).max(1.0),
            elements: Vec::new(),
            vector_effects: Vec::new(),
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::MovieClip,
        playback: InstancePlayback::SingleFrame(0),
        helper_symbols,
    })
}

fn prepare_pdf_ai_graphic(
    path: &std::path::Path,
    remaining: &mut usize,
) -> Result<PreparedImportGraphic, String> {
    let display = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("{display}: {e}"))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Vector")
        .to_owned();
    let (mut imported, report) = neoflash_import::import_pdf_ai_with_options(
        &bytes,
        &name,
        &neoflash_import::VectorImportOptions::default(),
    )
    .map_err(|e| format!("{display} [pdf-import]: {e}"))?;
    if report.statistics.elements == 0 {
        return Err(format!(
            "{display} [pdf-no-vector-content]: PDF/AI file contains no importable vector or text operators."
        ));
    }
    let resource_bytes = imported.resources.iter().map(ResourceUnit::byte_len).sum();
    *remaining = remaining.checked_sub(resource_bytes).ok_or_else(|| {
        format!("{display}: imported vector document resources exceed the remaining import budget.")
    })?;
    let root_id = imported.root.clone();
    let root_symbol = imported
        .symbols
        .iter()
        .find(|s| s.id == root_id)
        .cloned()
        .ok_or_else(|| format!("{display}: PDF/AI import produced no root symbol"))?;
    let helper_symbols: Vec<Symbol> = imported
        .symbols
        .into_iter()
        .filter(|s| s.id != root_id)
        .collect();
    Ok(PreparedImportGraphic {
        graphic: PreparedGraphic {
            name,
            width: (imported.width as f64).max(1.0),
            height: (imported.height as f64).max(1.0),
            elements: Vec::new(),
            vector_effects: Vec::new(),
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::Graphic,
        playback: InstancePlayback::SingleFrame(0),
        helper_symbols,
    })
}

fn prepare_fxg_graphic(path: &std::path::Path) -> Result<PreparedImportGraphic, String> {
    let display = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|e| format!("{display}: {e}"))?;
    let xml = std::str::from_utf8(&bytes)
        .map_err(|_| format!("{display} [fxg-utf8]: FXG file must be valid UTF-8 XML"))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("FXG")
        .to_owned();
    let (mut imported, report) = neoflash_import::import_fxg_str(xml, &name)
        .map_err(|e| format!("{display} [fxg-import]: {e}"))?;
    if report.statistics.elements == 0 && imported.symbols.len() <= 1 {
        return Err(format!(
            "{display} [fxg-empty]: FXG document contains no graphic elements or library definitions."
        ));
    }
    let root_id = imported.root.clone();
    let root_symbol = imported
        .symbols
        .iter()
        .find(|s| s.id == root_id)
        .cloned()
        .ok_or_else(|| format!("{display}: FXG import produced no root symbol"))?;
    let helper_symbols: Vec<Symbol> = imported
        .symbols
        .into_iter()
        .filter(|s| s.id != root_id)
        .collect();
    Ok(PreparedImportGraphic {
        graphic: PreparedGraphic {
            name,
            width: (imported.width as f64).max(1.0),
            height: (imported.height as f64).max(1.0),
            elements: Vec::new(),
            vector_effects: Vec::new(),
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::Graphic,
        playback: InstancePlayback::SingleFrame(0),
        helper_symbols,
    })
}

fn prepare_sequence_graphic(
    imported: neoflash_import::ImportedNumberedImageSequence,
    placement: neoflash_import::ImageSequencePlacement,
    remaining: &mut usize,
) -> Result<PreparedImportGraphic, String> {
    let neoflash_import::ImportedNumberedImageSequence {
        mut project,
        report,
        detection: _,
        paths: _,
    } = imported;
    freshen_imported_project_ids(&mut project, "seq");
    let resource_bytes = project
        .resources
        .iter()
        .try_fold(0usize, |total, resource| {
            total.checked_add(resource.byte_len())
        })
        .ok_or_else(|| "Image sequence resource byte count overflowed.".to_owned())?;
    if resource_bytes > *remaining {
        return Err("Image sequence exceeds remaining import budget.".into());
    }
    *remaining -= resource_bytes;
    let seq_name = project.name.clone();
    let (w, h) = (project.width, project.height);
    let root_id = project.root.clone();
    let source_symbol = project
        .symbols
        .iter()
        .find(|s| s.id != root_id)
        .or_else(|| project.symbols.iter().find(|s| s.id == root_id))
        .cloned()
        .ok_or_else(|| "Image sequence import produced no sequence symbol".to_owned())?;
    let (symbol_kind, playback) = match placement {
        neoflash_import::ImageSequencePlacement::ConvertToMovieClip => {
            (SymbolKind::MovieClip, InstancePlayback::Independent)
        }
        neoflash_import::ImageSequencePlacement::ConvertToGraphicSymbol
        | neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer
        | neoflash_import::ImageSequencePlacement::SeparateLayers => {
            (SymbolKind::Graphic, InstancePlayback::Loop)
        }
    };

    Ok(PreparedImportGraphic {
        graphic: PreparedGraphic {
            name: seq_name,
            width: f64::from(w),
            height: f64::from(h),
            elements: Vec::new(),
            vector_effects: Vec::new(),
            resources: std::mem::take(&mut project.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(source_symbol.timeline),
        symbol_kind,
        playback,
        helper_symbols: Vec::new(),
    })
}

fn freshen_imported_project_ids(imported: &mut Project, prefix: &str) {
    imported.freshen_all_ids(false, |kind| new_id(&format!("{prefix}-{kind}")));
}

fn prepare_imported_figma_project(
    path: &std::path::Path,
    mut imported: Project,
    report: &neoflash_import::ImportReport,
    remaining: &mut usize,
) -> Result<PreparedImportGraphic, String> {
    let display = path.display().to_string();
    let resource_bytes = imported
        .resources
        .iter()
        .try_fold(0usize, |total, resource| {
            total.checked_add(resource.byte_len())
        })
        .ok_or_else(|| format!("{display}: Figma resource byte count overflowed."))?;
    if resource_bytes > *remaining {
        return Err(format!(
            "{display}: imported Figma resources exceed remaining import budget."
        ));
    }
    *remaining -= resource_bytes;
    freshen_imported_project_ids(&mut imported, "fig");
    let root_id = imported.root.clone();
    let root_symbol = imported
        .symbols
        .iter()
        .find(|s| s.id == root_id)
        .cloned()
        .ok_or_else(|| format!("{display}: Figma import produced no root symbol"))?;
    let helper_symbols: Vec<Symbol> = imported
        .symbols
        .into_iter()
        .filter(|s| s.id != root_id)
        .collect();
    let raw_stem = path
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|n| {
            n.strip_suffix(".figma.json")
                .or_else(|| n.strip_suffix(".FIGMA.JSON"))
        })
        .or_else(|| path.file_stem().and_then(|s| s.to_str()))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Figma");
    Ok(PreparedImportGraphic {
        graphic: PreparedGraphic {
            name: raw_stem.to_owned(),
            width: (imported.width as f64).max(1.0),
            height: (imported.height as f64).max(1.0),
            elements: Vec::new(),
            vector_effects: Vec::new(),
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::MovieClip,
        playback: InstancePlayback::SingleFrame(0),
        helper_symbols,
    })
}

fn prepare_figma_graphic(
    path: &std::path::Path,
    remaining: &mut usize,
) -> Result<PreparedImportGraphic, String> {
    let display = path.display().to_string();
    let (imported, report) =
        neoflash_import::import_figma_path(path, &neoflash_import::FigmaImportOptions::default())
            .map_err(|e| format!("{display} [figma-import]: {e}"))?;
    prepare_imported_figma_project(path, imported, &report, remaining)
}

fn prepare_swf_graphic(
    path: &std::path::Path,
    remaining: &mut usize,
) -> Result<PreparedImportGraphic, String> {
    let display = path.display().to_string();
    let meta = std::fs::metadata(path).map_err(|e| format!("{display}: {e}"))?;
    let len = usize::try_from(meta.len()).unwrap_or(usize::MAX);
    if len > *remaining {
        return Err(format!(
            "{display}: file size exceeds remaining import budget."
        ));
    }
    *remaining = remaining.saturating_sub(len);
    let report =
        neoflash_import::inspect(path).map_err(|e| format!("{display} [swf-inspect]: {e}"))?;
    let mut imported = neoflash_import::import_project(path)
        .map_err(|e| format!("{display} [swf-import]: {e}"))?;
    freshen_imported_project_ids(&mut imported, "swf");
    let root_id = imported.root.clone();
    let root_symbol = imported
        .symbols
        .iter()
        .find(|s| s.id == root_id)
        .cloned()
        .ok_or_else(|| format!("{display}: SWF import produced no root symbol"))?;
    let helper_symbols: Vec<Symbol> = imported
        .symbols
        .into_iter()
        .filter(|s| s.id != root_id)
        .collect();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("SWF")
        .to_owned();
    Ok(PreparedImportGraphic {
        graphic: PreparedGraphic {
            name,
            width: (imported.width as f64).max(1.0),
            height: (imported.height as f64).max(1.0),
            elements: Vec::new(),
            vector_effects: Vec::new(),
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::MovieClip,
        playback: InstancePlayback::Independent,
        helper_symbols,
    })
}

fn prepare_graphic(unit: ResourceUnit, name: String) -> Result<PreparedGraphic, String> {
    let mut graphic = match unit.media_type.as_str() {
        "image/svg+xml" => crate::svg_graphics::prepare_svg(unit)?,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp" | "image/tiff" => {
            let image = unit.decode_image().map_err(|e| e.to_string())?;
            PreparedGraphic {
                name: name.clone(),
                width: image.width as f64,
                height: image.height as f64,
                elements: vec![Element {
                    id: new_id("object"),
                    name: name.clone(),
                    transform: Transform::default(),
                    kind: ElementKind::Bitmap {
                        resource_id: unit.id.clone(),
                        width: image.width,
                        height: image.height,
                        smoothing: true,
                    },
                    scripts: vec![],
                    accessibility: None,
                    ..Element::default()
                }],
                vector_effects: Vec::new(),
                resources: vec![unit],
                notice: None,
            }
        }
        _ => {
            return Err(format!(
                "{}: use SVG, PNG, JPEG, WebP, GIF, BMP or TIFF for Stage graphics.",
                name
            ));
        }
    };
    graphic.name = name;
    Ok(graphic)
}

pub(crate) fn import_svg_project_from_path(
    path: &std::path::Path,
) -> Result<(Project, Vec<String>), String> {
    let mut unit = crate::resources::read_resource(path, MAX_RESOURCE_TOTAL_BYTES)?;
    if unit.media_type != "image/svg+xml" {
        unit.media_type = "image/svg+xml".into();
    }
    let mut graphic = crate::svg_graphics::prepare_svg(unit)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("SVG")
        .to_owned();
    let mut project = Project::blank();
    project.name = name;
    project.width = graphic.width.round().clamp(1.0, 65_535.0) as u32;
    project.height = graphic.height.round().clamp(1.0, 65_535.0) as u32;
    project.format_version = project.format_version.max(3);
    merge_graphic_resources(&mut project, &mut graphic)?;
    let root_id = project.root.clone();
    if let Some(root_sym) = project.symbol_mut(&root_id)
        && let Some(layer) = root_sym.timeline.layers.first_mut()
    {
        layer.name = "SVG".into();
        if let Some(kf) = layer.keyframes.first_mut() {
            kf.elements = graphic.elements;
            kf.vector_effects = graphic.vector_effects;
        }
    }
    crate::timeline_commands::upgrade_all_format_versions(&mut project);
    project.validate().map_err(|e| e.to_string())?;
    let mut notices = Vec::new();
    if let Some(notice) = graphic.notice {
        notices.push(notice);
    }
    Ok((project, notices))
}

pub(crate) fn drop_window_is_front(ctx: &egui::Context, local: Pos2) -> bool {
    let Some(inner) = ctx.input(|input| input.viewport().inner_rect) else {
        return true;
    };
    let root = inner * ctx.zoom_factor();
    let global = (local + inner.min.to_vec2()) * ctx.zoom_factor();
    match crate::native_screens::front_dock_window_at(global, None, &[], false) {
        None => true,
        Some(None) => false,
        Some(Some(crate::native_screens::NativeDockWindow::Main(_))) => {
            ctx.viewport_id() == egui::ViewportId::ROOT
        }
        Some(Some(crate::native_screens::NativeDockWindow::Floating { rect, .. })) => {
            root.min.distance(rect.min) < 8. && root.max.distance(rect.max) < 8.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Tool;
    use image::{ImageEncoder, codecs::png::PngEncoder};

    fn make_png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut raw = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            raw.extend_from_slice(&rgba);
        }
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(&raw, w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn make_hierarchical_psd_bytes(canvas_w: u32, canvas_h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0u8; 6]);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&canvas_h.to_be_bytes());
        out.extend_from_slice(&canvas_w.to_be_bytes());
        out.extend_from_slice(&8u16.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        // This fixture exercises editable layers. Its dummy merged pixels are not a saved Photoshop composite.
        let mut version_info = Vec::from(*b"8BIM");
        version_info.extend_from_slice(&1057u16.to_be_bytes());
        version_info.extend_from_slice(&[0, 0]);
        version_info.extend_from_slice(&17u32.to_be_bytes());
        version_info.extend_from_slice(&1u32.to_be_bytes());
        version_info.push(0); // hasRealMergedData
        version_info.extend_from_slice(&[0; 8]); // empty writer and reader Unicode strings
        version_info.extend_from_slice(&1u32.to_be_bytes());
        version_info.push(0); // even resource length
        out.extend_from_slice(&(version_info.len() as u32).to_be_bytes());
        out.extend_from_slice(&version_info);

        struct LRec<'a> {
            name: &'a str,
            bounds: [i32; 4],
            blend: &'a [u8; 4],
            clipping: u8,
            flags: u8,
            extra: Vec<u8>,
            channels: Vec<(i16, Vec<u8>)>,
        }

        let raw_ch = |val: u8, n: usize| -> Vec<u8> {
            let mut b = Vec::with_capacity(2 + n);
            b.extend_from_slice(&0u16.to_be_bytes());
            b.extend(std::iter::repeat_n(val, n));
            b
        };

        let lsct = |kind: u32| -> Vec<u8> {
            let mut b = Vec::new();
            b.extend_from_slice(b"8BIMlsct");
            b.extend_from_slice(&4u32.to_be_bytes());
            b.extend_from_slice(&kind.to_be_bytes());
            b
        };

        let mut tysh_extra = Vec::new();
        tysh_extra.extend_from_slice(b"8BIMTySh");
        let mut tysh = Vec::new();
        tysh.extend_from_slice(&1u16.to_be_bytes());
        for &v in &[1.0f64, 0.0, 0.0, 1.0, 8.0, 12.0] {
            tysh.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        tysh.extend_from_slice(b"Txt TEXT");
        let utf16: Vec<u16> = "PSD Drop".encode_utf16().collect();
        tysh.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
        for u in utf16 {
            tysh.extend_from_slice(&u.to_be_bytes());
        }
        if tysh.len() % 2 != 0 {
            tysh.push(0);
        }
        tysh_extra.extend_from_slice(&(tysh.len() as u32).to_be_bytes());
        tysh_extra.extend_from_slice(&tysh);
        tysh_extra.extend_from_slice(b"8BIMcurv");
        tysh_extra.extend_from_slice(&4u32.to_be_bytes());
        tysh_extra.extend_from_slice(&0u32.to_be_bytes());

        let layers = vec![
            LRec {
                name: "</Layer group>",
                bounds: [0, 0, 0, 0],
                blend: b"norm",
                clipping: 0,
                flags: 0,
                extra: lsct(3),
                channels: vec![
                    (0, raw_ch(0, 0)),
                    (1, raw_ch(0, 0)),
                    (2, raw_ch(0, 0)),
                    (-1, raw_ch(0, 0)),
                ],
            },
            LRec {
                name: "MaskBase",
                bounds: [0, 0, 4, 4],
                blend: b"norm",
                clipping: 0,
                flags: 0,
                extra: Vec::new(),
                channels: vec![
                    (0, raw_ch(200, 16)),
                    (1, raw_ch(100, 16)),
                    (2, raw_ch(50, 16)),
                    (-1, raw_ch(255, 16)),
                ],
            },
            LRec {
                name: "ClippedMul",
                bounds: [0, 0, 4, 4],
                blend: b"mult",
                clipping: 1,
                flags: 0,
                extra: Vec::new(),
                channels: vec![
                    (0, raw_ch(40, 16)),
                    (1, raw_ch(180, 16)),
                    (2, raw_ch(240, 16)),
                    (-1, raw_ch(255, 16)),
                ],
            },
            LRec {
                name: "TitleText",
                bounds: [2, 2, 6, 6],
                blend: b"norm",
                clipping: 0,
                flags: 0,
                extra: tysh_extra,
                channels: vec![
                    (0, raw_ch(20, 16)),
                    (1, raw_ch(20, 16)),
                    (2, raw_ch(20, 16)),
                    (-1, raw_ch(255, 16)),
                ],
            },
            LRec {
                name: "RigFolder",
                bounds: [0, 0, 0, 0],
                blend: b"pass",
                clipping: 0,
                flags: 0,
                extra: lsct(1),
                channels: vec![
                    (0, raw_ch(0, 0)),
                    (1, raw_ch(0, 0)),
                    (2, raw_ch(0, 0)),
                    (-1, raw_ch(0, 0)),
                ],
            },
        ];

        let mut linfo = Vec::new();
        linfo.extend_from_slice(&(layers.len() as i16).to_be_bytes());
        for l in &layers {
            for b in l.bounds {
                linfo.extend_from_slice(&b.to_be_bytes());
            }
            linfo.extend_from_slice(&(l.channels.len() as u16).to_be_bytes());
            for (cid, blob) in &l.channels {
                linfo.extend_from_slice(&cid.to_be_bytes());
                linfo.extend_from_slice(&(blob.len() as u32).to_be_bytes());
            }
            linfo.extend_from_slice(b"8BIM");
            linfo.extend_from_slice(l.blend);
            linfo.push(255);
            linfo.push(l.clipping);
            linfo.push(l.flags);
            linfo.push(0);

            let mut extra = Vec::new();
            extra.extend_from_slice(&0u32.to_be_bytes());
            extra.extend_from_slice(&0u32.to_be_bytes());
            let nb = l.name.as_bytes();
            extra.push(nb.len() as u8);
            extra.extend_from_slice(nb);
            let pad = (4 - ((1 + nb.len()) % 4)) % 4;
            extra.extend(std::iter::repeat_n(0u8, pad));
            extra.extend_from_slice(&l.extra);

            linfo.extend_from_slice(&(extra.len() as u32).to_be_bytes());
            linfo.extend_from_slice(&extra);
        }
        for l in &layers {
            for (_, blob) in &l.channels {
                linfo.extend_from_slice(blob);
            }
        }
        let mut lsection = Vec::new();
        lsection.extend_from_slice(&(linfo.len() as u32).to_be_bytes());
        lsection.extend_from_slice(&linfo);
        out.extend_from_slice(&(lsection.len() as u32).to_be_bytes());
        out.extend_from_slice(&lsection);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend(std::iter::repeat_n(
            128u8,
            (canvas_w * canvas_h * 4) as usize,
        ));
        out
    }

    fn make_ase_bytes() -> Vec<u8> {
        let mut ase = Vec::new();
        ase.extend_from_slice(b"ASEF");
        ase.extend_from_slice(&1u16.to_be_bytes());
        ase.extend_from_slice(&0u16.to_be_bytes());
        ase.extend_from_slice(&1u32.to_be_bytes());
        let mut blk = Vec::new();
        let name_utf16: [u16; 5] = [b'G' as u16, b'o' as u16, b'l' as u16, b'd' as u16, 0];
        blk.extend_from_slice(&(name_utf16.len() as u16).to_be_bytes());
        for u in name_utf16 {
            blk.extend_from_slice(&u.to_be_bytes());
        }
        blk.extend_from_slice(b"RGB ");
        blk.extend_from_slice(&1.0f32.to_be_bytes());
        blk.extend_from_slice(&0.75f32.to_be_bytes());
        blk.extend_from_slice(&0.25f32.to_be_bytes());
        blk.extend_from_slice(&0u16.to_be_bytes());
        ase.extend_from_slice(&0x0001u16.to_be_bytes());
        ase.extend_from_slice(&(blk.len() as u32).to_be_bytes());
        ase.extend_from_slice(&blk);
        ase
    }

    #[test]
    fn professional_imports_via_finder_drop_and_menus_preserve_hierarchy_placement_undo_and_export()
    {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-pro-imports-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        let ctx = egui::Context::default();
        let mut base_project = Project::blank();
        base_project.name = "AuthoringHost".into();
        base_project.width = 960;
        base_project.height = 540;
        base_project.fps = 30;
        base_project.background = [18, 22, 28, 255];
        base_project.symbols[0].timeline.frame_count = 1;

        let mut editor = Editor::from_project(&ctx, base_project);
        editor.set_tool(Tool::Pen);
        editor.history.clear();
        let before = editor.project.clone();

        let mapping =
            crate::stage_mapping::StageMapping::try_new([1.2, 0.1, -0.2, 0.8, 40.0, 20.0]).unwrap();
        let origin = egui::pos2(50.0, 70.0);
        editor.zoom = 1.5;
        let local = egui::pos2(200.0, 140.0);
        let screen = mapping.edit_to_screen(origin, editor.zoom, local);
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));

        // 1. Finder drop of PSD onto Stage at pointer coordinates
        let psd_path = temp.join("HeroRig.psd");
        std::fs::write(&psd_path, make_hierarchical_psd_bytes(60, 40)).unwrap();
        let raw_psd = egui::RawInput {
            screen_rect: Some(area),
            events: vec![egui::Event::PointerMoved(screen)],
            dropped_files: vec![egui::DroppedFile {
                path: Some(psd_path.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw_psd, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.stage_file_drop(ui, area, origin, mapping);
            });
        });

        // Open-document identity and sticky tool are preserved
        assert_eq!(editor.tool, Tool::Pen);
        assert_eq!(editor.project.name, "AuthoringHost");
        assert_eq!(editor.project.width, 960);
        assert_eq!(editor.project.height, 540);
        assert_eq!(editor.project.fps, 30);
        assert_eq!(editor.project.background, [18, 22, 28, 255]);
        assert_eq!(editor.project.root, before.root);
        assert!(
            editor
                .status
                .contains("[psd-adjustment-layer-approximated]"),
            "PSD approximation warning must be reported to Output/status: {}",
            editor.status
        );

        let psd_sym = editor
            .project
            .symbols
            .iter()
            .find(|s| s.name == "HeroRig")
            .expect("HeroRig symbol created in Library");
        assert!(
            psd_sym
                .timeline
                .layers
                .iter()
                .any(|l| l.name == "RigFolder" && l.properties.role == LayerRole::Folder)
        );
        assert!(
            psd_sym
                .timeline
                .layers
                .iter()
                .any(|l| l.name == "MaskBase" && l.properties.role == LayerRole::Mask)
        );
        assert!(
            psd_sym
                .timeline
                .layers
                .iter()
                .any(|l| l.name == "ClippedMul" && l.properties.blend_mode == BlendMode::Multiply)
        );
        let placed_psd = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|el| {
                matches!(&el.kind, ElementKind::Instance { symbol_id, .. } if *symbol_id == psd_sym.id)
            })
            .expect("PSD symbol instance placed on Stage");
        assert!((placed_psd.transform.x - (f64::from(local.x) - 30.0)).abs() < 1e-4);
        assert!((placed_psd.transform.y - (f64::from(local.y) - 20.0)).abs() < 1e-4);
        editor.project.validate().unwrap();

        // Single Undo restores exact prior project
        editor.undo();
        assert_eq!(editor.project, before);

        // 2. Finder drop of numbered PNG sequence with gap hold (walk_0001, walk_0002, walk_0004)
        let f1 = temp.join("walk_0001.png");
        let f2 = temp.join("walk_0002.png");
        let f4 = temp.join("walk_0004.png");
        std::fs::write(&f1, make_png_bytes(40, 30, [255, 0, 0, 255])).unwrap();
        std::fs::write(&f2, make_png_bytes(40, 30, [0, 255, 0, 255])).unwrap();
        std::fs::write(&f4, make_png_bytes(40, 30, [0, 0, 255, 255])).unwrap();

        let raw_seq = egui::RawInput {
            screen_rect: Some(area),
            events: vec![egui::Event::PointerMoved(screen)],
            dropped_files: vec![
                egui::DroppedFile {
                    path: Some(f1.clone()),
                    ..Default::default()
                },
                egui::DroppedFile {
                    path: Some(f2.clone()),
                    ..Default::default()
                },
                egui::DroppedFile {
                    path: Some(f4.clone()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(raw_seq, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.stage_file_drop(ui, area, origin, mapping);
            });
        });

        assert!(
            editor.status.contains("[sequence-missing-frame]"),
            "Gap warning must be reported: {}",
            editor.status
        );
        let seq_sym = editor
            .project
            .symbols
            .iter()
            .find(|s| s.name == "walk")
            .expect("walk sequence symbol created");
        assert_eq!(seq_sym.timeline.frame_count, 4);
        assert_eq!(seq_sym.timeline.layers[0].keyframes.len(), 3);
        assert_eq!(editor.symbol().timeline.frame_count, 4);
        let placed_seq = editor
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|el| {
                matches!(
                    &el.kind,
                    ElementKind::Instance {
                        symbol_id,
                        playback: InstancePlayback::Loop,
                    } if *symbol_id == seq_sym.id
                )
            })
            .expect("looping sequence instance placed on Stage");
        assert!((placed_seq.transform.x - (f64::from(local.x) - 20.0)).abs() < 1e-4);
        assert!((placed_seq.transform.y - (f64::from(local.y) - 15.0)).abs() < 1e-4);
        editor.project.validate().unwrap();

        editor.undo();
        assert_eq!(editor.project, before);

        // 3. PDF/AI and FXG 2.0 imports + ACT/CLR/ASE palettes, then save/reopen and HTML5 export
        let pdf_path = temp.join("VectorCard.ai");
        let pdf_bytes = b"%PDF-1.4\n\
            1 0 obj << /Type /Page /MediaBox [0 0 100 80] >> endobj\n\
            2 0 obj << /Length 160 >>\n\
            stream\n\
            /CardLayer BDC\n\
            q 0 0 100 80 re W n 0.2 0.6 0.9 rg 10 10 m 90 10 l 90 70 l h f BT /F1 14 Tf 10 20 Td (Card) Tj ET Q\n\
            EMC\n\
            endstream\n\
            endobj\n";
        std::fs::write(&pdf_path, pdf_bytes).unwrap();

        let fxg_path = temp.join("IconPack.fxg");
        let fxg_xml = r##"<?xml version="1.0" encoding="utf-8"?>
<Graphic version="2.0" viewWidth="80" viewHeight="60" xmlns="http://ns.adobe.com/fxg/2008">
  <Library>
    <Definition name="StarSub">
      <Ellipse width="20" height="20"><fill><SolidColor color="#FFAA00"/></fill></Ellipse>
    </Definition>
  </Library>
  <Group id="IconGroup">
    <Rect x="0" y="0" width="80" height="60"><fill><SolidColor color="#113355"/></fill></Rect>
    <StarSub x="20" y="20"/>
  </Group>
</Graphic>"##;
        std::fs::write(&fxg_path, fxg_xml).unwrap();

        let act_path = temp.join("Theme.act");
        std::fs::write(
            &act_path,
            neoflash_import::export_act_palette(&[GlobalSwatch {
                id: "act-1".into(),
                name: "Crimson".into(),
                color: [210, 30, 50, 255],
            }]),
        )
        .unwrap();
        let clr_path = temp.join("Theme.clr");
        std::fs::write(
            &clr_path,
            neoflash_import::export_clr_palette(&[GlobalSwatch {
                id: "clr-1".into(),
                name: "Teal".into(),
                color: [20, 180, 170, 255],
            }]),
        )
        .unwrap();
        let ase_path = temp.join("Theme.ase");
        std::fs::write(&ase_path, make_ase_bytes()).unwrap();

        editor
            .import_graphics_paths(
                &[
                    psd_path.clone(),
                    pdf_path.clone(),
                    fxg_path.clone(),
                    act_path.clone(),
                    clr_path.clone(),
                    ase_path.clone(),
                ],
                Some([200.0, 140.0]),
            )
            .expect("combined batch import succeeds");
        assert_eq!(editor.project.name, "AuthoringHost");
        assert!(editor.project.symbols.iter().any(|s| s.name == "HeroRig"));
        assert!(
            editor
                .project
                .symbols
                .iter()
                .any(|s| s.name == "VectorCard")
        );
        assert!(editor.project.symbols.iter().any(|s| s.name == "IconPack"));
        assert!(editor.project.symbols.iter().any(|s| s.name == "StarSub"));
        assert_eq!(editor.project.custom_swatches().len(), 3);
        assert_eq!(editor.project.global_swatches().len(), 3);

        // Save/reopen round-trip and HTML5 export (`neoflash_export::export_project`)
        let saved_json = editor
            .project
            .to_json()
            .expect("serialize imported project");
        let reopened = Project::from_json(&saved_json).expect("reopen imported project");
        reopened.validate().expect("reopened project validates");
        assert_eq!(reopened, editor.project);
        let runtime_dir = temp.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("neoflash_web.js"),
            "globalThis.NeoFlashWasm = {};",
        )
        .unwrap();
        std::fs::write(
            runtime_dir.join("neoflash_web_bg.wasm"),
            b"\0asm\x01\x00\x00\x00",
        )
        .unwrap();
        let html_out = temp.join("html5-out");
        let index_path = neoflash_export::export_project(&reopened, &html_out, &runtime_dir)
            .expect("HTML5 export succeeds");
        assert!(index_path.exists());

        // Single Undo reverts the entire combined import batch
        editor.undo();
        assert_eq!(editor.project, before);

        // 4. Atomic failure on malformed inputs leaves `editor.project` completely untouched
        let bad_psd = temp.join("corrupt.psd");
        std::fs::write(&bad_psd, b"8BPS\x00\x01truncated").unwrap();
        let err_psd = editor
            .import_graphics_paths(std::slice::from_ref(&bad_psd), Some([100.0, 100.0]))
            .expect_err("truncated PSD must fail");
        assert!(err_psd.contains("corrupt.psd"), "{err_psd}");
        assert_eq!(editor.project, before);

        let bad_seq_2 = temp.join("walk_0002.png");
        std::fs::write(&bad_seq_2, b"NOT_A_PNG_FRAME").unwrap();
        let err_seq = editor
            .import_graphics_paths(&[f1.clone(), bad_seq_2], Some([100.0, 100.0]))
            .expect_err("corrupt sequence frame must fail atomically");
        assert!(err_seq.contains("sequence-unreadable-frame"), "{err_seq}");
        assert_eq!(editor.project, before);

        let bad_pdf = temp.join("empty.pdf");
        std::fs::write(&bad_pdf, b"%PDF-1.4\n1 0 obj << >> endobj\n").unwrap();
        let err_pdf = editor
            .import_graphics_paths(std::slice::from_ref(&bad_pdf), None)
            .expect_err("empty PDF must fail atomically");
        assert!(err_pdf.contains("pdf-no-vector-content"), "{err_pdf}");
        assert_eq!(editor.project, before);

        let bad_fxg = temp.join("broken.fxg");
        std::fs::write(&bad_fxg, b"<Graphic version='2.0'><unclosed").unwrap();
        let err_fxg = editor
            .import_graphics_paths(std::slice::from_ref(&bad_fxg), None)
            .expect_err("malformed FXG must fail atomically");
        assert!(err_fxg.contains("broken.fxg"), "{err_fxg}");
        assert_eq!(editor.project, before);

        let bad_ase = temp.join("broken.ase");
        std::fs::write(&bad_ase, b"ASEF\x00").unwrap();
        let err_ase = editor
            .import_palette_paths(std::slice::from_ref(&bad_ase))
            .expect_err("truncated ASE must fail atomically");
        assert!(err_ase.contains("broken.ase"), "{err_ase}");
        assert_eq!(editor.project, before);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn over_limit_numbered_sequence_fails_atomically_before_decoding() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-sequence-bounds-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let oversized = temp.join("bounded_0001.png");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_RESOURCE_BYTES as u64 + 1)
            .unwrap();
        let valid = temp.join("bounded_0002.png");
        std::fs::write(&valid, make_png_bytes(2, 2, [10, 20, 30, 255])).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.history.clear();
        let project_before = editor.project.clone();
        let saved_before = editor.saved_project.clone();
        let selection_before = editor.selection.clone();
        let dirty_before = editor.dirty();

        let error = editor
            .import_graphics_paths(&[oversized, valid], Some([25.0, 30.0]))
            .expect_err("oversized sequence frame must be rejected");
        assert!(error.contains("resource limit"), "{error}");
        assert_eq!(editor.project, project_before);
        assert_eq!(editor.saved_project, saved_before);
        assert_eq!(editor.selection, selection_before);
        assert_eq!(editor.dirty(), dirty_before);
        assert!(!editor.history.can_undo());

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn mixed_font_and_invalid_asset_import_is_byte_atomic_without_undo() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-mixed-font-atomic-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let font = temp.join("Inter-Regular.ttf");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../neoflash-ui/assets/fonts/Inter-Regular.ttf"),
            &font,
        )
        .unwrap();
        let invalid_graphic = temp.join("broken.png");
        std::fs::write(&invalid_graphic, b"not a PNG").unwrap();
        let invalid_palette = temp.join("broken.ase");
        std::fs::write(&invalid_palette, b"ASEF\0").unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.history.clear();
        let before = editor.project.to_json().unwrap();

        for invalid in [&invalid_graphic, &invalid_palette] {
            let error = editor
                .import_graphics_to_library_paths(&[font.clone(), invalid.clone()])
                .expect_err("a malformed palette/graphic must reject its mixed font batch");
            assert!(!error.trim().is_empty());
            assert_eq!(
                editor.project.to_json().unwrap(),
                before,
                "failed mixed import must leave the serialized project byte-identical"
            );
            assert_eq!(
                editor.history.undo_len(),
                0,
                "failed mixed import must not add a partial font Undo entry"
            );
        }

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn mixed_swc_font_palette_graphic_and_invalid_audio_leave_no_partial_state() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-mixed-import-failure-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let swc = write_test_swc(&temp);
        let font = temp.join("Inter-Regular.ttf");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../neoflash-ui/assets/fonts/Inter-Regular.ttf"),
            &font,
        )
        .unwrap();
        let palette = temp.join("Theme.ase");
        std::fs::write(&palette, make_ase_bytes()).unwrap();
        let graphic = temp.join("poster.png");
        std::fs::write(&graphic, make_png_bytes(4, 4, [20, 120, 220, 255])).unwrap();
        let invalid_audio = temp.join("oversized.wav");
        std::fs::File::create(&invalid_audio)
            .unwrap()
            .set_len(MAX_RESOURCE_BYTES as u64 + 1)
            .unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.history.clear();
        let project_before = editor.project.to_json().unwrap();
        let sources_before = editor.timeline_audio.source_paths.clone();
        let output_before = editor.output.len();
        let resource_selection_before = editor.resource_panel.selected.clone();

        let error = editor
            .import_graphics_paths(
                &[swc, font, invalid_audio, palette, graphic],
                Some([120.0, 80.0]),
            )
            .expect_err("an invalid late category must reject the complete mixed import");
        assert!(!error.trim().is_empty());
        assert_eq!(editor.project.to_json().unwrap(), project_before);
        assert_eq!(editor.history.undo_len(), 0);
        assert_eq!(editor.timeline_audio.source_paths, sources_before);
        assert_eq!(editor.output.len(), output_before);
        assert_eq!(editor.resource_panel.selected, resource_selection_before);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn late_graphic_capacity_failure_does_not_leak_audio_attachment_or_source_path() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-mixed-import-capacity-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let wav = write_test_wav(&temp);
        let graphic = temp.join("poster.png");
        std::fs::write(&graphic, make_png_bytes(4, 4, [20, 120, 220, 255])).unwrap();

        let mut project = Project::blank();
        for index in 0..(MAX_RESOURCES - 1) {
            project.resources.push(
                ResourceUnit::from_bytes(
                    format!("padding-{index}"),
                    format!("padding/{index:04}.txt"),
                    "text/plain",
                    &[],
                )
                .unwrap(),
            );
        }
        crate::timeline_commands::upgrade_all_format_versions(&mut project);
        project.validate().unwrap();
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, project);
        editor.history.clear();
        let project_before = editor.project.to_json().unwrap();
        let sources_before = editor.timeline_audio.source_paths.clone();
        let output_before = editor.output.len();

        let error = editor
            .import_graphics_paths(&[wav, graphic], Some([120.0, 80.0]))
            .expect_err("the final graphic must exceed the combined resource count");
        assert!(!error.trim().is_empty());
        assert_eq!(editor.project.to_json().unwrap(), project_before);
        assert_eq!(editor.history.undo_len(), 0);
        assert_eq!(editor.timeline_audio.source_paths, sources_before);
        assert_eq!(editor.output.len(), output_before);
        assert!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .sound
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn mixed_swc_audio_font_palette_and_graphic_import_is_one_undo() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-mixed-import-success-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let swc = write_test_swc(&temp);
        let wav = write_test_wav(&temp);
        let font = temp.join("Inter-Regular.ttf");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../neoflash-ui/assets/fonts/Inter-Regular.ttf"),
            &font,
        )
        .unwrap();
        let palette = temp.join("Theme.ase");
        std::fs::write(&palette, make_ase_bytes()).unwrap();
        let graphic = temp.join("poster.png");
        std::fs::write(&graphic, make_png_bytes(4, 4, [20, 120, 220, 255])).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.history.clear();
        let project_before = editor.project.to_json().unwrap();
        editor
            .import_graphics_paths(
                &[swc, wav.clone(), font, palette, graphic],
                Some([120.0, 80.0]),
            )
            .expect("all mixed categories should commit together");

        assert_eq!(editor.history.undo_len(), 1);
        assert!(
            editor
                .project
                .sources
                .iter()
                .any(|source| source.path.ends_with("com/example/BadgeClip.as"))
        );
        assert!(
            editor
                .project
                .resources
                .iter()
                .any(|unit| unit.media_type.starts_with("font/"))
        );
        assert!(
            editor
                .project
                .resources
                .iter()
                .any(|unit| unit.path == "fonts/embedding.json")
        );
        let audio_id = editor
            .project
            .resources
            .iter()
            .find(|unit| unit.path == "click.wav" && unit.media_type == "audio/wav")
            .unwrap()
            .id
            .clone();
        assert_eq!(
            editor.timeline_audio.source_paths.get(&audio_id),
            Some(&wav)
        );
        assert_eq!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .sound
                .as_ref()
                .map(|sound| &sound.resource),
            Some(&audio_id)
        );
        assert!(
            editor
                .project
                .global_swatches()
                .iter()
                .any(|swatch| swatch.name == "Gold")
        );
        let poster = editor
            .project
            .symbols
            .iter()
            .find(|symbol| symbol.name == "poster")
            .expect("graphic symbol");
        assert!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .elements
                .iter()
                .any(|element| matches!(
                    &element.kind,
                    ElementKind::Instance { symbol_id, .. } if symbol_id == &poster.id
                ))
        );
        assert!(
            editor
                .output
                .events()
                .iter()
                .any(|event| event.message.contains("Linked SWC 'badge_kit'"))
        );
        editor.project.validate().unwrap();

        editor.undo();
        assert_eq!(editor.project.to_json().unwrap(), project_before);
        assert_eq!(editor.history.undo_len(), 0);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn figma_swf_tiff_and_avi_imports_preserve_layers_undo_and_export() {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-t247-t249-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();

        // 1. Author a layered `.figma.json` file with background rectangle, vector path, and text
        let figma_path = temp.join("HeroBanner.figma.json");
        std::fs::write(
            &figma_path,
            r#"{
              "name": "HeroBanner",
              "document": {
                "id": "0:0",
                "type": "DOCUMENT",
                "children": [{
                  "id": "0:1",
                  "name": "Page 1",
                  "type": "CANVAS",
                  "children": [{
                    "id": "1:1",
                    "name": "HeroBanner",
                    "type": "FRAME",
                    "absoluteBoundingBox": { "x": 0, "y": 0, "width": 320, "height": 180 },
                    "fills": [{ "type": "SOLID", "color": { "r": 0.1, "g": 0.12, "b": 0.2, "a": 1.0 } }],
                    "children": [
                      {
                        "id": "1:2",
                        "name": "AccentBar",
                        "type": "RECTANGLE",
                        "absoluteBoundingBox": { "x": 16, "y": 16, "width": 120, "height": 32 },
                        "fills": [{ "type": "SOLID", "color": { "r": 0.95, "g": 0.45, "b": 0.15, "a": 1.0 } }]
                      },
                      {
                        "id": "1:3",
                        "name": "TitleText",
                        "type": "TEXT",
                        "characters": "NEOFLASH FIGMA",
                        "absoluteBoundingBox": { "x": 16, "y": 64, "width": 240, "height": 40 },
                        "style": { "fontFamily": "Inter", "fontSize": 20 },
                        "fills": [{ "type": "SOLID", "color": { "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 } }]
                      }
                    ]
                  }]
                }]
              }
            }"#,
        )
        .unwrap();

        // 2. Build a valid minimal SWF and TIFF + BMP + WebP rasters + MJPEG AVI video
        let mut swf_proj = Project::blank();
        swf_proj.name = "MiniClip".into();
        swf_proj.symbols[0].timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: new_id("rect"),
                name: "Box".into(),
                kind: ElementKind::Rectangle {
                    width: 48.0,
                    height: 36.0,
                    fill: [40, 190, 140, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
        let (swf_bytes, _) = neoflash_export::publish_swf_bytes(
            &swf_proj,
            &neoflash_export::SwfPublishOptions::default(),
        )
        .expect("compile minimal SWF fixture");
        let swf_path = temp.join("MiniClip.swf");
        std::fs::write(&swf_path, &swf_bytes).unwrap();

        let rgba_img = image::RgbaImage::from_pixel(12, 8, image::Rgba([220, 80, 120, 255]));
        let tiff_path = temp.join("Poster.tiff");
        {
            let mut cursor = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(rgba_img.clone())
                .write_to(&mut cursor, image::ImageFormat::Tiff)
                .unwrap();
            std::fs::write(&tiff_path, cursor.into_inner()).unwrap();
        }
        let bmp_path = temp.join("Badge.bmp");
        {
            let mut cursor = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(rgba_img.clone())
                .write_to(&mut cursor, image::ImageFormat::Bmp)
                .unwrap();
            std::fs::write(&bmp_path, cursor.into_inner()).unwrap();
        }
        let webp_path = temp.join("Sticker.webp");
        {
            let mut cursor = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(rgba_img)
                .write_to(&mut cursor, image::ImageFormat::WebP)
                .unwrap();
            std::fs::write(&webp_path, cursor.into_inner()).unwrap();
        }

        let jpeg_frames: Vec<Vec<u8>> = [[240, 60, 60], [60, 220, 120], [60, 120, 240]]
            .into_iter()
            .map(|rgb| {
                let frame = image::RgbImage::from_pixel(24, 16, image::Rgb(rgb));
                let mut cur = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgb8(frame)
                    .write_to(&mut cur, image::ImageFormat::Jpeg)
                    .unwrap();
                cur.into_inner()
            })
            .collect();
        let avi_bytes = neoflash_core::avi::build_minimal_mjpeg_avi(24, 16, 12, &jpeg_frames, None);
        let avi_path = temp.join("Teaser.avi");
        std::fs::write(&avi_path, &avi_bytes).unwrap();

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.history.clear();
        let initial = editor.project.clone();

        // Import Figma + TIFF + BMP + WebP + SWF #1 in one transaction
        editor
            .import_graphics_paths(
                &[
                    figma_path.clone(),
                    tiff_path.clone(),
                    bmp_path.clone(),
                    webp_path.clone(),
                    swf_path.clone(),
                ],
                Some([200.0, 150.0]),
            )
            .expect("import Figma, TIFF, BMP, WebP, and SWF #1");

        // Import the exact same SWF a second time to verify zero symbol/layer/element ID collisions
        editor
            .import_graphics_paths(std::slice::from_ref(&swf_path), Some([260.0, 180.0]))
            .expect("second SWF import succeeds without ID collisions");

        let hero_sym = editor
            .project
            .symbols
            .iter()
            .find(|s| s.name == "HeroBanner")
            .expect("HeroBanner MovieClip created");
        assert!(
            hero_sym.timeline.layers.len() >= 2,
            "Figma frame decomposed into separate timeline layers"
        );

        // Import AVI video onto the timeline
        let _vid_id = editor
            .import_video_path_at(&ctx, &avi_path, [160.0, 120.0])
            .expect("import AVI video onto Stage");
        editor
            .project
            .validate()
            .expect("project with Figma, repeated SWF, TIFF/BMP/WebP, and AVI validates");

        // Verify pure-Rust offline video decoder handles the imported AVI bytes
        let decoded_avi = neoflash_export::video::decode_video_stream_bytes(
            "Teaser.avi",
            "video/x-msvideo",
            &avi_bytes,
        )
        .expect("pure-Rust AVI offline video decode");
        assert_eq!(decoded_avi.width, 24);
        assert_eq!(decoded_avi.height, 16);
        assert_eq!(decoded_avi.frames.len(), 3);

        // Save/reopen roundtrip + HTML5 export
        let json = editor.project.to_json().expect("serialize project");
        let reopened = Project::from_json(&json).expect("deserialize project");
        reopened.validate().expect("reopened project validates");

        let runtime_dir = temp.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("neoflash_web.js"),
            "globalThis.NeoFlashWasm = {};",
        )
        .unwrap();
        std::fs::write(
            runtime_dir.join("neoflash_web_bg.wasm"),
            b"\0asm\x01\x00\x00\x00",
        )
        .unwrap();
        let html_out = temp.join("html5-out");
        let index_path = neoflash_export::export_project(&reopened, &html_out, &runtime_dir)
            .expect("HTML5 export succeeds with Figma + AVI + TIFF/BMP/WebP + SWF");
        assert!(index_path.exists());

        // Three Undos restore exact initial project state
        editor.undo();
        editor.undo();
        editor.undo();
        assert_eq!(editor.project, initial);

        let _ = std::fs::remove_dir_all(&temp);
    }

    fn build_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            let mut crc = 0xFFFF_FFFFu32;
            for &b in data {
                crc ^= u32::from(b);
                for _ in 0..8 {
                    let mask = (crc & 1).wrapping_neg();
                    crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
                }
            }
            !crc
        }

        let mut out = Vec::new();
        let mut central = Vec::new();
        for &(name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let len = data.len() as u32;
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len() as u16;

            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // store
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data);

            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&len.to_le_bytes());
            central.extend_from_slice(&len.to_le_bytes());
            central.extend_from_slice(&name_len.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name_bytes);
        }
        let cd_offset = out.len() as u32;
        let cd_len = central.len() as u32;
        let count = entries.len() as u16;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_len.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn write_test_swc(temp: &std::path::Path) -> PathBuf {
        let mut swc_source_project = Project::blank();
        let mut badge_symbol = swc_source_project.symbols[0].clone();
        badge_symbol.id = "sym-badge-mc".into();
        badge_symbol.name = "BadgeClip".into();
        badge_symbol.class_name = Some("com.example.BadgeClip".into());
        badge_symbol.timeline.layers[0].keyframes[0]
            .elements
            .push(Element {
                id: "badge-rect".into(),
                name: "Rect".into(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width: 32.0,
                    height: 32.0,
                    fill: [40, 160, 240, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
        swc_source_project.symbols.push(badge_symbol);
        let library_swf = temp.join("library.swf");
        neoflash_export::publish_swf_to_path(
            &swc_source_project,
            &library_swf,
            &neoflash_export::SwfPublishOptions {
                profile: neoflash_export::SwfPublishProfile::FlashPlayer10Avm2,
                scriptless_mode: true,
                ..neoflash_export::SwfPublishOptions::default()
            },
        )
        .expect("build library.swf for SWC fixture");
        let library_swf_bytes = std::fs::read(&library_swf).unwrap();
        let badge_png = make_png_bytes(4, 4, [40, 160, 240, 255]);
        let catalog_xml = br#"<?xml version="1.0" encoding="utf-8"?>
<swc xmlns="http://www.adobe.com/flash/swccatalog/9">
  <versions><swc version="1.2"/></versions>
  <features><feature-script-deps/><feature-files/></features>
  <libraries>
    <library path="library.swf">
      <script name="com/example/BadgeClip" mod="1700000100">
        <def id="com.example:BadgeClip"/>
        <dep id="flash.display:MovieClip" type="i"/>
      </script>
    </library>
  </libraries>
  <files>
    <file path="assets/badge.png" mod="1700000100"/>
  </files>
</swc>"#;
        let swc_path = temp.join("badge_kit.swc");
        let swc_bytes = build_stored_zip(&[
            ("catalog.xml", catalog_xml),
            ("library.swf", &library_swf_bytes),
            ("assets/badge.png", &badge_png),
        ]);
        std::fs::write(&swc_path, swc_bytes).unwrap();
        swc_path
    }

    fn write_test_wav(temp: &std::path::Path) -> PathBuf {
        let wav_path = temp.join("click.wav");
        let pcm_samples: [i16; 8] = [0, 4096, 8192, 4096, 0, -4096, -8192, -4096];
        let data_len = (pcm_samples.len() * 2) as u32;
        let mut wav_bytes = Vec::new();
        wav_bytes.extend_from_slice(b"RIFF");
        wav_bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav_bytes.extend_from_slice(b"WAVEfmt ");
        wav_bytes.extend_from_slice(&16u32.to_le_bytes());
        wav_bytes.extend_from_slice(&1u16.to_le_bytes());
        wav_bytes.extend_from_slice(&1u16.to_le_bytes());
        wav_bytes.extend_from_slice(&44100u32.to_le_bytes());
        wav_bytes.extend_from_slice(&88200u32.to_le_bytes());
        wav_bytes.extend_from_slice(&2u16.to_le_bytes());
        wav_bytes.extend_from_slice(&16u16.to_le_bytes());
        wav_bytes.extend_from_slice(b"data");
        wav_bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in pcm_samples {
            wav_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&wav_path, wav_bytes).unwrap();
        wav_path
    }

    #[test]
    fn audio_files_with_matching_names_keep_distinct_resource_paths_and_sources() {
        let temp = std::env::temp_dir().join(new_id("neoflash-audio-collision"));
        let first = temp.join("first");
        let second = temp.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let first_wav = write_test_wav(&first);
        let second_wav = second.join("CLICK.WAV");
        std::fs::copy(&first_wav, &second_wav).unwrap();
        let paths = [first_wav, second_wav];
        let ctx = egui::Context::default();

        for attach_to_timeline in [true, false] {
            let original = Project::blank();
            let mut editor = Editor::from_project(&ctx, original.clone());
            editor.history.clear();
            if attach_to_timeline {
                editor
                    .import_media_paths(
                        &paths,
                        MediaPlacementMode::Timeline(TimelineDropPlacement {
                            frame: 3,
                            hovered_layer: Some(0),
                        }),
                    )
                    .unwrap();
            } else {
                editor.import_resource_paths(&paths).unwrap();
            }
            let resources = &editor.project.resources;
            assert_eq!(resources.len(), 2);
            assert_eq!(resources[0].path, "click.wav");
            assert_eq!(resources[1].path, "CLICK-2.WAV");
            for (resource, file) in resources.iter().zip(&paths) {
                assert_eq!(
                    editor.timeline_audio.source_paths.get(&resource.id),
                    Some(file)
                );
            }
            if attach_to_timeline {
                let mut sounds = editor
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .flat_map(|layer| &layer.keyframes)
                    .filter_map(|key| key.sound.as_ref().map(|sound| (key.frame, &sound.resource)));
                assert_eq!(sounds.next(), Some((0, &resources[0].id)));
                assert_eq!(sounds.next(), Some((0, &resources[1].id)));
                assert_eq!(sounds.next(), None);
            }
            editor.project.validate().unwrap();
            assert_eq!(editor.history.undo_len(), 1);
            editor.undo();
            assert_eq!(editor.project, original);
        }

        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn t311_swc_linking_and_audio_import_reachability_undo_and_ffmpeg_diagnostics() {
        assert!(crate::app::Editor::is_legacy_flash_path(
            std::path::Path::new("ui_kit.swc")
        ));
        assert!(crate::app::Editor::is_legacy_flash_path(
            std::path::Path::new("Hero.figma.json")
        ));

        let temp = std::env::temp_dir().join(format!(
            "neoflash-editor-t311-swc-audio-{}-{}",
            std::process::id(),
            new_id("test")
        ));
        std::fs::create_dir_all(&temp).unwrap();

        // 1. Build a genuine `.swc` library archive (`catalog.xml` + `library.swf` + `assets/badge.png`).
        let swc_path = write_test_swc(&temp);

        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.history.clear();
        let initial = editor.project.clone();

        editor
            .import_graphics_to_library_paths(std::slice::from_ref(&swc_path))
            .expect("link SWC into active project via Import to Library");
        assert_eq!(editor.undo_menu_label(), "Undo Link SWC");
        assert!(
            editor
                .project
                .sources
                .iter()
                .any(|s| s.path.ends_with("com/example/BadgeClip.as"))
        );
        assert!(
            editor
                .project
                .resources
                .iter()
                .any(|r| r.path.contains("badge.png") && r.media_type == "image/png")
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);

        // 2. Build a minimal 16-bit PCM mono WAV and test Stage + Library import + single Undo.
        let wav_path = write_test_wav(&temp);

        editor
            .import_graphics_paths(std::slice::from_ref(&wav_path), Some([120.0, 80.0]))
            .expect("import WAV to Stage attaches frame sound");
        assert_eq!(editor.undo_menu_label(), "Undo Import audio");
        assert!(
            editor
                .project
                .resources
                .iter()
                .any(|r| r.path == "click.wav" && r.media_type == "audio/wav")
        );
        assert!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .sound
                .is_some(),
            "importing audio to Stage on an editable layer must attach FrameSound"
        );
        editor.project.validate().unwrap();
        editor.undo();
        assert_eq!(editor.project, initial);

        editor
            .import_graphics_to_library_paths(std::slice::from_ref(&wav_path))
            .expect("import WAV to Library");
        assert!(
            editor
                .project
                .resources
                .iter()
                .any(|r| r.path == "click.wav" && r.media_type == "audio/wav")
        );
        assert!(
            editor.symbol().timeline.layers[0].keyframes[0]
                .sound
                .is_none(),
            "importing audio to Library must not attach FrameSound to Stage"
        );
        editor.undo();
        assert_eq!(editor.project, initial);

        // 3. `.ogg` without FFmpeg returns an actionable diagnostic and NEVER `application/octet-stream`.
        let ogg_path = temp.join("music.ogg");
        std::fs::write(&ogg_path, b"OggS\x00\x02dummy").unwrap();
        let no_ffmpeg_err = crate::resources::read_audio_resource_with_ffmpeg(
            &ogg_path,
            neoflash_core::MAX_RESOURCE_BYTES,
            None,
        )
        .expect_err("missing FFmpeg must fail cleanly on .ogg");
        assert!(
            no_ffmpeg_err.contains("FFmpeg"),
            "expected actionable FFmpeg message, got: {no_ffmpeg_err}"
        );

        // If FFmpeg is available locally, verify real `.flac` -> 16-bit PCM WAV transcode.
        if let Some(ffmpeg) = neoflash_export::detect_ffmpeg() {
            let flac_path = temp.join("tone.flac");
            let mut cmd = std::process::Command::new(&ffmpeg);
            neoflash_export::configure_media_subprocess_command(&mut cmd, &ffmpeg);
            let status = cmd
                .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(&wav_path)
                .arg(&flac_path)
                .status();
            if status.is_ok_and(|s| s.success()) && flac_path.is_file() {
                editor
                    .import_graphics_to_library_paths(std::slice::from_ref(&flac_path))
                    .expect("transcode .flac to 16-bit PCM WAV via FFmpeg");
                let unit = editor
                    .project
                    .resources
                    .iter()
                    .find(|r| r.path == "tone.wav")
                    .expect("transcoded tone.wav resource");
                assert_eq!(unit.media_type, "audio/wav");
                let decoded = unit.decoded_bytes().unwrap();
                assert!(decoded.starts_with(b"RIFF") && &decoded[8..12] == b"WAVE");
                editor.undo();
            }
        }

        let _ = std::fs::remove_dir_all(&temp);
    }
}
