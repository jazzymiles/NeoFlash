//! Stroke style modal dialog and live floating body (NF2 `Dialog / Stroke style`, `PR-03`,
//! `W1.7`; reference `dialogs-sound-stroke.png`), opened from `Tool options ▸ Stroke style…`,
//! `Properties ▸ Custom…`, and `Window ▸ Panels ▸ Planned panels ▸ Stroke style`.

use crate::app::Editor;
use eframe::egui::{self, Color32, Ui};
use neoflash_core::{AppearancePaintEntry, ElementAppearance, ElementKind, PatternBrushKind};
use neoflash_ui::components as c;
use neoflash_ui::components::color_picker::hsv;
use neoflash_ui::tokens::{radius, space};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StrokeStyleKind {
    Solid,
    #[default]
    Dashed,
    Dotted,
    Ragged,
    Stipple,
    Hatched,
}

impl StrokeStyleKind {
    pub(crate) const ALL: [Self; 6] = [
        Self::Solid,
        Self::Dashed,
        Self::Dotted,
        Self::Ragged,
        Self::Stipple,
        Self::Hatched,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
            Self::Ragged => "Ragged",
            Self::Stipple => "Stipple",
            Self::Hatched => "Hatched",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StrokeStyleDraft {
    pub(crate) kind: StrokeStyleKind,
    pub(crate) thickness: f64,
    pub(crate) zoom_4x: bool,
    pub(crate) dash: f64,
    pub(crate) gap: f64,
    pub(crate) sharp_corners: bool,
}

impl Default for StrokeStyleDraft {
    fn default() -> Self {
        Self {
            kind: StrokeStyleKind::Dashed,
            thickness: 4.0,
            zoom_4x: true,
            dash: 12.0,
            gap: 8.0,
            sharp_corners: false,
        }
    }
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-stroke-style-draft")
}

fn inline_draft_id() -> egui::Id {
    egui::Id::new("nf2-stroke-style-inline-draft")
}

pub(crate) fn has_stroke_style_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<StrokeStyleDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn stroke_style_draft(ctx: &egui::Context) -> Option<StrokeStyleDraft> {
    ctx.data(|d| d.get_temp::<StrokeStyleDraft>(draft_id()))
        .or_else(|| ctx.data(|d| d.get_temp::<StrokeStyleDraft>(inline_draft_id())))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn update_stroke_style_draft(
    ctx: &egui::Context,
    mut f: impl FnMut(&mut StrokeStyleDraft),
) {
    for id in [draft_id(), inline_draft_id()] {
        if let Some(mut draft) = ctx.data(|d| d.get_temp::<StrokeStyleDraft>(id)) {
            f(&mut draft);
            ctx.data_mut(|d| d.insert_temp(id, draft));
        }
    }
}

impl Editor {
    /// Open the `Stroke style` modal dialog (`PR-03`).
    pub(crate) fn open_stroke_style(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let mut draft = StrokeStyleDraft {
            thickness: self.stroke_width.clamp(0.25, 200.0),
            ..StrokeStyleDraft::default()
        };
        if let Some(el) = self.selected_element()
            && let Some(sw) = match &el.kind {
                ElementKind::Rectangle { stroke_width, .. }
                | ElementKind::Ellipse { stroke_width, .. }
                | ElementKind::Path { stroke_width, .. }
                | ElementKind::PaintedPath { stroke_width, .. } => Some(*stroke_width),
                _ => None,
            }
            && sw > 0.0
        {
            draft.thickness = sw;
        }
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_stroke_style(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.remove::<StrokeStyleDraft>(draft_id()));
    }

    /// Apply the active `Stroke style` draft to the tool stroke width and any editable selected
    /// elements in one `"Stroke style"` Undo step.
    pub(crate) fn confirm_stroke_style(&mut self, ctx: &egui::Context) -> bool {
        let draft = ctx
            .data(|d| d.get_temp::<StrokeStyleDraft>(draft_id()))
            .or_else(|| ctx.data(|d| d.get_temp::<StrokeStyleDraft>(inline_draft_id())))
            .unwrap_or_default();

        let width = draft.thickness.clamp(0.1, 200.0);
        self.stroke_width = width;
        let selected_targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter_map(|(t, el)| {
                let stroke_col = match &el.kind {
                    ElementKind::Rectangle { stroke, .. }
                    | ElementKind::Ellipse { stroke, .. }
                    | ElementKind::Path { stroke, .. }
                    | ElementKind::PaintedPath { stroke, .. } => *stroke,
                    _ => self.stroke,
                };
                self.target_editable(&t).then_some((t, stroke_col))
            })
            .collect();

        if !selected_targets.is_empty() {
            let pattern = match draft.kind {
                StrokeStyleKind::Solid => None,
                StrokeStyleKind::Dotted => Some(PatternBrushKind::DashPattern {
                    dash: width.max(1.0),
                    gap: draft.gap.max(2.0),
                }),
                _ => Some(PatternBrushKind::DashPattern {
                    dash: draft.dash.max(1.0),
                    gap: draft.gap.max(1.0),
                }),
            };
            self.finish_edits();
            self.transact_named("Stroke style", |editor| {
                let frame = editor.frame;
                for (target, stroke_col) in selected_targets {
                    editor.ensure_key_for(&target.layer, false);
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(frame)
                    {
                        if let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element) {
                            match &mut el.kind {
                                ElementKind::Rectangle { stroke_width, .. }
                                | ElementKind::Ellipse { stroke_width, .. }
                                | ElementKind::Path { stroke_width, .. }
                                | ElementKind::PaintedPath { stroke_width, .. } => {
                                    *stroke_width = width;
                                }
                                _ => {}
                            }
                        }
                        if let Some(pat) = pattern.clone() {
                            key.appearances.retain(|a| a.element_id != target.element);
                            key.appearances.push(ElementAppearance {
                                element_id: target.element.clone(),
                                fill_swatch_id: None,
                                stroke_swatch_id: None,
                                style_id: None,
                                entries: vec![AppearancePaintEntry::Stroke {
                                    color: stroke_col,
                                    gradient: None,
                                    width,
                                    swatch_id: None,
                                    offset_distance: 0.0,
                                    pattern_brush: Some(pat),
                                    opacity: 1.0,
                                }],
                                modifiers: Vec::new(),
                                text_binding: None,
                            });
                        } else {
                            key.appearances.retain(|a| a.element_id != target.element);
                        }
                    }
                }
            });
        }

        ctx.data_mut(|d| d.remove::<StrokeStyleDraft>(draft_id()));
        self.message(format!(
            "Applied {} stroke style ({:.1} pt).",
            draft.kind.label(),
            width
        ));
        true
    }

    pub(crate) fn stroke_style_body(
        &mut self,
        ui: &mut Ui,
        draft: &mut StrokeStyleDraft,
    ) -> (bool, bool) {
        let mut close_cancel = false;
        let mut do_apply = false;
        let nf = neoflash_ui::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = space::S3;

        let (preview, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 96.0), egui::Sense::hover());
        ui.painter().rect_filled(
            preview,
            egui::CornerRadius::same(radius::MD as u8),
            Color32::from(hsv(0.0, 0.0, 1.0, 1.0)),
        );
        ui.painter().rect_stroke(
            preview,
            egui::CornerRadius::same(radius::MD as u8),
            egui::Stroke::new(1.0_f32, nf.border_subtle),
            egui::StrokeKind::Inside,
        );
        let y = preview.center().y;
        let zoom = if draft.zoom_4x { 1.0_f32 } else { 0.5_f32 };
        let disp_w = ((draft.thickness as f32) * zoom).clamp(1.0, 24.0);
        let p0 = egui::pos2(preview.left() + 48.0, y);
        let p1 = egui::pos2(preview.right() - 48.0, y);
        let stroke = egui::Stroke::new(disp_w, Color32::from(hsv(0.0, 0.0, 0.0, 1.0)));
        if draft.kind == StrokeStyleKind::Solid {
            ui.painter().line_segment([p0, p1], stroke);
        } else {
            let dash_len = if draft.kind == StrokeStyleKind::Dotted {
                disp_w.max(2.0)
            } else {
                ((draft.dash as f32) * zoom).max(2.0)
            };
            let gap_len = ((draft.gap as f32) * zoom).max(2.0);
            ui.painter().extend(egui::Shape::dashed_line(
                &[p0, p1],
                stroke,
                dash_len,
                gap_len,
            ));
        }

        ui.horizontal(|ui| {
            let type_lbl = format!("Type: {}", draft.kind.label());
            c::Select::new(ui.id().with("stroke-style-type"), &type_lbl)
                .width(196.0)
                .show(ui, |ui| {
                    for kind in StrokeStyleKind::ALL {
                        let lbl = format!("Type: {}", kind.label());
                        if ui
                            .add(c::MenuItem::new(&lbl).checked(draft.kind == kind))
                            .clicked()
                        {
                            draft.kind = kind;
                            if kind == StrokeStyleKind::Dotted {
                                draft.dash = draft.thickness.max(1.0);
                            }
                            ui.close();
                        }
                    }
                });
            ui.add(
                c::Field::new(&mut draft.thickness, "Thickness")
                    .unit("pt")
                    .range(0.1..=200.0)
                    .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                    .width(108.0),
            );
            ui.add(c::Switch::new(&mut draft.zoom_4x, "4× zoom"));
        });
        ui.horizontal(|ui| {
            ui.add(
                c::Field::new(&mut draft.dash, "Dash")
                    .prefix("Dash")
                    .unit("pt")
                    .range(1.0..=200.0)
                    .decimals(0)
                    .stepper()
                    .width(140.0),
            );
            ui.add(
                c::Field::new(&mut draft.gap, "Gap")
                    .prefix("Gap")
                    .unit("pt")
                    .range(1.0..=200.0)
                    .decimals(0)
                    .stepper()
                    .width(140.0),
            );
            ui.add(c::Checkbox::new(&mut draft.sharp_corners, "Sharp corners"));
        });
        ui.horizontal(|ui| {
            if ui.add(c::Button::ghost("Reset")).clicked() {
                *draft = StrokeStyleDraft::default();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(c::Button::primary("OK")).clicked() {
                    do_apply = true;
                }
                if ui.add(c::Button::new("Cancel")).clicked() {
                    close_cancel = true;
                }
            });
        });
        (close_cancel, do_apply)
    }

    pub(crate) fn stroke_style_inline_panel(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut draft = ctx
            .data(|d| d.get_temp::<StrokeStyleDraft>(inline_draft_id()))
            .unwrap_or_default();
        let (close_cancel, do_apply) = self.stroke_style_body(ui, &mut draft);
        ctx.data_mut(|d| d.insert_temp(inline_draft_id(), draft));
        if do_apply {
            self.confirm_stroke_style(&ctx);
            return true;
        }
        close_cancel
    }

    pub(crate) fn stroke_style_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<StrokeStyleDraft>(draft_id())) else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_apply = false;

        let modal = egui::Modal::new(egui::Id::new("stroke-style"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(480.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Stroke style",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 480.0,
                    header_top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(480.0 - 2.0 * space::S5);
                        let (c, ok) = self.stroke_style_body(ui, &mut draft);
                        close_cancel |= c;
                        do_apply |= ok;
                    });
            });

        if modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close_cancel = true;
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));

        if close_cancel {
            self.cancel_stroke_style(ctx);
            return;
        }
        if do_apply {
            self.confirm_stroke_style(ctx);
        }
    }
}
