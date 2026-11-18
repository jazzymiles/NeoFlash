//! Document settings modal dialog (NF2 `Dialog / Document settings`, DL-02 / W0.14; reference
//! `document-dialogs.png`), opened by `Document ▸ Document settings…` (`⌘J`).
//!
//! Edits live on a draft until confirmed: `Cancel` / `Esc` discards without touching the project,
//! `OK` / `Enter` commits stage size, optional content scaling / anchoring, units, background and
//! frame rate in a single Undo step (`"Document settings"`). `Make default` persists default stage
//! settings in `WorkspacePrefs::document_defaults`.
use super::dialog_helpers::{FormDialog, fixed_dialog_size, form_dialog_height, form_row};
use super::planned::caption;
use crate::app::Editor;
use crate::stage_view::RulerUnits;
use crate::theme::DesignUi as _;
use crate::workspace::DocumentDefaults;
use eframe::egui::{self, Color32, Rect};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::components::view_settings as vs;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::space;

#[derive(Clone, Debug)]
pub(crate) struct DocumentSettingsDraft {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) constrain: bool,
    pub(crate) aspect: f64,
    pub(crate) scale_content: bool,
    pub(crate) anchor: c::RegistrationPoint,
    pub(crate) units: RulerUnits,
    pub(crate) background: Color32,
    pub(crate) fps: f64,
    pub(crate) script_language: ScriptLanguage,
    pub(crate) also_compile_compatible_scripts: bool,
    /// Translation from `Match contents` so union `min` lands at `(0, 0)` on `OK`.
    pub(crate) match_shift: Option<(f64, f64)>,
}

fn anchor_factors(point: c::RegistrationPoint) -> (f64, f64) {
    use c::RegistrationPoint as R;
    match point {
        R::TopLeft => (0.0, 0.0),
        R::Top => (0.5, 0.0),
        R::TopRight => (1.0, 0.0),
        R::Left => (0.0, 0.5),
        R::Center => (0.5, 0.5),
        R::Right => (1.0, 0.5),
        R::BottomLeft => (0.0, 1.0),
        R::Bottom => (0.5, 1.0),
        R::BottomRight => (1.0, 1.0),
    }
}

fn root_scene_ids(project: &Project) -> Vec<Id> {
    if project.scenes.is_empty() {
        vec![project.root.clone()]
    } else {
        project.scenes.clone()
    }
}

fn units_label(units: RulerUnits) -> &'static str {
    let idx = RulerUnits::ALL
        .iter()
        .position(|u| *u == units)
        .unwrap_or(0);
    // The row label names the control; the select shows the value only.
    vs::UNITS[idx]
        .strip_prefix("Units: ")
        .unwrap_or(vs::UNITS[idx])
}

/// One size for the three states of the dialog (normal, scripts in other languages, no
/// compatible language): eight rows and room for the two-line script message.
pub(crate) fn document_settings_size(ctx: &egui::Context) -> egui::Vec2 {
    fixed_dialog_size(
        ctx,
        metrics::dialog_m(),
        form_dialog_height(
            8,
            0,
            metrics::row_gap() + 2.0 * metrics::toggle_row() + space::s3(),
        ),
    )
}

impl Editor {
    /// Open the `Document settings` modal dialog (`Document ▸ Document settings…`, `⌘J`).
    pub(crate) fn open_document_settings(&mut self) {
        self.finish_edits();
        let w = self.project.width.max(1) as f64;
        let h = self.project.height.max(1) as f64;
        let [r, g, b, a] = self.project.background;
        self.document_settings = Some(DocumentSettingsDraft {
            width: w,
            height: h,
            constrain: false,
            aspect: w / h,
            scale_content: false,
            anchor: c::RegistrationPoint::TopLeft,
            units: self.stage_view.prefs.units,
            background: Color32::from_rgba_unmultiplied(r, g, b, a),
            fps: self.project.fps.clamp(1, 120) as f64,
            script_language: self.project.effective_script_language(),
            also_compile_compatible_scripts: self.project.also_compile_compatible_scripts,
            match_shift: None,
        });
    }

    fn total_root_frames(&self) -> u32 {
        root_scene_ids(&self.project)
            .into_iter()
            .filter_map(|id| self.project.symbol(&id))
            .map(|s| s.timeline.frame_count.max(1))
            .sum::<u32>()
            .max(1)
    }

    /// Union bounding box of all root-scene artwork across keyframes (`Match contents`).
    pub(crate) fn root_contents_bounds(&self, ctx: &egui::Context) -> Option<Rect> {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut union = Rect::NOTHING;
        for scene_id in root_scene_ids(&self.project) {
            let Some(symbol) = self.project.symbol(&scene_id) else {
                continue;
            };
            let mut frames: Vec<u32> = symbol
                .timeline
                .layers
                .iter()
                .flat_map(|l| l.keyframes.iter().map(|k| k.frame))
                .collect();
            frames.push(0);
            frames.push(symbol.timeline.frame_count.saturating_sub(1));
            frames.sort_unstable();
            frames.dedup();
            for frame in frames {
                for item in evaluate(&self.project, &scene_id, frame) {
                    let r = crate::stage::world_bounds(&item, &painter);
                    if r.is_finite() && (r.width() > 0.0 || r.height() > 0.0) {
                        union = union.union(r);
                    }
                }
            }
        }
        (union.is_finite() && (union.width() > 0.0 || union.height() > 0.0)).then_some(union)
    }

    pub(crate) fn commit_document_settings(&mut self, draft: DocumentSettingsDraft) {
        let new_w = (draft.width.round() as u32).clamp(1, 65535);
        let new_h = (draft.height.round() as u32).clamp(1, 65535);
        let new_fps = (draft.fps.round() as u32).clamp(1, 120);
        let new_bg = draft.background.to_srgba_unmultiplied();
        self.stage_view.prefs.units = draft.units;

        let old_w = self.project.width.max(1) as f64;
        let old_h = self.project.height.max(1) as f64;
        let size_changed = new_w != self.project.width || new_h != self.project.height;
        let script_changed = self.project.script_language != Some(draft.script_language)
            || self.project.also_compile_compatible_scripts
                != draft.also_compile_compatible_scripts;
        let doc_changed = size_changed
            || new_fps != self.project.fps
            || new_bg != self.project.background
            || script_changed
            || draft
                .match_shift
                .is_some_and(|(dx, dy)| dx.abs() > 1e-6 || dy.abs() > 1e-6);

        if !doc_changed {
            return;
        }

        self.transact_named("Document settings", |s| {
            let scenes = root_scene_ids(&s.project);
            if let Some((dx, dy)) = draft.match_shift
                && (dx.abs() > 1e-6 || dy.abs() > 1e-6)
            {
                for scene_id in &scenes {
                    if let Some(symbol) = s.project.symbol_mut(scene_id) {
                        for layer in &mut symbol.timeline.layers {
                            for key in &mut layer.keyframes {
                                for el in &mut key.elements {
                                    el.transform.x += dx;
                                    el.transform.y += dy;
                                }
                            }
                        }
                        for guide in &mut symbol.timeline.guides {
                            guide.position += match guide.axis {
                                GuideAxis::Horizontal => dy,
                                GuideAxis::Vertical => dx,
                            };
                        }
                    }
                }
            } else if size_changed {
                if draft.scale_content {
                    let sx = new_w as f64 / old_w;
                    let sy = new_h as f64 / old_h;
                    for scene_id in &scenes {
                        if let Some(symbol) = s.project.symbol_mut(scene_id) {
                            for layer in &mut symbol.timeline.layers {
                                for key in &mut layer.keyframes {
                                    for el in &mut key.elements {
                                        el.transform.x *= sx;
                                        el.transform.y *= sy;
                                        el.transform.scale_x *= sx;
                                        el.transform.scale_y *= sy;
                                    }
                                }
                            }
                            for guide in &mut symbol.timeline.guides {
                                guide.position *= match guide.axis {
                                    GuideAxis::Horizontal => sy,
                                    GuideAxis::Vertical => sx,
                                };
                            }
                        }
                    }
                } else {
                    let (ax, ay) = anchor_factors(draft.anchor);
                    let dx = (new_w as f64 - old_w) * ax;
                    let dy = (new_h as f64 - old_h) * ay;
                    if dx.abs() > 1e-6 || dy.abs() > 1e-6 {
                        for scene_id in &scenes {
                            if let Some(symbol) = s.project.symbol_mut(scene_id) {
                                for layer in &mut symbol.timeline.layers {
                                    for key in &mut layer.keyframes {
                                        for el in &mut key.elements {
                                            el.transform.x += dx;
                                            el.transform.y += dy;
                                        }
                                    }
                                }
                                for guide in &mut symbol.timeline.guides {
                                    guide.position += match guide.axis {
                                        GuideAxis::Horizontal => dy,
                                        GuideAxis::Vertical => dx,
                                    };
                                }
                            }
                        }
                    }
                }
            }

            s.project.width = new_w;
            s.project.height = new_h;
            s.project.fps = new_fps;
            s.project.background = new_bg;
            s.project.script_language = Some(draft.script_language);
            s.project.also_compile_compatible_scripts = draft.also_compile_compatible_scripts;
            s.accumulator = 0.0;
            s.last_tick = std::time::Instant::now();
        });
    }

    pub(crate) fn document_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.document_settings.clone() else {
            return;
        };
        let p = neoflash_ui::palette(ctx);
        let total_frames = self.total_root_frames();
        let mut close_cancel = false;
        let mut close_ok = false;
        let mut match_contents = false;
        let mut match_printer = false;
        let mut make_default = false;

        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let size = document_settings_size(ctx);
        let modal = egui::Modal::new(egui::Id::new("document-settings"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Document settings", size.x, Some(size.y));
                close_cancel |= dialog.close;
                dialog.page(ui, "document-settings-page", |ui, _| {
                    let (old_w, old_h) = (draft.width, draft.height);
                    form_row(ui, "Stage size", |ui| {
                        let rw = ui.add(
                            c::Field::new(&mut draft.width, "Stage width")
                                .prefix("W")
                                .unit("px")
                                .decimals(0)
                                .range(1.0..=65535.0)
                                .width(metrics::field_s()),
                        );
                        let rh = ui.add(
                            c::Field::new(&mut draft.height, "Stage height")
                                .prefix("H")
                                .unit("px")
                                .decimals(0)
                                .range(1.0..=65535.0)
                                .width(metrics::field_s()),
                        );
                        if ui
                            .add(
                                c::IconButton::new(i16::LINK, "Constrain proportions")
                                    .selected(draft.constrain),
                            )
                            .clicked()
                        {
                            draft.constrain = !draft.constrain;
                            if draft.constrain {
                                draft.aspect = draft.width.max(1.0) / draft.height.max(1.0);
                            }
                        }
                        if rw.changed() || rh.changed() {
                            draft.match_shift = None;
                            if draft.constrain && draft.aspect > 1e-6 {
                                if rw.changed() && (draft.width - old_w).abs() > 1e-6 {
                                    draft.height =
                                        (draft.width / draft.aspect).round().clamp(1.0, 65535.0);
                                } else if rh.changed() && (draft.height - old_h).abs() > 1e-6 {
                                    draft.width =
                                        (draft.height * draft.aspect).round().clamp(1.0, 65535.0);
                                }
                            } else {
                                draft.aspect = draft.width.max(1.0) / draft.height.max(1.0);
                            }
                        }
                    });

                    form_row(ui, "", |ui| {
                        if ui.add(c::Button::ghost("Match contents")).clicked() {
                            match_contents = true;
                        }
                        if ui.add(c::Button::ghost("Match printer")).clicked() {
                            match_printer = true;
                        }
                    });

                    form_row(ui, "Scale", |ui| {
                        ui.spacing_mut().item_spacing.x = space::s3();
                        ui.add(c::Checkbox::new(
                            &mut draft.scale_content,
                            "Scale content with stage",
                        ));
                        ui.add(c::RegistrationPicker {
                            point: &mut draft.anchor,
                        });
                    });

                    form_row(ui, "Units", |ui| {
                        c::Select::new("doc-settings-units", units_label(draft.units))
                            .width(metrics::field_m())
                            .show(ui, |ui| {
                                for unit in RulerUnits::ALL {
                                    let label = units_label(unit);
                                    if ui
                                        .add(c::MenuItem::new(label).checked(draft.units == unit))
                                        .clicked()
                                    {
                                        draft.units = unit;
                                        ui.close();
                                    }
                                }
                            });
                    });

                    form_row(ui, "Background", |ui| {
                        let cf = c::ColorField::new(&mut draft.background, "Background")
                            .width(metrics::field_l())
                            .show(ui);
                        super::dialog_helpers::track_rgba_popup(ui.ctx(), cf.response.id);
                        crate::paint_controls::show_color32_field_popup(
                            ui,
                            &cf.response,
                            cf.swatch_clicked,
                            "Background",
                            &mut draft.background,
                        );
                    });

                    form_row(ui, "Frame rate", |ui| {
                        ui.add(
                            c::Field::new(&mut draft.fps, "Frame rate")
                                .unit("fps")
                                .decimals(0)
                                .range(1.0..=120.0)
                                .stepper()
                                .width(metrics::field_s()),
                        );
                        let secs = total_frames as f64 / draft.fps.round().clamp(1.0, 120.0);
                        caption(
                            ui,
                            &format!(
                                "{total_frames} frame{} · {secs:.2} s",
                                if total_frames == 1 { "" } else { "s" }
                            ),
                        );
                    });

                    form_row(ui, "Script", |ui| {
                        c::Select::new(
                            "doc-settings-script",
                            draft.script_language.display_label(),
                        )
                        .width(metrics::field_l())
                        .show(ui, |ui| {
                            for lang in ScriptLanguage::PROJECT_ORDER {
                                if ui
                                    .add(
                                        c::MenuItem::new(lang.display_label())
                                            .checked(draft.script_language == lang),
                                    )
                                    .clicked()
                                {
                                    draft.script_language = lang;
                                    ui.close();
                                }
                            }
                        });
                    });

                    // The option keeps its row in every state; only its label and state change.
                    form_row(ui, "", |ui| {
                        if let Some(compat) = draft.script_language.compatible_language() {
                            let label = format!("Also compile {}", compat.display_label());
                            ui.add(c::Checkbox::new(
                                &mut draft.also_compile_compatible_scripts,
                                &label,
                            ));
                        } else {
                            let mut unchecked = false;
                            let resp = ui.add_enabled_semantic(
                                false,
                                c::Checkbox::new(
                                    &mut unchecked,
                                    "Also compile compatible languages",
                                ),
                            );
                            crate::ui_icons::hover_tooltip(
                                resp,
                                "",
                                None,
                                Some("ActionScript 3.0 has no compatible language"),
                            );
                        }
                    });

                    if let Some((is_warning, msg)) = self
                        .project
                        .foreign_script_counts_for(
                            draft.script_language,
                            draft.also_compile_compatible_scripts,
                        )
                        .count_message()
                    {
                        let w = ui.available_width();
                        let im = if is_warning {
                            c::InlineMessage::warning(&msg)
                        } else {
                            c::InlineMessage::info(&msg)
                        };
                        ui.add(im.width(w));
                    }
                });
                dialog.footer(
                    ui,
                    |ui| make_default |= ui.add(c::Button::ghost("Make default")).clicked(),
                    |ui| {
                        close_ok |= ui.add(c::Button::primary("OK")).clicked();
                        close_cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });

        if match_contents {
            if let Some(bounds) = self.root_contents_bounds(ctx) {
                draft.width = (bounds.width().ceil() as f64).clamp(1.0, 65535.0);
                draft.height = (bounds.height().ceil() as f64).clamp(1.0, 65535.0);
                draft.aspect = draft.width / draft.height;
                draft.match_shift = Some((-bounds.min.x as f64, -bounds.min.y as f64));
            } else {
                self.message("Stage has no content to match.");
            }
        }

        if match_printer {
            draft.width = 612.0;
            draft.height = 792.0;
            draft.aspect = draft.width / draft.height;
            draft.match_shift = None;
        }

        if make_default {
            let mut defaults = DocumentDefaults {
                width: (draft.width.round() as u32).clamp(1, 65535),
                height: (draft.height.round() as u32).clamp(1, 65535),
                fps: (draft.fps.round() as u32).clamp(1, 120),
                background: draft.background.to_srgba_unmultiplied(),
                units: draft.units,
                script_language: draft.script_language,
                also_compile_compatible_scripts: draft.also_compile_compatible_scripts,
            };
            defaults.sanitize();
            self.workspace.document_defaults = Some(defaults);
            self.message("Document settings saved as default.");
        }

        if !close_cancel && !close_ok {
            match kb.resolve(ctx, &modal, true) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => close_ok = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        if close_cancel {
            self.document_settings = None;
        } else if close_ok {
            self.document_settings = None;
            self.commit_document_settings(draft);
        } else {
            self.document_settings = Some(draft);
        }
    }
}
