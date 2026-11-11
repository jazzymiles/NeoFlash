//! Export Lottie (`R-043`, Figma `BUz3pseopiuOg2Uiy3Fog7` › `v2 · Panels` › Dark `134:19883`,
//! Light `136:20439`) modal dialog and `Lottie export — N notes` compatibility report modal.
//!
//! Opened by `File ▸ Export ▸ Export Lottie…` (`⌥⇧⌘L`, [`crate::commands::CommandId::FileExportLottie`])
//! and by `Library ▸ Export symbol as Lottie…` (which preselects that symbol as `Source` and turns
//! `Background colour layer` off).
use super::dialog_helpers::{group, preview_box};
use super::planned::caption;
use crate::app::Editor;
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::{Id, Project, SymbolKind};
use neoflash_export::{
    LottieExportReport, LottieFormat, LottieImageFormat, LottieImageMode, LottieMovieClipMode,
    LottieNote, LottieNoteKind, LottieOptions, LottieTargetPlayer, LottieTextMode,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
};
use std::time::Duration;

fn select_index(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    selected: &mut usize,
    labels: &[&str],
    width: f32,
) -> bool {
    let current = labels.get(*selected).copied().unwrap_or("");
    let mut changed = false;
    c::Select::new(id_salt, current)
        .width(width)
        .show(ui, |ui| {
            for (idx, label) in labels.iter().copied().enumerate() {
                if ui
                    .add(c::MenuItem::new(label).checked(*selected == idx))
                    .clicked()
                {
                    if *selected != idx {
                        *selected = idx;
                        changed = true;
                    }
                    ui.close();
                }
            }
        });
    changed
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LottieSourceChoice {
    AllScenes,
    Scene(Id),
    Symbol(Id),
}

const SCALE_PRESETS: &[(f64, &str)] = &[
    (0.5, "0.5×"),
    (1.0, "1×"),
    (2.0, "2×"),
    (3.0, "3×"),
    (4.0, "4×"),
];

const FPS_PRESETS: &[Option<u32>] = &[None, Some(12), Some(24), Some(30), Some(60)];

#[derive(Debug)]
enum LottieTaskOutcome {
    Estimate(Result<LottieExportReport, String>),
    Export {
        path: PathBuf,
        result: Result<LottieExportReport, String>,
    },
}

struct RunningLottieTask {
    generation: u64,
    is_export: bool,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    rx: Receiver<Option<LottieTaskOutcome>>,
}

#[derive(Default)]
pub(crate) struct LottieAsyncWorker {
    generation: u64,
    running: Option<RunningLottieTask>,
}

impl std::fmt::Debug for LottieAsyncWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LottieAsyncWorker")
            .field("generation", &self.generation)
            .field("busy", &self.running.is_some())
            .finish()
    }
}

impl Drop for LottieAsyncWorker {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl LottieAsyncWorker {
    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(task) = self.running.take() {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn busy(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.generation == self.generation)
    }

    fn is_exporting(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.generation == self.generation && r.is_export)
    }

    fn progress(&self) -> f32 {
        self.running
            .as_ref()
            .filter(|r| r.generation == self.generation)
            .map(|r| f32::from_bits(r.progress.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }

    fn spawn_estimate(
        &mut self,
        project: Project,
        options: LottieOptions,
    ) -> Option<LottieTaskOutcome> {
        self.cancel();
        let generation = self.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let cancel_worker = cancel.clone();
        let progress_worker = progress.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = neoflash_export::export_lottie_json_with_progress(&project, &options, |p| {
                if cancel_worker.load(Ordering::Relaxed) {
                    return false;
                }
                let frac = if p.total_steps > 0 {
                    (p.completed_steps as f32 / p.total_steps as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                progress_worker.store(frac.to_bits(), Ordering::Relaxed);
                true
            });
            if cancel_worker.load(Ordering::Relaxed) {
                let _ = tx.send(None);
                return;
            }
            let outcome = match res {
                Ok((_, report)) => LottieTaskOutcome::Estimate(Ok(report)),
                Err(e) => LottieTaskOutcome::Estimate(Err(e.to_string())),
            };
            let _ = tx.send(Some(outcome));
        });

        // Fast-path non-blocking budget (12 ms < 1 frame at 60Hz): resolves instant estimates
        // without flicker while keeping large documents and transcoding off the UI thread.
        match rx.recv_timeout(Duration::from_millis(12)) {
            Ok(Some(outcome)) => Some(outcome),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => {
                self.running = Some(RunningLottieTask {
                    generation,
                    is_export: false,
                    cancel,
                    progress,
                    rx,
                });
                None
            }
        }
    }

    fn spawn_export(
        &mut self,
        project: Project,
        path: PathBuf,
        options: LottieOptions,
    ) -> Option<LottieTaskOutcome> {
        self.cancel();
        let generation = self.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let cancel_worker = cancel.clone();
        let progress_worker = progress.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = neoflash_export::export_project_lottie_with_progress(
                &project,
                &path,
                &options,
                |p| {
                    if cancel_worker.load(Ordering::Relaxed) {
                        return false;
                    }
                    let frac = if p.total_steps > 0 {
                        (p.completed_steps as f32 / p.total_steps as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    progress_worker.store(frac.to_bits(), Ordering::Relaxed);
                    true
                },
            );
            if cancel_worker.load(Ordering::Relaxed) {
                let _ = tx.send(None);
                return;
            }
            let outcome = LottieTaskOutcome::Export {
                path,
                result: res.map_err(|e| e.to_string()),
            };
            let _ = tx.send(Some(outcome));
        });

        match rx.recv_timeout(Duration::from_millis(12)) {
            Ok(Some(outcome)) => Some(outcome),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => {
                self.running = Some(RunningLottieTask {
                    generation,
                    is_export: true,
                    cancel,
                    progress,
                    rx,
                });
                None
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) -> Option<LottieTaskOutcome> {
        let Some(running) = &self.running else {
            return None;
        };
        match running.rx.try_recv() {
            Ok(outcome) => {
                let gen_match = running.generation == self.generation;
                self.running = None;
                ctx.request_repaint();
                if gen_match { outcome } else { None }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint();
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.running = None;
                None
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExportLottieDraft {
    pub(crate) source: LottieSourceChoice,
    pub(crate) custom_range: bool,
    pub(crate) from_frame: f64,
    pub(crate) to_frame: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) constrain: bool,
    pub(crate) aspect: f64,
    pub(crate) scale_idx: usize,
    pub(crate) fps_idx: usize,
    pub(crate) options: LottieOptions,
    pub(crate) preview_report: Option<LottieExportReport>,
    pub(crate) blocking_error: Option<String>,
    pub(crate) show_notes_modal: bool,
    pub(crate) dirty: bool,
    pub(crate) worker: Arc<Mutex<LottieAsyncWorker>>,
}

impl ExportLottieDraft {
    pub(crate) fn for_project(project: &Project, symbol_id: Option<&str>) -> Self {
        let mut options = LottieOptions::default();
        let source = if let Some(sym_id) = symbol_id {
            options.symbol = Some(sym_id.to_owned());
            options.include_background = false;
            LottieSourceChoice::Symbol(sym_id.to_owned())
        } else {
            LottieSourceChoice::AllScenes
        };
        let total_frames = source_frame_count(project, &source);
        let w = project.width.max(1) as f64;
        let h = project.height.max(1) as f64;
        let mut draft = Self {
            source,
            custom_range: false,
            from_frame: 1.0,
            to_frame: f64::from(total_frames),
            width: w,
            height: h,
            constrain: true,
            aspect: w / h,
            scale_idx: 1, // 1x
            fps_idx: 0,   // Document fps
            options,
            preview_report: None,
            blocking_error: None,
            show_notes_modal: false,
            dirty: true,
            worker: Arc::new(Mutex::new(LottieAsyncWorker::default())),
        };
        draft.refresh_estimate(project);
        draft
    }

    pub(crate) fn cancel_worker(&self) {
        if let Ok(mut w) = self.worker.lock() {
            w.cancel();
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.worker.lock().is_ok_and(|w| w.busy())
    }

    pub(crate) fn is_exporting(&self) -> bool {
        self.worker.lock().is_ok_and(|w| w.is_exporting())
    }

    pub(crate) fn progress(&self) -> f32 {
        self.worker.lock().map(|w| w.progress()).unwrap_or(0.0)
    }

    pub(crate) fn sync_options(&mut self, project: &Project) {
        match &self.source {
            LottieSourceChoice::AllScenes => {
                self.options.symbol = None;
                self.options.scene = None;
            }
            LottieSourceChoice::Scene(id) => {
                self.options.symbol = None;
                self.options.scene = Some(id.clone());
            }
            LottieSourceChoice::Symbol(id) => {
                self.options.symbol = Some(id.clone());
                self.options.scene = None;
            }
        }
        let max_frames = source_frame_count(project, &self.source);
        if self.custom_range {
            let from = (self.from_frame.round() as u32).max(1);
            let to = (self.to_frame.round() as u32).max(1);
            self.options.frame_range = Some((from, to));
            if from > to || from > max_frames || to > max_frames {
                self.blocking_error = Some(format!(
                    "Frame range {from}–{to} is outside 1–{max_frames}."
                ));
            } else {
                self.blocking_error = None;
            }
        } else {
            self.options.frame_range = None;
            self.blocking_error = None;
        }
        let w = (self.width.round() as u32).clamp(1, 16384);
        let h = (self.height.round() as u32).clamp(1, 16384);
        self.options.width = Some(w);
        self.options.height = Some(h);
        self.options.scale = SCALE_PRESETS
            .get(self.scale_idx)
            .map(|(s, _)| *s)
            .unwrap_or(1.0);
        self.options.fps = FPS_PRESETS.get(self.fps_idx).copied().flatten();
    }

    pub(crate) fn refresh_estimate(&mut self, project: &Project) {
        self.sync_options(project);
        self.dirty = false;
        if self.blocking_error.is_some() {
            self.cancel_worker();
            return;
        }
        let outcome = self
            .worker
            .lock()
            .ok()
            .and_then(|mut w| w.spawn_estimate(project.clone(), self.options.clone()));
        if let Some(LottieTaskOutcome::Estimate(res)) = outcome {
            match res {
                Ok(report) => {
                    self.preview_report = Some(report);
                    self.blocking_error = None;
                }
                Err(err) => {
                    self.blocking_error = Some(err);
                }
            }
        }
    }
}

fn root_scene_ids(project: &Project) -> Vec<Id> {
    if project.scenes.is_empty() {
        vec![project.root.clone()]
    } else {
        project.scenes.clone()
    }
}

fn source_frame_count(project: &Project, source: &LottieSourceChoice) -> u32 {
    match source {
        LottieSourceChoice::AllScenes => root_scene_ids(project)
            .into_iter()
            .filter_map(|id| project.symbol(&id))
            .map(|s| s.timeline.frame_count.max(1))
            .sum::<u32>()
            .max(1),
        LottieSourceChoice::Scene(id) | LottieSourceChoice::Symbol(id) => project
            .symbol(id)
            .map(|s| s.timeline.frame_count.max(1))
            .unwrap_or(1),
    }
}

fn source_label(project: &Project, source: &LottieSourceChoice) -> String {
    match source {
        LottieSourceChoice::AllScenes => "Main timeline · all scenes".into(),
        LottieSourceChoice::Scene(id) => project
            .symbol(id)
            .map(|s| format!("Scene: {}", s.name))
            .unwrap_or_else(|| "Current scene".into()),
        LottieSourceChoice::Symbol(id) => project
            .symbol(id)
            .map(|s| format!("Symbol: {}", s.name))
            .unwrap_or_else(|| format!("Symbol: {id}")),
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

impl Editor {
    fn persist_lottie_export_options(&mut self, options: &LottieOptions) {
        if options.symbol.is_none() {
            let mut saved = options.clone();
            saved.symbol = None;
            saved.scene = None;
            saved.frame_range = None;
            self.workspace.export_lottie_options = Some(saved);
        }
    }

    /// Open the `Export Lottie` modal dialog (`R-043`).
    pub fn open_export_lottie(&mut self, symbol_id: Option<&str>) {
        self.finish_edits();
        let mut draft = ExportLottieDraft::for_project(&self.project, symbol_id);
        if symbol_id.is_none()
            && let Some(saved) = &self.workspace.export_lottie_options
        {
            draft.options.format = saved.format;
            draft.options.include_background = saved.include_background;
            draft.options.include_hidden_layers = saved.include_hidden_layers;
            draft.options.include_markers = saved.include_markers;
            draft.options.movie_clip_mode = saved.movie_clip_mode;
            draft.options.text_mode = saved.text_mode;
            draft.options.image_mode = saved.image_mode;
            draft.options.pretty = saved.pretty;
            draft.options.precision = saved.precision;
            draft.options.player = saved.player;
            draft.options.html_preview = saved.html_preview;
            draft.options.image_format = saved.image_format;
            if let Some(idx) = SCALE_PRESETS
                .iter()
                .position(|(s, _)| (*s - saved.scale).abs() < 0.01)
            {
                draft.scale_idx = idx;
            }
            if let Some(idx) = FPS_PRESETS.iter().position(|p| *p == saved.fps) {
                draft.fps_idx = idx;
            }
            draft.dirty = true;
            draft.refresh_estimate(&self.project);
        }
        self.export_lottie = Some(draft);
    }

    fn finish_lottie_export(
        &mut self,
        path: &std::path::Path,
        result: Result<LottieExportReport, String>,
    ) -> Result<LottieExportReport, String> {
        match result {
            Ok(report) => {
                self.message(format!(
                    "Lottie exported: {} ({} bytes, {} layers, {} frames)",
                    path.display(),
                    report.bytes,
                    report.layer_count,
                    report.frame_count
                ));
                for warning in &report.warnings {
                    self.message(format!("Lottie note: {warning}"));
                }
                if !report.warnings.is_empty() {
                    self.show_panel(PanelId::Output);
                }
                self.last_lottie_report = Some(report.clone());
                Ok(report)
            }
            Err(e) => {
                let err = format!("Lottie export failed: {e}");
                self.message(err.clone());
                self.show_panel(PanelId::Output);
                Err(err)
            }
        }
    }

    /// Export using a specific `LottieOptions` configuration to `path`.
    #[cfg(test)]
    pub fn export_lottie_with_options_to_path(
        &mut self,
        path: &std::path::Path,
        options: &LottieOptions,
    ) -> Result<LottieExportReport, String> {
        self.finish_edits();
        let res = neoflash_export::export_project_lottie(&self.project, path, options)
            .map_err(|e| e.to_string());
        self.finish_lottie_export(path, res)
    }

    fn choose_export_lottie_path(&self, draft: &ExportLottieDraft) -> Option<PathBuf> {
        let base_name = match &draft.source {
            LottieSourceChoice::AllScenes => self.project.name.as_str(),
            LottieSourceChoice::Scene(id) | LottieSourceChoice::Symbol(id) => self
                .project
                .symbol(id)
                .map(|s| s.name.as_str())
                .unwrap_or(self.project.name.as_str()),
        };
        let stem: String = base_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let clean_stem = if stem.trim_matches('-').is_empty() {
            "animation"
        } else {
            stem.trim_matches('-')
        };
        let (ext, filter_name, filter_exts): (&str, &str, &[&str]) = match draft.options.format {
            LottieFormat::Json => ("lottie.json", "Lottie JSON", &["json"]),
            LottieFormat::DotLottie => ("lottie", "dotLottie archive", &["lottie"]),
        };
        let file_name = format!("{clean_stem}.{ext}");
        rfd::FileDialog::new()
            .set_title("Export Lottie")
            .add_filter(filter_name, filter_exts)
            .set_file_name(&file_name)
            .save_file()
    }

    pub(crate) fn jump_to_lottie_note(&mut self, note: &LottieNote) {
        if let Some(sym_id) = note.symbol_id.as_deref()
            && self.project.symbol(sym_id).is_some()
        {
            self.enter_explorer_timeline(sym_id);
        }
        if let Some(layer_id) = note.layer_id.as_deref()
            && let Some(idx) = self
                .symbol()
                .timeline
                .layers
                .iter()
                .position(|l| l.id == layer_id)
        {
            self.layer = idx;
        }
        if let Some(f) = note.frame {
            self.seek_frame(f);
        }
    }

    /// Render the `Export Lottie` modal dialog and optional `Lottie export — N notes` report modal (`R-043`).
    pub(crate) fn export_lottie_modal(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.export_lottie.clone() else {
            return;
        };

        let polled = draft.worker.lock().ok().and_then(|mut w| w.poll(ctx));
        if let Some(outcome) = polled {
            match outcome {
                LottieTaskOutcome::Estimate(Ok(report)) => {
                    draft.preview_report = Some(report);
                    draft.blocking_error = None;
                }
                LottieTaskOutcome::Estimate(Err(err)) => {
                    draft.blocking_error = Some(err);
                }
                LottieTaskOutcome::Export { path, result } => {
                    if result.is_ok() {
                        self.persist_lottie_export_options(&draft.options);
                    }
                    let _ = self.finish_lottie_export(&path, result);
                    self.export_lottie = None;
                    return;
                }
            }
        }

        if draft.dirty && !draft.is_exporting() {
            draft.refresh_estimate(&self.project);
        }

        if draft.show_notes_modal {
            let mut close_notes = false;
            let mut jump_target: Option<LottieNote> = None;
            let mut copy_text: Option<String> = None;
            let notes = draft
                .preview_report
                .as_ref()
                .map(|r| r.notes.clone())
                .unwrap_or_default();
            let nf = neoflash_ui::palette(ctx);
            let title = format!("Lottie export — {} notes", notes.len());
            let modal = egui::Modal::new(egui::Id::new("export-lottie-notes-modal"))
                .frame(c::Surface::Dialog.frame(ctx))
                .backdrop_color(nf.overlay)
                .show(ctx, |ui| {
                    ui.set_width(560.0);
                    close_notes |= c::DialogHeader { title: &title }.show(ui);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(space::S5 as i8))
                        .show(ui, |ui| {
                            ui.set_width(560.0 - 2.0 * space::S5);
                            ui.spacing_mut().item_spacing.y = space::S2;
                            egui::ScrollArea::vertical()
                                .max_height(320.0)
                                .show(ui, |ui| {
                                    for kind in [
                                        LottieNoteKind::Missing,
                                        LottieNoteKind::Approximated,
                                        LottieNoteKind::NotExported,
                                    ] {
                                        let group_notes: Vec<&LottieNote> =
                                            notes.iter().filter(|n| n.kind == kind).collect();
                                        if group_notes.is_empty() {
                                            continue;
                                        }
                                        group(
                                            ui,
                                            &format!("{} ({})", kind.label(), group_notes.len()),
                                        );
                                        for note in group_notes {
                                            let sev = match note.kind {
                                                LottieNoteKind::Missing => c::Severity::Error,
                                                LottieNoteKind::Approximated => {
                                                    c::Severity::Warning
                                                }
                                                LottieNoteKind::NotExported => c::Severity::Info,
                                            };
                                            ui.horizontal(|ui| {
                                                ui.add(c::LogLine {
                                                    severity: sev,
                                                    time: &note.location,
                                                    text: &note.message,
                                                });
                                                if (note.symbol_id.is_some()
                                                    || note.layer_id.is_some()
                                                    || note.frame.is_some())
                                                    && ui.add(c::Button::ghost("Show")).clicked()
                                                {
                                                    jump_target = Some((*note).clone());
                                                }
                                            });
                                        }
                                    }
                                });
                        });
                    c::surface::dialog_footer(
                        ui,
                        |ui| {
                            if ui.add(c::Button::new("Copy report")).clicked() {
                                let formatted = notes
                                    .iter()
                                    .map(|n| {
                                        format!(
                                            "[{}] {}: {}",
                                            n.kind.label(),
                                            n.location,
                                            n.message
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                copy_text = Some(formatted);
                            }
                        },
                        |ui| {
                            close_notes |= ui.add(c::Button::primary("Close")).clicked();
                        },
                    );
                });
            if let Some(text) = copy_text {
                ctx.copy_text(text);
                self.message("Copied Lottie compatibility report.");
            }
            if let Some(target) = jump_target {
                draft.show_notes_modal = false;
                self.export_lottie = Some(draft);
                self.jump_to_lottie_note(&target);
                return;
            }
            if close_notes || modal.should_close() {
                draft.show_notes_modal = false;
            }
            self.export_lottie = Some(draft);
            return;
        }

        let mut cancel = false;
        let mut do_export = false;
        let nf = neoflash_ui::palette(ctx);
        let modal = egui::Modal::new(egui::Id::new("export-lottie-modal"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                ui.set_width(760.0);
                cancel |= c::DialogHeader {
                    title: "Export Lottie",
                }
                .show(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(760.0 - 2.0 * space::S5);
                        ui.horizontal_top(|ui| {
                            // ── Left column (300 pt): Preview + Caption + Compatibility card ──
                            ui.vertical(|ui| {
                                ui.set_width(300.0);
                                ui.spacing_mut().item_spacing.y = space::S2;
                                let src_title = source_label(&self.project, &draft.source);
                                preview_box(ui, 300.0, 188.0, &src_title);
                                let scenes_cnt = root_scene_ids(&self.project).len();
                                let frames_cnt = draft
                                    .preview_report
                                    .as_ref()
                                    .map(|r| r.frame_count)
                                    .unwrap_or_else(|| {
                                        source_frame_count(&self.project, &draft.source)
                                    });
                                let fps_val = draft
                                    .options
                                    .fps
                                    .unwrap_or(self.project.fps)
                                    .clamp(1, 240);
                                let out_w = draft
                                    .preview_report
                                    .as_ref()
                                    .map(|r| r.width)
                                    .unwrap_or(self.project.width);
                                let out_h = draft
                                    .preview_report
                                    .as_ref()
                                    .map(|r| r.height)
                                    .unwrap_or(self.project.height);
                                caption(
                                    ui,
                                    &format!(
                                        "{src_title} · {scenes_cnt} scene(s) · {frames_cnt} frames · {fps_val} fps · {out_w} × {out_h}"
                                    ),
                                );

                                group(ui, "Compatibility");
                                egui::Frame::new()
                                    .fill(nf.bg_raised)
                                    .corner_radius(egui::CornerRadius::same(radius::MD as u8))
                                    .stroke(egui::Stroke::new(1.0_f32, nf.border_subtle))
                                    .inner_margin(egui::Margin::same(space::S3 as i8))
                                    .show(ui, |ui| {
                                        ui.set_width(300.0 - 2.0 * space::S3);
                                        let (exported, approx, omitted, note_cnt) =
                                            if let Some(rep) = &draft.preview_report {
                                                (
                                                    rep.exported_count,
                                                    rep.approximated_count,
                                                    rep.not_exported_count,
                                                    rep.notes.len(),
                                                )
                                            } else {
                                                (0, 0, 0, 0)
                                            };
                                        ui.horizontal(|ui| {
                                            ui.add(c::Badge::new(
                                                &format!("{exported} exported"),
                                                c::BadgeType::Accent,
                                            ));
                                            ui.add(c::Badge::new(
                                                &format!("{approx} approx."),
                                                if approx > 0 {
                                                    c::BadgeType::Warning
                                                } else {
                                                    c::BadgeType::Neutral
                                                },
                                            ));
                                            ui.add(c::Badge::new(
                                                &format!("{omitted} omitted"),
                                                if omitted > 0 {
                                                    c::BadgeType::Accent
                                                } else {
                                                    c::BadgeType::Neutral
                                                },
                                            ));
                                        });
                                        ui.horizontal(|ui| {
                                            caption(ui, &format!("{note_cnt} compatibility note(s)"));
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .add_enabled(
                                                            note_cnt > 0,
                                                            c::Button::ghost("Show details"),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.show_notes_modal = true;
                                                    }
                                                },
                                            );
                                        });
                                    });

                                if let Some(err) = &draft.blocking_error {
                                    ui.add(c::LogLine {
                                        severity: c::Severity::Error,
                                        time: "",
                                        text: err,
                                    });
                                } else if let Some(rep) = &draft.preview_report {
                                    for note in rep.notes.iter().take(2) {
                                        ui.add(c::LogLine {
                                            severity: match note.kind {
                                                LottieNoteKind::Missing => c::Severity::Error,
                                                LottieNoteKind::Approximated => {
                                                    c::Severity::Warning
                                                }
                                                LottieNoteKind::NotExported => c::Severity::Info,
                                            },
                                            time: "",
                                            text: &note.message,
                                        });
                                    }
                                }
                            });

                            ui.add_space(space::S5);

                            // ── Right column: Format, Source, Range, Size, Layers, Content, Output ──
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = space::S2;
                                let mut fmt_idx = match draft.options.format {
                                    LottieFormat::Json => 0usize,
                                    LottieFormat::DotLottie => 1usize,
                                };
                                c::Segmented::show(
                                    ui,
                                    &mut fmt_idx,
                                    &[(0, "JSON"), (1, "dotLottie (.lottie)")],
                                    false,
                                );
                                let new_fmt = if fmt_idx == 1 {
                                    LottieFormat::DotLottie
                                } else {
                                    LottieFormat::Json
                                };
                                if new_fmt != draft.options.format {
                                    draft.options.format = new_fmt;
                                    draft.dirty = true;
                                }

                                // Source + Range
                                group(ui, "Source & Range");
                                let mut sources: Vec<(LottieSourceChoice, String)> = vec![(
                                    LottieSourceChoice::AllScenes,
                                    "Main timeline · all scenes".into(),
                                )];
                                for scene_id in root_scene_ids(&self.project) {
                                    if let Some(sym) = self.project.symbol(&scene_id) {
                                        sources.push((
                                            LottieSourceChoice::Scene(scene_id),
                                            format!("Scene: {}", sym.name),
                                        ));
                                    }
                                }
                                for sym in &self.project.symbols {
                                    if !self.project.is_scene(&sym.id) && sym.id != self.project.root
                                    {
                                        let kind_str = match sym.kind {
                                            SymbolKind::MovieClip => "MovieClip",
                                            SymbolKind::Graphic => "Graphic",
                                            SymbolKind::Button => "Button",
                                        };
                                        sources.push((
                                            LottieSourceChoice::Symbol(sym.id.clone()),
                                            format!("Symbol: {} ({kind_str})", sym.name),
                                        ));
                                    }
                                }
                                let mut src_idx = sources
                                    .iter()
                                    .position(|(c, _)| *c == draft.source)
                                    .unwrap_or(0);
                                let src_labels: Vec<&str> =
                                    sources.iter().map(|(_, l)| l.as_str()).collect();
                                if select_index(
                                    ui,
                                    "lottie-source",
                                    &mut src_idx,
                                    &src_labels,
                                    ui.available_width(),
                                ) {
                                    draft.source = sources[src_idx].0.clone();
                                    let max_f = source_frame_count(&self.project, &draft.source);
                                    draft.from_frame = 1.0;
                                    draft.to_frame = f64::from(max_f);
                                    if matches!(draft.source, LottieSourceChoice::Symbol(_)) {
                                        draft.options.include_background = false;
                                    }
                                    draft.dirty = true;
                                }

                                ui.horizontal(|ui| {
                                    let mut range_idx = usize::from(draft.custom_range);
                                    if select_index(
                                        ui,
                                        "lottie-range",
                                        &mut range_idx,
                                        &["All frames", "Frame range"],
                                        160.0,
                                    ) {
                                        draft.custom_range = range_idx == 1;
                                        draft.dirty = true;
                                    }
                                    if draft.custom_range {
                                        if ui
                                            .add(
                                                c::Field::new(&mut draft.from_frame, "From frame")
                                                    .prefix("From")
                                                    .decimals(0)
                                                    .width(96.0),
                                            )
                                            .changed()
                                        {
                                            draft.dirty = true;
                                        }
                                        if ui
                                            .add(
                                                c::Field::new(&mut draft.to_frame, "To frame")
                                                    .prefix("To")
                                                    .decimals(0)
                                                    .width(96.0),
                                            )
                                            .changed()
                                        {
                                            draft.dirty = true;
                                        }
                                    }
                                });

                                // Size & Frame rate
                                group(ui, "Size & Frame rate");
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            c::Field::new(&mut draft.width, "Width")
                                                .prefix("W")
                                                .decimals(0)
                                                .width(96.0),
                                        )
                                        .changed()
                                    {
                                        if draft.constrain && draft.aspect > 1e-6 {
                                            draft.height = (draft.width / draft.aspect).round().max(1.0);
                                        }
                                        draft.dirty = true;
                                    }
                                    if ui
                                        .add(
                                            c::Field::new(&mut draft.height, "Height")
                                                .prefix("H")
                                                .decimals(0)
                                                .width(96.0),
                                        )
                                        .changed()
                                    {
                                        if draft.constrain && draft.aspect > 1e-6 {
                                            draft.width = (draft.height * draft.aspect).round().max(1.0);
                                        }
                                        draft.dirty = true;
                                    }
                                    if ui
                                        .add(
                                            c::IconButton::new(i16::LINK, "Constrain proportions")
                                                .selected(draft.constrain),
                                        )
                                        .clicked()
                                    {
                                        draft.constrain = !draft.constrain;
                                        if draft.constrain && draft.height > 0.0 {
                                            draft.aspect = draft.width / draft.height;
                                        }
                                    }
                                    let scale_labels: Vec<&str> =
                                        SCALE_PRESETS.iter().map(|(_, l)| *l).collect();
                                    if select_index(
                                        ui,
                                        "lottie-scale",
                                        &mut draft.scale_idx,
                                        &scale_labels,
                                        72.0,
                                    ) {
                                        draft.dirty = true;
                                    }
                                });
                                let doc_fps_label =
                                    format!("Frame rate: Document ({} fps)", self.project.fps);
                                let fps_labels = [
                                    doc_fps_label.as_str(),
                                    "Frame rate: 12 fps",
                                    "Frame rate: 24 fps",
                                    "Frame rate: 30 fps",
                                    "Frame rate: 60 fps",
                                ];
                                if select_index(
                                    ui,
                                    "lottie-fps",
                                    &mut draft.fps_idx,
                                    &fps_labels,
                                    ui.available_width(),
                                ) {
                                    draft.dirty = true;
                                }

                                // Layers
                                group(ui, "Layers");
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .add(c::Checkbox::new(
                                            &mut draft.options.include_background,
                                            "Background colour layer",
                                        ))
                                        .changed()
                                    {
                                        draft.dirty = true;
                                    }
                                    if ui
                                        .add(c::Checkbox::new(
                                            &mut draft.options.include_hidden_layers,
                                            "Hidden layers",
                                        ))
                                        .changed()
                                    {
                                        draft.dirty = true;
                                    }
                                    if ui
                                        .add(c::Checkbox::new(
                                            &mut draft.options.include_markers,
                                            "Frame labels as markers",
                                        ))
                                        .changed()
                                    {
                                        draft.dirty = true;
                                    }
                                });

                                // Content
                                group(ui, "Content");
                                ui.horizontal(|ui| {
                                    let mut mc_idx = match draft.options.movie_clip_mode {
                                        LottieMovieClipMode::Nested => 0usize,
                                        LottieMovieClipMode::FirstFrame => 1usize,
                                    };
                                    if select_index(
                                        ui,
                                        "lottie-mc-mode",
                                        &mut mc_idx,
                                        &["Movie clips: Nested", "Movie clips: First frame"],
                                        196.0,
                                    ) {
                                        draft.options.movie_clip_mode = if mc_idx == 1 {
                                            LottieMovieClipMode::FirstFrame
                                        } else {
                                            LottieMovieClipMode::Nested
                                        };
                                        draft.dirty = true;
                                    }

                                    let mut txt_idx = match draft.options.text_mode {
                                        LottieTextMode::Shapes => 0usize,
                                        LottieTextMode::TextLayers => 1usize,
                                    };
                                    if select_index(
                                        ui,
                                        "lottie-text-mode",
                                        &mut txt_idx,
                                        &["Text: Shapes", "Text: Text layers"],
                                        196.0,
                                    ) {
                                        draft.options.text_mode = if txt_idx == 0 {
                                            LottieTextMode::Shapes
                                        } else {
                                            LottieTextMode::TextLayers
                                        };
                                        draft.dirty = true;
                                    }
                                });
                                ui.horizontal(|ui| {
                                    let mut img_idx = match draft.options.image_mode {
                                        LottieImageMode::Embed => 0usize,
                                        LottieImageMode::Folder => 1usize,
                                    };
                                    if select_index(
                                        ui,
                                        "lottie-img-mode",
                                        &mut img_idx,
                                        &["Images: Embed", "Images: Folder"],
                                        196.0,
                                    ) {
                                        draft.options.image_mode = if img_idx == 1 {
                                            LottieImageMode::Folder
                                        } else {
                                            LottieImageMode::Embed
                                        };
                                        draft.dirty = true;
                                    }

                                    let mut fmt_img_idx = match draft.options.image_format {
                                        LottieImageFormat::Original => 0usize,
                                        LottieImageFormat::Png => 1usize,
                                        LottieImageFormat::Jpeg => 2usize,
                                    };
                                    if select_index(
                                        ui,
                                        "lottie-img-fmt",
                                        &mut fmt_img_idx,
                                        &[
                                            "Image format: Original",
                                            "Image format: PNG",
                                            "Image format: JPEG",
                                        ],
                                        196.0,
                                    ) {
                                        draft.options.image_format = match fmt_img_idx {
                                            1 => LottieImageFormat::Png,
                                            2 => LottieImageFormat::Jpeg,
                                            _ => LottieImageFormat::Original,
                                        };
                                        draft.dirty = true;
                                    }
                                });

                                // Output
                                group(ui, "Output");
                                ui.horizontal(|ui| {
                                    let mut minify = !draft.options.pretty;
                                    if ui.add(c::Switch::new(&mut minify, "Minify")).changed() {
                                        draft.options.pretty = !minify;
                                        draft.dirty = true;
                                    }
                                    let mut prec = f64::from(draft.options.precision);
                                    if ui
                                        .add(
                                            c::Field::new(&mut prec, "Precision")
                                                .prefix("Precision")
                                                .decimals(0)
                                                .width(116.0),
                                        )
                                        .changed()
                                    {
                                        draft.options.precision = (prec.round() as i32).clamp(0, 4) as u8;
                                        draft.dirty = true;
                                    }
                                    let mut player_idx = match draft.options.player {
                                        LottieTargetPlayer::LottieWeb => 0usize,
                                        LottieTargetPlayer::Mobile => 1usize,
                                        LottieTargetPlayer::Skottie => 2usize,
                                    };
                                    if select_index(
                                        ui,
                                        "lottie-player",
                                        &mut player_idx,
                                        &[
                                            "Player: lottie-web",
                                            "Player: Mobile",
                                            "Player: Skottie",
                                        ],
                                        164.0,
                                    ) {
                                        draft.options.player = match player_idx {
                                            1 => LottieTargetPlayer::Mobile,
                                            2 => LottieTargetPlayer::Skottie,
                                            _ => LottieTargetPlayer::LottieWeb,
                                        };
                                        draft.dirty = true;
                                    }
                                });
                                ui.add(c::Checkbox::new(
                                    &mut draft.options.html_preview,
                                    "Also write HTML preview page",
                                ));
                            });
                        });
                    });

                let busy = draft.is_busy();
                let exporting = draft.is_exporting();
                let progress = draft.progress();
                let footer_summary = if let Some(rep) = &draft.preview_report {
                    format!(
                        "~ {} · {} layers · {} frames",
                        format_bytes(rep.bytes),
                        rep.layer_count,
                        rep.frame_count
                    )
                } else {
                    "~ 0 B · 0 layers · 0 frames".into()
                };
                c::surface::dialog_footer(
                    ui,
                    |ui| {
                        if exporting {
                            c::progress(ui, "Exporting Lottie…", Some(progress), 260.0);
                        } else if busy {
                            c::progress(ui, "Computing estimate…", Some(progress), 260.0);
                        } else {
                            ui.label(
                                rich(&footer_summary, TextStyle::Small).color(nf.text_secondary),
                            );
                        }
                    },
                    |ui| {
                        do_export |= ui
                            .add_enabled(
                                !exporting && draft.blocking_error.is_none(),
                                c::Button::primary("Export Lottie"),
                            )
                            .clicked();
                        cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });

        do_export |= !draft.is_exporting()
            && draft.blocking_error.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

        if cancel || modal.should_close() {
            let was_exporting = draft.is_exporting();
            draft.cancel_worker();
            if was_exporting {
                self.message("Lottie export cancelled.");
                self.export_lottie = Some(draft);
            } else {
                self.export_lottie = None;
            }
        } else if do_export {
            draft.sync_options(&self.project);
            if let Some(path) = self.choose_export_lottie_path(&draft) {
                self.persist_lottie_export_options(&draft.options);
                let immediate = draft.worker.lock().ok().and_then(|mut w| {
                    w.spawn_export(self.project.clone(), path, draft.options.clone())
                });
                if let Some(LottieTaskOutcome::Export { path, result }) = immediate {
                    let _ = self.finish_lottie_export(&path, result);
                    self.export_lottie = None;
                } else {
                    ctx.request_repaint();
                    self.export_lottie = Some(draft);
                }
            } else {
                self.export_lottie = Some(draft);
            }
        } else {
            self.export_lottie = Some(draft);
        }
    }
}
