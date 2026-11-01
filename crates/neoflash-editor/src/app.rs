use crate::fonts::Fonts;
use crate::theme::ThemeMode;
use crate::workspace::{PanelId, WorkspacePrefs};
use eframe::egui::{self, Key, Modifiers, Vec2};
use neoflash_core::*;
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Copy, PartialEq)]
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
    pub saved_project: Option<Project>,
    pub editing: Vec<String>,
    pub frame: u32,
    pub layer: usize,
    pub selection: crate::selection::Selection,
    pub object_gesture: Option<crate::stage_selection::SelectionGesture>,
    pub tool: Tool,
    pub(crate) authoring_tools: crate::authoring_tools::AuthoringToolsState,
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
    pub onion_opacity: f32,
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
    pub resource_panel: crate::resources::ResourcePanel,
    pub bitmap_textures: crate::bitmaps::BitmapTextures,
    pub vector: crate::vector::VectorState,
    pub clipboard: crate::clipboard::ClipboardState,
    pub workspace: WorkspacePrefs,
    pub detached: crate::detached::DetachedState,
    pub(crate) stage_view: crate::stage_view::StageView,
    pub(crate) timeline_motion: crate::timeline_motion::MotionState,
    pub(crate) timeline_thumbnails: crate::timeline_preview::ThumbnailCache,
}

#[derive(Clone, Copy)]
pub enum Pending {
    New,
    Demo,
    Open,
    Import,
    Close,
}

impl Editor {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut s = Self::from_project(&cc.egui_ctx, Project::demo());
        // Screenshots ignore saved layouts so captures are reproducible (see shell::capture).
        let storage = if std::env::var_os("NEOFLASH_SCREENSHOT").is_some() {
            None
        } else {
            cc.storage
        };
        s.workspace =
            WorkspacePrefs::load_for_viewport(storage, cc.egui_ctx.viewport_rect().size().into());
        // `NEOFLASH_OPEN=path.nflash` opens a document at launch (screenshots of real documents).
        if let Some(path) = std::env::var_os("NEOFLASH_OPEN") {
            s.open_path(path.into(), &cc.egui_ctx);
        }
        crate::native_windows::install();
        if let Some(template) = crate::shell::screenshot_workspace() {
            s.workspace.apply_template(template);
            s.bottom = s.workspace.active_bottom;
        }
        if let Some(theme) = crate::shell::screenshot_theme() {
            s.workspace.theme = theme;
        }
        s.workspace.theme.apply(&cc.egui_ctx);
        s.bottom = s.workspace.active_bottom;
        s.stage_view.prefs = s.workspace.view_defaults.clone().unwrap_or_default();
        // Screenshot aid (editor-view-settings-open.png): open the View settings popover.
        s.stage_view.settings_open =
            std::env::var_os("NEOFLASH_SCREENSHOT_VIEW_SETTINGS").is_some();
        if let Some(path) = std::env::args().nth(1) {
            s.open_path(PathBuf::from(path), &cc.egui_ctx);
        }
        s
    }
    pub(crate) fn from_project(ctx: &egui::Context, project: Project) -> Self {
        crate::theme::initialize_editor_style(ctx);
        WorkspacePrefs::default().theme.apply(ctx);
        let saved_project = Some(project.clone());
        let root = project.root.clone();
        let mut s = Self {
            project, history: History::new(150), path: None, saved_project,
            editing: vec![root.clone()], frame: 0, layer: 0, selection: crate::selection::Selection { symbol: root, targets: vec![] }, object_gesture: None, tool: Tool::Select,
            playing: false, authored_preview: None, last_tick: Instant::now(), accumulator: 0.,
            onion: false, onion_outlines: false, onion_before: 3, onion_after: 3,
            onion_anchor: false, anchored_range: (0, 6), onion_opacity: 0.3,
            zoom: 0.75, pan: Vec2::ZERO, shape_draft: None,
            fill: [50, 117, 184, 255], stroke: [30, 50, 70, 255], stroke_width: 1.0,
            property_edit: None, authoring_modify: crate::authoring_modify::AuthoringModify::default(), bottom: BottomTab::Properties,
            authoring_tools: crate::authoring_tools::AuthoringToolsState::default(),
            script_target: ScriptTarget::Frame, script_event: ObjectEvent::Click,
            selected_source: None, code_feedback: None, actions_ui: Default::default(),
            status: "Ready · F5 frame · F6 keyframe · F7 blank keyframe · ⌘↵ Test Movie".into(),
            output: vec!["NeoFlash development preview. Editor playback shows authored animation; Test Movie executes scripts in the browser.".into()],
            fonts: Fonts::new(), font_filter: String::new(), show_about: false, confirm: None, allow_close: false,
            timeline_cell: neoflash_ui::metrics::FRAME, timeline_key_drag: None, timeline_edit: Default::default(), library_selected: None, symbol_conversion: None,
            library_panel: crate::library::LibraryPanel::default(),
            graphic_drop_position: None,
            resource_panel: crate::resources::ResourcePanel::default(),
            bitmap_textures: crate::bitmaps::BitmapTextures::default(),
            vector: crate::vector::VectorState::default(),
            clipboard: crate::clipboard::ClipboardState::default(),
            workspace: WorkspacePrefs::default(),
            detached: crate::detached::DetachedState::default(),
            stage_view: Default::default(),
            timeline_motion: Default::default(),
            timeline_thumbnails: Default::default(),
        };
        s.fonts.install_ui(ctx);
        s.load_project_fonts(ctx);
        s.normalize_selection();
        s
    }
    pub(crate) fn set_theme(&mut self, ctx: &egui::Context, theme: ThemeMode) {
        self.workspace.theme = theme;
        theme.apply(ctx);
    }
    pub(crate) fn show_panel(&mut self, panel: PanelId) {
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
        self.workspace.active_bottom = self.bottom;
        self.workspace.set_panel_visible(panel, visible);
        self.bottom = self.workspace.active_bottom;
    }
    /// Re-apply the current workspace template ("Reset <template> workspace").
    pub(crate) fn restore_workspace(&mut self, ctx: &egui::Context) {
        self.apply_workspace_template(ctx, self.workspace.template);
    }
    /// Switch to a built-in workspace template (Window ▸ Workspace, top-bar select).
    pub(crate) fn apply_workspace_template(
        &mut self,
        ctx: &egui::Context,
        template: crate::workspace::WorkspaceTemplate,
    ) {
        self.workspace.apply_template(template);
        if template == crate::workspace::WorkspaceTemplate::Classic {
            self.workspace
                .compact_default(ctx.viewport_rect().size().into());
        }
        self.detached = crate::detached::DetachedState::default();
        self.bottom = self.workspace.active_bottom;
        // Reset only the panel geometry that we own. Editor drafts, focus and
        // unrelated egui memory are not a workspace layout preference.
        ctx.data_mut(|data| {
            for id in [
                "timeline",
                "tools",
                "library",
                "properties",
                "dock-edge-top",
                "dock-edge-bottom",
                "dock-edge-left",
                "dock-edge-right",
            ] {
                data.remove::<egui::containers::panel::PanelState>(egui::Id::new(id));
            }
        });
        ctx.request_repaint();
    }
    pub fn symbol_id(&self) -> &str {
        self.editing
            .last()
            .map(String::as_str)
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
        self.vector
            .draft
            .as_ref()
            .is_some_and(|d| d.anchors.len() >= 2)
            || self.saved_project.as_ref() != Some(&self.project)
    }
    pub fn message(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.output.push(self.status.clone());
    }
    pub fn transact(&mut self, f: impl FnOnce(&mut Self)) {
        self.finish_property_edit();
        self.finish_selection_gesture();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.finish_point_drag();
        self.history.begin(&self.project);
        f(self);
        self.project.prune_authoring_groups();
        self.commit_history();
    }
    pub fn normalize_selection(&mut self) {
        self.cancel_stale_selection_gesture();
        self.editing.retain(|id| self.project.symbol(id).is_some());
        if self.editing.is_empty() {
            self.editing.push(self.project.root.clone());
        }
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
        self.frame = self
            .frame
            .min(self.symbol().timeline.frame_count.saturating_sub(1));
        self.normalize_targets();
    }
    pub fn undo(&mut self) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history.undo(&mut self.project);
        self.normalize_selection();
    }
    pub fn redo(&mut self) {
        self.finish_edits();
        self.timeline_key_drag = None;
        self.vector = Default::default();
        self.history.redo(&mut self.project);
        self.normalize_selection();
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
        self.transact(|s| {
            s.ensure_key(blank);
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
        self.transact(|s| {
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
            let element = Element {
                accessibility: None,
                id: id.clone(),
                name,
                transform,
                kind,
                scripts: vec![],
            };
            let layer = s.layer;
            let frame = s.frame;
            s.symbol_mut().timeline.layers[layer]
                .key_at_mut(frame)
                .unwrap()
                .elements
                .push(element);
            s.select_single_id(id);
        });
        self.tool = Tool::Select;
    }
    pub fn convert_to_symbol(&mut self) {
        self.begin_symbol_conversion();
    }
    pub fn enter_symbol(&mut self, id: String) {
        self.finish_edits();
        self.timeline_key_drag = None;
        if self.project.symbol(&id).is_none() {
            return;
        }
        self.editing.push(id);
        self.frame = 0;
        self.layer = 0;
        self.clear_selection();
        self.playing = false;
        self.pan = Vec2::ZERO;
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
            if let Err(e) = self.fonts.load_style(ctx, &family, bold, italic) {
                self.message(e);
            }
        }
    }
    pub fn request(&mut self, pending: Pending, ctx: &egui::Context) {
        self.finish_edits();
        if self.dirty() {
            self.confirm = Some(pending);
        } else {
            self.perform(pending, ctx);
        }
    }
    pub fn perform(&mut self, pending: Pending, ctx: &egui::Context) {
        self.cancel_edits();
        match pending {
            Pending::New | Pending::Demo => {
                self.project = if matches!(pending, Pending::New) {
                    Project::blank()
                } else {
                    Project::demo()
                };
                self.clipboard = Default::default();
                self.resource_panel = Default::default();
                self.library_panel = Default::default();
                self.timeline_edit = Default::default();
                self.graphic_drop_position = None;
                self.bitmap_textures = Default::default();
                self.editing = vec![self.project.root.clone()];
                self.frame = 0;
                self.layer = 0;
                self.clear_selection();
                self.path = None;
                self.history = History::new(150);
                self.saved_project = Some(self.project.clone());
                self.playing = false;
                self.load_project_fonts(ctx);
            }
            Pending::Open => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("NeoFlash project", &["nflash", "json"])
                    .pick_file()
                {
                    self.open_path(path, ctx)
                }
            }
            Pending::Import => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Flash source or movie", &["fla", "xfl", "xml", "swf"])
                    .pick_file()
                {
                    self.import_legacy(path, ctx);
                }
            }
            Pending::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close)
            }
        }
    }
    pub fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.cancel_edits();
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| Project::from_json(&s).map_err(|e| e.to_string()))
        {
            Ok(project) => {
                self.project = project;
                self.clipboard = Default::default();
                self.resource_panel = Default::default();
                self.library_panel = Default::default();
                self.timeline_edit = Default::default();
                self.graphic_drop_position = None;
                self.bitmap_textures = Default::default();
                self.editing = vec![self.project.root.clone()];
                self.frame = 0;
                self.layer = 0;
                self.clear_selection();
                self.path = Some(path);
                self.history = History::new(150);
                self.saved_project = Some(self.project.clone());
                self.playing = false;
                self.load_project_fonts(ctx);
                self.message("Project opened.");
            }
            Err(e) => self.message(format!("Open failed: {e}")),
        }
    }

    pub fn import_legacy(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.cancel_edits();
        match neoflash_import::inspect(&path) {
            Ok(report) => {
                self.message(format!(
                    "Legacy source: {:?} · {}",
                    report.format,
                    path.display()
                ));
                for d in &report.diagnostics {
                    self.message(format!("{}: {}", d.location, d.message));
                }
                if report.can_import {
                    match neoflash_import::import_project(&path) {
                        Ok(project) => {
                            self.project = project;
                            self.clipboard = Default::default();
                            self.resource_panel = Default::default();
                            self.library_panel = Default::default();
                            self.timeline_edit = Default::default();
                            self.graphic_drop_position = None;
                            self.bitmap_textures = Default::default();
                            self.editing = vec![self.project.root.clone()];
                            self.frame = 0;
                            self.layer = 0;
                            self.clear_selection();
                            self.path = None;
                            self.saved_project = None;
                            self.history = History::new(150);
                            self.playing = false;
                            self.load_project_fonts(ctx);
                            self.message(
                                "Imported supported XFL content. Save as a NeoFlash project.",
                            );
                        }
                        Err(e) => self.message(e.to_string()),
                    }
                } else {
                    self.message("Import stopped: this source contains features the current importer cannot preserve.");
                }
            }
            Err(e) => self.message(format!("Import failed: {e}")),
        }
        self.show_panel(PanelId::Output);
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("NeoFlash project", &["nflash"])
                .set_file_name("Untitled.nflash")
                .save_file()
        } else {
            self.path.clone()
        };
        let Some(path) = path else {
            return false;
        };
        self.finish_edits();
        let result = self
            .project
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|json| {
                let tmp = path.with_extension("nflash.tmp");
                std::fs::write(&tmp, json)
                    .and_then(|_| std::fs::rename(&tmp, &path))
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                self.saved_project = Some(self.project.clone());
                self.path = Some(path);
                self.message("Project saved.");
                true
            }
            Err(e) => {
                self.message(format!("Save failed: {e}"));
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
    pub fn export(&mut self, test: bool) {
        let output = if test {
            std::env::temp_dir()
                .join("neoflash-preview")
                .join(std::process::id().to_string())
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
        self.finish_edits();
        match neoflash_export::export_project(&self.project, &output, &Self::runtime_dir()) {
            Ok(index) => {
                self.message(format!("HTML5 exported: {}", index.display()));
                for warning in neoflash_export::export_warnings(&self.project) {
                    self.message(warning);
                }
                if test && let Err(e) = std::process::Command::new("open").arg(&index).spawn() {
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
        if self.symbol_conversion.is_some() {
            return;
        }
        self.menu_shortcuts(ctx);
        self.timeline_shortcuts(ctx);
        self.view_shortcuts(ctx);
        self.clipboard_events(ctx);
        let typing = ctx.wants_keyboard_input();
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)) {
            self.save(true);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.request(Pending::Open, ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.request(Pending::New, ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
            self.export(true);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F9)) {
            self.open_actions();
        }
        if !typing {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) {
                self.redo();
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
                self.undo();
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
                self.select_all();
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::D)) {
                self.duplicate();
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F6)) {
                self.make_key(false);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F7)) {
                self.make_key(true);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5)) {
                self.insert_frame();
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F8)) {
                self.begin_new_symbol();
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F8)) {
                self.convert_to_symbol();
            }
            if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
                self.delete_selected();
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                let captured = self.object_gesture.is_some()
                    || self.authoring_tools.is_drawing()
                    || self.shape_draft.is_some()
                    || self.vector.point_drag.is_some()
                    || self.vector.draft.is_some();
                self.cancel_edits();
                if !captured {
                    self.clear_selection();
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Enter)) && self.vector.draft.is_some() {
                self.finish_path(false);
            } else if ctx.input(|i| i.key_pressed(Key::Enter)) {
                self.set_playing(!self.playing);
            }
            for (key, tool) in [
                (Key::V, Tool::Select),
                (Key::Q, Tool::Transform),
                (Key::R, Tool::Rectangle),
                (Key::O, Tool::Ellipse),
                (Key::T, Tool::Text),
                (Key::P, Tool::Pen),
                (Key::Y, Tool::Pencil),
                (Key::A, Tool::Subselect),
                (Key::N, Tool::Line),
                (Key::B, Tool::Brush),
                (Key::K, Tool::Bucket),
                (Key::S, Tool::Ink),
                (Key::I, Tool::Eyedropper),
                (Key::E, Tool::Eraser),
                (Key::L, Tool::Lasso),
                (Key::H, Tool::Hand),
                (Key::Z, Tool::Zoom),
                (Key::M, Tool::Zoom),
                (Key::F, Tool::Gradient),
            ] {
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                    self.set_tool(tool);
                }
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
                self.finish_edits();
                self.reset_authored_preview();
                self.frame = self.frame.saturating_sub(1);
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
                self.finish_edits();
                self.reset_authored_preview();
                self.frame = (self.frame + 1).min(self.symbol().timeline.frame_count - 1);
            }
        }
    }

    /// Design §8 menu shortcuts not handled elsewhere. Modifiers match exactly, so ⇧F5 never runs F5
    /// and ⌥⇧R never selects the Rectangle tool. Shortcuts of unavailable menu items are consumed and
    /// do nothing.
    fn menu_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::timeline_edit::FrameCommand as F;
        if ctx.wants_keyboard_input() {
            return;
        }
        let (cmd, shift, alt, none) = (
            Modifiers::COMMAND,
            Modifiers::SHIFT,
            Modifiers::ALT,
            Modifiers::NONE,
        );
        let exact = |m: Modifiers, key: Key| {
            ctx.input_mut(|i| {
                let held = i.modifiers;
                held.alt == m.alt
                    && held.shift == m.shift
                    && held.command == m.command
                    && i.consume_key(m, key)
            })
        };
        // ⌥⌘X / ⌥⌘C / ⌥⌘V arrive as Cut / Copy / Paste events: frames, never objects.
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
        for (m, key, command) in [
            (alt | cmd, Key::X, F::Cut),
            (alt | cmd, Key::C, F::Copy),
            (alt | cmd, Key::V, F::Paste),
            (alt, Key::Backspace, F::Clear),
            (shift, Key::F5, F::Remove),
            (alt | cmd, Key::A, F::SelectAll),
        ] {
            if exact(m, key) {
                self.run_frame_command(ctx, command);
            }
        }
        // File.
        if exact(shift, Key::F12) {
            Self::open_planned(ctx, "File ▸ Publish settings…");
        }
        if exact(shift | alt, Key::F12) {
            self.export(false);
        }
        // Edit.
        if exact(cmd | shift, Key::A) {
            self.finish_edits();
            self.clear_selection();
        }
        if exact(cmd, Key::F) {
            Self::open_planned(ctx, "Edit ▸ Find and replace");
        }
        if exact(cmd, Key::E) {
            self.toggle_edit_symbols();
        }
        if exact(cmd, Key::U) {
            Self::open_planned(ctx, "Edit ▸ Preferences…");
        }
        if exact(alt | shift | cmd, Key::K) {
            Self::open_planned(ctx, "Edit ▸ Keyboard shortcuts…");
        }
        // View.
        if exact(alt | cmd, Key::G) {
            Self::open_planned(ctx, "View ▸ Grid ▸ Edit grid…");
        }
        // Modify.
        if exact(cmd, Key::J) {
            Self::open_planned(ctx, "Modify ▸ Document…");
        }
        if exact(cmd, Key::B) && !self.selection.is_empty() {
            self.break_apart_selection();
        }
        if exact(cmd | shift, Key::G) && !self.selection.is_empty() {
            self.ungroup_selection();
        }
        if exact(cmd, Key::G) && self.selection.len() >= 2 {
            self.group_selection();
        }
        // Control.
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        if exact(alt | shift, Key::R) {
            self.seek_frame(0);
        }
        if exact(alt | shift, Key::Period) {
            self.seek_frame(last);
        }
        if exact(none, Key::Period) {
            self.seek_frame((self.frame + 1).min(last));
        }
        if exact(none, Key::Comma) {
            self.seek_frame(self.frame.saturating_sub(1));
        }
        if exact(alt | cmd, Key::Enter) {
            Self::open_planned(ctx, "Control ▸ Test scene");
        }
        // Window.
        for (m, key, panel) in [
            (alt | cmd, Key::T, PanelId::Timeline),
            (cmd, Key::F2, PanelId::Tools),
            (cmd, Key::F3, PanelId::Properties),
            (cmd, Key::L, PanelId::Library),
            (cmd, Key::K, PanelId::Align),
            (cmd | shift, Key::F9, PanelId::Color),
            (cmd, Key::T, PanelId::Transform),
            (none, Key::F2, PanelId::Output),
            (shift, Key::F11, PanelId::Accessibility),
        ] {
            if exact(m, key) {
                let visible = self.workspace.is_panel_visible(panel);
                self.set_panel_visible(panel, !visible);
            }
        }
        for (m, key, path) in [
            (cmd, Key::F9, "Window ▸ Swatches"),
            (cmd, Key::I, "Window ▸ Info"),
            (cmd, Key::F10, "Window ▸ History"),
            (shift, Key::F2, "Window ▸ Scene"),
            (alt, Key::F2, "Window ▸ Compiler errors"),
        ] {
            if exact(m, key) {
                Self::toggle_planned(ctx, path);
            }
        }
        if exact(shift, Key::F6) {
            self.clear_keyframe();
        }
        // Unavailable items: consume so a shorter shortcut (F6, ⌘D, ⌘K …) does not run instead.
        for (m, key) in [
            (cmd, Key::W),
            (cmd, Key::P),
            (cmd | shift, Key::D),
            (cmd | shift, Key::W),
            (alt | shift | cmd, Key::R),
            (cmd, Key::Quote),
            (cmd | shift, Key::Quote),
            (cmd, Key::Semicolon),
            (cmd | shift, Key::Semicolon),
            (alt | cmd, Key::H),
            (alt | cmd, Key::M),
            (alt | cmd, Key::B),
            (alt | cmd, Key::K),
            (none, Key::F4),
            (none, Key::F1),
        ] {
            let _ = exact(m, key);
        }
    }

    pub fn move_key(&mut self, layer: usize, from: u32, to: u32, copy: bool) -> bool {
        let timeline = &self.symbol().timeline;
        let Some(l) = timeline.layers.get(layer) else {
            return false;
        };
        if layer_effectively_locked(timeline, l)
            || !layer_effectively_visible(timeline, l)
            || l.properties.role == LayerRole::Folder
            || to >= timeline.frame_count
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
        self.transact(|s| {
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
            if let Some(end) = &mut l.properties.end_frame {
                *end = (*end).max(to + 1);
            }
            if let Some(mut script) = script {
                let scripts = &mut s.symbol_mut().timeline.scripts;
                if !copy {
                    scripts.retain(|item| item.frame != from);
                }
                script.frame = to;
                scripts.push(script);
                scripts.sort_by_key(|item| item.frame);
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
        self.apply_frame_edit(TimelineEdit::Insert {
            layers: vec![self.symbol().timeline.layers[self.layer].id.clone()],
            at: (self.frame + 1).min(self.symbol().timeline.frame_count),
            count: 1,
            include_scripts: self.timeline_edit.include_scripts,
        });
    }

    pub fn draw_dialogs(&mut self, ctx: &egui::Context) {
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
                        |_| {},
                        |ui| close |= ui.add(Button::primary("Close").shortcut("↵")).clicked(),
                    );
                });
            let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if close || enter || modal.should_close() {
                self.show_about = false;
            }
        }
        if let Some(pending) = self.confirm {
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
                self.confirm = None;
                if action == 1 || (action == 0 && self.save(false)) {
                    self.perform(pending, ctx);
                }
            }
        }
    }
}

impl eframe::App for Editor {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.workspace.active_bottom = self.bottom;
        self.workspace.save(storage);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            self.finish_edits();
            if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Pending::Close);
            }
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
        self.normalize_selection();
        self.menu(ctx);
        self.status_bar(ctx);
        self.bitmap_textures.begin_frame(&self.project);
        self.prepare_docked_panel_focus(ctx);
        self.dock_grid(ctx);
        self.stage(ctx);
        self.draw_dialogs(ctx);
        self.symbol_creation_dialog(ctx);
        self.detached_panels(ctx);
        self.planned_panels(ctx);
        crate::native_windows::configure_panels();
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.project.name.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — NeoFlash",
            name,
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
