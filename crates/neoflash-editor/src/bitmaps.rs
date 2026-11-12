//! Bounded native textures for the pure authoring scene. Pixels remain resources.
use eframe::egui::{self, TextureHandle, TextureId, TextureOptions};
use neoflash_core::{ColorTransform, ElementKind, Project};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const MAX_TEXTURE_BYTES: usize = 256 * 1024 * 1024;
type Key = (String, bool, [u64; 8]);

struct Texture {
    source: Arc<str>,
    handle: TextureHandle,
    bytes: usize,
    image: Arc<egui::ColorImage>,
    smoothing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Scale9GuideDrag {
    pub symbol_id: String,
    pub edge_index: usize,
    pub initial_grid: [f64; 4],
    pub preview_grid: [f64; 4],
    pub mapping: crate::stage_mapping::StageMapping,
    pub moved: bool,
}

fn compute_nudged_scale9_grid(
    current: [f64; 4],
    edge_index: usize,
    new_coord: f64,
) -> Result<[f64; 4], String> {
    if !new_coord.is_finite() {
        return Err("9-slice guide coordinate must be finite.".into());
    }
    let mut x0 = current[0];
    let mut x1 = current[0] + current[2].max(1.0);
    let mut y0 = current[1];
    let mut y1 = current[1] + current[3].max(1.0);
    match edge_index {
        0 => x0 = new_coord.min(x1 - 1.0),
        1 => x1 = new_coord.max(x0 + 1.0),
        2 => y0 = new_coord.min(y1 - 1.0),
        3 => y1 = new_coord.max(y0 + 1.0),
        _ => return Err(format!("Invalid 9-slice guide edge index {edge_index}.")),
    }
    Ok([x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0)])
}

fn screen_line_distance_and_cursor(
    a: egui::Pos2,
    b: egui::Pos2,
    pos: egui::Pos2,
) -> (f32, egui::CursorIcon) {
    let dir = b - a;
    let len = dir.length();
    if len <= 1e-5 {
        return (f32::INFINITY, egui::CursorIcon::Default);
    }
    let cross = (dir.x * (pos.y - a.y) - dir.y * (pos.x - a.x)).abs();
    let dist = cross / len;
    // Dragging moves perpendicular to the guide line: a vertical guide moves horizontally.
    let cursor = if dir.y.abs() >= dir.x.abs() {
        egui::CursorIcon::ResizeHorizontal
    } else {
        egui::CursorIcon::ResizeVertical
    };
    (dist, cursor)
}

#[derive(Default)]
pub(crate) struct BitmapTextures {
    textures: HashMap<Key, Texture>,
    used: HashSet<Key>,
    reported: HashMap<String, Arc<str>>,
    warnings: Vec<String>,
}

impl BitmapTextures {
    pub fn begin_frame(&mut self, project: &Project) {
        self.used.clear();
        self.textures.retain(|(id, _, _), texture| {
            project.resources.iter().any(|resource| {
                resource.id == *id && Arc::ptr_eq(&resource.data_base64, &texture.source)
            })
        });
        self.reported.retain(|id, source| {
            project
                .resources
                .iter()
                .any(|resource| resource.id == *id && Arc::ptr_eq(&resource.data_base64, source))
        });
    }

    pub fn texture_transformed(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        kind: &ElementKind,
        color: ColorTransform,
    ) -> Option<TextureId> {
        let ElementKind::Bitmap {
            resource_id,
            smoothing,
            ..
        } = kind
        else {
            return None;
        };
        let resource = project
            .resources
            .iter()
            .find(|resource| resource.id == *resource_id)?;
        let channels: Vec<_> = color
            .multipliers
            .into_iter()
            .chain(color.offsets)
            .map(f64::to_bits)
            .collect();
        let key = (
            resource_id.clone(),
            *smoothing,
            channels.try_into().unwrap(),
        );
        self.used.insert(key.clone());
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.handle.id());
        }
        let decoded = match resource.decode_image() {
            Ok(image) => image,
            Err(error) => {
                self.warn(
                    resource_id,
                    &resource.data_base64,
                    format!("{}: {error}", resource.path),
                );
                return None;
            }
        };
        let bytes = decoded.rgba.len();
        let mut used_bytes: usize = self.textures.values().map(|texture| texture.bytes).sum();
        if used_bytes + bytes > MAX_TEXTURE_BYTES {
            self.textures.retain(|key, texture| {
                if !self.used.contains(key) && used_bytes + bytes > MAX_TEXTURE_BYTES {
                    used_bytes -= texture.bytes;
                    false
                } else {
                    true
                }
            });
        }
        if used_bytes + bytes > MAX_TEXTURE_BYTES {
            self.warn(
                resource_id,
                &resource.data_base64,
                "Native image preview exceeds its 64 MiB texture budget.".into(),
            );
            return None;
        }
        let sibling_key = (resource_id.clone(), !*smoothing, key.2);
        let image = if let Some(sibling) = self
            .textures
            .get(&sibling_key)
            .filter(|tex| Arc::ptr_eq(&tex.source, &resource.data_base64))
        {
            sibling.image.clone()
        } else if color.is_identity() {
            Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width as usize, decoded.height as usize],
                &decoded.rgba,
            ))
        } else {
            let mut pixels = decoded.rgba.clone();
            for pixel in Arc::make_mut(&mut pixels).as_chunks_mut::<4>().0 {
                let transformed = color.apply([pixel[0], pixel[1], pixel[2], pixel[3]]);
                pixel.copy_from_slice(&transformed);
            }
            Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width as usize, decoded.height as usize],
                &pixels,
            ))
        };
        let handle = ctx.load_texture(
            format!("bitmap-{resource_id}-{smoothing}"),
            image.clone(),
            if *smoothing {
                TextureOptions::LINEAR
            } else {
                TextureOptions::NEAREST
            },
        );
        let id = handle.id();
        self.textures.insert(
            key,
            Texture {
                source: resource.data_base64.clone(),
                handle,
                bytes,
                image,
                smoothing: *smoothing,
            },
        );
        Some(id)
    }

    pub(crate) fn image(&self, id: TextureId) -> Option<(Arc<egui::ColorImage>, bool)> {
        self.textures
            .values()
            .find(|texture| texture.handle.id() == id)
            .map(|texture| (texture.image.clone(), texture.smoothing))
    }

    fn warn(&mut self, id: &str, source: &Arc<str>, message: String) {
        if !self.reported.contains_key(id) {
            self.reported.insert(id.to_owned(), source.clone());
            self.warnings.push(message);
        }
    }

    pub fn take_warning(&mut self) -> Option<String> {
        let warning = self.warnings.first().cloned();
        self.warnings.clear();
        warning
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BitmapPropertiesDraft {
    pub(crate) resource_id: neoflash_core::Id,
    pub(crate) name: String,
    pub(crate) unit: neoflash_core::ResourceUnit,
    pub(crate) pending_source: Option<std::path::PathBuf>,
    pub(crate) export: neoflash_core::BitmapExportSettings,
    pub(crate) export_modified: bool,
}

fn bitmap_draft_id() -> egui::Id {
    egui::Id::new("nf2-bitmap-properties-draft")
}

pub(crate) fn has_bitmap_properties_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<BitmapPropertiesDraft>(bitmap_draft_id()))
        .is_some()
}

#[allow(dead_code)]
pub(crate) fn bitmap_properties_draft(ctx: &egui::Context) -> Option<BitmapPropertiesDraft> {
    ctx.data(|d| d.get_temp::<BitmapPropertiesDraft>(bitmap_draft_id()))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn update_bitmap_properties_draft(
    ctx: &egui::Context,
    f: impl FnOnce(&mut BitmapPropertiesDraft),
) {
    let id = bitmap_draft_id();
    if let Some(mut draft) = ctx.data(|d| d.get_temp::<BitmapPropertiesDraft>(id)) {
        f(&mut draft);
        ctx.data_mut(|d| d.insert_temp(id, draft));
    }
}

fn format_bitmap_stats(unit: &neoflash_core::ResourceUnit) -> String {
    let bytes = unit.byte_len();
    let size = if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.1} kB", bytes as f64 / 1_000.0)
    };
    let fmt = match unit.media_type.as_str() {
        "image/png" => "PNG",
        "image/jpeg" => "JPEG",
        "image/gif" => "GIF",
        "image/webp" => "WebP",
        "image/svg+xml" => "SVG",
        other => other,
    };
    match unit.decode_image() {
        Ok(decoded) => format!(
            "{} × {} · {fmt} · 32-bit RGBA · {size}",
            decoded.width, decoded.height
        ),
        Err(_) => format!("{fmt} · {size}"),
    }
}

pub(crate) fn apply_bitmap_fill_to_project(
    project: &mut Project,
    symbol_id: &str,
    layer_id: &str,
    frame: u32,
    fill: neoflash_core::AuthoredBitmapFill,
) -> Result<(), String> {
    if !project
        .resources
        .iter()
        .any(|resource| resource.id == fill.resource_id && resource.is_image())
    {
        return Err(format!(
            "Bitmap resource {} was not found.",
            fill.resource_id
        ));
    }
    let symbol = project
        .symbol_mut(symbol_id)
        .ok_or_else(|| "Active symbol not found.".to_string())?;
    let layer = symbol
        .timeline
        .layers
        .iter_mut()
        .find(|layer| layer.id == layer_id)
        .ok_or_else(|| "Selected layer was not found.".to_string())?;
    let key_index = layer
        .keyframes
        .iter()
        .rposition(|key| key.frame <= frame)
        .ok_or_else(|| "Selected element was not found on the current frame.".to_string())?;
    let key = &mut layer.keyframes[key_index];
    if !key
        .elements
        .iter()
        .any(|element| element.id == fill.element_id)
    {
        return Err("Selected element was not found on the current frame.".into());
    }
    if let Some(existing) = key
        .bitmap_fills
        .iter_mut()
        .find(|existing| existing.element_id == fill.element_id)
    {
        *existing = fill;
    } else {
        key.bitmap_fills.push(fill);
    }
    Ok(())
}

impl crate::app::Editor {
    fn resolve_bitmap_properties_target(
        &self,
        explicit: Option<neoflash_core::Id>,
    ) -> Option<neoflash_core::Id> {
        if let Some(id) = explicit
            && self
                .project
                .resources
                .iter()
                .any(|u| u.id == id && u.is_image())
        {
            return Some(id);
        }
        if let Some(selected) = &self.resource_panel.selected
            && self
                .project
                .resources
                .iter()
                .any(|u| &u.id == selected && u.is_image())
        {
            return Some(selected.clone());
        }
        if let Some(el) = self.selected_element()
            && let ElementKind::Bitmap { resource_id, .. } = &el.kind
            && self
                .project
                .resources
                .iter()
                .any(|u| u.id == *resource_id && u.is_image())
        {
            return Some(resource_id.clone());
        }
        self.project
            .resources
            .iter()
            .find(|u| u.is_image())
            .map(|u| u.id.clone())
    }

    /// Open the `Bitmap properties` modal dialog (`R-022`) for `resource_id` (or the selected bitmap).
    pub fn open_bitmap_properties(
        &mut self,
        ctx: &egui::Context,
        resource_id: Option<neoflash_core::Id>,
    ) {
        self.finish_edits();
        let Some(target_id) = self.resolve_bitmap_properties_target(resource_id) else {
            self.message("Import a bitmap image (PNG or JPEG) first.");
            return;
        };
        let Some(unit) = self
            .project
            .resources
            .iter()
            .find(|u| u.id == target_id)
            .cloned()
        else {
            return;
        };
        let export = unit.bitmap_export.unwrap_or_default();
        let draft = BitmapPropertiesDraft {
            resource_id: unit.id.clone(),
            name: unit.path.clone(),
            unit,
            pending_source: None,
            export,
            export_modified: false,
        };
        ctx.data_mut(|d| d.insert_temp(bitmap_draft_id(), draft));
    }

    pub fn cancel_bitmap_properties(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.remove::<BitmapPropertiesDraft>(bitmap_draft_id()));
    }

    pub fn confirm_bitmap_properties(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<BitmapPropertiesDraft>(bitmap_draft_id()))
        else {
            return Ok(());
        };
        let Some(index) = self
            .project
            .resources
            .iter()
            .position(|u| u.id == draft.resource_id && u.is_image())
        else {
            ctx.data_mut(|d| d.remove::<BitmapPropertiesDraft>(bitmap_draft_id()));
            return Err("Bitmap resource no longer exists.".into());
        };
        let trimmed = draft.name.trim();
        if trimmed.is_empty() {
            return Err("Bitmap name cannot be empty.".into());
        }
        draft.unit.id = draft.resource_id.clone();
        draft.unit.path = trimmed.to_owned();
        if draft.export_modified || draft.unit.bitmap_export.is_some() {
            draft.unit.bitmap_export = if draft.export.is_default() {
                None
            } else {
                Some(draft.export)
            };
        }
        let existing = &self.project.resources[index];
        let unchanged = existing.path == draft.unit.path
            && existing.media_type == draft.unit.media_type
            && existing.data_base64 == draft.unit.data_base64
            && existing.bitmap_export == draft.unit.bitmap_export;
        if !unchanged {
            let mut candidate = self.project.clone();
            candidate.format_version = candidate.format_version.max(3);
            let allow_smoothing = draft.export.allow_smoothing;
            let res_id = draft.unit.id.clone();
            candidate.resources[index] = draft.unit;
            if draft.export_modified {
                for sym in &mut candidate.symbols {
                    for layer in &mut sym.timeline.layers {
                        for key in &mut layer.keyframes {
                            for el in &mut key.elements {
                                if let ElementKind::Bitmap {
                                    resource_id,
                                    smoothing,
                                    ..
                                } = &mut el.kind
                                    && *resource_id == res_id
                                {
                                    *smoothing = allow_smoothing;
                                }
                            }
                            for bf in &mut key.bitmap_fills {
                                if bf.resource_id == res_id {
                                    bf.smoothing = allow_smoothing;
                                }
                            }
                        }
                    }
                }
            }
            candidate.upgrade_format_for_features();
            candidate.validate().map_err(|e| e.to_string())?;
            self.transact_named("Bitmap properties", |s| {
                s.project = candidate;
            });
            self.resource_panel.baseline = None;
        }
        ctx.data_mut(|d| d.remove::<BitmapPropertiesDraft>(bitmap_draft_id()));
        Ok(())
    }

    /// Replace a bitmap resource's bytes and/or export settings in one `"Bitmap properties"` Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn replace_bitmap_resource_bytes(
        &mut self,
        resource_id: &str,
        new_name: &str,
        new_media_type: &str,
        bytes: &[u8],
        export: Option<neoflash_core::BitmapExportSettings>,
    ) -> Result<(), String> {
        self.finish_edits();
        if !new_media_type.starts_with("image/") {
            return Err("Import a supported image format (PNG or JPEG).".into());
        }
        let Some(index) = self
            .project
            .resources
            .iter()
            .position(|u| u.id == resource_id && u.is_image())
        else {
            return Err(format!("Bitmap resource {resource_id} was not found."));
        };
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Bitmap name cannot be empty.".into());
        }
        let mut unit =
            neoflash_core::ResourceUnit::from_bytes(resource_id, trimmed, new_media_type, bytes)
                .map_err(|e| e.to_string())?;
        unit.bitmap_export = export
            .or_else(|| self.project.resources[index].bitmap_export)
            .filter(|e| !e.is_default());
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        candidate.resources[index] = unit;
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Bitmap properties", |s| {
            s.project = candidate;
        });
        self.resource_panel.baseline = None;
        Ok(())
    }

    /// Attach or update an `AuthoredBitmapFill` (resource ID, affine UV matrix, repeat, smoothing)
    /// on the selected shape element in a single `"Bitmap fill"` Undo transaction.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_selected_bitmap_fill(
        &mut self,
        resource_id: neoflash_core::Id,
        matrix: [f64; 6],
        repeat: bool,
        smoothing: bool,
    ) -> Result<(), String> {
        self.finish_edits();
        let Some(target) = self
            .selection
            .single()
            .filter(|target| self.target_editable(target))
            .cloned()
        else {
            return Err("Select a shape element first.".into());
        };
        let mut candidate = self.project.clone();
        let sym_id = self.symbol_id().to_owned();
        apply_bitmap_fill_to_project(
            &mut candidate,
            &sym_id,
            &target.layer,
            self.frame,
            neoflash_core::AuthoredBitmapFill {
                element_id: target.element,
                resource_id,
                matrix,
                repeat,
                smoothing,
            },
        )?;
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        if candidate == self.project {
            return Ok(());
        }
        self.transact_named("Bitmap fill", |s| {
            s.project = candidate;
        });
        Ok(())
    }

    /// Set or clear the 9-slice scaling grid (`[x, y, width, height]`) on a symbol in one Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_symbol_scale9_grid(
        &mut self,
        symbol_id: &str,
        grid: Option<[f64; 4]>,
    ) -> Result<(), String> {
        self.finish_edits();
        if self.project.symbol(symbol_id).and_then(|s| s.scale9_grid()) == grid {
            return Ok(());
        }
        let mut candidate = self.project.clone();
        let Some(sym) = candidate.symbol_mut(symbol_id) else {
            return Err(format!("Symbol {symbol_id} not found."));
        };
        sym.set_scale9_grid(grid);
        candidate.upgrade_format_for_features();
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("9-slice scaling", |s| {
            s.project = candidate;
        });
        Ok(())
    }

    /// Move one of the four 9-slice guide lines (`0 = left`, `1 = right`, `2 = top`, `3 = bottom`)
    /// on `symbol_id` in symbol coordinates in one `"9-slice scaling"` Undo step (`T-166`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn nudge_symbol_scale9_guide(
        &mut self,
        symbol_id: &str,
        edge_index: usize,
        new_coord: f64,
    ) -> Result<[f64; 4], String> {
        let current = self
            .project
            .symbol(symbol_id)
            .ok_or_else(|| format!("Symbol {symbol_id} not found."))?
            .scale9_grid()
            .unwrap_or([10.0, 10.0, 80.0, 80.0]);
        let next = compute_nudged_scale9_grid(current, edge_index, new_coord)?;
        self.set_symbol_scale9_grid(symbol_id, Some(next))?;
        Ok(next)
    }

    /// Interactive Stage hit-testing, hover cursors, live drag preview, `Escape` cancel, and
    /// 1-Undo release commit for the 4 dashed 9-slice scaling guides (`T-231`).
    pub(crate) fn scale9_guide_input(
        &mut self,
        ui: &egui::Ui,
        canvas: egui::Rect,
        origin: egui::Pos2,
        mapping: crate::stage_mapping::StageMapping,
    ) {
        const HIT_RADIUS_PT: f32 = 5.0;

        let active_symbol_id = self.symbol().id.clone();
        let active_kind = self.symbol().kind;
        let Some(authored_grid) = self.symbol().scale9_grid() else {
            self.scale9_guide_drag = None;
            return;
        };
        if self.playing || active_kind == neoflash_core::SymbolKind::Button {
            self.scale9_guide_drag = None;
            return;
        }
        if self
            .scale9_guide_drag
            .as_ref()
            .is_some_and(|d| d.symbol_id != active_symbol_id || d.mapping != mapping)
        {
            self.scale9_guide_drag = None;
        }

        let zoom = self.zoom;
        let show_rulers = self.stage_view.prefs.rulers;
        let rulers = neoflash_ui::components::canvas::RulerLayout::new(canvas);
        let in_interactive_canvas = |pos: egui::Pos2| {
            canvas.contains(pos)
                && !(show_rulers && rulers.contains(pos))
                && ui.ctx().layer_id_at(pos).is_none_or(|l| l == ui.layer_id())
        };
        let hit_edge = |grid: [f64; 4], pos: egui::Pos2| -> Option<(usize, egui::CursorIcon)> {
            let [gx, gy, gw, gh] = grid;
            let x0 = gx as f32;
            let x1 = (gx + gw) as f32;
            let y0 = gy as f32;
            let y1 = (gy + gh) as f32;
            let span = 256.0_f32;
            let edges = [
                (
                    0usize,
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x0, y0 - span)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x0, y0 + span)),
                ),
                (
                    1usize,
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x1, y0 - span)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x1, y0 + span)),
                ),
                (
                    2usize,
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x0 - span, y0)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x0 + span, y0)),
                ),
                (
                    3usize,
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x0 - span, y1)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(x0 + span, y1)),
                ),
            ];
            let mut best: Option<(usize, f32, egui::CursorIcon)> = None;
            for (idx, a, b) in edges {
                let (dist, cursor) = screen_line_distance_and_cursor(a, b, pos);
                if dist <= HIT_RADIUS_PT && best.is_none_or(|(_, bdist, _)| dist < bdist) {
                    best = Some((idx, dist, cursor));
                }
            }
            best.map(|(idx, _, cursor)| (idx, cursor))
        };
        let coord_for_edge = |edge_index: usize, pos: egui::Pos2| -> f64 {
            let pt = mapping.screen_to_edit(origin, zoom, pos);
            let raw = if edge_index < 2 {
                f64::from(pt.x)
            } else {
                f64::from(pt.y)
            };
            (raw * 10.0).round() / 10.0
        };

        let mut consumed_pointer = false;
        let mut consumed_escape = false;
        let mut commit_release: Option<(String, [f64; 4], [f64; 4])> = None;

        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if self.scale9_guide_drag.is_none()
                    && !self.stage_view.guide_pointer
                    && in_interactive_canvas(pos) =>
                {
                    if let Some((edge_index, cursor)) = hit_edge(authored_grid, pos) {
                        self.scale9_guide_drag = Some(Scale9GuideDrag {
                            symbol_id: active_symbol_id.clone(),
                            edge_index,
                            initial_grid: authored_grid,
                            preview_grid: authored_grid,
                            mapping,
                            moved: false,
                        });
                        self.stage_view.guide_pointer = true;
                        ui.ctx().set_cursor_icon(cursor);
                        consumed_pointer = true;
                    }
                }
                egui::Event::PointerMoved(pos) if self.scale9_guide_drag.is_some() => {
                    if let Some(drag) = &mut self.scale9_guide_drag {
                        let new_coord = coord_for_edge(drag.edge_index, pos);
                        if let Ok(next) = compute_nudged_scale9_grid(
                            drag.initial_grid,
                            drag.edge_index,
                            new_coord,
                        ) {
                            if next
                                .iter()
                                .zip(drag.initial_grid.iter())
                                .any(|(a, b)| (a - b).abs() > 1e-6)
                            {
                                drag.moved = true;
                            }
                            drag.preview_grid = next;
                        }
                    }
                    self.stage_view.guide_pointer = true;
                    consumed_pointer = true;
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } if self.scale9_guide_drag.is_some() => {
                    if let Some(mut drag) = self.scale9_guide_drag.take() {
                        if canvas.contains(pos) {
                            let new_coord = coord_for_edge(drag.edge_index, pos);
                            if let Ok(next) = compute_nudged_scale9_grid(
                                drag.initial_grid,
                                drag.edge_index,
                                new_coord,
                            ) {
                                if next
                                    .iter()
                                    .zip(drag.initial_grid.iter())
                                    .any(|(a, b)| (a - b).abs() > 1e-6)
                                {
                                    drag.moved = true;
                                }
                                drag.preview_grid = next;
                            }
                        }
                        if drag.moved {
                            commit_release =
                                Some((drag.symbol_id, drag.initial_grid, drag.preview_grid));
                        }
                    }
                    self.stage_view.guide_pointer = true;
                    consumed_pointer = true;
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if self.scale9_guide_drag.is_some() => {
                    self.scale9_guide_drag = None;
                    consumed_escape = true;
                }
                _ => {}
            }
        }

        if consumed_pointer || consumed_escape {
            ui.ctx().input_mut(|i| {
                i.events.retain(|event| {
                    !(consumed_pointer
                        && matches!(
                            event,
                            egui::Event::PointerButton {
                                button: egui::PointerButton::Primary,
                                ..
                            } | egui::Event::PointerMoved(_)
                        ))
                        && !(consumed_escape
                            && matches!(
                                event,
                                egui::Event::Key {
                                    key: egui::Key::Escape,
                                    pressed: true,
                                    ..
                                }
                            ))
                });
            });
        }

        if let Some((sym_id, initial, final_grid)) = commit_release
            && final_grid
                .iter()
                .zip(initial.iter())
                .any(|(a, b)| (a - b).abs() > 1e-6)
        {
            let _ = self.set_symbol_scale9_grid(&sym_id, Some(final_grid));
        }

        if let Some(drag) = &self.scale9_guide_drag {
            self.stage_view.guide_pointer = true;
            let [gx, gy, gw, gh] = drag.preview_grid;
            let (a, b) = match drag.edge_index {
                0 => (
                    mapping.edit_to_screen(origin, zoom, egui::pos2(gx as f32, 0.0)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(gx as f32, 100.0)),
                ),
                1 => (
                    mapping.edit_to_screen(origin, zoom, egui::pos2((gx + gw) as f32, 0.0)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2((gx + gw) as f32, 100.0)),
                ),
                2 => (
                    mapping.edit_to_screen(origin, zoom, egui::pos2(0.0, gy as f32)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(100.0, gy as f32)),
                ),
                _ => (
                    mapping.edit_to_screen(origin, zoom, egui::pos2(0.0, (gy + gh) as f32)),
                    mapping.edit_to_screen(origin, zoom, egui::pos2(100.0, (gy + gh) as f32)),
                ),
            };
            let (_, cursor) = screen_line_distance_and_cursor(a, b, a);
            ui.ctx().set_cursor_icon(cursor);
        } else if !self.stage_view.guide_pointer
            && let Some(pos) = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| in_interactive_canvas(*p))
            && let Some((_, cursor)) = hit_edge(authored_grid, pos)
        {
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    /// Paint the 4 dashed 9-slice scaling guide lines when editing a symbol that has `scale9_grid` enabled (`T-166`).
    pub(crate) fn paint_scale9_guides(
        &self,
        painter: &egui::Painter,
        stage_rect: egui::Rect,
        origin: egui::Pos2,
        mapping: crate::stage_mapping::StageMapping,
    ) {
        let active_id = &self.symbol().id;
        let Some([gx, gy, gw, gh]) = self
            .scale9_guide_drag
            .as_ref()
            .filter(|d| &d.symbol_id == active_id)
            .map(|d| d.preview_grid)
            .or_else(|| self.symbol().scale9_grid())
        else {
            return;
        };
        let p0 = mapping.edit_to_screen(origin, self.zoom, egui::pos2(gx as f32, gy as f32));
        let p1 = mapping.edit_to_screen(
            origin,
            self.zoom,
            egui::pos2((gx + gw) as f32, (gy + gh) as f32),
        );
        let color = neoflash_ui::palette(painter.ctx()).guide;
        let stroke = egui::Stroke::new(1.0_f32, color);
        for sx in [p0.x, p1.x] {
            if sx >= stage_rect.left() && sx <= stage_rect.right() {
                let mut y = stage_rect.top();
                while y < stage_rect.bottom() {
                    let y_end = (y + 5.0).min(stage_rect.bottom());
                    painter.line_segment([egui::pos2(sx, y), egui::pos2(sx, y_end)], stroke);
                    y += 9.0;
                }
            }
        }
        for sy in [p0.y, p1.y] {
            if sy >= stage_rect.top() && sy <= stage_rect.bottom() {
                let mut x = stage_rect.left();
                while x < stage_rect.right() {
                    let x_end = (x + 5.0).min(stage_rect.right());
                    painter.line_segment([egui::pos2(x, sy), egui::pos2(x_end, sy)], stroke);
                    x += 9.0;
                }
            }
        }
    }

    pub(crate) fn bitmap_properties_dialog(&mut self, ctx: &egui::Context) {
        use crate::panels::planned::caption;
        use neoflash_core::{BitmapCompression, VALID_JPEG_QUALITY_RANGE};
        use neoflash_ui::components as c;
        use neoflash_ui::components::timeline as tl;
        use neoflash_ui::tokens::{TextStyle, space};
        use neoflash_ui::typography::rich;

        let Some(mut draft) = ctx.data(|d| d.get_temp::<BitmapPropertiesDraft>(bitmap_draft_id()))
        else {
            return;
        };
        if !self
            .project
            .resources
            .iter()
            .any(|u| u.id == draft.resource_id && u.is_image())
        {
            self.cancel_bitmap_properties(ctx);
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let stats_display = format_bitmap_stats(&draft.unit);
        let source_display = draft
            .pending_source
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| draft.unit.path.clone());

        let pass_id = egui::Id::new("nf2-bitmap-properties-painted-pass");
        let pass_nr = ctx.cumulative_pass_nr();
        if ctx.data(|d| d.get_temp::<u64>(pass_id)) == Some(pass_nr) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(pass_id, pass_nr));

        let mut close_cancel = false;
        let mut close_ok = false;
        let mut do_import = false;
        let kb = crate::panels::dialog_helpers::DialogKeyboardState::capture(ctx);

        let modal = egui::Modal::new(egui::Id::new("bitmap-properties"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Bitmap properties",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 520.0,
                    header_top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        let w = 520.0 - 2.0 * space::S5;
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);

                        ui.horizontal(|ui| {
                            let (span, _) = ui
                                .allocate_exact_size(egui::vec2(120.0, 64.0), egui::Sense::hover());
                            tl::paint_span(
                                ui.painter(),
                                ui.ctx(),
                                span,
                                tl::SpanType::Static,
                                true,
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = space::S1;
                                ui.add(
                                    c::TextField::new(&mut draft.name, "Bitmap name")
                                        .width(w - 120.0 - space::S2),
                                );
                                caption(ui, &source_display);
                                caption(ui, &stats_display);
                            });
                        });

                        ui.add_space(space::S1);
                        ui.label(rich("Export", TextStyle::Overline).color(p.text_tertiary));
                        let comp_label =
                            format!("Compression: {}", draft.export.compression.label());
                        c::Select::new(ui.id().with("bitmap-props-compression"), &comp_label)
                            .width(w)
                            .show(ui, |ui| {
                                for comp in BitmapCompression::ALL {
                                    if ui
                                        .add(
                                            c::MenuItem::new(&format!(
                                                "Compression: {}",
                                                comp.label()
                                            ))
                                            .checked(draft.export.compression == comp),
                                        )
                                        .clicked()
                                    {
                                        draft.export.compression = comp;
                                        draft.export_modified = true;
                                        ui.close();
                                    }
                                }
                            });
                        let is_jpeg = draft.export.compression == BitmapCompression::PhotoJpeg;
                        ui.add_enabled_ui(is_jpeg, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .add(c::Checkbox::new(
                                        &mut draft.export.use_imported_jpeg_data,
                                        "Use publish quality",
                                    ))
                                    .changed()
                                {
                                    draft.export_modified = true;
                                }
                                ui.add_enabled_ui(!draft.export.use_imported_jpeg_data, |ui| {
                                    let mut q = f64::from(draft.export.jpeg_quality);
                                    if ui
                                        .add(
                                            c::Field::new(&mut q, "Quality")
                                                .prefix("Quality")
                                                .unit("%")
                                                .range(
                                                    f64::from(*VALID_JPEG_QUALITY_RANGE.start())
                                                        ..=f64::from(
                                                            *VALID_JPEG_QUALITY_RANGE.end(),
                                                        ),
                                                )
                                                .decimals(0)
                                                .step(1.0)
                                                .width(140.0),
                                        )
                                        .changed()
                                    {
                                        draft.export.jpeg_quality =
                                            q.round().clamp(0.0, 100.0) as u8;
                                        draft.export_modified = true;
                                    }
                                });
                            });
                        });
                        if ui
                            .add(c::Checkbox::new(
                                &mut draft.export.allow_smoothing,
                                "Allow smoothing",
                            ))
                            .changed()
                        {
                            draft.export_modified = true;
                        }

                        ui.horizontal(|ui| {
                            if ui.add(c::Button::ghost("Import…")).clicked() {
                                do_import = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.add(c::Button::primary("OK")).clicked() {
                                        close_ok = true;
                                    }
                                    if ui.add(c::Button::new("Cancel")).clicked() {
                                        close_cancel = true;
                                    }
                                },
                            );
                        });
                    });
            });

        if do_import
            && let Some(path) = rfd::FileDialog::new()
                .add_filter(
                    "Image",
                    &[
                        "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "svg",
                    ],
                )
                .pick_file()
        {
            match crate::resources::read_resource(&path, neoflash_core::MAX_RESOURCE_BYTES) {
                Ok(mut loaded) if loaded.is_image() => {
                    loaded.id = draft.resource_id.clone();
                    draft.name = loaded.path.clone();
                    draft.unit = loaded;
                    draft.pending_source = Some(path);
                }
                Ok(_) => self.message("Selected file is not a supported bitmap image."),
                Err(err) => self.message(format!("Bitmap import failed: {err}")),
            }
        }

        if !close_cancel && !close_ok && !do_import {
            match kb.resolve(ctx, &modal, !draft.name.trim().is_empty()) {
                crate::panels::dialog_helpers::DialogKeyboardOutcome::Cancel => {
                    close_cancel = true;
                }
                crate::panels::dialog_helpers::DialogKeyboardOutcome::Confirm => {
                    close_ok = true;
                }
                crate::panels::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        if close_cancel {
            self.cancel_bitmap_properties(ctx);
        } else if close_ok {
            ctx.data_mut(|d| d.insert_temp(bitmap_draft_id(), draft));
            if let Err(err) = self.confirm_bitmap_properties(ctx) {
                self.message(format!("Bitmap properties rejected: {err}"));
            }
        } else {
            ctx.data_mut(|d| d.insert_temp(bitmap_draft_id(), draft));
        }
    }
}
