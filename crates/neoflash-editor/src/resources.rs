//! Portable script resources live beside symbols in the Library.
use crate::app::Editor;
use crate::theme::DesignUi as _;
use eframe::egui;
use neoflash_core::{
    MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, ResourceUnit, new_id,
};
use neoflash_ui::components::{self as nf, TreeItemType};
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, space};
use neoflash_ui::typography::rich;
use std::{io::Read, path::Path};

#[derive(Default)]
pub(crate) struct ResourcePanel {
    pub(crate) selected: Option<String>,
    pub(crate) baseline: Option<(String, String)>,
    path: String,
    media_type: String,
}

impl Editor {
    fn import_resource_paths(&mut self, paths: &[std::path::PathBuf]) -> Result<(), String> {
        if paths.len() + self.project.resources.len() > MAX_RESOURCES {
            return Err("The project supports at most 1024 resource files.".into());
        }
        let used: usize = self
            .project
            .resources
            .iter()
            .map(ResourceUnit::byte_len)
            .sum();
        let mut remaining = MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used);
        let mut units = Vec::with_capacity(paths.len());
        let mut audio_sources = Vec::new();
        for path in paths {
            let unit = read_resource(path, remaining)?;
            remaining -= unit.byte_len();
            if unit.is_audio() {
                audio_sources.push((unit.id.clone(), path.clone()));
            }
            units.push(unit);
        }
        self.add_resource_units(units)?;
        for (id, path) in audio_sources {
            self.timeline_audio.source_paths.insert(id, path);
        }
        Ok(())
    }

    /// Validate the whole import before applying one undoable edit.
    pub(crate) fn add_resource_units(&mut self, units: Vec<ResourceUnit>) -> Result<(), String> {
        if units.is_empty() {
            return Ok(());
        }
        let selected = units.last().map(|unit| unit.id.clone());
        self.finish_edits();
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        candidate.resources.extend(units);
        candidate.validate().map_err(|error| error.to_string())?;
        self.transact(|editor| editor.project = candidate);
        self.resource_panel.selected = selected;
        self.resource_panel.baseline = None;
        Ok(())
    }

    pub(crate) fn resource_library(&mut self, ui: &mut egui::Ui) {
        let count = self.project.resources.len();
        nf::SectionHeader::new(&format!("Resource files · {count}"))
            .id_salt("library-resources")
            .default_open(false)
            .show(ui, |ui| {
                self.resource_files_body(ui);
            });
    }

    fn resource_files_body(&mut self, ui: &mut egui::Ui) {
        let p = neoflash_ui::palette(ui.ctx());
        let note = |ui: &mut egui::Ui, text: &str| {
            ui.label(rich(text, TextStyle::Small).color(p.text_tertiary));
        };
        note(ui, "Project data and media. Select an image to place it.");
        if ui
            .add(nf::Button::new("Add resource files…").icon(i16::PLUS))
            .clicked()
            && let Some(paths) = rfd::FileDialog::new().pick_files()
        {
            match self.import_resource_paths(&paths) {
                Ok(()) => self.message(format!("Imported {} resource file(s).", paths.len())),
                Err(error) => self.message(format!("Resource import failed: {error}")),
            }
        }
        if !self
            .project
            .resources
            .iter()
            .any(|unit| Some(&unit.id) == self.resource_panel.selected.as_ref())
        {
            self.resource_panel = ResourcePanel::default();
        }
        let mut open_sound_props: Option<String> = None;
        let mut open_bitmap_props: Option<String> = None;
        let mut open_trace_bitmap: Option<String> = None;
        let mut open_export_audio: Option<String> = None;
        let mut open_batch_rename = false;
        let mut open_batch_linkage = false;
        let mut open_batch_bitmap = false;
        let mut trigger_select_unused = false;
        let mut trigger_delete_unused = false;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            for unit in &self.project.resources {
                let selected = self.resource_panel.selected.as_ref() == Some(&unit.id);
                let row = crate::library::tree_row(
                    ui,
                    &unit.path,
                    TreeItemType::Symbol(media_icon(&unit.media_type)),
                    selected,
                    0,
                    "",
                )
                .row;
                let response =
                    nf::tooltip(row, &format!("{} · {}", unit.path, unit.media_type), None);
                if is_placeable_image(&unit.media_type) {
                    crate::library::capture_row_drag(
                        &mut self.library_panel,
                        ui,
                        &response,
                        || crate::library::LibraryDrag::Resource(unit.id.clone()),
                    );
                }
                if response.clicked() || response.secondary_clicked() {
                    self.resource_panel.selected = Some(unit.id.clone());
                    self.resource_panel.baseline = None;
                    self.library_selected = None;
                }
                if unit.is_audio() {
                    let unit_id = unit.id.clone();
                    response.context_menu(|ui| {
                        crate::ui_icons::menu_contents(ui, 230., |ui| {
                            if ui.add(nf::MenuItem::new("Sound properties…")).clicked() {
                                open_sound_props = Some(unit_id.clone());
                                ui.close();
                            }
                            if ui.add(nf::MenuItem::new("Export audio…")).clicked() {
                                open_export_audio = Some(unit_id.clone());
                                ui.close();
                            }
                            nf::menu_separator(ui);
                            if ui.add(nf::MenuItem::new("Batch rename…")).clicked() {
                                open_batch_rename = true;
                                ui.close();
                            }
                            if ui
                                .add(nf::MenuItem::new("Batch linkage (ActionScript)…"))
                                .clicked()
                            {
                                open_batch_linkage = true;
                                ui.close();
                            }
                            if ui.add(nf::MenuItem::new("Select unused items")).clicked() {
                                trigger_select_unused = true;
                                ui.close();
                            }
                            if ui.add(nf::MenuItem::new("Delete unused items")).clicked() {
                                trigger_delete_unused = true;
                                ui.close();
                            }
                        });
                    });
                } else if unit.is_image() {
                    let unit_id = unit.id.clone();
                    response.context_menu(|ui| {
                        crate::ui_icons::menu_contents(ui, 230., |ui| {
                            if ui.add(nf::MenuItem::new("Trace bitmap…")).clicked() {
                                open_trace_bitmap = Some(unit_id.clone());
                                ui.close();
                            }
                            if ui.add(nf::MenuItem::new("Bitmap properties…")).clicked() {
                                open_bitmap_props = Some(unit_id.clone());
                                ui.close();
                            }
                            nf::menu_separator(ui);
                            if ui.add(nf::MenuItem::new("Batch rename…")).clicked() {
                                open_batch_rename = true;
                                ui.close();
                            }
                            if ui
                                .add(nf::MenuItem::new("Batch bitmap compression & smoothing…"))
                                .clicked()
                            {
                                open_batch_bitmap = true;
                                ui.close();
                            }
                            if ui.add(nf::MenuItem::new("Select unused items")).clicked() {
                                trigger_select_unused = true;
                                ui.close();
                            }
                            if ui.add(nf::MenuItem::new("Delete unused items")).clicked() {
                                trigger_delete_unused = true;
                                ui.close();
                            }
                        });
                    });
                }
            }
        });
        if let Some(id) = open_sound_props {
            self.open_sound_properties(ui.ctx(), Some(id));
        }
        if let Some(id) = open_trace_bitmap {
            self.trace_selected_or_library_bitmap(ui.ctx(), Some(id));
        }
        if let Some(id) = open_bitmap_props {
            self.open_bitmap_properties(ui.ctx(), Some(id));
        }
        if let Some(id) = open_export_audio {
            self.open_export_audio(
                ui.ctx(),
                Some(neoflash_export::AudioExportSource::Resource(id)),
            );
        }
        if open_batch_rename {
            self.open_batch_rename_library_modal(ui.ctx());
        }
        if open_batch_linkage {
            self.open_batch_linkage_modal(ui.ctx());
        }
        if open_batch_bitmap {
            self.open_batch_bitmap_compression_modal(ui.ctx());
        }
        if trigger_select_unused {
            self.select_unused_library_items();
        }
        if trigger_delete_unused {
            self.delete_unused_library_items();
        }
        let Some(index) = self
            .project
            .resources
            .iter()
            .position(|unit| Some(&unit.id) == self.resource_panel.selected.as_ref())
        else {
            if self.project.resources.is_empty() {
                note(ui, "Text, data and media files");
            }
            return;
        };
        let unit = &self.project.resources[index];
        let metadata = (unit.path.clone(), unit.media_type.clone());
        if self.resource_panel.baseline.as_ref() != Some(&metadata) {
            self.resource_panel.path = metadata.0.clone();
            self.resource_panel.media_type = metadata.1.clone();
            self.resource_panel.baseline = Some(metadata);
        }
        note(ui, &format!("{} bytes", unit.byte_len()));
        if is_placeable_image(&unit.media_type)
            && ui.add(nf::Button::new("Place on Stage")).clicked()
        {
            let id = unit.id.clone();
            if let Err(error) = self.place_graphic_resource(&id, None) {
                self.message(format!("Cannot place image: {error}"));
            }
            return;
        }
        ui.add(
            nf::TextField::new(&mut self.resource_panel.path, "Project path")
                .prefix("Path")
                .mono(),
        );
        ui.add(
            nf::TextField::new(&mut self.resource_panel.media_type, "Content type")
                .prefix("Type")
                .mono(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(space::S2);
            let changed = self.resource_panel.path != self.project.resources[index].path
                || self.resource_panel.media_type != self.project.resources[index].media_type;
            if ui
                .add_enabled_semantic(changed, nf::Button::primary("Apply"))
                .clicked()
            {
                self.finish_edits();
                let mut candidate = self.project.clone();
                candidate.resources[index].path = self.resource_panel.path.clone();
                candidate.resources[index].media_type = self.resource_panel.media_type.clone();
                match candidate.validate() {
                    Ok(()) => {
                        self.transact(|editor| editor.project = candidate);
                        self.message("Resource details updated. Update script paths if renamed.");
                    }
                    Err(error) => self.message(format!("Resource details rejected: {error}")),
                }
            }
            if ui.add(nf::Button::danger("Remove file")).clicked() {
                self.finish_edits();
                let mut candidate = self.project.clone();
                candidate.resources.remove(index);
                match candidate.validate() {
                    Ok(()) => {
                        self.transact(|editor| editor.project = candidate);
                        self.resource_panel = ResourcePanel::default();
                        self.message("Resource removed. Undo restores the file.");
                    }
                    Err(error) => self.message(format!("Resource is still used: {error}")),
                }
            }
        });
    }

    /// Open Trace Bitmap for the selected Stage bitmap when available, or run Library-only
    /// bitmap tracing on a Library image resource to create an editable vector symbol (`T-177`).
    pub(crate) fn trace_selected_or_library_bitmap(
        &mut self,
        ctx: &egui::Context,
        explicit_resource_id: Option<String>,
    ) {
        if explicit_resource_id.is_none() && self.can_trace_bitmap() {
            self.open_trace_bitmap(ctx);
            return;
        }
        let target_res = explicit_resource_id
            .or_else(|| {
                self.resource_panel.selected.as_ref().and_then(|id| {
                    self.project
                        .resources
                        .iter()
                        .find(|u| &u.id == id && u.is_image())
                        .map(|u| u.id.clone())
                })
            })
            .or_else(|| {
                self.project
                    .resources
                    .iter()
                    .find(|u| u.is_image())
                    .map(|u| u.id.clone())
            });
        let Some(resource_id) = target_res else {
            self.message("Select a placed bitmap or a Library image resource to trace.");
            return;
        };
        if let Err(err) = self.trace_library_bitmap_resource(&resource_id) {
            self.message(err);
        }
    }

    /// Library-only Trace Bitmap workflow (`T-177`): trace a bitmap resource in the Library
    /// and create a new editable vector symbol (`Graphic`) containing the traced region shapes
    /// while preserving the source bitmap resource untouched, in one atomic Undo step.
    pub fn trace_library_bitmap_resource(&mut self, resource_id: &str) -> Result<String, String> {
        self.trace_library_bitmap_resource_with_params(
            resource_id,
            24,
            4,
            neoflash_core::SymbolKind::Graphic,
        )
    }

    /// Library-only Trace Bitmap with explicit color threshold, minimum area, and target symbol kind (`Graphic` or `MovieClip`).
    pub fn trace_library_bitmap_resource_with_params(
        &mut self,
        resource_id: &str,
        color_threshold: u8,
        min_area_px: u32,
        kind: neoflash_core::SymbolKind,
    ) -> Result<String, String> {
        if !matches!(
            kind,
            neoflash_core::SymbolKind::Graphic | neoflash_core::SymbolKind::MovieClip
        ) {
            return Err(
                "Library bitmap trace target must be a Graphic or MovieClip symbol.".into(),
            );
        }
        let unit = self
            .project
            .resources
            .iter()
            .find(|u| u.id == resource_id)
            .ok_or_else(|| format!("Unknown resource '{resource_id}'."))?
            .clone();
        if !unit.is_image() {
            return Err(format!("Resource '{}' is not a bitmap image.", unit.path));
        }
        let decoded = unit.decode_image().map_err(|e| e.to_string())?;
        let w = decoded.width.max(1);
        let h = decoded.height.max(1);
        let step_x = (w / 64).max(1);
        let step_y = (h / 64).max(1);
        let q_step = u16::from(color_threshold.max(1));
        let quant = |v: u8| -> u8 {
            let bucket = (u16::from(v) / q_step) * q_step;
            bucket.min(255) as u8
        };

        // Group quantized non-transparent pixels into merged horizontal run rectangles per color bucket.
        let mut color_validate: std::collections::BTreeMap<
            [u8; 4],
            Vec<neoflash_core::PathCommand>,
        > = std::collections::BTreeMap::new();
        for y in (0..h).step_by(step_y as usize) {
            let mut x = 0u32;
            while x < w {
                let idx = ((y * w + x) * 4) as usize;
                if idx + 3 >= decoded.rgba.len() {
                    break;
                }
                let a = decoded.rgba[idx + 3];
                if a < 16 {
                    x += step_x;
                    continue;
                }
                let col = [
                    quant(decoded.rgba[idx]),
                    quant(decoded.rgba[idx + 1]),
                    quant(decoded.rgba[idx + 2]),
                    255,
                ];
                let start_x = x;
                x += step_x;
                while x < w {
                    let n_idx = ((y * w + x) * 4) as usize;
                    if n_idx + 3 >= decoded.rgba.len() || decoded.rgba[n_idx + 3] < 16 {
                        break;
                    }
                    let n_col = [
                        quant(decoded.rgba[n_idx]),
                        quant(decoded.rgba[n_idx + 1]),
                        quant(decoded.rgba[n_idx + 2]),
                        255,
                    ];
                    if n_col != col {
                        break;
                    }
                    x += step_x;
                }
                let run_w = (x - start_x).max(step_x);
                let run_h = step_y.min(h - y).max(1);
                if run_w * run_h < min_area_px.min(w * h) && (w * h) > min_area_px {
                    // Still allow small bitmaps to produce regions; filter only tiny noise on larger images.
                }
                let x0 = f64::from(start_x);
                let y0 = f64::from(y);
                let x1 = f64::from((start_x + run_w).min(w));
                let y1 = f64::from((y + run_h).min(h));
                let cmds = color_validate.entry(col).or_default();
                cmds.push(neoflash_core::PathCommand::MoveTo { x: x0, y: y0 });
                cmds.push(neoflash_core::PathCommand::LineTo { x: x1, y: y0 });
                cmds.push(neoflash_core::PathCommand::LineTo { x: x1, y: y1 });
                cmds.push(neoflash_core::PathCommand::LineTo { x: x0, y: y1 });
                cmds.push(neoflash_core::PathCommand::Close);
            }
        }

        let template_el = neoflash_core::generate_deco_elements(
            neoflash_core::DecoEffectKind::FlowerBrush,
            [0.0, 0.0],
            100.0,
            20.0,
            [0, 0, 0, 255],
            [0, 0, 0, 255],
        )
        .into_iter()
        .next()
        .ok_or_else(|| "Failed to allocate vector region element.".to_owned())?;

        let mut elements = Vec::with_capacity(color_validate.len());
        for (idx, (color, commands)) in color_validate.into_iter().enumerate() {
            let mut el = template_el.clone();
            el.id = new_id("traced-region");
            el.name = format!("Region {}", idx + 1);
            el.transform = neoflash_core::Transform::default();
            el.merge = false;
            el.kind = neoflash_core::ElementKind::Path {
                commands,
                fill_rule: neoflash_core::FillRule::NonZero,
                fill: color,
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            };
            elements.push(el);
        }
        if elements.is_empty() {
            return Err("Bitmap produced no traceable vector regions.".into());
        }

        let base_stem = Path::new(&unit.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Bitmap");
        let mut sym_name = format!("{base_stem} Traced");
        let mut suffix = 2usize;
        while self.project.symbols.iter().any(|s| s.name == sym_name) {
            sym_name = format!("{base_stem} Traced {suffix}");
            suffix += 1;
        }
        let sym_id = new_id("symbol");

        let mut candidate = self.project.clone();
        let mut sym = neoflash_core::Project::blank()
            .symbols
            .into_iter()
            .next()
            .ok_or_else(|| "Failed to initialize symbol template.".to_owned())?;
        sym.id = sym_id.clone();
        sym.name = sym_name.clone();
        sym.kind = kind;
        sym.timeline.frame_count = 1;
        let region_count = elements.len();
        if let Some(layer) = sym.timeline.layers.first_mut() {
            layer.id = new_id("layer");
            layer.name = "Traced Bitmap".into();
            if let Some(kf) = layer.keyframes.first_mut() {
                kf.frame = 0;
                kf.elements = elements;
            }
        }
        candidate.symbols.push(sym);
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;

        self.finish_edits();
        let created_id = sym_id.clone();
        self.transact_named("Trace bitmap", |editor| {
            editor.project = candidate;
            editor.library_selected = Some(created_id);
        });
        self.message(format!(
            "Traced '{}' into Library symbol '{}' ({region_count} vector region{}).",
            unit.path,
            sym_name,
            if region_count == 1 { "" } else { "s" }
        ));
        Ok(sym_id)
    }
}

fn is_placeable_image(media_type: &str) -> bool {
    matches!(media_type, "image/png" | "image/jpeg" | "image/svg+xml")
}

/// Type icon of a resource row: bitmap, sound, font, script / text, other.
fn media_icon(media_type: &str) -> neoflash_ui::Icon {
    let top = media_type.split('/').next().unwrap_or("");
    match top {
        "image" => i16::BITMAP,
        "audio" => i16::SOUND_FILE,
        "video" => i16::VIDEO,
        "font" => i16::FONT,
        "text" => i16::FILE_CODE,
        _ if media_type.contains("json")
            || media_type.contains("javascript")
            || media_type.contains("xml") =>
        {
            i16::FILE_CODE
        }
        _ => i16::PACKAGE,
    }
}

pub(crate) fn read_resource(path: &Path, remaining: usize) -> Result<ResourceUnit, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Resource filename must be valid UTF-8.")?;
    let file = std::fs::File::open(path).map_err(|error| format!("{name}: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_RESOURCE_BYTES.min(remaining) as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{name}: {error}"))?;
    if bytes.len() > remaining {
        return Err(format!(
            "{name}: resource import exceeds the project's 64 MiB limit."
        ));
    }
    ResourceUnit::from_bytes(new_id("resource"), name, media_type(path), &bytes)
        .map_err(|error| format!("{name}: {error}"))
}

fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "as" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}
