//! Publish settings (DL-03 / R-032; v3 composition `S.publish`). The draft and estimate share one
//! PublishPrefs snapshot. The dialog has one size per density: a target sidebar, one scrolling page
//! on the form grid and a footer that is the same for every target.
use super::dialog_helpers::{
    DialogShell, fixed_dialog_size, form_block, form_hint, form_row, group_caption, nav_row,
    output_group,
};
use super::planned::caption;
use crate::{
    app::Editor,
    publish_controller::EstimateState,
    workspace::{PublishPrefs, PublishProfilePreset, PublishTarget},
};
use eframe::egui::{self, Ui};
use neoflash_export::{
    AtlasFormat, AtlasImageFormat, AudioExportPolicy, BannerOptions, FontExportPolicy,
    HtmlScaleMode, HtmlStageAlign, HtmlTemplate, ImageExportFormat, NetworkPreset,
    NetworkSimulationProfile, NetworkStartupEstimate, PublishProfile, SequencePublishPolicy,
    SizeEstimate, SizeLevel, VideoAudioBitrate, VideoCodec, VideoFormat, VideoOptions,
    VideoStopMode, duplicate_profile, export_profile_json, import_profile_json,
    load_profiles_from_project, simulate_network_startup, store_profiles_in_project,
};
use neoflash_ui::{components as c, icons::i16, metrics, tokens::TextStyle, typography::rich};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct PublishSettingsDraft {
    pub(crate) settings: PublishPrefs,
    pub(crate) selected_target: PublishTarget,
    /// Keep the URL while trying another level. Only Shared serializes it.
    runtime_url: String,
    error: Option<String>,
    pub(crate) custom_profiles: Vec<PublishProfile>,
    pub(crate) active_custom_profile_id: Option<String>,
    pub(crate) profiles_dirty: bool,
    pub(crate) network_preset: NetworkPreset,
    pub(crate) network_estimate: Option<NetworkStartupEstimate>,
}

impl PublishSettingsDraft {
    pub(crate) fn new(settings: PublishPrefs) -> Self {
        let runtime_url = match &settings.options.size {
            SizeLevel::Shared(url) => url.clone(),
            _ => String::new(),
        };
        let selected_target = if settings.profile == PublishProfilePreset::Criteo {
            PublishTarget::Html5
        } else {
            settings.default_target
        };
        Self {
            settings,
            selected_target,
            runtime_url,
            error: None,
            custom_profiles: Vec::new(),
            active_custom_profile_id: None,
            profiles_dirty: false,
            network_preset: NetworkPreset::Fast3G,
            network_estimate: None,
        }
    }

    pub(crate) fn new_for_project(
        settings: PublishPrefs,
        project: &neoflash_core::Project,
    ) -> Self {
        let mut draft = Self::new(settings);
        sync_publish_profile_with_render_mode(&mut draft.settings, project.html_render_mode());
        let has_stored_profiles = project.resources.iter().any(|r| {
            r.path
                .eq_ignore_ascii_case(neoflash_export::PUBLISH_PROFILES_RESOURCE_PATH)
        });
        if has_stored_profiles
            && let Ok((profiles, active_id)) = load_profiles_from_project(project)
        {
            draft.custom_profiles = profiles;
            if let Some(ref act) = active_id
                && let Some(found) = draft.custom_profiles.iter().find(|p| p.id == *act).cloned()
            {
                draft.restore_profile_delivery(&found);
                draft.active_custom_profile_id = Some(found.id.clone());
                draft.network_preset = found.network_profile.preset;
                draft.settings.render_mode = found.render_mode;
            }
        }
        let _ = draft.simulate_network(project);
        draft
    }

    pub(crate) fn active_profile_label(&self) -> String {
        if let Some(ref act_id) = self.active_custom_profile_id
            && let Some(found) = self.custom_profiles.iter().find(|p| p.id == *act_id)
        {
            return found.name.clone();
        }
        self.settings.profile.label().to_string()
    }

    pub(crate) fn to_publish_profile(&self) -> PublishProfile {
        if let Some(ref act_id) = self.active_custom_profile_id
            && let Some(found) = self.custom_profiles.iter().find(|p| p.id == *act_id)
        {
            let mut profile = found.clone();
            profile.options = self.settings.options.clone();
            profile.criteo_max_zip_kib = self.criteo_max_zip_kib();
            profile.network_profile.preset = self.network_preset;
            profile.render_mode = self.settings.render_mode;
            return profile;
        }
        PublishProfile {
            id: format!(
                "profile-{}",
                self.settings
                    .profile
                    .label()
                    .to_ascii_lowercase()
                    .replace(' ', "-")
            ),
            name: self.settings.profile.label().to_string(),
            options: self.settings.options.clone(),
            criteo_max_zip_kib: self.criteo_max_zip_kib(),
            html_template: None,
            network_profile: NetworkSimulationProfile {
                preset: self.network_preset,
                delayed_assets: Vec::new(),
                extra_asset_delay_ms: 0,
            },
            render_mode: self.settings.render_mode,
        }
    }

    fn criteo_max_zip_kib(&self) -> Option<u32> {
        (self.settings.profile == PublishProfilePreset::Criteo)
            .then_some(self.settings.criteo_max_zip_kib)
    }

    fn restore_profile_delivery(&mut self, profile: &PublishProfile) {
        restore_criteo_preferences(&mut self.settings, profile);
        if self.settings.profile == PublishProfilePreset::Criteo {
            self.selected_target = PublishTarget::Html5;
        }
    }

    pub(crate) fn select_custom_profile(&mut self, profile_id: &str) -> bool {
        let Some(found) = self
            .custom_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        else {
            return false;
        };
        self.restore_profile_delivery(&found);
        self.active_custom_profile_id = Some(found.id);
        self.settings.options = found.options;
        self.settings.render_mode = found.render_mode;
        self.runtime_url = match &self.settings.options.size {
            SizeLevel::Shared(url) => url.clone(),
            _ => self.runtime_url.clone(),
        };
        self.network_preset = found.network_profile.preset;
        self.error = None;
        true
    }

    pub(crate) fn duplicate_active_profile(&mut self, new_name: Option<&str>) -> PublishProfile {
        let base = self.to_publish_profile();
        let duplicated = duplicate_profile(&base, new_name);
        self.custom_profiles.push(duplicated.clone());
        self.active_custom_profile_id = Some(duplicated.id.clone());
        self.profiles_dirty = true;
        self.error = None;
        duplicated
    }

    pub(crate) fn export_active_profile_json(&self) -> Result<String, String> {
        let profile = self.to_publish_profile();
        export_profile_json(&profile).map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub(crate) fn export_active_profile_to_path(&self, path: &Path) -> Result<(), String> {
        self.export_active_profile_to_path_protecting(path, None)
    }

    pub(crate) fn export_active_profile_to_path_protecting(
        &self,
        path: &Path,
        protected_project_path: Option<&Path>,
    ) -> Result<(), String> {
        if let Some(proj_path) = protected_project_path
            && crate::headless::paths_refer_to_same_file(proj_path, path)
        {
            return Err(format!(
                "Refusing to overwrite source project '{}' with publish profile JSON. Choose a separate .json output path.",
                proj_path.display()
            ));
        }
        let json = self.export_active_profile_json()?;
        crate::windows_platform::atomic_replace_file(path, json.as_bytes())
    }

    pub(crate) fn import_profile_from_json(
        &mut self,
        json: &str,
    ) -> Result<PublishProfile, String> {
        let imported = import_profile_json(json).map_err(|e| e.to_string())?;
        if let Some(existing) = self
            .custom_profiles
            .iter_mut()
            .find(|p| p.id == imported.id)
        {
            *existing = imported.clone();
        } else {
            self.custom_profiles.push(imported.clone());
        }
        let _ = self.select_custom_profile(&imported.id);
        self.profiles_dirty = true;
        Ok(imported)
    }

    pub(crate) fn import_profile_from_path(
        &mut self,
        path: &Path,
    ) -> Result<PublishProfile, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        self.import_profile_from_json(&raw)
    }

    pub(crate) fn simulate_network(
        &mut self,
        project: &neoflash_core::Project,
    ) -> Result<NetworkStartupEstimate, String> {
        let profile = self.to_publish_profile();
        if profile.criteo_max_zip_kib.is_some() {
            self.network_estimate = None;
            return Err("Criteo ZIP plays entirely from the archive.".into());
        }
        let report = simulate_network_startup(project, &profile).map_err(|e| e.to_string())?;
        self.network_estimate = Some(report.clone());
        Ok(report)
    }

    /// Screenshot aid: select the target page by a short slug (unknown slugs keep the selection).
    pub(crate) fn select_target_slug(&mut self, slug: &str) {
        self.selected_target = match slug.trim() {
            "html5" => PublishTarget::Html5,
            "desktop" => PublishTarget::DesktopApp,
            "video" => PublishTarget::VideoMp4,
            "images" => PublishTarget::ImageSequence,
            "svg" => PublishTarget::SvgAnimation,
            _ => self.selected_target,
        };
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

/// Segments of the `Size level` control.
const LEVELS: [(usize, &str); 4] = [(0, "Full"), (1, "Balanced"), (2, "Compact"), (3, "Shared")];

/// Level name in summaries and messages.
fn level_label(level: &SizeLevel) -> &'static str {
    match level {
        SizeLevel::Shared(_) => "Shared runtime",
        level => LEVELS[level_index(level)].1,
    }
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
        EstimateState::Criteo { zip_bytes, .. } => {
            format!("Criteo · {} ZIP · 1 file", bytes_label(*zip_bytes))
        }
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

fn restore_criteo_preferences(settings: &mut PublishPrefs, profile: &PublishProfile) {
    if let Some(budget) = profile.criteo_max_zip_kib {
        settings.profile = PublishProfilePreset::Criteo;
        settings.criteo_max_zip_kib = budget;
    } else if settings.profile == PublishProfilePreset::Criteo {
        settings.profile = PublishProfilePreset::Default;
    }
}

fn active_document_profile(project: &neoflash_core::Project) -> Option<PublishProfile> {
    // With no stored profiles, the loader synthesizes a Default profile. In that case
    // the workspace preset is the explicitly chosen fallback, including Criteo.
    if !project.resources.iter().any(|r| {
        r.path
            .eq_ignore_ascii_case(neoflash_export::PUBLISH_PROFILES_RESOURCE_PATH)
    }) {
        return None;
    }
    let (profiles, active) = load_profiles_from_project(project).ok()?;
    let active = active?;
    profiles.into_iter().find(|profile| profile.id == active)
}

pub(crate) fn sync_document_publish_preferences(
    settings: &mut PublishPrefs,
    project: &neoflash_core::Project,
) {
    // A stored banner preset belongs to this document. It must not affect the next
    // document's workspace delivery or overwrite a different explicit ZIP budget.
    if active_document_profile(project).is_some_and(|profile| profile.criteo_max_zip_kib.is_some())
    {
        return;
    }
    sync_publish_profile_with_render_mode(settings, project.html_render_mode());
}

fn sync_publish_profile_with_render_mode(
    publish: &mut PublishPrefs,
    doc_mode: neoflash_core::HtmlRenderMode,
) {
    publish.render_mode = doc_mode;
    if publish.profile == PublishProfilePreset::Criteo {
        return;
    }
    match doc_mode {
        neoflash_core::HtmlRenderMode::Dom => {
            if publish.profile != PublishProfilePreset::WebAppDom {
                publish.profile = PublishProfilePreset::WebAppDom;
            }
        }
        neoflash_core::HtmlRenderMode::Canvas => {
            if publish.profile == PublishProfilePreset::WebAppDom {
                publish.profile = PublishProfilePreset::Default;
            }
        }
    }
}

fn stored_profiles_changed(
    project: &neoflash_core::Project,
    custom_profiles: &[PublishProfile],
    active_custom_profile_id: Option<&str>,
) -> bool {
    let has_stored = project.resources.iter().any(|r| {
        r.path
            .eq_ignore_ascii_case(neoflash_export::PUBLISH_PROFILES_RESOURCE_PATH)
    });
    if !has_stored {
        return !custom_profiles.is_empty() || active_custom_profile_id.is_some();
    }
    match load_profiles_from_project(project) {
        Ok((existing_profiles, existing_active)) => {
            existing_profiles != custom_profiles
                || existing_active.as_deref() != active_custom_profile_id
        }
        Err(_) => true,
    }
}

impl Editor {
    pub(crate) fn commit_publish_settings_to_project(&mut self, draft: &mut PublishSettingsDraft) {
        let profiles_changed = draft.profiles_dirty
            && stored_profiles_changed(
                &self.project,
                &draft.custom_profiles,
                draft.active_custom_profile_id.as_deref(),
            );
        let render_mode = draft.settings.render_mode;
        let render_changed = self.project.html_render_mode() != render_mode;
        if !profiles_changed && !render_changed {
            draft.profiles_dirty = false;
            return;
        }
        let custom_profiles = draft.custom_profiles.clone();
        let active_id = draft.active_custom_profile_id.clone();
        self.finish_edits();
        self.transact_named("Publish settings", |editor| {
            if profiles_changed {
                let _ = store_profiles_in_project(
                    &mut editor.project,
                    &custom_profiles,
                    active_id.as_deref(),
                );
            }
            if render_changed {
                editor.project.set_html_render_mode(render_mode);
            }
        });
        draft.profiles_dirty = false;
    }

    /// File > Publish and publish-on-save resolve the same document override as the dialog.
    pub(crate) fn effective_publish_preferences(&self) -> PublishPrefs {
        let mut settings = self.workspace.publish.clone();
        if let Some(profile) = active_document_profile(&self.project) {
            restore_criteo_preferences(&mut settings, &profile);
        }
        settings
    }

    fn save_publish_settings(&mut self, draft: &mut PublishSettingsDraft) {
        self.commit_publish_settings_to_project(draft);
        let mut settings = draft.settings.clone();
        if draft.active_custom_profile_id.is_some()
            && (settings.profile == PublishProfilePreset::Criteo
                || self.workspace.publish.profile == PublishProfilePreset::Criteo)
        {
            settings.profile = self.workspace.publish.profile;
            settings.criteo_max_zip_kib = self.workspace.publish.criteo_max_zip_kib;
        }
        self.workspace.publish = settings;
    }

    pub(crate) fn apply_publish_settings_draft(&mut self, draft: &mut PublishSettingsDraft) {
        draft.settings.sanitize();
        self.save_publish_settings(draft);
    }

    pub(crate) fn open_publish_settings(&mut self) {
        self.finish_edits();
        self.publish_controller.cancel();
        self.publish_settings = Some(PublishSettingsDraft::new_for_project(
            self.workspace.publish.clone(),
            &self.project,
        ));
    }

    /// Sidebar: the profile row (select + ⋯ menu) and one row per target. Returns the profile
    /// file action picked from the ⋯ menu.
    fn publish_sidebar(
        &mut self,
        ui: &mut Ui,
        draft: &mut PublishSettingsDraft,
    ) -> Option<ProfileFileAction> {
        let mut action = None;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), metrics::control()),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let more = ui.add(c::IconButton::outlined(i16::MORE, "Profile actions"));
                c::popup_below(&more, metrics::menu_width(), |ui| {
                    if ui.add(c::MenuItem::new("Duplicate")).clicked() {
                        let dup = draft.duplicate_active_profile(None);
                        let _ = draft.simulate_network(&self.project);
                        self.message(format!("Duplicated publish profile: {}", dup.name));
                        ui.close();
                    }
                    if ui.add(c::MenuItem::new("Import JSON…")).clicked() {
                        action = Some(ProfileFileAction::Import);
                        ui.close();
                    }
                    if ui.add(c::MenuItem::new("Export JSON…")).clicked() {
                        action = Some(ProfileFileAction::Export);
                        ui.close();
                    }
                });
                let profile_label = draft.active_profile_label();
                c::Select::new(ui.id().with("publish-settings-profile"), &profile_label)
                    .prefix("Profile")
                    .width(ui.available_width())
                    .show(ui, |ui| {
                        for preset in PublishProfilePreset::ALL {
                            let is_checked = draft.active_custom_profile_id.is_none()
                                && draft.settings.profile == preset;
                            if ui
                                .add(c::MenuItem::new(preset.label()).checked(is_checked))
                                .clicked()
                            {
                                if draft.active_custom_profile_id.take().is_some() {
                                    draft.profiles_dirty = true;
                                }
                                draft.settings.apply_profile(preset);
                                if preset == PublishProfilePreset::Criteo {
                                    draft.selected_target = PublishTarget::Html5;
                                }
                                let _ = draft.simulate_network(&self.project);
                                draft.error = None;
                                ui.close();
                            }
                        }
                        let custom_snapshot = draft.custom_profiles.clone();
                        for custom_prof in custom_snapshot {
                            let is_checked = draft.active_custom_profile_id.as_deref()
                                == Some(custom_prof.id.as_str());
                            if ui
                                .add(c::MenuItem::new(&custom_prof.name).checked(is_checked))
                                .clicked()
                            {
                                draft.select_custom_profile(&custom_prof.id);
                                let _ = draft.simulate_network(&self.project);
                                ui.close();
                            }
                        }
                    });
            },
        );
        ui.add_space(metrics::row_gap());
        for (target, icon, name) in publish_targets() {
            let selected = draft.selected_target == target;
            let enabled = match target {
                PublishTarget::Html5 => &mut draft.settings.html5_enabled,
                PublishTarget::DesktopApp => &mut draft.settings.desktop_app_enabled,
                PublishTarget::VideoMp4 => &mut draft.settings.video_enabled,
                PublishTarget::ImageSequence => &mut draft.settings.image_sequence_enabled,
                PublishTarget::SvgAnimation => &mut draft.settings.svg_enabled,
            };
            if nav_row(ui, icon, name, selected, Some(enabled)).clicked() {
                draft.selected_target = target;
            }
        }
        action
    }

    /// Footer summary of the selected target: format · size estimate · duration · dimensions.
    fn publish_summary(&self, draft: &PublishSettingsDraft, estimate: &EstimateState) -> String {
        match draft.selected_target {
            PublishTarget::VideoMp4 => video_footer_label(&self.project, draft),
            PublishTarget::DesktopApp => format!(
                "macOS .app + Windows Launcher · {}",
                level_label(&draft.settings.options.size)
            ),
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
                    match neoflash_export::estimate_native_banner(
                        &self.project,
                        &banner_options(&draft.settings),
                    ) {
                        Ok(est) => format!(
                            "Web Banner · {} · {} gzip · 1 file",
                            bytes_label(est.raw_bytes),
                            bytes_label(est.gzip_bytes)
                        ),
                        Err(_) => "Web Banner · Size unknown".into(),
                    }
                } else {
                    footer_label(draft, estimate)
                }
            }
        }
    }

    /// HTML5 page. Returns `true` when `Export runtime folder…` was clicked.
    fn html5_target_page(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        draft: &mut PublishSettingsDraft,
        estimate: &mut EstimateState,
    ) -> bool {
        let mut export_runtime = false;
        page_title(ui, PublishTarget::Html5, draft);
        // The exporter owns the entry file name; Browse selects its folder.
        let criteo = draft.settings.profile == PublishProfilePreset::Criteo;
        let entry_name = if criteo {
            crate::output_paths::criteo_archive_name(&self.project)
        } else if draft.settings.html_wrapper {
            "index.html".into()
        } else {
            "bundle.js".into()
        };
        let mut file = draft
            .settings
            .output_folder
            .as_deref()
            .map(|s| PathBuf::from(s).join(entry_name).display().to_string())
            .unwrap_or_default();
        if output_group(ui, "File", |ui, width| {
            ui.add_enabled(
                false,
                c::TextField::new(&mut file, "File")
                    .hint("Choose a file")
                    .width(width),
            );
        }) && let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose HTML5 output folder")
            .pick_folder()
        {
            draft.settings.output_folder = Some(folder.to_string_lossy().into_owned());
        }
        form_row(ui, "", |ui| {
            if criteo {
                ui.disable();
            }
            ui.add(c::Checkbox::new(
                &mut draft.settings.html_wrapper,
                "Include HTML wrapper",
            ));
            c::Select::new(
                ui.id().with("publish-settings-template"),
                if criteo {
                    "Criteo"
                } else {
                    draft.settings.template.label()
                },
            )
            .prefix("Template")
            .width(metrics::field_l())
            .show(ui, |ui| {
                for tmpl in HtmlTemplate::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(tmpl.label()).checked(draft.settings.template == tmpl),
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

        group_caption(ui, "Runtime");
        if criteo || draft.settings.profile == PublishProfilePreset::WebBannerNative {
            let (field_label, minimum, unit, hint) = if criteo {
                ("Max ZIP", 1, "KiB", "Self-contained HTML5 ZIP")
            } else {
                (
                    "Max gzip",
                    10,
                    "KB",
                    "Single-file HTML5 ad banner (no WASM)",
                )
            };
            form_row(ui, "Budget", |ui| {
                let stored = if criteo {
                    &mut draft.settings.criteo_max_zip_kib
                } else {
                    &mut draft.settings.banner_max_gzip_kb
                };
                let mut max_kb = f64::from((*stored).clamp(minimum, 5000));
                if ui
                    .add(
                        c::Field::new(&mut max_kb, field_label)
                            .prefix(field_label)
                            .unit(unit)
                            .decimals(0)
                            .range(f64::from(minimum)..=5000.0)
                            .width(metrics::field_m()),
                    )
                    .changed()
                    && max_kb.is_finite()
                {
                    *stored = max_kb.round().clamp(f64::from(minimum), 5000.0) as u32;
                    if criteo
                        && let Some(profile) = draft
                            .custom_profiles
                            .iter_mut()
                            .find(|p| Some(&p.id) == draft.active_custom_profile_id.as_ref())
                    {
                        profile.criteo_max_zip_kib = Some(*stored);
                        draft.profiles_dirty = true;
                    }
                    draft.error = None;
                }
                caption(ui, hint);
            });
            if criteo {
                form_hint(
                    ui,
                    "Packs index.html and its artwork into a ZIP. The budget applies to the finished archive.",
                );
                *estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
                if form_block(ui, |ui| weight(ui, estimate, draft)) {
                    self.publish_controller.cancel();
                    *estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
                }
                return false;
            }
            form_hint(
                ui,
                "Compiles eligible vector/bitmap artwork into one self-contained index.html with inline SVG, timeline transport, and window.clickTag.",
            );
            form_block(ui, |ui| {
                match neoflash_export::estimate_native_banner(
                    &self.project,
                    &banner_options(&draft.settings),
                ) {
                    Ok(est) => {
                        c::weight_totals(
                            ui,
                            &[
                                ("On disk", &bytes_label(est.raw_bytes)),
                                ("Transfer (gzip)", &bytes_label(est.gzip_bytes)),
                                (
                                    "Budget",
                                    &format!("{} KB gzip", draft.settings.banner_max_gzip_kb),
                                ),
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
            });
        } else {
            form_row(ui, "Size level", |ui| {
                let mut selected = if draft.custom() {
                    usize::MAX
                } else {
                    level_index(&draft.settings.options.size)
                };
                if c::Segmented::show(ui, &mut selected, &LEVELS, false).is_some() {
                    draft.select_level(selected);
                }
                if draft.custom() {
                    ui.add(c::Badge::new("Custom", c::BadgeType::Neutral));
                    if ui
                        .add(c::Button::ghost(&format!(
                            "Reset to {}",
                            level_label(&draft.settings.options.size)
                        )))
                        .clicked()
                    {
                        draft.select_level(level_index(&draft.settings.options.size));
                    }
                }
            });
            if matches!(draft.settings.options.size, SizeLevel::Shared(_)) {
                form_row(ui, "Runtime URL", |ui| {
                    let width = ui.available_width();
                    if ui
                        .add(c::TextField::new(&mut draft.runtime_url, "Runtime URL").width(width))
                        .changed()
                    {
                        draft.settings.options.size = SizeLevel::Shared(draft.runtime_url.clone());
                        draft.error = None;
                    }
                });
                if let Some(error) = draft.url_error() {
                    form_block(ui, |ui| ui.add(c::InlineMessage::error(&error)));
                }
                form_row(ui, "", |ui| {
                    export_runtime |= ui.add(c::Button::ghost("Export runtime folder…")).clicked();
                });
            }
            form_hint(
                ui,
                match draft.settings.options.size {
                    SizeLevel::Full => "Includes the full runtime. Opens directly from disk.",
                    SizeLevel::Balanced => {
                        "Includes all runtime families. Opens from disk or HTTP(S)."
                    }
                    SizeLevel::Compact => {
                        "Strips unused runtime families. Opens from disk or HTTP(S); dynamically named classes may need Balanced."
                    }
                    SizeLevel::Shared(_) => {
                        "Needs HTTP(S) hosting and a runtime from the same build. Cross-origin hosting needs CORS."
                    }
                },
            );
            // Refresh after editing as well as before reading: never paint stale totals.
            *estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
            if form_block(ui, |ui| weight(ui, estimate, draft)) {
                self.publish_controller.cancel();
                *estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
            }
        }

        group_caption(ui, "Playback");
        form_row(ui, "", |ui| {
            ui.add(c::Checkbox::new(&mut draft.settings.loop_timeline, "Loop"));
            ui.add(c::Checkbox::new(&mut draft.settings.autoplay, "Autoplay"));
            ui.add(c::Checkbox::new(&mut draft.settings.preloader, "Preloader"));
            ui.add(c::Checkbox::new(
                &mut draft.settings.allow_fullscreen,
                "Fullscreen",
            ));
        });
        form_row(ui, "Stage", |ui| {
            c::Select::new(
                ui.id().with("publish-settings-scale"),
                draft.settings.scale_mode.label(),
            )
            .prefix("Scale")
            .width(metrics::field_l())
            .show(ui, |ui| {
                for mode in HtmlScaleMode::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(mode.label())
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
                draft.settings.stage_align.label(),
            )
            .prefix("Align")
            .width(metrics::field_l())
            .show(ui, |ui| {
                for align in HtmlStageAlign::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(align.label())
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

        group_caption(ui, "Assets");
        form_row(ui, "JPEG quality", |ui| {
            let mut q = f64::from(draft.settings.jpeg_quality.clamp(1, 100));
            if ui
                .add(
                    c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality")
                        .value(c::SliderValue::Field)
                        .unit("%")
                        .width(metrics::field_m()),
                )
                .changed()
                && q.is_finite()
            {
                draft.settings.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
                draft.error = None;
            }
        });
        form_row(ui, "Audio", |ui| {
            c::Select::new(
                ui.id().with("publish-settings-audio"),
                draft.settings.options.audio.label(),
            )
            .width(metrics::field_m())
            .show(ui, |ui| {
                for policy in AudioExportPolicy::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(policy.label())
                                .checked(draft.settings.options.audio == policy),
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
        form_row(ui, "Bitmaps", |ui| {
            ui.add(c::Checkbox::new(
                &mut draft.settings.sprite_sheets,
                "Sprite sheets",
            ));
            let mut subset_fonts = draft.settings.options.fonts == FontExportPolicy::Subset;
            if ui
                .add(c::Checkbox::new(&mut subset_fonts, "Embed fonts (subset)"))
                .changed()
            {
                draft.settings.options.fonts = if subset_fonts {
                    FontExportPolicy::Subset
                } else {
                    FontExportPolicy::Embed
                };
                draft.error = None;
            }
        });
        form_row(ui, "Sequences", |ui| {
            c::Select::new(
                ui.id().with("publish-settings-sequence-policy"),
                draft.settings.sequence_policy.label(),
            )
            .width(metrics::field_l())
            .show(ui, |ui| {
                for policy in SequencePublishPolicy::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(policy.label())
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

        group_caption(ui, "Scripts");
        form_row(ui, "", |ui| {
            ui.add(c::Checkbox::new(&mut draft.settings.strict, "Strict mode"));
            ui.add(c::Checkbox::new(
                &mut draft.settings.include_trace,
                "Include trace()",
            ));
            ui.add(c::Checkbox::new(&mut draft.settings.minify, "Minify"));
            ui.add(c::Checkbox::new(
                &mut draft.settings.source_maps,
                "Source maps",
            ));
        });

        c::SectionHeader::new("Network startup simulator")
            .id_salt("publish-network-startup-simulator")
            .default_open(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = metrics::row_gap();
                form_row(ui, "Network", |ui| {
                    c::Select::new(
                        ui.id().with("publish-network-preset"),
                        draft.network_preset.short_label(),
                    )
                    .width(metrics::field_l())
                    .show(ui, |ui| {
                        for preset in NetworkPreset::ALL_PRESETS {
                            if ui
                                .add(
                                    c::MenuItem::new(preset.label())
                                        .checked(draft.network_preset == preset),
                                )
                                .clicked()
                            {
                                draft.network_preset = preset;
                                let _ = draft.simulate_network(&self.project);
                                ui.close();
                            }
                        }
                    });
                });
                if draft.network_estimate.is_none() {
                    let _ = draft.simulate_network(&self.project);
                }
                if let Some(ref sim) = draft.network_estimate {
                    let bootstrap_str = format!(
                        "{:.0} ms ({})",
                        sim.initial_html_js_wasm_ms,
                        bytes_label(sim.initial_html_js_wasm_bytes)
                    );
                    let total_str = format!("{:.0} ms", sim.total_download_ms);
                    let preload_str = if sim.buffer_underrun_frames.is_empty() {
                        "0 stalls (ready)".to_string()
                    } else {
                        format!(
                            "{} stalls · {:.0} ms ({} f)",
                            sim.buffer_underrun_frames.len(),
                            sim.recommended_preload_ms,
                            sim.recommended_preload_frames
                        )
                    };
                    form_block(ui, |ui| {
                        c::weight_totals(
                            ui,
                            &[
                                ("Bootstrap", &bootstrap_str),
                                ("Total download", &total_str),
                                ("Preload / Stalls", &preload_str),
                            ],
                        );
                    });
                }
            });
        export_runtime
    }

    pub(crate) fn publish_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.publish_settings.take() else {
            return;
        };
        let mut close = false;
        let mut save = false;
        let mut publish = false;
        let mut export_runtime = false;
        let mut import_profile_dialog = false;
        let mut export_profile_dialog = false;
        let kb = super::dialog_helpers::DialogKeyboardState::capture(ctx);
        let p = neoflash_ui::palette(ctx);
        let mut estimate = self.request_publish_estimate(ctx, &draft.settings).clone();
        // v3: one size for every target and option; the sidebar, the Output row and the footer
        // never move, a short page leaves empty space.
        let size = fixed_dialog_size(ctx, metrics::dialog_xl(), metrics::dialog_l_height());
        let modal = egui::Modal::new(egui::Id::new("publish-settings"))
            .frame(c::Surface::Dialog.frame(ctx))
            .backdrop_color(p.overlay)
            .show(ctx, |ui| {
                let shell =
                    DialogShell::show(ui, "Publish settings", size, metrics::sidebar_wide());
                close |= shell.close;
                shell.sidebar(ui, |ui| match self.publish_sidebar(ui, &mut draft) {
                    Some(ProfileFileAction::Import) => import_profile_dialog = true,
                    Some(ProfileFileAction::Export) => export_profile_dialog = true,
                    None => {}
                });
                shell.page(ui, "publish-settings-right-scroll", |ui, _| {
                    match draft.selected_target {
                        PublishTarget::Html5 => {
                            export_runtime |=
                                self.html5_target_page(ui, ctx, &mut draft, &mut estimate);
                        }
                        PublishTarget::DesktopApp => desktop_app_target_page(ui, &mut draft),
                        PublishTarget::VideoMp4 => {
                            video_target_page(ui, &self.project, &mut draft);
                        }
                        PublishTarget::ImageSequence => {
                            image_sequence_target_page(ui, &self.project, &mut draft);
                        }
                        PublishTarget::SvgAnimation => {
                            svg_target_page(ui, &self.project, &mut draft);
                        }
                    }
                    if let Some(error) = &draft.error {
                        ui.add(c::InlineMessage::error(error));
                    }
                });
                // Same footer for every target; action validity follows the draft URL.
                let summary = self.publish_summary(&draft, &estimate);
                let valid = draft.url_error().is_none();
                shell.footer(
                    ui,
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                rich(summary.as_str(), TextStyle::Small).color(p.text_secondary),
                            )
                            .truncate(),
                        );
                    },
                    |ui| {
                        publish |= ui
                            .add_enabled(valid, c::Button::primary("Publish").icon(i16::PUBLISH))
                            .clicked();
                        save |= ui.add_enabled(valid, c::Button::new("OK")).clicked();
                        close |= ui.add(c::Button::new("Cancel")).clicked();
                    },
                );
            });
        if !close
            && !save
            && !publish
            && !export_runtime
            && !import_profile_dialog
            && !export_profile_dialog
        {
            match kb.resolve(ctx, &modal, draft.url_error().is_none()) {
                super::dialog_helpers::DialogKeyboardOutcome::Cancel => close = true,
                super::dialog_helpers::DialogKeyboardOutcome::Confirm => save = true,
                super::dialog_helpers::DialogKeyboardOutcome::None => {}
            }
        }
        if !close && save {
            self.apply_publish_settings_draft(&mut draft);
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
                    let prev_mode = self.project.html_render_mode();
                    if prev_mode != draft.settings.render_mode {
                        self.project
                            .set_html_render_mode(draft.settings.render_mode);
                    }
                    ok &= self.publish_desktop_app_with_settings(
                        None,
                        &Self::runtime_dir(),
                        &draft.settings,
                    );
                    if prev_mode != draft.settings.render_mode {
                        self.project.set_html_render_mode(prev_mode);
                    }
                }
                if ok && publish_images {
                    ok &= self.publish_image_sequence_with_settings(None, &draft.settings);
                }
                if ok && publish_svg {
                    ok &= self.publish_svg_with_settings(None, &draft.settings);
                }
                if ok {
                    self.save_publish_settings(&mut draft);
                    close = true;
                } else {
                    draft.error = Some(self.status.clone());
                }
            } else if let Some(output) = publish_output(&draft.settings, &self.project) {
                let mut ok = true;
                if publish_html5 {
                    ok &= self.publish_html5_atomic(&output, &Self::runtime_dir(), &draft.settings);
                }
                if ok && publish_desktop {
                    let prev_mode = self.project.html_render_mode();
                    if prev_mode != draft.settings.render_mode {
                        self.project
                            .set_html_render_mode(draft.settings.render_mode);
                    }
                    ok &= self.publish_desktop_app_with_settings(
                        Some(&output),
                        &Self::runtime_dir(),
                        &draft.settings,
                    );
                    if prev_mode != draft.settings.render_mode {
                        self.project.set_html_render_mode(prev_mode);
                    }
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
                    self.save_publish_settings(&mut draft);
                    close = true;
                } else {
                    draft.error = Some(self.status.clone());
                }
            }
        } else if !close
            && import_profile_dialog
            && let Some(path) = rfd::FileDialog::new()
                .set_title("Import Publish Profile JSON")
                .add_filter("JSON Profile", &["json"])
                .pick_file()
        {
            match draft.import_profile_from_path(&path) {
                Ok(prof) => {
                    let _ = draft.simulate_network(&self.project);
                    self.message(format!("Imported publish profile: {}", prof.name));
                }
                Err(error) => draft.error = Some(error),
            }
        } else if !close
            && export_profile_dialog
            && let Some(path) = rfd::FileDialog::new()
                .set_title("Export Publish Profile JSON")
                .set_file_name("publish-profile.json")
                .add_filter("JSON Profile", &["json"])
                .save_file()
        {
            match draft.export_active_profile_to_path_protecting(&path, self.path.as_deref()) {
                Ok(()) => self.message(format!("Exported publish profile: {}", path.display())),
                Err(error) => draft.error = Some(error),
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
        super::dialog_helpers::record_end_of_frame_focus(ctx);
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

fn sync_publish_sequence_path_extension(
    target: &mut Option<String>,
    new_ext: &str,
    known_exts: &[&str],
) {
    let Some(raw) = target.as_deref() else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut pb = PathBuf::from(trimmed);
    let matches_known = pb
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| known_exts.iter().any(|k| ext.eq_ignore_ascii_case(k)));
    if matches_known {
        pb.set_extension(new_ext);
        *target = Some(pb.to_string_lossy().into_owned());
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

/// Profile file commands of the sidebar ⋯ menu; their file dialogs open after the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfileFileAction {
    Import,
    Export,
}

/// Publish targets in sidebar order: icon and the short name of the sidebar row.
fn publish_targets() -> [(PublishTarget, neoflash_ui::Icon, &'static str); 5] {
    [
        (PublishTarget::Html5, i16::GLOBE, "HTML5"),
        (PublishTarget::DesktopApp, i16::DESKTOP, "Desktop app"),
        (PublishTarget::VideoMp4, i16::VIDEO, "Video (MP4)"),
        (PublishTarget::ImageSequence, i16::GRID, "Image sequence"),
        (PublishTarget::SvgAnimation, i16::CODE, "SVG animation"),
    ]
}

/// Short target name, the same in the Publish settings sidebar and Preferences ▸ Publish.
pub(crate) fn target_short_name(target: PublishTarget) -> &'static str {
    publish_targets()
        .iter()
        .find(|(t, _, _)| *t == target)
        .map_or("", |(_, _, name)| name)
}

/// Page title: the full target name.
fn page_title(ui: &mut Ui, target: PublishTarget, draft: &PublishSettingsDraft) {
    let p = neoflash_ui::palette(ui.ctx());
    let title = PublishTarget::OPTIONS
        .iter()
        .find(|(t, _)| *t == target)
        .map_or("", |(_, title)| title);
    let title = if target == PublishTarget::Html5
        && draft.settings.profile == PublishProfilePreset::Criteo
    {
        PublishProfilePreset::Criteo.label()
    } else {
        title
    };
    ui.label(rich(title, TextStyle::Title).color(p.text_primary));
}

fn banner_options(settings: &PublishPrefs) -> BannerOptions {
    BannerOptions {
        max_gzip_bytes: Some(settings.banner_max_gzip_kb as usize * 1024),
        max_raw_bytes: Some(settings.banner_max_gzip_kb as usize * 4096),
        loop_timeline: settings.loop_timeline,
        autoplay: settings.autoplay,
        jpeg_quality: settings.jpeg_quality,
        ..BannerOptions::default()
    }
}

fn video_target_page(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    let mut opts = draft.effective_video_options(project);
    let mut changed = false;

    page_title(ui, PublishTarget::VideoMp4, draft);
    let default_hint = draft
        .settings
        .output_folder
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| PathBuf::from(s).join("movie.mp4").display().to_string())
        .unwrap_or_else(|| "Choose a file".into());
    let mut video_file = draft.settings.video_file.clone().unwrap_or_default();
    let mut edited = false;
    let browse = output_group(ui, "File", |ui, width| {
        edited = ui
            .add(
                c::TextField::new(&mut video_file, "Video file")
                    .hint(&default_hint)
                    .width(width),
            )
            .changed();
    });
    if edited {
        draft.settings.video_file = (!video_file.trim().is_empty()).then_some(video_file);
        draft.error = None;
    }
    if browse
        && let Some(picked) = rfd::FileDialog::new()
            .set_title("Choose MP4 output file")
            .add_filter("MPEG-4 Video", &["mp4"])
            .set_file_name("movie.mp4")
            .save_file()
    {
        draft.settings.video_file = Some(picked.to_string_lossy().into_owned());
        draft.error = None;
    }

    group_caption(ui, "Video");
    form_row(ui, "Size", |ui| {
        let mut w_val = f64::from(opts.width.max(1));
        if ui
            .add(
                c::Field::new(&mut w_val, "Width")
                    .prefix("W")
                    .unit("px")
                    .decimals(0)
                    .range(16.0..=7680.0)
                    .width(metrics::field_s()),
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
                    .width(metrics::field_s()),
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
            .add(
                c::IconButton::new(i16::LINK, "Constrain proportions")
                    .selected(opts.constrain_proportions),
            )
            .clicked()
        {
            opts.constrain_proportions = !opts.constrain_proportions;
            changed = true;
        }
    });
    form_row(ui, "Frame rate", |ui| {
        let mut fps_val = f64::from(opts.fps);
        if ui
            .add(
                c::Field::new(&mut fps_val, "Frame rate")
                    .unit("fps")
                    .decimals(0)
                    .range(1.0..=120.0)
                    .stepper()
                    .width(metrics::field_s()),
            )
            .changed()
            && fps_val.is_finite()
        {
            opts.fps = (fps_val as f32).clamp(1.0, 120.0);
            changed = true;
        }
    });
    form_row(ui, "Codec", |ui| {
        c::Select::new(ui.id().with("publish-video-codec"), opts.codec.label())
            .width(metrics::field_m())
            .show(ui, |ui| {
                for codec in [VideoCodec::H264, VideoCodec::H265] {
                    if ui
                        .add(c::MenuItem::new(codec.label()).checked(opts.codec == codec))
                        .clicked()
                    {
                        opts.codec = codec;
                        changed = true;
                        ui.close();
                    }
                }
            });
    });
    form_row(ui, "Quality", |ui| {
        let mut q_val = f64::from(opts.quality);
        if ui
            .add(
                c::Slider::new(&mut q_val, 1.0..=100.0, "Quality")
                    .step(1.0)
                    .width(metrics::field_m()),
            )
            .changed()
            && q_val.is_finite()
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

    group_caption(ui, "Content");
    form_row(ui, "", |ui| {
        changed |= ui
            .add(c::Checkbox::new(
                &mut opts.runtime_capture,
                "Render nested movie clip animation and scripts",
            ))
            .changed();
    });
    form_row(ui, "Duration", |ui| {
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
                        .width(metrics::field_num()),
                )
                .changed()
                && secs_val.is_finite()
            {
                opts.stop_mode = VideoStopMode::AfterSeconds(secs_val.clamp(0.1, 3600.0));
                changed = true;
            }
        });
    });

    group_caption(ui, "Audio");
    form_row(ui, "", |ui| {
        changed |= ui
            .add(c::Checkbox::new(&mut opts.include_audio, "Include audio"))
            .changed();
        ui.add_enabled_ui(opts.include_audio, |ui| {
            c::Select::new(
                ui.id().with("publish-video-audio-bitrate"),
                opts.audio_bitrate.label(),
            )
            .width(metrics::field_m())
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
}

fn desktop_app_target_page(ui: &mut Ui, draft: &mut PublishSettingsDraft) {
    page_title(ui, PublishTarget::DesktopApp, draft);
    let mut folder = draft
        .settings
        .desktop_app_folder
        .clone()
        .or_else(|| draft.settings.output_folder.clone())
        .unwrap_or_default();
    let mut edited = false;
    let browse = output_group(ui, "Folder", |ui, width| {
        edited = ui
            .add(
                c::TextField::new(&mut folder, "Desktop app folder")
                    .hint("Choose a folder")
                    .width(width),
            )
            .changed();
    });
    if edited {
        draft.settings.desktop_app_folder =
            (!folder.trim().is_empty()).then(|| folder.trim().to_owned());
        draft.error = None;
    }
    if browse
        && let Some(picked) = rfd::FileDialog::new()
            .set_title("Choose desktop app output folder")
            .pick_folder()
    {
        draft.settings.desktop_app_folder = Some(picked.to_string_lossy().into_owned());
        draft.error = None;
    }
    form_hint(
        ui,
        "Exports a self-contained macOS .app bundle and Windows launcher wrapping the Clean HTML5 runtime.",
    );
}

fn image_sequence_target_page(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    page_title(ui, PublishTarget::ImageSequence, draft);
    let mut folder = draft
        .settings
        .image_sequence_folder
        .clone()
        .or_else(|| draft.settings.output_folder.clone())
        .unwrap_or_default();
    let mut edited = false;
    let browse = output_group(ui, "Folder", |ui, width| {
        edited = ui
            .add(
                c::TextField::new(&mut folder, "Image sequence folder")
                    .hint("Choose a folder")
                    .width(width),
            )
            .changed();
    });
    if edited {
        draft.settings.image_sequence_folder =
            (!folder.trim().is_empty()).then(|| folder.trim().to_owned());
        draft.error = None;
    }
    if browse
        && let Some(picked) = rfd::FileDialog::new()
            .set_title("Choose image sequence / sprite sheet folder")
            .pick_folder()
    {
        draft.settings.image_sequence_folder = Some(picked.to_string_lossy().into_owned());
        draft.error = None;
    }

    group_caption(ui, "Format");
    form_row(ui, "Type", |ui| {
        c::Segmented::show(
            ui,
            &mut draft.settings.image_sequence_sprite_sheet,
            &[
                (false, "Frame sequence"),
                (true, "Sprite sheet / Texture atlas"),
            ],
            false,
        );
    });
    if draft.settings.image_sequence_sprite_sheet {
        form_row(ui, "Metadata", |ui| {
            c::Select::new(
                ui.id().with("publish-atlas-format"),
                draft.settings.atlas_format.label(),
            )
            .width(metrics::field_l())
            .show(ui, |ui| {
                for fmt in AtlasFormat::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(fmt.label())
                                .checked(draft.settings.atlas_format == fmt),
                        )
                        .clicked()
                    {
                        draft.settings.atlas_format = fmt;
                        sync_publish_sequence_path_extension(
                            &mut draft.settings.image_sequence_folder,
                            fmt.extension(),
                            &["json", "xml", "css"],
                        );
                        draft.error = None;
                        ui.close();
                    }
                }
            });
        });
        form_row(ui, "Image", |ui| {
            c::Select::new(
                ui.id().with("publish-atlas-img-format"),
                draft.settings.atlas_image_format.label(),
            )
            .width(metrics::field_m())
            .show(ui, |ui| {
                for fmt in AtlasImageFormat::ALL {
                    if ui
                        .add(
                            c::MenuItem::new(fmt.label())
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
        });
        form_row(ui, "Extrude", |ui| {
            let mut ext_val = f64::from(draft.settings.atlas_extrude.min(8));
            if ui
                .add(
                    c::Field::new(&mut ext_val, "Extrude")
                        .unit("px")
                        .decimals(0)
                        .range(0.0..=8.0)
                        .width(metrics::field_s()),
                )
                .changed()
                && ext_val.is_finite()
            {
                draft.settings.atlas_extrude = ext_val.round().clamp(0.0, 8.0) as u32;
                draft.error = None;
            }
        });
    } else {
        form_row(ui, "Image", |ui| {
            c::Select::new(
                ui.id().with("publish-seq-format"),
                draft.settings.image_sequence_format.label(),
            )
            .width(metrics::field_m())
            .show(ui, |ui| {
                for fmt in ImageExportFormat::SEQUENCE_FORMATS {
                    if ui
                        .add(
                            c::MenuItem::new(fmt.label())
                                .checked(draft.settings.image_sequence_format == fmt),
                        )
                        .clicked()
                    {
                        draft.settings.image_sequence_format = fmt;
                        sync_publish_sequence_path_extension(
                            &mut draft.settings.image_sequence_folder,
                            fmt.extension(),
                            &["png", "jpg", "jpeg", "webp", "svg", "gif", "bmp"],
                        );
                        draft.error = None;
                        ui.close();
                    }
                }
            });
        });
        if draft.settings.image_sequence_format == ImageExportFormat::Jpeg {
            form_row(ui, "JPEG quality", |ui| {
                let mut q = f64::from(draft.settings.jpeg_quality.clamp(1, 100));
                if ui
                    .add(
                        c::Slider::new(&mut q, 1.0..=100.0, "JPEG quality")
                            .step(1.0)
                            .value(c::SliderValue::Field)
                            .unit("%")
                            .width(metrics::field_m()),
                    )
                    .changed()
                    && q.is_finite()
                {
                    draft.settings.jpeg_quality = q.round().clamp(1.0, 100.0) as u8;
                    draft.error = None;
                }
            });
        }
    }
    form_hint(
        ui,
        &format!("Stage resolution: {}×{} px", project.width, project.height),
    );
}

fn svg_target_page(
    ui: &mut Ui,
    project: &neoflash_core::Project,
    draft: &mut PublishSettingsDraft,
) {
    page_title(ui, PublishTarget::SvgAnimation, draft);
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
    let mut edited = false;
    let browse = output_group(ui, "File", |ui, width| {
        edited = ui
            .add(
                c::TextField::new(&mut file, "SVG file")
                    .hint("Choose a file")
                    .width(width),
            )
            .changed();
    });
    if edited {
        draft.settings.svg_file = (!file.trim().is_empty()).then(|| file.trim().to_owned());
        draft.error = None;
    }
    if browse
        && let Some(picked) = rfd::FileDialog::new()
            .set_title("Export SVG animation")
            .add_filter("SVG", &["svg"])
            .set_file_name("animation.svg")
            .save_file()
    {
        draft.settings.svg_file = Some(picked.to_string_lossy().into_owned());
        draft.error = None;
    }
    form_hint(
        ui,
        &format!(
            "Exports vector SVG 1.1 ({}×{} px).",
            project.width, project.height
        ),
    );
}

fn publish_output(settings: &PublishPrefs, project: &neoflash_core::Project) -> Option<PathBuf> {
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
        .map(|parent| crate::output_paths::project_output_dir(&parent, project))
}

/// Returns whether Retry was pressed. All byte data comes from the exporter's assembled package.
fn weight(ui: &mut Ui, state: &EstimateState, draft: &PublishSettingsDraft) -> bool {
    let criteo = draft.settings.profile == PublishProfilePreset::Criteo;
    let pending_totals = if criteo {
        [("HTML", "—"), ("ZIP", "—"), ("Budget", "—")]
    } else {
        [("On disk", "—"), ("Transfer (gzip)", "—"), ("Files", "—")]
    };
    match state {
        EstimateState::Criteo {
            html_bytes,
            zip_bytes,
        } => {
            let budget = draft.settings.criteo_max_zip_kib;
            c::weight_totals(
                ui,
                &[
                    ("HTML", &bytes_label(*html_bytes)),
                    ("ZIP", &format!("{zip_bytes} B")),
                    ("Budget", &format!("{budget} KiB")),
                ],
            );
            if *zip_bytes > budget as usize * 1024 {
                ui.add(c::InlineMessage::error("ZIP exceeds the selected budget."));
            }
            false
        }
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
                &format!(
                    "Estimating {}…",
                    if criteo {
                        "ZIP"
                    } else {
                        level_label(&draft.settings.options.size)
                    }
                ),
                None,
                ui.available_width(),
            );
            c::weight_totals(ui, &pending_totals);
            false
        }
        EstimateState::Failed(error) => {
            // Invalid URLs already have their error directly under the field.
            let mut retry = false;
            if draft.url_error().is_none() {
                ui.add(c::InlineMessage::error(error));
                retry = ui.add(c::Button::ghost("Retry")).clicked();
            }
            c::weight_totals(ui, &pending_totals);
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

#[cfg(test)]
mod profile_and_network_sim_tests {
    use super::*;
    use neoflash_core::{Project, new_id};

    #[test]
    fn publish_settings_profile_duplicate_json_import_export_and_network_simulator() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        editor.open_publish_settings();

        let draft = editor
            .publish_settings
            .as_mut()
            .expect("publish settings draft opened");
        draft.settings.options.size = SizeLevel::Compact;
        draft.settings.options.fonts = FontExportPolicy::Subset;
        draft.settings.options.audio = AudioExportPolicy::Mp3_96;
        draft.network_preset = NetworkPreset::Slow3G;

        // 1. Duplicate profile inside PublishSettingsDraft
        let duplicated = draft.duplicate_active_profile(Some("Studio Compact Copy"));
        assert_eq!(duplicated.name, "Studio Compact Copy");
        assert_eq!(
            draft.active_custom_profile_id.as_deref(),
            Some(duplicated.id.as_str())
        );
        assert!(draft.profiles_dirty);
        assert_eq!(draft.active_profile_label(), "Studio Compact Copy");

        // 2. Export & Import profile JSON round-trip via filesystem
        let tmp_dir = std::env::temp_dir().join(format!(
            "neoflash-publish-settings-profile-{}-{}",
            std::process::id(),
            new_id("prof")
        ));
        let json_path = tmp_dir.join("studio-compact.json");
        draft
            .export_active_profile_to_path(&json_path)
            .expect("export profile JSON to path");
        assert!(json_path.is_file());

        let mut fresh_draft =
            PublishSettingsDraft::new_for_project(PublishPrefs::default(), &editor.project);
        assert_eq!(fresh_draft.settings.options.size, SizeLevel::Full);
        let imported = fresh_draft
            .import_profile_from_path(&json_path)
            .expect("import profile JSON from path");
        assert_eq!(imported.name, "Studio Compact Copy");
        assert_eq!(fresh_draft.settings.options.size, SizeLevel::Compact);
        assert_eq!(fresh_draft.settings.options.fonts, FontExportPolicy::Subset);
        assert_eq!(
            fresh_draft.settings.options.audio,
            AudioExportPolicy::Mp3_96
        );
        assert_eq!(fresh_draft.network_preset, NetworkPreset::Slow3G);

        // 3. Network Startup Simulator across Slow 3G, Fast 3G, Cable, and Offline / Local
        let mut bootstrap_times = Vec::new();
        for preset in [
            NetworkPreset::Slow3G,
            NetworkPreset::Fast3G,
            NetworkPreset::Cable5Mbps,
            NetworkPreset::OfflineLocal,
        ] {
            fresh_draft.network_preset = preset;
            let report = fresh_draft
                .simulate_network(&editor.project)
                .expect("simulate network startup");
            assert_eq!(report.preset, preset);
            assert!(report.initial_html_js_wasm_bytes > 0);
            bootstrap_times.push(report.initial_html_js_wasm_ms);
        }
        assert!(
            bootstrap_times[0] > bootstrap_times[1]
                && bootstrap_times[1] > bootstrap_times[2]
                && bootstrap_times[2] > bootstrap_times[3],
            "expected monotonic decrease in bootstrap download time from Slow 3G to Offline/Local: {bootstrap_times:?}"
        );

        let _ = std::fs::remove_dir_all(tmp_dir);
    }
}
