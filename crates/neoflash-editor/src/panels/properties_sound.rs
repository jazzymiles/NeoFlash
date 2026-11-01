//! Properties ▸ Sound (PR-08 / PP-13, `panel-properties-sound.png`): the frame sound of the current
//! keyframe. The composition is the former planned preview's: sound select, Effect + Envelope…,
//! Sync + Repeat/Loop + count, waveform span, file info, Play and Sound properties….
//! Envelope… and Sound properties… still open their planned dialogs (DL-09, DL-08).
use super::planned::caption;
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{FrameSound, Id, SoundEffect, SoundSync};
use neoflash_ui::components::{self as c, MenuItem, timeline as tl};
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::space;

/// One change requested by the section; applied after drawing as one Undo step.
enum SoundEdit {
    Resource(Option<Id>),
    Effect(SoundEffect),
    Sync(SoundSync),
    LoopForever(bool),
    Loops(u32),
}

impl Editor {
    /// Sound section of frame Properties; shown only when the current keyframe has a sound.
    pub(crate) fn frame_sound_section(&mut self, ui: &mut egui::Ui) {
        let Some((_, sound)) = self.frame_sound() else {
            return;
        };
        let editable = self.layer_editable();
        let sounds = self.audio_resources();
        let name = sounds
            .iter()
            .find(|(id, _)| *id == sound.resource)
            .map_or("Missing sound", |(_, path)| path.as_str())
            .to_owned();
        let info = self.sound_info(&sound);
        let mut edit = None;
        let mut play = false;
        let mut open_props = false;
        let w = ui.available_width();
        c::SectionHeader::new("Sound")
            .id_salt("properties-sound")
            .show(ui, |ui| {
                ui.add_enabled_ui(editable, |ui| {
                    c::Select::new("frame-sound-resource", &name)
                        .width(w)
                        .show(ui, |ui| {
                            if ui.add(MenuItem::new("None")).clicked() {
                                edit = Some(SoundEdit::Resource(None));
                            }
                            for (id, path) in &sounds {
                                let item = MenuItem::new(path).checked(*id == sound.resource);
                                if ui.add(item).clicked() {
                                    edit = Some(SoundEdit::Resource(Some(id.clone())));
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        let effect = format!("Effect: {}", sound.effect.label());
                        c::Select::new("frame-sound-effect", &effect)
                            .width(168.0)
                            .show(ui, |ui| {
                                for option in SoundEffect::ALL {
                                    let item = MenuItem::new(option.label())
                                        .checked(option == sound.effect);
                                    if ui.add(item).clicked() {
                                        edit = Some(SoundEdit::Effect(option));
                                    }
                                }
                            });
                        if ui
                            .add(
                                c::Button::new("Envelope…")
                                    .icon(i16::WAVEFORM)
                                    .width(w - 168.0 - space::S2),
                            )
                            .clicked()
                        {
                            Self::open_planned(ui.ctx(), "Library menu ▸ Edit envelope…");
                        }
                    });
                    ui.horizontal(|ui| {
                        let sync = format!("Sync: {}", sound.sync.label());
                        c::Select::new("frame-sound-sync", &sync)
                            .width(144.0)
                            .show(ui, |ui| {
                                for option in SoundSync::ALL {
                                    let item =
                                        MenuItem::new(option.label()).checked(option == sound.sync);
                                    if ui.add(item).clicked() {
                                        edit = Some(SoundEdit::Sync(option));
                                    }
                                }
                            });
                        let repeat = if sound.loop_forever { "Loop" } else { "Repeat" };
                        c::Select::new("frame-sound-repeat", repeat)
                            .width(92.0)
                            .show(ui, |ui| {
                                for (forever, label) in [(false, "Repeat"), (true, "Loop")] {
                                    let item =
                                        MenuItem::new(label).checked(forever == sound.loop_forever);
                                    if ui.add(item).clicked() {
                                        edit = Some(SoundEdit::LoopForever(forever));
                                    }
                                }
                            });
                        let mut count = f64::from(sound.loops);
                        let response = ui.add_enabled(
                            !sound.loop_forever,
                            c::Field::new(&mut count, "Repeat count")
                                .range(1.0..=f64::from(FrameSound::MAX_LOOPS))
                                .decimals(0)
                                .width(w - 236.0 - 2.0 * space::S2),
                        );
                        if response.changed() && count as u32 != sound.loops {
                            edit = Some(SoundEdit::Loops(count as u32));
                        }
                    });
                });
                let (span, _) = ui.allocate_exact_size(egui::vec2(w, 27.0), egui::Sense::hover());
                tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Sound, true);
                caption(ui, &info);
                ui.horizontal(|ui| {
                    if ui.add(c::Button::ghost("Play").icon(i16::PLAY)).clicked() {
                        play = true;
                    }
                    if ui
                        .add(c::Button::ghost("Sound properties…").icon(i16::SOUND_FILE))
                        .clicked()
                    {
                        open_props = true;
                    }
                });
            });
        if play {
            self.timeline_audio.audition(&self.project, &sound);
        }
        if open_props {
            self.open_sound_properties(ui.ctx(), Some(sound.resource.clone()));
        }
        match edit {
            Some(SoundEdit::Resource(resource)) => self.set_frame_sound(resource),
            Some(SoundEdit::Effect(effect)) => self.edit_frame_sound(|s| s.effect = effect),
            Some(SoundEdit::Sync(sync)) => self.edit_frame_sound(|s| s.sync = sync),
            Some(SoundEdit::LoopForever(forever)) => {
                self.edit_frame_sound(|s| s.loop_forever = forever)
            }
            Some(SoundEdit::Loops(loops)) => self.edit_frame_sound(|s| s.loops = loops),
            None => {}
        }
    }

    /// "8 kHz · Mono · 1.5 s · 24.0 kB" from the decoded resource (size only when undecodable).
    fn sound_info(&mut self, sound: &FrameSound) -> String {
        let bytes = self
            .project
            .resources
            .iter()
            .find(|unit| unit.id == sound.resource)
            .map_or(0, |unit| unit.byte_len());
        let size = format!("{:.1} kB", bytes as f64 / 1000.0);
        match self.timeline_audio.decoded(&self.project, &sound.resource) {
            Ok(decoded) => format!(
                "{} kHz · {} · {:.1} s · {size}",
                f64::from(decoded.rate()) / 1000.0,
                if decoded.channels() == 1 {
                    "Mono"
                } else {
                    "Stereo"
                },
                decoded.duration()
            ),
            Err(_) => format!("Not decodable for preview · {size}"),
        }
    }
}
