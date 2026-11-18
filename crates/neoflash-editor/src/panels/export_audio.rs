//! Export audio modal dialog (NF2 `Dialog / Export audio`, DL-06; reference
//! `dialogs-export.png` / `dialog-export-audio.png`), opened from `File ▸ Export ▸ Export audio…`
//! (`⌥⇧⌘A`), `Library ▸ Export audio…`, and `Window ▸ Panels ▸ Planned panels ▸ Export audio`.
//!
//! Edits live on a draft in `egui::Context` temp memory until confirmed: `Cancel` / `Esc` discards
//! without writing any file, while `Export` / `Enter` prompts for an output `.wav`/`.mp3`/`.ogg`/`.aac`
//! path (or writes directly in headless tests via [`Editor::confirm_export_audio_to_path`]).
use super::dialog_helpers::{
    FormDialog, dialog_width, field_block, footer_note, form_dialog_height, form_line, form_row,
    group_caption,
};
use crate::app::Editor;
use eframe::egui;
use neoflash_core::{Id, LayerRole, Project, WaveformPeaks};
use neoflash_export::{
    AudioBitDepth, AudioBitRate, AudioChannels, AudioExportOptions, AudioExportRange,
    AudioExportReport, AudioExportSource, AudioFormat, AudioSampleRate,
    estimate_audio_export_bytes, export_project_audio, format_audio_bytes_label,
    format_audio_duration_label, render_project_audio_mix,
};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline as tl;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, space};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioPreviewKey {
    pub(crate) source: AudioExportSource,
    pub(crate) range: AudioExportRange,
    pub(crate) channels: AudioChannels,
    pub(crate) sample_rate: AudioSampleRate,
    pub(crate) trim_silence: bool,
}

impl From<&AudioExportOptions> for AudioPreviewKey {
    fn from(options: &AudioExportOptions) -> Self {
        Self {
            source: options.source.clone(),
            range: options.range.clone(),
            channels: options.channels,
            sample_rate: options.sample_rate,
            trim_silence: options.trim_silence,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExportAudioDraft {
    pub(crate) options: AudioExportOptions,
    pub(crate) output_path: PathBuf,
    pub(crate) cached_summary: String,
    pub(crate) cached_duration: f64,
    pub(crate) has_sound: bool,
    pub(crate) empty_reason: Option<String>,
    pub(crate) cached_peaks: Option<WaveformPeaks>,
    pub(crate) computed_key: Option<AudioPreviewKey>,
}

pub(crate) fn default_audio_output_path(
    project: &Project,
    project_path: Option<&Path>,
    format: AudioFormat,
) -> PathBuf {
    let raw_stem = project_path
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(project.name.as_str());
    let sanitized: String = raw_stem
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
        .collect();
    let stem = if sanitized.trim().is_empty() {
        "Untitled"
    } else {
        sanitized.trim()
    };
    let file_name = format!("{stem}.{}", format.extension());
    match project_path
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
    {
        Some(dir) => dir.join(file_name),
        None => PathBuf::from(file_name),
    }
}

pub(crate) fn sync_audio_output_path_extension(path: &mut PathBuf, format: AudioFormat) {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let raw_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled.mp3");
    let mut stem = raw_name;
    for ext in [".wav", ".wave", ".mp3", ".ogg", ".flac", ".aac", ".m4a"] {
        if raw_name.len() > ext.len()
            && raw_name[raw_name.len() - ext.len()..].eq_ignore_ascii_case(ext)
        {
            stem = &raw_name[..raw_name.len() - ext.len()];
            break;
        }
    }
    let stem = if stem.trim().is_empty() {
        "Untitled"
    } else {
        stem
    };
    let next_name = format!("{stem}.{}", format.extension());
    *path = match parent {
        Some(dir) => dir.join(next_name),
        None => PathBuf::from(next_name),
    };
}

impl ExportAudioDraft {
    pub(crate) fn set_format(&mut self, format: AudioFormat) {
        self.options.format = format;
        sync_audio_output_path_extension(&mut self.output_path, format);
    }
}

#[derive(Clone)]
struct PendingAudioPreview {
    generation: u64,
    key: AudioPreviewKey,
    rx: Arc<Mutex<Receiver<AudioPreviewResult>>>,
}

struct AudioPreviewResult {
    generation: u64,
    key: AudioPreviewKey,
    summary: String,
    duration: f64,
    has_sound: bool,
    empty_reason: Option<String>,
    peaks: Option<WaveformPeaks>,
}

static PREVIEW_GEN: AtomicU64 = AtomicU64::new(1);

fn draft_id() -> egui::Id {
    egui::Id::new("nf2-export-audio-draft")
}

fn preview_worker_id() -> egui::Id {
    egui::Id::new("nf2-export-audio-preview-worker")
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

#[cfg(test)]
pub(crate) fn has_pending_audio_preview(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<PendingAudioPreview>(preview_worker_id()))
        .is_some()
}

/// Downsample `peaks` into `columns` `(min, max)` pairs per output lane: 1 lane for Mono, 2 for Stereo.
pub(crate) fn downsample_waveform_columns(
    peaks: &WaveformPeaks,
    channels: AudioChannels,
    columns: usize,
) -> Vec<Vec<(f32, f32)>> {
    let cols = columns.max(1);
    let sample_lane = |lane: &[[f32; 2]]| -> Vec<(f32, f32)> {
        let total = lane.len();
        if total == 0 {
            return vec![(0.0, 0.0); cols];
        }
        (0..cols)
            .map(|col| {
                let start = (col * total) / cols;
                let end = (((col + 1) * total) / cols).max(start + 1).min(total);
                lane[start..end]
                    .iter()
                    .fold((0.0_f32, 0.0_f32), |(lo, hi), &[a, b]| {
                        (lo.min(a), hi.max(b))
                    })
            })
            .collect()
    };
    if peaks.left.is_empty() {
        return Vec::new();
    }
    let mut out = vec![sample_lane(&peaks.left)];
    if channels == AudioChannels::Stereo {
        let right = if peaks.right.is_empty() {
            &peaks.left
        } else {
            &peaks.right
        };
        out.push(sample_lane(right));
    }
    out
}

fn compute_audio_preview(
    project: &Project,
    options: &AudioExportOptions,
    generation: u64,
) -> AudioPreviewResult {
    let key = AudioPreviewKey::from(options);
    let doc_has_audio = project.resources.iter().any(|r| r.is_audio());
    if !doc_has_audio {
        return AudioPreviewResult {
            generation,
            key,
            summary: "No sounds".to_owned(),
            duration: 0.0,
            has_sound: false,
            empty_reason: Some("This document has no sounds to export.".to_owned()),
            peaks: None,
        };
    }
    match render_project_audio_mix(project, options) {
        Ok(rendered) => {
            let silent_subrange = matches!(options.range, AudioExportRange::Frames { .. })
                && !matches!(options.source, AudioExportSource::Resource(_))
                && rendered.frames.iter().all(|&[l, r]| l == 0.0 && r == 0.0);
            if rendered.source_summary == "No sounds" || silent_subrange {
                AudioPreviewResult {
                    generation,
                    key,
                    summary: "No sounds".to_owned(),
                    duration: 0.0,
                    has_sound: false,
                    empty_reason: Some("No sound in the selected range.".to_owned()),
                    peaks: None,
                }
            } else {
                let duration = rendered.duration_seconds();
                AudioPreviewResult {
                    generation,
                    key,
                    summary: rendered.source_summary,
                    duration,
                    has_sound: true,
                    empty_reason: None,
                    peaks: Some(rendered.peaks),
                }
            }
        }
        Err(_) => AudioPreviewResult {
            generation,
            key,
            summary: "No sounds".to_owned(),
            duration: 0.0,
            has_sound: false,
            empty_reason: Some("No sound in the selected range.".to_owned()),
            peaks: None,
        },
    }
}

fn apply_audio_preview_result(draft: &mut ExportAudioDraft, result: AudioPreviewResult) {
    draft.cached_summary = result.summary;
    draft.cached_duration = result.duration;
    draft.has_sound = result.has_sound;
    draft.empty_reason = result.empty_reason;
    draft.cached_peaks = result.peaks;
    draft.computed_key = Some(result.key);
}

const FORMAT_TABS: [(usize, &str); 4] = [(0, "WAV"), (1, "MP3"), (2, "OGG"), (3, "AAC")];

/// Height of the waveform / empty-state strip (artwork).
const WAVEFORM_STRIP: f32 = 48.0;

/// `Export audio` has one height per density with and without sounds: the format switch, the
/// strip, three captions and six form rows.
pub(crate) fn export_audio_dialog_height() -> f32 {
    form_dialog_height(7, 3, WAVEFORM_STRIP + metrics::row_gap())
}

/// Value of the `Range` row: the export range label without its "Range: " prefix.
fn range_value_label(range: &AudioExportRange) -> String {
    let label = range.label();
    let value = label.strip_prefix("Range: ").unwrap_or(&label);
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

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
        let generation = PREVIEW_GEN.fetch_add(1, Ordering::Relaxed);
        let res = compute_audio_preview(&self.project, &draft.options, generation);
        apply_audio_preview_result(draft, res);
    }

    fn spawn_export_audio_preview(
        &self,
        ctx: &egui::Context,
        draft: &mut ExportAudioDraft,
        key: AudioPreviewKey,
    ) {
        let generation = PREVIEW_GEN.fetch_add(1, Ordering::Relaxed);
        draft.cached_peaks = None;
        let project = self.project.clone();
        let options = draft.options.clone();
        let ctx_clone = ctx.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = compute_audio_preview(&project, &options, generation);
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
        ctx.data_mut(|d| {
            d.insert_temp(
                preview_worker_id(),
                PendingAudioPreview {
                    generation,
                    key,
                    rx: Arc::new(Mutex::new(rx)),
                },
            );
        });
    }

    /// Open the `Export audio` modal dialog (`DL-06`).
    pub(crate) fn open_export_audio(
        &mut self,
        ctx: &egui::Context,
        initial_source: Option<AudioExportSource>,
    ) {
        self.finish_edits();
        self.timeline_audio.stop_audition();
        ctx.data_mut(|d| d.remove::<PendingAudioPreview>(preview_worker_id()));
        let mut options = self
            .workspace
            .export_audio_options
            .clone()
            .unwrap_or_default();
        if let Some(src) = initial_source {
            options.source = src;
        } else if let AudioExportSource::Layer(ref lid) = options.source
            && !self
                .sound_layers_in_active_symbol()
                .iter()
                .any(|(id, _)| id == lid)
        {
            options.source = AudioExportSource::TimelineMix;
        }
        let output_path =
            default_audio_output_path(&self.project, self.path.as_deref(), options.format);
        let mut draft = ExportAudioDraft {
            options,
            output_path,
            cached_summary: String::new(),
            cached_duration: 0.0,
            has_sound: false,
            empty_reason: None,
            cached_peaks: None,
            computed_key: None,
        };
        self.refresh_export_audio_summary(&mut draft);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft));
    }

    pub(crate) fn cancel_export_audio(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| {
            d.remove::<ExportAudioDraft>(draft_id());
            d.remove::<PendingAudioPreview>(preview_worker_id());
        });
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
        self.workspace.export_audio_options = Some(draft.options.clone());
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

        let mut is_computing = false;
        let mut pending_key = None;
        if let Some(pending) = ctx.data(|d| d.get_temp::<PendingAudioPreview>(preview_worker_id()))
        {
            match pending.rx.lock().map(|rx| rx.try_recv()) {
                Ok(Ok(result)) => {
                    if result.generation == pending.generation {
                        apply_audio_preview_result(&mut draft, result);
                    }
                    ctx.data_mut(|d| d.remove::<PendingAudioPreview>(preview_worker_id()));
                }
                Ok(Err(TryRecvError::Empty)) => {
                    is_computing = true;
                    pending_key = Some(pending.key);
                }
                Ok(Err(TryRecvError::Disconnected)) | Err(_) => {
                    ctx.data_mut(|d| d.remove::<PendingAudioPreview>(preview_worker_id()));
                }
            }
        }

        let p = neoflash_ui::palette(ctx);
        let mut close_cancel = false;
        let mut do_export = false;
        let mut refresh_needed = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);

        let symbol_id = self.symbol_id().to_owned();
        let sound_layers = self.sound_layers_in_active_symbol();
        let audio_resources = self.audio_resources_in_project();
        let frame_count = self.symbol().timeline.frame_count.max(1);
        let sel_range = self.selected_frame_range();

        // v3: one size with and without sounds; the waveform strip keeps its height and shows
        // the empty state (R-074), the summary reads "No sounds" and Export is disabled.
        let width = dialog_width(ctx, metrics::dialog_m());
        let height = export_audio_dialog_height();
        let can_export = draft.has_sound && !is_computing;
        let modal = egui::Modal::new(egui::Id::new("export-audio"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let dialog = FormDialog::show(ui, "Export audio", width, Some(height));
                close_cancel |= dialog.close;
                dialog.page(ui, "export-audio-page", |ui, _| {
                    form_line(ui, |ui| {
                        let mut fmt_idx = draft.options.format.index();
                        if c::Segmented::show(ui, &mut fmt_idx, &FORMAT_TABS, false).is_some() {
                            draft.set_format(AudioFormat::from_index(fmt_idx));
                        }
                    });

                    let strip = egui::vec2(ui.available_width(), WAVEFORM_STRIP);
                    if !draft.has_sound && !is_computing {
                        let empty_msg = draft
                            .empty_reason
                            .as_deref()
                            .unwrap_or("This document has no sounds to export.");
                        let rect = field_block(ui, strip);
                        let icon = egui::Rect::from_center_size(
                            egui::pos2(rect.center().x, rect.top() + WAVEFORM_STRIP * 0.35),
                            egui::Vec2::splat(metrics::ICON_UI),
                        );
                        neoflash_ui::icons::paint(
                            ui.painter(),
                            ui.ctx(),
                            icon,
                            i16::SOUND,
                            p.text_tertiary,
                        );
                        ui.painter().with_clip_rect(rect).text(
                            egui::pos2(rect.center().x, rect.bottom() - space::s1()),
                            egui::Align2::CENTER_BOTTOM,
                            empty_msg,
                            neoflash_ui::typography::font(TextStyle::Small),
                            p.text_tertiary,
                        );
                    } else {
                        let (span, _) = ui.allocate_exact_size(strip, egui::Sense::hover());
                        tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Static, true);
                        if is_computing || draft.cached_peaks.is_none() {
                            tl::paint_sound_peaks(ui.painter(), ui.ctx(), span, &[]);
                        } else if let Some(peaks) = &draft.cached_peaks {
                            let cols = span.width().ceil().max(1.0) as usize;
                            let lanes =
                                downsample_waveform_columns(peaks, draft.options.channels, cols);
                            let channels: Vec<_> = lanes.iter().map(Vec::as_slice).collect();
                            tl::paint_sound_peaks(ui.painter(), ui.ctx(), span, &channels);
                        }
                    }

                    group_caption(ui, "Source");
                    form_row(ui, "Sound", |ui| {
                        let src_lbl =
                            source_label(&self.project, &symbol_id, &draft.options.source);
                        c::Select::new(ui.id().with("export-audio-source"), &src_lbl)
                            .width(ui.available_width())
                            .show(ui, |ui| {
                                let is_mix =
                                    matches!(draft.options.source, AudioExportSource::TimelineMix);
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
                    });
                    form_row(ui, "Range", |ui| {
                        let range_lbl = range_value_label(&draft.options.range);
                        let range_enabled =
                            !matches!(draft.options.source, AudioExportSource::Resource(_));
                        ui.add_enabled_ui(range_enabled, |ui| {
                            c::Select::new(ui.id().with("export-audio-range"), &range_lbl)
                                .width(metrics::field_m())
                                .show(ui, |ui| {
                                    if ui
                                        .add(c::MenuItem::new("Whole movie").checked(matches!(
                                            draft.options.range,
                                            AudioExportRange::WholeMovie
                                        )))
                                        .clicked()
                                    {
                                        draft.options.range = AudioExportRange::WholeMovie;
                                        refresh_needed = true;
                                        ui.close();
                                    }
                                    if ui
                                        .add(c::MenuItem::new("Current scene").checked(matches!(
                                            draft.options.range,
                                            AudioExportRange::CurrentScene { .. }
                                        )))
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
                                    let frames_lbl = range_value_label(&frames_choice);
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

                    group_caption(ui, "Format");
                    form_row(ui, "Sample rate", |ui| {
                        c::Select::new(
                            ui.id().with("export-audio-rate"),
                            draft.options.sample_rate.label(),
                        )
                        .width(metrics::field_s())
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
                            .width(metrics::field_s())
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
                    });
                    form_row(ui, "Channels", |ui| {
                        c::Select::new(
                            ui.id().with("export-audio-channels"),
                            draft.options.channels.label(),
                        )
                        .width(metrics::field_s())
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
                                    refresh_needed = true;
                                    ui.close();
                                }
                            }
                        });
                        ui.add_enabled_ui(draft.options.format != AudioFormat::Wav, |ui| {
                            c::Select::new(
                                ui.id().with("export-audio-bitrate"),
                                draft.options.bit_rate.cbr_label(),
                            )
                            .width(metrics::field_m())
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

                    group_caption(ui, "Processing");
                    form_row(ui, "", |ui| {
                        ui.add(c::Checkbox::new(
                            &mut draft.options.normalize,
                            "Normalize to −1 dB",
                        ));
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
                    form_row(ui, "Fade out", |ui| {
                        if ui
                            .add(
                                c::Field::new(&mut draft.options.fade_out_seconds, "Fade out")
                                    .unit("s")
                                    .decimals(1)
                                    .range(0.0..=30.0)
                                    .step(0.1)
                                    .stepper()
                                    .width(metrics::field_s()),
                            )
                            .changed()
                        {
                            draft.options.fade_out_seconds =
                                if draft.options.fade_out_seconds.is_finite() {
                                    draft.options.fade_out_seconds.clamp(0.0, 30.0)
                                } else {
                                    0.0
                                };
                        }
                    });
                });

                let footer_summary = if draft.has_sound {
                    let est_bytes =
                        estimate_audio_export_bytes(draft.cached_duration, &draft.options);
                    format!(
                        "{} · {} · ~ {}",
                        draft.cached_summary,
                        format_audio_duration_label(draft.cached_duration),
                        format_audio_bytes_label(est_bytes),
                    )
                } else {
                    "No sounds".to_owned()
                };
                let disabled_tooltip = (!draft.has_sound).then(|| {
                    draft
                        .empty_reason
                        .as_deref()
                        .unwrap_or("This document has no sounds to export.")
                        .trim_end_matches('.')
                });
                dialog.footer(
                    ui,
                    |ui| {
                        footer_note(ui, &footer_summary);
                    },
                    |ui| {
                        let export_btn = ui.add_enabled(
                            can_export,
                            c::Button::primary("Export").icon(i16::UPLOAD),
                        );
                        if crate::ui_icons::hover_tooltip(export_btn, "", None, disabled_tooltip)
                            .clicked()
                        {
                            do_export = true;
                        }
                        if ui.add(c::Button::new("Cancel")).clicked() {
                            close_cancel = true;
                        }
                    },
                );
            });

        let can_export_now = can_export;
        let wanted_key = AudioPreviewKey::from(&draft.options);
        if refresh_needed
            || (draft.computed_key.as_ref() != Some(&wanted_key)
                && pending_key.as_ref() != Some(&wanted_key))
        {
            self.spawn_export_audio_preview(ctx, &mut draft, wanted_key);
        }

        if !close_cancel && !do_export {
            match kb.resolve(ctx, &modal, can_export_now) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close_cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_export = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }

        if close_cancel {
            self.cancel_export_audio(ctx);
            super::dialog_helpers::record_end_of_frame_focus(ctx);
            return;
        }

        sync_audio_output_path_extension(&mut draft.output_path, draft.options.format);
        ctx.data_mut(|d| d.insert_temp(draft_id(), draft.clone()));

        if do_export {
            let ext = draft.options.format.extension();
            let default_file = draft
                .output_path
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    default_audio_output_path(
                        &self.project,
                        self.path.as_deref(),
                        draft.options.format,
                    )
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled.mp3")
                    .to_owned()
                });
            let mut dialog = rfd::FileDialog::new()
                .set_title("Export audio")
                .set_file_name(&default_file)
                .add_filter(draft.options.format.label(), &[ext]);
            if let Some(dir) = draft
                .output_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                dialog = dialog.set_directory(dir);
            }
            if let Some(path) = dialog.save_file()
                && let Err(err) = self.confirm_export_audio_to_path(ctx, &path)
            {
                self.message(format!("Audio export failed: {err}"));
            }
        }
        super::dialog_helpers::record_end_of_frame_focus(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::FrameSound;

    fn tiny_pcm_wav(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data_bytes = (samples.len() * 2) as u32;
        let byte_rate = sample_rate * u32::from(channels) * 2;
        let block_align = channels * 2;
        let mut out = Vec::with_capacity(44 + data_bytes as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_bytes.to_le_bytes());
        for &s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    fn audio_project() -> Project {
        let mut samples = Vec::with_capacity(8820);
        for i in 0..4410 {
            let v = ((i as f32 * 0.1).sin() * 12_000.0) as i16;
            samples.push(v);
            samples.push(-v / 2);
        }
        let wav = tiny_pcm_wav(2, 44_100, &samples);
        let mut project = Project::blank();
        project.format_version = 6;
        project.resources.push(
            neoflash_core::ResourceUnit::from_bytes("snd1", "beep.wav", "audio/wav", &wav).unwrap(),
        );
        let timeline = &mut project.symbols[0].timeline;
        timeline.frame_count = 12;
        timeline.layers[0].keyframes[0].sound = Some(FrameSound::new("snd1"));
        project.upgrade_format_for_features();
        project
    }

    fn pump_audio_modal(editor: &mut Editor, ctx: &egui::Context) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920.0, 1080.0),
                )),
                ..Default::default()
            },
            |ctx| editor.export_audio_dialog(ctx),
        )
    }

    #[test]
    fn audio_preview_computes_real_peaks_and_empty_range_states() {
        let blank = Project::blank();
        let empty = compute_audio_preview(&blank, &AudioExportOptions::default(), 1);
        assert!(!empty.has_sound);
        assert_eq!(empty.summary, "No sounds");
        assert_eq!(
            empty.empty_reason.as_deref(),
            Some("This document has no sounds to export.")
        );
        assert!(empty.peaks.is_none());

        let project = audio_project();
        let mono_opts = AudioExportOptions {
            channels: AudioChannels::Mono,
            ..Default::default()
        };
        let res = compute_audio_preview(&project, &mono_opts, 2);
        assert!(res.has_sound);
        assert!(res.duration > 0.0);
        let peaks = res.peaks.as_ref().expect("real mix peaks");
        assert_eq!(
            downsample_waveform_columns(peaks, AudioChannels::Mono, 16).len(),
            1
        );
        let stereo_cols = downsample_waveform_columns(peaks, AudioChannels::Stereo, 16);
        assert_eq!(stereo_cols.len(), 2);
        assert_eq!(stereo_cols[0].len(), 16);
        assert!(stereo_cols[0].iter().any(|&(lo, hi)| lo < 0.0 || hi > 0.0));

        let silent_range = AudioExportOptions {
            range: AudioExportRange::Frames {
                scene_id: None,
                start: 10,
                end: 11,
            },
            ..Default::default()
        };
        let out_of_range = compute_audio_preview(&project, &silent_range, 3);
        assert!(!out_of_range.has_sound);
        assert_eq!(
            out_of_range.empty_reason.as_deref(),
            Some("No sound in the selected range.")
        );
    }

    #[test]
    fn export_audio_modal_caches_preview_and_keeps_enter_inert_when_empty() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut empty_editor = Editor::from_project(&ctx, Project::blank());
        empty_editor.open_export_audio(&ctx, None);
        let mut out = pump_audio_modal(&mut empty_editor, &ctx);
        for _ in 0..80 {
            if !has_pending_audio_preview(&ctx)
                && export_audio_draft(&ctx).is_some_and(|d| d.computed_key.is_some())
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            out = pump_audio_modal(&mut empty_editor, &ctx);
        }
        assert!(!has_pending_audio_preview(&ctx));
        let draft = export_audio_draft(&ctx).expect("empty draft open");
        assert!(!draft.has_sound);
        assert_eq!(draft.cached_summary, "No sounds");
        let export_disabled = out.platform_output.accesskit_update.as_ref().and_then(|u| {
            u.nodes
                .iter()
                .find(|(_, n)| n.label() == Some("Export"))
                .map(|(_, n)| n.is_disabled())
        });
        assert_eq!(export_disabled, Some(true));
        // Enter must stay inert while no sound is present.
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920.0, 1080.0),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| empty_editor.export_audio_dialog(ctx),
        );
        assert!(has_export_audio_dialog(&ctx));
        empty_editor.cancel_export_audio(&ctx);

        let ctx_audio = egui::Context::default();
        ctx_audio.enable_accesskit();
        let mut editor = Editor::from_project(&ctx_audio, audio_project());
        editor.open_export_audio(&ctx_audio, None);
        let mut audio_out = pump_audio_modal(&mut editor, &ctx_audio);
        for _ in 0..100 {
            if !has_pending_audio_preview(&ctx_audio)
                && export_audio_draft(&ctx_audio).is_some_and(|d| d.computed_key.is_some())
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            audio_out = pump_audio_modal(&mut editor, &ctx_audio);
        }
        let settled = export_audio_draft(&ctx_audio).expect("audio draft settled");
        assert!(settled.has_sound);
        assert!(settled.cached_peaks.is_some());
        let first_key = settled.computed_key.clone();
        for _ in 0..3 {
            audio_out = pump_audio_modal(&mut editor, &ctx_audio);
            assert!(!has_pending_audio_preview(&ctx_audio));
        }
        assert_eq!(
            export_audio_draft(&ctx_audio).unwrap().computed_key,
            first_key
        );
        let has_footer_estimate = audio_out
            .platform_output
            .accesskit_update
            .as_ref()
            .is_some_and(|u| {
                u.nodes
                    .iter()
                    .filter_map(|(_, n)| n.label().or(n.value()))
                    .any(|lbl| lbl.contains(" · ~ "))
            });
        assert!(has_footer_estimate);
        editor.cancel_export_audio(&ctx_audio);
    }
}
