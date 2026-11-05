//! Document-owned editor state. The switcher remains gated by NF2 request R-035.
use crate::app::{Editor, Pending, ScriptTarget};
use eframe::egui::{self, Vec2};
use neoflash_core::{History, ObjectEvent, Project};
use std::path::{Path, PathBuf};

// A single field list makes capture/restore symmetric. The active slot is None: live state
// belongs exclusively to Editor, so a stale snapshot can never replace a saved document.
macro_rules! document_state {
    ($project:ident, $path:ident, $undo_levels:ident, $recovery:ident, $saved_project:ident, $root:ident; $($field:ident: $ty:ty = $default:expr),* $(,)?) => {
        pub(crate) struct DocumentSession { $(pub(crate) $field: $ty,)* }
        impl DocumentSession {
            fn new($project: Project, $path: Option<PathBuf>, $undo_levels: usize,
                $recovery: crate::recovery::RecoveryManager) -> Self {
                let $saved_project = Some($project.clone());
                let $root = $project.root.clone();
                Self { $($field: $default,)* }
            }
            fn exchange(&mut self, editor: &mut Editor) {
                $(std::mem::swap(&mut self.$field, &mut editor.$field);)*
                // Launch recovery decisions belong to the application, not a document.
                self.recovery.move_prompts_to(&mut editor.recovery);
            }
            pub(crate) fn dirty(&self) -> bool {
                self.saved_project.as_ref() != Some(&self.project)
            }
        }
    }
}
document_state! { project, path, undo_levels, recovery, saved_project, root;
    clipboard: crate::clipboard::ClipboardState = Default::default(),
    project: Project = project,
    history: History = History::new(undo_levels),
    path: Option<PathBuf> = path,
    saved_project: Option<Project> = saved_project,
    editing: Vec<String> = vec![root.clone()],
    frame: u32 = 0,
    layer: usize = 0,
    selection: crate::selection::Selection = crate::selection::Selection { symbol: root, targets: vec![] },
    zoom: f32 = 0.75,
    pan: Vec2 = Vec2::ZERO,
    onion: bool = false,
    onion_outlines: bool = false,
    onion_before: u32 = 3,
    onion_after: u32 = 3,
    onion_anchor: bool = false,
    anchored_range: (u32, u32) = (0, 6),
    script_target: ScriptTarget = ScriptTarget::Frame,
    script_event: ObjectEvent = ObjectEvent::Click,
    selected_source: Option<String> = None,
    code_feedback: Option<crate::sources::CodeFeedback> = None,
    actions_ui: crate::panels::actions_sidebar::ActionsUi = Default::default(),
    output: Vec<String> = vec![],
    timeline_edit: crate::timeline_edit::TimelineSelection = Default::default(),
    library_selected: Option<String> = None,
    library_panel: crate::library::LibraryPanel = Default::default(),
    resource_panel: crate::resources::ResourcePanel = Default::default(),
    project_panel: crate::panels::project::ProjectPanelState = Default::default(),
    compiler_errors: crate::panels::compiler_errors::CompilerErrorsState = Default::default(),
    movie_explorer: crate::panels::movie_explorer::MovieExplorerState = Default::default(),
    authoring_modify: crate::authoring_modify::AuthoringModify = Default::default(),
    vector: crate::vector::VectorState = Default::default(),
    stage_view: crate::stage_view::StageView = Default::default(),
    timeline_motion: crate::timeline_motion::MotionState = Default::default(),
    timeline_shape_hints: crate::timeline_shape_hints::ShapeHintsState = Default::default(),
    solo_layers: std::collections::HashMap<neoflash_core::Id, std::collections::BTreeSet<neoflash_core::Id>> = Default::default(),
    recovery: crate::recovery::RecoveryManager = recovery,
}

impl Editor {
    /// Replace only the active document after loading/validation has succeeded.
    pub(crate) fn install_document(
        &mut self,
        ctx: &egui::Context,
        project: Project,
        path: Option<PathBuf>,
        saved: bool,
    ) {
        self.cancel_edits();
        self.prepare_session_switch(ctx);
        self.clear_active_recovery();
        let recovery = self.recovery.for_document(path.as_deref());
        let mut incoming =
            DocumentSession::new(project, path, self.workspace.undo_levels as usize, recovery);
        incoming.stage_view.prefs = self.stage_view.prefs.clone();
        if !saved {
            incoming.saved_project = None;
        }
        incoming.exchange(self);
        self.session_installed(ctx);
    }

    #[allow(dead_code)] // No approved document switcher yet (R-035).
    pub(crate) fn session_count(&self) -> usize {
        self.sessions.len().max(1)
    }

    fn prepare_session_switch(&mut self, ctx: &egui::Context) {
        self.publish_controller.cancel();
        self.debugger.reset_document();
        self.finish_edits();
        // Keep document-scoped widget IDs, but never replay delayed clipboard input after a switch.
        let identity = self.clipboard.session.clone();
        self.clipboard = Default::default();
        self.clipboard.session = identity;
        self.cancel_guide_drag();
        self.set_playing(false);
        self.timeline_key_drag = None;
        self.document_settings = None;
        self.edit_envelope = None;
        self.find_replace = None;
        self.confirm_revert = false;
        self.symbol_conversion = None;
        self.graphic_drop_position = None;
        self.clear_simple_button_state();
        self.cancel_sound_properties(ctx);
        self.cancel_layer_properties(ctx);
        self.close_symbols_popover(ctx);
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    fn session_installed(&mut self, ctx: &egui::Context) {
        self.bitmap_textures = Default::default();
        self.timeline_thumbnails = Default::default();
        self.authored_preview = None;
        self.load_project_fonts(ctx);
        self.normalize_selection();
    }

    #[allow(dead_code)]
    pub(crate) fn new_document_session(&mut self, ctx: &egui::Context) -> usize {
        let mut project = Project::blank();
        if let Some(mut defaults) = self.workspace.document_defaults {
            defaults.sanitize();
            project.width = defaults.width;
            project.height = defaults.height;
            project.fps = defaults.fps;
            project.background = defaults.background;
        }
        let previous_count = self.session_count();
        let index = self.open_new_session_tab(ctx, project, None);
        if self.session_count() > previous_count
            && let Some(defaults) = self.workspace.document_defaults
        {
            self.stage_view.prefs.units = defaults.units;
        }
        index
    }

    #[allow(dead_code)]
    pub(crate) fn open_new_session_tab(
        &mut self,
        ctx: &egui::Context,
        project: Project,
        path: Option<PathBuf>,
    ) -> usize {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return self.active_session;
        }
        if let Err(error) = project.validate() {
            self.message(format!("Open failed: {error}"));
            return self.active_session;
        }
        if let Some(index) = path.as_deref().and_then(|path| self.session_for_path(path)) {
            self.switch_session_tab(ctx, index);
            return index;
        }
        self.prepare_session_switch(ctx);
        if self.sessions.is_empty() {
            self.sessions.push(None);
        }
        let recovery = self.recovery.for_document(path.as_deref());
        let mut incoming =
            DocumentSession::new(project, path, self.workspace.undo_levels as usize, recovery);
        incoming.stage_view.prefs = self.workspace.view_defaults.clone().unwrap_or_default();
        incoming.exchange(self);
        self.sessions[self.active_session] = Some(incoming);
        self.active_session = self.sessions.len();
        self.sessions.push(None);
        self.session_installed(ctx);
        self.active_session
    }

    #[allow(dead_code)]
    pub(crate) fn open_path_in_session(&mut self, ctx: &egui::Context, path: PathBuf) -> bool {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        if let Some(index) = self.session_for_path(&path) {
            self.switch_session_tab(ctx, index);
            return true;
        }
        // Parse and validate first. Failure cannot consume a draft, history or recovery.
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|raw| Project::from_json(&raw).map_err(|e| e.to_string()))
        {
            Ok(project) => {
                self.open_new_session_tab(ctx, project, Some(path.clone()));
                self.workspace.general.push_recent(&path);
                true
            }
            Err(error) => {
                self.message(format!("Open failed: {error}"));
                false
            }
        }
    }

    pub(crate) fn session_for_path(&self, path: &Path) -> Option<usize> {
        let same = |other: &Path| same_path(path, other);
        if self.path.as_deref().is_some_and(same) {
            return Some(self.active_session);
        }
        self.sessions.iter().position(|slot| {
            slot.as_ref()
                .and_then(|s| s.path.as_deref())
                .is_some_and(same)
        })
    }

    #[allow(dead_code)]
    pub(crate) fn switch_session_tab(&mut self, ctx: &egui::Context, index: usize) -> bool {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        self.switch_session(ctx, index)
    }

    fn switch_session(&mut self, ctx: &egui::Context, index: usize) -> bool {
        if index == self.active_session || index >= self.sessions.len() {
            return false;
        }
        self.prepare_session_switch(ctx);
        let Some(mut incoming) = self.sessions[index].take() else {
            return false;
        };
        incoming.exchange(self);
        self.sessions[self.active_session] = Some(incoming);
        self.active_session = index;
        self.session_installed(ctx);
        true
    }

    /// Request close; dirty inactive documents become the prompt's target. Cancel returns
    /// to the original document. A request being accepted does not imply it was discarded.
    #[allow(dead_code)]
    pub(crate) fn close_session_tab(&mut self, ctx: &egui::Context, index: usize) -> bool {
        if index >= self.session_count()
            || self.confirm.is_some()
            || self.close_origin.is_some()
            || self.timeline_edit.dragging()
        {
            return false;
        }
        self.close_origin = Some(self.active_session);
        self.switch_session(ctx, index);
        self.finish_edits();
        if self.dirty() {
            self.confirm = Some(Pending::CloseSession(index));
        } else {
            self.perform(Pending::CloseSession(index), ctx);
        }
        true
    }

    pub(crate) fn finish_session_close(&mut self, ctx: &egui::Context, index: usize) {
        if index != self.active_session {
            return;
        }
        self.clear_active_recovery();
        let origin = self.close_origin.take().unwrap_or(index);
        if self.sessions.len() <= 1 {
            self.sessions.clear();
            self.active_session = 0;
            self.perform(Pending::New, ctx);
            return;
        }
        self.sessions.remove(index);
        let next = if origin != index {
            origin - usize::from(origin > index)
        } else {
            index.min(self.sessions.len() - 1)
        };
        let mut incoming = self.sessions[next]
            .take()
            .expect("inactive document owns its state");
        self.prepare_session_switch(ctx);
        incoming.exchange(self);
        self.active_session = next;
        self.session_installed(ctx);
    }

    pub(crate) fn request_quit(&mut self, ctx: &egui::Context) {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return;
        }
        self.close_origin = Some(self.active_session);
        self.quit_approved.clear();
        self.finish_edits();
        self.continue_quit(ctx);
    }

    pub(crate) fn approve_quit_document(&mut self, ctx: &egui::Context) {
        self.close_origin.get_or_insert(self.active_session);
        self.quit_approved.push(self.active_session);
        self.continue_quit(ctx);
    }

    fn continue_quit(&mut self, ctx: &egui::Context) {
        let next = (0..self.session_count()).find(|index| {
            !self.quit_approved.contains(index)
                && if *index == self.active_session {
                    self.dirty()
                } else {
                    self.sessions[*index]
                        .as_ref()
                        .is_some_and(DocumentSession::dirty)
                }
        });
        if let Some(index) = next {
            self.switch_session(ctx, index);
            self.confirm = Some(Pending::Close);
        } else {
            // Discard is deferred until every document has a decision: a later Cancel is lossless.
            self.clear_all_session_recovery();
            self.close_origin = None;
            self.quit_approved.clear();
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(crate) fn cancel_confirmation(&mut self, ctx: &egui::Context) {
        self.confirm = None;
        self.quit_approved.clear();
        if let Some(origin) = self.close_origin.take() {
            self.switch_session(ctx, origin);
        }
    }

    pub(crate) fn continue_confirmation(&mut self, ctx: &egui::Context) {
        if let Some(pending) = self.confirm.take() {
            self.perform(pending, ctx);
        }
    }

    pub(crate) fn clear_all_session_recovery(&mut self) {
        let protected = self.recovery.pending_ids();
        self.clear_active_recovery();
        let errors: Vec<_> = self
            .sessions
            .iter_mut()
            .flatten()
            .flat_map(|s| s.recovery.clear_active_except(&protected))
            .collect();
        for error in errors {
            self.message(format!("Recovery cleanup failed: {error}"));
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let absolute = |p: &Path| {
        std::fs::canonicalize(p).unwrap_or_else(|_| {
            if p.is_absolute() {
                p.to_owned()
            } else {
                std::env::current_dir().unwrap_or_default().join(p)
            }
        })
    };
    absolute(a) == absolute(b)
}
