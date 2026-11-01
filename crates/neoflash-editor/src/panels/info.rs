//! Info panel (NF2 `Panel / Info`, PP-11 / W0.11; reference `panels-more-2.png`): 2×2 selection
//! bounds (`W` / `H`, `X` / `Y`, editable with one Undo step when an editable selection exists)
//! and live stage `Cursor` + `Colour` RGBA readouts under the pointer.
use super::planned::{form_label, mono};
use crate::app::Editor;
use crate::stage_view::RulerUnits;
use crate::theme::DesignUi as _;
use crate::workspace::PanelId;
use eframe::egui::{self, Pos2, Rect, Ui, Vec2};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::tokens::space;
use std::collections::HashMap;

#[derive(Clone)]
struct CachedBitmap {
    id: Id,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn sample_bitmap_pixel(
    ctx: &egui::Context,
    project: &Project,
    resource_id: &str,
    local: Pos2,
    drawn_w: f64,
    drawn_h: f64,
) -> Option<Color> {
    if drawn_w <= 0.0 || drawn_h <= 0.0 {
        return None;
    }
    let cache_id = egui::Id::new("nf2-info-bitmap-cache");
    let cached = ctx
        .data(|d| d.get_temp::<CachedBitmap>(cache_id))
        .filter(|b| b.id == resource_id);
    let bmp = match cached {
        Some(bmp) => bmp,
        None => {
            let res = project.resources.iter().find(|r| r.id == resource_id)?;
            let bytes = res.decoded_bytes().ok()?;
            let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
            let (width, height) = img.dimensions();
            let bmp = CachedBitmap {
                id: res.id.clone(),
                width,
                height,
                rgba: img.into_raw(),
            };
            ctx.data_mut(|d| d.insert_temp(cache_id, bmp.clone()));
            bmp
        }
    };
    if bmp.width == 0 || bmp.height == 0 {
        return None;
    }
    let u = (local.x as f64 / drawn_w).clamp(0.0, 0.999_999);
    let v = (local.y as f64 / drawn_h).clamp(0.0, 0.999_999);
    let px = ((u * bmp.width as f64) as u32).min(bmp.width - 1);
    let py = ((v * bmp.height as f64) as u32).min(bmp.height - 1);
    let idx = ((py * bmp.width + px) * 4) as usize;
    let slice = bmp.rgba.get(idx..idx + 4)?;
    Some([slice[0], slice[1], slice[2], slice[3]])
}

fn sample_draw_item(
    ctx: &egui::Context,
    project: &Project,
    item: &DrawItem,
    stage_pt: Pos2,
) -> Option<Color> {
    let base = match &item.kind {
        ElementKind::Rectangle { fill, stroke, .. }
        | ElementKind::Ellipse { fill, stroke, .. }
        | ElementKind::Path { fill, stroke, .. } => {
            if fill[3] > 0 {
                *fill
            } else if stroke[3] > 0 {
                *stroke
            } else {
                return None;
            }
        }
        ElementKind::PaintedPath {
            fill,
            stroke,
            fill_gradient,
            stroke_gradient,
            ..
        } => {
            let local = crate::stage::inverse(item.transform, stage_pt)?;
            if let Some(grad) = fill_gradient {
                let c = grad.sample(local.x as f64, local.y as f64);
                if c[3] > 0 {
                    c
                } else if stroke[3] > 0 {
                    *stroke
                } else {
                    return None;
                }
            } else if fill[3] > 0 {
                *fill
            } else if let Some(grad) = stroke_gradient {
                let c = grad.sample(local.x as f64, local.y as f64);
                if c[3] > 0 {
                    c
                } else {
                    return None;
                }
            } else if stroke[3] > 0 {
                *stroke
            } else {
                return None;
            }
        }
        ElementKind::Text { color, .. } => *color,
        ElementKind::TextBox { format, .. } => {
            let [_, r, g, b] = format.color.to_be_bytes();
            [r, g, b, 255]
        }
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            ..
        } => {
            let local = crate::stage::inverse(item.transform, stage_pt)?;
            let px = sample_bitmap_pixel(
                ctx,
                project,
                resource_id,
                local,
                f64::from(*width),
                f64::from(*height),
            )?;
            if px[3] == 0 {
                return None;
            }
            px
        }
        ElementKind::Video { .. } | ElementKind::Instance { .. } => return None,
    };
    let mut transformed = item.color_transform.apply(base);
    transformed[3] = (transformed[3] as f64 * item.alpha)
        .round()
        .clamp(0.0, 255.0) as u8;
    (transformed[3] > 0).then_some(transformed)
}

impl Editor {
    /// Union world bounding box of the current selection (`(bounds, any_editable)`).
    pub(crate) fn selection_info_bounds(&self, ctx: &egui::Context) -> Option<(Rect, bool)> {
        if self.selection.is_empty() {
            return None;
        }
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut by_id = HashMap::<String, Rect>::new();
        for item in evaluate(&self.project, self.symbol_id(), self.frame) {
            if let Some(id) = item.instance_path.first() {
                let r = crate::stage::world_bounds(&item, &painter);
                if r.is_finite() {
                    by_id
                        .entry(id.clone())
                        .and_modify(|acc| *acc = acc.union(r))
                        .or_insert(r);
                }
            }
        }
        let mut union = Rect::NOTHING;
        let mut editable = false;
        for (target, element) in self.sampled_targets() {
            let rect = by_id.get(&element.id).copied().unwrap_or_else(|| {
                Rect::from_min_size(
                    Pos2::new(element.transform.x as f32, element.transform.y as f32),
                    Vec2::ZERO,
                )
            });
            union = union.union(rect);
            editable |= self.target_editable(&target);
        }
        union.is_finite().then_some((union, editable))
    }

    /// Sample the stage coordinates and RGBA colour at a stage-space point `stage_pt`.
    pub(crate) fn sample_stage_color_at(&self, ctx: &egui::Context, stage_pt: Pos2) -> Color {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let scene = evaluate(&self.project, self.symbol_id(), self.frame);
        for item in scene.iter().rev() {
            if crate::stage::hit_item(item, &painter, stage_pt, self.zoom.max(0.01))
                && let Some(c) = sample_draw_item(ctx, &self.project, item, stage_pt)
            {
                return c;
            }
        }
        self.project.background
    }

    pub(crate) fn apply_info_bounds_edit(
        &mut self,
        old_bounds: Rect,
        new_x: f64,
        new_y: f64,
        new_w: f64,
        new_h: f64,
    ) {
        let dx = new_x - old_bounds.min.x as f64;
        let dy = new_y - old_bounds.min.y as f64;
        let old_w = old_bounds.width() as f64;
        let old_h = old_bounds.height() as f64;
        let w_changed = (new_w - old_w).abs() > 1e-3 && old_w > 1e-3;
        let h_changed = (new_h - old_h).abs() > 1e-3 && old_h > 1e-3;
        let sx = if w_changed {
            (new_w.max(0.1)) / old_w
        } else {
            1.0
        };
        let sy = if h_changed {
            (new_h.max(0.1)) / old_h
        } else {
            1.0
        };
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .collect();
        if targets.is_empty() {
            return;
        }
        let anchor_x = old_bounds.min.x as f64;
        let anchor_y = old_bounds.min.y as f64;
        let single = targets.len() == 1;
        for (target, mut element) in targets {
            let mut resized_intrinsic = false;
            if single
                && (w_changed || h_changed)
                && element.transform.rotation == 0.0
                && element.transform.skew == 0.0
            {
                let scx = element.transform.scale_x.abs().max(1e-6);
                let scy = element.transform.scale_y.abs().max(1e-6);
                match &mut element.kind {
                    ElementKind::Rectangle { width, height, .. }
                    | ElementKind::Ellipse { width, height, .. }
                    | ElementKind::Video { width, height }
                    | ElementKind::TextBox { width, height, .. } => {
                        if w_changed {
                            *width = (new_w.max(1.0) / scx).clamp(1.0, 100_000.0);
                        }
                        if h_changed {
                            *height = (new_h.max(1.0) / scy).clamp(1.0, 100_000.0);
                        }
                        resized_intrinsic = true;
                    }
                    _ => {}
                }
            }
            if !resized_intrinsic && (w_changed || h_changed) {
                element.transform.x = anchor_x + (element.transform.x - anchor_x) * sx;
                element.transform.y = anchor_y + (element.transform.y - anchor_y) * sy;
                element.transform.scale_x *= sx;
                element.transform.scale_y *= sy;
                if element.transform.skew != 0.0 || !element.transform.color_transform.is_identity()
                {
                    self.project.format_version = self.project.format_version.max(5);
                }
            }
            element.transform.x += dx;
            element.transform.y += dy;

            self.ensure_key_for(&target.layer, false);
            let frame = self.frame;
            if let Some(layer) = self
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == target.layer)
                && let Some(el) = layer
                    .key_at_mut(frame)
                    .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
            {
                el.transform = element.transform;
                el.kind = element.kind;
            }
        }
    }

    pub(crate) fn info_panel_ui(&mut self, ui: &mut Ui) {
        self.panel_title(ui, PanelId::Info, "Info");
        let ctx = ui.ctx().clone();
        let units = self.stage_view.prefs.units;
        let px_per_unit = units.pixels() as f64;
        let selection_bounds = self.selection_info_bounds(&ctx);
        let editable = selection_bounds.is_some_and(|(_, ed)| ed);
        let (mut w, mut h, mut x, mut y) = match selection_bounds {
            Some((b, _)) => (
                b.width() as f64 / px_per_unit,
                b.height() as f64 / px_per_unit,
                b.min.x as f64 / px_per_unit,
                b.min.y as f64 / px_per_unit,
            ),
            None => (0.0, 0.0, 0.0, 0.0),
        };
        let (old_w, old_h, old_x, old_y) = (w, h, x, y);

        let scope = egui::Id::new((
            "info-panel-bounds",
            &self.clipboard.session,
            self.symbol_id(),
            self.frame,
        ));
        self.property_edit_scope(scope);

        let half = ((ui.available_width() - space::S2) * 0.5).max(60.0);
        let mut edited_control = None;
        ui.add_enabled_ui_semantic(editable, |ui| {
            ui.horizontal(|ui| {
                let rw = ui.add(
                    c::Field::new(&mut w, "Width")
                        .prefix("W")
                        .decimals(1)
                        .range(0.1..=100_000.0)
                        .width(half),
                );
                if rw.changed() {
                    edited_control = Some((rw.id, "Resize"));
                }
                let rh = ui.add(
                    c::Field::new(&mut h, "Height")
                        .prefix("H")
                        .decimals(1)
                        .range(0.1..=100_000.0)
                        .width(half),
                );
                if rh.changed() {
                    edited_control = Some((rh.id, "Resize"));
                }
            });
            ui.horizontal(|ui| {
                let rx = ui.add(
                    c::Field::new(&mut x, "X")
                        .prefix("X")
                        .decimals(1)
                        .range(-1_000_000.0..=1_000_000.0)
                        .width(half),
                );
                if rx.changed() {
                    edited_control = Some((rx.id, "Move"));
                }
                let ry = ui.add(
                    c::Field::new(&mut y, "Y")
                        .prefix("Y")
                        .decimals(1)
                        .range(-1_000_000.0..=1_000_000.0)
                        .width(half),
                );
                if ry.changed() {
                    edited_control = Some((ry.id, "Move"));
                }
            });
        });

        if editable
            && let Some((bounds, _)) = selection_bounds
            && let Some((control, label)) = edited_control
            && ((w - old_w).abs() > 1e-4
                || (h - old_h).abs() > 1e-4
                || (x - old_x).abs() > 1e-4
                || (y - old_y).abs() > 1e-4)
        {
            self.begin_property_edit(scope, control);
            self.history.set_label(label);
            self.apply_info_bounds_edit(
                bounds,
                x * px_per_unit,
                y * px_per_unit,
                w * px_per_unit,
                h * px_per_unit,
            );
        }
        self.end_property_interaction(&ctx);

        let canvas = self.stage_view.canvas;
        let pointer = ctx
            .input(|i| i.pointer.latest_pos())
            .filter(|pos| canvas.is_finite() && canvas.contains(*pos));
        let (cursor_text, color_text) = match pointer {
            Some(pos) => {
                let stage_pt = Pos2::new(
                    (pos.x - self.stage_view.origin.x) / self.zoom.max(0.01),
                    (pos.y - self.stage_view.origin.y) / self.zoom.max(0.01),
                );
                let ux = (stage_pt.x as f64 - self.stage_view.zero[0]) / px_per_unit;
                let uy = (stage_pt.y as f64 - self.stage_view.zero[1]) / px_per_unit;
                let coords = match units {
                    RulerUnits::Pixels | RulerUnits::Points => {
                        format!("X {:.0}   Y {:.0}", ux.round(), uy.round())
                    }
                    _ => format!("X {ux:.1}   Y {uy:.1}"),
                };
                let [r, g, b, a] = self.sample_stage_color_at(&ctx, stage_pt);
                let alpha_pct = ((a as f32 / 255.0) * 100.0).round() as u32;
                (coords, format!("R {r}  G {g}  B {b}  A {alpha_pct}%"))
            }
            None => ("—".to_owned(), "—".to_owned()),
        };

        for (label, value) in [
            ("Cursor", cursor_text.as_str()),
            ("Colour", color_text.as_str()),
        ] {
            ui.horizontal(|ui| {
                form_label(ui, label, 64.0);
                mono(ui, value);
            });
        }
    }
}
