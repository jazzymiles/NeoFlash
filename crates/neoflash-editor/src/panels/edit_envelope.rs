//! Edit envelope modal dialog (NF2 `Dialog / Edit envelope`, DL-09; reference
//! `dialogs-sound-stroke.png`), opened by `Properties ▸ Sound ▸ Envelope…` and
//! `Library menu ▸ Edit envelope…`.
//!
//! Edits live on a draft until confirmed: `Cancel` / `Esc` discards without touching the project,
//! `OK` / `Enter` commits `FrameSound.effect` and `FrameSound.envelope` in a single Undo step
//! (`"Edit sound envelope"`).
use super::planned::caption;
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{FrameSound, Id, SoundEffect};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EditEnvelopeDraft {
    pub(crate) symbol_id: Id,
    pub(crate) layer_id: Id,
    pub(crate) key_frame: u32,
    pub(crate) resource: Id,
    pub(crate) effect: SoundEffect,
    pub(crate) left: Vec<(f32, f32)>,
    pub(crate) right: Vec<(f32, f32)>,
    pub(crate) selected_left: Option<usize>,
    pub(crate) selected_right: Option<usize>,
    /// `0` = Seconds, `1` = Frames.
    pub(crate) unit: usize,
    /// Horizontal time ruler zoom (`1.0..=8.0`).
    pub(crate) zoom: f64,
}

impl EditEnvelopeDraft {
    pub(crate) fn from_sound(
        symbol_id: Id,
        layer_id: Id,
        key_frame: u32,
        sound: &FrameSound,
    ) -> Self {
        let points = sound.envelope_points();
        let (left, right) = split_stereo_envelope(&points);
        Self {
            symbol_id,
            layer_id,
            key_frame,
            resource: sound.resource.clone(),
            effect: sound.effect,
            left,
            right,
            selected_left: None,
            selected_right: None,
            unit: 0,
            zoom: 1.0,
        }
    }

    pub(crate) fn apply_preset(&mut self, effect: SoundEffect) {
        self.effect = effect;
        if effect != SoundEffect::Custom {
            let points: Vec<[f64; 3]> = effect
                .envelope()
                .iter()
                .map(|&(p, l, r)| [p, l, r])
                .collect();
            let (left, right) = split_stereo_envelope(&points);
            self.left = left;
            self.right = right;
            self.selected_left = None;
            self.selected_right = None;
        }
    }

    pub(crate) fn preview_sound(&self, base: Option<&FrameSound>) -> FrameSound {
        let mut sound = base
            .cloned()
            .unwrap_or_else(|| FrameSound::new(self.resource.clone()));
        sound.resource = self.resource.clone();
        sound.effect = self.effect;
        sound.envelope = (self.effect == SoundEffect::Custom)
            .then(|| merge_stereo_envelope(&self.left, &self.right));
        sound
    }
}

pub(crate) type StereoEnvelopeLanes = (Vec<(f32, f32)>, Vec<(f32, f32)>);

pub(crate) fn split_stereo_envelope(points: &[[f64; 3]]) -> StereoEnvelopeLanes {
    if points.is_empty() {
        return (vec![(0.0, 1.0)], vec![(0.0, 1.0)]);
    }
    let left = points
        .iter()
        .map(|p| (p[0].clamp(0.0, 1.0) as f32, p[1].clamp(0.0, 1.0) as f32))
        .collect();
    let right = points
        .iter()
        .map(|p| (p[0].clamp(0.0, 1.0) as f32, p[2].clamp(0.0, 1.0) as f32))
        .collect();
    (left, right)
}

fn sample_channel(points: &[(f32, f32)], t: f64) -> f64 {
    if points.is_empty() {
        return 1.0;
    }
    let t = t.clamp(0.0, 1.0);
    let first = points[0];
    if t <= f64::from(first.0) {
        return f64::from(first.1).clamp(0.0, 1.0);
    }
    for pair in points.windows(2) {
        let (t0, v0) = (f64::from(pair[0].0), f64::from(pair[0].1));
        let (t1, v1) = (f64::from(pair[1].0), f64::from(pair[1].1));
        if t <= t1 {
            let span = t1 - t0;
            let u = if span > 1e-9 { (t - t0) / span } else { 1.0 };
            return (v0 + (v1 - v0) * u).clamp(0.0, 1.0);
        }
    }
    f64::from(points.last().unwrap().1).clamp(0.0, 1.0)
}

/// Merge independent L and R `(t, gain)` handle lists into sorted `[position, left, right]` points
/// capped at [`FrameSound::MAX_ENVELOPE_POINTS`].
pub(crate) fn merge_stereo_envelope(left: &[(f32, f32)], right: &[(f32, f32)]) -> Vec<[f64; 3]> {
    let mut left_sorted: Vec<(f32, f32)> = left
        .iter()
        .map(|&(t, v)| (t.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
        .collect();
    let mut right_sorted: Vec<(f32, f32)> = right
        .iter()
        .map(|&(t, v)| (t.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
        .collect();
    left_sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    right_sorted.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut times: Vec<f64> = left_sorted
        .iter()
        .chain(right_sorted.iter())
        .map(|&(t, _)| f64::from(t).clamp(0.0, 1.0))
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let mut unique_times: Vec<f64> = Vec::with_capacity(times.len());
    for t in times {
        if unique_times
            .last()
            .is_none_or(|prev| (t - *prev).abs() > 1e-4)
        {
            unique_times.push(t);
        }
    }
    if unique_times.is_empty() {
        return vec![[0.0, 1.0, 1.0]];
    }
    let mut merged: Vec<[f64; 3]> = unique_times
        .into_iter()
        .map(|t| {
            [
                t,
                sample_channel(&left_sorted, t),
                sample_channel(&right_sorted, t),
            ]
        })
        .collect();
    while merged.len() > FrameSound::MAX_ENVELOPE_POINTS {
        // Drop the interior point closest in time to its neighbours so endpoints and shape stay intact.
        let mut best_idx = 1;
        let mut best_span = f64::INFINITY;
        for i in 1..merged.len() - 1 {
            let span = merged[i + 1][0] - merged[i - 1][0];
            if span < best_span {
                best_span = span;
                best_idx = i;
            }
        }
        merged.remove(best_idx);
    }
    merged
}

fn unique_time_count(left: &[(f32, f32)], right: &[(f32, f32)]) -> usize {
    let mut times: Vec<f64> = left
        .iter()
        .chain(right.iter())
        .map(|&(t, _)| f64::from(t).clamp(0.0, 1.0))
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    let mut count = 0;
    let mut last: Option<f64> = None;
    for t in times {
        if last.is_none_or(|prev| (t - prev).abs() > 1e-4) {
            count += 1;
            last = Some(t);
        }
    }
    count
}

fn changed_point_index(before: &[(f32, f32)], after: &[(f32, f32)]) -> Option<usize> {
    after
        .iter()
        .enumerate()
        .find(|&(i, p)| before.get(i) != Some(p))
        .map(|(i, _)| i)
}

fn choose_second_step(visible_secs: f64) -> f64 {
    for step in [0.1, 0.2, 0.5, 1.0, 2.0, 4.0, 5.0, 10.0, 15.0, 30.0, 60.0] {
        if visible_secs / step <= 9.0 {
            return step;
        }
    }
    (visible_secs / 8.0).ceil().max(1.0)
}

impl Editor {
    /// Open the `Edit envelope` modal dialog for the frame sound at the playhead.
    pub(crate) fn open_edit_envelope(&mut self) {
        self.finish_edits();
        let target = if let Some((key_frame, sound)) = self.frame_sound() {
            if !self.layer_editable() {
                self.message("Select an unlocked, visible layer to edit its sound envelope.");
                return;
            }
            let layer_id = self.symbol().timeline.layers[self.layer].id.clone();
            Some((self.layer, layer_id, key_frame, sound))
        } else {
            let timeline = &self.symbol().timeline;
            timeline.layers.iter().enumerate().find_map(|(idx, layer)| {
                if neoflash_core::layer_effectively_locked(timeline, layer)
                    || !neoflash_core::layer_effectively_visible(timeline, layer)
                {
                    return None;
                }
                let key = layer.key_at(self.frame)?;
                let sound = key.sound.clone()?;
                Some((idx, layer.id.clone(), key.frame, sound))
            })
        };
        let Some((layer_idx, layer_id, key_frame, sound)) = target else {
            self.message("Select a keyframe with a sound first.");
            return;
        };
        self.layer = layer_idx;
        let symbol_id = self.symbol_id().to_owned();
        self.edit_envelope = Some(EditEnvelopeDraft::from_sound(
            symbol_id, layer_id, key_frame, &sound,
        ));
    }

    pub(crate) fn cancel_edit_envelope(&mut self) {
        if self.edit_envelope.take().is_some() {
            self.timeline_audio.stop_audition();
        }
    }

    pub(crate) fn commit_edit_envelope(&mut self, draft: EditEnvelopeDraft) {
        self.timeline_audio.stop_audition();
        let new_envelope = (draft.effect == SoundEffect::Custom)
            .then(|| merge_stereo_envelope(&draft.left, &draft.right));
        let unchanged = self
            .project
            .symbol(&draft.symbol_id)
            .and_then(|s| s.timeline.layers.iter().find(|l| l.id == draft.layer_id))
            .and_then(|l| l.keyframes.iter().find(|k| k.frame == draft.key_frame))
            .and_then(|k| k.sound.as_ref())
            .is_some_and(|s| s.effect == draft.effect && s.envelope == new_envelope);
        if unchanged {
            return;
        }
        self.transact_named("Edit sound envelope", |s| {
            if let Some(sound) = s
                .project
                .symbol_mut(&draft.symbol_id)
                .and_then(|sym| {
                    sym.timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == draft.layer_id)
                })
                .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == draft.key_frame))
                .and_then(|k| k.sound.as_mut())
            {
                sound.effect = draft.effect;
                sound.envelope = new_envelope;
            }
        });
    }

    pub(crate) fn edit_envelope_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.edit_envelope.clone() else {
            return;
        };
        const WIDTH: f32 = 680.0;
        let nf = neoflash_ui::palette(ctx);
        let duration_secs = self
            .timeline_audio
            .decoded(&self.project, &draft.resource)
            .map(|d| d.duration().max(0.1))
            .unwrap_or_else(|_| {
                let frames = self.symbol().timeline.frame_count.max(1) as f64;
                let fps = self.project.fps.max(1) as f64;
                (frames / fps).max(1.0)
            });
        let fps = self.project.fps.max(1) as f64;

        let mut close_cancel = false;
        let mut close_ok = false;
        let mut play_preview = false;
        let mut stop_preview = false;
        let mut over_limit = false;

        let modal = egui::Modal::new(egui::Id::new("edit-envelope"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Edit envelope",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + WIDTH,
                    header_top.y + metrics::DIALOG_HEADER - 0.5,
                    egui::Stroke::new(1.0_f32, nf.border_subtle),
                );

                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(WIDTH - 2.0 * space::S5);
                        ui.spacing_mut().item_spacing.y = space::S3;

                        ui.horizontal(|ui| {
                            let effect_label = format!("Effect: {}", draft.effect.label());
                            let mut picked = None;
                            c::Select::new("edit-envelope-effect", &effect_label)
                                .width(200.0)
                                .show(ui, |ui| {
                                    for option in SoundEffect::ALL {
                                        if ui
                                            .add(
                                                c::MenuItem::new(option.label())
                                                    .checked(option == draft.effect),
                                            )
                                            .clicked()
                                        {
                                            picked = Some(option);
                                            ui.close();
                                        }
                                    }
                                });
                            if let Some(effect) = picked {
                                draft.apply_preset(effect);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    c::Segmented::show(
                                        ui,
                                        &mut draft.unit,
                                        &[(0, "Seconds"), (1, "Frames")],
                                        false,
                                    );
                                },
                            );
                        });

                        let prev_left = draft.left.clone();
                        let resp_l = ui.add(c::EnvelopeLane {
                            channel: "L",
                            points: &mut draft.left,
                            selected: draft.selected_left,
                            height: 96.0,
                        });
                        if resp_l.changed() {
                            if draft.left.len() > prev_left.len()
                                && unique_time_count(&draft.left, &draft.right)
                                    > FrameSound::MAX_ENVELOPE_POINTS
                            {
                                draft.left = prev_left;
                                over_limit = true;
                            } else {
                                draft.selected_left = changed_point_index(&prev_left, &draft.left)
                                    .or(draft.selected_left);
                                draft.selected_right = None;
                                draft.effect = SoundEffect::Custom;
                            }
                        }

                        let (ruler, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 18.0),
                            egui::Sense::hover(),
                        );
                        let visible_secs = (duration_secs / draft.zoom).max(0.1);
                        if draft.unit == 1 {
                            let visible_frames = ((visible_secs * fps).round() as u32).max(1);
                            let major = (visible_frames / 8).max(1).div_ceil(5) * 5;
                            c::paint_frame_scale(
                                ui.painter(),
                                ui.ctx(),
                                ruler,
                                visible_frames,
                                major.max(1),
                            );
                        } else {
                            let step = choose_second_step(visible_secs);
                            let steps = (visible_secs / step).floor() as usize;
                            for i in 0..=steps {
                                let s = i as f64 * step;
                                let x = ruler.left()
                                    + (s / visible_secs).clamp(0.0, 1.0) as f32 * ruler.width();
                                let align = if i == 0 {
                                    egui::Align2::LEFT_CENTER
                                } else if (s - visible_secs).abs() < 1e-3 {
                                    egui::Align2::RIGHT_CENTER
                                } else {
                                    egui::Align2::CENTER_CENTER
                                };
                                let label = if step >= 1.0 {
                                    format!("{:.0}s", s)
                                } else {
                                    format!("{:.1}s", s)
                                };
                                ui.painter().text(
                                    egui::pos2(x, ruler.center().y),
                                    align,
                                    label,
                                    neoflash_ui::typography::font(TextStyle::MonoSmall),
                                    nf.text_tertiary,
                                );
                            }
                        }

                        let prev_right = draft.right.clone();
                        let resp_r = ui.add(c::EnvelopeLane {
                            channel: "R",
                            points: &mut draft.right,
                            selected: draft.selected_right,
                            height: 96.0,
                        });
                        if resp_r.changed() {
                            if draft.right.len() > prev_right.len()
                                && unique_time_count(&draft.left, &draft.right)
                                    > FrameSound::MAX_ENVELOPE_POINTS
                            {
                                draft.right = prev_right;
                                over_limit = true;
                            } else {
                                draft.selected_right =
                                    changed_point_index(&prev_right, &draft.right)
                                        .or(draft.selected_right);
                                draft.selected_left = None;
                                draft.effect = SoundEffect::Custom;
                            }
                        }

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S1;
                            if ui.add(c::IconButton::new(i16::PLAY, "Play")).clicked() {
                                play_preview = true;
                            }
                            if ui.add(c::IconButton::new(i16::STOP, "Stop")).clicked() {
                                stop_preview = true;
                            }
                            if ui
                                .add_enabled(
                                    draft.zoom < 8.0,
                                    c::IconButton::new(i16::ZOOMIN, "Zoom in"),
                                )
                                .clicked()
                            {
                                draft.zoom = (draft.zoom * 2.0).min(8.0);
                            }
                            if ui
                                .add_enabled(
                                    draft.zoom > 1.0,
                                    c::IconButton::new(i16::ZOOMOUT, "Zoom out"),
                                )
                                .clicked()
                            {
                                draft.zoom = (draft.zoom * 0.5).max(1.0);
                            }
                            ui.add_space(space::S2);
                            caption(
                                ui,
                                "Drag handles to shape volume · click a lane to add a point",
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = space::S2;
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

        if over_limit {
            self.message("Envelope can have at most 32 points.");
        }
        if play_preview {
            let base = self.frame_sound().map(|(_, s)| s);
            let preview = draft.preview_sound(base.as_ref());
            self.timeline_audio.audition(&self.project, &preview);
        }
        if stop_preview {
            self.timeline_audio.stop_audition();
        }

        if !close_cancel && !close_ok {
            let typing = ctx.wants_keyboard_input();
            let popup_open = egui::Popup::is_any_open(ctx);
            if !typing
                && !popup_open
                && ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                })
            {
                if let Some(idx) = draft.selected_left
                    && draft.left.len() > 1
                    && idx < draft.left.len()
                {
                    draft.left.remove(idx);
                    draft.selected_left = (idx > 0).then(|| idx - 1);
                    draft.effect = SoundEffect::Custom;
                } else if let Some(idx) = draft.selected_right
                    && draft.right.len() > 1
                    && idx < draft.right.len()
                {
                    draft.right.remove(idx);
                    draft.selected_right = (idx > 0).then(|| idx - 1);
                    draft.effect = SoundEffect::Custom;
                }
            }
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
            self.cancel_edit_envelope();
        } else if close_ok {
            self.edit_envelope = None;
            self.commit_edit_envelope(draft);
        } else {
            self.edit_envelope = Some(draft);
        }
    }
}
