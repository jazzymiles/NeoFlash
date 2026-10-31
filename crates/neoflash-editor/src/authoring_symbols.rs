//! Symbol authoring and the compact, frame-preserving timeline timing controls.
use crate::{app::Editor, paint_controls::property_row};
use eframe::egui;
use neoflash_core::{InstancePlayback, SymbolKind};

pub(crate) const BUTTON_STATES: [&str; 4] = ["Up", "Over", "Down", "Hit"];

#[derive(Clone)]
struct TimingDraft {
    baseline: u32,
    frames: u32,
    /// Fractional while scrubbing the `FPS` prefix; committed rounded.
    fps: f64,
    seconds: f64,
    pending: bool,
}

impl Editor {
    /// Timeline controls bar: `NF2/Field` FPS and Duration (frame count stays fixed), or the
    /// Up · Over · Down · Hit segmented states of a button symbol. One undo step per commit.
    pub(crate) fn timeline_timing_ui(&mut self, ui: &mut egui::Ui) {
        use neoflash_ui::components as nf;
        use neoflash_ui::{TextStyle, typography};
        if self.symbol().kind == SymbolKind::Button {
            let count = self.symbol().timeline.frame_count as usize;
            if count < 4 && ui.add(nf::Button::new("Enable four states")).clicked() {
                self.transact(|s| s.symbol_mut().timeline.frame_count = 4);
            }
            let options: Vec<(u32, &str)> = BUTTON_STATES
                .iter()
                .enumerate()
                .filter(|(frame, _)| *frame < count)
                .map(|(frame, label)| (frame as u32, *label))
                .collect();
            let mut state = self.frame;
            nf::Segmented::show(ui, &mut state, &options, false);
            if state != self.frame {
                self.finish_edits();
                self.set_playing(false);
                self.reset_authored_preview();
                self.frame = state;
                self.normalize_selection();
            }
            let p = neoflash_ui::palette(ui.ctx());
            ui.label(
                typography::rich(
                    if self.frame == 3 {
                        "Hit area · invisible in HTML5"
                    } else {
                        "Button state"
                    },
                    TextStyle::Small,
                )
                .color(p.text_tertiary),
            );
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
                fps: self.project.fps as f64,
                seconds: frames as f64 / self.project.fps.max(1) as f64,
                pending: false,
            });
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let fps = ui
            .add(
                nf::Field::new(&mut draft.fps, "Frame rate")
                    .prefix("FPS")
                    .range(1.0..=1000.0)
                    .step(0.25)
                    .width(92.),
            )
            .on_hover_text("Frame rate. Frame count stays fixed; duration changes.");
        if fps.changed() {
            draft.seconds = frames as f64 / draft.fps.round().max(1.);
            draft.pending = true;
        }
        let duration = ui
            .add(
                nf::Field::new(&mut draft.seconds, "Duration")
                    .prefix("Duration")
                    .unit("s")
                    .range(frames as f64 / 1000. ..=frames as f64)
                    .step(0.02)
                    .decimals(2)
                    .width(140.),
            )
            .on_hover_text(
                "Total duration. Keeps all frames and sets the nearest whole-number FPS.",
            );
        if duration.changed() {
            draft.fps = (frames as f64 / draft.seconds).round().clamp(1., 1000.);
            draft.pending = true;
        }
        if escape {
            ui.data_mut(|data| data.remove::<TimingDraft>(id));
            return;
        }
        if draft.pending && !ui.input(|i| i.pointer.primary_down()) {
            let next = (draft.fps.round() as u32).clamp(1, 1000);
            if next != self.project.fps {
                self.finish_edits();
                self.transact(|s| {
                    s.project.fps = next;
                    s.accumulator = 0.;
                    s.last_tick = std::time::Instant::now();
                });
            }
            draft.baseline = self.project.fps;
            draft.fps = self.project.fps as f64;
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
