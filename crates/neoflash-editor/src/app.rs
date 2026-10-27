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
    pub playing: bool,
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
    pub bottom: BottomTab,
    pub script_target: ScriptTarget,
    pub script_event: ObjectEvent,
    pub selected_source: Option<String>,
    pub code_feedback: Option<crate::sources::CodeFeedback>,
    pub status: String,
    pub output: Vec<String>,
    pub fonts: Fonts,
    pub font_filter: String,
    pub show_about: bool,
    pub confirm: Option<Pending>,
    pub allow_close: bool,
    pub timeline_cell: f32,
    pub timeline_key_drag: Option<(String, String, u32)>,
    pub library_selected: Option<String>,
    pub resource_panel: crate::resources::ResourcePanel,
    pub bitmap_textures: crate::bitmaps::BitmapTextures,
    pub vector: crate::vector::VectorState,
    pub clipboard: crate::clipboard::ClipboardState,
    pub workspace: WorkspacePrefs,
    pub detached: crate::detached::DetachedState,
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
        s.workspace = WorkspacePrefs::load(cc.storage);
        s.workspace.theme.apply(&cc.egui_ctx);
        s.bottom = s.workspace.active_bottom;
        if let Some(path) = std::env::args().nth(1) {
            s.open_path(PathBuf::from(path), &cc.egui_ctx);
        }
        s
    }
    pub(crate) fn from_project(ctx: &egui::Context, project: Project) -> Self {
        WorkspacePrefs::default().theme.apply(ctx);
        let saved_project = Some(project.clone());
        let root = project.root.clone();
        let mut s = Self {
            project, history: History::new(150), path: None, saved_project,
            editing: vec![root.clone()], frame: 0, layer: 0, selection: crate::selection::Selection { symbol: root, targets: vec![] }, object_gesture: None, tool: Tool::Select,
            playing: false, last_tick: Instant::now(), accumulator: 0.,
            onion: false, onion_outlines: false, onion_before: 3, onion_after: 3,
            onion_anchor: false, anchored_range: (0, 6), onion_opacity: 0.3,
            zoom: 0.75, pan: Vec2::ZERO, shape_draft: None,
            fill: [50, 117, 184, 255], bottom: BottomTab::Properties,
            script_target: ScriptTarget::Frame, script_event: ObjectEvent::Click,
            selected_source: None, code_feedback: None,
            status: "Ready · F5 frame · F6 keyframe · F7 blank keyframe · ⌘↵ Test Movie".into(),
            output: vec!["NeoFlash development preview. Editor playback shows authored animation; Test Movie executes scripts in the browser.".into()],
            fonts: Fonts::new(), font_filter: String::new(), show_about: false, confirm: None, allow_close: false,
            timeline_cell: 13., timeline_key_drag: None, library_selected: None,
            resource_panel: crate::resources::ResourcePanel::default(),
            bitmap_textures: crate::bitmaps::BitmapTextures::default(),
            vector: crate::vector::VectorState::default(),
            clipboard: crate::clipboard::ClipboardState::default(),
            workspace: WorkspacePrefs::default(),
            detached: crate::detached::DetachedState::default(),
        };
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
    pub(crate) fn set_panel_visible(&mut self, panel: PanelId, visible: bool) {
        self.workspace.active_bottom = self.bottom;
        self.workspace.set_panel_visible(panel, visible);
        self.bottom = self.workspace.active_bottom;
    }
    pub(crate) fn restore_workspace(&mut self, ctx: &egui::Context) {
        self.workspace.restore_layout();
        self.detached = crate::detached::DetachedState::default();
        self.bottom = self.workspace.active_bottom;
        // Reset only the panel geometry that we own. Editor drafts, focus and
        // unrelated egui memory are not a workspace layout preference.
        ctx.data_mut(|data| {
            for id in ["timeline", "tools", "library", "properties"] {
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
        self.finish_selection_gesture();
        self.shape_draft = None;
        self.timeline_key_drag = None;
        self.finish_point_drag();
        self.history.begin(&self.project);
        f(self);
        self.history.commit(&self.project);
    }
    pub fn normalize_selection(&mut self) {
        self.cancel_stale_selection_gesture();
        self.editing.retain(|id| self.project.symbol(id).is_some());
        if self.editing.is_empty() {
            self.editing.push(self.project.root.clone());
        }
        if self.symbol().timeline.layers.is_empty() {
            self.symbol_mut().timeline.layers.push(Layer {
                id: new_id("layer"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![],
                    tween: None,
                    label: String::new(),
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
            .is_some_and(|l| !l.locked && l.visible)
    }
    pub fn make_key(&mut self, blank: bool) {
        self.finish_edits();
        if !self.layer_editable() {
            return;
        }
        self.transact(|s| s.ensure_key(blank));
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
                ElementKind::Path { .. } => "path",
                ElementKind::Bitmap { .. } => "bitmap",
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
        self.finish_edits();
        if !self.selected_editable() {
            return;
        }
        let Some(element) = self.selected_sampled() else {
            return;
        };
        self.transact(|s| {
            s.ensure_selected_key();
            let id = new_id("symbol");
            let mut content = element.clone();
            content.transform = Transform::default();
            content.id = new_id("object");
            content.scripts.clear();
            content.accessibility = None;
            s.project.symbols.push(Symbol {
                class_name: None,
                linkage_id: None,
                id: id.clone(),
                name: format!("{} symbol", element.name),
                kind: SymbolKind::MovieClip,
                timeline: Timeline {
                    frame_count: 24,
                    layers: vec![Layer {
                        id: new_id("layer"),
                        name: "Layer 1".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![content],
                            tween: None,
                            label: String::new(),
                        }],
                    }],
                    scripts: vec![],
                },
            });
            if let Some(e) = s.selected_element_mut() {
                e.kind = ElementKind::Instance {
                    symbol_id: id.clone(),
                    playback: InstancePlayback::Independent,
                };
            }
            s.library_selected = Some(id);
        });
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
        self.clipboard_events(ctx);
        let typing = ctx.wants_keyboard_input();
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
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
            self.show_panel(PanelId::Actions);
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
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F8)) {
                self.convert_to_symbol();
            }
            if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
                self.delete_selected();
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                let captured = self.object_gesture.is_some()
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
            ] {
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                    self.set_tool(tool);
                }
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
                self.finish_edits();
                self.frame = self.frame.saturating_sub(1);
            }
            if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
                self.finish_edits();
                self.frame = (self.frame + 1).min(self.symbol().timeline.frame_count - 1);
            }
        }
    }

    pub fn move_key(&mut self, layer: usize, from: u32, to: u32, copy: bool) -> bool {
        let timeline = &self.symbol().timeline;
        let Some(l) = timeline.layers.get(layer) else {
            return false;
        };
        if l.locked || !l.visible || to >= timeline.frame_count || from == to {
            return false;
        }
        if l.keyframes.iter().any(|k| k.frame == to) {
            self.message("A keyframe already exists at the destination.");
            return false;
        }
        let Some(mut key) = l.keyframes.iter().find(|k| k.frame == from).cloned() else {
            return false;
        };
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
            s.frame = to;
            s.layer = layer;
            s.clear_selection();
        });
        true
    }

    pub fn insert_frame(&mut self) {
        self.finish_edits();
        if !self.layer_editable() {
            return;
        }
        self.transact(|s| {
            let frame = s.frame;
            let layer = s.layer;
            let t = &mut s.symbol_mut().timeline;
            for k in &mut t.layers[layer].keyframes {
                if k.frame > frame {
                    k.frame += 1;
                }
            }
            t.frame_count += 1;
        });
    }
    pub fn draw_dialogs(&mut self, ctx: &egui::Context) {
        if self.show_about {
            egui::Window::new("About NeoFlash")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .show(ctx, |ui| {
                    ui.heading("NeoFlash 0.1 — development preview");
                    ui.label("Classic Flash workflow. Rust core. HTML5 publishing.");
                    ui.label("Open source · MIT OR Apache-2.0");
                    ui.separator();
                    ui.label(
                        "This is an early authoring foundation, not complete Flash compatibility.",
                    );
                    ui.label("See docs/COMPATIBILITY.md and docs/ROADMAP.md for the exact scope.");
                });
        }
        if let Some(pending) = self.confirm {
            let mut action = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Save changes to this project before continuing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            action = Some(0);
                        }
                        if ui.button("Discard").clicked() {
                            action = Some(1);
                        }
                        if ui.button("Cancel").clicked() {
                            action = Some(2);
                        }
                    });
                });
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
        if self.playing {
            let elapsed = self.last_tick.elapsed().as_secs_f64().min(0.25);
            self.last_tick = Instant::now();
            self.accumulator += elapsed;
            let step = 1. / self.project.fps.max(1) as f64;
            while self.accumulator >= step {
                self.frame = (self.frame + 1) % self.symbol().timeline.frame_count.max(1);
                self.accumulator -= step;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(8));
        }
        self.normalize_selection();
        self.menu(ctx);
        self.timeline(ctx);
        self.tools(ctx);
        self.library(ctx);
        self.prepare_docked_panel_focus(ctx);
        self.bottom_panel(ctx);
        self.stage(ctx);
        self.draw_dialogs(ctx);
        self.detached_panels(ctx);
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
    }
}

fn reaches(
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
