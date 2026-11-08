use crate::fonts::Fonts;
use crate::theme::ThemeMode;
use crate::workspace::{PanelId, WorkspacePrefs};
use eframe::egui::{self, Key, Modifiers, Vec2};
use neoflash_core::*;
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
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

pub struct Editor {
    pub project: Project,
    pub history: History,
    pub path: Option<PathBuf>,
    /// Session-only identity of an imported source; independent of its converted Save As path.
    pub(crate) import_source: Option<PathBuf>,
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
    pub output: Vec<String>,
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
    pub(crate) confirm_revert: bool,
    pub(crate) panels_hidden: bool,
    pub enable_simple_buttons: bool,
    pub(crate) hovered_simple_button: Option<Id>,
    pub(crate) pressed_simple_button: Option<Id>,
    pub(crate) simple_button_capturing: bool,
    pub resource_panel: crate::resources::ResourcePanel,
    pub bitmap_textures: crate::bitmaps::BitmapTextures,
    pub vector: crate::vector::VectorState,
    pub clipboard: crate::clipboard::ClipboardState,
    pub workspace: WorkspacePrefs,
    pub(crate) publish_controller: crate::publish_controller::PublishController,
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
    /// Autosave timer and crash-recovery storage (`recovery.rs`).
    pub(crate) recovery: crate::recovery::RecoveryManager,
    /// Inactive document state; the active slot is None and lives in Editor.
    pub(crate) sessions: Vec<Option<crate::document_sessions::DocumentSession>>,
    pub(crate) active_session: usize,
    pub(crate) close_origin: Option<usize>,
    pub(crate) quit_approved: Vec<usize>,
    pub(crate) close_targets: Vec<usize>,
    pub(crate) close_quit: bool,
    pub(crate) workspace_dialog: Option<crate::workspace_dialogs::WorkspaceDialog>,
    pub(crate) notifications: crate::notifications::NotificationCenter,
    pub(crate) bitmap_trace: crate::bitmap_trace::BitmapTraceController,
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
        let arg_open = std::env::args_os().nth(1);
        if env_open.is_some() || arg_open.is_some() || !is_screenshot {
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
        if let Some(path) = arg_open {
            s.open_path(PathBuf::from(path), &cc.egui_ctx);
        } else if env_open.is_none() && !is_screenshot {
            s.apply_on_launch(&cc.egui_ctx);
        }
        if !is_screenshot {
            s.recovery.retarget(s.path.as_deref());
            s.recovery
                .set_dir(crate::recovery::RecoveryManager::default_recovery_dir());
            s.check_recovery_on_launch();
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_ROWS=short|compact` captures the Frame View row heights.
        if is_screenshot {
            // `NEOFLASH_SCREENSHOT_DENSITY=compact|classic` captures a density profile.
            match std::env::var("NEOFLASH_SCREENSHOT_DENSITY").as_deref() {
                Ok("compact") => s
                    .workspace
                    .set_density(crate::workspace::DensityPref::Compact),
                Ok("classic") => s
                    .workspace
                    .set_density(crate::workspace::DensityPref::ClassicFlash),
                _ => {}
            }
            // `NEOFLASH_SCREENSHOT_STAGEBAR=collapsed|editbar` captures a Stage toolbar state.
            match std::env::var("NEOFLASH_SCREENSHOT_STAGEBAR").as_deref() {
                Ok("collapsed") => {
                    s.workspace.stage_toolbar = crate::workspace::StageToolbarMode::Collapsed
                }
                // The collapsed pill expanded in place, as while the pointer is over it.
                Ok("hover") => {
                    s.workspace.stage_toolbar = crate::workspace::StageToolbarMode::Collapsed;
                    cc.egui_ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new("stage-toolbar-linger"), f64::MAX)
                    });
                }
                Ok("editbar") => {
                    s.workspace.stage_toolbar = crate::workspace::StageToolbarMode::EditBar
                }
                _ => {}
            }
            // `NEOFLASH_SCREENSHOT_PRIORITY=library` captures Library priority (R-054).
            if std::env::var("NEOFLASH_SCREENSHOT_PRIORITY").as_deref() == Ok("library") {
                s.workspace.dock.give_priority(PanelId::Library);
            }
            // `NEOFLASH_SCREENSHOT_COLOR=expanded` shows the full Color picker (R-055);
            // `NEOFLASH_SCREENSHOT_RECENT=1` fills the recent colours from the document palette.
            if std::env::var("NEOFLASH_SCREENSHOT_COLOR").as_deref() == Ok("expanded") {
                s.workspace.color_picker_expanded = true;
            }
            if std::env::var("NEOFLASH_SCREENSHOT_RECENT").as_deref() == Ok("1") {
                s.workspace.recent_colors = vec![
                    [0x0C, 0x10, 0x24, 255],
                    [0xF0, 0xBE, 0x91, 255],
                    [0xE5, 0x39, 0x35, 255],
                    [0x2E, 0xA8, 0x4F, 255],
                    [0x1E, 0x63, 0xD6, 255],
                    [0xFF, 0xD4, 0x00, 255],
                    [0xB0, 0x17, 0x7A, 255],
                    [0xFF, 0xFF, 0xFF, 255],
                ];
            }
            match std::env::var("NEOFLASH_SCREENSHOT_ROWS").as_deref() {
                Ok("short") => s.workspace.timeline.short_rows = true,
                Ok("compact") => s.workspace.timeline.compact_rows = true,
                _ => {}
            }
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_FRAME=<layer row>,<frame>` selects that frame
        // (zero-based), so captures show frame Properties (e.g. Properties ▸ Sound).
        if let Some((layer, frame)) =
            std::env::var("NEOFLASH_SCREENSHOT_FRAME")
                .ok()
                .and_then(|value| {
                    let (layer, frame) = value.split_once(',')?;
                    Some((
                        layer.trim().parse::<usize>().ok()?,
                        frame.trim().parse::<u32>().ok()?,
                    ))
                })
            && let Some(id) = s.symbol().timeline.layers.get(layer).map(|l| l.id.clone())
        {
            s.layer = layer;
            s.frame = frame;
            s.normalize_selection();
            s.select_frame_range(FrameRange {
                layers: vec![id],
                start: s.frame,
                end: s.frame,
            });
        }
        match std::env::var("NEOFLASH_SCREENSHOT_ONION").as_deref() {
            Ok("outlines") => {
                s.onion = true;
                s.onion_outlines = true;
            }
            Ok("solid" | "1" | "true") => {
                s.onion = true;
                s.onion_outlines = false;
            }
            _ => {}
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_SELECT=select|transform|subselect|brush|pencil`
        // selects every editable object on the current frame with that tool, so captures show the
        // NF2 selection boxes or the transform box.
        if let Ok(tool) = std::env::var("NEOFLASH_SCREENSHOT_SELECT") {
            let tool = match tool.as_str() {
                "transform" => Some(Tool::Transform),
                "subselect" => Some(Tool::Subselect),
                "select" => Some(Tool::Select),
                "brush" => Some(Tool::Brush),
                "pencil" => Some(Tool::Pencil),
                _ => None,
            };
            if let Some(tool) = tool {
                s.set_tool(tool);
                if matches!(tool, Tool::Brush | Tool::Pencil) {
                    cc.egui_ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::new("properties-tool-tab"), true)
                    });
                } else {
                    s.select_all();
                }
            }
        }
        if std::env::var("NEOFLASH_SCREENSHOT_SELECT").as_deref() == Ok("motion") {
            s.set_tool(Tool::Select);
            if let Some(layer) = s.symbol().timeline.layers.get(s.layer)
                && let Some(element) = sample_layer_in_at_fps(
                    &s.symbol().timeline,
                    layer,
                    s.frame,
                    f64::from(s.project.fps),
                )
                .first()
            {
                s.set_selection(vec![crate::selection::SelectionTarget {
                    layer: layer.id.clone(),
                    element: element.id.clone(),
                }]);
            }
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_PANELS=swatches,info,history,project` reveals real panels.
        if let Ok(list) = std::env::var("NEOFLASH_SCREENSHOT_PANELS") {
            for raw in list.split(',') {
                let panel = match raw.trim().to_ascii_lowercase().as_str() {
                    "history" => Some(PanelId::History),
                    "brushes" => Some(PanelId::Brushes),
                    "swatches" => Some(PanelId::Swatches),
                    "color" => Some(PanelId::Color),
                    "info" => Some(PanelId::Info),
                    "project" => Some(PanelId::Project),
                    "ease" => Some(PanelId::Ease),
                    "scene" => Some(PanelId::Scene),
                    "motion_editor" => Some(PanelId::MotionEditor),
                    "filters" => Some(PanelId::Filters),
                    "compiler_errors" => Some(PanelId::CompilerErrors),
                    "code_snippets" => Some(PanelId::CodeSnippets),
                    "movie_explorer" => Some(PanelId::MovieExplorer),
                    _ => None,
                };
                if let Some(panel) = panel {
                    s.show_panel(panel);
                }
            }
        }
        match std::env::var("NEOFLASH_SCREENSHOT_DIALOG").as_deref() {
            Ok("document_settings") => s.open_document_settings(),
            Ok("trace_bitmap") => {
                s.select_all();
                s.open_trace_bitmap(&cc.egui_ctx);
            }
            Ok("customize_tools") => s.open_customize_tools(&cc.egui_ctx),
            Ok("workspace_save") => {
                s.workspace_dialog = Some(crate::workspace_dialogs::WorkspaceDialog::Save {
                    name: "My workspace".into(),
                    focus: false,
                })
            }
            Ok("publish_settings") => s.open_publish_settings(),
            Ok("export_video") => s.open_export_video(None),
            Ok("export_audio") => s.open_export_audio(&cc.egui_ctx, None),
            Ok("sound_properties") => s.open_sound_properties(&cc.egui_ctx, None),
            Ok("edit_envelope") => s.open_edit_envelope(),
            Ok("find_replace") => s.open_find_replace(),
            Ok("paste_motion_special") => {
                s.copy_motion();
                s.open_paste_motion_special();
            }
            _ => {}
        }
        match std::env::var("NEOFLASH_SCREENSHOT_DOCUMENTS").as_deref() {
            Ok("empty") => s.request_close_documents(&cc.egui_ctx, false),
            Ok("tabs") => {
                s.project.name = "Website".into();
                s.new_document_session(&cc.egui_ctx);
                s.project.name = "Animation".into();
                s.new_document_session(&cc.egui_ctx);
                s.project.name = "Game".into();
                s.switch_session_tab(&cc.egui_ctx, 1);
            }
            _ => {}
        }
        if std::env::var_os("NEOFLASH_SCREENSHOT_TOAST").is_some() {
            s.notify(neoflash_ui::components::ToastType::Warning,
                "Partial stroke erasing on masked artwork is not available yet; no artwork was changed.");
        }
        // Screenshot aid: `NEOFLASH_SCREENSHOT_PREFERENCES=<page index>` opens Preferences on that page.
        if let Some(page) = std::env::var("NEOFLASH_SCREENSHOT_PREFERENCES")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            s.open_preferences(page);
        }
        s
    }
    pub(crate) fn from_project(ctx: &egui::Context, project: Project) -> Self {
        crate::theme::initialize_editor_style(ctx);
        WorkspacePrefs::default().theme.apply(ctx);
        let saved_project = Some(project.clone());
        let root = project.root.clone();
        let mut s = Self {
            project, history: History::new(150), path: None, import_source: None, saved_project,
            edit_session: crate::symbol_edit::SymbolEditSession::for_scene(root.clone(), 0.75, Vec2::ZERO),
            editing: vec![root.clone()], frame: 0, layer: 0, selection: crate::selection::Selection { symbol: root, targets: vec![] }, object_gesture: None, stage_cursor: None, shown_stage_cursor: None, tool: Tool::Select,
            playing: false, authored_preview: None, last_tick: Instant::now(), accumulator: 0.,
            onion: false, onion_outlines: false, onion_before: 3, onion_after: 3,
            onion_anchor: false, anchored_range: (0, 6),
            zoom: 0.75, pan: Vec2::ZERO, shape_draft: None,
            fill: [50, 117, 184, 255], stroke: [30, 50, 70, 255], stroke_width: 1.0,
            property_edit: None, authoring_modify: crate::authoring_modify::AuthoringModify::default(), bottom: BottomTab::Properties,
            authoring_tools: crate::authoring_tools::AuthoringToolsState::default(),
            tablet_input: crate::tablet_input::TabletInput::default(),
            script_target: ScriptTarget::Frame, script_event: ObjectEvent::Click,
            selected_source: None, code_feedback: None, actions_ui: Default::default(),
            debugger: Default::default(),
            status: "Ready · F5 frame · F6 keyframe · F7 blank keyframe · ⌘↵ Test Movie".into(),
            output: vec!["NeoFlash development preview. Editor playback shows authored animation; Test Movie executes scripts in the browser.".into()],
            fonts: Fonts::new(), font_filter: String::new(), show_about: false, confirm: None, allow_close: false,
            timeline_cell: neoflash_ui::metrics::FRAME, timeline_key_drag: None, timeline_edit: Default::default(), library_selected: None, symbol_conversion: None,
            document_settings: None, export_lottie: None, last_lottie_report: None, export_video: None, last_video_report: None, preferences: None, edit_envelope: None, project_panel: Default::default(),
            compiler_errors: Default::default(),
            code_snippets: Default::default(),
            movie_explorer: Default::default(),
            find_replace: None,
            confirm_revert: false,
            panels_hidden: false,
            enable_simple_buttons: false,
            hovered_simple_button: None,
            pressed_simple_button: None,
            simple_button_capturing: false,
            library_panel: crate::library::LibraryPanel::default(),
            graphic_drop_position: None,
            resource_panel: crate::resources::ResourcePanel::default(),
            bitmap_textures: crate::bitmaps::BitmapTextures::default(),
            vector: crate::vector::VectorState::default(),
            clipboard: crate::clipboard::ClipboardState::default(),
            workspace: WorkspacePrefs::default(),
            publish_controller: Default::default(),
            publish_settings: None,
            detached: crate::detached::DetachedState::default(),
            stage_view: Default::default(),
            timeline_motion: Default::default(),
            timeline_shape_hints: Default::default(),
            timeline_thumbnails: Default::default(),
            solo_layers: Default::default(),
            timeline_audio: Default::default(),
            recovery: Default::default(),
            sessions: vec![None],
            active_session: 0,
            close_origin: None,
            quit_approved: Vec::new(),
            close_targets: Vec::new(),
            close_quit: false,
            workspace_dialog: None,
            notifications: Default::default(),
            bitmap_trace: Default::default(),
        };
        s.fonts.install_ui(ctx);
        s.load_project_fonts(ctx);
        s.normalize_selection();
        s
    }

    /// Apply `Preferences ▸ General ▸ On launch` when no startup document path was given.
    pub(crate) fn apply_on_launch(&mut self, ctx: &egui::Context) {
        self.sessions.clear();
        match self.workspace.general.on_launch {
            crate::workspace::OnLaunch::NewDocument => {
                self.perform(Pending::New, ctx);
            }
            crate::workspace::OnLaunch::Example => {
                self.perform(Pending::Demo, ctx);
            }
            crate::workspace::OnLaunch::LastDocument => {
                if let Some(path) = self
                    .workspace
                    .general
                    .existing_recent_files()
                    .into_iter()
                    .next()
                {
                    if !self.try_open_path(path, ctx) {
                        let err = self.status.clone();
                        self.perform(Pending::Demo, ctx);
                        self.message(format!("{err} Opened example document."));
                    }
                } else {
                    self.perform(Pending::Demo, ctx);
                    self.message("No recent document found; opened example document.");
                }
            }
        }
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
        self.workspace.theme = theme;
        theme.apply(ctx);
    }
    /// Inspector / Library priority (R-054): show `panel` and give its stacked dock group the larger
    /// share of the side dock. Layout only; the document is untouched.
    pub(crate) fn give_dock_priority(&mut self, panel: PanelId) {
        self.show_panel(panel);
        self.workspace.dock.give_priority(panel);
    }
    pub(crate) fn show_panel(&mut self, panel: PanelId) {
        self.panels_hidden = false;
        self.workspace.show_panel(panel);
        self.bottom = self.workspace.active_bottom;
        if panel.detachable() && !self.workspace.is_panel_docked(panel) {
            self.detached.focus = Some(panel);
        }
    }
    pub(crate) fn open_frame_actions(&mut self) {
        self.script_target = ScriptTarget::Frame;
        self.open_actions();
    }
    pub(crate) fn open_actions(&mut self) {
        self.set_playing(false);
        if self.script_target == ScriptTarget::Object && !self.selected_editable() {
            self.script_target = ScriptTarget::Frame;
        }
        self.show_panel(PanelId::Actions);
        self.detached.focus_docked_actions = self.workspace.is_panel_docked(PanelId::Actions);
    }
    pub(crate) fn set_panel_visible(&mut self, panel: PanelId, visible: bool) {
        if visible {
            self.panels_hidden = false;
        }
        self.workspace.active_bottom = self.bottom;
        self.workspace.set_panel_visible(panel, visible);
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
    pub(crate) fn clear_simple_button_state(&mut self) {
        self.hovered_simple_button = None;
        self.pressed_simple_button = None;
        self.simple_button_capturing = false;
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
    pub fn dirty(&self) -> bool {
        if !self.has_document() {
            return false;
        }
        self.vector
            .draft
            .as_ref()
            .is_some_and(|d| d.anchors.len() >= 2)
            || self.saved_project.as_ref() != Some(&self.project)
    }
    pub(crate) fn has_modal_dialog(&self) -> bool {
        self.confirm.is_some()
            || self.workspace_dialog.is_some()
            || self.bitmap_trace.is_open()
            || self.symbol_conversion.is_some()
            || self.timeline_motion.paste_special_open
            || self.document_settings.is_some()
            || self.publish_settings.is_some()
            || self.export_lottie.is_some()
            || self.export_video.is_some()
            || self.preferences.is_some()
            || self.edit_envelope.is_some()
            || self.authoring_modify.scale_and_rotate_dialog.is_some()
            || self.find_replace.is_some()
            || self.confirm_revert
            || (self.recovery.has_prompt() && (!self.dirty() || !self.sessions.is_empty()))
    }
    pub(crate) fn has_modal_dialog_in(&self, ctx: &egui::Context) -> bool {
        self.has_modal_dialog()
            || self.customize_tools_open(ctx)
            || crate::panels::export_audio::has_export_audio_dialog(ctx)
            || crate::panels::export_image::has_export_image_dialog(ctx)
            || crate::panels::import_video::has_import_video_dialog(ctx)
            || crate::panels::stroke_style::has_stroke_style_dialog(ctx)
            || crate::panels::font_embedding::has_font_embedding_dialog(ctx)
            || crate::panels::sound_properties::has_sound_properties_dialog(ctx)
            || crate::panels::layer_properties::has_layer_properties_dialog(ctx)
    }
    pub fn message(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.output.push(self.status.clone());
    }
    pub fn transact(&mut self, f: impl FnOnce(&mut Self)) {
        self.transact_named("Edit", f);
    }
    pub fn transact_named(&mut self, label: impl Into<String>, f: impl FnOnce(&mut Self)) {
        self.finish_property_edit();
        self.finish_selection_gesture();
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
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history.undo(&mut self.project);
        self.normalize_selection();
        self.clamp_playhead_to_timeline();
    }
    pub fn redo(&mut self) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history.redo(&mut self.project);
        self.normalize_selection();
        self.clamp_playhead_to_timeline();
    }
    pub fn step_history_to(&mut self, index: usize) -> bool {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        let moved = self.history.step_to(&mut self.project, index);
        self.normalize_selection();
        self.clamp_playhead_to_timeline();
        moved
    }
    pub fn clear_history(&mut self) {
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
            })
    }
    pub fn make_key(&mut self, blank: bool) {
        self.finish_edits();
        if !self.layer_editable() {
            return;
        }
        let label = if blank { "Blank keyframe" } else { "Keyframe" };
        self.transact_named(label, |s| {
            let id = s.symbol().timeline.layers[s.layer].id.clone();
            s.ensure_key_at(&id, s.frame, blank);
            // F6 inside a motion tween keys every property at this frame (◆).
            let (l, f) = (s.layer, s.frame);
            if let Some(key) = s.symbol_mut().timeline.layers[l]
                .keyframes
                .iter_mut()
                .find(|key| key.frame == f && key.keyed_properties.is_some())
                && !blank
            {
                key.keyed_properties = Some(TweenProperty::ALL.to_vec());
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
            if s.apply_drawing_gradients_to_element(&mut element) {
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
        for (family, bold, italic) in families {
            if let Err(e) = self.fonts.load_style(ctx, &family, bold, italic)
                && self.workspace.text_prefs.substitution_warning
            {
                self.message(e);
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
            Pending::Open => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(
                        "NeoFlash or Flash project",
                        &["nflash", "json", "fla", "xfl", "swf"],
                    )
                    .add_filter("NeoFlash project", &["nflash", "json"])
                    .add_filter("Flash source or movie", &["fla", "xfl", "xml", "swf"])
                    .pick_file()
                {
                    self.open_path(path, ctx)
                }
            }
            Pending::OpenPath(path) => self.open_path(path, ctx),
            Pending::Import => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Flash source or movie", &["fla", "xfl", "xml", "swf"])
                    .pick_file()
                {
                    self.import_legacy(path, ctx);
                }
            }
            Pending::CloseSession(index) => self.finish_session_close(ctx, index),
            Pending::Close | Pending::CloseSessions => self.approve_quit_document(ctx),
        }
    }

    pub(crate) fn is_legacy_flash_path(path: &std::path::Path) -> bool {
        if path.is_dir() {
            return true;
        }
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "fla" | "xfl" | "swf" | "xml"
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
                || (n >= 8 && magic == [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
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
        if Self::is_legacy_flash_path(&path) {
            return self.try_import_legacy(path, ctx);
        }
        if self.open_path_in_session(ctx, path) {
            self.message("Project opened.");
            true
        } else {
            false
        }
    }

    pub fn can_revert(&self) -> bool {
        self.path.is_some() && self.dirty()
    }

    pub fn request_revert(&mut self) {
        self.finish_edits();
        if self.can_revert() {
            self.confirm_revert = true;
        }
    }

    pub fn revert_to_saved(&mut self, ctx: &egui::Context) -> bool {
        self.confirm_revert = false;
        let Some(path) = self.path.clone() else {
            return false;
        };
        self.cancel_edits();
        let loaded = if Self::is_legacy_flash_path(&path) {
            neoflash_import::inspect(&path)
                .map_err(|e| e.to_string())
                .and_then(|report| {
                    for diagnostic in &report.diagnostics {
                        self.message(format!(
                            "{} [{}]: {}",
                            diagnostic.location, diagnostic.code, diagnostic.message
                        ));
                    }
                    neoflash_import::import_project(&path).map_err(|e| e.to_string())
                })
        } else {
            std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| Project::from_json(&s).map_err(|e| e.to_string()))
        };
        match loaded {
            Ok(project) => {
                self.project = project;
                self.clipboard = Default::default();
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
                self.cancel_sound_properties(ctx);
                self.cancel_layer_properties(ctx);
                self.close_symbols_popover(ctx);
                self.timeline_edit = Default::default();
                self.graphic_drop_position = None;
                self.bitmap_textures = Default::default();
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
                self.workspace.general.push_recent(&path);
                self.path = Some(path);
                self.clear_and_retarget_recovery();
                self.history = History::new(self.workspace.undo_levels as usize);
                self.saved_project = Some(self.project.clone());
                self.playing = false;
                self.load_project_fonts(ctx);
                self.message("Project reverted to saved version.");
                true
            }
            Err(e) => {
                self.message(format!("Revert failed: {e}"));
                false
            }
        }
    }

    pub fn import_legacy(&mut self, path: PathBuf, ctx: &egui::Context) {
        let _ = self.try_import_legacy(path, ctx);
    }

    fn try_import_legacy(&mut self, path: PathBuf, ctx: &egui::Context) -> bool {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        if let Some(index) = self.session_for_open_path(&path) {
            self.switch_session_tab(ctx, index);
            return true;
        }
        let mut imported = false;
        let mut messages = Vec::new();
        match neoflash_import::inspect(&path) {
            Ok(report) => {
                messages.push(format!(
                    "Legacy source: {:?} · {}",
                    report.format,
                    path.display()
                ));
                for d in &report.diagnostics {
                    messages.push(format!("{}: {}", d.location, d.message));
                }
                if report.can_import {
                    match neoflash_import::import_project(&path) {
                        Ok(project) => {
                            self.workspace.general.push_recent(&path);
                            self.open_new_session_tab(ctx, project, None);
                            self.import_source =
                                Some(std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()));
                            self.saved_project = None;
                            let label = match report.format {
                                neoflash_import::ImportFormat::Swf => "SWF movie",
                                neoflash_import::ImportFormat::Swc => "SWC library",
                                neoflash_import::ImportFormat::LegacyBinaryFla => {
                                    "binary FLA document"
                                }
                                neoflash_import::ImportFormat::XflArchive
                                | neoflash_import::ImportFormat::XflDirectory => "XFL document",
                                neoflash_import::ImportFormat::Unknown => "Flash document",
                            };
                            messages.push(format!(
                                "Imported {label}. Save as a NeoFlash project (.nflash) or Flash FLA (.fla)."
                            ));
                            imported = true;
                        }
                        Err(e) => messages.push(e.to_string()),
                    }
                } else {
                    messages.push("Import stopped: this source contains features the current importer cannot preserve.".into());
                }
            }
            Err(e) => messages.push(format!("Import failed: {e}")),
        }
        // Successful imports own their diagnostics. A failed import stays on the current tab.
        for message in messages {
            self.message(message);
        }
        self.show_panel(PanelId::Output);
        imported
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        if !self.has_document() {
            return false;
        }
        let default_name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                let stem: String = self
                    .project
                    .name
                    .chars()
                    .filter(|c| !matches!(c, '/' | '\\' | ':' | '\0'))
                    .collect();
                let stem = stem.trim();
                if stem.is_empty() {
                    "Untitled.nflash".into()
                } else {
                    format!("{stem}.nflash")
                }
            });
        let path = if save_as || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("NeoFlash project", &["nflash"])
                .add_filter("Flash FLA (XFL archive)", &["fla"])
                .set_file_name(&default_name)
                .save_file()
        } else {
            self.path.clone()
        };
        let Some(path) = path else {
            return false;
        };
        self.save_to_path(path)
    }

    fn format_project_scripts_in_place(&mut self) {
        let tab_size = self.workspace.code_editor.tab_size;
        for unit in &mut self.project.sources {
            if let Some(formatted) =
                crate::code_tools::auto_format_with_tab_size(&unit.source, tab_size)
            {
                unit.source = formatted;
            }
        }
        for symbol in &mut self.project.symbols {
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
        if self
            .session_for_path(&path)
            .is_some_and(|index| index != self.active_session)
        {
            self.notify_with_output(neoflash_ui::components::ToastType::Error, "Save failed: this file is already open in another document. Choose a different path.");
            return false;
        }
        self.finish_edits();
        if self.workspace.code_editor.format_on_save {
            self.history
                .begin_named(&self.project, "Format scripts on save");
            self.format_project_scripts_in_place();
        }
        let is_fla = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fla"));
        let result = if is_fla {
            neoflash_import::export_fla_bytes(&self.project)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    let tmp = path.with_extension("fla.tmp");
                    std::fs::write(&tmp, bytes)
                        .and_then(|_| std::fs::rename(&tmp, &path))
                        .map_err(|e| e.to_string())
                })
        } else {
            self.project
                .to_json()
                .map_err(|e| e.to_string())
                .and_then(|json| {
                    let tmp = path.with_extension("nflash.tmp");
                    std::fs::write(&tmp, json)
                        .and_then(|_| std::fs::rename(&tmp, &path))
                        .map_err(|e| e.to_string())
                })
        };
        match result {
            Ok(()) => {
                if self.workspace.code_editor.format_on_save {
                    self.commit_history();
                }
                self.workspace.general.push_recent(&path);
                self.saved_project = Some(self.project.clone());
                self.path = Some(path);
                self.clear_and_retarget_recovery();
                self.message(if is_fla {
                    "FLA archive saved."
                } else {
                    "Project saved."
                });
                if !is_fla && self.workspace.publish.publish_on_save {
                    let settings = self.workspace.publish.clone();
                    if let Some(output) = self.configured_publish_folder() {
                        if settings.html5_enabled {
                            self.publish_html5_to(&output, runtime_dir);
                        }
                        if settings.video_enabled
                            || settings.default_target == crate::workspace::PublishTarget::VideoMp4
                        {
                            self.publish_video_with_settings(Some(&output), &settings);
                        }
                    } else if settings.video_file.is_some()
                        && (settings.video_enabled
                            || settings.default_target == crate::workspace::PublishTarget::VideoMp4)
                    {
                        self.publish_video_with_settings(None, &settings);
                    } else {
                        self.message(
                            "Publish on save skipped: set an Output folder in Preferences ▸ Publish.",
                        );
                    }
                }
                true
            }
            Err(e) => {
                if self.workspace.code_editor.format_on_save {
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
        if let Ok(path) = std::env::var("NEOFLASH_RUNTIME_DIR") {
            return path.into();
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let bundle = parent.join("../Resources/runtime");
            if bundle.exists() {
                return bundle;
            }
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/web-runtime")
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
    fn log_export_report(&mut self, project: &Project) {
        if let Ok(summary) = neoflash_export::compute_load_summary(project) {
            self.output.push(summary.summary_line());
        }
        if let Ok(report) = neoflash_export::format_size_report(project) {
            for line in report.lines() {
                if !line.trim().is_empty() {
                    self.output.push(line.to_string());
                }
            }
        }
    }

    pub(crate) fn publish_html5_to(
        &mut self,
        output: &std::path::Path,
        runtime_dir: &std::path::Path,
    ) -> bool {
        let settings = self.workspace.publish.clone();
        self.publish_html5_with_settings(output, runtime_dir, &settings)
    }

    /// Publish the same draft used by the estimate without committing workspace defaults.
    pub(crate) fn publish_html5_with_settings(
        &mut self,
        output: &std::path::Path,
        runtime_dir: &std::path::Path,
        settings: &crate::workspace::PublishPrefs,
    ) -> bool {
        self.finish_edits();
        let warnings = neoflash_export::export_warnings(&self.project);
        let options = match settings.export_options(&self.project) {
            Ok(options) => options,
            Err(error) => {
                self.message(format!("Publish failed: {error}"));
                self.show_panel(PanelId::Output);
                return false;
            }
        };
        match neoflash_export::export_project_with_options(
            &self.project,
            output,
            runtime_dir,
            &options,
        ) {
            Ok(index) => {
                self.message(format!("HTML5 exported: {}", index.display()));
                let project_snapshot = self.project.clone();
                self.log_export_report(&project_snapshot);
                for warning in warnings {
                    self.message(warning);
                }
                true
            }
            Err(e) => {
                self.message(format!("Export failed: {e}"));
                self.show_panel(PanelId::Output);
                false
            }
        }
    }
    pub fn export(&mut self, test: bool) {
        if !self.has_document() {
            return;
        }
        if test {
            if self.debugger.active() {
                self.debug_command(crate::debugger_protocol::DebugCommand::Restart);
                return;
            }
            let output = std::env::temp_dir()
                .join("neoflash-preview")
                .join(std::process::id().to_string());
            self.finish_edits();
            match neoflash_export::export_project(&self.project, &output, &Self::runtime_dir()) {
                Ok(index) => {
                    self.message(format!("HTML5 exported: {}", index.display()));
                    let project_snapshot = self.project.clone();
                    self.log_export_report(&project_snapshot);
                    for warning in neoflash_export::export_warnings(&self.project) {
                        self.message(warning);
                    }
                    self.show_panel(PanelId::Output);
                    if let Err(e) = std::process::Command::new("open").arg(&index).spawn() {
                        self.message(format!("Cannot open preview: {e}"));
                    }
                }
                Err(e) => {
                    self.message(format!("Export failed: {e}"));
                    self.show_panel(PanelId::Output);
                }
            }
            return;
        }
        let settings = self.workspace.publish.clone();
        let publish_video = settings.video_enabled
            || settings.default_target == crate::workspace::PublishTarget::VideoMp4;
        if publish_video && !settings.html5_enabled {
            self.publish_video_with_settings(None, &settings);
            return;
        }
        let output = if let Some(configured) = self.configured_publish_folder() {
            configured
        } else {
            let Some(base) = rfd::FileDialog::new()
                .set_title("Choose HTML5 output parent folder")
                .pick_folder()
            else {
                return;
            };
            base.join(format!(
                "neoflash-export-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ))
        };
        if settings.html5_enabled {
            self.publish_html5_to(&output, &Self::runtime_dir());
        }
        if publish_video {
            self.publish_video_with_settings(Some(&output), &settings);
        }
    }
    /// Export the document (or a specific `symbol_id`) to a Bodymovin / Lottie `.json` file.
    #[cfg(test)]
    pub fn export_lottie_to_path(
        &mut self,
        path: &std::path::Path,
        symbol_id: Option<&str>,
    ) -> Result<neoflash_export::LottieExportReport, String> {
        let options = neoflash_export::LottieOptions {
            symbol: symbol_id.map(str::to_owned),
            include_background: symbol_id.is_none(),
            ..neoflash_export::LottieOptions::default()
        };
        self.export_lottie_with_options_to_path(path, &options)
    }
    pub fn export_lottie_dialog(&mut self, symbol_id: Option<&str>) {
        self.open_export_lottie(symbol_id);
    }
    /// Single-scene copy of `self.project` rooted at the currently edited scene (`Control ▸ Test scene`).
    pub(crate) fn test_scene_project(&self) -> Project {
        let mut cloned = self.project.clone();
        let scene_id = self.current_scene().to_owned();
        cloned
            .symbols
            .retain(|s| s.id == scene_id || !self.project.is_scene(&s.id));
        cloned.root = scene_id.clone();
        cloned.scenes.clear();
        let doc_class = cloned.document_class.clone();
        if let Some(root_sym) = cloned.symbol_mut(&scene_id) {
            root_sym.folder.clear();
            root_sym.linkage_id = None;
            if doc_class.is_some()
                && root_sym.class_name.is_some()
                && root_sym.class_name != doc_class
            {
                root_sym.class_name = None;
            }
        }
        cloned
    }

    /// `Control ▸ Test scene` (`⌥⌘↵`): export and open only the currently edited scene in the browser.
    pub fn test_scene(&mut self) {
        self.finish_edits();
        let project = self.test_scene_project();
        let output = std::env::temp_dir()
            .join("neoflash-preview-scene")
            .join(std::process::id().to_string());
        match neoflash_export::export_project(&project, &output, &Self::runtime_dir()) {
            Ok(index) => {
                self.message(format!("HTML5 scene exported: {}", index.display()));
                self.log_export_report(&project);
                for warning in neoflash_export::export_warnings(&project) {
                    self.message(warning);
                }
                self.show_panel(PanelId::Output);
                if !cfg!(test)
                    && let Err(e) = std::process::Command::new("open").arg(&index).spawn()
                {
                    self.message(format!("Cannot open preview: {e}"));
                }
            }
            Err(e) => {
                self.message(format!("Export failed: {e}"));
                self.show_panel(PanelId::Output);
            }
        }
    }

    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        use crate::clipboard::ClipboardCommand;
        use crate::commands::{CommandId as C, modifiers_match};
        if self.has_modal_dialog_in(ctx)
            || crate::panels::symbols_popover::is_symbols_popover_open(ctx)
            || crate::panels::function_search::is_function_search_open(ctx)
        {
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        if self.document_shortcuts(ctx) {
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        if !self.has_document() {
            if self.take_command_shortcut(ctx, C::FileNew) {
                self.request(Pending::New, ctx);
            }
            if self.take_command_shortcut(ctx, C::FileOpen) {
                self.request(Pending::Open, ctx);
            }
            if self.take_command_shortcut(ctx, C::EditPreferences) {
                Self::open_planned(ctx, "Edit ▸ Preferences…");
            }
            crate::commands::clear_queued_commands(ctx);
            return;
        }
        self.menu_shortcuts(ctx);
        self.timeline_shortcuts(ctx);
        self.view_shortcuts(ctx);
        self.clipboard_events(ctx);

        // Global commands allowed even while a text field has focus (when bound to a non-typing chord).
        if self.take_command_shortcut(ctx, C::FileSaveAs) {
            self.save(true);
        }
        if self.take_command_shortcut(ctx, C::FileSave) {
            self.save(false);
        }
        if self.take_command_shortcut(ctx, C::FileOpen) {
            self.request(Pending::Open, ctx);
        }
        if self.take_command_shortcut(ctx, C::FileNew) {
            self.request(Pending::New, ctx);
        }
        if self.take_command_shortcut(ctx, C::ControlTestScene) {
            self.test_scene();
        }
        if self.take_command_shortcut(ctx, C::ControlTestMovie) {
            self.export(true);
        }
        if self.take_command_shortcut(ctx, C::WindowActions) {
            self.open_actions();
        }

        let typing = ctx.wants_keyboard_input();
        if !typing {
            if self.take_command_shortcut(ctx, C::EditRedo) {
                if self.history.can_redo() {
                    self.redo();
                } else if !self.selection.is_empty()
                    && !crate::commands::has_override(
                        &self.workspace.shortcuts,
                        C::ModifyRemoveTransform,
                    )
                {
                    self.reset_selection_transform(ctx);
                }
            }
            if self.take_command_shortcut(ctx, C::EditUndo) {
                self.undo();
            }
            if self.take_command_shortcut(ctx, C::EditCut) {
                self.request_clipboard(ctx, ClipboardCommand::Cut);
            }
            if self.take_command_shortcut(ctx, C::EditCopy) {
                self.request_clipboard(ctx, ClipboardCommand::Copy);
            }
            if self.take_command_shortcut(ctx, C::EditPasteInPlace) {
                self.request_clipboard(ctx, ClipboardCommand::PasteInPlace);
            }
            if self.take_command_shortcut(ctx, C::EditPaste) {
                self.request_clipboard(ctx, ClipboardCommand::Paste);
            }
            if self.take_command_shortcut(ctx, C::EditSelectAll) {
                self.select_all();
            }
            if self.take_command_shortcut(ctx, C::EditDuplicate) {
                self.duplicate();
            }
            if self.take_command_shortcut(ctx, C::InsertKeyframe) {
                self.make_key(false);
            }
            if self.take_command_shortcut(ctx, C::InsertBlankKeyframe) {
                self.make_key(true);
            }
            if self.take_command_shortcut(ctx, C::InsertFrame) {
                self.insert_frame();
            }
            if self.take_command_shortcut(ctx, C::InsertNewSymbol) {
                self.begin_new_symbol();
            }
            if self.take_command_shortcut(ctx, C::ModifyConvertToSymbol) {
                self.convert_to_symbol();
            }
            if self.take_command_shortcut(ctx, C::EditClear) {
                self.delete_selected();
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                let finish_open_pen = self
                    .vector
                    .draft
                    .as_ref()
                    .is_some_and(|draft| !draft.pencil && draft.anchors.len() >= 2);
                let captured = self.object_gesture.is_some()
                    || self.timeline_motion.drag.is_some()
                    || self.timeline_motion.curve_drag.is_some()
                    || self.timeline_shape_hints.drag.is_some()
                    || self.authoring_tools.is_drawing()
                    || self.shape_draft.is_some()
                    || self.vector.point_drag.is_some()
                    || self.vector.draft.is_some();
                if finish_open_pen {
                    // Escape ends the current open contour and leaves Pen active, so the next
                    // click begins a second path instead of silently losing the first one.
                    self.finish_path(false);
                } else {
                    self.cancel_edits();
                }
                if !captured {
                    self.clear_selection();
                }
            }
            let finish_draft = self.vector.draft.is_some()
                && ctx.input_mut(|i| {
                    let before = i.events.len();
                    let mut found = false;
                    i.events.retain(|event| {
                        let hit = !found
                            && matches!(
                                event,
                                egui::Event::Key {
                                    key: Key::Enter,
                                    pressed: true,
                                    modifiers,
                                    ..
                                } if modifiers_match(*modifiers, Modifiers::NONE)
                            );
                        found |= hit;
                        !hit
                    });
                    i.events.len() != before
                });
            if finish_draft {
                self.finish_path(false);
            } else if self.take_command_shortcut(ctx, C::ControlPlay) {
                self.set_playing(!self.playing);
            }
            for tool in [
                Tool::Select,
                Tool::Transform,
                Tool::Rectangle,
                Tool::Ellipse,
                Tool::PolyStar,
                Tool::Text,
                Tool::Pen,
                Tool::Pencil,
                Tool::Subselect,
                Tool::Line,
                Tool::Brush,
                Tool::Bucket,
                Tool::Ink,
                Tool::Eyedropper,
                Tool::Eraser,
                Tool::Lasso,
                Tool::Hand,
                Tool::Zoom,
                Tool::Gradient,
            ] {
                if self.take_command_shortcut(ctx, C::for_tool(tool)) {
                    self.set_tool(tool);
                }
            }
            if self.take_command_shortcut(ctx, C::ToolObjectDrawing) {
                self.workspace.drawing.object_drawing = !self.workspace.drawing.object_drawing;
            }
            if !self.timeline_edit.focused
                && !self.selection.is_empty()
                && self.object_gesture.is_none()
                && !self.authoring_tools.is_drawing()
                && self.shape_draft.is_none()
                && self.vector.draft.is_none()
            {
                let mut nudge = [0.0, 0.0];
                ctx.input_mut(|i| {
                    if !i.modifiers.command && !i.modifiers.alt && !i.modifiers.ctrl {
                        let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                        let m = if i.modifiers.shift {
                            Modifiers::SHIFT
                        } else {
                            Modifiers::NONE
                        };
                        if i.consume_key(m, Key::ArrowLeft) {
                            nudge[0] -= step;
                        }
                        if i.consume_key(m, Key::ArrowRight) {
                            nudge[0] += step;
                        }
                        if i.consume_key(m, Key::ArrowUp) {
                            nudge[1] -= step;
                        }
                        if i.consume_key(m, Key::ArrowDown) {
                            nudge[1] += step;
                        }
                    }
                });
                if nudge != [0.0, 0.0] && !self.nudge_selected_vector_points(nudge[0], nudge[1]) {
                    self.nudge_selection(nudge[0], nudge[1]);
                }
            }
            let last = self.symbol().timeline.frame_count.saturating_sub(1);
            if self.take_command_shortcut(ctx, C::ControlStepBack) {
                self.seek_frame(self.frame.saturating_sub(1));
            }
            if self.take_command_shortcut(ctx, C::ControlStepForward) {
                self.seek_frame((self.frame + 1).min(last));
            }
        }
        crate::commands::clear_queued_commands(ctx);
    }

    /// Design §8 menu shortcuts routed through [`crate::commands`]. Modifiers match exactly, so ⇧F5
    /// never runs F5 and ⌥⇧R never selects the Rectangle tool. Shortcuts of unavailable menu items are
    /// consumed at the end if unbound to an active command.
    fn menu_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::authoring_modify::Arrange;
        use crate::commands::{CommandId as C, has_override, modifiers_match};
        use crate::timeline_edit::FrameCommand as F;
        // Text creation and text-object editing deliberately focus the Properties text field.
        // Classic Flash object shortcuts remain object commands while a Stage selection exists,
        // so handle them before the general typing guard. Their Command-modified chords are safe
        // from ordinary character input and each command still validates the current selection.
        if !self.selection.is_empty() && self.take_command_shortcut(ctx, C::ModifyBreakApart) {
            self.break_apart_selection();
        }
        if self.can_ungroup_selection() && self.take_command_shortcut(ctx, C::ModifyUngroup) {
            self.ungroup_selection();
        }
        if !self.selection.is_empty() && self.take_command_shortcut(ctx, C::ModifyGroup) {
            self.group_selection();
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        // ⌥⌘X / ⌥⌘C / ⌥⌘V arrive as Cut / Copy / Paste events on macOS when un-overridden.
        let default_frame_clip = !has_override(&self.workspace.shortcuts, C::EditTimelineCutFrames)
            && !has_override(&self.workspace.shortcuts, C::EditTimelineCopyFrames)
            && !has_override(&self.workspace.shortcuts, C::EditTimelinePasteFrames);
        if default_frame_clip {
            let frame_clipboard = ctx.input_mut(|i| {
                if !(i.modifiers.alt && i.modifiers.command) {
                    return None;
                }
                let mut command = None;
                i.events.retain(|event| {
                    let found = match event {
                        egui::Event::Cut => Some(F::Cut),
                        egui::Event::Copy => Some(F::Copy),
                        egui::Event::Paste(_) => Some(F::Paste),
                        _ => None,
                    };
                    command = command.or(found);
                    found.is_none()
                });
                command
            });
            if let Some(command) = frame_clipboard {
                self.run_frame_command(ctx, command);
            }
        }
        for command in [F::Cut, F::Copy, F::Paste, F::Clear, F::Remove, F::SelectAll] {
            if self.take_command_shortcut(ctx, C::for_frame_command(command)) {
                self.run_frame_command(ctx, command);
            }
        }
        if self.take_command_shortcut(ctx, C::EditTimelineReverseFrames)
            && self.can_reverse_frames()
        {
            self.reverse_frames();
        }
        if self.take_command_shortcut(ctx, C::EditTimelineSyncSymbols) && self.can_sync_symbols() {
            self.sync_symbols();
        }
        if self.take_command_shortcut(ctx, C::EditTimelineCopyMotion) && self.can_copy_motion() {
            self.copy_motion();
        }
        if self.take_command_shortcut(ctx, C::EditTimelinePasteMotion) && self.can_paste_motion() {
            self.paste_motion();
        }

        // File.
        if self.take_command_shortcut(ctx, C::FileClose) {
            if ctx.viewport_id() != egui::ViewportId::ROOT
                && self.close_floating_viewport(ctx.viewport_id())
            {
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            } else {
                self.request(Pending::CloseDocument, ctx);
            }
        }
        if self.take_command_shortcut(ctx, C::FileRevert) && self.can_revert() {
            self.request_revert();
        }
        if self.take_command_shortcut(ctx, C::FileImportGraphics) {
            self.import_graphics_dialog();
        }
        if self.take_command_shortcut(ctx, C::FileImportLegacyFlash) {
            self.request(Pending::Import, ctx);
        }
        if self.take_command_shortcut(ctx, C::FileExportVideo) {
            self.open_export_video(None);
        }
        if self.take_command_shortcut(ctx, C::FileExportAudio) {
            self.open_export_audio(ctx, None);
        }
        if self.take_command_shortcut(ctx, C::FileExportLottie) {
            self.export_lottie_dialog(None);
        }
        if self.take_command_shortcut(ctx, C::FilePublishSettings) {
            Self::open_planned(ctx, "File ▸ Publish settings…");
        }
        if self.take_command_shortcut(ctx, C::FilePublish) {
            self.export(false);
        }

        // Edit.
        if self.take_command_shortcut(ctx, C::EditDeselect) {
            self.finish_edits();
            self.clear_selection();
        }
        if self.take_command_shortcut(ctx, C::EditFindAndReplace) {
            self.open_find_replace();
        }
        if self.take_command_shortcut(ctx, C::EditSymbols) {
            self.toggle_edit_symbols();
        }
        if self.take_command_shortcut(ctx, C::EditPreferences) {
            Self::open_planned(ctx, "Edit ▸ Preferences…");
        }
        if self.take_command_shortcut(ctx, C::EditKeyboardShortcuts) {
            Self::open_planned(ctx, "Edit ▸ Keyboard shortcuts…");
        }

        // Insert.
        if self.take_command_shortcut(ctx, C::InsertLayer) {
            self.insert_timeline_layer();
        }
        if self.take_command_shortcut(ctx, C::InsertLayerFolder) {
            self.insert_layer_folder(self.layer);
        }
        if self.take_command_shortcut(ctx, C::InsertMotionGuide)
            && self.can_add_motion_guide_to(self.layer)
        {
            self.add_motion_guide();
        }
        for (cmd, kind) in [
            (C::InsertClassicTween, neoflash_core::TweenKind::Classic),
            (C::InsertShapeTween, neoflash_core::TweenKind::Shape),
            (C::InsertMotionTween, neoflash_core::TweenKind::Motion),
        ] {
            if self.take_command_shortcut(ctx, cmd)
                && self.can_create_tween()
                && self.current_tween() != Some(kind)
            {
                self.create_tween(kind);
            }
        }
        if self.take_command_shortcut(ctx, C::InsertScene) {
            self.new_scene();
        }

        // Modify.
        if self.take_command_shortcut(ctx, C::ModifyDocument) {
            self.open_document_settings();
        }
        if self.take_command_shortcut(ctx, C::ModifyDistributeToLayers)
            && self.can_distribute_to_layers()
        {
            self.distribute_to_layers();
        }
        if self.take_command_shortcut(ctx, C::ModifyTraceBitmap) && self.can_trace_bitmap() {
            self.open_trace_bitmap(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineLayerProperties) {
            self.open_layer_properties(ctx, self.layer);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineClearKeyframe) {
            self.clear_keyframe();
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineConvertToKeyframes)
            && self.layer_editable()
        {
            self.convert_to_keyframes(false);
        }
        if self.take_command_shortcut(ctx, C::ModifyTimelineConvertToBlankKeyframes)
            && self.layer_editable()
        {
            self.convert_to_keyframes(true);
        }
        if self.take_command_shortcut(ctx, C::ModifyFreeTransform) && !self.selection.is_empty() {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::FreeTransform);
        }
        if self.take_command_shortcut(ctx, C::ModifyDistort)
            && self.can_distort_or_envelope_selection()
        {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::Distort);
        }
        if self.take_command_shortcut(ctx, C::ModifyEnvelope)
            && self.can_distort_or_envelope_selection()
        {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::Envelope);
        }
        if self.take_command_shortcut(ctx, C::ModifyScale) && !self.selection.is_empty() {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::Scale);
        }
        if self.take_command_shortcut(ctx, C::ModifyRotateAndSkew) && !self.selection.is_empty() {
            self.set_free_transform_mode(crate::authoring_tools::FreeTransformMode::RotateAndSkew);
        }
        if self.take_command_shortcut(ctx, C::ModifyScaleAndRotate) && !self.selection.is_empty() {
            self.open_scale_and_rotate_dialog();
        }
        if self.take_command_shortcut(ctx, C::ModifyRotate90Cw) && !self.selection.is_empty() {
            self.rotate_90_cw_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyRotate90Ccw) && !self.selection.is_empty() {
            self.rotate_90_ccw_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyFlipVertical) && !self.selection.is_empty() {
            self.flip_vertical_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyFlipHorizontal) && !self.selection.is_empty() {
            self.flip_horizontal_selection(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyRemoveTransform) && !self.selection.is_empty() {
            self.reset_selection_transform(ctx);
        }
        if self.take_command_shortcut(ctx, C::ModifyDeleteEnvelope)
            && self.can_delete_envelope_selection()
        {
            self.delete_envelope_selection();
        }
        for (cmd, dir) in [
            (C::ModifyArrangeFront, Arrange::Front),
            (C::ModifyArrangeForward, Arrange::Forward),
            (C::ModifyArrangeBackward, Arrange::Backward),
            (C::ModifyArrangeBack, Arrange::Back),
        ] {
            if self.take_command_shortcut(ctx, cmd) && !self.selection.is_empty() {
                self.arrange_selection(dir);
            }
        }
        // Control.
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        if self.take_command_shortcut(ctx, C::ControlRewind) {
            self.seek_frame(0);
        }
        if self.take_command_shortcut(ctx, C::ControlGoToEnd) {
            self.seek_frame(last);
        }
        if self.take_command_shortcut(ctx, C::ControlLoopPlayback) {
            self.workspace.timeline.loop_playback = !self.workspace.timeline.loop_playback;
        }
        if self.take_command_shortcut(ctx, C::ControlPlayAllScenes) {
            self.timeline_edit.play_all_scenes = !self.timeline_edit.play_all_scenes;
        }
        if self.take_command_shortcut(ctx, C::ControlMuteSounds) {
            self.toggle_mute_sounds();
        }
        if self.take_command_shortcut(ctx, C::ControlEnableSimpleButtons) {
            self.set_enable_simple_buttons(!self.enable_simple_buttons);
        }

        // Window.
        for panel in PanelId::DETACHABLE {
            if panel == PanelId::Actions {
                continue;
            }
            if self.take_command_shortcut(ctx, C::for_panel(panel)) {
                let visible = !self.panels_hidden && self.workspace.is_panel_active(panel);
                self.set_panel_visible(panel, !visible);
            }
        }
        if self.take_command_shortcut(ctx, C::WindowHidePanels) {
            self.toggle_hide_panels();
        }
        if self.take_command_shortcut(ctx, C::WindowInspectorPriority) {
            self.give_dock_priority(PanelId::Properties);
        }
        if self.take_command_shortcut(ctx, C::WindowLibraryPriority) {
            self.give_dock_priority(PanelId::Library);
        }

        // Unavailable items: consume only after active registry commands have had a chance to run,
        // so a user can rebind an active command onto F4 / ⌥⌘K / etc. if desired.
        let (cmd, shift, alt, none) = (
            Modifiers::COMMAND,
            Modifiers::SHIFT,
            Modifiers::ALT,
            Modifiers::NONE,
        );
        if ctx.input_mut(|i| i.consume_key(alt | cmd, Key::H)) {
            self.toggle_shape_hints();
        }
        if ctx.input_mut(|i| i.consume_key(shift | cmd, Key::H)) {
            self.add_shape_hint();
        }
        if ctx.input_mut(|input| input.consume_key(none, Key::F1)) {
            // Help search is an immediate command palette; Preferences remains the separate place
            // for reviewing and changing shortcuts.
            self.open_function_search(ctx);
        }
        for (m, key) in [(cmd, Key::P), (cmd | shift, Key::W), (alt | cmd, Key::K)] {
            ctx.input_mut(|i| {
                let held = i.modifiers;
                if modifiers_match(held, m) {
                    let _ = i.consume_key(m, key);
                }
            });
        }
    }

    pub fn move_key(&mut self, layer: usize, from: u32, to: u32, copy: bool) -> bool {
        let is_button = self.symbol().kind == SymbolKind::Button;
        let timeline = &self.symbol().timeline;
        let Some(l) = timeline.layers.get(layer) else {
            return false;
        };
        let max_frames = if is_button {
            timeline.frame_count
        } else {
            1_000_000
        };
        if layer_effectively_locked(timeline, l)
            || !layer_effectively_visible(timeline, l)
            || l.properties.role == LayerRole::Folder
            || to >= max_frames
            || from == to
        {
            return false;
        }
        if l.keyframes.iter().any(|k| k.frame == to) {
            self.message("A keyframe already exists at the destination.");
            return false;
        }
        let Some(mut key) = l.keyframes.iter().find(|k| k.frame == from).cloned() else {
            return false;
        };
        let script = self
            .timeline_edit
            .include_scripts
            .then(|| {
                timeline
                    .scripts
                    .iter()
                    .find(|script| script.frame == from)
                    .cloned()
            })
            .flatten();
        if script.is_some() && timeline.scripts.iter().any(|script| script.frame == to) {
            self.message("A frame script already exists at the destination. The key and script were not moved.");
            return false;
        }
        let step_label = if copy {
            "Copy keyframe"
        } else {
            "Move keyframe"
        };
        self.transact_named(step_label, |s| {
            let old_count = s.symbol().timeline.frame_count;
            let (layer_id, old_layer_end, was_trailing_single) = {
                let l = &s.symbol().timeline.layers[layer];
                let old_end = l.properties.end_frame.unwrap_or(old_count);
                let was_last = l.keyframes.last().is_some_and(|k| k.frame == from);
                let still_last = l
                    .keyframes
                    .iter()
                    .filter(|k| k.frame != from)
                    .all(|k| k.frame < to);
                (
                    l.id.clone(),
                    old_end,
                    !copy && from > 0 && was_last && still_last && old_end == from + 1,
                )
            };
            let new_layer_end = if was_trailing_single {
                to + 1
            } else {
                old_layer_end.max(to + 1)
            };
            let l = &mut s.symbol_mut().timeline.layers[layer];
            if !copy {
                l.keyframes.retain(|k| k.frame != from);
            }
            key.frame = to;
            if copy {
                key.label.clear();
            }
            l.keyframes.push(key);
            l.keyframes.sort_by_key(|k| k.frame);
            l.normalize_motion_keys();
            if let Some(mut script) = script {
                let scripts = &mut s.symbol_mut().timeline.scripts;
                if !copy {
                    scripts.retain(|item| item.frame != from);
                }
                script.frame = to;
                scripts.push(script);
                scripts.sort_by_key(|item| item.frame);
            }
            if new_layer_end != old_layer_end {
                let timeline = &mut s.symbol_mut().timeline;
                for other in &mut timeline.layers {
                    if other.id == layer_id {
                        other.properties.end_frame = Some(new_layer_end);
                    } else if other.properties.role != LayerRole::Folder
                        && other.properties.end_frame.is_none()
                    {
                        other.properties.end_frame = Some(old_count);
                    }
                }
                let next_count = timeline
                    .layers
                    .iter()
                    .filter(|other| other.properties.role != LayerRole::Folder)
                    .map(|other| other.properties.end_frame.unwrap_or(old_count))
                    .chain(timeline.scripts.iter().map(|sc| sc.frame + 1))
                    .max()
                    .unwrap_or(1)
                    .max(1);
                timeline.frame_count = next_count;
                for other in &mut timeline.layers {
                    if other.properties.role == LayerRole::Folder
                        && other.properties.end_frame.is_some()
                    {
                        other.properties.end_frame = Some(next_count);
                    }
                }
                s.project.format_version = s.project.format_version.max(5);
            } else if let Some(end) =
                &mut s.symbol_mut().timeline.layers[layer].properties.end_frame
            {
                *end = (*end).max(to + 1);
            }
            s.frame = to;
            s.layer = layer;
            s.clear_selection();
        });
        true
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
        let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
        if self.frame >= layer_end {
            let target = self.frame;
            if self.apply_frame_edit(TimelineEdit::SetEnd {
                layers: vec![layer_id],
                end_frame: target.saturating_add(1),
            }) {
                self.frame = target;
            }
            return;
        }
        self.apply_frame_edit(TimelineEdit::Insert {
            layers: vec![layer_id],
            at: (self.frame + 1).min(count),
            count: 1,
            include_scripts: self.timeline_edit.include_scripts,
        });
    }

    pub fn draw_dialogs(&mut self, ctx: &egui::Context) {
        self.drain_converted_entry_triggers(ctx);
        self.document_settings_dialog(ctx);
        self.publish_settings_dialog(ctx);
        self.export_lottie_modal(ctx);
        self.export_video_modal(ctx);
        if let Some(font) = self
            .preferences
            .as_ref()
            .map(|draft| draft.code_editor.font.clone())
        {
            self.fonts.ensure_loaded(ctx, &font);
        }
        self.preferences_dialog(ctx);
        self.find_replace_dialog(ctx);
        self.edit_envelope_dialog(ctx);
        self.paste_motion_special_dialog(ctx);
        self.sound_properties_dialog(ctx);
        self.export_audio_dialog(ctx);
        self.export_image_dialog(ctx);
        self.import_video_dialog(ctx);
        self.stroke_style_dialog(ctx);
        self.font_embedding_dialog(ctx);
        self.layer_properties_dialog(ctx);
        self.polystar_settings_modal(ctx);
        self.scale_and_rotate_modal(ctx);
        self.help_manual_dialog(ctx);
        use neoflash_ui::{
            TextStyle,
            components::{Button, DialogHeader, Surface, logo, surface::dialog_footer},
            tokens::space,
            typography::rich,
        };
        let p = neoflash_ui::palette(ctx);
        let body = egui::Margin {
            left: space::S5 as i8,
            right: space::S5 as i8,
            top: 0,
            bottom: space::S5 as i8,
        };
        if self.show_about {
            let mut close = false;
            let mut open_manual = false;
            let modal = egui::Modal::new(egui::Id::new("about-neoflash"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    ui.set_width(440.);
                    close |= DialogHeader { title: "About NeoFlash" }.show(ui);
                    egui::Frame::new().inner_margin(body).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = space::S2;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = space::S3;
                            logo(ui, 40.);
                            ui.vertical(|ui| {
                                ui.label(
                                    rich("NeoFlash 0.1 — development preview", TextStyle::Title)
                                        .color(p.text_primary),
                                );
                                ui.label(
                                    rich("Open source · MIT OR Apache-2.0", TextStyle::Small)
                                        .color(p.text_secondary),
                                );
                            });
                        });
                        for line in [
                            "Classic Flash workflow. Rust core. HTML5 publishing.",
                            "This is an early authoring foundation, not complete Flash compatibility.",
                            "See docs/COMPATIBILITY.md and docs/ROADMAP.md for the exact scope.",
                        ] {
                            ui.add(
                                egui::Label::new(
                                    rich(line, TextStyle::DialogBody).color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        }
                    });
                    dialog_footer(
                        ui,
                        |ui| {
                            open_manual |= ui.add(Button::new("Help & Reference…")).clicked();
                        },
                        |ui| close |= ui.add(Button::primary("Close").shortcut("↵")).clicked(),
                    );
                });
            let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if open_manual {
                self.show_about = false;
                crate::panels::help_manual::open_help_manual(ctx);
            } else if close || enter || modal.should_close() {
                self.show_about = false;
            }
        }
        if self.confirm_revert {
            // 0 = Revert, 1 = Cancel.
            let mut action = None;
            let name = self.project.name.clone();
            let modal = egui::Modal::new(egui::Id::new("revert-changes"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    ui.set_width(480.);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(space::S5 as i8))
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S4;
                                let (badge, _) = ui.allocate_exact_size(
                                    egui::vec2(40., 40.),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(badge.center(), 20., p.warning_subtle);
                                neoflash_ui::icons::paint(
                                    ui.painter(),
                                    ctx,
                                    badge,
                                    neoflash_ui::icons::i16::WARNING,
                                    p.warning_default,
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = space::S1;
                                    ui.label(
                                        rich("Revert to saved version?", TextStyle::DialogTitle)
                                            .color(p.text_primary),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            rich(
                                                format!(
                                                    "Your unsaved changes to “{name}” will be lost and the last saved version on disk will be reloaded."
                                                ),
                                                TextStyle::DialogBody,
                                            )
                                            .color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        });
                    let (mut revert, mut cancel) = (false, false);
                    dialog_footer(
                        ui,
                        |_| {},
                        |ui| {
                            revert = ui.add(Button::danger("Revert").shortcut("↵")).clicked();
                            cancel = ui.add(Button::new("Cancel").shortcut("Esc")).clicked();
                        },
                    );
                    if revert {
                        action = Some(0);
                    } else if cancel {
                        action = Some(1);
                    }
                });
            if action.is_none() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    action = Some(0);
                } else if modal.should_close() {
                    action = Some(1);
                }
            }
            if let Some(action) = action {
                self.confirm_revert = false;
                if action == 0 {
                    self.revert_to_saved(ctx);
                }
            }
        }
        if self.confirm.is_some() {
            // 0 = Save, 1 = Discard, 2 = Cancel.
            let mut action = None;
            let name = self.project.name.clone();
            let modal = egui::Modal::new(egui::Id::new("unsaved-changes"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    ui.set_width(480.);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(space::S5 as i8))
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S4;
                                let (badge, _) = ui.allocate_exact_size(
                                    egui::vec2(40., 40.),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(badge.center(), 20., p.warning_subtle);
                                neoflash_ui::icons::paint(
                                    ui.painter(),
                                    ctx,
                                    badge,
                                    neoflash_ui::icons::i16::WARNING,
                                    p.warning_default,
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = space::S1;
                                    ui.label(
                                        rich("Save changes before continuing?", TextStyle::DialogTitle)
                                            .color(p.text_primary),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            rich(
                                                format!(
                                                    "Your changes to “{name}” will be lost if you discard them."
                                                ),
                                                TextStyle::DialogBody,
                                            )
                                            .color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        });
                    let (mut discard, mut save, mut cancel) = (false, false, false);
                    dialog_footer(
                        ui,
                        |ui| discard = ui.add(Button::danger("Discard")).clicked(),
                        |ui| {
                            save = ui.add(Button::primary("Save").shortcut("↵")).clicked();
                            cancel = ui.add(Button::new("Cancel").shortcut("Esc")).clicked();
                        },
                    );
                    if save {
                        action = Some(0);
                    } else if discard {
                        action = Some(1);
                    } else if cancel {
                        action = Some(2);
                    }
                });
            if action.is_none() {
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    action = Some(0);
                } else if modal.should_close() {
                    action = Some(2);
                }
            }
            if let Some(action) = action {
                if action == 2 {
                    self.cancel_confirmation(ctx);
                } else if action == 1 || (action == 0 && self.save(false)) {
                    self.continue_confirmation(ctx);
                } else if action == 0 {
                    self.cancel_confirmation(ctx);
                }
            }
        }
        if self.confirm.is_none()
            && (!self.dirty() || !self.sessions.is_empty())
            && let Some(summary) = self
                .recovery
                .current_prompt_summary(crate::recovery::unix_now_secs())
        {
            // 0 = Restore, 1 = Discard.
            let mut action = None;
            let modal = egui::Modal::new(egui::Id::new("recovery-prompt"))
                .frame(Surface::Dialog.frame(ctx))
                .backdrop_color(p.overlay)
                .show(ctx, |ui| {
                    ui.set_width(480.);
                    egui::Frame::new()
                        .inner_margin(egui::Margin::same(space::S5 as i8))
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = space::S4;
                                let (badge, _) = ui.allocate_exact_size(
                                    egui::vec2(40., 40.),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .circle_filled(badge.center(), 20., p.warning_subtle);
                                neoflash_ui::icons::paint(
                                    ui.painter(),
                                    ctx,
                                    badge,
                                    neoflash_ui::icons::i16::WARNING,
                                    p.warning_default,
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = space::S1;
                                    ui.label(
                                        rich("Recover unsaved work?", TextStyle::DialogTitle)
                                            .color(p.text_primary),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            rich(summary, TextStyle::DialogBody)
                                                .color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        });
                    let (mut discard, mut restore) = (false, false);
                    dialog_footer(
                        ui,
                        |ui| discard = ui.add(Button::danger("Discard")).clicked(),
                        |ui| {
                            restore = ui.add(Button::primary("Restore").shortcut("↵")).clicked();
                        },
                    );
                    if restore {
                        action = Some(0);
                    } else if discard {
                        action = Some(1);
                    }
                });
            let _ = modal;
            if action.is_none()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                action = Some(0);
            }
            match action {
                Some(0) => self.restore_recovery_prompt(ctx),
                Some(1) => self.discard_recovery_prompt(),
                _ => {}
            }
        }
    }
}

impl eframe::App for Editor {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.tablet_input.append_events(ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.workspace.active_bottom = self.bottom;
        self.workspace.save(storage);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn on_exit(&mut self) {
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

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_density(ctx);
        self.poll_debugger(ctx);
        let native_quit = crate::native_quit::take_requested();
        if (native_quit || ctx.input(|i| i.viewport().close_requested())) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit(ctx);
        }
        let now_secs = ctx.input(|i| i.time);
        if let Some(wait) = self.tick_autosave(now_secs, crate::recovery::unix_now_secs()) {
            ctx.request_repaint_after(wait);
        }
        self.shortcuts(ctx);
        self.sync_authored_preview();
        if self.playing {
            let elapsed = self.last_tick.elapsed().as_secs_f64().min(0.25);
            self.last_tick = Instant::now();
            self.accumulator += elapsed;
            let step = 1. / self.project.fps.max(1) as f64;
            while self.playing && self.accumulator >= step {
                let before = self.frame;
                self.tick_authored_preview();
                self.after_playback_tick(before);
                self.accumulator -= step;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(8));
        }
        self.pump_timeline_audio();
        self.normalize_selection();
        self.sync_closed_detached_panels(ctx);
        self.menu(ctx);
        self.status_bar(ctx);
        self.bitmap_textures.begin_frame(&self.project);
        self.prepare_docked_panel_focus(ctx);
        if self.has_document() {
            if !self.panels_hidden {
                self.dock_grid(ctx);
            }
            self.document_bar(ctx);
            self.stage(ctx);
        } else {
            self.no_document_view(ctx);
        }
        self.draw_dialogs(ctx);
        self.workspace_dialog(ctx);
        self.customize_tools_dialog(ctx);
        self.trace_bitmap_dialog(ctx);
        self.symbol_creation_dialog(ctx);
        if self.has_document() {
            self.detached_panels(ctx);
        }
        self.planned_panels(ctx);
        self.update_publish_estimate(ctx);
        self.notification_host(ctx, self.has_document().then_some(self.stage_view.canvas));
        crate::native_windows::configure_panels();
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.project.name.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — NeoFlash",
            if self.has_document() {
                name
            } else {
                "No document open".into()
            },
            if self.dirty() { " •" } else { "" }
        )));
        crate::shell::capture(ctx, ctx.cumulative_pass_nr());
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
