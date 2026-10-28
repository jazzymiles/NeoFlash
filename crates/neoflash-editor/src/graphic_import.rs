//! One atomic graphic import shared by the File menu and native file drops.
use crate::{
    app::Editor,
    selection::{Selection, SelectionTarget},
};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use std::{collections::HashMap, path::PathBuf};

pub(crate) struct PreparedGraphic {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub elements: Vec<Element>,
    pub resources: Vec<ResourceUnit>,
    pub notice: Option<String>,
}

impl Editor {
    pub(crate) fn import_graphics_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Graphics", &["svg", "png", "jpg", "jpeg"])
            .pick_files()
            && let Err(error) = self.import_graphics_paths(&paths, None)
        {
            self.message(format!("Graphic import failed: {error}"));
        }
    }

    pub(crate) fn stage_file_drop(&mut self, ui: &egui::Ui, area: Rect, origin: Pos2) {
        let ctx = ui.ctx();
        let (hovering, pointer, dropped) = ctx.input(|input| {
            (
                !input.raw.hovered_files.is_empty(),
                input.pointer.latest_pos(),
                input.raw.dropped_files.clone(),
            )
        });
        if hovering {
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
                egui::FontId::proportional(18.),
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
        let point = [
            (position.x - origin.x) as f64 / self.zoom as f64,
            (position.y - origin.y) as f64 / self.zoom as f64,
        ];
        if let Err(error) = self.import_graphics_paths(&paths, Some(point)) {
            self.message(format!("Graphic import failed: {error}"));
        }
    }

    pub(crate) fn import_graphics_paths(
        &mut self,
        paths: &[PathBuf],
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        if !self.layer_editable() {
            return Err("Select an unlocked, visible artwork layer first.".into());
        }
        if paths.len() > 64 {
            return Err("Import at most 64 graphics at once.".into());
        }
        self.finish_edits();
        let mut remaining = MAX_RESOURCE_TOTAL_BYTES;
        let mut graphics = Vec::with_capacity(paths.len());
        for path in paths {
            let unit = crate::resources::read_resource(path, remaining)?;
            remaining = remaining.saturating_sub(unit.byte_len());
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Graphic")
                .to_owned();
            let mut graphic = match unit.media_type.as_str() {
                "image/svg+xml" => crate::svg_graphics::prepare_svg(unit)?,
                "image/png" | "image/jpeg" => {
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
                        }],
                        resources: vec![unit],
                        notice: None,
                    }
                }
                _ => {
                    return Err(format!(
                        "{}: use SVG, PNG or JPEG for Stage graphics.",
                        path.display()
                    ));
                }
            };
            graphic.name = name;
            graphics.push(graphic);
        }
        self.place_prepared_graphics(graphics, point)
    }

    pub(crate) fn place_svg_resource(&mut self, resource_id: &str) -> Result<(), String> {
        let unit = self
            .project
            .resources
            .iter()
            .find(|unit| unit.id == resource_id)
            .ok_or("This SVG resource no longer exists.")?
            .clone();
        let mut graphic = crate::svg_graphics::prepare_svg(unit)?;
        graphic.name = std::path::Path::new(&graphic.name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("SVG")
            .to_owned();
        self.place_prepared_graphics(vec![graphic], None)
    }

    fn place_prepared_graphics(
        &mut self,
        graphics: Vec<PreparedGraphic>,
        point: Option<[f64; 2]>,
    ) -> Result<(), String> {
        if !self.layer_editable() {
            return Err("Select an unlocked, visible artwork layer first.".into());
        }
        self.finish_edits();
        let count = graphics.len();
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        let folder = self.library_panel.selected_folder.clone();
        candidate.add_library_folder(&folder)?;
        let mut placed = Vec::with_capacity(graphics.len());
        let mut notices = Vec::new();
        let mut last_symbol = None;
        for (index, mut graphic) in graphics.into_iter().enumerate() {
            merge_graphic_resources(&mut candidate, &mut graphic)?;
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
            let scale = (candidate.width as f64 * 0.8 / graphic.width)
                .min(candidate.height as f64 * 0.8 / graphic.height)
                .min(1.);
            let at = point.unwrap_or(if self.editing.len() > 1 {
                [0., 0.]
            } else {
                [candidate.width as f64 / 2., candidate.height as f64 / 2.]
            });
            placed.push(Element {
                id: new_id("object"),
                name: name.clone(),
                transform: Transform {
                    x: at[0] - graphic.width * scale / 2. + index as f64 * 16.,
                    y: at[1] - graphic.height * scale / 2. + index as f64 * 16.,
                    scale_x: scale,
                    scale_y: scale,
                    ..Default::default()
                },
                kind: ElementKind::Instance {
                    symbol_id: symbol_id.clone(),
                    playback: InstancePlayback::SingleFrame(0),
                },
                scripts: vec![],
                accessibility: None,
            });
            candidate.symbols.push(Symbol {
                id: symbol_id.clone(),
                name,
                folder: folder.clone(),
                kind: SymbolKind::Graphic,
                class_name: None,
                linkage_id: None,
                timeline: Timeline {
                    frame_count: 1,
                    layers: vec![Layer {
                        properties: Default::default(),
                        id: new_id("layer"),
                        name: "Artwork".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: graphic.elements,
                            tween: None,
                            label: String::new(),
                        }],
                    }],
                    scripts: vec![],
                },
            });
            if let Some(notice) = graphic.notice {
                notices.push(notice);
            }
            last_symbol = Some(symbol_id);
        }
        let symbol_id = self.symbol_id().to_owned();
        let layer = &mut candidate
            .symbol_mut(&symbol_id)
            .ok_or("The current symbol was removed.")?
            .timeline
            .layers[self.layer];
        let layer_id = layer.id.clone();
        if !layer.keyframes.iter().any(|key| key.frame == self.frame) {
            let mut key = layer.key_at(self.frame).cloned().unwrap_or(Keyframe {
                frame: self.frame,
                elements: vec![],
                tween: None,
                label: String::new(),
            });
            key.elements = sample_layer(layer, self.frame);
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
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact(|editor| {
            editor.project = candidate;
            editor.selection = Selection {
                symbol: symbol_id,
                targets,
            };
            editor.library_selected = last_symbol;
        });
        self.tool = crate::app::Tool::Select;
        let suffix = if notices.is_empty() {
            String::new()
        } else {
            format!(" {}", notices.join(" "))
        };
        self.message(format!("Imported {count} graphic(s).{suffix}"));
        Ok(())
    }
}

fn merge_graphic_resources(
    project: &mut Project,
    graphic: &mut PreparedGraphic,
) -> Result<(), String> {
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
        ids.insert(unit.id.clone(), unit.id.clone());
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
    Ok(())
}
