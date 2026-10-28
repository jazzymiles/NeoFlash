//! Symbol authoring and the compact, frame-preserving timeline timing controls.
use crate::{app::Editor, paint_controls::property_row};
use eframe::egui;
use neoflash_core::{InstancePlayback, SymbolKind};

pub(crate) const BUTTON_STATES: [&str; 4] = ["Up", "Over", "Down", "Hit"];

#[derive(Clone)]
struct TimingDraft {
    baseline: u32,
    frames: u32,
    fps: u32,
    seconds: f64,
    pending: bool,
}

impl Editor {
    pub(crate) fn timeline_timing_ui(&mut self, ui: &mut egui::Ui) {
        if self.symbol().kind == SymbolKind::Button {
            if self.symbol().timeline.frame_count < 4
                && ui.small_button("Enable four states").clicked()
            {
                self.transact(|s| s.symbol_mut().timeline.frame_count = 4);
            }
            for (frame, label) in BUTTON_STATES.iter().enumerate() {
                if ui
                    .add_enabled(
                        frame < self.symbol().timeline.frame_count as usize,
                        egui::Button::new(*label).selected(self.frame == frame as u32),
                    )
                    .clicked()
                {
                    self.finish_edits();
                    self.set_playing(false);
                    self.reset_authored_preview();
                    self.frame = frame as u32;
                    self.normalize_selection();
                }
            }
            ui.small(if self.frame == 3 {
                "Hit area · invisible in HTML5"
            } else {
                "Button state"
            });
            return;
        }
        let frames = self.symbol().timeline.frame_count.max(1);
        let id = ui
            .id()
            .with(("timeline-timing", &self.clipboard.session, self.symbol_id()));
        let mut draft = ui
            .data_mut(|data| data.get_temp::<TimingDraft>(id))
            .filter(|draft| draft.baseline == self.project.fps && draft.frames == frames)
            .unwrap_or(TimingDraft {
                baseline: self.project.fps,
                frames,
                fps: self.project.fps,
                seconds: frames as f64 / self.project.fps.max(1) as f64,
                pending: false,
            });
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let fps = ui
            .add_sized(
                [64., 21.],
                egui::DragValue::new(&mut draft.fps)
                    .range(1..=1000)
                    .speed(0.25)
                    .suffix(" fps")
                    .update_while_editing(false),
            )
            .on_hover_text("Frame rate. Frame count stays fixed; duration changes.");
        if fps.changed() {
            draft.seconds = frames as f64 / draft.fps.max(1) as f64;
            draft.pending = true;
        }
        let duration = ui
            .add_sized(
                [76., 21.],
                egui::DragValue::new(&mut draft.seconds)
                    .range(frames as f64 / 1000. ..=frames as f64)
                    .speed(0.02)
                    .fixed_decimals(2)
                    .suffix(" s")
                    .update_while_editing(false),
            )
            .on_hover_text(
                "Total duration. Keeps all frames and sets the nearest whole-number FPS.",
            );
        if duration.changed() {
            draft.fps = (frames as f64 / draft.seconds).round().clamp(1., 1000.) as u32;
            draft.pending = true;
        }
        ui.small(format!(
            "{} / {} F · {:.2} s",
            self.frame + 1,
            frames,
            self.frame as f64 / draft.fps.max(1) as f64
        ))
        .on_hover_text("Current frame and elapsed time");
        if escape {
            ui.data_mut(|data| data.remove::<TimingDraft>(id));
            return;
        }
        if draft.pending && !ui.input(|i| i.pointer.primary_down()) {
            let next = draft.fps.clamp(1, 1000);
            if next != self.project.fps {
                self.finish_edits();
                self.transact(|s| {
                    s.project.fps = next;
                    s.accumulator = 0.;
                    s.last_tick = std::time::Instant::now();
                });
            }
            draft.baseline = self.project.fps;
            draft.seconds = frames as f64 / self.project.fps as f64;
            draft.pending = false;
        }
        ui.data_mut(|data| data.insert_temp(id, draft));
    }

    /// Playback is a placement property; editing it never changes sibling instances.
    pub(crate) fn instance_playback_ui(
        &mut self,
        ui: &mut egui::Ui,
        symbol_id: &str,
        playback: &mut InstancePlayback,
    ) -> Option<egui::Id> {
        let symbol = self.project.symbol(symbol_id)?;
        let count = symbol.timeline.frame_count.max(1);
        let kind = symbol.kind;
        property_row(ui, "Type", |ui| {
            ui.label(match kind {
                SymbolKind::MovieClip => "Movie Clip",
                SymbolKind::Graphic => "Graphic",
                SymbolKind::Button => "Button",
            })
        });
        if kind == SymbolKind::Button {
            ui.small("Up / Over / Down / Hit");
            ui.small("AS2 callbacks and AS3 events run on this named instance.");
            return None;
        }
        let before = *playback;
        let (mut mode, mut first) = match *playback {
            InstancePlayback::Independent => (0, 0),
            InstancePlayback::Loop => (1, 0),
            InstancePlayback::LoopFrom(f) => (1, f),
            InstancePlayback::PlayOnce => (2, 0),
            InstancePlayback::PlayOnceFrom(f) => (2, f),
            InstancePlayback::SingleFrame(f) => (3, f),
        };
        let labels = ["Independent", "Loop", "Play once", "Single frame"];
        let mut control = None;
        let response = property_row(ui, "Playback", |ui| {
            egui::ComboBox::from_id_salt("instance-playback")
                .width(ui.available_width().min(140.))
                .selected_text(labels[mode])
                .show_ui(ui, |ui| {
                    for (index, name) in labels.iter().enumerate() {
                        ui.selectable_value(&mut mode, index, *name);
                    }
                })
                .response
        });
        if mode != 0 {
            let mut displayed = first.saturating_add(1).min(count);
            let field = property_row(ui, "First frame", |ui| {
                ui.add_sized(
                    [76., 22.],
                    egui::DragValue::new(&mut displayed)
                        .range(1..=count)
                        .update_while_editing(false),
                )
            });
            first = displayed - 1;
            if field.changed() {
                control = Some(field.id);
            }
        }
        *playback = match mode {
            0 => InstancePlayback::Independent,
            1 if first == 0 => InstancePlayback::Loop,
            1 => InstancePlayback::LoopFrom(first),
            2 if first == 0 => InstancePlayback::PlayOnce,
            2 => InstancePlayback::PlayOnceFrom(first),
            _ => InstancePlayback::SingleFrame(first),
        };
        if before != *playback {
            control.get_or_insert(response.id);
        }
        control
    }
}
