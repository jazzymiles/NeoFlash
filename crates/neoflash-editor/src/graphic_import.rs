//! One atomic graphic import shared by the File menu and native file drops.
use crate::{
    app::{Editor, reaches},
    selection::{Selection, SelectionTarget},
    stage_mapping::StageMapping,
};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use std::{collections::HashMap, path::PathBuf};

fn is_supported_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "m4v" | "mov" | "webm" | "ogv" | "avi"
            )
        })
}

fn is_known_unsupported_video_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "flv" | "mkv" | "wmv"
            )
        })
}

pub(crate) struct PreparedGraphic {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub elements: Vec<Element>,
    pub resources: Vec<ResourceUnit>,
    pub notice: Option<String>,
}

struct PreparedImportGraphic {
    pub graphic: PreparedGraphic,
    pub timeline_override: Option<Timeline>,
    pub symbol_kind: SymbolKind,
    pub playback: InstancePlayback,
    pub helper_symbols: Vec<Symbol>,
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
    "json", "swf", "pdf", "ai", "fxg", "act", "clr", "ase",
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
            .add_filter("Graphics & Interchange", IMPORT_GRAPHICS_EXTENSIONS)
            .pick_files()
            && let Err(error) = self.import_graphics_paths(&paths, None)
        {
            self.message(format!("Graphic import failed: {error}"));
        }
    }

    pub(crate) fn import_to_library_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Graphics & Interchange", IMPORT_GRAPHICS_EXTENSIONS)
            .pick_files()
            && let Err(error) = self.import_graphics_to_library_paths(&paths)
        {
            self.message(format!("Library import failed: {error}"));
        }
    }

    pub(crate) fn stage_file_drop(
        &mut self,
        ui: &egui::Ui,
        area: Rect,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        let ctx = ui.ctx();
        let (hovering, mut pointer, dropped) = ctx.input(|input| {
            (
                !input.raw.hovered_files.is_empty(),
                input.pointer.latest_pos(),
                input.raw.dropped_files.clone(),
            )
        });
        // During a native file drag, winit does not always emit CursorMoved.
        // Its last egui position may still be over the button that opened Finder.
        if (hovering || !dropped.is_empty())
            && let Some((global, _)) = crate::native_screens::pointer_state()
            && let Some(inner) = ctx.input(|input| input.viewport().inner_rect)
        {
            pointer = Some(global / ctx.zoom_factor() - inner.min.to_vec2());
        }
        if hovering {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            if let Some(position) = pointer {
                self.graphic_drop_position = Some(position);
            }
            let hot = self.graphic_drop_position.is_some_and(|p| area.contains(p));
            let color = ui.visuals().selection.bg_fill;
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("graphic-file-drop"),
            ));
            painter.rect_filled(
                area.shrink(4.),
                4.,
                color.gamma_multiply(if hot { 0.3 } else { 0.12 }),
            );
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Drop SVG, PNG or JPEG onto the Stage",
                egui::FontId::proportional(crate::theme::PANEL_FONT),
                ui.visuals().text_color(),
            );
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
        if !area.contains(position) {
            self.message("Drop graphics onto the Stage, or use File → Import graphics to Stage.");
            return;
        }
        let paths: Option<Vec<_>> = dropped.iter().map(|file| file.path.clone()).collect();
        let Some(paths) = paths else {
            self.message("This drop has no local file path. Save the image first, then drag that file onto the Stage.");
            return;
        };
        let point = mapped_drop_point(mapping, origin, self.zoom, position);
        let videos: Vec<_> = paths
            .iter()
            .filter(|path| is_supported_video_path(path))
            .collect();
        if paths
            .iter()
            .any(|path| is_known_unsupported_video_path(path))
        {
            self.message(
                "Video import supports H.264 MP4/M4V/MOV, VP9 WebM, Ogg and AVI video. Transcode this file, then drop it again.",
            );
            return;
        }
        if !videos.is_empty() {
            if videos.len() != 1 || paths.len() != 1 {
                self.message("Drop one video at a time; import graphics in a separate drop.");
                return;
            }
            if let Err(error) = self.import_video_path_at(ctx, videos[0], point) {
                self.message(format!("Video import failed: {error}"));
            }
            return;
        }
        if let Err(error) = self.import_graphics_paths(&paths, Some(point)) {
            self.message(format!("Graphic import failed: {error}"));
        }
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
        let hit = area.contains(position) && stage_window_is_front(ctx, position);
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
            if !(alt
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
            crate::library::LibraryDrag::Resource(id) => self.place_graphic_resource(&id, point),
            crate::library::LibraryDrag::Folder(_) => Ok(()),
        };
        if let Err(error) = result {
            self.message(format!("Cannot place on Stage: {error}"));
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
        self.import_graphics_paths_internal(paths, point, true)
    }

    pub(crate) fn import_graphics_to_library_paths(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<(), String> {
        self.import_graphics_paths_internal(paths, None, false)
    }

    fn import_graphics_paths_internal(
        &mut self,
        paths: &[PathBuf],
        point: Option<[f64; 2]>,
        place_on_stage: bool,
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        if paths.len() > 64 {
            return Err("Import at most 64 graphics at once.".into());
        }

        // Separate palette files from graphic/interchange files, validating all upfront.
        let mut palette_paths = Vec::new();
        let mut graphic_paths = Vec::new();
        for path in paths {
            let ext = path_ext_lower(path);
            if is_palette_ext(&ext) {
                palette_paths.push(path.clone());
            } else {
                graphic_paths.push(path.clone());
            }
        }

        let mut parsed_swatches = Vec::new();
        for pal_path in &palette_paths {
            let display = pal_path.display().to_string();
            let bytes = std::fs::read(pal_path).map_err(|e| format!("{display}: {e}"))?;
            let mut swatches = Self::parse_palette_file_bytes(&bytes, &display)?;
            parsed_swatches.append(&mut swatches);
        }

        if graphic_paths.is_empty() {
            let count = parsed_swatches.len();
            self.finish_edits();
            self.transact_named("Import colour palette", |editor| {
                Self::merge_swatches_into_project(&mut editor.project, &parsed_swatches);
            });
            self.message(format!(
                "Imported {count} swatch(es) from {} palette file(s).",
                palette_paths.len()
            ));
            return Ok(());
        }

        let mut remaining = MAX_RESOURCE_TOTAL_BYTES;
        let mut items = Vec::with_capacity(graphic_paths.len());
        let sequence_request =
            graphic_paths.len() >= 2 || (graphic_paths.len() == 1 && graphic_paths[0].is_dir());
        let seq_opts = self.workspace.sequence_import_options.clone();
        let imported_sequence = if sequence_request {
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
                return self.place_sequence_keyframes_on_active_layer(
                    imported_sequence,
                    parsed_swatches,
                    point,
                    &mut remaining,
                );
            }
            let seq_item =
                prepare_sequence_graphic(imported_sequence, seq_opts.placement, &mut remaining)?;
            items.push(seq_item);
        } else {
            for path in &graphic_paths {
                let ext = path_ext_lower(path);
                let item = if is_figma_graphic_path(path, &ext) {
                    prepare_figma_graphic(path, &mut remaining)?
                } else {
                    match ext.as_str() {
                        "psd" | "psb" => prepare_psd_graphic(path, &mut remaining)?,
                        "swf" => prepare_swf_graphic(path, &mut remaining)?,
                        "pdf" | "ai" => prepare_pdf_ai_graphic(path)?,
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
        }

        self.place_prepared_import_graphics(items, parsed_swatches, point, place_on_stage)
    }

    pub(crate) fn place_graphic_resource(
        &mut self,
        resource_id: &str,
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        let unit = self
            .project
            .resources
            .iter()
            .find(|unit| unit.id == resource_id)
            .ok_or("This image resource no longer exists.")?
            .clone();
        let name = std::path::Path::new(&unit.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Graphic")
            .to_owned();
        let graphic = prepare_graphic(unit, name)?;
        self.place_prepared_graphics(vec![graphic], point)
    }

    fn place_prepared_graphics(
        &mut self,
        graphics: Vec<PreparedGraphic>,
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        let items = graphics
            .into_iter()
            .map(PreparedImportGraphic::from)
            .collect();
        self.place_prepared_import_graphics(items, Vec::new(), point, true)
    }

    fn place_prepared_import_graphics(
        &mut self,
        items: Vec<PreparedImportGraphic>,
        swatches: Vec<GlobalSwatch>,
        point: Option<[f64; 2]>,
        place_on_stage: bool,
    ) -> Result<(), String> {
        self.finish_edits();
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        if !swatches.is_empty() {
            Self::merge_swatches_into_project(&mut candidate, &swatches);
        }
        let folder = self.library_panel.selected_folder.clone();
        candidate.add_library_folder(&folder)?;
        let mut placed = Vec::with_capacity(items.len());
        let mut notices = Vec::new();
        let mut last_symbol = None;
        let mut max_imported_frames = 1u32;
        let exact_position = point.is_some() || self.editing_in_place();
        let at = point.unwrap_or_else(|| self.paste_center());
        for (index, mut item) in items.into_iter().enumerate() {
            merge_import_graphic_resources(&mut candidate, &mut item)?;
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
            // A native/Library drop is an explicit authored coordinate. Pasteboard artwork is
            // valid, so never move it merely to keep the complete bounds inside the page.
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
            if place_on_stage {
                placed.push(Element {
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
                });
            }
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
                        tween: None,
                        label: String::new(),
                        ..Default::default()
                    }],
                }],
                scripts: vec![],
            });
            max_imported_frames = max_imported_frames.max(timeline.frame_count);
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
            candidate.upgrade_format_for_features();
            candidate.validate().map_err(|e| e.to_string())?;
            self.transact_named("Import to Library", |editor| {
                editor.project = candidate;
                editor.library_selected = last_symbol;
            });
            let suffix = if notices.is_empty() {
                String::new()
            } else {
                format!(" {}", notices.join(" "))
            };
            self.message(format!("Imported symbol(s) into Library.{suffix}"));
            return Ok(());
        }
        self.commit_graphic_placements_with_span(
            candidate,
            placed,
            last_symbol,
            notices,
            max_imported_frames,
        )
    }

    pub(crate) fn place_library_symbol(
        &mut self,
        ctx: &egui::Context,
        symbol: &str,
        point: Option<[f64; 2]>,
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
        let painter = ctx.layer_painter(egui::LayerId::background());
        let bounds = evaluate_scene(&self.project, symbol, 0, false, true)
            .draws
            .iter()
            .fold(Rect::NOTHING, |rect, draw| {
                rect.union(crate::stage::world_bounds(&draw.item, &painter))
            });
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
            name: source.name.clone(),
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
        self.commit_graphic_placements(
            self.project.clone(),
            vec![element],
            Some(symbol.into()),
            vec![],
        )
    }

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
        let count = placed.len();
        self.set_playing(false);
        let symbol_id = self.symbol_id().to_owned();
        let fps = f64::from(candidate.fps);
        let timeline = &mut candidate
            .symbol_mut(&symbol_id)
            .ok_or("The current symbol was removed.")?
            .timeline;
        // Import must remain visible even when a folder, mask, guide, hidden or
        // locked layer is selected. Add a normal top layer without altering it.
        let use_current = timeline.layers.get(self.layer).is_some_and(|layer| {
            layer.properties.role == LayerRole::Normal
                && layer.properties.mask.is_none()
                && layer_effective_outline_color(timeline, layer).is_none()
                && layer_effectively_visible(timeline, layer)
                && !layer_effectively_locked(timeline, layer)
        });
        let target_layer = if use_current {
            self.layer
        } else {
            timeline.layers.push(Layer {
                id: new_id("layer"),
                name: "Imported graphics".into(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: vec![],
            });
            timeline.layers.len() - 1
        };
        let needed_end = self.frame.saturating_add(min_frame_span.max(1));
        timeline.frame_count = timeline.frame_count.max(needed_end);
        let layer = &mut timeline.layers[target_layer];
        if let Some(end) = &mut layer.properties.end_frame
            && *end < needed_end
        {
            *end = needed_end;
        }
        let layer_id = layer.id.clone();
        if !layer.keyframes.iter().any(|key| key.frame == self.frame) {
            let mut key = layer.key_at(self.frame).cloned().unwrap_or(Keyframe {
                frame: self.frame,
                elements: vec![],
                tween: None,
                label: String::new(),
                ..Default::default()
            });
            key.elements = sample_layer_at_fps(layer, self.frame, fps);
            key.frame = self.frame;
            key.label.clear();
            layer.keyframes.push(key);
            layer.keyframes.sort_by_key(|key| key.frame);
        }
        let targets = placed
            .iter()
            .map(|element| SelectionTarget {
                layer: layer_id.clone(),
                element: element.id.clone(),
            })
            .collect();
        layer
            .key_at_mut(self.frame)
            .ok_or("Could not create an import keyframe.")?
            .elements
            .extend(placed);
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact(|editor| {
            editor.project = candidate;
            editor.layer = target_layer;
            editor.selection = Selection {
                symbol: symbol_id,
                targets,
            };
            editor.library_selected = last_symbol;
        });
        let suffix = if notices.is_empty() {
            String::new()
        } else {
            format!(" {}", notices.join(" "))
        };
        let target_note = if use_current {
            ""
        } else {
            " Created a visible Imported graphics layer."
        };
        self.message(format!(
            "Placed {count} graphic(s) on Stage.{target_note}{suffix}"
        ));
        Ok(())
    }

    fn place_sequence_keyframes_on_active_layer(
        &mut self,
        imported: neoflash_import::ImportedNumberedImageSequence,
        swatches: Vec<GlobalSwatch>,
        point: Option<[f64; 2]>,
        remaining: &mut usize,
    ) -> Result<(), String> {
        let mut seq_item = prepare_sequence_graphic(
            imported,
            neoflash_import::ImageSequencePlacement::KeyframesOnSingleLayer,
            remaining,
        )?;
        self.finish_edits();
        self.set_playing(false);
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        if !swatches.is_empty() {
            Self::merge_swatches_into_project(&mut candidate, &swatches);
        }
        merge_import_graphic_resources(&mut candidate, &mut seq_item)?;

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
        let timeline = &mut candidate
            .symbol_mut(&symbol_id)
            .ok_or("The current symbol was removed.")?
            .timeline;
        let use_current = timeline.layers.get(self.layer).is_some_and(|layer| {
            layer.properties.role == LayerRole::Normal
                && layer.properties.mask.is_none()
                && layer_effective_outline_color(timeline, layer).is_none()
                && layer_effectively_visible(timeline, layer)
                && !layer_effectively_locked(timeline, layer)
        });
        let target_layer = if use_current {
            self.layer
        } else {
            timeline.layers.push(Layer {
                id: new_id("layer"),
                name: seq_item.graphic.name.clone(),
                visible: true,
                locked: false,
                properties: Default::default(),
                keyframes: vec![],
            });
            timeline.layers.len() - 1
        };
        let base_frame = self.frame;
        let needed_end = base_frame.saturating_add(seq_timeline.frame_count.max(1));
        timeline.frame_count = timeline.frame_count.max(needed_end);
        let layer = &mut timeline.layers[target_layer];
        if let Some(end) = &mut layer.properties.end_frame
            && *end < needed_end
        {
            *end = needed_end;
        }
        let layer_id = layer.id.clone();
        let mut first_targets = Vec::new();
        let keyframe_count = seq_layer.keyframes.len();
        for (idx, mut kf) in seq_layer.keyframes.into_iter().enumerate() {
            let dest_frame = base_frame.saturating_add(kf.frame);
            kf.frame = dest_frame;
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
                existing.elements.extend(kf.elements);
            } else {
                layer.keyframes.push(kf);
            }
        }
        layer.keyframes.sort_by_key(|k| k.frame);
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        let notice = seq_item.graphic.notice.take();
        self.transact_named("Import Image Sequence", |editor| {
            editor.project = candidate;
            editor.layer = target_layer;
            editor.selection = Selection {
                symbol: symbol_id,
                targets: first_targets,
            };
        });
        let suffix = notice.map(|n| format!(" {n}")).unwrap_or_default();
        self.message(format!(
            "Placed {keyframe_count} sequence keyframe(s) on timeline.{suffix}"
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
    let meta = std::fs::metadata(path).map_err(|e| format!("{display}: {e}"))?;
    let len = usize::try_from(meta.len()).unwrap_or(usize::MAX);
    if len > *remaining {
        return Err(format!(
            "{display}: file size exceeds remaining import budget."
        ));
    }
    *remaining = remaining.saturating_sub(len);
    let (mut imported, report) =
        neoflash_import::import_psd_path(path, &neoflash_import::PsdImportOptions::default())
            .map_err(|e| format!("{display} [psd-import]: {e}"))?;
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
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::MovieClip,
        playback: InstancePlayback::SingleFrame(0),
        helper_symbols,
    })
}

fn prepare_pdf_ai_graphic(path: &std::path::Path) -> Result<PreparedImportGraphic, String> {
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
    let mut sym_map = std::collections::BTreeMap::new();
    for sym in &imported.symbols {
        sym_map.insert(sym.id.clone(), new_id(&format!("{prefix}-sym")));
    }
    if let Some(next_root) = sym_map.get(&imported.root).cloned() {
        imported.root = next_root;
    }
    for scene in &mut imported.scenes {
        if let Some(next_scene) = sym_map.get(scene) {
            *scene = next_scene.clone();
        }
    }
    for sym in &mut imported.symbols {
        if let Some(next_id) = sym_map.get(&sym.id) {
            sym.id = next_id.clone();
        }
        sym.timeline.remap_internal_ids(
            |kind| new_id(&format!("{prefix}-{kind}")),
            Some(&sym_map),
            None,
        );
    }
}

fn prepare_figma_graphic(
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
    let (mut imported, report) =
        neoflash_import::import_figma_path(path, &neoflash_import::FigmaImportOptions::default())
            .map_err(|e| format!("{display} [figma-import]: {e}"))?;
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
            resources: std::mem::take(&mut imported.resources),
            notice: format_import_diagnostics(&report),
        },
        timeline_override: Some(root_symbol.timeline),
        symbol_kind: SymbolKind::MovieClip,
        playback: InstancePlayback::SingleFrame(0),
        helper_symbols,
    })
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

fn stage_window_is_front(ctx: &egui::Context, local: Pos2) -> bool {
    let Some(inner) = ctx.input(|input| input.viewport().inner_rect) else {
        return true;
    };
    let root = inner * ctx.zoom_factor();
    let global = (local + inner.min.to_vec2()) * ctx.zoom_factor();
    match crate::native_screens::front_dock_window_at(global, None, &[], false) {
        None => true,
        Some(None) => false,
        Some(Some(crate::native_screens::NativeDockWindow::Main(_))) => true,
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
        out.extend_from_slice(&0u32.to_be_bytes());

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
}
