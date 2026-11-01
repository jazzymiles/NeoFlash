//! Sound properties modal dialog (NF2 `Dialog / Sound properties`, DL-08; reference
//! `dialogs-sound-stroke.png`), opened from `Properties ▸ Sound ▸ Sound properties…`,
//! `Library menu ▸ Sound properties…`, and the Library audio resource context menu.
//!
//! Edits live on a draft in `egui::Context` temp memory until confirmed: `Cancel` / `Esc` discards
//! without touching the project (and stops any active audition), while `OK` / `Enter` commits name
//! and audio byte replacements in a single Undo step (`"Sound properties"`).
use super::planned::{caption, select};
use super::planned_dialogs::group;
use crate::app::Editor;
use crate::timeline_sound::decode_unit;
use eframe::egui;
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::components::timeline as tl;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::space;
use std::path::PathBuf;

const AUDIO_REENCODING_REASON: &str = "Audio re-encoding at publish";

#[derive(Clone, Debug)]
pub(crate) struct SoundPropertiesDraft {
    pub(crate) resource_id: Id,
    pub(crate) name: String,
    pub(crate) unit: ResourceUnit,
    pub(crate) pending_source: Option<PathBuf>,
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-sound-properties-draft")
}

pub(crate) fn has_sound_properties_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<SoundPropertiesDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
pub(crate) fn sound_properties_draft(ctx: &egui::Context) -> Option<SoundPropertiesDraft> {
    ctx.data(|d| d.get_temp::<SoundPropertiesDraft>(draft_id()))
}

#[cfg(test)]
pub(crate) fn set_sound_properties_draft_name(ctx: &egui::Context, name: impl Into<String>) {
    let id = draft_id();
    if let Some(mut draft) = ctx.data(|d| d.get_temp::<SoundPropertiesDraft>(id)) {
        draft.name = name.into();
        ctx.data_mut(|d| d.insert_temp(id, draft));
    }
}

fn format_sound_stats(unit: &ResourceUnit) -> String {
    let bytes = unit.byte_len();
    let size = if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.1} kB", bytes as f64 / 1_000.0)
    };
    let fmt = match unit.media_type.as_str() {
        "audio/mpeg" => "MP3",
        "audio/wav" | "audio/x-wav" => "WAV",
        other => other,
    };
    match decode_unit(unit) {
        Ok(decoded) => format!(
            "{} kHz · {} · {fmt} · {:.1} s · {size}",
            f64::from(decoded.rate()) / 1000.0,
            if decoded.channels() == 1 {
                "Mono"
            } else {
                "Stereo"
            },
            decoded.duration()
        ),
        Err(_) => format!("{fmt} · Not decodable for preview · {size}"),
    }
}

fn is_supported_audio_media_type(media_type: &str) -> bool {
    matches!(media_type, "audio/mpeg" | "audio/wav" | "audio/x-wav")
}

impl Editor {
    fn resolve_sound_properties_target(&self, explicit: Option<Id>) -> Option<Id> {
        if let Some(id) = explicit
            && self
                .project
                .resources
                .iter()
                .any(|u| u.id == id && u.is_audio())
        {
            return Some(id);
        }
        if let Some(selected) = &self.resource_panel.selected
            && self
                .project
                .resources
                .iter()
                .any(|u| &u.id == selected && u.is_audio())
        {
            return Some(selected.clone());
        }
        if let Some((_, sound)) = self.frame_sound()
            && self
                .project
                .resources
                .iter()
                .any(|u| u.id == sound.resource && u.is_audio())
        {
            return Some(sound.resource);
        }
        self.project
            .resources
            .iter()
            .find(|u| u.is_audio())
            .map(|u| u.id.clone())
    }

    /// Open the `Sound properties` modal dialog for `resource_id` (or the selected / active audio
    /// resource when `None`).
    pub(crate) fn open_sound_properties(&mut self, ctx: &egui::Context, resource_id: Option<Id>) {
        self.finish_edits();
        self.timeline_audio.stop_audition();
        let Some(target_id) = self.resolve_sound_properties_target(resource_id) else {
            self.message("Import a sound file (MP3 or WAV) first.");
            return;
        };
        let Some(unit) = self
            .project
            .resources
            .iter()
            .find(|u| u.id == target_id)
            .cloned()
        else {
            return;
        };
        let draft = SoundPropertiesDraft {
            resource_id: unit.id.clone(),
            name: unit.path.clone(),
            unit,
            pending_source: None,
        };
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    fn remaining_resource_budget_excluding(&self, resource_id: &str) -> usize {
        let used: usize = self
            .project
            .resources
            .iter()
            .filter(|u| u.id != resource_id)
            .map(ResourceUnit::byte_len)
            .sum();
        MAX_RESOURCE_TOTAL_BYTES.saturating_sub(used)
    }

    /// Replace the draft audio payload inside an open `Sound properties` dialog without touching
    /// the document until `OK` (`confirm_sound_properties`).
    #[cfg(test)]
    pub(crate) fn replace_sound_properties_draft_bytes(
        &mut self,
        ctx: &egui::Context,
        new_name: &str,
        new_media_type: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let id = draft_id();
        let Some(mut draft) = ctx.data(|d| d.get_temp::<SoundPropertiesDraft>(id)) else {
            return Err("Sound properties dialog is not open.".into());
        };
        if !is_supported_audio_media_type(new_media_type) {
            return Err("Import MP3 or WAV audio.".into());
        }
        let budget = self.remaining_resource_budget_excluding(&draft.resource_id);
        if bytes.len() > MAX_RESOURCE_BYTES.min(budget) {
            return Err("Sound file exceeds the project's 64 MiB resource limit.".into());
        }
        let trimmed = new_name.trim();
        let target_name = if trimmed.is_empty() {
            draft.name.trim().to_owned()
        } else {
            trimmed.to_owned()
        };
        let replacement = ResourceUnit::from_bytes(
            draft.resource_id.clone(),
            target_name.clone(),
            new_media_type,
            bytes,
        )
        .map_err(|e| e.to_string())?;
        self.timeline_audio.stop_audition();
        draft.name = target_name;
        draft.unit = replacement;
        ctx.data_mut(|d| d.insert_temp(id, draft));
        Ok(())
    }

    fn load_sound_file_into_draft(
        &mut self,
        draft: &mut SoundPropertiesDraft,
        path: &std::path::Path,
        update_name: bool,
    ) -> Result<(), String> {
        let budget = self.remaining_resource_budget_excluding(&draft.resource_id);
        let mut loaded = crate::resources::read_resource(path, budget)?;
        if !is_supported_audio_media_type(&loaded.media_type) {
            return Err(format!(
                "{} is not a supported sound file. Import MP3 or WAV audio.",
                loaded.path
            ));
        }
        self.timeline_audio.stop_audition();
        loaded.id = draft.resource_id.clone();
        if update_name {
            draft.name = loaded.path.clone();
        }
        draft.unit = loaded;
        draft.pending_source = Some(path.to_path_buf());
        Ok(())
    }

    /// Headless helper: replace `resource_id`'s name, media type and bytes in one `"Sound properties"`
    /// Undo step while preserving its ID (and all `FrameSound` references).
    #[cfg(test)]
    pub(crate) fn replace_sound_resource_bytes(
        &mut self,
        resource_id: &str,
        new_name: &str,
        new_media_type: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        self.finish_edits();
        if !is_supported_audio_media_type(new_media_type) {
            return Err("Import MP3 or WAV audio.".into());
        }
        let Some(index) = self
            .project
            .resources
            .iter()
            .position(|u| u.id == resource_id && u.is_audio())
        else {
            return Err(format!("Sound resource {resource_id} was not found."));
        };
        let budget = self.remaining_resource_budget_excluding(resource_id);
        if bytes.len() > MAX_RESOURCE_BYTES.min(budget) {
            return Err("Sound file exceeds the project's 64 MiB resource limit.".into());
        }
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("Sound name cannot be empty.".into());
        }
        let unit = ResourceUnit::from_bytes(resource_id, trimmed, new_media_type, bytes)
            .map_err(|e| e.to_string())?;
        self.commit_sound_resource_unit(index, unit, None)
    }

    fn commit_sound_resource_unit(
        &mut self,
        index: usize,
        unit: ResourceUnit,
        pending_source: Option<PathBuf>,
    ) -> Result<(), String> {
        let existing = &self.project.resources[index];
        let unchanged = existing.path == unit.path
            && existing.media_type == unit.media_type
            && existing.data_base64 == unit.data_base64;
        if unchanged {
            if let Some(path) = pending_source {
                self.timeline_audio
                    .source_paths
                    .insert(unit.id.clone(), path);
            }
            return Ok(());
        }
        let mut candidate = self.project.clone();
        candidate.format_version = candidate.format_version.max(3);
        let id = unit.id.clone();
        candidate.resources[index] = unit;
        candidate.validate().map_err(|e| e.to_string())?;
        self.transact_named("Sound properties", |s| {
            s.project = candidate;
        });
        if let Some(path) = pending_source {
            self.timeline_audio.source_paths.insert(id, path);
        }
        self.resource_panel.baseline = None;
        Ok(())
    }

    pub(crate) fn confirm_sound_properties(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.timeline_audio.stop_audition();
        let Some(mut draft) = ctx.data(|d| d.get_temp::<SoundPropertiesDraft>(draft_id())) else {
            return Ok(());
        };
        let Some(index) = self
            .project
            .resources
            .iter()
            .position(|u| u.id == draft.resource_id && u.is_audio())
        else {
            ctx.data_mut(|d| d.remove::<SoundPropertiesDraft>(draft_id()));
            return Err("Sound resource no longer exists.".into());
        };
        let trimmed = draft.name.trim();
        if trimmed.is_empty() {
            return Err("Sound name cannot be empty.".into());
        }
        draft.unit.id = draft.resource_id.clone();
        draft.unit.path = trimmed.to_owned();
        self.commit_sound_resource_unit(index, draft.unit, draft.pending_source)?;
        ctx.data_mut(|d| d.remove::<SoundPropertiesDraft>(draft_id()));
        Ok(())
    }

    pub(crate) fn cancel_sound_properties(&mut self, ctx: &egui::Context) {
        self.timeline_audio.stop_audition();
        ctx.data_mut(|d| d.remove::<SoundPropertiesDraft>(draft_id()));
    }

    pub(crate) fn sound_properties_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<SoundPropertiesDraft>(draft_id())) else {
            return;
        };
        if !self
            .project
            .resources
            .iter()
            .any(|u| u.id == draft.resource_id && u.is_audio())
        {
            self.cancel_sound_properties(ctx);
            return;
        }

        let p = neoflash_ui::palette(ctx);
        let known_source = draft.pending_source.clone().or_else(|| {
            self.timeline_audio
                .source_paths
                .get(&draft.resource_id)
                .cloned()
        });
        let source_display = known_source
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| draft.unit.path.clone());
        let stats_display = format_sound_stats(&draft.unit);
        let playing = self.timeline_audio.is_auditioning();

        let mut close_cancel = false;
        let mut close_ok = false;
        let mut do_update = false;
        let mut do_import = false;
        let mut do_toggle_test = false;

        let modal = egui::Modal::new(egui::Id::new("sound-properties"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Sound properties",
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
                        let w = 520.0 - 2.0 * space::S5;
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);

                        ui.horizontal(|ui| {
                            let (span, _) = ui
                                .allocate_exact_size(egui::vec2(120.0, 64.0), egui::Sense::hover());
                            tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Sound, true);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = space::S1;
                                ui.add(
                                    c::TextField::new(&mut draft.name, "Sound name")
                                        .width(w - 120.0 - space::S2),
                                );
                                caption(ui, &source_display);
                                caption(ui, &stats_display);
                            });
                        });

                        group(ui, "Export");
                        ui.scope(|ui| {
                            ui.disable();
                            let r = ui
                                .vertical(|ui| {
                                    ui.set_width(w);
                                    select(ui, "Compression: MP3", w);
                                    ui.horizontal(|ui| {
                                        let half = (w - space::S2) / 2.0;
                                        select(ui, "Bit rate: 128 kbps", half);
                                        select(ui, "Quality: Best", half);
                                    });
                                    let mut mono = false;
                                    ui.add(c::Checkbox::new(&mut mono, "Convert stereo to mono"));
                                })
                                .response;
                            let _ = c::not_available(r, AUDIO_REENCODING_REASON);
                        });
                        caption(ui, "Source audio bytes are published without re-encoding");

                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(known_source.is_some(), c::Button::ghost("Update"))
                                .clicked()
                            {
                                do_update = true;
                            }
                            if ui.add(c::Button::ghost("Import…")).clicked() {
                                do_import = true;
                            }
                            let test_btn = if playing {
                                c::Button::ghost("Stop").icon(i16::STOP)
                            } else {
                                c::Button::ghost("Test").icon(i16::PLAY)
                            };
                            if ui.add(test_btn).clicked() {
                                do_toggle_test = true;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
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

        if do_update
            && let Some(path) = known_source.as_ref()
            && let Err(err) = self.load_sound_file_into_draft(&mut draft, path, false)
        {
            self.message(format!("Sound update failed: {err}"));
        }
        if do_import
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Sound", &["mp3", "wav"])
                .pick_file()
            && let Err(err) = self.load_sound_file_into_draft(&mut draft, &path, true)
        {
            self.message(format!("Sound import failed: {err}"));
        }
        if do_toggle_test {
            if playing {
                self.timeline_audio.stop_audition();
            } else if let Err(err) = self.timeline_audio.audition_unit(&draft.unit) {
                self.message(format!("Sound preview failed: {err}"));
            }
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
            self.cancel_sound_properties(ctx);
        } else if close_ok {
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
            if let Err(err) = self.confirm_sound_properties(ctx) {
                self.message(format!("Sound properties rejected: {err}"));
            }
        } else {
            ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
        }
    }
}
