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
    #[cfg(test)]
    pub(crate) fn init_stroke_style_draft(&self) -> StrokeStyleDraft {
        let mut draft = StrokeStyleDraft {
            thickness: self.stroke_width.clamp(0.1, 200.0),
            kind: StrokeStyleKind::Solid,
            ..StrokeStyleDraft::default()
        };
        if let Some((_, el)) = self.sampled_targets().into_iter().next()
            && let Some(sw) = match &el.kind {
                ElementKind::Rectangle { stroke_width, .. }
                | ElementKind::Ellipse { stroke_width, .. }
                | ElementKind::Path { stroke_width, .. }
                | ElementKind::PaintedPath { stroke_width, .. } => Some(*stroke_width),
                _ => None,
            }
            && sw > 0.0
        {
            draft.thickness = sw.clamp(0.1, 200.0);
        }
        if let Some(target) = self.selection.single()
            && let Some(layer) = self
                .symbol()
                .timeline
                .layers
                .iter()
                .find(|l| l.id == target.layer)
            && let Some(key) = layer.key_at(self.frame)
            && let Some(app) = key
                .appearances
                .iter()
                .find(|a| a.element_id == target.element)
            && let Some(PatternBrushKind::DashPattern { dash, gap }) =
                app.entries.iter().find_map(|entry| match entry {
                    AppearancePaintEntry::Stroke { pattern_brush, .. } => pattern_brush.as_ref(),
                    _ => None,
                })
        {
            draft.dash = (*dash).clamp(1.0, 200.0);
            draft.gap = (*gap).clamp(1.0, 200.0);
            draft.kind = if (*dash - draft.thickness).abs() <= 0.25 && *gap >= 2.0 {
                StrokeStyleKind::Dotted
            } else {
                StrokeStyleKind::Dashed
            };
        }
        draft
    }

    /// Open the `Stroke style` modal dialog (`PR-03`).
    #[cfg(test)]
    pub(crate) fn open_stroke_style(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let draft = self.init_stroke_style_draft();
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_stroke_style(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.remove::<StrokeStyleDraft>(draft_id()));
    }

    /// Apply the active `Stroke style` draft to the tool stroke width and any editable selected
    /// elements in one `"Stroke style"` Undo step.
    /// Apply a supported stroke style (`Solid`, `Dashed`, `Dotted`) and thickness to editable
    /// selected elements in one atomic `"Stroke style"` Undo step.
    pub(crate) fn apply_stroke_style_to_selection(
        &mut self,
        kind: StrokeStyleKind,
        thickness: f64,
        dash: f64,
        gap: f64,
    ) -> bool {
        let width = thickness.clamp(0.1, 200.0);
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

        let pattern = match kind {
            StrokeStyleKind::Solid => None,
            StrokeStyleKind::Dotted => Some(PatternBrushKind::DashPattern {
                dash: width.max(1.0),
                gap: gap.max(2.0),
            }),
            StrokeStyleKind::Dashed => Some(PatternBrushKind::DashPattern {
                dash: dash.max(1.0),
                gap: gap.max(1.0),
            }),
            StrokeStyleKind::Ragged | StrokeStyleKind::Stipple | StrokeStyleKind::Hatched => {
                self.message(format!(
                    "{} stroke style is not available yet.",
                    kind.label()
                ));
                return false;
            }
        };

        if !selected_targets.is_empty() {
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
                            if let Some(app) = key
                                .appearances
                                .iter_mut()
                                .find(|a| a.element_id == target.element)
                            {
                                let mut updated_stroke = false;
                                for entry in &mut app.entries {
                                    if let AppearancePaintEntry::Stroke {
                                        color,
                                        width: entry_w,
                                        pattern_brush,
                                        ..
                                    } = entry
                                    {
                                        *color = stroke_col;
                                        *entry_w = width;
                                        *pattern_brush = Some(pat.clone());
                                        updated_stroke = true;
                                    }
                                }
                                if !updated_stroke {
                                    app.entries.push(AppearancePaintEntry::Stroke {
                                        color: stroke_col,
                                        gradient: None,
                                        width,
                                        swatch_id: None,
                                        offset_distance: 0.0,
                                        pattern_brush: Some(pat),
                                        opacity: 1.0,
                                    });
                                }
                            } else {
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
                            }
                        } else if let Some(pos) = key
                            .appearances
                            .iter()
                            .position(|a| a.element_id == target.element)
                        {
                            let app = &mut key.appearances[pos];
                            app.entries.retain(|entry| {
                                !matches!(
                                    entry,
                                    AppearancePaintEntry::Stroke {
                                        pattern_brush: Some(PatternBrushKind::DashPattern { .. }),
                                        swatch_id: None,
                                        gradient: None,
                                        offset_distance,
                                        ..
                                    } if *offset_distance == 0.0
                                )
                            });
                            for entry in &mut app.entries {
                                if let AppearancePaintEntry::Stroke {
                                    width: entry_w,
                                    pattern_brush,
                                    ..
                                } = entry
                                {
                                    *entry_w = width;
                                    if matches!(
                                        pattern_brush,
                                        Some(PatternBrushKind::DashPattern { .. })
                                    ) {
                                        *pattern_brush = None;
                                    }
                                }
                            }
                            if app.entries.is_empty()
                                && app.fill_swatch_id.is_none()
                                && app.stroke_swatch_id.is_none()
                                && app.style_id.is_none()
                                && app.modifiers.is_empty()
                                && app.text_binding.is_none()
                            {
                                key.appearances.remove(pos);
                            }
                        }
                    }
                }
            });
        }
        true
    }

    pub(crate) fn confirm_stroke_style(&mut self, ctx: &egui::Context) -> bool {
        let draft = ctx
            .data(|d| d.get_temp::<StrokeStyleDraft>(draft_id()))
            .or_else(|| ctx.data(|d| d.get_temp::<StrokeStyleDraft>(inline_draft_id())))
            .unwrap_or_default();
        if !self.apply_stroke_style_to_selection(draft.kind, draft.thickness, draft.dash, draft.gap)
        {
            return false;
        }
        let width = draft.thickness.clamp(0.1, 200.0);
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
                        let supported = matches!(
                            kind,
                            StrokeStyleKind::Solid
                                | StrokeStyleKind::Dashed
                                | StrokeStyleKind::Dotted
                        );
                        if supported {
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
                        } else {
                            let resp = ui.add_enabled(false, c::MenuItem::new(&lbl));
                            let _ = c::not_available(resp, kind.label());
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
            let dash_enabled = draft.kind == StrokeStyleKind::Dashed;
            let dash_resp = ui.add_enabled(
                dash_enabled,
                c::Field::new(&mut draft.dash, "Dash")
                    .prefix("Dash")
                    .unit("pt")
                    .range(1.0..=200.0)
                    .decimals(0)
                    .stepper()
                    .width(140.0),
            );
            if !dash_enabled {
                let _ = c::not_available(
                    dash_resp,
                    if draft.kind == StrokeStyleKind::Dotted {
                        "Independent dot dash length"
                    } else {
                        "Dash length (Solid)"
                    },
                );
            }
            let gap_enabled = matches!(
                draft.kind,
                StrokeStyleKind::Dashed | StrokeStyleKind::Dotted
            );
            let gap_resp = ui.add_enabled(
                gap_enabled,
                c::Field::new(&mut draft.gap, "Gap")
                    .prefix("Gap")
                    .unit("pt")
                    .range(1.0..=200.0)
                    .decimals(0)
                    .stepper()
                    .width(140.0),
            );
            if !gap_enabled {
                let _ = c::not_available(gap_resp, "Gap length (Solid)");
            }
            let sharp_resp = ui.add_enabled(
                false,
                c::Checkbox::new(&mut draft.sharp_corners, "Sharp corners"),
            );
            let _ = c::not_available(sharp_resp, "Sharp corners");
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

    pub(crate) fn stroke_style_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<StrokeStyleDraft>(draft_id())) else {
            return;
        };
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
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

        if !close_cancel && !do_apply {
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_apply = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
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
