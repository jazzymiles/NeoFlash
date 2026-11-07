//! Publish settings (DL-03 / R-032). The draft and estimate share one PublishPrefs snapshot.
use super::planned::{body_text, caption, check, select, slider, switch};
use super::planned_dialogs::group;
use crate::{
    app::Editor,
    publish_controller::EstimateState,
    workspace::{PublishPrefs, PublishTarget},
};
use eframe::egui::{self, Ui};
use neoflash_export::{
    AudioExportPolicy, FontExportPolicy, SizeEstimate, SizeLevel, VideoAudioBitrate, VideoCodec,
    VideoFormat, VideoOptions, VideoStopMode,
};
use neoflash_ui::{
    components as c,
    icons::{self, i16},
    tokens::{TextStyle, space},
    typography::rich,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct PublishSettingsDraft {
    pub(crate) settings: PublishPrefs,
    pub(crate) selected_target: PublishTarget,
    /// Keep the URL while trying another level. Only Shared serializes it.
    runtime_url: String,
    error: Option<String>,
}

impl PublishSettingsDraft {
    fn new(settings: PublishPrefs) -> Self {
        let runtime_url = match &settings.options.size {
            SizeLevel::Shared(url) => url.clone(),
            _ => String::new(),
        };
        let selected_target = settings.default_target;
        Self {
            settings,
            selected_target,
            runtime_url,
            error: None,
        }
    }

    fn custom(&self) -> bool {
        self.settings.options.fonts != FontExportPolicy::Embed
            || self.settings.options.audio != AudioExportPolicy::Original
    }

    fn select_level(&mut self, index: usize) {
        self.settings.options.size = SizeLevel::all(&self.runtime_url)[index].clone();
        self.settings.options.fonts = FontExportPolicy::Embed;
        self.settings.options.audio = AudioExportPolicy::Original;
        self.error = None;
    }

    fn url_error(&self) -> Option<String> {
        if self.selected_target == PublishTarget::VideoMp4 && !self.settings.html5_enabled {
            return None;
        }
        self.settings.options.validated().err()
    }

    fn effective_video_options(&self, project: &neoflash_core::Project) -> VideoOptions {
        let mut opts = self.settings.video_options.clone();
        opts.format = VideoFormat::Mp4;
        if !matches!(opts.codec, VideoCodec::H264 | VideoCodec::H265) {
            opts.codec = VideoCodec::H264;
        }
        if opts.width == 0 {
            opts.width = project.width.max(1);
        }
        if opts.height == 0 {
            opts.height = project.height.max(1);
        }
        if !opts.fps.is_finite() || opts.fps <= 0.0 {
            opts.fps = project.fps.max(1) as f32;
        }
        opts
    }

    fn ensure_video_options_initialized(&mut self, project: &neoflash_core::Project) {
        self.settings.video_options = self.effective_video_options(project);
    }
}

fn level_index(level: &SizeLevel) -> usize {
    match level {
        SizeLevel::Full => 0,
        SizeLevel::Balanced => 1,
        SizeLevel::Compact => 2,
        SizeLevel::Shared(_) => 3,
    }
}

const LEVELS: [(usize, &str); 4] = [
    (0, "Full"),
    (1, "Balanced"),
    (2, "Compact"),
    (3, "Shared runtime"),
];

fn level_label(level: &SizeLevel) -> &'static str {
    LEVELS[level_index(level)].1
}

fn bytes_label(bytes: usize) -> String {
    match bytes {
        0..=999 => format!("{bytes} B"),
        1_000..=999_999 => format!("{:.1} KB", bytes as f64 / 1_000.0),
        _ => format!("{:.2} MB", bytes as f64 / 1_000_000.0),
    }
}

fn footer_label(draft: &PublishSettingsDraft, state: &EstimateState) -> String {
    match state {
        EstimateState::Ready(size) => format!(
            "{} · {} · {} gzip · {} files",
            if draft.custom() {
                "Custom"
            } else {
                level_label(&size.level)
            },
            bytes_label(size.bytes),
            bytes_label(size.gzip_bytes),
            size.files
        ),
        EstimateState::Idle | EstimateState::Computing => "Estimating…".into(),
        EstimateState::Failed(_) => "Size unknown".into(),
    }
}

fn video_footer_label(project: &neoflash_core::Project, draft: &PublishSettingsDraft) -> String {
    let opts = draft.effective_video_options(project);
    match neoflash_export::estimate_video(project, &opts) {
        Ok(est) => {
            let mins = (est.duration_secs / 60.0).floor() as u32;
            let secs = (est.duration_secs % 60.0).round() as u32;
            format!(
                "MP4 ({}) · ~ {} · {mins:02}:{secs:02} · {}×{}",
                opts.codec.label(),
                bytes_label(est.estimated_bytes as usize),
                est.width,
                est.height
            )
        }
        Err(_) => format!(
            "MP4 ({}) · {}×{}",
            opts.codec.label(),
            opts.width,
            opts.height
        ),
    }
}

fn disabled(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.add_enabled_ui(false, content)
        .response
        .on_disabled_hover_text(super::menu::NOT_AVAILABLE);
}

impl Editor {
    pub(crate) fn open_publish_settings(&mut self) {
        self.finish_edits();
        self.publish_controller.cancel();
        self.publish_settings = Some(PublishSettingsDraft::new(self.workspace.publish.clone()));
    }

    pub(crate) fn publish_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.publish_settings.take() else {
            return;
        };
        let mut close = false;
        let mut save = false;
        let mut publish = false;
        let mut export_runtime = false;
        let p = neoflash_ui::palette(ctx);
        let mut estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
        let modal = egui::Modal::new(egui::Id::new("publish-settings"))
            .frame(c::Surface::Dialog.frame(ctx)).backdrop_color(p.overlay)
            .show(ctx, |ui| {
                // R-032 retains the 900pt dialog: 260pt targets, 24pt gap, 566pt settings.
                ui.set_width(898.0);
                let top = ui.cursor().min;
                close |= c::DialogHeader { title: "Publish settings" }.show(ui);
                ui.painter().hline(top.x..=top.x + 898.0,
                    top.y + neoflash_ui::metrics::dialog_header() - 0.5,
                    egui::Stroke::new(1.0_f32, p.border_subtle));
                egui::Frame::new().inner_margin(egui::Margin::same(space::S5 as i8)).show(ui, |ui| {
                    ui.set_width(850.0);
                    egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 240.0).max(200.0)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = space::S5;
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(egui::vec2(260.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(260.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.spacing_mut().item_spacing.x = space::S2;
                                disabled(ui, |ui| {
                                    select(ui, "Profile: Default", 260.0);
                                });
                                ui.horizontal(|ui| {
                                    ui.set_min_height(32.0);
                                    if ui
                                        .add(c::Checkbox::new(&mut draft.settings.html5_enabled, ""))
                                        .clicked()
                                    {
                                        draft.selected_target = PublishTarget::Html5;
                                    }
                                    let active = draft.selected_target == PublishTarget::Html5;
                                    icons::show(
                                        ui,
                                        i16::GLOBE,
                                        if active { p.accent_default } else { p.text_secondary },
                                    );
                                    let lbl = ui.add(
                                        egui::Label::new(
                                            rich(
                                                "HTML5 · Rust / WebAssembly",
                                                if active {
                                                    TextStyle::BodyStrong
                                                } else {
                                                    TextStyle::Body
                                                },
                                            )
                                            .color(p.text_primary),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if lbl.clicked() {
                                        draft.selected_target = PublishTarget::Html5;
                                    }
                                });
                                disabled(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.set_min_height(32.0);
                                        check(ui, false, "");
                                        icons::show(ui, i16::DESKTOP, p.text_secondary);
                                        body_text(ui, "Desktop app (macOS / Windows)");
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.set_min_height(32.0);
                                    if ui
                                        .add(c::Checkbox::new(&mut draft.settings.video_enabled, ""))
                                        .clicked()
                                    {
                                        draft.selected_target = PublishTarget::VideoMp4;
                                    }
                                    let active = draft.selected_target == PublishTarget::VideoMp4;
                                    icons::show(
                                        ui,
                                        i16::VIDEO,
                                        if active { p.accent_default } else { p.text_secondary },
                                    );
                                    let lbl = ui.add(
                                        egui::Label::new(
                                            rich(
                                                "Video (MP4)",
                                                if active {
                                                    TextStyle::BodyStrong
                                                } else {
                                                    TextStyle::Body
                                                },
                                            )
                                            .color(p.text_primary),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if lbl.clicked() {
                                        draft.selected_target = PublishTarget::VideoMp4;
                                    }
                                });
                                disabled(ui, |ui| {
                                    for (on, icon, label) in [
                                        (false, i16::GRID, "Image sequence / sprite sheet"),
                                        (false, i16::CODE, "SVG animation"),
                                    ] {
                                        ui.horizontal(|ui| {
                                            ui.set_min_height(32.0);
                                            check(ui, on, "");
                                            icons::show(ui, icon, p.text_secondary);
                                            body_text(ui, label);
                                        });
                                    }
                                });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(566.0);
                                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);
                                if draft.selected_target == PublishTarget::VideoMp4 {
                                    draw_video_target_settings(ui, &self.project, &mut draft);
                                } else {
                                    ui.label(rich("HTML5 · Rust / WebAssembly", TextStyle::Title).color(p.text_primary));
                                    group(ui, "Output");
                                    ui.horizontal(|ui| {
                                        // The exporter owns the index.html filename; Browse selects its folder.
                                        let mut file = draft.settings.output_folder.as_deref().map(|s| PathBuf::from(s).join("index.html").display().to_string()).unwrap_or_default();
                                        ui.add_enabled(false, c::TextField::new(&mut file, "File").prefix("File").width(380.0));
                                        if ui.add(c::Button::new("…")).clicked()
                                            && let Some(folder) = rfd::FileDialog::new().set_title("Choose HTML5 output folder").pick_folder()
                                        {
                                            draft.settings.output_folder = Some(folder.to_string_lossy().into_owned());
                                        }
                                    });
                                    disabled(ui, |ui| ui.horizontal(|ui| {
                                        check(ui, true, "Include HTML wrapper");
                                        select(ui, "Template: Responsive", 200.0);
                                    }).inner);
                                    group(ui, "Runtime");
                                    let mut selected = if draft.custom() { usize::MAX } else { level_index(&draft.settings.options.size) };
                                    if c::Segmented::show(ui, &mut selected, &LEVELS, false).is_some() {
                                        draft.select_level(selected);
                                    }
                                    if draft.custom() {
                                        ui.horizontal(|ui| {
                                            ui.add(c::Badge::new("Custom", c::BadgeType::Neutral));
                                            if ui.add(c::Button::ghost(&format!("Reset to {}", level_label(&draft.settings.options.size)))).clicked() {
                                                draft.select_level(level_index(&draft.settings.options.size));
                                            }
                                        });
                                    }
                                    if matches!(draft.settings.options.size, SizeLevel::Shared(_)) {
                                        if ui.add(c::TextField::new(&mut draft.runtime_url, "Runtime URL").prefix("Runtime URL").width(566.0)).changed() {
                                            draft.settings.options.size = SizeLevel::Shared(draft.runtime_url.clone());
                                            draft.error = None;
                                        }
                                        if let Some(error) = draft.url_error() {
                                            ui.add(c::InlineMessage::error(&error));
                                        }
                                        export_runtime |= ui.add(c::Button::ghost("Export runtime folder…")).clicked();
                                    }
                                    let tradeoff = match draft.settings.options.size {
                                        SizeLevel::Full => "Includes the full runtime. Opens directly from disk.",
                                        SizeLevel::Balanced => "Needs HTTP(S) hosting. Includes all runtime families.",
                                        SizeLevel::Compact => "Needs HTTP(S) hosting. Dynamically named classes may need Balanced.",
                                        SizeLevel::Shared(_) => "Needs HTTP(S) hosting and a runtime from the same build. Cross-origin hosting needs CORS.",
                                    };
                                    ui.add(c::InlineMessage::info(tradeoff));
                                    // Refresh after editing as well as before reading: never paint stale totals.
                                    estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
                                    if weight(ui, &estimate, &draft) {
                                        self.publish_controller.cancel();
                                        estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
                                    }
                                    disabled(ui, |ui| ui.horizontal(|ui| {
                                        check(ui, true, "Loop");
                                        check(ui, true, "Autoplay");
                                        check(ui, true, "Preloader");
                                        check(ui, false, "Source maps");
                                        select(ui, "Scale: Fit, keep ratio", 200.0);
                                    }).inner);
                                    group(ui, "Assets");
                                    ui.horizontal(|ui| {
                                        disabled(ui, |ui| {
                                            caption(ui, "JPEG quality");
                                            slider(ui, 80.0, "JPEG quality", 160.0);
                                            caption(ui, "80");
                                        });
                                        let audio_lbl =
                                            draft.settings.options.audio.publish_select_label();
                                        c::Select::new(
                                            ui.id().with("publish-settings-audio"),
                                            &audio_lbl,
                                        )
                                        .width(180.0)
                                        .show(ui, |ui| {
                                            for policy in AudioExportPolicy::ALL {
                                                let lbl = policy.publish_select_label();
                                                if ui
                                                    .add(
                                                        c::MenuItem::new(&lbl).checked(
                                                            draft.settings.options.audio == policy,
                                                        ),
                                                    )
                                                    .clicked()
                                                {
                                                    draft.settings.options.audio = policy;
                                                    draft.error = None;
                                                    ui.close();
                                                }
                                            }
                                        });
                                    });
                                    disabled(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            check(ui, true, "Sprite sheets for bitmaps");
                                            check(ui, draft.settings.options.fonts == FontExportPolicy::Subset, "Embed fonts (subset)");
                                        });
                                    });
                                    group(ui, "Scripts");
                                    disabled(ui, |ui| ui.horizontal(|ui| {
                                        switch(ui, true, "Strict mode"); switch(ui, false, "Include trace()"); switch(ui, true, "Minify");
                                    }).inner);
                                    if let Some(error) = &draft.error { ui.add(c::InlineMessage::error(error)); }
                                }
                            });
                        });
                    });
                    // Same NF2 footer composition, with action validity driven by the draft URL.
                    ui.add_space(space::S2);
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline(r.x_range(), r.top() - space::S1, egui::Stroke::new(1.0_f32, p.border_subtle));
                    ui.add_space(space::S2);
                    let footer_str = if draft.selected_target == PublishTarget::VideoMp4 {
                        video_footer_label(&self.project, &draft)
                    } else {
                        footer_label(&draft, &estimate)
                    };
                    ui.horizontal(|ui| {
                        caption(ui, &footer_str);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let valid = draft.url_error().is_none();
                            publish |= ui.add_enabled(valid, c::Button::primary("Publish").icon(i16::PUBLISH)).clicked();
                            save |= ui.add_enabled(valid, c::Button::new("OK")).clicked();
                            close |= ui.add(c::Button::new("Cancel")).clicked();
                        });
                    });
                });
            });
        close |= modal.should_close();
        if !close
            && draft.url_error().is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            save = true;
        }
        if !close && save {
            self.workspace.publish = draft.settings.clone();
            close = true;
        } else if !close && publish {
            let publish_video =
                draft.settings.video_enabled || draft.selected_target == PublishTarget::VideoMp4;
            let publish_html5 = draft.settings.html5_enabled
                && (draft.selected_target == PublishTarget::Html5 || draft.settings.html5_enabled);
            if publish_video && !draft.settings.html5_enabled {
                if self.publish_video_with_settings(None, &draft.settings) {
                    close = true;
                } else {
                    draft.error = Some(self.status.clone());
                }
            } else if let Some(output) = publish_output(&draft.settings) {
                let mut ok = true;
                if publish_html5 {
                    ok &= self.publish_html5_with_settings(
                        &output,
                        &Self::runtime_dir(),
                        &draft.settings,
                    );
                }
                if ok && publish_video {
                    ok &= self.publish_video_with_settings(Some(&output), &draft.settings);
                }
                if ok {
                    close = true;
                } else {
                    draft.error = Some(self.status.clone());
                }
            }
        } else if !close
            && export_runtime
            && let Some(parent) = rfd::FileDialog::new()
                .set_title("Choose a parent folder for the shared runtime")
                .pick_folder()
        {
            // A distinct child protects a project export selected as the parent.
            match neoflash_export::export_shared_runtime(
                &parent.join("neoflash-runtime"),
                &Self::runtime_dir(),
            ) {
                Ok(path) => self.message(format!("Shared runtime exported: {}", path.display())),
                Err(error) => draft.error = Some(error.to_string()),
            }
        }
        if close {
            self.publish_controller.cancel();
        } else {
            self.publish_settings = Some(draft);
        }
    }

    pub(crate) fn publish_video_with_settings(
        &mut self,
        fallback_folder: Option<&Path>,
        settings: &PublishPrefs,
    ) -> bool {
        let stem: String = self
            .project
            .name
            .chars()
            .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
            .collect();
        let stem = if stem.trim().is_empty() {
            "Untitled"
        } else {
            stem.trim()
        };
        let path = resolve_publish_video_path(settings, fallback_folder, stem).or_else(|| {
            rfd::FileDialog::new()
                .set_title("Publish Video (MP4)")
                .add_filter("MPEG-4 Video", &["mp4"])
                .set_file_name(format!("{stem}.mp4"))
                .save_file()
        });
        let Some(mut path) = path else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("mp4");
        }
        let mut opts = settings.video_options.clone();
        opts.format = VideoFormat::Mp4;
        if !matches!(opts.codec, VideoCodec::H264 | VideoCodec::H265) {
            opts.codec = VideoCodec::H264;
        }
        if opts.width == 0 {
            opts.width = self.project.width.max(1);
        }
        if opts.height == 0 {
            opts.height = self.project.height.max(1);
        }
        if !opts.fps.is_finite() || opts.fps <= 0.0 {
            opts.fps = self.project.fps.max(1) as f32;
        }
        self.export_video_with_options_to_path(&path, &opts).is_ok()
    }
}

fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if (trimmed == "~" || trimmed.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(trimmed.strip_prefix("~/").unwrap_or(""));
    }
    PathBuf::from(trimmed)
}

fn resolve_publish_video_path(
    settings: &PublishPrefs,
    fallback_folder: Option<&Path>,
    stem: &str,
) -> Option<PathBuf> {
    if let Some(raw) = settings
        .video_file
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(expand_tilde(raw));
    }
    if let Some(dir) = fallback_folder {
        return Some(dir.join(format!("{stem}.mp4")));
    }
    if let Some(folder) = settings
        .output_folder
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(expand_tilde(folder).join(format!("{stem}.mp4")));
    }
    None
}

fn draw_video_target_settings(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let mut opts = draft.effective_video_options(project);
    let mut changed = false;

    ui.label(rich("Video (MP4)", TextStyle::Title).color(p.text_primary));
    group(ui, "Output");
    ui.horizontal(|ui| {
        let default_hint = draft
            .settings
            .output_folder
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| PathBuf::from(s).join("movie.mp4").display().to_string())
            .unwrap_or_else(|| "movie.mp4".into());
        let mut video_file = draft.settings.video_file.clone().unwrap_or_default();
        if ui
            .add(
                c::TextField::new(&mut video_file, "Video file")
                    .prefix("File")
                    .hint(&default_hint)
                    .width(380.0),
            )
            .changed()
        {
            draft.settings.video_file = (!video_file.trim().is_empty()).then_some(video_file);
            draft.error = None;
        }
        if ui.add(c::Button::new("…")).clicked()
            && let Some(picked) = rfd::FileDialog::new()
                .set_title("Choose MP4 output file")
                .add_filter("MPEG-4 Video", &["mp4"])
                .set_file_name("movie.mp4")
                .save_file()
        {
            draft.settings.video_file = Some(picked.to_string_lossy().into_owned());
            draft.error = None;
        }
    });

    group(ui, "Video");
    ui.horizontal(|ui| {
        let mut w_val = f64::from(opts.width.max(1));
        if ui
            .add(
                c::Field::new(&mut w_val, "Width")
                    .prefix("W")
                    .unit("px")
                    .decimals(0)
                    .range(16.0..=7680.0)
                    .width(96.0),
            )
            .changed()
        {
            let nw = w_val.round().clamp(16.0, 7680.0) as u32;
            if opts.constrain_proportions && project.width > 0 {
                let ratio = f64::from(project.height.max(1)) / f64::from(project.width.max(1));
                opts.height = ((f64::from(nw) * ratio).round() as u32).clamp(16, 4320);
            }
            opts.width = nw;
            changed = true;
        }
        let mut h_val = f64::from(opts.height.max(1));
        if ui
            .add(
                c::Field::new(&mut h_val, "Height")
                    .prefix("H")
                    .unit("px")
                    .decimals(0)
                    .range(16.0..=4320.0)
                    .width(96.0),
            )
            .changed()
        {
            let nh = h_val.round().clamp(16.0, 4320.0) as u32;
            if opts.constrain_proportions && project.height > 0 {
                let ratio = f64::from(project.width.max(1)) / f64::from(project.height.max(1));
                opts.width = ((f64::from(nh) * ratio).round() as u32).clamp(16, 7680);
            }
            opts.height = nh;
            changed = true;
        }
        if ui
            .add(c::Checkbox::new(
                &mut opts.constrain_proportions,
                "Constrain",
            ))
            .changed()
        {
            changed = true;
        }
        let mut fps_val = f64::from(opts.fps);
        if ui
            .add(
                c::Field::new(&mut fps_val, "Frame rate")
                    .unit("fps")
                    .decimals(0)
                    .range(1.0..=120.0)
                    .stepper()
                    .width(94.0),
            )
            .changed()
        {
            opts.fps = (fps_val as f32).clamp(1.0, 120.0);
            changed = true;
        }
    });

    ui.horizontal(|ui| {
        let codec_lbl = format!("Codec: {}", opts.codec.label());
        c::Select::new(ui.id().with("publish-video-codec"), &codec_lbl)
            .width(150.0)
            .show(ui, |ui| {
                for codec in [VideoCodec::H264, VideoCodec::H265] {
                    let item_lbl = format!("Codec: {}", codec.label());
                    if ui
                        .add(c::MenuItem::new(&item_lbl).checked(opts.codec == codec))
                        .clicked()
                    {
                        opts.codec = codec;
                        changed = true;
                        ui.close();
                    }
                }
            });
        caption(ui, "Quality");
        let mut q_val = f64::from(opts.quality);
        if ui
            .add(
                c::Slider::new(&mut q_val, 1.0..=100.0, "Quality")
                    .step(1.0)
                    .width(150.0),
            )
            .changed()
        {
            opts.quality = q_val.round().clamp(1.0, 100.0) as u8;
            changed = true;
        }
        let mbps = neoflash_export::estimate_video(project, &opts)
            .map(|e| e.bitrate_mbps)
            .unwrap_or_else(|_| opts.target_bitrate_mbps(opts.width, opts.height, opts.fps));
        let tier = if opts.quality >= 75 {
            "High"
        } else if opts.quality >= 45 {
            "Medium"
        } else {
            "Low"
        };
        caption(ui, &format!("{tier} (~{:.0} Mbps)", mbps.max(1.0)));
    });

    group(ui, "Content");
    if ui
        .add(c::Checkbox::new(
            &mut opts.runtime_capture,
            "Render nested movie clip animation and scripts",
        ))
        .changed()
    {
        changed = true;
    }
    ui.horizontal(|ui| {
        let stop_last = matches!(opts.stop_mode, VideoStopMode::LastFrame);
        if ui
            .add(c::Radio::new(stop_last, "Stop at last frame"))
            .clicked()
            && !stop_last
        {
            opts.stop_mode = VideoStopMode::LastFrame;
            changed = true;
        }
        let stop_after = matches!(opts.stop_mode, VideoStopMode::AfterSeconds(_));
        let mut secs_val = match opts.stop_mode {
            VideoStopMode::AfterSeconds(s) => s,
            VideoStopMode::LastFrame => 10.0,
        };
        if ui.add(c::Radio::new(stop_after, "Stop after")).clicked() && !stop_after {
            opts.stop_mode = VideoStopMode::AfterSeconds(secs_val);
            changed = true;
        }
        ui.add_enabled_ui(stop_after, |ui| {
            if ui
                .add(
                    c::Field::new(&mut secs_val, "Stop after seconds")
                        .unit("s")
                        .decimals(0)
                        .range(1.0..=3600.0)
                        .width(86.0),
                )
                .changed()
            {
                opts.stop_mode = VideoStopMode::AfterSeconds(secs_val.clamp(0.1, 3600.0));
                changed = true;
            }
        });
    });

    group(ui, "Audio");
    ui.horizontal(|ui| {
        if ui
            .add(c::Checkbox::new(&mut opts.include_audio, "Include audio"))
            .changed()
        {
            changed = true;
        }
        ui.add_enabled_ui(opts.include_audio, |ui| {
            c::Select::new(
                ui.id().with("publish-video-audio-bitrate"),
                opts.audio_bitrate.label(),
            )
            .width(150.0)
            .show(ui, |ui| {
                for br in [
                    VideoAudioBitrate::Aac128,
                    VideoAudioBitrate::Aac192,
                    VideoAudioBitrate::Aac256,
                ] {
                    if ui
                        .add(c::MenuItem::new(br.label()).checked(opts.audio_bitrate == br))
                        .clicked()
                    {
                        opts.audio_bitrate = br;
                        changed = true;
                        ui.close();
                    }
                }
            });
        });
    });

    if changed {
        draft.settings.video_options = opts;
        draft.ensure_video_options_initialized(project);
        draft.error = None;
    }

    if let Some(error) = &draft.error {
        ui.add(c::InlineMessage::error(error));
    }
}

fn publish_output(settings: &PublishPrefs) -> Option<PathBuf> {
    if let Some(folder) = settings
        .output_folder
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if (folder == "~" || folder.starts_with("~/"))
            && let Some(home) = std::env::var_os("HOME")
        {
            return Some(PathBuf::from(home).join(folder.strip_prefix("~/").unwrap_or("")));
        }
        return Some(PathBuf::from(folder));
    }
    rfd::FileDialog::new()
        .set_title("Choose HTML5 output parent folder")
        .pick_folder()
        .map(|parent| parent.join("neoflash-export"))
}

/// Returns whether Retry was pressed. All byte data comes from the exporter's assembled package.
fn weight(ui: &mut Ui, state: &EstimateState, draft: &PublishSettingsDraft) -> bool {
    match state {
        EstimateState::Ready(estimate) => {
            if matches!(estimate.level, SizeLevel::Compact)
                && let Some(omissions) = estimate
                    .tradeoffs
                    .iter()
                    .find(|line| line.starts_with("Left out"))
            {
                ui.horizontal_wrapped(|ui| {
                    ui.add(c::Badge::new("Left out", c::BadgeType::Warning));
                    caption(ui, omissions);
                });
            }
            ready_weight(ui, estimate);
            false
        }
        EstimateState::Idle | EstimateState::Computing => {
            c::progress(
                ui,
                &format!("Estimating {}…", level_label(&draft.settings.options.size)),
                None,
                ui.available_width(),
            );
            c::weight_totals(
                ui,
                &[("On disk", "—"), ("Transfer (gzip)", "—"), ("Files", "—")],
            );
            false
        }
        EstimateState::Failed(error) => {
            // Invalid URLs already have their error directly under the field.
            let mut retry = false;
            if draft.url_error().is_none() {
                ui.add(c::InlineMessage::error(error));
                retry = ui.add(c::Button::ghost("Retry")).clicked();
            }
            c::weight_totals(
                ui,
                &[("On disk", "—"), ("Transfer (gzip)", "—"), ("Files", "—")],
            );
            retry
        }
    }
}

fn ready_weight(ui: &mut Ui, estimate: &SizeEstimate) {
    let data: Vec<_> = [
        (c::WeightPart::WebAssembly, "wasm"),
        (c::WeightPart::RuntimeScripts, "runtime_js"),
        (c::WeightPart::ProjectScripts, "scripts"),
        (c::WeightPart::Content, "content"),
        (c::WeightPart::Resources, "resources"),
        (c::WeightPart::Fonts, "fonts"),
        (c::WeightPart::Page, "page"),
    ]
    .into_iter()
    .map(|(part, key)| {
        let bytes = estimate
            .parts
            .iter()
            .filter(|p| p.kind == key)
            .map(|p| p.bytes)
            .sum::<usize>();
        (part, bytes, bytes_label(bytes))
    })
    .collect();
    let segments: Vec<_> = data
        .iter()
        .map(|(part, bytes, value)| c::WeightSegment {
            part: *part,
            bytes: *bytes as u64,
            value,
        })
        .collect();
    c::WeightBar::new(&segments).show(ui, ui.available_width());
    c::weight_legend(ui, &segments, ui.available_width());
    c::weight_totals(
        ui,
        &[
            ("On disk", &bytes_label(estimate.bytes)),
            ("Transfer (gzip)", &bytes_label(estimate.gzip_bytes)),
            ("Files", &estimate.files.to_string()),
        ],
    );
    if let (Some(bytes), Some(gzip)) = (
        estimate.shared_runtime_bytes,
        estimate.shared_runtime_gzip_bytes,
    ) {
        ui.add(c::InlineMessage::info(&format!(
            "Shared runtime: {} on disk · {} gzip, downloaded once and reused.",
            bytes_label(bytes),
            bytes_label(gzip)
        )));
    }
}

#[cfg(test)]
#[path = "publish_settings_tests.rs"]
mod tests;
