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
    pub(crate) fn import_resource_paths(
        &mut self,
        paths: &[std::path::PathBuf],
    ) -> Result<(), String> {
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
            let unit = if is_font_import_path(path) {
                let mut f_unit = read_font_resource(path, remaining)?;
                f_unit.path = unique_font_resource_path(&self.project.resources, &f_unit.path);
                f_unit
            } else {
                read_resource(path, remaining)?
            };
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
        let font_units: Vec<ResourceUnit> = units
            .iter()
            .filter(|u| neoflash_core::is_font_resource_unit(u))
            .cloned()
            .collect();
        candidate.resources.extend(units);
        for font_unit in &font_units {
            upsert_imported_font_embedding_metadata(&mut candidate.resources, font_unit)?;
        }
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
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate);
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
    matches!(
        media_type,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/tiff"
            | "image/svg+xml"
    )
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

pub(crate) const IMPORT_FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "otc", "woff", "woff2"];

pub(crate) fn is_font_import_path(path: &Path) -> bool {
    path.extension()
        .and_then(|part| part.to_str())
        .is_some_and(|ext| {
            IMPORT_FONT_EXTENSIONS
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(ext))
        })
}

fn is_valid_font_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && matches!(
            &bytes[..4],
            b"\x00\x01\x00\x00" | b"true" | b"typ1" | b"OTTO" | b"ttcf" | b"wOFF" | b"wOF2"
        )
}

pub(crate) fn unique_font_resource_path(existing: &[ResourceUnit], requested: &str) -> String {
    if !existing
        .iter()
        .any(|r| r.path.eq_ignore_ascii_case(requested))
    {
        return requested.to_owned();
    }
    let req_path = Path::new(requested);
    let parent = req_path
        .parent()
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("fonts");
    let stem = req_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("font");
    let ext = req_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    for idx in 2..=10_000 {
        let candidate = if ext.is_empty() {
            format!("{parent}/{stem}-{idx}")
        } else {
            format!("{parent}/{stem}-{idx}.{ext}")
        };
        if !existing
            .iter()
            .any(|r| r.path.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    format!("{parent}/{stem}-{}", new_id("font"))
}

pub(crate) fn read_font_resource(path: &Path, remaining: usize) -> Result<ResourceUnit, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Font filename must be valid UTF-8.")?;
    let ext = path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "ttc" | "otc" => "font/collection",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => {
            return Err(format!(
                "{name}: unsupported font format '.{ext}'. Supported formats: TTF, OTF, TTC, OTC, WOFF, WOFF2."
            ));
        }
    };
    let file = std::fs::File::open(path).map_err(|error| format!("{name}: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_RESOURCE_BYTES.min(remaining) as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{name}: {error}"))?;
    if bytes.is_empty() {
        return Err(format!("{name}: font file is empty."));
    }
    if bytes.len() > remaining || bytes.len() > MAX_RESOURCE_BYTES {
        return Err(format!(
            "{name}: resource import exceeds the project's 64 MiB limit."
        ));
    }
    if !is_valid_font_magic(&bytes) {
        return Err(format!(
            "{name}: invalid or unsupported font binary header (expected TTF, OTF, TTC, WOFF, or WOFF2)."
        ));
    }
    let rel_path = format!("fonts/{name}");
    ResourceUnit::from_bytes(new_id("res"), rel_path, mime, &bytes)
        .map_err(|error| format!("{name}: {error}"))
}

fn fallback_family_and_style_from_stem(stem: &str) -> (String, String) {
    let trimmed = stem.trim();
    for (suffix, style) in [
        ("-BoldItalic", "Bold Italic"),
        ("_BoldItalic", "Bold Italic"),
        ("-BoldOblique", "Bold Italic"),
        ("_BoldOblique", "Bold Italic"),
        ("-SemiBold", "Semi Bold"),
        ("_SemiBold", "Semi Bold"),
        ("-Bold", "Bold"),
        ("_Bold", "Bold"),
        ("-Italic", "Italic"),
        ("_Italic", "Italic"),
        ("-Oblique", "Italic"),
        ("_Oblique", "Italic"),
        ("-Regular", "Regular"),
        ("_Regular", "Regular"),
    ] {
        if trimmed.len() > suffix.len()
            && trimmed[trimmed.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
        {
            let fam = trimmed[..trimmed.len() - suffix.len()].trim();
            if !fam.is_empty() {
                return (fam.to_owned(), style.to_owned());
            }
        }
    }
    let fam = if trimmed.is_empty() {
        "EmbeddedFont"
    } else {
        trimmed
    };
    (fam.to_owned(), "Regular".to_owned())
}

pub(crate) fn upsert_imported_font_embedding_metadata(
    resources: &mut Vec<ResourceUnit>,
    unit: &ResourceUnit,
) -> Result<crate::panels::font_embedding::EmbeddedFontEntry, String> {
    use crate::panels::font_embedding::EmbeddedFontEntry;
    const META_PATH: &str = "fonts/embedding.json";

    let raw_bytes = unit.decoded_bytes().map_err(|e| e.to_string())?;
    let stem = Path::new(&unit.path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&unit.id);
    let (fallback_family, fallback_style) = fallback_family_and_style_from_stem(stem);
    let (family, style) = if let Some(desc) =
        neoflash_core::inspect_sfnt_font_descriptor(&raw_bytes, &fallback_family)
    {
        let ui_style = match desc.font_style.as_str() {
            "boldItalic" => "Bold Italic",
            "bold" => "Bold",
            "italic" => "Italic",
            _ => "Regular",
        };
        let fam = if desc.font_name.trim().is_empty() {
            fallback_family
        } else {
            desc.font_name.trim().to_owned()
        };
        (fam, ui_style.to_owned())
    } else {
        (fallback_family, fallback_style)
    };

    let existing_meta_idx = resources
        .iter()
        .position(|r| r.path.eq_ignore_ascii_case(META_PATH));
    if existing_meta_idx.is_none() && resources.len() >= MAX_RESOURCES {
        return Err("The project supports at most 1024 resource files.".into());
    }
    let metadata_id = existing_meta_idx
        .map(|i| resources[i].id.clone())
        .unwrap_or_else(|| new_id("res"));
    let mut entries: Vec<EmbeddedFontEntry> = existing_meta_idx
        .and_then(|i| resources[i].decoded_bytes().ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let display_name = if style.eq_ignore_ascii_case("Regular") {
        family.clone()
    } else {
        format!("{family} {style}")
    };
    let mut base_class: String = display_name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    if base_class.starts_with(|c: char| c.is_ascii_digit()) {
        base_class.insert(0, '_');
    }
    let mut class_name = base_class.clone();
    if !class_name.is_empty() {
        let mut suffix = 2usize;
        while entries
            .iter()
            .any(|e| e.id != unit.id && e.class_name == class_name)
        {
            class_name = format!("{base_class}_{suffix}");
            suffix += 1;
        }
    }

    let entry = EmbeddedFontEntry {
        id: unit.id.clone(),
        name: display_name,
        family,
        style,
        ranges: [true, true, true, true, true, false, false, false],
        extra_chars: String::new(),
        export_for_actionscript: !class_name.is_empty(),
        class_name,
    };
    if let Some(existing) = entries.iter_mut().find(|e| e.id == unit.id) {
        *existing = entry.clone();
    } else {
        entries.push(entry.clone());
    }
    let meta_bytes = serde_json::to_vec(&entries).map_err(|e| e.to_string())?;
    let meta_unit =
        ResourceUnit::from_bytes(metadata_id, META_PATH, "application/json", &meta_bytes)
            .map_err(|e| e.to_string())?;
    if let Some(idx) = existing_meta_idx {
        resources[idx] = meta_unit;
    } else {
        resources.push(meta_unit);
    }
    Ok(entry)
}

pub(crate) const IMPORT_AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac", "aac", "m4a"];

pub(crate) fn is_audio_import_path(path: &Path) -> bool {
    path.extension()
        .and_then(|part| part.to_str())
        .is_some_and(|ext| {
            IMPORT_AUDIO_EXTENSIONS
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(ext))
        })
}

pub(crate) fn read_audio_resource(path: &Path, remaining: usize) -> Result<ResourceUnit, String> {
    read_audio_resource_with_ffmpeg(path, remaining, neoflash_export::detect_ffmpeg())
}

pub(crate) fn read_audio_resource_with_ffmpeg(
    path: &Path,
    remaining: usize,
    ffmpeg_override: Option<std::path::PathBuf>,
) -> Result<ResourceUnit, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Audio filename must be valid UTF-8.")?;
    let ext = path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp3" | "wav" => {
            let file = std::fs::File::open(path).map_err(|error| format!("{name}: {error}"))?;
            let mut bytes = Vec::new();
            file.take(MAX_RESOURCE_BYTES.min(remaining) as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("{name}: {error}"))?;
            if bytes.len() > remaining || bytes.len() > MAX_RESOURCE_BYTES {
                return Err(format!(
                    "{name}: resource import exceeds the project's 64 MiB limit."
                ));
            }
            let mime = if ext == "mp3" {
                "audio/mpeg"
            } else {
                "audio/wav"
            };
            ResourceUnit::from_bytes(new_id("resource"), name, mime, &bytes)
                .map_err(|error| format!("{name}: {error}"))
        }
        "ogg" | "flac" | "aac" | "m4a" => {
            if !path.is_file() {
                return Err(format!("{name}: audio file does not exist."));
            }
            let ffmpeg = ffmpeg_override.ok_or_else(|| {
                format!(
                    "{name}: importing .{ext} audio requires FFmpeg to transcode to 16-bit PCM WAV. Install ffmpeg on PATH, set NEOFLASH_FFMPEG, or convert to .wav / .mp3."
                )
            })?;
            static AUDIO_TRANSCODE_SEQ: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let seq = AUDIO_TRANSCODE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let tmp_wav = std::env::temp_dir().join(format!(
                "neoflash-audio-import-{}-{seq}.wav",
                std::process::id()
            ));
            let mut cmd = std::process::Command::new(&ffmpeg);
            neoflash_export::configure_media_subprocess_command(&mut cmd, &ffmpeg);
            cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(path)
                .args(["-vn", "-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2"])
                .arg(&tmp_wav);
            let output = cmd.output().map_err(|error| {
                let _ = std::fs::remove_file(&tmp_wav);
                format!(
                    "{name}: failed to run ffmpeg ({}): {error}",
                    ffmpeg.display()
                )
            })?;
            if !output.status.success() {
                let _ = std::fs::remove_file(&tmp_wav);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let detail = stderr.trim();
                return Err(if detail.is_empty() {
                    format!(
                        "{name}: ffmpeg failed to transcode .{ext} audio (exit {}).",
                        output.status
                    )
                } else {
                    format!("{name}: ffmpeg failed to transcode .{ext} audio: {detail}")
                });
            }
            let wav_res = (|| -> Result<Vec<u8>, String> {
                let file =
                    std::fs::File::open(&tmp_wav).map_err(|error| format!("{name}: {error}"))?;
                let mut bytes = Vec::new();
                file.take(MAX_RESOURCE_BYTES.min(remaining) as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| format!("{name}: {error}"))?;
                if bytes.is_empty() {
                    return Err(format!(
                        "{name}: ffmpeg produced an empty WAV file while transcoding .{ext}."
                    ));
                }
                if bytes.len() > remaining || bytes.len() > MAX_RESOURCE_BYTES {
                    return Err(format!(
                        "{name}: transcoded WAV exceeds the project's 64 MiB limit."
                    ));
                }
                Ok(bytes)
            })();
            let _ = std::fs::remove_file(&tmp_wav);
            let bytes = wav_res?;
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("sound");
            let wav_name = format!("{stem}.wav");
            ResourceUnit::from_bytes(new_id("resource"), &wav_name, "audio/wav", &bytes)
                .map_err(|error| format!("{name}: {error}"))
        }
        _ => Err(format!(
            "{name}: unsupported audio format '.{ext}'. Supported formats: MP3, WAV, OGG, FLAC, AAC, M4A."
        )),
    }
}

pub(crate) fn read_resource(path: &Path, remaining: usize) -> Result<ResourceUnit, String> {
    if is_audio_import_path(path) {
        return read_audio_resource(path, remaining);
    }
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
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" | "m4v" | "mov" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "avi" => "video/x-msvideo",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}
