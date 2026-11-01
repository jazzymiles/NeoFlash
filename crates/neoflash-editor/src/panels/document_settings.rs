//! Document settings modal dialog (NF2 `Dialog / Document settings`, DL-02 / W0.14; reference
//! `document-dialogs.png`), opened by `Modify ▸ Document…` (`⌘J`).
//!
//! Edits live on a draft until confirmed: `Cancel` / `Esc` discards without touching the project,
//! `OK` / `Enter` commits stage size, optional content scaling / anchoring, units, background and
//! frame rate in a single Undo step (`"Document settings"`). `Make default` persists default stage
//! settings in `WorkspacePrefs::document_defaults`.
use super::menu::NOT_AVAILABLE;
use super::planned::{FooterClick, caption, footer_clicked, form_label};
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
    vs::UNITS[idx]
}

fn primary_script_label(project: &Project) -> &'static str {
    let lang = project
        .sources
        .first()
        .map(|s| s.language)
        .or_else(|| {
            project
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.scripts)
                .next()
                .map(|s| s.script.language)
        })
        .unwrap_or(ScriptLanguage::ActionScript3);
    match lang {
        ScriptLanguage::ActionScript3 => "ActionScript 3.0",
        ScriptLanguage::ActionScript2 => "ActionScript 2.0",
        ScriptLanguage::ActionScript1 => "ActionScript 1.0",
        ScriptLanguage::TypeScript => "TypeScript",
        ScriptLanguage::JavaScript => "JavaScript",
    }
}

impl Editor {
    /// Open the `Document settings` modal dialog (`Modify ▸ Document…`, `⌘J`).
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
        let doc_changed = size_changed
            || new_fps != self.project.fps
            || new_bg != self.project.background
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
        let script_label = primary_script_label(&self.project);
        let mut close_cancel = false;
        let mut close_ok = false;
        let mut match_contents = false;
        let mut make_default = false;

        let modal = egui::Modal::new(egui::Id::new("document-settings"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Document settings",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 520.0,
                    header_top.y + metrics::DIALOG_HEADER - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );

                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        const L: f32 = 100.0;
                        ui.set_width(520.0 - 2.0 * space::S5);
                        ui.spacing_mut().item_spacing.y = space::S3;

                        let (old_w, old_h) = (draft.width, draft.height);
                        ui.horizontal(|ui| {
                            form_label(ui, "Stage size", L);
                            let rw = ui.add(
                                c::Field::new(&mut draft.width, "Stage width")
                                    .prefix("W")
                                    .decimals(0)
                                    .range(1.0..=65535.0)
                                    .width(120.0),
                            );
                            let rh = ui.add(
                                c::Field::new(&mut draft.height, "Stage height")
                                    .prefix("H")
                                    .decimals(0)
                                    .range(1.0..=65535.0)
                                    .width(120.0),
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
                                        draft.height = (draft.width / draft.aspect)
                                            .round()
                                            .clamp(1.0, 65535.0);
                                    } else if rh.changed() && (draft.height - old_h).abs() > 1e-6 {
                                        draft.width = (draft.height * draft.aspect)
                                            .round()
                                            .clamp(1.0, 65535.0);
                                    }
                                } else {
                                    draft.aspect = draft.width.max(1.0) / draft.height.max(1.0);
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            form_label(ui, "", L);
                            if ui.add(c::Button::ghost("Match contents")).clicked() {
                                match_contents = true;
                            }
                            ui.add_enabled(false, c::Button::ghost("Match printer"))
                                .on_disabled_hover_text(NOT_AVAILABLE);
                        });

                        ui.horizontal(|ui| {
                            form_label(ui, "Scale", L);
                            ui.add(c::Checkbox::new(
                                &mut draft.scale_content,
                                "Scale content with stage",
                            ));
                            ui.add(c::RegistrationPicker {
                                point: &mut draft.anchor,
                            });
                        });

                        ui.horizontal(|ui| {
                            form_label(ui, "Units", L);
                            c::Select::new("doc-settings-units", units_label(draft.units))
                                .width(160.0)
                                .show(ui, |ui| {
                                    for unit in RulerUnits::ALL {
                                        let label = units_label(unit);
                                        if ui
                                            .add(
                                                c::MenuItem::new(label)
                                                    .checked(draft.units == unit),
                                            )
                                            .clicked()
                                        {
                                            draft.units = unit;
                                            ui.close();
                                        }
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            form_label(ui, "Background", L);
                            c::ColorField::new(&mut draft.background, "Background")
                                .width(200.0)
                                .show(ui);
                        });

                        ui.horizontal(|ui| {
                            form_label(ui, "Frame rate", L);
                            ui.add(
                                c::Field::new(&mut draft.fps, "Frame rate")
                                    .prefix("fps")
                                    .decimals(0)
                                    .range(1.0..=120.0)
                                    .stepper()
                                    .width(140.0),
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

                        ui.horizontal(|ui| {
                            form_label(ui, "Script", L);
                            ui.add_enabled_ui_semantic(false, |ui| {
                                c::Select::new("doc-settings-script", script_label)
                                    .width(220.0)
                                    .show(ui, |_| {});
                            })
                            .response
                            .on_disabled_hover_text(NOT_AVAILABLE);
                        });

                        match footer_clicked(
                            ui,
                            "",
                            Some(("Make default", true)),
                            &["Cancel", "OK"],
                        ) {
                            Some(FooterClick::Ghost) => make_default = true,
                            Some(FooterClick::Button(0)) => close_cancel = true,
                            Some(FooterClick::Button(_)) => close_ok = true,
                            None => {}
                        }
                    });
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

        if make_default {
            let mut defaults = DocumentDefaults {
                width: (draft.width.round() as u32).clamp(1, 65535),
                height: (draft.height.round() as u32).clamp(1, 65535),
                fps: (draft.fps.round() as u32).clamp(1, 120),
                background: draft.background.to_srgba_unmultiplied(),
                units: draft.units,
            };
            defaults.sanitize();
            self.workspace.document_defaults = Some(defaults);
            self.message("Document settings saved as default.");
        }

        if !close_cancel && !close_ok {
            let typing = ctx.wants_keyboard_input();
            let popup_open = egui::Popup::is_any_open(ctx);
            if !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                close_cancel = true;
            } else if !typing
                && !popup_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                close_ok = true;
            } else if modal.should_close() {
                close_cancel = true;
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
