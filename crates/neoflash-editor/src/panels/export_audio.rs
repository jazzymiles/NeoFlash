//! Export audio modal dialog (NF2 `Dialog / Export audio`, DL-06; reference
//! `dialogs-export.png` / `dialog-export-audio.png`), opened from `File ▸ Export ▸ Export audio…`
//! (`⌥⇧⌘A`), `Library ▸ Export audio…`, and `Window ▸ Panels ▸ Planned panels ▸ Export audio`.
//!
//! Edits live on a draft in `egui::Context` temp memory until confirmed: `Cancel` / `Esc` discards
//! without writing any file, while `Export` / `Enter` prompts for an output `.wav`/`.mp3`/`.ogg`/`.aac`
//! path (or writes directly in headless tests via [`Editor::confirm_export_audio_to_path`]).
use super::planned::{FooterClick, footer_clicked};
use super::planned_dialogs::group;
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{Id, LayerRole};
use neoflash_export::{
    AudioBitDepth, AudioBitRate, AudioChannels, AudioExportOptions, AudioExportRange,
    AudioExportReport, AudioExportSource, AudioFormat, AudioSampleRate,
    estimate_audio_export_bytes, export_project_audio, format_audio_bytes_label,
    format_audio_duration_label, render_project_audio_mix,
};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline as tl;
use neoflash_ui::tokens::space;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExportAudioDraft {
    pub(crate) options: AudioExportOptions,
    pub(crate) cached_summary: String,
    pub(crate) cached_duration: f64,
}

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-export-audio-draft")
}

pub(crate) fn has_export_audio_dialog(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<ExportAudioDraft>(draft_id()))
        .is_some()
}

#[cfg(test)]
pub(crate) fn export_audio_draft(ctx: &egui::Context) -> Option<ExportAudioDraft> {
    ctx.data(|d| d.get_temp::<ExportAudioDraft>(draft_id()))
}

#[cfg(test)]
pub(crate) fn update_export_audio_draft(
    ctx: &egui::Context,
    f: impl FnOnce(&mut ExportAudioDraft),
) {
    let id = draft_id();
    if let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportAudioDraft>(id)) {
        f(&mut draft);
        ctx.data_mut(|d| d.insert_temp(id, draft));
    }
}

const FORMAT_TABS: [(usize, &str); 4] = [(0, "WAV"), (1, "MP3"), (2, "OGG"), (3, "AAC")];

fn source_label(
    project: &neoflash_core::Project,
    symbol_id: &str,
    source: &AudioExportSource,
) -> String {
    match source {
        AudioExportSource::TimelineMix => "Timeline mix (all sound layers)".to_owned(),
        AudioExportSource::Layer(layer_id) => {
            let name = project
                .symbol(symbol_id)
                .and_then(|s| s.timeline.layers.iter().find(|l| &l.id == layer_id))
                .map(|l| l.name.as_str())
                .unwrap_or(layer_id.as_str());
            format!("Layer: {name}")
        }
        AudioExportSource::Resource(res_id) => {
            let name = project
                .resources
                .iter()
                .find(|r| &r.id == res_id)
                .map(|r| r.path.rsplit('/').next().unwrap_or(r.path.as_str()))
                .unwrap_or(res_id.as_str());
            format!("Resource: {name}")
        }
    }
}

impl Editor {
    fn refresh_export_audio_summary(&self, draft: &mut ExportAudioDraft) {
        match render_project_audio_mix(&self.project, &draft.options) {
            Ok(rendered) => {
                draft.cached_summary = rendered.source_summary.clone();
                draft.cached_duration = rendered.duration_seconds();
            }
            Err(_) => {
                draft.cached_summary = "timeline mix".to_owned();
                let frames = self.symbol().timeline.frame_count.max(1);
                draft.cached_duration = f64::from(frames) / f64::from(self.project.fps.max(1));
            }
        }
    }

    /// Open the `Export audio` modal dialog (`DL-06`).
    pub(crate) fn open_export_audio(
        &mut self,
        ctx: &egui::Context,
        initial_source: Option<AudioExportSource>,
    ) {
        self.finish_edits();
        self.timeline_audio.stop_audition();
        let mut options = AudioExportOptions::default();
        if let Some(src) = initial_source {
            options.source = src;
        }
        let mut draft = ExportAudioDraft {
            options,
            cached_summary: String::new(),
            cached_duration: 0.0,
        };
        self.refresh_export_audio_summary(&mut draft);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_export_audio(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.remove::<ExportAudioDraft>(draft_id()));
    }

    /// Export audio to `path` using the current `Export audio` modal draft and close the dialog on
    /// success.
    pub(crate) fn confirm_export_audio_to_path(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
    ) -> Result<AudioExportReport, String> {
        let Some(draft) = ctx.data(|d| d.get_temp::<ExportAudioDraft>(draft_id())) else {
            return Err("Export audio dialog is not open.".into());
        };
        let report =
            export_project_audio(&self.project, path, &draft.options).map_err(|e| e.to_string())?;
        ctx.data_mut(|d| d.remove::<ExportAudioDraft>(draft_id()));
        self.message(format!(
            "Exported {} audio to {} ({}, {}).",
            report.format.label(),
            path.display(),
            format_audio_duration_label(report.duration_seconds),
            format_audio_bytes_label(report.bytes),
        ));
        Ok(report)
    }

    fn sound_layers_in_active_symbol(&self) -> Vec<(Id, String)> {
        self.symbol()
            .timeline
            .layers
            .iter()
            .filter(|layer| {
                !matches!(layer.properties.role, LayerRole::Guide | LayerRole::Folder)
                    && layer.keyframes.iter().any(|k| k.sound.is_some())
            })
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect()
    }

    fn audio_resources_in_project(&self) -> Vec<(Id, String)> {
        self.project
            .resources
            .iter()
            .filter(|r| r.is_audio())
            .map(|r| {
                (
                    r.id.clone(),
                    r.path.rsplit('/').next().unwrap_or(&r.path).to_owned(),
                )
            })
            .collect()
    }

    pub(crate) fn export_audio_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = ctx.data(|d| d.get_temp::<ExportAudioDraft>(draft_id())) else {
            return;
        };

        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_export = false;
        let mut refresh_needed = false;

        let symbol_id = self.symbol_id().to_owned();
        let sound_layers = self.sound_layers_in_active_symbol();
        let audio_resources = self.audio_resources_in_project();
        let frame_count = self.symbol().timeline.frame_count.max(1);
        let sel_range = self.selected_frame_range();

        let modal = egui::Modal::new(egui::Id::new("export-audio"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                ui.set_width(640.0);
                let header_top = ui.cursor().min;
                close_cancel |= c::DialogHeader {
                    title: "Export audio",
                }
                .show(ui);
                ui.painter().hline(
                    header_top.x..=header_top.x + 640.0,
                    header_top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle),
                );

                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        let w = 640.0 - 2.0 * space::S5;
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);

                        let mut fmt_idx = draft.options.format.index();
                        if c::Segmented::show(ui, &mut fmt_idx, &FORMAT_TABS, false).is_some() {
                            draft.options.format = AudioFormat::from_index(fmt_idx);
                        }

                        let (span, _) =
                            ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
                        tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Sound, true);

                        group(ui, "Source");
                        ui.horizontal(|ui| {
                            let src_lbl =
                                source_label(&self.project, &symbol_id, &draft.options.source);
                            c::Select::new(ui.id().with("export-audio-source"), &src_lbl)
                                .width(300.0)
                                .show(ui, |ui| {
                                    let is_mix = matches!(
                                        draft.options.source,
                                        AudioExportSource::TimelineMix
                                    );
                                    if ui
                                        .add(
                                            c::MenuItem::new("Timeline mix (all sound layers)")
                                                .checked(is_mix),
                                        )
                                        .clicked()
                                    {
                                        draft.options.source = AudioExportSource::TimelineMix;
                                        refresh_needed = true;
                                        ui.close();
                                    }
                                    if !sound_layers.is_empty() {
                                        c::menu_separator(ui);
                                        for (lid, lname) in &sound_layers {
                                            let checked = draft.options.source
                                                == AudioExportSource::Layer(lid.clone());
                                            if ui
                                                .add(
                                                    c::MenuItem::new(&format!("Layer: {lname}"))
                                                        .checked(checked),
                                                )
                                                .clicked()
                                            {
                                                draft.options.source =
                                                    AudioExportSource::Layer(lid.clone());
                                                refresh_needed = true;
                                                ui.close();
                                            }
                                        }
                                    }
                                    if !audio_resources.is_empty() {
                                        c::menu_separator(ui);
                                        for (rid, rname) in &audio_resources {
                                            let checked = draft.options.source
                                                == AudioExportSource::Resource(rid.clone());
                                            if ui
                                                .add(
                                                    c::MenuItem::new(&format!("Resource: {rname}"))
                                                        .checked(checked),
                                                )
                                                .clicked()
                                            {
                                                draft.options.source =
                                                    AudioExportSource::Resource(rid.clone());
                                                refresh_needed = true;
                                                ui.close();
                                            }
                                        }
                                    }
                                });

                            let range_lbl = draft.options.range.label();
                            let range_enabled =
                                !matches!(draft.options.source, AudioExportSource::Resource(_));
                            ui.add_enabled_ui(range_enabled, |ui| {
                                c::Select::new(ui.id().with("export-audio-range"), &range_lbl)
                                    .width(280.0)
                                    .show(ui, |ui| {
                                        if ui
                                            .add(c::MenuItem::new("Range: whole movie").checked(
                                                matches!(
                                                    draft.options.range,
                                                    AudioExportRange::WholeMovie
                                                ),
                                            ))
                                            .clicked()
                                        {
                                            draft.options.range = AudioExportRange::WholeMovie;
                                            refresh_needed = true;
                                            ui.close();
                                        }
                                        if ui
                                            .add(c::MenuItem::new("Range: current scene").checked(
                                                matches!(
                                                    draft.options.range,
                                                    AudioExportRange::CurrentScene { .. }
                                                ),
                                            ))
                                            .clicked()
                                        {
                                            draft.options.range = AudioExportRange::CurrentScene {
                                                scene_id: symbol_id.clone(),
                                            };
                                            refresh_needed = true;
                                            ui.close();
                                        }
                                        let (f_start, f_end) = sel_range
                                            .as_ref()
                                            .map(|r| (r.0, r.1))
                                            .unwrap_or((0, frame_count.saturating_sub(1)));
                                        let frames_choice = AudioExportRange::Frames {
                                            scene_id: Some(symbol_id.clone()),
                                            start: f_start,
                                            end: f_end.max(f_start),
                                        };
                                        let frames_lbl = frames_choice.label();
                                        if ui
                                            .add(c::MenuItem::new(&frames_lbl).checked(matches!(
                                                draft.options.range,
                                                AudioExportRange::Frames { .. }
                                            )))
                                            .clicked()
                                        {
                                            draft.options.range = frames_choice;
                                            refresh_needed = true;
                                            ui.close();
                                        }
                                    });
                            });
                        });

                        group(ui, "Format");
                        ui.horizontal(|ui| {
                            c::Select::new(
                                ui.id().with("export-audio-rate"),
                                draft.options.sample_rate.label(),
                            )
                            .width(140.0)
                            .show(ui, |ui| {
                                for rate in AudioSampleRate::ALL {
                                    if ui
                                        .add(
                                            c::MenuItem::new(rate.label())
                                                .checked(draft.options.sample_rate == rate),
                                        )
                                        .clicked()
                                    {
                                        draft.options.sample_rate = rate;
                                        refresh_needed = true;
                                        ui.close();
                                    }
                                }
                            });

                            ui.add_enabled_ui(draft.options.format == AudioFormat::Wav, |ui| {
                                c::Select::new(
                                    ui.id().with("export-audio-depth"),
                                    draft.options.bit_depth.label(),
                                )
                                .width(120.0)
                                .show(ui, |ui| {
                                    for depth in AudioBitDepth::ALL {
                                        if ui
                                            .add(
                                                c::MenuItem::new(depth.label())
                                                    .checked(draft.options.bit_depth == depth),
                                            )
                                            .clicked()
                                        {
                                            draft.options.bit_depth = depth;
                                            ui.close();
                                        }
                                    }
                                });
                            });

                            c::Select::new(
                                ui.id().with("export-audio-channels"),
                                draft.options.channels.label(),
                            )
                            .width(120.0)
                            .show(ui, |ui| {
                                for ch in AudioChannels::ALL {
                                    if ui
                                        .add(
                                            c::MenuItem::new(ch.label())
                                                .checked(draft.options.channels == ch),
                                        )
                                        .clicked()
                                    {
                                        draft.options.channels = ch;
                                        ui.close();
                                    }
                                }
                            });

                            ui.add_enabled_ui(draft.options.format != AudioFormat::Wav, |ui| {
                                c::Select::new(
                                    ui.id().with("export-audio-bitrate"),
                                    draft.options.bit_rate.cbr_label(),
                                )
                                .width(180.0)
                                .show(ui, |ui| {
                                    for br in AudioBitRate::ALL {
                                        if ui
                                            .add(
                                                c::MenuItem::new(br.cbr_label())
                                                    .checked(draft.options.bit_rate == br),
                                            )
                                            .clicked()
                                        {
                                            draft.options.bit_rate = br;
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        });

                        group(ui, "Processing");
                        ui.horizontal(|ui| {
                            ui.add(c::Switch::new(
                                &mut draft.options.normalize,
                                "Normalize to −1 dB",
                            ));
                            if ui
                                .add(
                                    c::Field::new(&mut draft.options.fade_out_seconds, "Fade out")
                                        .prefix("Fade out")
                                        .unit("s")
                                        .decimals(1)
                                        .range(0.0..=30.0)
                                        .step(0.1)
                                        .stepper()
                                        .width(160.0),
                                )
                                .changed()
                            {
                                draft.options.fade_out_seconds =
                                    draft.options.fade_out_seconds.clamp(0.0, 30.0);
                            }
                            if ui
                                .add(c::Checkbox::new(
                                    &mut draft.options.trim_silence,
                                    "Trim silence",
                                ))
                                .changed()
                            {
                                refresh_needed = true;
                            }
                        });

                        let est_bytes =
                            estimate_audio_export_bytes(draft.cached_duration, &draft.options);
                        let footer_summary = format!(
                            "{} · {} · ~ {}",
                            draft.cached_summary,
                            format_audio_duration_label(draft.cached_duration),
                            format_audio_bytes_label(est_bytes),
                        );
                        match footer_clicked(ui, &footer_summary, None, &["Cancel", "Export"]) {
                            Some(FooterClick::Button(0)) => close_cancel = true,
                            Some(FooterClick::Button(1)) => do_export = true,
                            _ => {}
                        }
                    });
            });

        if refresh_needed {
            self.refresh_export_audio_summary(&mut draft);
        }

        if !close_cancel && !do_export {
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
                do_export = true;
            } else if modal.should_close() {
                close_cancel = true;
            }
        }

        if close_cancel {
            self.cancel_export_audio(ctx);
            return;
        }

        ctx.data_mut(|d| d.insert_temp(draft_id(), draft.clone()));

        if do_export {
            let ext = draft.options.format.extension();
            let default_stem = self
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(self.project.name.as_str());
            let default_file = format!("{default_stem}.{ext}");
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Export audio")
                .set_file_name(&default_file)
                .add_filter(draft.options.format.label(), &[ext])
                .save_file()
                && let Err(err) = self.confirm_export_audio_to_path(ctx, &path)
            {
                self.message(format!("Audio export failed: {err}"));
            }
        }
    }
}
