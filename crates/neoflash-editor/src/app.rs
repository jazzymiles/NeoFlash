use crate::fonts::Fonts;
use crate::theme::ThemeMode;
use crate::workspace::{PanelId, WorkspacePrefs};
use eframe::egui::{self, Key, Modifiers, Vec2};
use neoflash_core::*;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

mod dialogs;
mod export;
mod screenshot_boot;
mod shortcuts;
pub(crate) mod standalone_script;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Transform,
    Rectangle,
    Ellipse,
    Text,
    Pen,
    Pencil,
    Subselect,
    Line,
    Brush,
    Bucket,
    Ink,
    Eyedropper,
    Eraser,
    Lasso,
    Hand,
    Zoom,
    Gradient,
    PolyStar,
    Razor,
    Deco,
    SprayBrush,
    Bone,
    Bind,
    AssetWarp,
    Translate3D,
    Rotate3D,
}

impl Tool {
    pub const fn preserves_selection_on_rail_click(self) -> bool {
        matches!(
            self,
            Self::Select
                | Self::Bone
                | Self::Bind
                | Self::AssetWarp
                | Self::Translate3D
                | Self::Rotate3D
                | Self::Razor
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BottomTab {
    Properties,
    Actions,
    Accessibility,
    Output,
}
#[derive(Clone, Copy, PartialEq)]
pub enum ScriptTarget {
    Frame,
    Object,
    Source,
}

fn atomic_save_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "save destination directory does not exist: {}",
            parent.display()
        ));
    }
    crate::windows_platform::atomic_replace_file(path, bytes)
}

pub(crate) fn file_has_nflash_bundle_prefix(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut prefix = [0u8; 8];
    let mut read = 0usize;
    while read < prefix.len() {
        match std::io::Read::read(&mut file, &mut prefix[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    neoflash_core::has_nflash_bundle_prefix(&prefix[..read])
}

/// Bounded `.nflash` / `.json` project loader supporting both ordinary UTF-8 JSON (`<= 128 MiB`)
/// and self-contained `NFLASHB1` binary bundles (`<= 640 MiB`) (`T-471`).
pub(crate) fn read_nflash_project_file(path: &Path) -> Result<Project, String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let declared_len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut prefix = [0u8; 8];
    let mut prefix_len = 0usize;
    while prefix_len < prefix.len() {
        match file.read(&mut prefix[prefix_len..]) {
            Ok(0) => break,
            Ok(n) => prefix_len += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    if &prefix[..prefix_len] == neoflash_core::NFLASH_BUNDLE_MAGIC {
        if declared_len > neoflash_core::MAX_NFLASH_BUNDLE_BYTES as u64 {
            return Err("document exceeds the 640 MiB NFLASHB1 bundle limit".into());
        }
        let cap = usize::try_from(declared_len)
            .unwrap_or(neoflash_core::MAX_NFLASH_BUNDLE_BYTES)
            .min(neoflash_core::MAX_NFLASH_BUNDLE_BYTES);
        let mut bytes = Vec::with_capacity(cap);
        bytes.extend_from_slice(&prefix[..prefix_len]);
        let rem_cap = (neoflash_core::MAX_NFLASH_BUNDLE_BYTES - prefix_len) as u64 + 1;
        file.take(rem_cap)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > neoflash_core::MAX_NFLASH_BUNDLE_BYTES {
            return Err("document exceeds the 640 MiB NFLASHB1 bundle limit".into());
        }
        return Project::from_nflash_bytes(&bytes).map_err(|e| e.to_string());
    }
    if neoflash_core::has_nflash_bundle_prefix(&prefix[..prefix_len]) {
        return Err("truncated or unsupported NFLASHB1 bundle header".into());
    }
    if declared_len > neoflash_core::MAX_PROJECT_JSON_BYTES as u64 {
        return Err("document exceeds the 128 MiB encoded JSON limit".into());
    }
    let cap = usize::try_from(declared_len)
        .unwrap_or(neoflash_core::MAX_PROJECT_JSON_BYTES)
        .min(neoflash_core::MAX_PROJECT_JSON_BYTES);
    let mut bytes = Vec::with_capacity(cap);
    bytes.extend_from_slice(&prefix[..prefix_len]);
    let rem_cap = (neoflash_core::MAX_PROJECT_JSON_BYTES - prefix_len) as u64 + 1;
    file.take(rem_cap)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > neoflash_core::MAX_PROJECT_JSON_BYTES {
        return Err("document exceeds the 128 MiB encoded JSON limit".into());
    }
    let raw = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    Project::from_json(raw).map_err(|e| e.to_string())
}

pub struct Editor {
    pub project: Project,
    pub history: History,
    pub path: Option<PathBuf>,
    /// Session-only identity of an imported source; independent of its converted Save As path.
    pub(crate) import_source: Option<PathBuf>,
    /// True when this session edits a standalone `.as` script file in a pure Script Editor tab (`T-412`).
    pub standalone_script: bool,
    pub saved_project: Option<Project>,
    pub(crate) edit_session: crate::symbol_edit::SymbolEditSession,
    /// Compatibility mirror for code being migrated to `edit_session`. Never infer
    /// isolated/in-place semantics from its length.
    pub editing: Vec<String>,
    pub frame: u32,
    pub layer: usize,
    pub selection: crate::selection::Selection,
    pub object_gesture: Option<crate::stage_selection::SelectionGesture>,
    /// NF2 cursor proposed for the stage this frame (`stage_cursor.rs`).
    pub(crate) stage_cursor: Option<neoflash_ui::cursors::Cursor>,
    /// NF2 cursor painted over the stage in the last frame, if any.
    pub(crate) shown_stage_cursor: Option<neoflash_ui::cursors::Cursor>,
    pub tool: Tool,
    pub(crate) authoring_tools: crate::authoring_tools::AuthoringToolsState,
    tablet_input: crate::tablet_input::TabletInput,
    pub playing: bool,
    pub(crate) authored_preview: Option<crate::preview::AuthoringPreview>,
    pub last_tick: Instant,
    pub accumulator: f64,
    pub onion: bool,
    pub onion_outlines: bool,
    pub onion_before: u32,
    pub onion_after: u32,
    pub onion_anchor: bool,
    pub anchored_range: (u32, u32),
    pub zoom: f32,
    pub pan: Vec2,
    pub shape_draft: Option<crate::stage_selection::ShapeDraft>,
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f64,
    pub(crate) property_edit: Option<crate::paint_controls::PropertyEdit>,
    pub(crate) authoring_modify: crate::authoring_modify::AuthoringModify,
    pub bottom: BottomTab,
    pub script_target: ScriptTarget,
    pub script_event: ObjectEvent,
    pub selected_source: Option<String>,
    pub code_feedback: Option<crate::sources::CodeFeedback>,
    pub(crate) debugger: crate::debugger::Debugger,
    pub(crate) actions_ui: crate::panels::actions_sidebar::ActionsUi,
    pub status: String,
    pub output: crate::output_pipeline::OutputPipeline,
    pub fonts: Fonts,
    pub font_filter: String,
    pub show_about: bool,
    pub confirm: Option<Pending>,
    pub allow_close: bool,
    pub timeline_cell: f32,
    pub timeline_key_drag: Option<(String, String, u32)>,
    pub(crate) timeline_edit: crate::timeline_edit::TimelineSelection,
    pub library_selected: Option<String>,
    pub(crate) library_panel: crate::library::LibraryPanel,
    pub(crate) graphic_drop_position: Option<egui::Pos2>,
    pub(crate) symbol_conversion: Option<crate::symbol_dialog::SymbolConversion>,
    pub(crate) document_settings: Option<crate::panels::document_settings::DocumentSettingsDraft>,
    pub(crate) export_lottie: Option<crate::panels::export_lottie::ExportLottieDraft>,
    pub(crate) last_lottie_report: Option<neoflash_export::LottieExportReport>,
    pub(crate) export_video: Option<crate::panels::export_video::ExportVideoDraft>,
    pub(crate) last_video_report: Option<neoflash_export::VideoExportReport>,
    /// Open Preferences dialog draft (`panels/preferences.rs`).
    pub(crate) preferences: Option<crate::panels::preferences::PreferencesDraft>,
    pub(crate) edit_envelope: Option<crate::panels::edit_envelope::EditEnvelopeDraft>,
    pub(crate) project_panel: crate::panels::project::ProjectPanelState,
    pub(crate) compiler_errors: crate::panels::compiler_errors::CompilerErrorsState,
    pub(crate) code_snippets: crate::panels::code_snippets::CodeSnippetsState,
    pub(crate) movie_explorer: crate::panels::movie_explorer::MovieExplorerState,
    pub(crate) find_replace: Option<crate::panels::find_replace::FindReplaceState>,
    pub(crate) test_movie_preview_open: bool,
    pub(crate) confirm_revert: bool,
    pub(crate) panels_hidden: bool,
    pub enable_simple_buttons: bool,
    pub live_script_preview: bool,
    pub(crate) hovered_simple_button: Option<Id>,
    pub(crate) pressed_simple_button: Option<Id>,
    pub(crate) simple_button_capturing: bool,
    pub(crate) hovered_live_preview_target: Option<Vec<Id>>,
    pub(crate) pressed_live_preview_target: Option<Vec<Id>>,
    pub(crate) live_preview_capturing: bool,
    pub resource_panel: crate::resources::ResourcePanel,
    pub bitmap_textures: crate::bitmaps::BitmapTextures,
    pub video_textures: crate::video_cache::VideoTextures,
    pub vector: crate::vector::VectorState,
    pub clipboard: crate::clipboard::ClipboardState,
    pub workspace: WorkspacePrefs,
    pub(crate) publish_controller: crate::publish_controller::PublishController,
    pub(crate) updater: crate::updater::UpdaterController,
    pub(crate) beta_feedback: crate::panels::beta_feedback::BetaFeedbackState,
    pub(crate) publish_settings: Option<crate::panels::publish_settings::PublishSettingsDraft>,
    pub detached: crate::detached::DetachedState,
    pub(crate) stage_view: crate::stage_view::StageView,
    pub(crate) timeline_motion: crate::timeline_motion::MotionState,
    pub(crate) timeline_shape_hints: crate::timeline_shape_hints::ShapeHintsState,
    pub(crate) timeline_thumbnails: crate::timeline_preview::ThumbnailCache,
    /// Transient per-symbol soloed layer IDs (`R-045`); never saved in `.nflash` or Undo.
    pub(crate) solo_layers: std::collections::HashMap<Id, std::collections::BTreeSet<Id>>,
    /// Native preview of timeline sounds (Control ▸ Mute sounds).
    pub(crate) timeline_audio: crate::timeline_sound::TimelineAudio,
    /// Native preview synchronization state for timeline video spans (`T-167`).
    pub(crate) timeline_video: crate::preview::TimelineVideoPreview,
    /// Autosave timer and crash-recovery storage (`recovery.rs`).
    pub(crate) recovery: crate::recovery::RecoveryManager,
    /// Inactive document state; the active slot is None and lives in Editor.
    pub(crate) sessions: Vec<Option<crate::document_sessions::DocumentSession>>,
    pub(crate) active_session: usize,
    pub(crate) close_origin: Option<usize>,
    pub(crate) quit_origin_session: Option<usize>,
    pub(crate) quit_approved: Vec<usize>,
    pub(crate) close_targets: Vec<usize>,
    pub(crate) close_quit: bool,
    pub(crate) workspace_dialog: Option<crate::workspace_dialogs::WorkspaceDialog>,
    pub(crate) notifications: crate::notifications::NotificationCenter,
    pub(crate) bitmap_trace: crate::bitmap_trace::BitmapTraceController,
    pub(crate) stage_text_edit: crate::stage_text_edit::StageTextEditState,
    pub(crate) scale9_guide_drag: Option<crate::bitmaps::Scale9GuideDrag>,
    pub(crate) ai_manager: crate::ai_connections::AiConnectionManager,
    pub(crate) ai_connection_modal: Option<crate::panels::ai_dialogs::AiConnectionModal>,
    pub(crate) ai_session: crate::panels::ai_dialogs::AiDocumentSession,
    pub(crate) shared_frame_clipboard: Option<crate::timeline_edit::SharedFrameClipboard>,
    pub(crate) accessibility_bridge: crate::accessibility_bridge::AccessibilityBridge,
    pub(crate) persist_workspace_requested: bool,
    pub(crate) dirty_cache: std::cell::Cell<crate::document_sessions::DirtyCacheState>,
    dirty_ui_pass: u64,
    next_dirty_ui_pass: u64,
    last_workspace_snapshot: Option<Box<WorkspacePrefs>>,
    last_viewport_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    New,
    Demo,
    Open,
    OpenPath(PathBuf),
    Import,
    CloseDocument,
    CloseSession(usize),
    CloseSessions,
    Close,
}

impl Editor {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        crate::clipboard::enable_native_rich_clipboard(&cc.egui_ctx);
        let mut s = Self::from_project(&cc.egui_ctx, Project::demo());
        let is_screenshot = std::env::vars_os().any(|(k, _)| {
            k.to_str()
                .is_some_and(|name| name.starts_with("NEOFLASH_SCREENSHOT"))
        });
        // Screenshots ignore saved layouts so captures are reproducible (see shell::capture).
        let storage = if std::env::var_os("NEOFLASH_SCREENSHOT").is_some() {
            None
        } else {
            cc.storage
        };
        s.workspace =
            WorkspacePrefs::load_for_viewport(storage, cc.egui_ctx.viewport_rect().size().into());
        let env_open = std::env::var_os("NEOFLASH_OPEN");
        let mut arg_open = crate::macos_menu::startup_file_args();
        crate::macos_menu::drain_startup_open_files(&mut arg_open);
        if env_open.is_some() || !arg_open.is_empty() || !is_screenshot {
            s.sessions.clear();
        }
        // `NEOFLASH_OPEN=path.nflash` opens a document at launch (screenshots of real documents).
        if let Some(path) = env_open.clone() {
            s.open_path(path.into(), &cc.egui_ctx);
        }
        if let Err(error) = crate::native_quit::install(&cc.egui_ctx) {
            s.message(error);
        }
        crate::native_windows::install();
        if let Err(error) = crate::macos_menu::install(&cc.egui_ctx) {
            s.message(error);
        }
        crate::macos_menu::drain_startup_open_files(&mut arg_open);
        s.tablet_input = crate::tablet_input::TabletInput::install();
        if let Some(template) = crate::shell::screenshot_workspace() {
            s.workspace.apply_template(template);
            s.bottom = s.workspace.active_bottom;
        }
        if let Some(theme) = crate::shell::screenshot_theme() {
            s.workspace.theme = theme;
        }
        s.workspace.theme.apply(&cc.egui_ctx);
        s.apply_editor_preferences(&cc.egui_ctx);
        s.bottom = s.workspace.active_bottom;
        s.stage_view.prefs = s.workspace.view_defaults.clone().unwrap_or_default();
        // Screenshot aid (editor-view-settings-open.png): open the View settings popover.
        s.stage_view.settings_open =
            std::env::var_os("NEOFLASH_SCREENSHOT_VIEW_SETTINGS").is_some();
        if !arg_open.is_empty() {
            if !is_screenshot
                && s.workspace.general.on_launch == crate::workspace::OnLaunch::LastDocument
            {
                let _ = s.restore_session_tabs_on_launch(&cc.egui_ctx);
            }
            for path in arg_open {
                s.open_path(path, &cc.egui_ctx);
            }
        } else if env_open.is_none() && !is_screenshot {
            s.apply_on_launch(&cc.egui_ctx);
        }
        if !is_screenshot {
            s.ai_manager = crate::ai_connections::AiConnectionManager::system_default();
            let rec_dir = crate::recovery::RecoveryManager::default_recovery_dir();
            s.propagate_recovery_dir(rec_dir);
            s.check_recovery_on_launch();
        }
        s.apply_screenshot_boot(&cc.egui_ctx, is_screenshot);
        s
    }
    pub(crate) fn from_project(ctx: &egui::Context, project: Project) -> Self {
        crate::theme::initialize_editor_style(ctx);
        WorkspacePrefs::default().theme.apply(ctx);
        let saved_project = Some(project.clone());
        let root = project.root.clone();
        let mut s = Self {
            project,
            history: History::new(150),
            path: None,
            import_source: None,
            standalone_script: false,
            saved_project,
            edit_session: crate::symbol_edit::SymbolEditSession::for_scene(
                root.clone(),
                0.75,
                Vec2::ZERO,
            ),
            editing: vec![root.clone()],
            frame: 0,
            layer: 0,
            selection: crate::selection::Selection {
                symbol: root,
                targets: vec![],
            },
            object_gesture: None,
            stage_cursor: None,
            shown_stage_cursor: None,
            tool: Tool::Select,
            playing: false,
            authored_preview: None,
            last_tick: Instant::now(),
            accumulator: 0.,
            onion: false,
            onion_outlines: false,
            onion_before: 3,
            onion_after: 3,
            onion_anchor: false,
            anchored_range: (0, 6),
            zoom: 0.75,
            pan: Vec2::ZERO,
            shape_draft: None,
            fill: [50, 117, 184, 255],
            stroke: [30, 50, 70, 255],
            stroke_width: 1.0,
            property_edit: None,
            authoring_modify: crate::authoring_modify::AuthoringModify::default(),
            bottom: BottomTab::Properties,
            authoring_tools: crate::authoring_tools::AuthoringToolsState::default(),
            tablet_input: crate::tablet_input::TabletInput::default(),
            script_target: ScriptTarget::Frame,
            script_event: ObjectEvent::Click,
            selected_source: None,
            code_feedback: None,
            actions_ui: Default::default(),
            debugger: Default::default(),
            status: "Ready".into(),
            output: crate::output_pipeline::OutputPipeline::new(),
            fonts: Fonts::new(),
            font_filter: String::new(),
            show_about: false,
            confirm: None,
            allow_close: false,
            timeline_cell: neoflash_ui::metrics::FRAME,
            timeline_key_drag: None,
            timeline_edit: Default::default(),
            library_selected: None,
            symbol_conversion: None,
            document_settings: None,
            export_lottie: None,
            last_lottie_report: None,
            export_video: None,
            last_video_report: None,
            preferences: None,
            edit_envelope: None,
            project_panel: Default::default(),
            compiler_errors: Default::default(),
            code_snippets: Default::default(),
            movie_explorer: Default::default(),
            find_replace: None,
            test_movie_preview_open: false,
            confirm_revert: false,
            panels_hidden: false,
            enable_simple_buttons: false,
            live_script_preview: true,
            hovered_simple_button: None,
            pressed_simple_button: None,
            simple_button_capturing: false,
            hovered_live_preview_target: None,
            pressed_live_preview_target: None,
            live_preview_capturing: false,
            library_panel: crate::library::LibraryPanel::default(),
            graphic_drop_position: None,
            resource_panel: crate::resources::ResourcePanel::default(),
            bitmap_textures: crate::bitmaps::BitmapTextures::default(),
            video_textures: crate::video_cache::VideoTextures::default(),
            vector: crate::vector::VectorState::default(),
            clipboard: crate::clipboard::ClipboardState::default(),
            workspace: WorkspacePrefs::default(),
            publish_controller: Default::default(),
            updater: Default::default(),
            publish_settings: None,
            detached: crate::detached::DetachedState::default(),
            stage_view: Default::default(),
            timeline_motion: Default::default(),
            timeline_shape_hints: Default::default(),
            timeline_thumbnails: Default::default(),
            solo_layers: Default::default(),
            timeline_audio: Default::default(),
            timeline_video: Default::default(),
            recovery: Default::default(),
            sessions: vec![None],
            active_session: 0,
            close_origin: None,
            quit_origin_session: None,
            quit_approved: Vec::new(),
            close_targets: Vec::new(),
            close_quit: false,
            workspace_dialog: None,
            notifications: Default::default(),
            bitmap_trace: Default::default(),
            stage_text_edit: Default::default(),
            scale9_guide_drag: None,
            ai_manager: crate::ai_connections::AiConnectionManager::in_memory(),
            ai_connection_modal: None,
            ai_session: Default::default(),
            beta_feedback: Default::default(),
            shared_frame_clipboard: None,
            accessibility_bridge: Default::default(),
            persist_workspace_requested: false,
            dirty_cache: std::cell::Cell::new(crate::document_sessions::DirtyCacheState::default()),
            dirty_ui_pass: 0,
            next_dirty_ui_pass: 1,
            last_workspace_snapshot: None,
            last_viewport_title: None,
        };
        crate::panels::publish_settings::sync_document_publish_preferences(
            &mut s.workspace.publish,
            &s.project,
        );
        s.sync_output_document_id();
        s.fonts.install_ui(ctx);
        s.load_project_fonts(ctx);
        s.normalize_selection();
        s
    }

    pub(crate) fn propagate_recovery_dir(&mut self, dir: Option<PathBuf>) {
        let tracked_active = self.tracked_disk_path().map(Path::to_path_buf);
        self.recovery.retarget(tracked_active.as_deref());
        self.recovery.set_dir(dir.clone());
        for session in self.sessions.iter_mut().flatten() {
            let tracked = session.tracked_disk_path().map(Path::to_path_buf);
            session.recovery.retarget(tracked.as_deref());
            session.recovery.set_dir(dir.clone());
        }
    }

    /// Non-blocking poll for GitHub Releases update checks and verified downloads (`T-298`).
    pub(crate) fn poll_updater(&mut self, ctx: &egui::Context, allow_auto_check: bool) {
        let screenshot_mode = std::env::var_os("NEOFLASH_SCREENSHOT").is_some();
        let (update_check_done, _) = self.updater.poll(ctx, !cfg!(test) && !screenshot_mode);
        for item in self.updater.drain_output_events() {
            let loc = item
                .path
                .map(crate::output_pipeline::OutputLocation::for_file);
            self.push_output_coalesced(
                crate::output_pipeline::OutputEvent::new(
                    item.severity,
                    crate::output_pipeline::OutputSource::Updater,
                    item.message,
                )
                .with_optional_location(loc),
            );
        }
        if update_check_done {
            let now = crate::recovery::unix_now_secs();
            self.workspace.updates.last_check_unix = Some(now);
            if let Some(draft) = &mut self.preferences {
                draft.updates.last_check_unix = Some(now);
            }
        }
        if allow_auto_check && !cfg!(test) && !screenshot_mode {
            self.updater.maybe_auto_check(
                &self.workspace.updates,
                crate::recovery::unix_now_secs(),
                ctx,
            );
        }
    }

    /// Restore validated open document tabs from `WorkspacePrefs::general.session_files` in saved
    /// left-to-right order without duplicates or recovery loss (`T-231`).
    pub(crate) fn restore_session_tabs_on_launch(
        &mut self,
        ctx: &egui::Context,
    ) -> (usize, Option<String>) {
        let mut unique_recorded = Vec::new();
        for raw in &self.workspace.general.session_files {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.len() > 4096 {
                continue;
            }
            let canon = crate::recovery::canonical_or_normalized_path(Path::new(trimmed));
            if !unique_recorded.contains(&canon) {
                unique_recorded.push(canon);
            }
        }
        let existing_paths = self.workspace.general.existing_session_files();
        if existing_paths.is_empty() {
            return (0, None);
        }
        let target_active = self.workspace.general.active_session_file.clone();
        let preserved_recent = self.workspace.general.recent_files.clone();
        let mut opened = 0usize;
        let mut last_err = None;
        let mut failed_count = unique_recorded.len().saturating_sub(existing_paths.len());
        for path in existing_paths {
            if self.session_for_open_path(&path).is_some() {
                continue;
            }
            if self.try_open_path(path, ctx) {
                opened += 1;
            } else {
                failed_count += 1;
                last_err = Some(self.status.clone());
            }
        }
        self.workspace.general.recent_files = preserved_recent;
        if opened > 0 {
            if let Some(active_raw) = target_active
                && let Some(idx) = self.session_for_open_path(Path::new(&active_raw))
            {
                self.switch_session_tab(ctx, idx);
            }
            if let Some(active_path) = self.tracked_disk_path().map(Path::to_path_buf) {
                self.workspace.general.push_recent(&active_path);
            }
            self.sync_session_tabs_to_prefs();
            if failed_count > 0 {
                self.message(format!(
                    "Restored {opened} document tab{}; skipped {failed_count} missing or invalid tab{}.",
                    if opened == 1 { "" } else { "s" },
                    if failed_count == 1 { "" } else { "s" },
                ));
            } else if opened > 1 {
                self.message(format!("Restored {opened} document tabs."));
            }
        }
        (opened, last_err)
    }

    /// Apply `Preferences ▸ General ▸ On launch` when no startup document path was given.
    pub(crate) fn apply_on_launch(&mut self, ctx: &egui::Context) {
        // Hold `recovery.dir = None` while installing launch tabs so unprompted crash-recovery
        // candidates on disk are never cleared before `check_recovery_on_launch()` runs (`T-231`).
        let saved_dir = self.recovery.take_dir();
        let mut held_prompts = crate::recovery::RecoveryManager::default();
        self.recovery.move_prompts_to(&mut held_prompts);
        self.sessions.clear();
        match self.workspace.general.on_launch {
            crate::workspace::OnLaunch::NewDocument => {
                self.perform(Pending::New, ctx);
            }
            crate::workspace::OnLaunch::Example => {
                self.perform(Pending::Demo, ctx);
            }
            crate::workspace::OnLaunch::LastDocument => {
                let (restored, session_err) = self.restore_session_tabs_on_launch(ctx);
                if restored == 0 {
                    let mut opened_recent = false;
                    let mut last_err = session_err;
                    for path in self.workspace.general.existing_recent_files() {
                        if self.try_open_path(path, ctx) {
                            opened_recent = true;
                            break;
                        }
                        last_err = Some(self.status.clone());
                    }
                    if !opened_recent {
                        self.perform(Pending::Demo, ctx);
                        if let Some(err) = last_err {
                            self.message(format!("{err} Opened example document."));
                        } else {
                            self.message("No recent document found; opened example document.");
                        }
                    }
                }
            }
        }
        self.propagate_recovery_dir(saved_dir);
        held_prompts.move_prompts_to(&mut self.recovery);
    }
    /// Apply the Preferences stored in the workspace prefs: UI scale, undo levels, onion range, code font.
    pub(crate) fn apply_editor_preferences(&mut self, ctx: &egui::Context) {
        ctx.set_zoom_factor(self.workspace.ui_scale);
        self.history
            .set_capacity(self.workspace.undo_levels as usize);
        [self.onion_before, self.onion_after] = self.workspace.onion_range;
        self.fonts
            .ensure_loaded(ctx, &self.workspace.code_editor.font);
    }
    /// Switch the density profile (Window ▸ Density): sizes only, identical behaviour.
    pub(crate) fn set_density(
        &mut self,
        ctx: &egui::Context,
        density: crate::workspace::DensityPref,
    ) {
        self.workspace.set_density(density);
        self.apply_density(ctx);
    }
    /// Make NF2 metrics follow the current density (the Preferences draft while the dialog is open)
    /// and re-install egui spacing when it changed.
    pub(crate) fn apply_density(&self, ctx: &egui::Context) {
        let density = self
            .preferences
            .as_ref()
            .map_or(self.workspace.density, |draft| draft.density)
            .ui();
        neoflash_ui::metrics::set_density(density);
        if ctx.style().spacing.interact_size.y != neoflash_ui::metrics::control() {
            crate::theme::initialize_editor_style(ctx);
        }
    }
    pub(crate) fn set_theme(&mut self, ctx: &egui::Context, theme: ThemeMode) {
        if self.workspace.theme != theme {
            self.workspace.theme = theme;
            self.persist_workspace_requested = true;
        }
        theme.apply(ctx);
    }
    /// Inspector / Library priority (R-054): show `panel` and give its stacked dock group the larger
    /// share of the side dock. Layout only; the document is untouched.
    pub(crate) fn give_dock_priority(&mut self, panel: PanelId) {
        if self.is_standalone_script_session() {
            return;
        }
        self.show_panel(panel);
        self.workspace.dock.give_priority(panel);
        self.persist_workspace_requested = true;
    }
    fn reveal_panel(&mut self, panel: PanelId) {
        if self.is_standalone_script_session() {
            return;
        }
        self.panels_hidden = false;
        if self.workspace.show_panel(panel) {
            self.persist_workspace_requested = true;
        }
        self.bottom = self.workspace.active_bottom;
        if panel.detachable() && !self.workspace.is_panel_docked(panel) {
            self.detached.focus = Some(panel);
        }
    }
    pub(crate) fn show_panel(&mut self, panel: PanelId) {
        if self.is_standalone_script_session() {
            return;
        }
        if panel == PanelId::Actions {
            self.sync_actions_context_for_activation();
        }
        self.reveal_panel(panel);
    }
    pub(crate) fn open_frame_actions(&mut self) {
        if self.is_standalone_script_session() {
            return;
        }
        self.script_target = ScriptTarget::Frame;
        self.open_actions();
    }
    pub(crate) fn open_actions(&mut self) {
        if self.is_standalone_script_session() {
            return;
        }
        self.set_playing(false);
        if self.script_target == ScriptTarget::Object && !self.selected_editable() {
            self.script_target = ScriptTarget::Frame;
        }
        self.activate_current_actions_location();
        self.reveal_panel(PanelId::Actions);
        self.detached.focus_docked_actions = self.workspace.is_panel_docked(PanelId::Actions);
    }
    pub(crate) fn set_panel_visible(&mut self, panel: PanelId, visible: bool) {
        if self.is_standalone_script_session() {
            return;
        }
        if visible {
            self.panels_hidden = false;
            if panel == PanelId::Actions {
                self.sync_actions_context_for_activation();
            }
        }
        self.workspace.active_bottom = self.bottom;
        if self.workspace.set_panel_visible(panel, visible) {
            self.persist_workspace_requested = true;
        }
        self.bottom = self.workspace.active_bottom;
    }
    pub fn set_panels_hidden(&mut self, hidden: bool) {
        self.panels_hidden = hidden;
    }
    pub fn toggle_hide_panels(&mut self) {
        self.panels_hidden = !self.panels_hidden;
    }
    pub fn set_enable_simple_buttons(&mut self, enabled: bool) {
        self.enable_simple_buttons = enabled;
        if !enabled {
            self.clear_simple_button_state();
        }
    }
    pub fn set_live_script_preview(&mut self, enabled: bool) {
        if self.live_script_preview == enabled {
            return;
        }
        self.live_script_preview = enabled;
        // Recreate the complete authoring session (including nested clocks and SWF state) at the
        // current authored playhead. AuthoredPlayback also makes its direct toggle safe, while the
        // editor-level reset gives native previews a new epoch and silences the previous session.
        self.reset_authored_preview();
        self.sync_authored_preview();
    }
    pub(crate) fn clear_simple_button_state(&mut self) {
        self.hovered_simple_button = None;
        self.pressed_simple_button = None;
        self.simple_button_capturing = false;
    }
    pub(crate) fn clear_live_preview_pointer_state(&mut self) {
        self.hovered_live_preview_target = None;
        self.pressed_live_preview_target = None;
        self.live_preview_capturing = false;
    }
    /// Switch to a built-in workspace template (Window ▸ Workspace, top-bar select).
    pub(crate) fn apply_workspace_template(
        &mut self,
        ctx: &egui::Context,
        template: crate::workspace::WorkspaceTemplate,
    ) {
        self.panels_hidden = false;
        self.workspace.apply_template(template);
        if template == crate::workspace::WorkspaceTemplate::Classic {
            self.workspace
                .compact_default(ctx.viewport_rect().size().into());
        }
        self.persist_workspace_requested = true;
        self.reset_workspace_layout_state(ctx);
    }
    pub fn symbol_id(&self) -> &str {
        self.edit_session
            .current_symbol()
            .unwrap_or(&self.project.root)
    }
    pub fn symbol(&self) -> &Symbol {
        self.project
            .symbol(self.symbol_id())
            .expect("editor has a valid symbol")
    }
    pub fn symbol_mut(&mut self) -> &mut Symbol {
        let id = self.symbol_id().to_owned();
        self.project
            .symbol_mut(&id)
            .expect("editor has a valid symbol")
    }
    fn has_in_flight_project_edit(&self) -> bool {
        self.property_edit.is_some()
            || self.object_gesture.is_some()
            || self.vector.point_drag.is_some()
            || self.vector.live_deform_drag.is_some()
            || self.shape_draft.is_some()
            || self.timeline_key_drag.is_some()
            || self.timeline_edit.dragging()
            || self.scale9_guide_drag.is_some()
            || self.stage_view.drag.is_some()
            || self.timeline_motion.drag.is_some()
            || self.timeline_motion.curve_drag.is_some()
            || self.timeline_motion.sketch_recording
            || self.timeline_shape_hints.drag.is_some()
            || self.authoring_modify.distort_session.is_some()
            || self.authoring_modify.envelope_session.is_some()
            || self.stage_text_edit.coalesced_typing_active
            || self.stage_text_edit.ime_preedit.is_some()
            || (self.history.can_undo() && self.history.undo_len() == 0)
    }
    pub(crate) fn invalidate_dirty_cache(&self) {
        crate::document_sessions::DirtyCacheState::invalidate(&self.dirty_cache);
    }

    pub fn dirty(&self) -> bool {
        if !self.has_document() {
            return false;
        }
        if self
            .vector
            .draft
            .as_ref()
            .is_some_and(|d| d.anchors.len() >= 2)
        {
            return true;
        }
        let ui_pass_if_in_flight = if self.has_in_flight_project_edit() {
            (self.dirty_ui_pass > 0).then_some(self.dirty_ui_pass)
        } else {
            Some(0)
        };
        crate::document_sessions::compute_dirty_cached(
            &self.project,
            self.saved_project.as_ref(),
            &self.history,
            Some(self.symbol_id()),
            ui_pass_if_in_flight,
            &self.dirty_cache,
        )
    }
    pub fn message(&mut self, msg: impl Into<String>) {
        let _ = self.message_event(
            crate::output_pipeline::OutputSeverity::Info,
            crate::output_pipeline::OutputSource::System,
            msg,
            None,
        );
    }
    pub fn transact(&mut self, f: impl FnOnce(&mut Self)) {
        self.transact_named("Edit", f);
    }
    pub fn transact_named(&mut self, label: impl Into<String>, f: impl FnOnce(&mut Self)) {
        self.finish_property_edit();
        self.finish_selection_gesture();
        self.finish_stage_text_typing();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.finish_point_drag();
        self.history.begin_named(&self.project, label);
        f(self);
        self.project.prune_authoring_groups();
        self.commit_history();
    }
    pub fn normalize_selection(&mut self) {
        self.normalize_symbol_edit_session();
        self.cancel_stale_selection_gesture();
        if self.symbol().timeline.layers.is_empty() {
            self.symbol_mut().timeline.layers.push(Layer {
                properties: Default::default(),
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![],
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                }],
            });
        }
        self.layer = self
            .layer
            .min(self.symbol().timeline.layers.len().saturating_sub(1));
        let max_frame = if self.symbol().kind == SymbolKind::Button {
            self.symbol().timeline.frame_count.saturating_sub(1)
        } else {
            999_999
        };
        self.frame = self.frame.min(max_frame);
        self.normalize_targets();
        self.sync_stage_text_edit();
    }
    fn clamp_playhead_to_timeline(&mut self) {
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        self.frame = self.frame.min(last);
        if let Some(range) = &mut self.timeline_edit.range {
            range.end = range.end.min(last);
            range.start = range.start.min(range.end);
        }
    }
    pub fn undo(&mut self) {
        if !self.has_document() {
            return;
        }
        self.finish_edits();
        self.exit_stage_text_edit();
        self.scale9_guide_drag = None;
        self.authoring_modify.distort_session = None;
        self.authoring_modify.envelope_session = None;
        self.timeline_edit.cancel_gesture();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history.undo(&mut self.project);
        self.normalize_selection();
        self.clamp_playhead_to_timeline();
    }
    pub fn redo(&mut self) {
        if !self.has_document() {
            return;
        }
        self.finish_edits();
        self.exit_stage_text_edit();
        self.scale9_guide_drag = None;
        self.authoring_modify.distort_session = None;
        self.authoring_modify.envelope_session = None;
        self.timeline_edit.cancel_gesture();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history.redo(&mut self.project);
        self.normalize_selection();
        self.clamp_playhead_to_timeline();
    }
    pub fn step_history_to(&mut self, index: usize) -> bool {
        if !self.has_document() {
            return false;
        }
        self.finish_edits();
        self.exit_stage_text_edit();
        self.scale9_guide_drag = None;
        self.authoring_modify.distort_session = None;
        self.authoring_modify.envelope_session = None;
        self.timeline_edit.cancel_gesture();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        let moved = self.history.step_to(&mut self.project, index);
        self.normalize_selection();
        self.clamp_playhead_to_timeline();
        moved
    }
    pub fn clear_history(&mut self) {
        if !self.has_document() {
            return;
        }
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history = History::new(self.workspace.undo_levels as usize);
    }
    pub fn layer_editable(&self) -> bool {
        self.symbol()
            .timeline
            .layers
            .get(self.layer)
            .is_some_and(|layer| {
                !layer_effectively_locked(&self.symbol().timeline, layer)
                    && layer.properties.role != LayerRole::Folder
                    && layer_effectively_visible(&self.symbol().timeline, layer)
                    && self.is_layer_id_effectively_soloed(&layer.id)
            })
    }
    pub fn make_key(&mut self, blank: bool) {
        self.finish_edits();
        let target_layer_ids: Vec<Id> = {
            let timeline = &self.symbol().timeline;
            let candidates: Vec<Id> = match self.frame_range() {
                Some(r) if r.layers.len() > 1 => r.layers,
                _ => timeline
                    .layers
                    .get(self.layer)
                    .map(|l| vec![l.id.clone()])
                    .unwrap_or_default(),
            };
            candidates
                .into_iter()
                .filter(|id| {
                    timeline.layers.iter().any(|layer| {
                        &layer.id == id
                            && !layer_effectively_locked(timeline, layer)
                            && layer.properties.role != LayerRole::Folder
                            && layer_effectively_visible(timeline, layer)
                            && self.is_layer_id_effectively_soloed(&layer.id)
                    })
                })
                .collect()
        };
        if target_layer_ids.is_empty() {
            return;
        }
        let label = if blank { "Blank keyframe" } else { "Keyframe" };
        self.transact_named(label, |s| {
            let f = s.frame;
            for id in &target_layer_ids {
                s.ensure_key_at(id, f, blank);
                // F6 inside a motion tween keys every property at this frame (◆).
                if !blank
                    && let Some(layer) = s
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| &l.id == id)
                    && let Some(key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|key| key.frame == f && key.keyed_properties.is_some())
                {
                    key.motion_keys = Some(
                        key.elements
                            .iter()
                            .map(|element| MotionPropertyKey {
                                element_id: element.id.clone(),
                                properties: TweenProperty::ALL.to_vec(),
                                roving: false,
                            })
                            .collect(),
                    );
                    key.sync_motion_key_union();
                }
            }
        });
    }
    pub fn ensure_key(&mut self, blank: bool) {
        let id = self.symbol().timeline.layers[self.layer].id.clone();
        self.ensure_key_for(&id, blank);
    }
    pub fn add_element(&mut self, kind: ElementKind, x: f64, y: f64) {
        self.add_element_transformed(
            kind,
            Transform {
                x,
                y,
                ..Default::default()
            },
        );
    }
    pub(crate) fn add_element_transformed(&mut self, kind: ElementKind, transform: Transform) {
        if !self.layer_editable() {
            self.message("Select an unlocked, visible layer first.");
            return;
        }
        if let ElementKind::Instance { symbol_id, .. } = &kind
            && reaches(
                &self.project,
                symbol_id,
                self.symbol_id(),
                &mut std::collections::HashSet::new(),
            )
        {
            self.message("Cannot place this symbol here: it would create a recursive symbol.");
            return;
        }
        let step_label = match &kind {
            ElementKind::Rectangle { .. } => "Rectangle",
            ElementKind::Ellipse { .. } => "Oval",
            ElementKind::Text { .. } | ElementKind::TextBox { .. } => "Text",
            ElementKind::Instance { .. } => "Place symbol",
            ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => "Path",
            ElementKind::Bitmap { .. } => "Bitmap",
            ElementKind::Video { .. } => "Video",
        };
        self.transact_named(step_label, |s| {
            s.ensure_key(false);
            let id = new_id("object");
            let name = match &kind {
                ElementKind::Rectangle { .. } => "rectangle",
                ElementKind::Ellipse { .. } => "oval",
                ElementKind::Text { .. } | ElementKind::TextBox { .. } => "text",
                ElementKind::Instance { .. } => "clip",
                ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => "path",
                ElementKind::Bitmap { .. } => "bitmap",
                ElementKind::Video { .. } => "video",
            };
            let name = format!("{name}_{}", id.rsplit('-').next().unwrap_or("1"));
            let mut element = Element {
                accessibility: None,
                id: id.clone(),
                name,
                transform,
                kind,
                scripts: vec![],
                ..Element::default()
            };
            if s.apply_drawing_gradients_to_element(&mut element)
                || matches!(element.kind, ElementKind::PaintedPath { .. })
            {
                s.project.format_version = s.project.format_version.max(5);
            }
            let layer = s.layer;
            let frame = s.frame;
            let merge_mode = !s.workspace.drawing.object_drawing
                && matches!(
                    element.kind,
                    ElementKind::Rectangle { .. }
                        | ElementKind::Ellipse { .. }
                        | ElementKind::Path { .. }
                        | ElementKind::PaintedPath { .. }
                );
            if merge_mode {
                let brush_mode = s.authoring_tools.brush_paint_mode;
                let inside_seed = s.authoring_tools.brush_inside_seed.take();
                let selected_ids: Vec<Id> = s
                    .selection
                    .targets
                    .iter()
                    .map(|t| t.element.clone())
                    .collect();
                let use_brush_mode =
                    s.tool == Tool::Brush && brush_mode != BrushPaintMode::PaintNormal;
                let created_ids = crate::stage_selection::edit_ungrouped_merge_shapes(
                    &mut s.symbol_mut().timeline.layers[layer],
                    frame,
                    |elements| {
                        if use_brush_mode {
                            apply_brush_stroke_with_mode(
                                elements,
                                element,
                                brush_mode,
                                &selected_ids,
                                inside_seed,
                            )
                        } else {
                            apply_merge_shape_to_keyframe(elements, element)
                        }
                    },
                )
                .expect("drawing creates its destination keyframe before merging");
                let layer_id = s.symbol().timeline.layers[layer].id.clone();
                let targets = created_ids
                    .into_iter()
                    .map(|element| crate::selection::SelectionTarget {
                        layer: layer_id.clone(),
                        element,
                    })
                    .collect();
                s.set_selection(targets);
            } else {
                s.symbol_mut().timeline.layers[layer]
                    .key_at_mut(frame)
                    .unwrap()
                    .elements
                    .push(element);
                s.select_single_id(id);
            }
        });
        // Creating content selects it for inspection, but the chosen authoring
        // tool stays active for the next gesture until the user changes it.
    }
    pub fn convert_to_symbol(&mut self) {
        self.begin_symbol_conversion();
    }
    #[cfg(test)]
    pub fn enter_symbol(&mut self, id: String) {
        self.enter_symbol_isolated(id);
    }
    pub fn load_project_fonts(&mut self, ctx: &egui::Context) {
        let families: Vec<(String, bool, bool)> = self
            .project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .filter_map(|e| match &e.kind {
                ElementKind::Text { font_family, .. } => Some((font_family.clone(), false, false)),
                ElementKind::TextBox { format, .. } => {
                    Some((format.font.clone(), format.bold, format.italic))
                }
                _ => None,
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        for (family, bold, italic) in families {
            if !seen.insert((family.clone(), bold, italic)) {
                continue;
            }
            if let Err(e) = self.fonts.load_style(ctx, &family, bold, italic)
                && self.workspace.text_prefs.substitution_warning
            {
                self.status = e.clone();
                let _ = self.push_output_unique(crate::output_pipeline::OutputEvent::new(
                    crate::output_pipeline::OutputSeverity::Warning,
                    crate::output_pipeline::OutputSource::System,
                    e,
                ));
            }
        }
    }
    pub fn request(&mut self, pending: Pending, ctx: &egui::Context) {
        if self.confirm.is_some() {
            return;
        }
        if pending == Pending::Close {
            self.request_quit(ctx);
            return;
        }
        if pending == Pending::CloseDocument {
            self.close_session_tab(ctx, self.active_session);
            return;
        }
        // New/Open preserve the active document, including unfinished edits, in its own tab.
        self.perform(pending, ctx);
    }
    pub fn perform(&mut self, pending: Pending, ctx: &egui::Context) {
        match pending {
            Pending::New => {
                self.new_document_session(ctx);
            }
            Pending::Demo => {
                self.open_new_session_tab(ctx, Project::demo(), None);
            }
            Pending::CloseDocument => {
                self.close_session_tab(ctx, self.active_session);
            }
            Pending::Open | Pending::Import => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "NeoFlash, ActionScript, Flash, Figma or Vector/PSD document",
                        &[
                            "nflash", "json", "as", "fla", "xfl", "xml", "swf", "swc", "fig",
                            "figma", "svg", "psd", "psb", "pdf", "ai", "eps", "fxg",
                        ],
                    )
                    .add_filter("NeoFlash project", &["nflash", "json"])
                    .add_filter("ActionScript source (.as)", &["as"])
                    .add_filter(
                        "Flash source or movie",
                        &["fla", "xfl", "xml", "swf", "swc"],
                    )
                    .add_filter("Figma design", &["fig", "figma", "json"])
                    .add_filter(
                        "Vector & Photoshop documents",
                        &["svg", "psd", "psb", "pdf", "ai", "eps", "fxg"],
                    )
                    .pick_file()
                {
                    if self.session_for_open_path(&path).is_none()
                        && crate::panels::figma_import::is_figma_file_path(&path)
                    {
                        self.open_figma_import_dialog_for_open(ctx, path);
                    } else {
                        self.open_path(path, ctx);
                    }
                }
            }
            Pending::OpenPath(path) => {
                if self.session_for_open_path(&path).is_none()
                    && crate::panels::figma_import::is_figma_file_path(&path)
                {
                    self.open_figma_import_dialog_for_open(ctx, path);
                } else {
                    self.open_path(path, ctx);
                }
            }
            Pending::CloseSession(index) => self.finish_session_close(ctx, index),
            Pending::Close | Pending::CloseSessions => self.approve_quit_document(ctx),
        }
    }

    pub(crate) fn is_native_nflash_path(path: &std::path::Path) -> bool {
        !path.is_dir()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("nflash"))
    }

    pub(crate) fn is_legacy_flash_path(path: &std::path::Path) -> bool {
        if path.is_dir() {
            return true;
        }
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".figma.json"))
        {
            return true;
        }
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "fla" | "xfl" | "swf" | "swc" | "xml" | "fig" | "figma"
                )
            })
        {
            return true;
        }
        let mut magic = [0u8; 8];
        if let Ok(mut file) = std::fs::File::open(path)
            && let Ok(n) = std::io::Read::read(&mut file, &mut magic)
            && n >= 4
        {
            return matches!(&magic[..3], b"FWS" | b"CWS" | b"ZWS")
                || magic.starts_with(b"PK\x03\x04")
                || (n >= 8
                    && (magic == [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]
                        || &magic == b"fig-kiwi"
                        || &magic == b"fig-jam."));
        }
        false
    }

    pub fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        let _ = self.try_open_path(path, ctx);
    }

    pub(crate) fn try_open_path(&mut self, path: PathBuf, ctx: &egui::Context) -> bool {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        if let Some(index) = self.session_for_open_path(&path) {
            self.switch_session_tab(ctx, index);
            return true;
        }
        if Self::is_actionscript_source_path(&path) {
            if self.open_path_in_session(ctx, path) {
                self.message("ActionScript file opened.");
                return true;
            }
            return false;
        }
        if Self::is_non_native_graphic_document_path(&path) || Self::is_legacy_flash_path(&path) {
            return self.try_import_legacy(path, ctx);
        }
        if self.open_path_in_session(ctx, path) {
            if self.path.is_some() {
                self.message("Project opened.");
            }
            true
        } else {
            false
        }
    }

    pub fn can_revert(&self) -> bool {
        if !self.has_document() || self.tracked_disk_path().is_none() {
            return false;
        }
        if self.path.is_some() {
            self.dirty()
        } else {
            self.history.can_undo() || self.recovery.external_change().is_some()
        }
    }

    pub fn request_revert(&mut self) {
        if self.can_revert() {
            self.confirm_revert = true;
        }
    }

    pub fn revert_to_saved(&mut self, ctx: &egui::Context) -> bool {
        self.confirm_revert = false;
        if !self.has_document() {
            return false;
        }
        let Some(disk_path) = self.tracked_disk_path().map(PathBuf::from) else {
            return false;
        };
        let was_standalone_script =
            self.standalone_script || Self::is_actionscript_source_path(&disk_path);
        let canonical_path = self.path.clone();
        let preserved_import_source = self.import_source.clone();
        let loaded_disk = if !was_standalone_script
            && !Self::is_non_native_graphic_document_path(&disk_path)
            && !Self::is_legacy_flash_path(&disk_path)
            && file_has_nflash_bundle_prefix(&disk_path)
        {
            read_nflash_project_file(&disk_path).map(|project| (project, Vec::new()))
        } else {
            Self::load_project_from_disk_path(&disk_path)
        };
        match loaded_disk {
            Ok((project, diagnostics)) => {
                self.cancel_edits();
                for diagnostic in diagnostics {
                    let _ = self.message_event(
                        crate::output_pipeline::OutputSeverity::Warning,
                        crate::output_pipeline::OutputSource::Import,
                        diagnostic,
                        Some(crate::output_pipeline::OutputLocation::for_file(
                            disk_path.clone(),
                        )),
                    );
                }
                self.set_playing(false);
                self.timeline_audio.stop_all();
                self.timeline_video.follow_preview(None);
                self.exit_stage_text_edit();
                self.authoring_tools.reset_document_transient();
                self.authoring_modify = Default::default();
                self.project = project;
                let identity = self.clipboard.session.clone();
                self.clipboard = Default::default();
                self.clipboard.session = identity;
                self.resource_panel = Default::default();
                self.library_panel = Default::default();
                self.project_panel = Default::default();
                self.document_settings = None;
                self.export_lottie = None;
                if let Some(d) = &self.export_video {
                    d.cancel_worker();
                }
                self.export_video = None;
                self.find_replace = None;
                self.edit_envelope = None;
                self.clear_simple_button_state();
                self.clear_live_preview_pointer_state();
                self.cancel_bitmap_properties(ctx);
                self.cancel_sound_properties(ctx);
                self.cancel_layer_properties(ctx);
                self.close_symbols_popover(ctx);
                self.timeline_edit = Default::default();
                self.graphic_drop_position = None;
                self.bitmap_textures = Default::default();
                self.video_textures = Default::default();
                let root = self.project.root.clone();
                self.edit_session = crate::symbol_edit::SymbolEditSession::for_scene(
                    root.clone(),
                    self.zoom,
                    self.pan,
                );
                self.editing = vec![root];
                self.stage_view.zero = [0., 0.];
                self.frame = 0;
                self.layer = 0;
                self.clear_selection();
                self.workspace.general.push_recent(&disk_path);
                self.path = canonical_path;
                self.import_source = preserved_import_source;
                if was_standalone_script {
                    self.configure_active_standalone_script_state();
                } else {
                    self.standalone_script = false;
                }
                self.clear_and_retarget_recovery();
                self.sync_disk_mtime();
                self.history = History::new(self.workspace.undo_levels as usize);
                self.saved_project = if self.path.is_some() {
                    Some(self.project.clone())
                } else {
                    None
                };
                self.invalidate_dirty_cache();
                self.playing = false;
                self.sync_session_tabs_to_prefs();
                if was_standalone_script {
                    self.invalidate_include_dependents_across_sessions();
                    self.message("ActionScript file reverted to saved version.");
                } else {
                    self.load_project_fonts(ctx);
                    self.message("Project reverted to saved version.");
                }
                true
            }
            Err(e) => {
                let _ = self.message_event(
                    crate::output_pipeline::OutputSeverity::Error,
                    crate::output_pipeline::OutputSource::System,
                    format!("Revert failed: {e}"),
                    Some(crate::output_pipeline::OutputLocation::for_file(disk_path)),
                );
                false
            }
        }
    }

    #[cfg(test)]
    pub fn import_legacy(&mut self, path: PathBuf, ctx: &egui::Context) {
        let _ = self.try_import_legacy(path, ctx);
    }

    pub(crate) fn try_import_legacy(&mut self, path: PathBuf, ctx: &egui::Context) -> bool {
        use crate::output_pipeline::{OutputEvent, OutputLocation, OutputSeverity, OutputSource};

        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        if let Some(index) = self.session_for_open_path(&path) {
            self.switch_session_tab(ctx, index);
            return true;
        }
        let mut imported = false;
        let mut messages: Vec<(OutputSeverity, String)> = Vec::new();
        if Self::is_non_native_graphic_document_path(&path) {
            match Self::import_non_native_graphic_document(&path) {
                Ok((project, label, diags)) => {
                    messages.push((
                        OutputSeverity::Info,
                        format!("Imported source: {label} · {}", path.display()),
                    ));
                    messages.extend(diags);
                    self.workspace.general.push_recent(&path);
                    self.open_new_session_tab(ctx, project, None);
                    self.import_source =
                        Some(std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()));
                    self.saved_project = None;
                    self.invalidate_dirty_cache();
                    self.sync_disk_mtime();
                    self.sync_session_tabs_to_prefs();
                    messages.push((
                        OutputSeverity::Info,
                        format!(
                            "Imported {label}. Save as a NeoFlash project (.nflash) or Flash FLA (.fla)."
                        ),
                    ));
                    imported = true;
                }
                Err(e) => messages.push((OutputSeverity::Error, format!("Import failed: {e}"))),
            }
        } else {
            match neoflash_import::inspect(&path) {
                Ok(report) => {
                    messages.push((
                        OutputSeverity::Info,
                        format!("Legacy source: {:?} · {}", report.format, path.display()),
                    ));
                    for d in &report.diagnostics {
                        let severity = match d.severity {
                            neoflash_import::Severity::Info => OutputSeverity::Info,
                            neoflash_import::Severity::Warning => OutputSeverity::Warning,
                            neoflash_import::Severity::Error => OutputSeverity::Error,
                        };
                        messages.push((severity, format!("{}: {}", d.location, d.message)));
                    }
                    if report.can_import {
                        match neoflash_import::import_project(&path) {
                            Ok(project) => {
                                self.workspace.general.push_recent(&path);
                                self.open_new_session_tab(ctx, project, None);
                                self.import_source = Some(
                                    std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()),
                                );
                                self.saved_project = None;
                                self.invalidate_dirty_cache();
                                self.sync_disk_mtime();
                                self.sync_session_tabs_to_prefs();
                                let label = match report.format {
                                    neoflash_import::ImportFormat::Swf => "SWF movie",
                                    neoflash_import::ImportFormat::Swc => "SWC library",
                                    neoflash_import::ImportFormat::LegacyBinaryFla => {
                                        "binary FLA document"
                                    }
                                    neoflash_import::ImportFormat::XflArchive
                                    | neoflash_import::ImportFormat::XflDirectory => "XFL document",
                                    neoflash_import::ImportFormat::Figma => "Figma composition",
                                    neoflash_import::ImportFormat::Unknown => "Flash document",
                                };
                                messages.push((
                                    OutputSeverity::Info,
                                    format!(
                                        "Imported {label}. Save as a NeoFlash project (.nflash) or Flash FLA (.fla)."
                                    ),
                                ));
                                if report.format == neoflash_import::ImportFormat::Swf
                                    && let Ok(swf_bytes) = std::fs::read(&path)
                                {
                                    let previous_frame = self.frame;
                                    let previous_playing = self.playing;
                                    if let Ok(play_res) =
                                        self.play_swf_bytes_in_editor(&swf_bytes, 1)
                                    {
                                        self.frame = previous_frame;
                                        self.playing = previous_playing;
                                        let _ = self.seek_swf_preview(previous_frame);
                                        messages.push((
                                            OutputSeverity::Info,
                                            format!(
                                                "SWF bytecode runtime attached ({:?} VM, SWF v{}, {} frames): live display-list state is projected onto Stage without mutating the authored document.",
                                                play_res.vm_kind,
                                                play_res.swf_version,
                                                play_res.total_frames
                                            ),
                                        ));
                                    }
                                }
                                imported = true;
                            }
                            Err(e) => {
                                if report.format == neoflash_import::ImportFormat::Swf
                                    && let Ok(swf_bytes) = std::fs::read(&path)
                                    && self
                                        .attach_standalone_swf_preview_bytes(
                                            &swf_bytes,
                                            Some(&path),
                                            Some(&e.to_string()),
                                        )
                                        .is_ok()
                                {
                                    // Fallback message and frame-0 traces are already routed to Output.
                                } else {
                                    messages.push((OutputSeverity::Error, e.to_string()));
                                }
                            }
                        }
                    } else if report.format == neoflash_import::ImportFormat::Swf
                        && let Ok(swf_bytes) = std::fs::read(&path)
                    {
                        let first_err = report
                            .diagnostics
                            .iter()
                            .find(|d| d.severity == neoflash_import::Severity::Error)
                            .map(|d| format!("{}: {}", d.code, d.message));
                        if self
                            .attach_standalone_swf_preview_bytes(
                                &swf_bytes,
                                Some(&path),
                                first_err.as_deref(),
                            )
                            .is_err()
                        {
                            messages.push((
                                OutputSeverity::Error,
                                "Import stopped: this SWF could not be represented or executed safely."
                                    .into(),
                            ));
                        }
                    } else {
                        messages.push((
                            OutputSeverity::Error,
                            "Import stopped: this source contains features the current importer cannot preserve.".into(),
                        ));
                    }
                }
                Err(e) => messages.push((OutputSeverity::Error, format!("Import failed: {e}"))),
            }
        }
        // Successful imports own their diagnostics. A failed import stays on the current tab.
        let file_loc = OutputLocation::for_file(path);
        let mut seen_in_batch = std::collections::HashSet::new();
        for (severity, message) in messages {
            self.status = message.clone();
            if seen_in_batch.insert((severity, message.clone())) {
                self.push_output_coalesced(
                    OutputEvent::new(severity, OutputSource::Import, message)
                        .with_location(file_loc.clone()),
                );
            }
        }
        self.show_panel(PanelId::Output);
        imported
    }

    /// Open a `.swf` movie into an editable document tab when static timeline reconstruction
    /// succeeds, or attach a bounded `SwfBytecodePlayer` Stage fallback preview when static
    /// reconstruction fails — leaving the active authored document and Undo history untouched (`T-471`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn open_swf_with_bounded_preview_fallback(
        &mut self,
        path: &Path,
        ctx: &egui::Context,
    ) -> Result<neoflash_import::SwfPlaybackResult, String> {
        let report = neoflash_import::inspect(path).map_err(|e| e.to_string())?;
        if report.format != neoflash_import::ImportFormat::Swf {
            return Err(format!(
                "expected SWF source, found {:?} ({})",
                report.format,
                path.display()
            ));
        }
        let swf_bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if report.can_import && self.try_import_legacy(path.to_path_buf(), ctx) {
            let mut probe = neoflash_import::SwfBytecodePlayer::from_bytes(&swf_bytes)
                .map_err(|e| e.to_string())?;
            let probe_res = probe.run_frames(1);
            let _ = self.seek_swf_preview(0);
            return Ok(probe_res);
        }
        let reason = report
            .diagnostics
            .iter()
            .find(|d| d.severity == neoflash_import::Severity::Error)
            .map(|d| format!("{}: {}", d.code, d.message));
        let res =
            self.attach_standalone_swf_preview_bytes(&swf_bytes, Some(path), reason.as_deref())?;
        self.show_panel(PanelId::Output);
        Ok(res)
    }

    pub(crate) fn default_save_dialog_target(&self) -> (Option<PathBuf>, String) {
        fn clean_stem(raw: &str) -> String {
            let filtered: String = raw
                .chars()
                .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
                .collect();
            let trimmed = filtered.trim();
            if trimmed.is_empty() {
                "Untitled".into()
            } else {
                trimmed.to_owned()
            }
        }

        if let Some(path) = &self.path {
            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf);
            if Self::is_native_nflash_path(path)
                && let Some(name) = path.file_name().map(|s| s.to_string_lossy().into_owned())
            {
                return (dir, name);
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.project.name.clone());
            return (dir, format!("{}.nflash", clean_stem(&stem)));
        }

        if let Some(src) = &self.import_source {
            let anchor = if src
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("DOMDocument.xml"))
                && let Some(parent) = src.parent()
            {
                parent
            } else {
                src.as_path()
            };
            let dir = anchor
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf);
            let raw_name = anchor
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let stem = if raw_name.len() > ".figma.json".len()
                && raw_name
                    .get(raw_name.len() - ".figma.json".len()..)
                    .is_some_and(|suf| suf.eq_ignore_ascii_case(".figma.json"))
            {
                raw_name[..raw_name.len() - ".figma.json".len()].to_owned()
            } else {
                anchor
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.project.name.clone())
            };
            return (dir, format!("{}.nflash", clean_stem(&stem)));
        }

        (None, format!("{}.nflash", clean_stem(&self.project.name)))
    }

    #[cfg(test)]
    pub fn save(&mut self, save_as: bool) -> bool {
        self.save_with_ctx(save_as, None)
    }

    pub(crate) fn save_with_ctx(&mut self, save_as: bool, ctx: Option<&egui::Context>) -> bool {
        if !self.has_document() {
            return false;
        }
        if self.is_standalone_script_session() {
            let current_path = self.path.clone();
            let needs_save_as = save_as
                || !current_path
                    .as_deref()
                    .is_some_and(Self::is_actionscript_source_path);
            if !needs_save_as && !self.guard_save_on_external_conflict(current_path.as_deref()) {
                return false;
            }
            let path = if needs_save_as {
                let (default_dir, default_name) = self.default_standalone_script_save_target();
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("ActionScript source", &["as"])
                    .set_file_name(&default_name);
                if let Some(dir) = default_dir {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file()
            } else {
                current_path
            };
            let Some(path) = path else {
                return false;
            };
            return self.save_standalone_script_to_path(path);
        }
        let current_path = self.path.clone();
        let needs_save_as = save_as
            || !current_path
                .as_deref()
                .is_some_and(Self::is_native_nflash_path);
        if !needs_save_as && !self.guard_save_on_external_conflict(current_path.as_deref()) {
            return false;
        }
        if needs_save_as {
            let (default_dir, default_name) = self.default_save_dialog_target();
            let project_lang = self
                .project
                .with_resolved_script_language()
                .effective_script_language();
            let Some(choice) = crate::save_as_panel::prompt_save_as_dialog(
                default_dir.as_deref(),
                &default_name,
                project_lang,
            ) else {
                return false;
            };
            if let Some(conversion) = choice.conversion {
                return self.save_as_converted_with_ctx(choice.path, conversion, ctx);
            }
            return self.save_to_path(choice.path);
        }
        let Some(path) = current_path else {
            return false;
        };
        self.save_to_path(path)
    }

    /// Deterministically converts all project scripts to `target` (`AS2 <-> AS3`, `AS2/AS3 -> JS`),
    /// writes the converted copy to `path`, logs a full conversion report to `Output`, and opens
    /// `.nflash` copies in a new session tab without discarding unsaved edits on the source tab (`T-455`).
    #[allow(dead_code)]
    pub fn save_as_converted(
        &mut self,
        path: PathBuf,
        target: neoflash_script::ConversionTarget,
    ) -> bool {
        self.save_as_converted_with_ctx(path, target, None)
    }

    pub(crate) fn save_as_converted_with_ctx(
        &mut self,
        path: PathBuf,
        target: neoflash_script::ConversionTarget,
        ctx: Option<&egui::Context>,
    ) -> bool {
        if !self.has_document() || self.is_standalone_script_session() {
            return false;
        }
        let path = if !path.is_dir() && path.extension().is_none() {
            path.with_extension("nflash")
        } else {
            path
        };
        let fla_profile = self.resolve_fla_target_profile_for_path(&path);
        if fla_profile.is_none() && !Self::is_native_nflash_path(&path) {
            self.notify_with_output(
                neoflash_ui::components::ToastType::Error,
                "Save As (converted) failed: choose a .nflash, .fla, or .xfl destination.",
            );
            return false;
        }
        if self.save_destination_in_use(&path, false) {
            self.notify_with_output(
                neoflash_ui::components::ToastType::Error,
                "Save As (converted) failed: this destination would replace an open document or its source. Choose a different path.",
            );
            return false;
        }
        if fla_profile.is_none() && self.timeline_edit.dragging() {
            self.message("Finish the timeline drag before opening a converted copy.");
            return false;
        }
        self.finish_edits();

        let hydrated = self
            .hydrate_arbitrary_project_with_editor_context(&self.project)
            .0;
        let (mut converted_project, conv_report) =
            match neoflash_script::convert_project_scripts(&hydrated, target) {
                Ok(pair) => pair,
                Err(err) => {
                    self.notify_with_output(
                        neoflash_ui::components::ToastType::Error,
                        format!("Script conversion failed: {err}"),
                    );
                    self.show_panel(PanelId::Output);
                    return false;
                }
            };

        if self.workspace.code_editor.format_on_save {
            Self::format_project_scripts(
                &mut converted_project,
                self.workspace.code_editor.tab_size,
            );
        }
        crate::timeline_commands::upgrade_all_format_versions(&mut converted_project);

        let file_loc = crate::output_pipeline::OutputLocation::for_file(path.clone());
        if let Some(profile) = fla_profile {
            let resolved_fla = converted_project.with_resolved_script_language();
            match neoflash_export::export_fla_with_profile(
                resolved_fla.as_ref(),
                profile,
                &self.workspace.publish.fla_export_options,
                &path,
            ) {
                Ok(fla_report) => {
                    self.log_script_conversion_report(&conv_report, &file_loc);
                    for item in &fla_report.items {
                        if item.disposition != neoflash_export::FlaFeatureDisposition::Exact {
                            self.push_output_coalesced(
                                crate::output_pipeline::OutputEvent::new(
                                    crate::output_pipeline::OutputSeverity::Warning,
                                    crate::output_pipeline::OutputSource::Publish,
                                    format!(
                                        "[FLA {:?}] {} ({}): {}",
                                        item.disposition, item.code, item.location, item.message
                                    ),
                                )
                                .with_location(file_loc.clone()),
                            );
                        }
                    }
                    self.show_panel(PanelId::Output);
                    return true;
                }
                Err(err) => {
                    self.notify_with_output(
                        neoflash_ui::components::ToastType::Error,
                        format!("Save As FLA (converted) failed: {err}"),
                    );
                    return false;
                }
            }
        }

        let write_res = converted_project
            .to_nflash_bytes()
            .map_err(|e| e.to_string())
            .and_then(|bytes| atomic_save_file(&path, &bytes));
        if let Err(err) = write_res {
            self.notify_with_output(
                neoflash_ui::components::ToastType::Error,
                format!("Save As (converted) failed: {err}"),
            );
            return false;
        }

        self.workspace.general.push_recent(&path);
        let fallback_ctx;
        let effective_ctx = if let Some(c) = ctx {
            c
        } else {
            fallback_ctx = egui::Context::default();
            &fallback_ctx
        };
        // Choosing a converted copy in the unsaved-changes dialog keeps the source open.
        if self.confirm.is_some() || self.close_origin.is_some() {
            self.cancel_confirmation(effective_ctx);
        }
        self.open_new_session_tab(effective_ctx, converted_project, Some(path));
        self.log_script_conversion_report(&conv_report, &file_loc);
        if !conv_report.issues.is_empty() {
            self.show_panel(PanelId::Output);
        }
        true
    }

    fn log_script_conversion_report(
        &mut self,
        report: &neoflash_script::ConversionReport,
        file_loc: &crate::output_pipeline::OutputLocation,
    ) {
        self.push_output_coalesced(
            crate::output_pipeline::OutputEvent::new(
                crate::output_pipeline::OutputSeverity::Info,
                crate::output_pipeline::OutputSource::Publish,
                format!(
                    "Converted scripts {} -> {} ({} frame script(s), {} object handler(s), {} source file(s), {} auto-named instance(s), {} note(s)).",
                    report.source_language.display_label(),
                    report.target_language.display_label(),
                    report.converted_frame_scripts,
                    report.converted_object_scripts,
                    report.converted_source_units,
                    report.auto_named_instances,
                    report.issues.len(),
                ),
            )
            .with_location(file_loc.clone()),
        );
        for issue in &report.issues {
            let sev = match issue.kind {
                neoflash_script::ConversionIssueKind::ShimInjected => {
                    crate::output_pipeline::OutputSeverity::Info
                }
                neoflash_script::ConversionIssueKind::Approximated
                | neoflash_script::ConversionIssueKind::ManualAttention => {
                    crate::output_pipeline::OutputSeverity::Warning
                }
            };
            self.push_output_coalesced(
                crate::output_pipeline::OutputEvent::new(
                    sev,
                    crate::output_pipeline::OutputSource::Publish,
                    format!(
                        "[Convert {:?}] {}:{}:{}: {}",
                        issue.kind, issue.location, issue.line, issue.column, issue.message
                    ),
                )
                .with_location(file_loc.clone()),
            );
        }
    }

    pub fn resolve_fla_target_profile_for_path(
        &self,
        path: &std::path::Path,
    ) -> Option<neoflash_export::FlaTargetProfile> {
        use neoflash_export::FlaTargetProfile;
        let ext = path.extension().and_then(|s| s.to_str())?;
        if ext.eq_ignore_ascii_case("xfl") {
            return Some(FlaTargetProfile::UncompressedXfl);
        }
        if !ext.eq_ignore_ascii_case("fla") {
            return None;
        }
        if let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
        {
            for (suffixes, profile) in [
                (
                    &[".cs5_5", "-cs5_5", "_cs5_5", ".cs55", "-cs55", "_cs55"][..],
                    FlaTargetProfile::FlashCS5_5,
                ),
                (&[".cs6", "-cs6", "_cs6"][..], FlaTargetProfile::FlashCS6),
                (&[".cs5", "-cs5", "_cs5"][..], FlaTargetProfile::FlashCS5),
                (
                    &[".cs4", "-cs4", "_cs4"][..],
                    FlaTargetProfile::LegacyFlashCS4,
                ),
                (
                    &[".flash8", "-flash8", "_flash8", ".f8", "-f8", "_f8"][..],
                    FlaTargetProfile::LegacyFlash8,
                ),
                (
                    &[".animate", "-animate", "_animate"][..],
                    FlaTargetProfile::AnimateCC,
                ),
            ] {
                if suffixes.iter().any(|suf| stem.ends_with(suf)) {
                    return Some(profile);
                }
            }
        }
        Some(match self.workspace.publish.fla_target_profile {
            // A `.fla` target is a ZIP archive. Never reinterpret it as an
            // uncompressed XFL directory just because an older workspace
            // persisted that profile; `.xfl` above is the explicit directory
            // form and filename suffixes still select legacy archive profiles.
            FlaTargetProfile::UncompressedXfl => FlaTargetProfile::AnimateCC,
            profile => profile,
        })
    }

    pub(crate) fn resolve_confirmation_save(&mut self, ctx: &egui::Context, save_succeeded: bool) {
        // A derived export (for example Save As .fla) can succeed while the
        // canonical NeoFlash document remains dirty. Never treat that copy as
        // permission to close, replace or otherwise discard the native edits.
        if save_succeeded && !self.dirty() {
            self.continue_confirmation(ctx);
        } else {
            self.cancel_confirmation(ctx);
        }
    }

    fn format_project_scripts(project: &mut Project, tab_size: u8) {
        for unit in &mut project.sources {
            if let Some(formatted) =
                crate::code_tools::auto_format_with_tab_size(&unit.source, tab_size)
            {
                unit.source = formatted;
            }
        }
        for symbol in &mut project.symbols {
            for frame_script in &mut symbol.timeline.scripts {
                if let Some(formatted) = crate::code_tools::auto_format_with_tab_size(
                    &frame_script.script.source,
                    tab_size,
                ) {
                    frame_script.script.source = formatted;
                }
            }
            for layer in &mut symbol.timeline.layers {
                for keyframe in &mut layer.keyframes {
                    for element in &mut keyframe.elements {
                        for object_script in &mut element.scripts {
                            if let Some(formatted) = crate::code_tools::auto_format_with_tab_size(
                                &object_script.script.source,
                                tab_size,
                            ) {
                                object_script.script.source = formatted;
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn save_to_path(&mut self, path: PathBuf) -> bool {
        self.save_to_path_with_runtime(path, &Self::runtime_dir())
    }

    pub(crate) fn save_to_path_with_runtime(
        &mut self,
        path: PathBuf,
        runtime_dir: &std::path::Path,
    ) -> bool {
        if !self.has_document() {
            return false;
        }
        if self.is_standalone_script_session() {
            return self.save_standalone_script_to_path(path);
        }
        let path = if !path.is_dir() && path.extension().is_none() {
            path.with_extension("nflash")
        } else {
            path
        };
        let fla_profile = self.resolve_fla_target_profile_for_path(&path);
        if fla_profile.is_none() && !Self::is_native_nflash_path(&path) {
            let bad_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| path.display().to_string());
            self.notify_with_output(
                neoflash_ui::components::ToastType::Error,
                format!(
                    "Save failed: cannot overwrite non-native file “{bad_name}” with a NeoFlash project. Save As “.nflash” (or export via File ▸ Export)."
                ),
            );
            return false;
        }
        if self.save_destination_in_use(&path, true) {
            self.notify_with_output(neoflash_ui::components::ToastType::Error, "Save failed: this destination would replace an open document or its source. Choose a different path.");
            return false;
        }
        if !self.guard_save_on_external_conflict(Some(&path)) {
            return false;
        }
        self.finish_edits();
        let is_fla = fla_profile.is_some();
        let format_on_save = self.workspace.code_editor.format_on_save;
        let tab_size = self.workspace.code_editor.tab_size;
        let derived_project = if is_fla && format_on_save {
            let mut project = self.project.clone();
            Self::format_project_scripts(&mut project, tab_size);
            Some(project)
        } else {
            None
        };
        if format_on_save && !is_fla {
            self.history
                .begin_named(&self.project, "Format scripts on save");
            Self::format_project_scripts(&mut self.project, tab_size);
        }
        let result: Result<Option<neoflash_export::FlaCompatibilityReport>, String> =
            if let Some(profile) = fla_profile {
                let base_proj = derived_project.as_ref().unwrap_or(&self.project);
                let hydrated = self
                    .hydrate_arbitrary_project_with_editor_context(base_proj)
                    .0;
                let resolved_fla_project = hydrated.with_resolved_script_language();
                neoflash_export::export_fla_with_profile(
                    resolved_fla_project.as_ref(),
                    profile,
                    &self.workspace.publish.fla_export_options,
                    &path,
                )
                .map(Some)
                .map_err(|e| e.to_string())
            } else {
                crate::timeline_commands::upgrade_all_format_versions(&mut self.project);
                self.project
                    .to_nflash_bytes()
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| atomic_save_file(&path, &bytes))
                    .map(|()| None)
            };
        match result {
            Ok(fla_report) => {
                if format_on_save && !is_fla {
                    self.commit_history();
                }
                if let Some(report) = fla_report {
                    // FLA export can contain compatibility approximations, so it is a
                    // derived copy rather than the editor's canonical save target.
                    let file_loc = crate::output_pipeline::OutputLocation::for_file(path.clone());
                    let mut seen_fla_items = std::collections::HashSet::new();
                    for item in &report.items {
                        if item.disposition != neoflash_export::FlaFeatureDisposition::Exact {
                            let line = format!(
                                "[FLA {:?}] {} ({}): {}",
                                item.disposition, item.code, item.location, item.message
                            );
                            if seen_fla_items.insert(line.clone()) {
                                self.push_output_coalesced(
                                    crate::output_pipeline::OutputEvent::new(
                                        crate::output_pipeline::OutputSeverity::Warning,
                                        crate::output_pipeline::OutputSource::Publish,
                                        line,
                                    )
                                    .with_location(file_loc.clone()),
                                );
                            }
                        }
                    }
                    if self
                        .tracked_disk_path()
                        .is_some_and(|tracked| crate::document_sessions::same_path(tracked, &path))
                    {
                        self.sync_disk_mtime();
                    }
                    self.push_output_coalesced(
                        crate::output_pipeline::OutputEvent::new(
                            crate::output_pipeline::OutputSeverity::Info,
                            crate::output_pipeline::OutputSource::Publish,
                            format!(
                                "Exported derived FLA ({}): {}",
                                report.profile.label(),
                                path.display()
                            ),
                        )
                        .with_location(file_loc.clone()),
                    );
                    let _ = self.message_event(
                        crate::output_pipeline::OutputSeverity::Info,
                        crate::output_pipeline::OutputSource::Publish,
                        format!("Exported derived FLA ({}).", report.profile.label()),
                        Some(file_loc),
                    );
                } else {
                    self.workspace.general.push_recent(&path);
                    self.saved_project = Some(self.project.clone());
                    self.invalidate_dirty_cache();
                    self.path = Some(path);
                    self.clear_and_retarget_recovery();
                    self.sync_session_tabs_to_prefs();
                    self.message("Project saved.");
                    if self.workspace.publish.publish_on_save {
                        let settings = self.effective_publish_preferences();
                        let output = self.configured_publish_folder();
                        self.publish_enabled_targets_with_settings(
                            output.as_deref(),
                            runtime_dir,
                            &settings,
                            false,
                        );
                    }
                }
                true
            }
            Err(e) => {
                if format_on_save && !is_fla {
                    self.history.cancel(&mut self.project);
                }
                self.notify_with_output(
                    neoflash_ui::components::ToastType::Error,
                    format!("Save failed: {e}"),
                );
                false
            }
        }
    }
    pub fn runtime_dir() -> PathBuf {
        let exe = std::env::current_exe().ok();
        let development_dir = exe
            .as_deref()
            .and_then(|path| {
                path.ancestors().find_map(|ancestor| {
                    let candidate = ancestor.join("target/web-runtime");
                    if candidate.is_dir() || ancestor.join("web").is_dir() {
                        Some(candidate)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| {
                #[cfg(debug_assertions)]
                {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime")
                }
                #[cfg(not(debug_assertions))]
                {
                    PathBuf::from("target/web-runtime")
                }
            });
        Self::runtime_dir_for(
            std::env::var_os("NEOFLASH_RUNTIME_DIR").map(PathBuf::from),
            exe.as_deref(),
            development_dir,
        )
    }
    pub(crate) fn runtime_dir_for(
        override_dir: Option<PathBuf>,
        executable: Option<&std::path::Path>,
        development_dir: PathBuf,
    ) -> PathBuf {
        if let Some(path) = override_dir {
            return path;
        }
        if let Some(parent) = executable.and_then(std::path::Path::parent) {
            // macOS application bundles keep the runtime in Contents/Resources.
            #[cfg(target_os = "macos")]
            let bundle = parent.join("../Resources/runtime");
            #[cfg(target_os = "macos")]
            if bundle.is_dir() {
                return bundle;
            }
            // Windows and Linux portable archives keep it adjacent to the executable.
            let portable = parent.join("runtime");
            if portable.is_dir() {
                return portable;
            }
        }
        development_dir
    }
    pub(crate) fn configured_publish_folder(&self) -> Option<PathBuf> {
        let raw = self.workspace.publish.output_folder.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        if raw == "~" {
            if let Some(home) = std::env::var_os("HOME") {
                return Some(PathBuf::from(home));
            }
        } else if let Some(rest) = raw.strip_prefix("~/")
            && let Some(home) = std::env::var_os("HOME")
        {
            return Some(PathBuf::from(home).join(rest));
        }
        Some(PathBuf::from(raw))
    }

    pub fn insert_frame(&mut self) {
        if self.timeline_edit.focused && self.selected_frame_range().is_some() {
            self.insert_frame_range();
            return;
        }
        self.finish_edits();
        if !self.layer_editable() {
            return;
        }
        let count = self.symbol().timeline.frame_count;
        let layer = &self.symbol().timeline.layers[self.layer];
        let layer_id = layer.id.clone();
        let layers = self.expand_with_linked_layers(&[layer_id]);
        let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
        if self.frame >= layer_end {
            let target = self.frame;
            if self.apply_frame_edit(TimelineEdit::SetEnd {
                layers,
                end_frame: target.saturating_add(1),
            }) {
                self.frame = target;
            }
            return;
        }
        self.apply_frame_edit(TimelineEdit::Insert {
            layers,
            at: (self.frame + 1).min(count),
            count: 1,
            include_scripts: self.timeline_edit.include_scripts,
        });
    }
}

impl Editor {
    pub(crate) fn request_workspace_persist(&mut self) {
        self.workspace.sanitize();
        self.persist_workspace_requested = true;
    }

    pub(crate) fn flush_workspace_to_storage(&mut self, storage: &mut dyn eframe::Storage) {
        self.sync_session_tabs_to_prefs();
        self.workspace.active_bottom = self.bottom;
        self.workspace.save(storage);
        storage.flush();
        self.persist_workspace_requested = false;
    }
}

impl eframe::App for Editor {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.tablet_input.append_events(ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.flush_workspace_to_storage(storage);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn on_exit(&mut self) {
        self.set_playing(false);
        self.timeline_audio.stop_all();
        self.timeline_video.follow_preview(None);
        crate::preview::shutdown_shared_preview_server();
        if self.allow_close {
            self.clear_all_session_recovery();
        } else {
            let protected = self.recovery.pending_ids();
            if !self.dirty() {
                self.clear_active_recovery();
            }
            for session in self.sessions.iter_mut().flatten() {
                if !session.dirty() {
                    let _ = session.recovery.clear_active_except(&protected);
                }
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let pass_nr = self.next_dirty_ui_pass;
        self.next_dirty_ui_pass = self.next_dirty_ui_pass.wrapping_add(1).max(1);
        self.dirty_ui_pass = pass_nr;
        self.workspace.active_bottom = self.bottom;
        if self.last_workspace_snapshot.as_deref() != Some(&self.workspace) {
            self.last_workspace_snapshot = Some(Box::new(self.workspace.clone()));
        }
        self.apply_density(ctx);
        self.poll_preview_output(ctx);
        self.poll_debugger(ctx);
        self.poll_updater(ctx, true);
        self.poll_beta_feedback(ctx);
        let native_quit = crate::native_quit::take_requested();
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if (native_quit || close_requested) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit(ctx);
        }
        crate::macos_menu::drain_events(self, ctx);
        let now_secs = ctx.input(|i| i.time);
        if let Some(wait) = self.tick_autosave(now_secs, crate::recovery::unix_now_secs()) {
            ctx.request_repaint_after(wait);
        }
        self.shortcuts(ctx);
        let _ = self.handle_window_document_or_as_file_drop(ctx);
        let standalone_as = self.is_standalone_script_session();
        if !standalone_as {
            self.sync_authored_preview();
            if self.playing {
                let elapsed = self.last_tick.elapsed().as_secs_f64().min(0.25);
                self.last_tick = Instant::now();
                self.accumulator += elapsed;
                let step = 1. / self.project.fps.max(1) as f64;
                while self.playing && self.accumulator >= step {
                    let before = self.frame;
                    self.tick_authored_preview();
                    let keep_1frame_live = self.symbol().timeline.frame_count <= 1
                        && !(self.editing_scene()
                            && self.timeline_edit.play_all_scenes
                            && self.project.scene_ids().len() > 1)
                        && self.live_script_preview
                        && crate::preview::symbol_has_continuous_preview_activity(
                            &self.project,
                            self.symbol_id(),
                        );
                    if !keep_1frame_live {
                        self.after_playback_tick(before);
                    }
                    self.accumulator -= step;
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(8));
            }
            self.pump_timeline_video();
            self.pump_timeline_audio();
            if !self.playing && self.timeline_video.has_playing_events() {
                ctx.request_repaint_after(std::time::Duration::from_millis(8));
            }
        }
        self.normalize_selection();
        if standalone_as {
            crate::native_windows::set_panel_window_levels(
                true,
                true,
                std::collections::HashSet::new(),
                std::collections::HashSet::new(),
            );
        } else {
            self.sync_closed_detached_panels(ctx);
        }
        self.menu(ctx);
        self.status_bar(ctx);
        if !standalone_as {
            self.bitmap_textures.begin_frame(&self.project);
            let project_base_dir = self
                .tracked_disk_path()
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf);
            self.video_textures.begin_frame(
                &self.project,
                project_base_dir.as_deref(),
                self.history.revision(),
            );
            self.prepare_docked_panel_focus(ctx);
        }
        if self.has_document() {
            if standalone_as {
                self.document_bar(ctx);
                self.standalone_script_editor_central(ctx);
            } else {
                if !self.panels_hidden {
                    self.dock_grid(ctx);
                }
                self.document_bar(ctx);
                self.stage(ctx);
            }
        } else {
            self.no_document_view(ctx);
        }
        self.draw_dialogs(ctx);
        crate::panels::productivity_tools::show_productivity_modals(self, ctx);
        self.workspace_dialog(ctx);
        self.customize_tools_dialog(ctx);
        self.trace_bitmap_dialog(ctx);
        self.symbol_creation_dialog(ctx);
        if self.has_document() && !standalone_as {
            self.detached_panels(ctx);
        }
        if self.property_edit.is_some() {
            self.end_property_interaction(ctx);
        }
        self.test_movie_preview_window(ctx);
        crate::panels::ui_studio::show_studio_window(ctx);
        if !standalone_as {
            self.update_publish_estimate(ctx);
        }
        self.notification_host(
            ctx,
            (self.has_document() && !standalone_as).then_some(self.stage_view.canvas),
        );
        self.reconcile_stage_cursor_after_overlays(ctx);
        crate::native_windows::configure_panels();
        let win_state = crate::macos_menu::sync(self, ctx);
        if self.last_viewport_title.as_deref() != Some(win_state.title.as_str()) {
            self.last_viewport_title = Some(win_state.title.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(win_state.title));
        }
        self.workspace.active_bottom = self.bottom;
        if self.last_workspace_snapshot.as_deref() != Some(&self.workspace) {
            self.last_workspace_snapshot = Some(Box::new(self.workspace.clone()));
            self.persist_workspace_requested = true;
        }
        if self.persist_workspace_requested || self.allow_close || native_quit || close_requested {
            if let Some(storage) = frame.storage_mut() {
                self.flush_workspace_to_storage(storage);
            } else {
                self.persist_workspace_requested = false;
            }
        }
        crate::panels::dialog_helpers::record_end_of_frame_focus(ctx);
        crate::shell::capture(ctx, ctx.cumulative_pass_nr());
        self.dirty_ui_pass = 0;
    }
}

pub(crate) fn reaches(
    project: &Project,
    from: &str,
    to: &str,
    seen: &mut std::collections::HashSet<String>,
) -> bool {
    if from == to {
        return true;
    }
    if !seen.insert(from.into()) {
        return false;
    }
    project.symbol(from).is_some_and(|s| {
        s.timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .any(|e| {
                if let ElementKind::Instance { symbol_id, .. } = &e.kind {
                    reaches(project, symbol_id, to, seen)
                } else {
                    false
                }
            })
    })
}

#[cfg(test)]
mod portable_runtime_tests {
    use super::Editor;

    struct Fixture(std::path::PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn t289_portable_runtime_lookup_prefers_override_then_bundle_then_adjacent_then_development() {
        let root =
            std::env::temp_dir().join(format!("neoflash-runtime-lookup-{}", std::process::id()));
        let fixture = Fixture(root);
        let executable = fixture.0.join("bin").join("neoflash-editor");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"fixture").unwrap();
        let development = fixture.0.join("development-runtime");
        let explicit = fixture.0.join("explicit-runtime");

        assert_eq!(
            Editor::runtime_dir_for(None, Some(&executable), development.clone()),
            development
        );

        let adjacent = executable.parent().unwrap().join("runtime");
        std::fs::create_dir_all(&adjacent).unwrap();
        assert_eq!(
            Editor::runtime_dir_for(None, Some(&executable), development.clone()),
            adjacent
        );

        let bundle = executable.parent().unwrap().join("../Resources/runtime");
        std::fs::create_dir_all(&bundle).unwrap();
        let expected = if cfg!(target_os = "macos") {
            bundle
        } else {
            adjacent
        };
        assert_eq!(
            Editor::runtime_dir_for(None, Some(&executable), development.clone()),
            expected
        );
        assert_eq!(
            Editor::runtime_dir_for(Some(explicit.clone()), Some(&executable), development),
            explicit
        );
    }
}

#[cfg(test)]
#[path = "t471_large_doc_and_swf_stage_tests.rs"]
mod t471_large_doc_and_swf_stage_tests;
