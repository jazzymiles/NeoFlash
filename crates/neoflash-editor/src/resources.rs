//! Portable script resources live beside symbols in the Library.
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{
    ElementKind, MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, ResourceUnit,
    Transform, new_id,
};
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
        ui.add_space(18.);
        ui.strong("Resource files");
        ui.separator();
        ui.small("Stored in the project. Load by path or place PNG, JPEG and SVG graphics.");
        if ui.button("Import files…").clicked()
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
        for unit in &self.project.resources {
            let selected = self.resource_panel.selected.as_ref() == Some(&unit.id);
            if ui
                .selectable_label(selected, &unit.path)
                .on_hover_text(&unit.media_type)
                .clicked()
            {
                self.resource_panel.selected = Some(unit.id.clone());
                self.resource_panel.baseline = None;
                self.library_selected = None;
            }
        }
        let Some(index) = self
            .project
            .resources
            .iter()
            .position(|unit| Some(&unit.id) == self.resource_panel.selected.as_ref())
        else {
            if self.project.resources.is_empty() {
                ui.small("Text, data and media files");
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
        ui.small(format!("{} bytes", unit.byte_len()));
        let svg_to_place = (unit.media_type == "image/svg+xml"
            && ui.button("Place SVG on Stage").clicked())
        .then(|| unit.id.clone());
        if matches!(unit.media_type.as_str(), "image/png" | "image/jpeg")
            && ui.button("Place image").clicked()
        {
            match unit.decode_image() {
                Ok(image) => {
                    let width = image.width;
                    let height = image.height;
                    let kind = ElementKind::Bitmap {
                        resource_id: unit.id.clone(),
                        width,
                        height,
                        smoothing: false,
                    };
                    let scale = (self.project.width as f64 * 0.8 / width as f64)
                        .min(self.project.height as f64 * 0.8 / height as f64)
                        .min(1.);
                    self.add_element_transformed(
                        kind,
                        Transform {
                            x: (self.project.width as f64 - width as f64 * scale) / 2.,
                            y: (self.project.height as f64 - height as f64 * scale) / 2.,
                            scale_x: scale,
                            scale_y: scale,
                            ..Default::default()
                        },
                    );
                }
                Err(error) => self.message(format!("Cannot place image: {error}")),
            }
        }
        if let Some(id) = svg_to_place {
            if let Err(error) = self.place_svg_resource(&id) {
                self.message(format!("Cannot place SVG: {error}"));
            }
            return;
        }
        ui.label("Project path");
        ui.add(
            egui::TextEdit::singleline(&mut self.resource_panel.path).desired_width(f32::INFINITY),
        );
        ui.label("Content type");
        ui.add(
            egui::TextEdit::singleline(&mut self.resource_panel.media_type)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            let changed = self.resource_panel.path != self.project.resources[index].path
                || self.resource_panel.media_type != self.project.resources[index].media_type;
            if ui
                .add_enabled(changed, egui::Button::new("Apply"))
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
            if ui.button("Remove file").clicked() {
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
