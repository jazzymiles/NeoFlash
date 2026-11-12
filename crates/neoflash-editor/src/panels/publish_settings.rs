//! Publish settings (DL-03 / R-032). The draft and estimate share one PublishPrefs snapshot.
use super::dialog_helpers::group;
use super::planned::caption;
use crate::{
    app::Editor,
    publish_controller::EstimateState,
    workspace::{PublishPrefs, PublishProfilePreset, PublishTarget},
};
use eframe::egui::{self, Ui};
use neoflash_export::{
    AtlasFormat, AtlasImageFormat, AudioExportPolicy, BannerOptions, FontExportPolicy,
    HtmlScaleMode, HtmlStageAlign, HtmlTemplate, ImageExportFormat, SequencePublishPolicy,
    SizeEstimate, SizeLevel, VideoAudioBitrate, VideoCodec, VideoFormat, VideoOptions,
    VideoStopMode,
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
        if self.selected_target != PublishTarget::Html5
            && self.selected_target != PublishTarget::DesktopApp
            && !self.settings.html5_enabled
            && !self.settings.desktop_app_enabled
        {
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
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
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
                                c::Select::new(
                                    ui.id().with("publish-settings-profile"),
                                    draft.settings.profile.select_label(),
                                )
                                .width(260.0)
                                .show(ui, |ui| {
                                    for preset in PublishProfilePreset::ALL {
                                        if ui
                                            .add(
                                                c::MenuItem::new(preset.select_label())
                                                    .checked(draft.settings.profile == preset),
                                            )
                                            .clicked()
                                        {
                                            draft.settings.apply_profile(preset);
                                            draft.error = None;
                                            ui.close();
                                        }
                                    }
                                });
                                for (target, icon, label) in [
                                    (
                                        PublishTarget::Html5,
                                        i16::GLOBE,
                                        "HTML5 · Rust / WebAssembly",
                                    ),
                                    (
                                        PublishTarget::DesktopApp,
                                        i16::DESKTOP,
                                        "Desktop app (macOS / Windows)",
                                    ),
                                    (PublishTarget::VideoMp4, i16::VIDEO, "Video (MP4)"),
                                    (
                                        PublishTarget::ImageSequence,
                                        i16::GRID,
                                        "Image sequence / sprite sheet",
                                    ),
                                    (PublishTarget::SvgAnimation, i16::CODE, "SVG animation"),
                                ] {
                                    ui.horizontal(|ui| {
                                        ui.set_min_height(32.0);
                                        let enabled_ref = match target {
                                            PublishTarget::Html5 => &mut draft.settings.html5_enabled,
                                            PublishTarget::DesktopApp => {
                                                &mut draft.settings.desktop_app_enabled
                                            }
                                            PublishTarget::VideoMp4 => {
                                                &mut draft.settings.video_enabled
                                            }
                                            PublishTarget::ImageSequence => {
                                                &mut draft.settings.image_sequence_enabled
                                            }
                                            PublishTarget::SvgAnimation => {
                                                &mut draft.settings.svg_enabled
                                            }
                                        };
                                        if ui.add(c::Checkbox::new(enabled_ref, "")).clicked() {
                                            draft.selected_target = target;
                                            draft.settings.default_target = target;
                                        }
                                        let active = draft.selected_target == target;
                                        icons::show(
                                            ui,
                                            icon,
                                            if active { p.accent_default } else { p.text_secondary },
                                        );
                                        let lbl = ui.add(
                                            egui::Label::new(
                                                rich(
                                                    label,
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
                                            draft.selected_target = target;
                                        }
                                    });
                                }
                            });
                            ui.vertical(|ui| {
                                ui.set_width(566.0);
                                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S3);
                                match draft.selected_target {
                                    PublishTarget::VideoMp4 => {
                                        draw_video_target_settings(ui, &self.project, &mut draft);
                                    }
                                    PublishTarget::DesktopApp => {
                                        draw_desktop_app_target_settings(ui, &self.project, &mut draft);
                                    }
                                    PublishTarget::ImageSequence => {
                                        draw_image_sequence_target_settings(ui, &self.project, &mut draft);
                                    }
                                    PublishTarget::SvgAnimation => {
                                        draw_svg_target_settings(ui, &self.project, &mut draft);
                                    }
                                    PublishTarget::Html5 => {
                                        ui.label(rich("HTML5 · Rust / WebAssembly", TextStyle::Title).color(p.text_primary));
                                        group(ui, "Output");
                                        ui.horizontal(|ui| {
                                            // The exporter owns the index.html filename; Browse selects its folder.
                                            let entry_name = if draft.settings.html_wrapper {
                                                "index.html"
                                            } else {
                                                "bundle.js"
                                            };
                                            let mut file = draft
                                                .settings
                                                .output_folder
                                                .as_deref()
                                                .map(|s| PathBuf::from(s).join(entry_name).display().to_string())
                                                .unwrap_or_default();
                                            ui.add_enabled(false, c::TextField::new(&mut file, "File").prefix("File").width(380.0));
                                            if ui.add(c::Button::new("…")).clicked()
                                                && let Some(folder) = rfd::FileDialog::new().set_title("Choose HTML5 output folder").pick_folder()
                                            {
                                                draft.settings.output_folder = Some(folder.to_string_lossy().into_owned());
                                            }
                                        });
                                        ui.horizontal(|ui| {
                                            ui.add(c::Checkbox::new(
                                                &mut draft.settings.html_wrapper,
                                                "Include HTML wrapper",
                                            ));
                                            c::Select::new(
                                                ui.id().with("publish-settings-template"),
                                                draft.settings.template.publish_select_label(),
                                            )
                                            .width(200.0)
                                            .show(ui, |ui| {
                                                for tmpl in HtmlTemplate::ALL {
                                                    let lbl = tmpl.publish_select_label();
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(lbl)
                                                                .checked(draft.settings.template == tmpl),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.settings.template = tmpl;
                                                        draft.error = None;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                        });
                                        group(ui, "Runtime");
                                        if draft.settings.profile == PublishProfilePreset::WebBannerNative {
                                            ui.horizontal(|ui| {
                                                let mut max_kb = f64::from(draft.settings.banner_max_gzip_kb.clamp(10, 5000));
                                                if ui
                                                    .add(
                                                        c::Field::new(&mut max_kb, "Max gzip")
                                                            .prefix("Max gzip")
                                                            .unit("KB")
                                                            .decimals(0)
                                                            .range(10.0..=5000.0)
                                                            .width(160.0),
                                                    )
                                                    .changed()
                                                    && max_kb.is_finite()
                                                {
                                                    draft.settings.banner_max_gzip_kb =
                                                        max_kb.round().clamp(10.0, 5000.0) as u32;
                                                    draft.error = None;
                                                }
                                                caption(ui, "Single-file HTML5 ad banner (no WASM)");
                                            });
                                            let banner_opts = BannerOptions {
                                                max_gzip_bytes: Some(draft.settings.banner_max_gzip_kb as usize * 1024),
                                                max_raw_bytes: Some(draft.settings.banner_max_gzip_kb as usize * 4096),
                                                loop_timeline: draft.settings.loop_timeline,
                                                autoplay: draft.settings.autoplay,
                                                jpeg_quality: draft.settings.jpeg_quality,
                                                ..BannerOptions::default()
                                            };
                                            ui.add(c::InlineMessage::info(
                                                "Compiles eligible vector/bitmap artwork into one self-contained index.html with inline SVG, timeline transport, and window.clickTag.",
                                            ));
                                            match neoflash_export::estimate_native_banner(&self.project, &banner_opts) {
                                                Ok(est) => {
                                                    c::weight_totals(
                                                        ui,
                                                        &[
                                                            ("On disk", &bytes_label(est.raw_bytes)),
                                                            ("Transfer (gzip)", &bytes_label(est.gzip_bytes)),
                                                            ("Budget", &format!("{} KB gzip", draft.settings.banner_max_gzip_kb)),
                                                        ],
                                                    );
                                                    for diag in &est.diagnostics {
                                                        ui.add(c::InlineMessage::error(&format!(
                                                            "[{}] {}: {}",
                                                            diag.code, diag.location, diag.message
                                                        )));
                                                    }
                                                }
                                                Err(err) => {
                                                    ui.add(c::InlineMessage::error(&err.to_string()));
                                                }
                                            }
                                        } else {
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
                                                SizeLevel::Balanced => "Includes all runtime families. Opens from disk or HTTP(S).",
                                                SizeLevel::Compact => "Strips unused runtime families. Opens from disk or HTTP(S); dynamically named classes may need Balanced.",
                                                SizeLevel::Shared(_) => "Needs HTTP(S) hosting and a runtime from the same build. Cross-origin hosting needs CORS.",
                                            };
                                            ui.add(c::InlineMessage::info(tradeoff));
                                            // Refresh after editing as well as before reading: never paint stale totals.
                                            estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
                                            if weight(ui, &estimate, &draft) {
                                                self.publish_controller.cancel();
                                                estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
                                            }
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add(c::Checkbox::new(&mut draft.settings.loop_timeline, "Loop"));
                                            ui.add(c::Checkbox::new(&mut draft.settings.autoplay, "Autoplay"));
                                            ui.add(c::Checkbox::new(&mut draft.settings.preloader, "Preloader"));
                                            ui.add(c::Checkbox::new(&mut draft.settings.allow_fullscreen, "Fullscreen"));
                                            ui.add(c::Checkbox::new(&mut draft.settings.source_maps, "Source maps"));
                                            c::Select::new(
                                                ui.id().with("publish-settings-scale"),
                                                draft.settings.scale_mode.publish_select_label(),
                                            )
                                            .width(196.0)
                                            .show(ui, |ui| {
                                                for mode in HtmlScaleMode::ALL {
                                                    let lbl = mode.publish_select_label();
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(lbl)
                                                                .checked(draft.settings.scale_mode == mode),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.settings.scale_mode = mode;
                                                        draft.error = None;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                            c::Select::new(
                                                ui.id().with("publish-settings-stage-align"),
                                                draft.settings.stage_align.publish_select_label(),
                                            )
                                            .width(176.0)
                                            .show(ui, |ui| {
                                                for align in HtmlStageAlign::ALL {
                                                    let lbl = align.publish_select_label();
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(lbl)
                                                                .checked(draft.settings.stage_align == align),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.settings.stage_align = align;
                                                        draft.error = None;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                        });
                                        group(ui, "Assets");
                                        ui.horizontal(|ui| {
                                            caption(ui, "JPEG quality");
                                            let mut q = f64::from(draft.settings.jpeg_quality.clamp(1, 100));
                                            if ui
                                                .add(
                                                    c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality")
                                                        .width(160.0),
                                                )
                                                .changed()
                                            {
                                                draft.settings.jpeg_quality =
                                                    q.round().clamp(1.0, 100.0) as u8;
                                                draft.error = None;
                                            }
                                            caption(ui, &draft.settings.jpeg_quality.to_string());
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
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add(c::Checkbox::new(
                                                &mut draft.settings.sprite_sheets,
                                                "Sprite sheets for bitmaps",
                                            ));
                                            let mut subset_fonts =
                                                draft.settings.options.fonts == FontExportPolicy::Subset;
                                            if ui
                                                .add(c::Checkbox::new(
                                                    &mut subset_fonts,
                                                    "Embed fonts (subset)",
                                                ))
                                                .changed()
                                            {
                                                draft.settings.options.fonts = if subset_fonts {
                                                    FontExportPolicy::Subset
                                                } else {
                                                    FontExportPolicy::Embed
                                                };
                                                draft.error = None;
                                            }
                                            let seq_policy_lbl = format!(
                                                "Sequences: {}",
                                                draft.settings.sequence_policy.label()
                                            );
                                            c::Select::new(
                                                ui.id().with("publish-settings-sequence-policy"),
                                                &seq_policy_lbl,
                                            )
                                            .width(220.0)
                                            .show(ui, |ui| {
                                                for policy in SequencePublishPolicy::ALL {
                                                    let item_lbl =
                                                        format!("Sequences: {}", policy.label());
                                                    if ui
                                                        .add(
                                                            c::MenuItem::new(&item_lbl)
                                                                .checked(draft.settings.sequence_policy == policy),
                                                        )
                                                        .clicked()
                                                    {
                                                        draft.settings.sequence_policy = policy;
                                                        draft.error = None;
                                                        ui.close();
                                                    }
                                                }
                                            });
                                        });
                                        group(ui, "Scripts");
                                        ui.horizontal(|ui| {
                                            ui.add(c::Switch::new(
                                                &mut draft.settings.strict,
                                                "Strict mode",
                                            ));
                                            ui.add(c::Switch::new(
                                                &mut draft.settings.include_trace,
                                                "Include trace()",
                                            ));
                                            ui.add(c::Switch::new(
                                                &mut draft.settings.minify,
                                                "Minify",
                                            ));
                                        });
                                        if let Some(error) = &draft.error { ui.add(c::InlineMessage::error(error)); }
                                    }
                                }
                            });
                        });
                    });
                    // Same NF2 footer composition, with action validity driven by the draft URL.
                    ui.add_space(space::S2);
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline(r.x_range(), r.top() - space::S1, egui::Stroke::new(1.0_f32, p.border_subtle));
                    ui.add_space(space::S2);
                    let footer_str = match draft.selected_target {
                        PublishTarget::VideoMp4 => video_footer_label(&self.project, &draft),
                        PublishTarget::DesktopApp => {
                            format!("macOS .app + Windows Launcher · {}", level_label(&draft.settings.options.size))
                        }
                        PublishTarget::ImageSequence => {
                            if draft.settings.image_sequence_sprite_sheet {
                                format!(
                                    "Sprite sheet ({} + {}) · {}×{}",
                                    draft.settings.atlas_image_format.label(),
                                    draft.settings.atlas_format.label(),
                                    self.project.width,
                                    self.project.height
                                )
                            } else {
                                format!(
                                    "{} sequence · {}×{}",
                                    draft.settings.image_sequence_format.label(),
                                    self.project.width,
                                    self.project.height
                                )
                            }
                        }
                        PublishTarget::SvgAnimation => {
                            format!("SVG 1.1 · {}×{}", self.project.width, self.project.height)
                        }
                        PublishTarget::Html5 => {
                            if draft.settings.profile == PublishProfilePreset::WebBannerNative {
                                let banner_opts = BannerOptions {
                                    max_gzip_bytes: Some(draft.settings.banner_max_gzip_kb as usize * 1024),
                                    max_raw_bytes: Some(draft.settings.banner_max_gzip_kb as usize * 4096),
                                    loop_timeline: draft.settings.loop_timeline,
                                    autoplay: draft.settings.autoplay,
                                    jpeg_quality: draft.settings.jpeg_quality,
                                    ..BannerOptions::default()
                                };
                                match neoflash_export::estimate_native_banner(&self.project, &banner_opts) {
                                    Ok(est) => format!(
                                        "Web Banner · {} · {} gzip · 1 file",
                                        bytes_label(est.raw_bytes),
                                        bytes_label(est.gzip_bytes)
                                    ),
                                    Err(_) => "Web Banner · Size unknown".into(),
                                }
                            } else {
                                footer_label(&draft, &estimate)
                            }
                        }
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
        if !close && !save && !publish && !export_runtime {
            match kb.resolve(ctx, &modal, draft.url_error().is_none()) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => save = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        if !close && save {
            draft.settings.sanitize();
            self.workspace.publish = draft.settings.clone();
            close = true;
        } else if !close && publish {
            draft.settings.sanitize();
            if !draft.settings.has_any_enabled_target() {
                draft.settings.enable_target(draft.selected_target);
                draft.settings.default_target = draft.selected_target;
            }
            let publish_video = draft.settings.video_enabled;
            let publish_html5 = draft.settings.html5_enabled;
            let publish_desktop = draft.settings.desktop_app_enabled;
            let publish_images = draft.settings.image_sequence_enabled;
            let publish_svg = draft.settings.svg_enabled;
            let unconfigured =
                |opt: &Option<String>| opt.as_deref().is_none_or(|s| s.trim().is_empty());
            let needs_shared_output = publish_html5
                || (publish_desktop && unconfigured(&draft.settings.desktop_app_folder))
                || (publish_images && unconfigured(&draft.settings.image_sequence_folder))
                || (publish_svg && unconfigured(&draft.settings.svg_file));
            if !needs_shared_output {
                let mut ok = true;
                if publish_video {
                    ok &= self.publish_video_with_settings(None, &draft.settings);
                }
                if ok && publish_desktop {
                    ok &= self.publish_desktop_app_with_settings(
                        None,
                        &Self::runtime_dir(),
                        &draft.settings,
                    );
                }
                if ok && publish_images {
                    ok &= self.publish_image_sequence_with_settings(None, &draft.settings);
                }
                if ok && publish_svg {
                    ok &= self.publish_svg_with_settings(None, &draft.settings);
                }
                if ok {
                    self.workspace.publish = draft.settings.clone();
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
                if ok && publish_desktop {
                    ok &= self.publish_desktop_app_with_settings(
                        Some(&output),
                        &Self::runtime_dir(),
                        &draft.settings,
                    );
                }
                if ok && publish_video {
                    ok &= self.publish_video_with_settings(Some(&output), &draft.settings);
                }
                if ok && publish_images {
                    ok &= self.publish_image_sequence_with_settings(Some(&output), &draft.settings);
                }
                if ok && publish_svg {
                    ok &= self.publish_svg_with_settings(Some(&output), &draft.settings);
                }
                if ok {
                    self.workspace.publish = draft.settings.clone();
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
            && w_val.is_finite()
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
            && h_val.is_finite()
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
            && fps_val.is_finite()
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

fn draw_desktop_app_target_settings(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.label(rich("Desktop app (macOS / Windows)", TextStyle::Title).color(p.text_primary));
    group(ui, "Output");
    ui.horizontal(|ui| {
        let mut folder = draft
            .settings
            .desktop_app_folder
            .clone()
            .or_else(|| draft.settings.output_folder.clone())
            .unwrap_or_default();
        let hint = format!("{}.app", project.name);
        if ui
            .add(
                c::TextField::new(&mut folder, "Desktop app folder")
                    .prefix("Folder")
                    .hint(&hint)
                    .width(420.0),
            )
            .changed()
        {
            draft.settings.desktop_app_folder =
                (!folder.trim().is_empty()).then(|| folder.trim().to_owned());
            draft.error = None;
        }
        if ui.add(c::Button::new("Browse…")).clicked()
            && let Some(picked) = rfd::FileDialog::new()
                .set_title("Choose desktop app output folder")
                .pick_folder()
        {
            draft.settings.desktop_app_folder = Some(picked.to_string_lossy().into_owned());
            draft.error = None;
        }
    });
    ui.add(c::InlineMessage::info(
        "Exports a self-contained macOS .app bundle and Windows launcher wrapping the Clean HTML5 runtime.",
    ));
    if let Some(error) = &draft.error {
        ui.add(c::InlineMessage::error(error));
    }
}

fn draw_image_sequence_target_settings(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.label(rich("Image sequence / sprite sheet", TextStyle::Title).color(p.text_primary));
    group(ui, "Output");
    ui.horizontal(|ui| {
        let mut folder = draft
            .settings
            .image_sequence_folder
            .clone()
            .or_else(|| draft.settings.output_folder.clone())
            .unwrap_or_default();
        if ui
            .add(
                c::TextField::new(&mut folder, "Image sequence folder")
                    .prefix("Folder")
                    .hint("~/Projects/ideas/frames")
                    .width(420.0),
            )
            .changed()
        {
            draft.settings.image_sequence_folder =
                (!folder.trim().is_empty()).then(|| folder.trim().to_owned());
            draft.error = None;
        }
        if ui.add(c::Button::new("Browse…")).clicked()
            && let Some(picked) = rfd::FileDialog::new()
                .set_title("Choose image sequence / sprite sheet folder")
                .pick_folder()
        {
            draft.settings.image_sequence_folder = Some(picked.to_string_lossy().into_owned());
            draft.error = None;
        }
    });
    group(ui, "Format");
    c::Segmented::show(
        ui,
        &mut draft.settings.image_sequence_sprite_sheet,
        &[
            (false, "Frame sequence"),
            (true, "Sprite sheet / Texture atlas"),
        ],
        false,
    );
    if draft.settings.image_sequence_sprite_sheet {
        ui.horizontal_wrapped(|ui| {
            let meta_lbl = format!("Metadata: {}", draft.settings.atlas_format.label());
            c::Select::new(ui.id().with("publish-atlas-format"), &meta_lbl)
                .width(210.0)
                .show(ui, |ui| {
                    for fmt in AtlasFormat::ALL {
                        if ui
                            .add(
                                c::MenuItem::new(&format!("Metadata: {}", fmt.label()))
                                    .checked(draft.settings.atlas_format == fmt),
                            )
                            .clicked()
                        {
                            draft.settings.atlas_format = fmt;
                            draft.error = None;
                            ui.close();
                        }
                    }
                });
            let img_lbl = format!("Image: {}", draft.settings.atlas_image_format.label());
            c::Select::new(ui.id().with("publish-atlas-img-format"), &img_lbl)
                .width(150.0)
                .show(ui, |ui| {
                    for fmt in AtlasImageFormat::ALL {
                        if ui
                            .add(
                                c::MenuItem::new(&format!("Image: {}", fmt.label()))
                                    .checked(draft.settings.atlas_image_format == fmt),
                            )
                            .clicked()
                        {
                            draft.settings.atlas_image_format = fmt;
                            draft.error = None;
                            ui.close();
                        }
                    }
                });
            let mut ext_val = f64::from(draft.settings.atlas_extrude.min(8));
            if ui
                .add(
                    c::Field::new(&mut ext_val, "Extrude")
                        .prefix("Extrude")
                        .unit("px")
                        .decimals(0)
                        .range(0.0..=8.0)
                        .width(120.0),
                )
                .changed()
            {
                draft.settings.atlas_extrude = ext_val.round().clamp(0.0, 8.0) as u32;
                draft.error = None;
            }
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            let seq_lbl = format!(
                "Sequence format: {}",
                draft.settings.image_sequence_format.label()
            );
            c::Select::new(ui.id().with("publish-seq-format"), &seq_lbl)
                .width(220.0)
                .show(ui, |ui| {
                    for fmt in ImageExportFormat::SEQUENCE_FORMATS {
                        if ui
                            .add(
                                c::MenuItem::new(&format!("Sequence format: {}", fmt.label()))
                                    .checked(draft.settings.image_sequence_format == fmt),
                            )
                            .clicked()
                        {
                            draft.settings.image_sequence_format = fmt;
                            draft.error = None;
                            ui.close();
                        }
                    }
                });
            if draft.settings.image_sequence_format == ImageExportFormat::Jpeg {
                let mut q = f64::from(draft.settings.jpeg_quality.clamp(1, 100));
                if ui
                    .add(
                        c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality")
                            .step(1.0)
                            .width(160.0),
                    )
                    .changed()
                {
                    draft.settings.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
                    draft.error = None;
                }
                caption(ui, &format!("{} %", draft.settings.jpeg_quality));
            }
        });
    }
    caption(
        ui,
        &format!("Stage resolution: {}×{} px", project.width, project.height),
    );
    if let Some(error) = &draft.error {
        ui.add(c::InlineMessage::error(error));
    }
}

fn draw_svg_target_settings(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.label(rich("SVG animation", TextStyle::Title).color(p.text_primary));
    group(ui, "Output");
    ui.horizontal(|ui| {
        let mut file = draft
            .settings
            .svg_file
            .clone()
            .or_else(|| {
                draft
                    .settings
                    .output_folder
                    .as_deref()
                    .map(|s| PathBuf::from(s).join("animation.svg").display().to_string())
            })
            .unwrap_or_default();
        if ui
            .add(
                c::TextField::new(&mut file, "SVG file")
                    .prefix("File")
                    .hint("animation.svg")
                    .width(420.0),
            )
            .changed()
        {
            draft.settings.svg_file = (!file.trim().is_empty()).then(|| file.trim().to_owned());
            draft.error = None;
        }
        if ui.add(c::Button::new("Browse…")).clicked()
            && let Some(picked) = rfd::FileDialog::new()
                .set_title("Export SVG animation")
                .add_filter("SVG", &["svg"])
                .set_file_name("animation.svg")
                .save_file()
        {
            draft.settings.svg_file = Some(picked.to_string_lossy().into_owned());
            draft.error = None;
        }
    });
    caption(
        ui,
        &format!(
            "Exports vector SVG 1.1 ({}×{} px).",
            project.width, project.height
        ),
    );
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
