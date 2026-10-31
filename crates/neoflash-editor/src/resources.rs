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
    selected: Option<String>,
    baseline: Option<(String, String)>,
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
        for path in paths {
            let unit = read_resource(path, remaining)?;
            remaining -= unit.byte_len();
            units.push(unit);
        }
        self.add_resource_units(units)
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
                if response.clicked() {
                    self.resource_panel.selected = Some(unit.id.clone());
                    self.resource_panel.baseline = None;
                    self.library_selected = None;
                }
            }
        });
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
