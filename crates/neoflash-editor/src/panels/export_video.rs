//! Export video (`DL-05` / `W3.3`, Figma `BUz3pseopiuOg2Uiy3Fog7` › `v2 · Panels` › `Dialog / Export video`)
//! modal dialog.
//!
//! Opened by `File ▸ Export ▸ Export video…` ([`crate::commands::CommandId::FileExportVideo`])
//! and by `Library ▸ Export symbol as video…`.

use super::dialog_helpers::{
    DialogShell, fixed_dialog_size, footer_note, form_line, form_row, group_caption,
};
use super::planned::caption;
use crate::app::Editor;
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::{Id, Project, SymbolKind};
use neoflash_export::{
    VideoAudioBitrate, VideoEstimate, VideoExportReport, VideoFormat, VideoOptions, VideoStopMode,
    estimate_video, export_project_video, export_project_video_with_progress,
    render_project_frame_rgba_with_base_dir, video_scene_ranges,
};
use neoflash_ui::components as c;
use neoflash_ui::icons::i16;
use neoflash_ui::metrics;
use neoflash_ui::tokens::{TextStyle, radius};
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

struct EmptyOutputDirCleanup(Option<PathBuf>);

impl EmptyOutputDirCleanup {
    fn new(path: Option<PathBuf>) -> Self {
        Self(path)
    }

    fn keep(&mut self) {
        self.0 = None;
    }

    fn cleanup_now(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir(path);
        }
    }
}

impl Drop for EmptyOutputDirCleanup {
    fn drop(&mut self) {
        self.cleanup_now();
    }
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
        empty_output_dir_cleanup: Option<PathBuf>,
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
            let mut output_dir_cleanup = EmptyOutputDirCleanup::new(empty_output_dir_cleanup);
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
                output_dir_cleanup.cleanup_now();
                let _ = tx.send(None);
                return;
            }
            if res.is_ok() {
                output_dir_cleanup.keep();
            } else {
                output_dir_cleanup.cleanup_now();
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
    pub(crate) output_path: PathBuf,
    pub(crate) estimate: Option<VideoEstimate>,
    pub(crate) preview_rgba: Option<(u32, u32, Vec<u8>)>,
    pub(crate) preview_texture: Option<egui::TextureHandle>,
    pub(crate) preview_dirty: bool,
    pub(crate) error_message: Option<String>,
    pub(crate) estimate_blocked: bool,
    pub(crate) worker: Arc<Mutex<VideoAsyncWorker>>,
}

impl std::fmt::Debug for ExportVideoDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportVideoDraft")
            .field("range_choice", &self.range_choice)
            .field("from_frame", &self.from_frame)
            .field("to_frame", &self.to_frame)
            .field("options", &self.options)
            .field("output_path", &self.output_path)
            .finish()
    }
}

pub(crate) fn default_video_file_name(
    project: &Project,
    range_choice: &VideoRangeChoice,
    format: VideoFormat,
) -> String {
    let base_name = match range_choice {
        VideoRangeChoice::Symbol(id) | VideoRangeChoice::Scene(id) => project
            .symbol(id)
            .map(|s| s.name.as_str())
            .unwrap_or(project.name.as_str()),
        _ => project.name.as_str(),
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
        "movie".to_owned()
    } else {
        crate::output_paths::safe_output_file_stem(stem.trim_matches('-'), "movie")
    };
    let ext = format.extension();
    if format == VideoFormat::PngSequence {
        format!("{clean_stem}_0001.png")
    } else {
        format!("{clean_stem}.{ext}")
    }
}

pub(crate) fn sync_video_output_path_extension(path: &mut PathBuf, format: VideoFormat) {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);
    let raw_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("movie.mp4");
    let mut base = raw_name;
    for ext in [".mp4", ".m4v", ".webm", ".mov", ".gif", ".png"] {
        if raw_name.len() > ext.len()
            && raw_name[raw_name.len() - ext.len()..].eq_ignore_ascii_case(ext)
        {
            base = &raw_name[..raw_name.len() - ext.len()];
            break;
        }
    }
    let stem = base.strip_suffix("_0001").unwrap_or(base);
    let stem = if stem.trim().is_empty() {
        "movie".to_owned()
    } else {
        crate::output_paths::safe_output_file_stem(stem, "movie")
    };
    let next_name = if format == VideoFormat::PngSequence {
        format!("{stem}_0001.png")
    } else {
        format!("{stem}.{}", format.extension())
    };
    *path = match parent {
        Some(dir) => dir.join(next_name),
        None => PathBuf::from(next_name),
    };
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
        let output_path = PathBuf::from(default_video_file_name(
            project,
            &range_choice,
            options.format,
        ));

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
            output_path,
            estimate: None,
            preview_rgba: None,
            preview_texture: None,
            preview_dirty: true,
            error_message: None,
            estimate_blocked: false,
            worker: Arc::new(Mutex::new(VideoAsyncWorker::default())),
        };
        draft.sync_and_refresh(project, current_frame);
        draft
    }

    pub(crate) fn set_format(&mut self, format: VideoFormat) {
        if self.options.format != format {
            self.options.format = format;
            self.options.codec = format.default_codec();
            self.preview_dirty = true;
        }
        sync_video_output_path_extension(&mut self.output_path, format);
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
        let from_1 = if self.from_frame.is_finite() {
            (self
                .from_frame
                .round()
                .clamp(f64::from(min_1), f64::from(max_1)) as u32)
                .clamp(min_1, max_1)
        } else {
            min_1
        };
        let to_1 = if self.to_frame.is_finite() {
            (self
                .to_frame
                .round()
                .clamp(f64::from(from_1), f64::from(max_1)) as u32)
                .clamp(from_1, max_1)
        } else {
            max_1.max(from_1)
        };
        self.from_frame = f64::from(from_1);
        self.to_frame = f64::from(to_1);
        self.options.from_frame = from_1;
        self.options.to_frame = Some(to_1);

        let safe_w = if self.width.is_finite() {
            (self.width.round().clamp(2.0, 7680.0) as u32).clamp(2, 7680)
        } else {
            project.width.clamp(2, 7680)
        };
        let safe_h = if self.height.is_finite() {
            (self.height.round().clamp(2.0, 4320.0) as u32).clamp(2, 4320)
        } else {
            project.height.clamp(2, 4320)
        };
        self.width = f64::from(safe_w);
        self.height = f64::from(safe_h);
        if !self.aspect.is_finite() || self.aspect <= 0.0 {
            self.aspect = (self.width / self.height).max(0.01);
        }
        self.options.width = safe_w;
        self.options.height = safe_h;
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
        if !self.stop_after_secs.is_finite() {
            self.stop_after_secs = 10.0;
        } else {
            self.stop_after_secs = self.stop_after_secs.clamp(0.1, 3600.0);
        }
        if let VideoStopMode::AfterSeconds(_) = self.options.stop_mode {
            self.options.stop_mode = VideoStopMode::AfterSeconds(self.stop_after_secs);
        }
        sync_video_output_path_extension(&mut self.output_path, self.options.format);

        match estimate_video(project, &self.options) {
            Ok(est) => {
                self.estimate = Some(est);
                self.estimate_blocked = false;
                self.error_message = None;
            }
            Err(e) => {
                self.estimate_blocked = true;
                self.error_message = Some(e.to_string());
            }
        }

        if self.preview_dirty {
            self.preview_dirty = false;
            let (pw, ph) = preview_dimensions(project.width.max(1), project.height.max(1));
            let transparent =
                self.options.transparent_bg && self.options.format.supports_transparent_bg();
            if let Ok(rgba) = render_project_frame_rgba_with_base_dir(
                project,
                self.options.symbol.as_deref(),
                preview_frame,
                pw,
                ph,
                transparent,
                self.options.runtime_capture,
                self.options.base_dir.as_deref(),
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
    let total = if secs.is_finite() {
        secs.max(0.0).round() as u64
    } else {
        0
    };
    let mins = total / 60;
    let rem = total % 60;
    format!("{mins:02}:{rem:02}")
}

/// `Export video` has one size per density in every state (idle, exporting, paused, failed).
pub(crate) fn export_video_dialog_size(ctx: &egui::Context) -> egui::Vec2 {
    fixed_dialog_size(ctx, metrics::dialog_xl(), metrics::dialog_l_height())
}

/// Artwork column of `Export video`: 288 pt in a Normal `dialog_xl()`, scaled with the dialog.
fn export_video_aside_width(size: egui::Vec2) -> f32 {
    (288.0 * size.x / metrics::DIALOG_XL).round()
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
    let r = egui::CornerRadius::same(radius::lg() as u8);
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
    fn persist_video_export_options(&mut self, options: &VideoOptions) {
        if options.symbol.is_none() {
            let mut saved = options.clone();
            saved.base_dir = None;
            saved.symbol = None;
            saved.scene = None;
            self.workspace.export_video_options = Some(saved);
        }
    }

    /// Open the `Export video` modal dialog (`DL-05` / `W3.3`).
    pub fn open_export_video(&mut self, symbol_id: Option<&str>) {
        self.finish_edits();
        let mut draft = ExportVideoDraft::for_project(&self.project, self.frame, symbol_id);
        if symbol_id.is_none()
            && let Some(saved) = &self.workspace.export_video_options
        {
            draft.options.format = saved.format;
            draft.options.codec = saved.codec;
            draft.options.quality = saved.quality;
            draft.options.transparent_bg = saved.transparent_bg;
            draft.options.include_audio = saved.include_audio;
            draft.options.audio_bitrate = saved.audio_bitrate;
            draft.options.runtime_capture = saved.runtime_capture;
            draft.options.constrain_proportions = saved.constrain_proportions;
            draft.options.stop_mode = saved.stop_mode;
            if let VideoStopMode::AfterSeconds(secs) = saved.stop_mode {
                draft.stop_after_secs = secs;
            }
            if let Some(idx) = SCALE_PRESETS
                .iter()
                .position(|(s, _)| (*s - saved.scale).abs() < 0.01)
            {
                draft.scale_idx = idx;
            }
            if let Some(idx) = FPS_PRESETS.iter().position(|preset| match preset {
                None => saved.fps <= 0.0,
                Some(v) => (*v as f32 - saved.fps).abs() < 0.01,
            }) {
                draft.fps_idx = idx;
            }
            if !saved.constrain_proportions && saved.width >= 2 && saved.height >= 2 {
                draft.width = f64::from(saved.width);
                draft.height = f64::from(saved.height);
            }
        }
        draft.options.base_dir = self
            .tracked_disk_path()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        draft.preview_dirty = true;
        draft.sync_and_refresh(&self.project, self.frame);
        self.export_video = Some(draft);
    }

    pub(crate) fn finish_video_export(
        &mut self,
        _path: &Path,
        result: Result<VideoExportReport, String>,
    ) -> Result<VideoExportReport, String> {
        match result {
            Ok(report) => {
                let file_name = report
                    .output_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("video");
                self.toast(
                    c::ToastType::Success,
                    format!("Video exported — {file_name}"),
                );
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
        let mut resolved_options = options.clone();
        if resolved_options.base_dir.is_none() {
            resolved_options.base_dir = self
                .tracked_disk_path()
                .and_then(Path::parent)
                .map(Path::to_path_buf);
        }
        let res =
            export_project_video(&self.project, path, &resolved_options).map_err(|e| e.to_string());
        self.finish_video_export(path, res)
    }

    fn choose_export_video_path(
        &self,
        draft: &ExportVideoDraft,
    ) -> Result<Option<(PathBuf, Option<PathBuf>)>, String> {
        let ext = draft.options.format.extension();
        let file_name = draft
            .output_path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                default_video_file_name(&self.project, &draft.range_choice, draft.options.format)
            });
        let file_name = crate::output_paths::safe_output_file_name(&file_name, "movie", ext);
        if draft.options.format == VideoFormat::PngSequence {
            let mut parent_dialog =
                rfd::FileDialog::new().set_title("Choose PNG sequence parent folder");
            if let Some(parent) = draft
                .output_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                parent_dialog = parent_dialog.set_directory(parent);
            }
            let Some(parent) = parent_dialog.pick_folder() else {
                return Ok(None);
            };
            let (output_dir, created) =
                crate::output_paths::prepare_project_output_dir(&parent, &self.project)
                    .map_err(|err| format!("Could not prepare the PNG sequence folder: {err}"))?;
            let picked = rfd::FileDialog::new()
                .set_title("Export video")
                .add_filter(draft.options.format.label(), &[ext])
                .set_directory(&output_dir)
                .set_file_name(&file_name)
                .save_file();
            let Some(picked) = picked else {
                crate::output_paths::remove_created_output_dir_if_empty(&output_dir, created);
                return Ok(None);
            };
            let output_path = match crate::output_paths::project_sequence_output_path(
                &picked,
                &output_dir,
                "movie",
                ext,
            ) {
                Ok(path) => path,
                Err(err) => {
                    crate::output_paths::remove_created_output_dir_if_empty(&output_dir, created);
                    return Err(err);
                }
            };
            let cleanup = created.then(|| output_dir.clone());
            return Ok(Some((output_path, cleanup)));
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Export video")
            .add_filter(draft.options.format.label(), &[ext])
            .set_file_name(&file_name);
        if let Some(dir) = draft
            .output_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            dialog = dialog.set_directory(dir);
        }
        Ok(dialog.save_file().map(|path| (path, None)))
    }

    /// The form column of `Export video`: format switch, then the Range / Video / Content / Audio
    /// rows on the dialog form grid. Returns `true` when an option changed.
    fn export_video_form(&self, ui: &mut egui::Ui, draft: &mut ExportVideoDraft) -> bool {
        let mut changed = false;
        form_line(ui, |ui| {
            let fmt_opts: [(VideoFormat, &str); 5] = [
                (VideoFormat::Mp4, "MP4"),
                (VideoFormat::WebM, "WebM"),
                (VideoFormat::Mov, "MOV"),
                (VideoFormat::Gif, "GIF"),
                (VideoFormat::PngSequence, "PNG seq."),
            ];
            let prev_fmt = draft.options.format;
            let mut next_fmt = prev_fmt;
            c::Segmented::show(ui, &mut next_fmt, &fmt_opts, false);
            if next_fmt != prev_fmt {
                draft.set_format(next_fmt);
                changed = true;
            }
        });
        form_row(ui, "Range", |ui| {
            let cur_label = range_choice_label(&self.project, &draft.range_choice);
            let active_scene_id = self.symbol_id().to_owned();
            c::Select::new("export-video-range", &cur_label)
                .width(metrics::field_l())
                .show(ui, |ui| {
                    let mut choices = vec![
                        (
                            VideoRangeChoice::WholeMovie,
                            "Whole movie (all scenes)".to_owned(),
                        ),
                        (
                            VideoRangeChoice::CurrentScene(active_scene_id.clone()),
                            "Current scene".to_owned(),
                        ),
                        (VideoRangeChoice::CustomRange, "Custom range".to_owned()),
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
                            std::iter::once(self.project.root.as_str()).collect()
                        } else {
                            self.project.scenes.iter().map(String::as_str).collect()
                        };
                    for sym in &self.project.symbols {
                        if !scene_ids.contains(sym.id.as_str())
                            && matches!(sym.kind, SymbolKind::MovieClip | SymbolKind::Graphic)
                        {
                            choices.push((
                                VideoRangeChoice::Symbol(sym.id.clone()),
                                format!("Symbol: {}", sym.name),
                            ));
                        }
                    }
                    for (choice, label) in choices {
                        let is_sel = draft.range_choice == choice;
                        if ui.add(c::MenuItem::new(&label).checked(is_sel)).clicked() {
                            if !is_sel {
                                let (s1, e1) = range_bounds_1based(&self.project, &choice);
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
        });
        form_row(ui, "Frames", |ui| {
            let r_from = ui.add(
                c::Field::new(&mut draft.from_frame, "From frame")
                    .prefix("From")
                    .decimals(0)
                    .width(metrics::field_s()),
            );
            let r_to = ui.add(
                c::Field::new(&mut draft.to_frame, "To frame")
                    .prefix("To")
                    .decimals(0)
                    .width(metrics::field_s()),
            );
            if r_from.changed() || r_to.changed() {
                if matches!(draft.range_choice, VideoRangeChoice::WholeMovie) {
                    draft.range_choice = VideoRangeChoice::CustomRange;
                }
                changed = true;
            }
        });

        group_caption(ui, "Video");
        form_row(ui, "Size", |ui| {
            let r_w = ui.add(
                c::Field::new(&mut draft.width, "Width")
                    .prefix("W")
                    .unit("px")
                    .decimals(0)
                    .width(metrics::field_s()),
            );
            if r_w.changed() && draft.width.is_finite() {
                draft.width = draft.width.clamp(2.0, 7680.0);
                if draft.options.constrain_proportions {
                    draft.height = (draft.width / draft.aspect.max(0.01))
                        .round()
                        .clamp(2.0, 4320.0);
                }
                changed = true;
            }
            let r_h = ui.add(
                c::Field::new(&mut draft.height, "Height")
                    .prefix("H")
                    .unit("px")
                    .decimals(0)
                    .width(metrics::field_s()),
            );
            if r_h.changed() && draft.height.is_finite() {
                draft.height = draft.height.clamp(2.0, 4320.0);
                if draft.options.constrain_proportions {
                    draft.width = (draft.height * draft.aspect.max(0.01))
                        .round()
                        .clamp(2.0, 7680.0);
                }
                changed = true;
            }
            if ui
                .add(
                    c::IconButton::new(i16::LINK, "Constrain proportions")
                        .selected(draft.options.constrain_proportions),
                )
                .clicked()
            {
                draft.options.constrain_proportions = !draft.options.constrain_proportions;
                if draft.options.constrain_proportions && draft.height > 0.0 {
                    draft.aspect = (draft.width / draft.height).max(0.01);
                }
                changed = true;
            }
            let scale_label = SCALE_PRESETS
                .get(draft.scale_idx)
                .map(|(_, l)| *l)
                .unwrap_or("1×");
            c::Select::new("export-video-scale", scale_label)
                .width(metrics::field_num())
                .show(ui, |ui| {
                    for (idx, (mult, label)) in SCALE_PRESETS.iter().copied().enumerate() {
                        if ui
                            .add(c::MenuItem::new(label).checked(draft.scale_idx == idx))
                            .clicked()
                        {
                            draft.scale_idx = idx;
                            draft.width = (f64::from(self.project.width) * f64::from(mult))
                                .round()
                                .clamp(2.0, 7680.0);
                            draft.height = (f64::from(self.project.height) * f64::from(mult))
                                .round()
                                .clamp(2.0, 4320.0);
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
        form_row(ui, "Frame rate", |ui| {
            let doc_fps_label = format!("{} fps (document)", self.project.fps);
            let fps_label = |preset: Option<u32>| match preset {
                None => doc_fps_label.clone(),
                Some(v) => format!("{v} fps"),
            };
            let cur_fps_label = fps_label(FPS_PRESETS.get(draft.fps_idx).copied().flatten());
            c::Select::new("export-video-fps", &cur_fps_label)
                .width(metrics::field_m())
                .show(ui, |ui| {
                    for (idx, preset) in FPS_PRESETS.iter().copied().enumerate() {
                        if ui
                            .add(c::MenuItem::new(&fps_label(preset)).checked(draft.fps_idx == idx))
                            .clicked()
                        {
                            draft.fps_idx = idx;
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
        form_row(ui, "Codec", |ui| {
            let codecs = draft.options.format.codecs();
            c::Select::new("export-video-codec", draft.options.codec.label())
                .width(metrics::field_m())
                .show(ui, |ui| {
                    for &codec in codecs {
                        if ui
                            .add(
                                c::MenuItem::new(codec.label())
                                    .checked(draft.options.codec == codec),
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
        form_row(ui, "Quality", |ui| {
            let mut q = f64::from(draft.options.quality);
            let r_q =
                ui.add(c::Slider::new(&mut q, 1.0..=100.0, "Quality").width(metrics::field_m()));
            if r_q.changed() && q.is_finite() {
                draft.options.quality = q.round().clamp(1.0, 100.0) as u8;
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
        form_row(ui, "Background", |ui| {
            ui.add_enabled_ui(draft.options.format.supports_transparent_bg(), |ui| {
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

        group_caption(ui, "Content");
        form_line(ui, |ui| {
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
        });
        form_row(ui, "Duration", |ui| {
            let is_last = matches!(draft.options.stop_mode, VideoStopMode::LastFrame);
            if ui
                .add(c::Radio::new(is_last, "Stop at last frame"))
                .clicked()
                && !is_last
            {
                draft.options.stop_mode = VideoStopMode::LastFrame;
                changed = true;
            }
            if ui.add(c::Radio::new(!is_last, "Stop after")).clicked() && is_last {
                draft.options.stop_mode =
                    VideoStopMode::AfterSeconds(draft.stop_after_secs.max(0.1));
                changed = true;
            }
            let r_secs = ui.add(
                c::Field::new(&mut draft.stop_after_secs, "Stop after seconds")
                    .unit("s")
                    .decimals(0)
                    .width(metrics::field_num()),
            );
            if r_secs.changed() && draft.stop_after_secs.is_finite() {
                draft.stop_after_secs = draft.stop_after_secs.clamp(0.1, 3600.0);
                draft.options.stop_mode = VideoStopMode::AfterSeconds(draft.stop_after_secs);
                changed = true;
            }
        });

        group_caption(ui, "Audio");
        form_row(ui, "", |ui| {
            ui.add_enabled_ui(draft.options.format.supports_audio(), |ui| {
                if ui
                    .add(c::Checkbox::new(
                        &mut draft.options.include_audio,
                        "Include audio",
                    ))
                    .changed()
                {
                    changed = true;
                }
                ui.add_enabled_ui(draft.options.include_audio, |ui| {
                    c::Select::new(
                        "export-video-audio-bitrate",
                        draft.options.audio_bitrate.label(),
                    )
                    .width(metrics::field_m())
                    .show(ui, |ui| {
                        for br in VideoAudioBitrate::ALL {
                            if ui
                                .add(
                                    c::MenuItem::new(br.label())
                                        .checked(draft.options.audio_bitrate == br),
                                )
                                .clicked()
                            {
                                draft.options.audio_bitrate = br;
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                });
            });
        });
        changed
    }

    /// Render the `Dialog / Export video` modal (`DL-05`).
    pub(crate) fn export_video_modal(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.export_video.clone() else {
            return;
        };

        if let Some((path, res)) = draft.worker.lock().ok().and_then(|mut w| w.poll(ctx)) {
            match res {
                Ok(report) => {
                    self.persist_video_export_options(&draft.options);
                    let _ = self.finish_video_export(&path, Ok(report));
                    self.export_video = None;
                    return;
                }
                Err(err) => {
                    let msg = err.clone();
                    let _ = self.finish_video_export(&path, Err(err));
                    draft.error_message = Some(msg);
                }
            }
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
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);

        let nf = neoflash_ui::palette(ctx);
        let can_export_now = !is_exporting
            && !draft.estimate_blocked
            && (!draft.options.format.requires_ffmpeg()
                || draft.estimate.as_ref().is_none_or(|e| e.ffmpeg_available));
        // v3: one size in the idle, exporting, paused and failed states.
        let size = export_video_dialog_size(ctx);
        let aside_width = export_video_aside_width(size);
        let modal = egui::Modal::new(egui::Id::new("export-video-modal"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(nf.overlay)
            .show(ctx, |ui| {
                let shell = DialogShell::single(ui, "Export video", size);
                cancel |= shell.close;
                // Artwork column: preview, then the progress (exporting only) or the frame note.
                shell.aside(ui, aside_width, |ui| {
                    let preview_resp = paint_video_preview_box(
                        ui,
                        ctx,
                        &mut draft,
                        aside_width,
                        (aside_width * 0.625).round(),
                    );
                    if is_exporting && preview_resp.clicked() {
                        toggle_pause = true;
                    }
                    if is_exporting {
                        let total_frames =
                            draft.estimate.as_ref().map(|e| e.frame_count).unwrap_or(1);
                        let (done, total) = progress_counts.unwrap_or((0, total_frames));
                        c::progress(
                            ui,
                            &format!("Exporting frames {done} / {total}"),
                            Some((done as f32 / total.max(1) as f32).clamp(0.0, 1.0)),
                            aside_width,
                        );
                        caption(
                            ui,
                            if is_paused {
                                "Preview paused · ▶ to resume"
                            } else {
                                "Preview while rendering · ⏸ to pause"
                            },
                        );
                    } else {
                        caption(
                            ui,
                            &format!("Preview · frame {}", draft.options.from_frame.max(1)),
                        );
                        if let Some(err) = &draft.error_message {
                            ui.add(c::InlineMessage::error(err).width(aside_width));
                        }
                    }
                });
                shell.form(ui, "export-video-form", aside_width, |ui, _| {
                    ui.add_enabled_ui(!is_exporting, |ui| {
                        changed |= self.export_video_form(ui, &mut draft);
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
                // Footer: ghost Pause / Resume (exporting only), summary, Cancel, Export.
                shell.footer(
                    ui,
                    |ui| {
                        if is_exporting {
                            let (label, icon) = if is_paused {
                                ("Resume", i16::PLAY)
                            } else {
                                ("Pause", i16::PAUSE)
                            };
                            if ui.add(c::Button::ghost(label).icon(icon)).clicked() {
                                toggle_pause = true;
                            }
                        }
                        footer_note(ui, &footer_summary);
                    },
                    |ui| {
                        do_export |= ui
                            .add_enabled(
                                can_export_now,
                                c::Button::primary("Export").icon(i16::UPLOAD),
                            )
                            .clicked();
                        cancel |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });
        if !cancel && !do_export && !toggle_pause {
            match kb.resolve(ctx, &modal, can_export_now) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => cancel = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => do_export = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        if toggle_pause && let Ok(w) = draft.worker.lock() {
            w.toggle_pause();
        }
        if cancel {
            draft.cancel_worker();
            self.export_video = None;
            super::dialog_helpers::record_end_of_frame_focus(ctx);
            return;
        }
        if changed {
            draft.sync_and_refresh(&self.project, self.frame);
        }
        if do_export {
            draft.error_message = None;
            draft.sync_and_refresh(&self.project, self.frame);
            if !draft.estimate_blocked {
                match self.choose_export_video_path(&draft) {
                    Ok(Some((path, empty_output_dir_cleanup))) => {
                        self.persist_video_export_options(&draft.options);
                        let immediate = match draft.worker.lock() {
                            Ok(mut worker) => worker.spawn_export(
                                self.project.clone(),
                                path.clone(),
                                draft.options.clone(),
                                empty_output_dir_cleanup,
                            ),
                            Err(_) => {
                                if let Some(dir) = empty_output_dir_cleanup.as_ref() {
                                    let _ = std::fs::remove_dir(dir);
                                }
                                Some((path, Err("Video export worker is unavailable.".to_owned())))
                            }
                        };
                        if let Some((out_path, res)) = immediate {
                            match res {
                                Ok(report) => {
                                    let _ = self.finish_video_export(&out_path, Ok(report));
                                    self.export_video = None;
                                    super::dialog_helpers::record_end_of_frame_focus(ctx);
                                    return;
                                }
                                Err(err) => {
                                    let msg = err.clone();
                                    let _ = self.finish_video_export(&out_path, Err(err));
                                    draft.error_message = Some(msg);
                                }
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(err) => draft.error_message = Some(err),
                }
            }
        }
        self.export_video = Some(draft);
        super::dialog_helpers::record_end_of_frame_focus(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid_video_project() -> Project {
        let mut project = Project::blank();
        project.root = "missing-root".into();
        project
    }

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

    #[test]
    fn export_video_state_machine_and_footer_button_order() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.open_export_video(None);
        let run_modal = |editor: &mut Editor| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1920.0, 1080.0),
                    )),
                    ..Default::default()
                },
                |ctx| editor.export_video_modal(ctx),
            )
        };
        let label_rect = |out: &egui::FullOutput, label: &str| -> Option<egui::Rect> {
            out.platform_output
                .accesskit_update
                .as_ref()?
                .nodes
                .iter()
                .find_map(|(_, n)| {
                    (n.label().or(n.value()) == Some(label))
                        .then(|| n.bounds())
                        .flatten()
                        .map(|r| {
                            egui::Rect::from_min_max(
                                egui::pos2(r.x0 as f32, r.y0 as f32),
                                egui::pos2(r.x1 as f32, r.y1 as f32),
                            )
                        })
                })
        };

        // 1. Idle state: Preview caption, Cancel before Export, no Pause/Resume.
        let idle = run_modal(&mut editor);
        assert!(label_rect(&idle, "Preview · frame 1").is_some());
        let cancel_r = label_rect(&idle, "Cancel").expect("Idle Cancel");
        let export_r = label_rect(&idle, "Export").expect("Idle Export");
        assert!(cancel_r.left() < export_r.left());
        assert!(label_rect(&idle, "Pause").is_none());
        assert!(label_rect(&idle, "Resume").is_none());

        // 2. Exporting state: progress bar, pause caption, Pause · Cancel · Export order.
        let (tx, rx) = mpsc::channel();
        let worker = editor.export_video.as_ref().unwrap().worker.clone();
        {
            let mut w = worker.lock().unwrap();
            w.generation = 7;
            w.running = Some(RunningVideoExport {
                generation: 7,
                cancel: Arc::new(AtomicBool::new(false)),
                paused: Arc::new(AtomicBool::new(false)),
                frame_index: Arc::new(AtomicU32::new(2)),
                frame_count: Arc::new(AtomicU32::new(5)),
                latest_frame: Arc::new(Mutex::new(None)),
                rx,
            });
        }
        let exporting = run_modal(&mut editor);
        assert_eq!(worker.lock().unwrap().progress_counts(), Some((2, 5)));
        let has_progress_galley = exporting.shapes.iter().any(|s| {
            matches!(
                &s.shape,
                egui::Shape::Text(t) if t.galley.text().contains("Exporting frames 2 / 5")
            )
        });
        assert!(has_progress_galley);
        assert!(label_rect(&exporting, "Preview while rendering · ⏸ to pause").is_some());
        let pause_r = label_rect(&exporting, "Pause").expect("Exporting Pause");
        let cancel_r = label_rect(&exporting, "Cancel").expect("Exporting Cancel");
        let export_r = label_rect(&exporting, "Export").expect("Exporting Export");
        assert!(pause_r.left() < cancel_r.left() && cancel_r.left() < export_r.left());

        // 3. Paused state: Resume label and order Resume · Cancel · Export.
        worker.lock().unwrap().toggle_pause();
        let paused = run_modal(&mut editor);
        assert!(label_rect(&paused, "Preview paused · ▶ to resume").is_some());
        let resume_r = label_rect(&paused, "Resume").expect("Paused Resume");
        let cancel_r = label_rect(&paused, "Cancel").expect("Paused Cancel");
        let export_r = label_rect(&paused, "Export").expect("Paused Export");
        assert!(resume_r.left() < cancel_r.left() && cancel_r.left() < export_r.left());

        // 4. Failed state: error row in left column while modal stays open.
        tx.send(Some((
            PathBuf::from("/tmp/out.mp4"),
            Err("encoder failed".to_owned()),
        )))
        .unwrap();
        let failed = run_modal(&mut editor);
        assert!(editor.export_video.is_some());
        assert!(label_rect(&failed, "Preview · frame 1").is_some());
        assert!(failed.shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t) if t.galley.text().contains("encoder failed")
        )));
        assert!(!failed.shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t) if t.galley.text().contains("Exporting frames")
        )));

        // 5. Finished state: modal closes and records last_video_report.
        let (tx_done, rx_done) = mpsc::channel();
        {
            let mut w = worker.lock().unwrap();
            w.generation = 8;
            w.running = Some(RunningVideoExport {
                generation: 8,
                cancel: Arc::new(AtomicBool::new(false)),
                paused: Arc::new(AtomicBool::new(false)),
                frame_index: Arc::new(AtomicU32::new(5)),
                frame_count: Arc::new(AtomicU32::new(5)),
                latest_frame: Arc::new(Mutex::new(None)),
                rx: rx_done,
            });
        }
        let report = VideoExportReport {
            output_path: PathBuf::from("/tmp/out.gif"),
            additional_files: Vec::new(),
            bytes: 1024,
            frame_count: 5,
            width: 550,
            height: 400,
            audio_included: false,
            warnings: Vec::new(),
        };
        tx_done
            .send(Some((PathBuf::from("/tmp/out.gif"), Ok(report.clone()))))
            .unwrap();
        let _ = run_modal(&mut editor);
        assert!(editor.export_video.is_none());
        assert_eq!(editor.last_video_report.as_ref(), Some(&report));
    }

    /// v3 "nothing jumps": every dialog whose content changes while it is open keeps one outer
    /// rectangle in each of its states, in all three densities.
    #[test]
    fn e7b_dialogs_keep_one_outer_rect_in_every_state_and_density() {
        let _feedback = crate::beta_feedback_tests::BetaFlagGuard::set(Some(true));
        use crate::panels::{
            ai_dialogs, beta_feedback, export_audio, export_image, export_lottie, import_video,
        };
        use neoflash_ui::metrics::{self, Density};

        type State = (&'static str, fn(&mut Editor, &egui::Context));
        struct Case {
            name: &'static str,
            modal: &'static str,
            /// Expected size of the dialog content (inside the 1 pt frame border).
            size: fn(&egui::Context) -> egui::Vec2,
            open: fn(&mut Editor, &egui::Context),
            show: fn(&mut Editor, &egui::Context),
            states: &'static [State],
        }

        fn inject_export(editor: &mut Editor, paused: bool) {
            let worker = editor.export_video.as_ref().unwrap().worker.clone();
            let mut w = worker.lock().unwrap();
            w.generation = 7;
            // The receiver stays connected for the rest of the test.
            let (tx, rx) = mpsc::channel();
            std::mem::forget(tx);
            w.running = Some(RunningVideoExport {
                generation: 7,
                cancel: Arc::new(AtomicBool::new(false)),
                paused: Arc::new(AtomicBool::new(paused)),
                frame_index: Arc::new(AtomicU32::new(2)),
                frame_count: Arc::new(AtomicU32::new(5)),
                latest_frame: Arc::new(Mutex::new(None)),
                rx,
            });
        }
        fn audio_state(ctx: &egui::Context, has_sound: bool, format: usize) {
            export_audio::update_export_audio_draft(ctx, |d| {
                d.set_format(neoflash_export::AudioFormat::from_index(format));
                d.has_sound = has_sound;
                d.empty_reason = (!has_sound).then(|| "No sound in the selected range.".to_owned());
                d.cached_summary = "beep.wav".to_owned();
                d.cached_duration = 1.5;
                d.computed_key = Some(export_audio::AudioPreviewKey::from(&d.options));
            });
        }
        fn png_bytes() -> Vec<u8> {
            let mut out = std::io::Cursor::new(Vec::new());
            image::RgbaImage::new(2, 2)
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        }

        let cases = [
            Case {
                name: "Export video",
                modal: "export-video-modal",
                size: export_video_dialog_size,
                open: |e, _| e.open_export_video(None),
                show: |e, ctx| e.export_video_modal(ctx),
                states: &[
                    ("idle", |_, _| {}),
                    ("exporting", |e, _| inject_export(e, false)),
                    ("paused", |e, _| inject_export(e, true)),
                    ("failed", |e, _| {
                        let draft = e.export_video.as_mut().unwrap();
                        draft.worker.lock().unwrap().running = None;
                        draft.error_message =
                            Some("FFmpeg exited with status 1: encoder failed".to_owned());
                    }),
                    ("GIF", |e, _| {
                        e.export_video
                            .as_mut()
                            .unwrap()
                            .set_format(VideoFormat::Gif);
                    }),
                ],
            },
            Case {
                name: "Export audio",
                modal: "export-audio",
                size: |ctx| {
                    egui::vec2(
                        crate::panels::dialog_helpers::dialog_width(ctx, metrics::dialog_m()),
                        export_audio::export_audio_dialog_height(),
                    )
                },
                open: |e, ctx| e.open_export_audio(ctx, None),
                show: |e, ctx| e.export_audio_dialog(ctx),
                states: &[
                    ("no sounds", |_, ctx| audio_state(ctx, false, 1)),
                    ("with sounds, MP3", |_, ctx| audio_state(ctx, true, 1)),
                    ("with sounds, WAV", |_, ctx| audio_state(ctx, true, 0)),
                ],
            },
            Case {
                name: "Export image",
                modal: "export-image",
                size: export_image::export_image_dialog_size,
                open: |e, ctx| e.open_export_image(ctx),
                show: |e, ctx| e.export_image_dialog(ctx),
                states: &[
                    ("PNG", |_, ctx| {
                        export_image::select_export_image_format_slug(ctx, "png")
                    }),
                    ("JPEG", |_, ctx| {
                        export_image::select_export_image_format_slug(ctx, "jpg")
                    }),
                    ("SVG", |_, ctx| {
                        export_image::select_export_image_format_slug(ctx, "svg")
                    }),
                    ("PSD", |_, ctx| {
                        export_image::select_export_image_format_slug(ctx, "psd")
                    }),
                    ("AI", |_, ctx| {
                        export_image::select_export_image_format_slug(ctx, "ai")
                    }),
                    ("PDF", |_, ctx| {
                        export_image::select_export_image_format_slug(ctx, "pdf")
                    }),
                ],
            },
            Case {
                name: "Export Lottie",
                modal: "export-lottie-modal",
                size: export_lottie::export_lottie_dialog_size,
                open: |e, _| e.open_export_lottie(None),
                show: |e, ctx| e.export_lottie_modal(ctx),
                states: &[
                    ("idle", |_, _| {}),
                    ("blocking error", |e, _| {
                        let draft = e.export_lottie.as_mut().unwrap();
                        draft.blocking_error =
                            Some("The selected source has no frames to export.".to_owned());
                    }),
                ],
            },
            Case {
                name: "Import video",
                modal: "import-video",
                size: import_video::import_video_dialog_size,
                open: |e, ctx| e.open_import_video(ctx),
                show: |e, ctx| e.import_video_dialog(ctx),
                states: &[
                    ("no file", |_, _| {}),
                    ("file without preview", |_, ctx| {
                        import_video::update_import_video_draft(ctx, |d| {
                            d.file_path = "/nonexistent/clip.mp4".to_owned();
                            d.source_fps = 30;
                        });
                    }),
                    ("external video", |_, ctx| {
                        import_video::update_import_video_draft(ctx, |d| d.deployment = 1);
                    }),
                ],
            },
            Case {
                name: "AI connection",
                modal: "nf2-ai-connection-dialog",
                size: |ctx| {
                    egui::vec2(
                        crate::panels::dialog_helpers::dialog_width(ctx, metrics::dialog_m()),
                        ai_dialogs::ai_connection_dialog_height(),
                    )
                },
                open: |e, _| assert!(e.open_sample_ai_dialog("ai_connection")),
                show: |e, ctx| e.ai_dialogs(ctx),
                states: &[
                    ("cloud provider", |_, _| {}),
                    ("test passed", |e, _| {
                        e.ai_connection_modal.as_mut().unwrap().test_outcome =
                            Some(Ok("Connected · gpt-4o".to_owned()));
                    }),
                    ("local provider, failed test", |e, _| {
                        assert!(e.open_sample_ai_dialog("ai_connection_failed"));
                    }),
                ],
            },
            Case {
                name: "AI answer",
                modal: "nf2-ai-answer-dialog",
                size: ai_dialogs::ai_answer_dialog_size,
                open: |e, _| assert!(e.open_sample_ai_dialog("ai_answer_waiting")),
                show: |e, ctx| e.ai_dialogs(ctx),
                states: &[
                    ("waiting", |_, _| {}),
                    ("streaming", |e, _| {
                        assert!(e.open_sample_ai_dialog("ai_answer_streaming"));
                    }),
                    ("proposals", |e, _| {
                        assert!(e.open_sample_ai_dialog("ai_answer_proposals"));
                    }),
                    ("cancelled", |e, _| {
                        assert!(e.open_sample_ai_dialog("ai_answer_cancelled"));
                    }),
                    ("failed", |e, _| {
                        assert!(e.open_sample_ai_dialog("ai_answer_failed"));
                    }),
                ],
            },
            Case {
                name: "Beta feedback",
                modal: "beta-feedback",
                size: |ctx| {
                    egui::vec2(
                        crate::panels::dialog_helpers::dialog_width(ctx, metrics::dialog_m()),
                        beta_feedback::beta_feedback_dialog_height(),
                    )
                },
                open: |e, _| e.open_beta_feedback(),
                show: |e, ctx| e.beta_feedback_dialog(ctx),
                states: &[
                    ("text only", |_, _| {}),
                    ("two attachments", |e, _| {
                        let png = png_bytes();
                        assert!(e.beta_feedback.attach_bytes("first.png", &png));
                        assert!(e.beta_feedback.attach_bytes("second.png", &png));
                    }),
                    ("five attachments and an error", |e, _| {
                        let png = png_bytes();
                        for name in ["third.png", "fourth.png", "fifth.png"] {
                            assert!(e.beta_feedback.attach_bytes(name, &png));
                        }
                        e.beta_feedback.error = Some(
                            "Feedback could not be sent: the server did not answer. Try again later."
                                .to_owned(),
                        );
                    }),
                ],
            },
        ];

        for density in Density::ALL {
            for case in &cases {
                let ctx = egui::Context::default();
                let mut editor = Editor::from_project(&ctx, Project::demo());
                metrics::set_density(density);
                (case.open)(&mut editor, &ctx);
                let mut first: Option<(egui::Rect, &str)> = None;
                for (state, apply) in case.states {
                    apply(&mut editor, &ctx);
                    for _ in 0..3 {
                        metrics::set_density(density);
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(1920.0, 1080.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| (case.show)(&mut editor, ctx),
                        );
                    }
                    let what = format!("{} / {state} / {}", case.name, density.label());
                    let rect = ctx
                        .memory(|m| m.area_rect(egui::Id::new(case.modal)))
                        .unwrap_or_else(|| panic!("{what}: the dialog is not open"));
                    // The frame adds its 1 pt border on every side.
                    let expected = (case.size)(&ctx) + egui::Vec2::splat(2.0);
                    assert!(
                        (rect.size() - expected).abs().max_elem() < 0.5,
                        "{what}: size {:?}, expected {expected:?}",
                        rect.size()
                    );
                    match first {
                        None => first = Some((rect, state)),
                        Some((reference, reference_state)) => assert!(
                            (rect.min - reference.min).abs().max_elem() < 0.5
                                && (rect.max - reference.max).abs().max_elem() < 0.5,
                            "{what}: rect {rect:?} differs from {reference:?} ({reference_state})"
                        ),
                    }
                }
            }
        }
        metrics::set_density(Density::Comfortable);
    }

    #[test]
    fn png_sequence_worker_removes_only_new_empty_folder_after_failure_or_cancel() {
        let root = std::env::temp_dir().join(format!(
            "neoflash-video-empty-dir-cleanup-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let ctx = egui::Context::default();
        let options = VideoOptions {
            format: VideoFormat::PngSequence,
            width: 2,
            height: 2,
            ..VideoOptions::default()
        };

        let failed_dir = root.join("failed");
        std::fs::create_dir(&failed_dir).unwrap();
        let mut failed_worker = VideoAsyncWorker::default();
        failed_worker.spawn_export(
            invalid_video_project(),
            failed_dir.join("movie_0001.png"),
            options.clone(),
            Some(failed_dir.clone()),
        );
        let failure = (0..200).find_map(|_| {
            let outcome = failed_worker.poll(&ctx);
            if outcome.is_none() {
                std::thread::sleep(Duration::from_millis(10));
            }
            outcome
        });
        assert!(failure.is_some_and(|(_, result)| result.is_err()));
        assert!(!failed_dir.exists());

        let cancelled_dir = root.join("cancelled");
        std::fs::create_dir(&cancelled_dir).unwrap();
        let mut cancelled_worker = VideoAsyncWorker::default();
        cancelled_worker.spawn_export(
            invalid_video_project(),
            cancelled_dir.join("movie_0001.png"),
            options,
            Some(cancelled_dir.clone()),
        );
        cancelled_worker.cancel();
        for _ in 0..200 {
            if !cancelled_dir.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!cancelled_dir.exists());

        let nonempty_dir = root.join("nonempty");
        std::fs::create_dir(&nonempty_dir).unwrap();
        std::fs::write(nonempty_dir.join("keep.txt"), b"user content").unwrap();
        let mut nonempty_worker = VideoAsyncWorker::default();
        nonempty_worker.spawn_export(
            invalid_video_project(),
            nonempty_dir.join("movie_0001.png"),
            VideoOptions {
                format: VideoFormat::PngSequence,
                width: 2,
                height: 2,
                ..VideoOptions::default()
            },
            Some(nonempty_dir.clone()),
        );
        let nonempty_failure = (0..200).find_map(|_| {
            let outcome = nonempty_worker.poll(&ctx);
            if outcome.is_none() {
                std::thread::sleep(Duration::from_millis(10));
            }
            outcome
        });
        assert!(nonempty_failure.is_some_and(|(_, result)| result.is_err()));
        assert_eq!(
            std::fs::read(nonempty_dir.join("keep.txt")).unwrap(),
            b"user content"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
