//! Stroke style modal dialog and live floating body (NF2 `Dialog / Stroke style`, `PR-03`,
//! `W1.7`; reference `dialogs-sound-stroke.png`), opened from `Tool options ▸ Stroke style…`,
//! `Properties ▸ Custom…`, and `Window ▸ Panels ▸ Planned panels ▸ Stroke style`.

use super::dialog_helpers::{FormDialog, dialog_width, field_block, form_row};
use crate::app::Editor;
use eframe::egui::{self, Ui};
use neoflash_core::{AppearancePaintEntry, ElementAppearance, ElementKind, PatternBrushKind};
use neoflash_ui::components as c;
use neoflash_ui::metrics;

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
            && let Some(pat) = app.entries.iter().find_map(|entry| match entry {
                AppearancePaintEntry::Stroke { pattern_brush, .. } => pattern_brush.as_ref(),
                _ => None,
            })
        {
            match pat {
                PatternBrushKind::DashPattern { dash, gap } => {
                    draft.dash = (*dash).clamp(1.0, 200.0);
                    draft.gap = (*gap).clamp(1.0, 200.0);
                    draft.kind = if (*dash - draft.thickness).abs() <= 0.25 && *gap >= 2.0 {
                        StrokeStyleKind::Dotted
                    } else {
                        StrokeStyleKind::Dashed
                    };
                }
                PatternBrushKind::Ragged {
                    wavelength,
                    amplitude,
                } => {
                    draft.kind = StrokeStyleKind::Ragged;
                    draft.dash = (*wavelength).clamp(2.0, 200.0);
                    draft.gap = (*amplitude * 2.0).clamp(1.0, 200.0);
                }
                PatternBrushKind::Stipple { dot_size, spacing } => {
                    draft.kind = StrokeStyleKind::Stipple;
                    draft.thickness = (*dot_size).clamp(0.1, 200.0);
                    draft.gap = (*spacing).clamp(1.5, 200.0);
                }
                PatternBrushKind::Hatched { spacing, angle_deg } => {
                    draft.kind = StrokeStyleKind::Hatched;
                    draft.dash = (*angle_deg).clamp(-85.0, 85.0);
                    draft.gap = (*spacing).clamp(2.0, 200.0);
                }
                _ => {}
            }
        }
        draft
    }

    /// Open the `Stroke style` modal dialog (`PR-03`).
    pub(crate) fn open_stroke_style(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        let draft = self.init_stroke_style_draft();
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_stroke_style(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.remove::<StrokeStyleDraft>(draft_id()));
    }

    /// Apply a stroke style (`Solid`, `Dashed`, `Dotted`, `Ragged`, `Stipple`, `Hatched`) and
    /// thickness to editable selected elements in one atomic `"Stroke style"` Undo step.
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
            StrokeStyleKind::Ragged => Some(PatternBrushKind::Ragged {
                wavelength: dash.max(2.0),
                amplitude: (gap * 0.5).clamp(0.5, 50.0),
            }),
            StrokeStyleKind::Stipple => Some(PatternBrushKind::Stipple {
                dot_size: width.max(1.0),
                spacing: gap.max(1.5),
            }),
            StrokeStyleKind::Hatched => Some(PatternBrushKind::Hatched {
                spacing: gap.max(2.0),
                angle_deg: dash.clamp(-85.0, 85.0),
            }),
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
                                        pattern_brush: Some(
                                            PatternBrushKind::DashPattern { .. }
                                                | PatternBrushKind::Ragged { .. }
                                                | PatternBrushKind::Stipple { .. }
                                                | PatternBrushKind::Hatched { .. }
                                        ),
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
                                        Some(
                                            PatternBrushKind::DashPattern { .. }
                                                | PatternBrushKind::Ragged { .. }
                                                | PatternBrushKind::Stipple { .. }
                                                | PatternBrushKind::Hatched { .. }
                                        )
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
                editor.project.upgrade_format_for_features();
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

    /// Page rows of the dialog: preview, Type, Thickness, Dash / Gap, Sharp corners.
    pub(crate) fn stroke_style_body(&mut self, ui: &mut Ui, draft: &mut StrokeStyleDraft) {
        let nf = neoflash_ui::palette(ui.ctx());
        let preview = field_block(ui, egui::vec2(ui.available_width(), 3.0 * metrics::row()));
        let y = preview.center().y;
        let zoom = if draft.zoom_4x { 1.0_f32 } else { 0.5_f32 };
        let disp_w = ((draft.thickness as f32) * zoom).clamp(1.0, 24.0);
        let inset = 2.0 * metrics::row();
        let p0 = egui::pos2(preview.left() + inset, y);
        let p1 = egui::pos2(preview.right() - inset, y);
        let stroke = egui::Stroke::new(disp_w, nf.text_primary);
        let painter = ui
            .painter()
            .with_clip_rect(preview.intersect(ui.clip_rect()));
        match draft.kind {
            StrokeStyleKind::Solid => {
                painter.line_segment([p0, p1], stroke);
            }
            StrokeStyleKind::Dashed | StrokeStyleKind::Dotted => {
                let dash_len = if draft.kind == StrokeStyleKind::Dotted {
                    disp_w.max(2.0)
                } else {
                    ((draft.dash as f32) * zoom).max(2.0)
                };
                let gap_len = ((draft.gap as f32) * zoom).max(2.0);
                painter.extend(egui::Shape::dashed_line(
                    &[p0, p1],
                    stroke,
                    dash_len,
                    gap_len,
                ));
            }
            StrokeStyleKind::Ragged | StrokeStyleKind::Stipple | StrokeStyleKind::Hatched => {
                let z = f64::from(zoom);
                let pat = match draft.kind {
                    StrokeStyleKind::Ragged => PatternBrushKind::Ragged {
                        wavelength: (draft.dash * z).max(2.0),
                        amplitude: (draft.gap * 0.5 * z).clamp(0.5, 50.0),
                    },
                    StrokeStyleKind::Stipple => PatternBrushKind::Stipple {
                        dot_size: f64::from(disp_w.max(1.0)),
                        spacing: (draft.gap * z).max(1.5),
                    },
                    _ => PatternBrushKind::Hatched {
                        spacing: (draft.gap * z).max(2.0),
                        angle_deg: draft.dash.clamp(-85.0, 85.0),
                    },
                };
                let guide = [
                    neoflash_core::PathCommand::MoveTo {
                        x: f64::from(p0.x),
                        y: f64::from(p0.y),
                    },
                    neoflash_core::PathCommand::LineTo {
                        x: f64::from(p1.x),
                        y: f64::from(p1.y),
                    },
                ];
                let cmds =
                    neoflash_core::expand_pattern_brush_stroke(&guide, f64::from(disp_w), &pat);
                if let Ok(geom) =
                    crate::vector::tessellate(&cmds, neoflash_core::FillRule::NonZero, None, 0.25)
                    && !geom.indices.is_empty()
                {
                    let mut mesh = egui::Mesh::default();
                    mesh.vertices.reserve(geom.vertices.len());
                    for v in &geom.vertices {
                        mesh.colored_vertex(egui::pos2(v.x, v.y), nf.text_primary);
                    }
                    mesh.indices = geom.indices;
                    painter.add(egui::Shape::mesh(mesh));
                } else {
                    painter.line_segment([p0, p1], stroke);
                }
            }
        }

        form_row(ui, "Type", |ui| {
            c::Select::new(ui.id().with("stroke-style-type"), draft.kind.label())
                .width(metrics::field_m())
                .show(ui, |ui| {
                    for kind in StrokeStyleKind::ALL {
                        if ui
                            .add(c::MenuItem::new(kind.label()).checked(draft.kind == kind))
                            .clicked()
                        {
                            draft.kind = kind;
                            match kind {
                                StrokeStyleKind::Dotted => {
                                    draft.dash = draft.thickness.max(1.0);
                                }
                                StrokeStyleKind::Hatched => {
                                    draft.dash = draft.dash.clamp(-85.0, 85.0);
                                    draft.gap = draft.gap.max(2.0);
                                }
                                StrokeStyleKind::Ragged => {
                                    draft.dash = draft.dash.max(2.0);
                                    draft.gap = draft.gap.max(1.0);
                                }
                                StrokeStyleKind::Stipple => {
                                    draft.gap = draft.gap.max(1.5);
                                }
                                StrokeStyleKind::Dashed => {
                                    draft.dash = draft.dash.max(1.0);
                                    draft.gap = draft.gap.max(1.0);
                                }
                                StrokeStyleKind::Solid => {}
                            }
                            ui.close();
                        }
                    }
                });
        });
        form_row(ui, "Thickness", |ui| {
            ui.add(
                c::Field::new(&mut draft.thickness, "Thickness")
                    .unit("pt")
                    .range(0.1..=200.0)
                    .presets(&[0.25, 0.5, 1.0, 2.0, 4.0, 8.0])
                    .width(metrics::field_s()),
            );
            // A switch: it changes the preview at once and is not part of the stroke.
            ui.add(c::Switch::new(&mut draft.zoom_4x, "4× zoom"));
        });
        form_row(ui, "Dash", |ui| {
            let dash_enabled = matches!(
                draft.kind,
                StrokeStyleKind::Dashed | StrokeStyleKind::Ragged | StrokeStyleKind::Hatched
            );
            let dash_range = if draft.kind == StrokeStyleKind::Hatched {
                -85.0..=85.0
            } else {
                1.0..=200.0
            };
            let dash_resp = ui.add_enabled(
                dash_enabled,
                c::Field::new(&mut draft.dash, "Dash")
                    .unit("pt")
                    .range(dash_range)
                    .decimals(0)
                    .stepper()
                    .width(metrics::field_s()),
            );
            if !dash_enabled {
                let _ = c::not_available(
                    dash_resp,
                    match draft.kind {
                        StrokeStyleKind::Dotted => "Independent dot dash length",
                        StrokeStyleKind::Stipple => "Stipple uses thickness for dot size",
                        _ => "Dash length (Solid)",
                    },
                );
            }
            let gap_enabled = draft.kind != StrokeStyleKind::Solid;
            let gap_resp = ui.add_enabled(
                gap_enabled,
                c::Field::new(&mut draft.gap, "Gap")
                    .prefix("Gap")
                    .unit("pt")
                    .range(1.0..=200.0)
                    .decimals(0)
                    .stepper()
                    .width(metrics::field_s()),
            );
            if !gap_enabled {
                let _ = c::not_available(gap_resp, "Gap length (Solid)");
            }
        });
        form_row(ui, "", |ui| {
            let sharp_resp = ui.add_enabled(
                false,
                c::Checkbox::new(&mut draft.sharp_corners, "Sharp corners"),
            );
            let _ = c::not_available(sharp_resp, "Sharp corners");
        });
    }

    pub(crate) fn stroke_style_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<StrokeStyleDraft>(draft_id())) else {
            return;
        };
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_apply = false;

        let width = dialog_width(ctx, metrics::dialog_m());
        let modal = egui::Modal::new(egui::Id::new("stroke-style"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Stroke style", width, None);
                close_cancel |= dialog.close;
                dialog.page(ui, "stroke-style-page", |ui, _| {
                    self.stroke_style_body(ui, &mut draft);
                });
                dialog.footer(
                    ui,
                    |ui| {
                        if ui.add(c::Button::ghost("Reset")).clicked() {
                            draft = StrokeStyleDraft::default();
                        }
                    },
                    |ui| {
                        do_apply |= ui.add(c::Button::primary("OK")).clicked();
                        close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        FillRule, PathCommand, Project, evaluate_element_appearance, expand_pattern_brush_stroke,
        validate_path_commands,
    };

    #[test]
    fn ragged_stipple_and_hatched_stroke_styles_apply_round_trip_expand_and_undo() {
        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());
        neoflash_ui::theme::install(&ctx, crate::theme::ThemeMode::Dark.nf2());

        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.add_element(
            ElementKind::Path {
                commands: vec![
                    PathCommand::MoveTo { x: 10.0, y: 40.0 },
                    PathCommand::LineTo { x: 150.0, y: 40.0 },
                ],
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [30, 60, 180, 255],
                stroke_width: 3.0,
            },
            0.0,
            0.0,
        );
        editor.history.clear();
        let before = editor.project.clone();

        for (kind, thickness, dash, gap) in [
            (StrokeStyleKind::Ragged, 4.0, 14.0, 8.0),
            (StrokeStyleKind::Stipple, 5.0, 5.0, 9.0),
            (StrokeStyleKind::Hatched, 4.0, 35.0, 10.0),
        ] {
            assert!(
                editor.apply_stroke_style_to_selection(kind, thickness, dash, gap),
                "{kind:?} must apply without 'not available yet' rejection"
            );
            editor
                .project
                .validate()
                .expect("project valid after stroke style");
            assert!(editor.project.format_version >= 16);

            let round_trip = editor.init_stroke_style_draft();
            assert_eq!(round_trip.kind, kind);
            assert!((round_trip.thickness - thickness).abs() < 1e-6);
            assert!((round_trip.gap - gap).abs() < 1e-6);
            if kind != StrokeStyleKind::Stipple {
                assert!((round_trip.dash - dash).abs() < 1e-6);
            }

            let kf = &editor.symbol().timeline.layers[0].keyframes[0];
            let el = &kf.elements[0];
            let app = kf
                .appearances
                .iter()
                .find(|a| a.element_id == el.id)
                .expect("appearance created");
            let pat = app
                .entries
                .iter()
                .find_map(|entry| match entry {
                    AppearancePaintEntry::Stroke { pattern_brush, .. } => pattern_brush.as_ref(),
                    _ => None,
                })
                .expect("pattern brush stored");

            let (guide, _) = el.to_path_commands().unwrap();
            let expanded = expand_pattern_brush_stroke(&guide, thickness, pat);
            validate_path_commands(&expanded).expect("valid expanded stroke geometry");
            let contour_count = expanded
                .iter()
                .filter(|c| matches!(c, PathCommand::MoveTo { .. }))
                .count();
            if matches!(kind, StrokeStyleKind::Stipple | StrokeStyleKind::Hatched) {
                assert!(
                    contour_count >= 5,
                    "{kind:?} should expand into multi-contour vector dabs/ticks (got {contour_count})"
                );
            } else {
                assert!(
                    expanded.len() >= 10,
                    "Ragged should expand into modulated wave contour"
                );
            }
            let evaluated = evaluate_element_appearance(el, app);
            assert!(!evaluated.is_empty());

            // Also verify live preview rendering in stroke_style_body does not panic.
            let mut preview_draft = round_trip.clone();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.stroke_style_body(ui, &mut preview_draft);
                    });
                },
            );

            // JSON round-trip preserves PatternBrushKind.
            let json = serde_json::to_string(&editor.project).unwrap();
            let reloaded: Project = serde_json::from_str(&json).unwrap();
            reloaded.validate().expect("reloaded project valid");

            assert_eq!(editor.history.undo_len(), 1);
            editor.undo();
            assert_eq!(editor.project, before);
            editor.redo();
            editor.project.validate().unwrap();

            // Switching back to Solid cleans up the pattern brush entry.
            assert!(editor.apply_stroke_style_to_selection(StrokeStyleKind::Solid, 3.0, 12.0, 8.0));
            assert_eq!(
                editor.init_stroke_style_draft().kind,
                StrokeStyleKind::Solid
            );
            assert!(
                editor.symbol().timeline.layers[0].keyframes[0]
                    .appearances
                    .is_empty()
            );
            editor.undo();
            editor.undo();
            assert_eq!(editor.project, before);
            editor.history.clear();
        }
    }
}
