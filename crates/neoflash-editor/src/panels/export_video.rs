//! Export video (`DL-05` / `W3.3`, Figma `BUz3pseopiuOg2Uiy3Fog7` › `v2 · Panels` › `Dialog / Export video`)
//! modal dialog.
//!
//! Opened by `File ▸ Export ▸ Export video…` ([`crate::commands::CommandId::FileExportVideo`])
//! and by `Library ▸ Export symbol as video…`.

use super::planned::caption;
use super::planned_dialogs::group;
use crate::app::Editor;
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::{Id, Project, SymbolKind};
use neoflash_export::{
    VideoAudioBitrate, VideoEstimate, VideoExportReport, VideoFormat, VideoOptions, VideoStopMode,
    estimate_video, export_project_video, export_project_video_with_progress,
    render_project_frame_rgba, video_scene_ranges,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::tokens::{TextStyle, radius, space};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::time::Duration;

const SCALE_PRESETS: &[(f32, &str)] = &[
    (0.5, "0.5×"),
    (1.0, "1×"),
    (2.0, "2×"),
    (3.0, "3×"),
    (4.0, "4×"),
];

const FPS_PRESETS: &[Option<u32>] = &[None, Some(12), Some(24), Some(25), Some(30), Some(60)];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VideoRangeChoice {
    WholeMovie,
    CurrentScene(Id),
    CustomRange,
    Scene(Id),
    Symbol(Id),
}

struct SharedPreviewFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct RunningVideoExport {
    generation: u64,
    cancel: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    frame_index: Arc<AtomicU32>,
    frame_count: Arc<AtomicU32>,
    latest_frame: Arc<Mutex<Option<SharedPreviewFrame>>>,
    rx: Receiver<Option<(PathBuf, Result<VideoExportReport, String>)>>,
}

#[derive(Default)]
pub(crate) struct VideoAsyncWorker {
    generation: u64,
    running: Option<RunningVideoExport>,
}

impl std::fmt::Debug for VideoAsyncWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoAsyncWorker")
            .field("generation", &self.generation)
            .field("exporting", &self.running.is_some())
            .finish()
    }
}

impl Drop for VideoAsyncWorker {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl VideoAsyncWorker {
    pub(crate) fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(task) = self.running.take() {
            task.paused.store(false, Ordering::Relaxed);
            task.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn is_exporting(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.generation == self.generation)
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.generation == self.generation && r.paused.load(Ordering::Relaxed))
    }

    pub(crate) fn toggle_pause(&self) {
        if let Some(r) = &self.running
            && r.generation == self.generation
        {
            let prev = r.paused.load(Ordering::Relaxed);
            r.paused.store(!prev, Ordering::Relaxed);
        }
    }

    pub(crate) fn progress_counts(&self) -> Option<(u32, u32)> {
        let r = self.running.as_ref()?;
        if r.generation != self.generation {
            return None;
        }
        let idx = r.frame_index.load(Ordering::Relaxed);
        let cnt = r.frame_count.load(Ordering::Relaxed).max(1);
        Some((idx, cnt))
    }

    pub(crate) fn take_latest_frame(&self) -> Option<(u32, u32, Vec<u8>)> {
        let r = self.running.as_ref()?;
        if r.generation != self.generation {
            return None;
        }
        let mut guard = r.latest_frame.lock().ok()?;
        let frame = guard.take()?;
        Some((frame.width, frame.height, frame.rgba))
    }

    pub(crate) fn spawn_export(
        &mut self,
        project: Project,
        path: PathBuf,
        options: VideoOptions,
    ) -> Option<(PathBuf, Result<VideoExportReport, String>)> {
        self.cancel();
        let generation = self.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let frame_index = Arc::new(AtomicU32::new(0));
        let frame_count = Arc::new(AtomicU32::new(1));
        let latest_frame = Arc::new(Mutex::new(None));

        let cancel_w = cancel.clone();
        let paused_w = paused.clone();
        let idx_w = frame_index.clone();
        let cnt_w = frame_count.clone();
        let frame_w = latest_frame.clone();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let path_for_export = path.clone();
            let res =
                export_project_video_with_progress(&project, &path_for_export, &options, |p| {
                    while paused_w.load(Ordering::Relaxed) && !cancel_w.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    if cancel_w.load(Ordering::Relaxed) {
                        return false;
                    }
                    idx_w.store(p.frame_index, Ordering::Relaxed);
                    cnt_w.store(p.frame_count.max(1), Ordering::Relaxed);
                    if let Ok(mut guard) = frame_w.lock() {
                        *guard = Some(SharedPreviewFrame {
                            width: p.width,
                            height: p.height,
                            rgba: p.rgba,
                        });
                    }
                    true
                });
            if cancel_w.load(Ordering::Relaxed) {
                let _ = tx.send(None);
                return;
            }
            let _ = tx.send(Some((path, res.map_err(|e| e.to_string()))));
        });

        self.running = Some(RunningVideoExport {
            generation,
            cancel,
            paused,
            frame_index,
            frame_count,
            latest_frame,
            rx,
        });
        None
    }

    pub(crate) fn poll(
        &mut self,
        ctx: &egui::Context,
    ) -> Option<(PathBuf, Result<VideoExportReport, String>)> {
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

#[derive(Clone)]
pub(crate) struct ExportVideoDraft {
    pub(crate) range_choice: VideoRangeChoice,
    pub(crate) from_frame: f64,
    pub(crate) to_frame: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) aspect: f64,
    pub(crate) scale_idx: usize,
    pub(crate) fps_idx: usize,
    pub(crate) stop_after_secs: f64,
    pub(crate) options: VideoOptions,
    pub(crate) estimate: Option<VideoEstimate>,
    pub(crate) preview_rgba: Option<(u32, u32, Vec<u8>)>,
    pub(crate) preview_texture: Option<egui::TextureHandle>,
    pub(crate) preview_dirty: bool,
    pub(crate) error_message: Option<String>,
    pub(crate) worker: Arc<Mutex<VideoAsyncWorker>>,
}

impl std::fmt::Debug for ExportVideoDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportVideoDraft")
            .field("range_choice", &self.range_choice)
            .field("from_frame", &self.from_frame)
            .field("to_frame", &self.to_frame)
            .field("options", &self.options)
            .finish()
    }
}

impl ExportVideoDraft {
    pub(crate) fn for_project(
        project: &Project,
        current_frame: u32,
        symbol_id: Option<&str>,
    ) -> Self {
        let mut options = VideoOptions::default();
        let range_choice = if let Some(sym) = symbol_id {
            options.symbol = Some(sym.to_owned());
            VideoRangeChoice::Symbol(sym.to_owned())
        } else {
            VideoRangeChoice::WholeMovie
        };
        let (start_1, end_1) = range_bounds_1based(project, &range_choice);
        let w = project.width.max(2) as f64;
        let h = project.height.max(2) as f64;
        options.width = project.width.max(2);
        options.height = project.height.max(2);
        options.from_frame = start_1;
        options.to_frame = Some(end_1);

        let mut draft = Self {
            range_choice,
            from_frame: f64::from(start_1),
            to_frame: f64::from(end_1),
            width: w,
            height: h,
            aspect: (w / h).max(0.01),
            scale_idx: 1, // 1x
            fps_idx: 0,   // Document fps
            stop_after_secs: 10.0,
            options,
            estimate: None,
            preview_rgba: None,
            preview_texture: None,
            preview_dirty: true,
            error_message: None,
            worker: Arc::new(Mutex::new(VideoAsyncWorker::default())),
        };
        draft.sync_and_refresh(project, current_frame);
        draft
    }

    pub(crate) fn cancel_worker(&self) {
        if let Ok(mut w) = self.worker.lock() {
            w.cancel();
        }
    }

    pub(crate) fn is_exporting(&self) -> bool {
        self.worker.lock().is_ok_and(|w| w.is_exporting())
    }

    pub(crate) fn sync_and_refresh(&mut self, project: &Project, preview_frame: u32) {
        match &self.range_choice {
            VideoRangeChoice::WholeMovie | VideoRangeChoice::CustomRange => {
                self.options.symbol = None;
                self.options.scene = None;
            }
            VideoRangeChoice::CurrentScene(id) | VideoRangeChoice::Scene(id) => {
                self.options.symbol = None;
                self.options.scene = Some(id.clone());
            }
            VideoRangeChoice::Symbol(id) => {
                self.options.symbol = Some(id.clone());
                self.options.scene = None;
            }
        }
        let (min_1, max_1) = range_bounds_1based(project, &self.range_choice);
        let from_1 = (self.from_frame.round() as u32).clamp(min_1, max_1);
        let to_1 = (self.to_frame.round() as u32).clamp(from_1, max_1);
        self.from_frame = f64::from(from_1);
        self.to_frame = f64::from(to_1);
        self.options.from_frame = from_1;
        self.options.to_frame = Some(to_1);

        self.options.width = (self.width.round() as u32).clamp(2, 7680);
        self.options.height = (self.height.round() as u32).clamp(2, 4320);
        self.options.scale = SCALE_PRESETS
            .get(self.scale_idx)
            .map(|(s, _)| *s)
            .unwrap_or(1.0);
        self.options.fps = FPS_PRESETS
            .get(self.fps_idx)
            .copied()
            .flatten()
            .map(|v| v as f32)
            .unwrap_or(0.0);
        self.options.codec = self.options.resolved_codec();
        if let VideoStopMode::AfterSeconds(_) = self.options.stop_mode {
            self.stop_after_secs = self.stop_after_secs.clamp(0.1, 3600.0);
            self.options.stop_mode = VideoStopMode::AfterSeconds(self.stop_after_secs);
        }

        match estimate_video(project, &self.options) {
            Ok(est) => {
                self.estimate = Some(est);
                self.error_message = None;
            }
            Err(e) => {
                self.error_message = Some(e.to_string());
            }
        }

        if self.preview_dirty {
            self.preview_dirty = false;
            let (pw, ph) = preview_dimensions(project.width.max(1), project.height.max(1));
            let transparent =
                self.options.transparent_bg && self.options.format.supports_transparent_bg();
            if let Ok(rgba) = render_project_frame_rgba(
                project,
                self.options.symbol.as_deref(),
                preview_frame,
                pw,
                ph,
                transparent,
                self.options.runtime_capture,
            ) {
                self.preview_rgba = Some((pw, ph, rgba));
                self.preview_texture = None;
            }
        }
    }
}

fn preview_dimensions(stage_w: u32, stage_h: u32) -> (u32, u32) {
    let max_w = 272.0_f32;
    let max_h = 160.0_f32;
    let scale = (max_w / stage_w as f32)
        .min(max_h / stage_h as f32)
        .clamp(0.05, 2.0);
    let w = ((stage_w as f32 * scale).round() as u32).clamp(16, 300);
    let h = ((stage_h as f32 * scale).round() as u32).clamp(16, 188);
    (w, h)
}

fn range_bounds_1based(project: &Project, choice: &VideoRangeChoice) -> (u32, u32) {
    match choice {
        VideoRangeChoice::Symbol(id) => {
            let total = project
                .symbol(id)
                .map(|s| s.timeline.frame_count.max(1))
                .unwrap_or(1);
            (1, total)
        }
        VideoRangeChoice::CurrentScene(id) | VideoRangeChoice::Scene(id) => {
            for (scene_id, _, start_1, end_1) in video_scene_ranges(project) {
                if &scene_id == id {
                    return (start_1, end_1.max(start_1));
                }
            }
            let total = project
                .symbol(id)
                .map(|s| s.timeline.frame_count.max(1))
                .unwrap_or(1);
            (1, total)
        }
        VideoRangeChoice::WholeMovie | VideoRangeChoice::CustomRange => {
            let ranges = video_scene_ranges(project);
            let end_1 = ranges.last().map(|r| r.3).unwrap_or_else(|| {
                project
                    .symbol(&project.root)
                    .map(|s| s.timeline.frame_count.max(1))
                    .unwrap_or(1)
            });
            (1, end_1.max(1))
        }
    }
}

fn range_choice_label(project: &Project, choice: &VideoRangeChoice) -> String {
    match choice {
        VideoRangeChoice::WholeMovie => "Whole movie (all scenes)".into(),
        VideoRangeChoice::CurrentScene(_) => "Current scene".into(),
        VideoRangeChoice::CustomRange => "Custom range".into(),
        VideoRangeChoice::Scene(id) => project
            .symbol(id)
            .map(|s| format!("Scene: {}", s.name))
            .unwrap_or_else(|| "Scene".into()),
        VideoRangeChoice::Symbol(id) => project
            .symbol(id)
            .map(|s| format!("Symbol: {}", s.name))
            .unwrap_or_else(|| format!("Symbol: {id}")),
    }
}

fn quality_tier_label(quality: u8) -> &'static str {
    if quality >= 90 {
        "Max"
    } else if quality >= 70 {
        "High"
    } else if quality >= 45 {
        "Medium"
    } else {
        "Low"
    }
}

pub(crate) fn format_video_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn format_mm_ss(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let mins = total / 60;
    let rem = total % 60;
    format!("{mins:02}:{rem:02}")
}

fn paint_video_preview_box(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    draft: &mut ExportVideoDraft,
    width: f32,
    height: f32,
) -> egui::Response {
    let nf = neoflash_ui::palette(ctx);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let r = egui::CornerRadius::same(radius::LG as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);

    if draft.preview_texture.is_none()
        && let Some((pw, ph, rgba)) = &draft.preview_rgba
        && rgba.len() == (*pw as usize) * (*ph as usize) * 4
    {
        let image = egui::ColorImage::from_rgba_unmultiplied([*pw as usize, *ph as usize], rgba);
        draft.preview_texture =
            Some(ctx.load_texture("export-video-preview", image, egui::TextureOptions::LINEAR));
    }

    if let Some(tex) = &draft.preview_texture {
        let tex_size = tex.size_vec2();
        let max_size = rect.size() * 0.84;
        let fit_scale = (max_size.x / tex_size.x.max(1.0))
            .min(max_size.y / tex_size.y.max(1.0))
            .min(2.0);
        let stage_rect = egui::Rect::from_center_size(rect.center(), tex_size * fit_scale);
        ui.painter().rect_filled(stage_rect, 0.0, nf.bg_stage);
        ui.painter().image(
            tex.id(),
            stage_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
        ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
        ui.painter().text(
            stage.center(),
            egui::Align2::CENTER_CENTER,
            "Stage preview",
            neoflash_ui::typography::font(TextStyle::Small),
            nf.text_tertiary,
        );
    }

    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
    response
}

impl Editor {
    /// Open the `Export video` modal dialog (`DL-05` / `W3.3`).
    pub fn open_export_video(&mut self, symbol_id: Option<&str>) {
        self.finish_edits();
        self.export_video = Some(ExportVideoDraft::for_project(
            &self.project,
            self.frame,
            symbol_id,
        ));
    }

    pub(crate) fn finish_video_export(
        &mut self,
        _path: &Path,
        result: Result<VideoExportReport, String>,
    ) -> Result<VideoExportReport, String> {
        match result {
            Ok(report) => {
                self.message(format!(
                    "Video exported: {} ({}, {} frames, {}×{})",
                    report.output_path.display(),
                    format_video_bytes(report.bytes),
                    report.frame_count,
                    report.width,
                    report.height
                ));
                for warning in &report.warnings {
                    self.message(format!("Video export note: {warning}"));
                }
                if !report.warnings.is_empty() {
                    self.show_panel(PanelId::Output);
                }
                self.last_video_report = Some(report.clone());
                Ok(report)
            }
            Err(e) => {
                let err = format!("Video export failed: {e}");
                self.message(err.clone());
                self.show_panel(PanelId::Output);
                Err(err)
            }
        }
    }

    /// Export video synchronously to `path` using `options` (used by `Publish` when `Video (MP4)`
    /// target is enabled and by automated tests).
    pub fn export_video_with_options_to_path(
        &mut self,
        path: &Path,
        options: &VideoOptions,
    ) -> Result<VideoExportReport, String> {
        self.finish_edits();
        let res = export_project_video(&self.project, path, options).map_err(|e| e.to_string());
        self.finish_video_export(path, res)
    }

    fn choose_export_video_path(&self, draft: &ExportVideoDraft) -> Option<PathBuf> {
        let base_name = match &draft.range_choice {
            VideoRangeChoice::Symbol(id) | VideoRangeChoice::Scene(id) => self
                .project
                .symbol(id)
                .map(|s| s.name.as_str())
                .unwrap_or(self.project.name.as_str()),
            _ => self.project.name.as_str(),
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
            "movie"
        } else {
            stem.trim_matches('-')
        };
        let ext = draft.options.format.extension();
        let file_name = if draft.options.format == VideoFormat::PngSequence {
            format!("{clean_stem}_0001.png")
        } else {
            format!("{clean_stem}.{ext}")
        };
        rfd::FileDialog::new()
            .set_title("Export video")
            .add_filter(draft.options.format.label(), &[ext])
            .set_file_name(&file_name)
            .save_file()
    }

    /// Render the `Dialog / Export video` modal (`DL-05`).
    pub(crate) fn export_video_modal(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.export_video.clone() else {
            return;
        };

        if let Some((path, res)) = draft.worker.lock().ok().and_then(|mut w| w.poll(ctx)) {
            let _ = self.finish_video_export(&path, res);
            self.export_video = None;
            return;
        }

        if let Some((pw, ph, rgba)) = draft.worker.lock().ok().and_then(|w| w.take_latest_frame()) {
            draft.preview_rgba = Some((pw, ph, rgba));
            draft.preview_texture = None;
        }

        let is_exporting = draft.is_exporting();
        let is_paused = draft.worker.lock().is_ok_and(|w| w.is_paused());
        let progress_counts = draft.worker.lock().ok().and_then(|w| w.progress_counts());

        let mut cancel = false;
        let mut do_export = false;
        let mut toggle_pause = false;
        let mut changed = false;

        let nf = neoflash_ui::palette(ctx);
        let modal = egui::Modal::new(egui::Id::new("export-video-modal"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                ui.set_width(840.0);
                cancel |= c::DialogHeader {
                    title: "Export video",
                }
                .show(ui);
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S5 as i8))
                    .show(ui, |ui| {
                        ui.set_width(840.0 - 2.0 * space::S5);
                        ui.horizontal_top(|ui| {
                            // ── Left column (300 pt): Preview + Progress + Pause caption ──
                            ui.vertical(|ui| {
                                ui.set_width(300.0);
                                let preview_resp =
                                    paint_video_preview_box(ui, ctx, &mut draft, 300.0, 188.0);
                                if is_exporting && preview_resp.clicked() {
                                    toggle_pause = true;
                                }
                                let total_frames = draft
                                    .estimate
                                    .as_ref()
                                    .map(|e| e.frame_count)
                                    .unwrap_or(1);
                                let (prog_label, prog_frac) =
                                    if let Some((done, total)) = progress_counts {
                                        (
                                            format!("Exporting frames {done} / {total}"),
                                            Some((done as f32 / total.max(1) as f32).clamp(0.0, 1.0)),
                                        )
                                    } else {
                                        (
                                            format!("Exporting frames 0 / {total_frames}"),
                                            Some(0.0),
                                        )
                                    };
                                c::progress(ui, &prog_label, prog_frac, 300.0);
                                let status_text = if is_exporting && is_paused {
                                    "Preview paused · ▶ to resume"
                                } else {
                                    "Preview while rendering · ⏸ to pause"
                                };
                                caption(ui, status_text);
                                if let Some(err) = &draft.error_message {
                                    ui.add(c::LogLine {
                                        severity: c::Severity::Error,
                                        time: "",
                                        text: err,
                                    });
                                }
                            });
                            ui.add_space(space::S5);
                            // ── Right column: Format, Range, Video, Content ──
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = space::S2;
                                ui.add_enabled_ui(!is_exporting, |ui| {
                                    let fmt_opts: [(VideoFormat, &str); 5] = [
                                        (VideoFormat::Mp4, "MP4"),
                                        (VideoFormat::WebM, "WebM"),
                                        (VideoFormat::Mov, "MOV"),
                                        (VideoFormat::Gif, "GIF"),
                                        (VideoFormat::PngSequence, "PNG seq."),
                                    ];
                                    let prev_fmt = draft.options.format;
                                    c::Segmented::show(
                                        ui,
                                        &mut draft.options.format,
                                        &fmt_opts,
                                        false,
                                    );
                                    if draft.options.format != prev_fmt {
                                        draft.options.codec = draft.options.format.default_codec();
                                        draft.preview_dirty = true;
                                        changed = true;
                                    }

                                    group(ui, "Range");
                                    ui.horizontal(|ui| {
                                        let cur_label =
                                            range_choice_label(&self.project, &draft.range_choice);
                                        let active_scene_id = self.symbol_id().to_owned();
                                        c::Select::new("export-video-range", &cur_label)
                                            .width(220.0)
                                            .show(ui, |ui| {
                                                let mut choices = vec![
                                                    (
                                                        VideoRangeChoice::WholeMovie,
                                                        "Whole movie (all scenes)".to_owned(),
                                                    ),
                                                    (
                                                        VideoRangeChoice::CurrentScene(
                                                            active_scene_id.clone(),
                                                        ),
                                                        "Current scene".to_owned(),
                                                    ),
                                                    (
                                                        VideoRangeChoice::CustomRange,
                                                        "Custom range".to_owned(),
                                                    ),
                                                ];
                                                let scenes = video_scene_ranges(&self.project);
                                                if scenes.len() > 1 {
                                                    for (sc_id, sc_name, _, _) in scenes {
                                                        choices.push((
                                                            VideoRangeChoice::Scene(sc_id),
                                                            format!("Scene: {sc_name}"),
                                                        ));
                                                    }
                                                }
                                                let scene_ids: std::collections::HashSet<&str> =
                                                    if self.project.scenes.is_empty() {
                                                        std::iter::once(self.project.root.as_str())
                                                            .collect()
                                                    } else {
                                                        self.project
                                                            .scenes
                                                            .iter()
                                                            .map(String::as_str)
                                                            .collect()
                                                    };
                                                for sym in &self.project.symbols {
                                                    if !scene_ids.contains(sym.id.as_str())
                                                        && matches!(
                                                            sym.kind,
                                                            SymbolKind::MovieClip
                                                                | SymbolKind::Graphic
                                                        )
                                                    {
                                                        choices.push((
                                                            VideoRangeChoice::Symbol(
                                                                sym.id.clone(),
                                                            ),
                                                            format!("Symbol: {}", sym.name),
                                                        ));
                                                    }
                                                }
                                                for (choice, label) in choices {
                                                    let is_sel = draft.range_choice == choice;
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(&label)
                                                                .checked(is_sel),
                                                        )
                                                        .clicked()
                                                    {
                                                        if !is_sel {
                                                            let (s1, e1) = range_bounds_1based(
                                                                &self.project,
                                                                &choice,
                                                            );
                                                            draft.range_choice = choice;
                                                            draft.from_frame = f64::from(s1);
                                                            draft.to_frame = f64::from(e1);
                                                            draft.preview_dirty = true;
                                                            changed = true;
                                                        }
                                                        ui.close();
                                                    }
                                                }
                                            });

                                        let r_from = ui.add(
                                            c::Field::new(&mut draft.from_frame, "From frame")
                                                .prefix("From")
                                                .decimals(0)
                                                .width(90.0),
                                        );
                                        let r_to = ui.add(
                                            c::Field::new(&mut draft.to_frame, "To frame")
                                                .prefix("To")
                                                .decimals(0)
                                                .width(90.0),
                                        );
                                        if r_from.changed() || r_to.changed() {
                                            if matches!(
                                                draft.range_choice,
                                                VideoRangeChoice::WholeMovie
                                            ) {
                                                draft.range_choice = VideoRangeChoice::CustomRange;
                                            }
                                            changed = true;
                                        }
                                    });

                                    group(ui, "Video");
                                    ui.horizontal(|ui| {
                                        let r_w = ui.add(
                                            c::Field::new(&mut draft.width, "Width")
                                                .prefix("W")
                                                .decimals(0)
                                                .width(110.0),
                                        );
                                        if r_w.changed() {
                                            draft.width = draft.width.clamp(2.0, 7680.0);
                                            if draft.options.constrain_proportions {
                                                draft.height = (draft.width / draft.aspect)
                                                    .round()
                                                    .clamp(2.0, 4320.0);
                                            }
                                            changed = true;
                                        }
                                        let r_h = ui.add(
                                            c::Field::new(&mut draft.height, "Height")
                                                .prefix("H")
                                                .decimals(0)
                                                .width(110.0),
                                        );
                                        if r_h.changed() {
                                            draft.height = draft.height.clamp(2.0, 4320.0);
                                            if draft.options.constrain_proportions {
                                                draft.width = (draft.height * draft.aspect)
                                                    .round()
                                                    .clamp(2.0, 7680.0);
                                            }
                                            changed = true;
                                        }
                                        if ui
                                            .add(
                                                c::IconButton::new(
                                                    i16::LINK,
                                                    "Constrain proportions",
                                                )
                                                .selected(draft.options.constrain_proportions),
                                            )
                                            .clicked()
                                        {
                                            draft.options.constrain_proportions =
                                                !draft.options.constrain_proportions;
                                            if draft.options.constrain_proportions
                                                && draft.height > 0.0
                                            {
                                                draft.aspect =
                                                    (draft.width / draft.height).max(0.01);
                                            }
                                            changed = true;
                                        }
                                        let scale_label = SCALE_PRESETS
                                            .get(draft.scale_idx)
                                            .map(|(_, l)| *l)
                                            .unwrap_or("1×");
                                        c::Select::new("export-video-scale", scale_label)
                                            .width(76.0)
                                            .show(ui, |ui| {
                                                for (idx, (mult, label)) in
                                                    SCALE_PRESETS.iter().copied().enumerate()
                                                {
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(label)
                                                                .checked(draft.scale_idx == idx),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.scale_idx = idx;
                                                        draft.width = (f64::from(self.project.width)
                                                            * f64::from(mult))
                                                        .round()
                                                        .clamp(2.0, 7680.0);
                                                        draft.height =
                                                            (f64::from(self.project.height)
                                                                * f64::from(mult))
                                                            .round()
                                                            .clamp(2.0, 4320.0);
                                                        changed = true;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                    });

                                    ui.horizontal(|ui| {
                                        let doc_fps_label =
                                            format!("{} fps (document)", self.project.fps);
                                        let cur_fps_label =
                                            match FPS_PRESETS.get(draft.fps_idx).copied().flatten()
                                            {
                                                None => doc_fps_label.clone(),
                                                Some(v) => format!("{v} fps"),
                                            };
                                        c::Select::new("export-video-fps", &cur_fps_label)
                                            .width(160.0)
                                            .show(ui, |ui| {
                                                for (idx, preset) in
                                                    FPS_PRESETS.iter().copied().enumerate()
                                                {
                                                    let label = match preset {
                                                        None => doc_fps_label.clone(),
                                                        Some(v) => format!("{v} fps"),
                                                    };
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(&label)
                                                                .checked(draft.fps_idx == idx),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.fps_idx = idx;
                                                        changed = true;
                                                        ui.close();
                                                    }
                                                }
                                            });

                                        let codec_label =
                                            format!("Codec: {}", draft.options.codec.label());
                                        let codecs = draft.options.format.codecs();
                                        c::Select::new("export-video-codec", &codec_label)
                                            .width(160.0)
                                            .show(ui, |ui| {
                                                for &codec in codecs {
                                                    let lbl = format!("Codec: {}", codec.label());
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(&lbl).checked(
                                                                draft.options.codec == codec,
                                                            ),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.options.codec = codec;
                                                        changed = true;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                    });

                                    ui.horizontal(|ui| {
                                        caption(ui, "Quality");
                                        let mut q = f64::from(draft.options.quality);
                                        let r_q = ui.add(
                                            c::Slider::new(&mut q, 1.0..=100.0, "Quality")
                                                .width(180.0),
                                        );
                                        if r_q.changed() {
                                            draft.options.quality =
                                                (q.round() as u8).clamp(1, 100);
                                            changed = true;
                                        }
                                        let mbps = draft
                                            .estimate
                                            .as_ref()
                                            .map(|e| e.bitrate_mbps)
                                            .unwrap_or(8.0);
                                        caption(
                                            ui,
                                            &format!(
                                                "{} · ~{:.0} Mbps",
                                                quality_tier_label(draft.options.quality),
                                                mbps.max(1.0)
                                            ),
                                        );
                                    });

                                    group(ui, "Content");
                                    if ui
                                        .add(c::Checkbox::new(
                                            &mut draft.options.runtime_capture,
                                            "Render nested movie clip animation and scripts (runtime capture)",
                                        ))
                                        .changed()
                                    {
                                        draft.preview_dirty = true;
                                        changed = true;
                                    }
                                    ui.horizontal(|ui| {
                                        let is_last = matches!(
                                            draft.options.stop_mode,
                                            VideoStopMode::LastFrame
                                        );
                                        if ui
                                            .add(c::Radio::new(is_last, "Stop at last frame"))
                                            .clicked()
                                            && !is_last
                                        {
                                            draft.options.stop_mode = VideoStopMode::LastFrame;
                                            changed = true;
                                        }
                                        if ui
                                            .add(c::Radio::new(!is_last, "Stop after"))
                                            .clicked()
                                            && is_last
                                        {
                                            draft.options.stop_mode = VideoStopMode::AfterSeconds(
                                                draft.stop_after_secs.max(0.1),
                                            );
                                            changed = true;
                                        }
                                        let r_secs = ui.add(
                                            c::Field::new(
                                                &mut draft.stop_after_secs,
                                                "Stop after seconds",
                                            )
                                            .unit("s")
                                            .decimals(0)
                                            .width(72.0),
                                        );
                                        if r_secs.changed() {
                                            draft.stop_after_secs =
                                                draft.stop_after_secs.clamp(0.1, 3600.0);
                                            draft.options.stop_mode = VideoStopMode::AfterSeconds(
                                                draft.stop_after_secs,
                                            );
                                            changed = true;
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        let audio_supported =
                                            draft.options.format.supports_audio();
                                        ui.add_enabled_ui(audio_supported, |ui| {
                                            if ui
                                                .add(c::Switch::new(
                                                    &mut draft.options.include_audio,
                                                    "Include audio",
                                                ))
                                                .changed()
                                            {
                                                changed = true;
                                            }
                                            ui.add_enabled_ui(
                                                draft.options.include_audio,
                                                |ui| {
                                                    c::Select::new(
                                                        "export-video-audio-bitrate",
                                                        draft.options.audio_bitrate.label(),
                                                    )
                                                    .width(150.0)
                                                    .show(ui, |ui| {
                                                        for br in VideoAudioBitrate::ALL {
                                                            if ui
                                                                .add(
                                                                    c::MenuItem::new(br.label())
                                                                        .checked(
                                                                            draft
                                                                                .options
                                                                                .audio_bitrate
                                                                                == br,
                                                                        ),
                                                                )
                                                                .clicked()
                                                            {
                                                                draft.options.audio_bitrate = br;
                                                                changed = true;
                                                                ui.close();
                                                            }
                                                        }
                                                    });
                                                },
                                            );
                                        });
                                        let alpha_supported =
                                            draft.options.format.supports_transparent_bg();
                                        ui.add_enabled_ui(alpha_supported, |ui| {
                                            if ui
                                                .add(c::Checkbox::new(
                                                    &mut draft.options.transparent_bg,
                                                    "Transparent bg (MOV/WebM)",
                                                ))
                                                .changed()
                                            {
                                                draft.preview_dirty = true;
                                                changed = true;
                                            }
                                        });
                                    });
                                });
                            });
                        });
                    });

                let footer_summary = if let Some(est) = &draft.estimate {
                    format!(
                        "~ {} · {} · {}×{}",
                        format_video_bytes(est.estimated_bytes),
                        format_mm_ss(est.duration_secs),
                        est.width,
                        est.height
                    )
                } else {
                    format!("{}×{}", self.project.width, self.project.height)
                };
                c::surface::dialog_footer(
                    ui,
                    |ui| {
                        caption(ui, &footer_summary);
                    },
                    |ui| {
                        if is_exporting {
                            let pause_lbl = if is_paused { "Resume" } else { "Pause" };
                            if ui.add(c::Button::ghost(pause_lbl)).clicked() {
                                toggle_pause = true;
                            }
                        }
                        if ui.add(c::Button::new("Cancel")).clicked() {
                            cancel = true;
                        }
                        let can_export = !is_exporting
                            && draft.error_message.is_none()
                            && (!draft.options.format.requires_ffmpeg()
                                || draft
                                    .estimate
                                    .as_ref()
                                    .is_none_or(|e| e.ffmpeg_available));
                        if ui
                            .add_enabled(can_export, c::Button::primary("Export"))
                            .clicked()
                        {
                            do_export = true;
                        }
                    },
                );
            });

        if toggle_pause && let Ok(w) = draft.worker.lock() {
            w.toggle_pause();
        }
        if cancel || modal.should_close() {
            draft.cancel_worker();
            self.export_video = None;
            return;
        }
        if changed {
            draft.sync_and_refresh(&self.project, self.frame);
        }
        if do_export {
            draft.sync_and_refresh(&self.project, self.frame);
            if let Some(path) = self.choose_export_video_path(&draft) {
                let immediate = draft.worker.lock().ok().and_then(|mut w| {
                    w.spawn_export(self.project.clone(), path, draft.options.clone())
                });
                if let Some((out_path, res)) = immediate {
                    let _ = self.finish_video_export(&out_path, res);
                    self.export_video = None;
                    return;
                }
            }
        }
        self.export_video = Some(draft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_export_video_and_export_gif_and_mp4_through_editor() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.open_export_video(None);
        let draft = editor.export_video.as_ref().expect("draft opened");
        assert_eq!(draft.options.format, VideoFormat::Mp4);
        assert!(draft.estimate.is_some());
        assert!(draft.preview_rgba.is_some());

        let dir =
            std::env::temp_dir().join(format!("neoflash-editor-video-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let gif_path = dir.join("editor.gif");
        let report = editor
            .export_video_with_options_to_path(
                &gif_path,
                &VideoOptions {
                    format: VideoFormat::Gif,
                    width: 120,
                    height: 80,
                    from_frame: 1,
                    to_frame: Some(2),
                    ..VideoOptions::default()
                },
            )
            .expect("editor gif export");
        assert_eq!(report.frame_count, 2);
        assert!(gif_path.is_file());
        assert_eq!(editor.last_video_report.as_ref(), Some(&report));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_video_modal_renders_and_updates_draft_in_both_themes() {
        for theme in [
            crate::theme::ThemeMode::Dark,
            crate::theme::ThemeMode::Light,
        ] {
            let ctx = egui::Context::default();
            let mut editor = Editor::from_project(&ctx, Project::demo());
            editor.set_theme(&ctx, theme);
            editor.open_export_video(None);
            assert!(editor.has_modal_dialog());
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1920.0, 1080.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        editor.export_video_modal(ctx);
                    },
                );
            }
            assert!(editor.export_video.is_some());
        }
    }
}
