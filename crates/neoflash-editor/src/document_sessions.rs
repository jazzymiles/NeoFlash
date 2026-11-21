//! Document-owned state and lossless multi-document close sequences (R-035).
use crate::app::{Editor, Pending, ScriptTarget};
use eframe::egui::{self, Vec2};
use neoflash_core::{History, ObjectEvent, Project};
use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::io::Read as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirtyCacheEntry {
    history_revision: u64,
    undo_len: usize,
    redo_len: usize,
    can_undo: bool,
    saved_symbols_ptr: usize,
    saved_resources_ptr: usize,
    saved_sources_ptr: usize,
    saved_header_hash: u64,
    project_header_hash: u64,
    ui_pass_if_in_flight: u64,
    is_dirty: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DirtyCacheState {
    entry: Option<DirtyCacheEntry>,
    #[cfg(test)]
    pub(crate) full_comparisons: usize,
}

impl DirtyCacheState {
    pub(crate) fn invalidate(cache: &Cell<Self>) {
        let mut state = cache.get();
        state.entry = None;
        cache.set(state);
    }
}

fn fast_project_header_hash(project: &Project, active_symbol_id: Option<&str>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    project.format_version.hash(&mut h);
    project.name.hash(&mut h);
    project.width.hash(&mut h);
    project.height.hash(&mut h);
    project.fps.hash(&mut h);
    project.background.hash(&mut h);
    project.root.hash(&mut h);
    project.document_class.hash(&mut h);
    project
        .script_language
        .as_ref()
        .map(std::mem::discriminant)
        .hash(&mut h);
    project.also_compile_compatible_scripts.hash(&mut h);
    std::mem::discriminant(&project.html_render_mode()).hash(&mut h);
    project.library_folders.len().hash(&mut h);
    project.scenes.len().hash(&mut h);
    project.sources.len().hash(&mut h);
    if !project.sources.is_empty() {
        (project.sources.as_ptr() as usize).hash(&mut h);
    }
    project.resources.len().hash(&mut h);
    if !project.resources.is_empty() {
        (project.resources.as_ptr() as usize).hash(&mut h);
    }
    project.symbols.len().hash(&mut h);
    if !project.symbols.is_empty() {
        (project.symbols.as_ptr() as usize).hash(&mut h);
    }
    let hash_symbol_shallow =
        |sym: &neoflash_core::Symbol, h: &mut std::collections::hash_map::DefaultHasher| {
            sym.id.hash(h);
            sym.name.hash(h);
            sym.folder.hash(h);
            std::mem::discriminant(&sym.kind).hash(h);
            sym.class_name.hash(h);
            sym.linkage_id.hash(h);
            sym.timeline.frame_count.hash(h);
            sym.timeline.layers.len().hash(h);
            sym.timeline.scripts.len().hash(h);
            if let Some(layer) = sym.timeline.layers.first() {
                layer.id.hash(h);
                layer.name.hash(h);
                layer.visible.hash(h);
                layer.locked.hash(h);
                layer.keyframes.len().hash(h);
                if let Some(kf) = layer.keyframes.first() {
                    kf.frame.hash(h);
                    kf.elements.len().hash(h);
                }
            }
        };
    if let Some(first) = project.symbols.first() {
        hash_symbol_shallow(first, &mut h);
    }
    if let Some(sym_id) = active_symbol_id
        && project.symbols.first().is_none_or(|f| f.id != sym_id)
        && let Some(sym) = project.symbol(sym_id)
    {
        hash_symbol_shallow(sym, &mut h);
    }
    h.finish()
}

pub(crate) fn compute_dirty_cached(
    project: &Project,
    saved_project: Option<&Project>,
    history: &History,
    active_symbol_id: Option<&str>,
    ui_pass_if_in_flight: Option<u64>,
    cache: &Cell<DirtyCacheState>,
) -> bool {
    let Some(saved) = saved_project else {
        let mut state = cache.get();
        state.entry = None;
        cache.set(state);
        return true;
    };
    if saved.symbols.is_empty() || project.symbols.is_empty() {
        return saved != project;
    }
    let Some(ui_pass_tag) = ui_pass_if_in_flight else {
        let mut state = cache.get();
        state.entry = None;
        #[cfg(test)]
        {
            state.full_comparisons += 1;
        }
        cache.set(state);
        return saved != project;
    };

    let saved_symbols_ptr = saved.symbols.as_ptr() as usize;
    let saved_resources_ptr = if saved.resources.is_empty() {
        0
    } else {
        saved.resources.as_ptr() as usize
    };
    let saved_sources_ptr = if saved.sources.is_empty() {
        0
    } else {
        saved.sources.as_ptr() as usize
    };
    let history_revision = history.revision();
    let undo_len = history.undo_len();
    let redo_len = history.redo_len();
    let can_undo = history.can_undo();
    let saved_header_hash = fast_project_header_hash(saved, active_symbol_id);
    let project_header_hash = fast_project_header_hash(project, active_symbol_id);

    let mut state = cache.get();
    if let Some(prev) = state.entry
        && prev.history_revision == history_revision
        && prev.undo_len == undo_len
        && prev.redo_len == redo_len
        && prev.can_undo == can_undo
        && prev.saved_symbols_ptr == saved_symbols_ptr
        && prev.saved_resources_ptr == saved_resources_ptr
        && prev.saved_sources_ptr == saved_sources_ptr
        && prev.saved_header_hash == saved_header_hash
        && prev.project_header_hash == project_header_hash
        && prev.ui_pass_if_in_flight == ui_pass_tag
    {
        return prev.is_dirty;
    }

    let is_dirty = saved != project;
    #[cfg(test)]
    {
        state.full_comparisons += 1;
    }
    state.entry = Some(DirtyCacheEntry {
        history_revision,
        undo_len,
        redo_len,
        can_undo,
        saved_symbols_ptr,
        saved_resources_ptr,
        saved_sources_ptr,
        saved_header_hash,
        project_header_hash,
        ui_pass_if_in_flight: ui_pass_tag,
        is_dirty,
    });
    cache.set(state);
    is_dirty
}

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
                let mut session = Self { $($field: $default,)* };
                session.output.set_document_id(session.clipboard.session.clone());
                session
            }
            fn exchange(&mut self, editor: &mut Editor) {
                $(std::mem::swap(&mut self.$field, &mut editor.$field);)*
                // Launch recovery decisions belong to the application, not a document.
                self.recovery.move_prompts_to(&mut editor.recovery);
            }
            pub(crate) fn dirty(&self) -> bool {
                compute_dirty_cached(
                    &self.project,
                    self.saved_project.as_ref(),
                    &self.history,
                    self.edit_session.current_symbol(),
                    Some(0),
                    &self.dirty_cache,
                )
            }
            pub(crate) fn tracked_disk_path(&self) -> Option<&Path> {
                self.path.as_deref().or(self.import_source.as_deref())
            }
            pub(crate) fn document_display_name(&self) -> String {
                self.tracked_disk_path()
                    .and_then(|p| p.file_name())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.project.name.clone())
            }
            #[cfg_attr(not(test), allow(dead_code))]
            pub(crate) fn keep_editing_external_change(&mut self) {
                self.recovery.acknowledge_external_change();
                self.saved_project = None;
                DirtyCacheState::invalidate(&self.dirty_cache);
            }
            #[cfg_attr(not(test), allow(dead_code))]
            #[allow(clippy::too_many_arguments)]
            pub(crate) fn new_for_reload(
                project: Project,
                path: Option<PathBuf>,
                import_source: Option<PathBuf>,
                standalone_script: bool,
                undo_levels: usize,
                recovery: crate::recovery::RecoveryManager,
                onion_before: u32,
                onion_after: u32,
                stage_prefs: crate::stage_view::ViewPrefs,
            ) -> Self {
                let mut session = Self::new(project, path, undo_levels, recovery);
                if session.path.is_none() {
                    session.saved_project = None;
                }
                session.import_source = import_source;
                session.standalone_script = standalone_script;
                if standalone_script {
                    session.script_target = ScriptTarget::Source;
                    session.selected_source = session.project.sources.first().map(|u| u.id.clone());
                }
                session.onion_before = onion_before;
                session.onion_after = onion_after;
                session.stage_view.prefs = stage_prefs;
                session
            }
        }
    }
}
document_state! { project, path, undo_levels, recovery, saved_project, root;
    clipboard: crate::clipboard::ClipboardState = Default::default(),
    project: Project = project,
    history: History = History::new(undo_levels),
    path: Option<PathBuf> = path,
    import_source: Option<PathBuf> = None,
    standalone_script: bool = false,
    saved_project: Option<Project> = saved_project,
    dirty_cache: Cell<DirtyCacheState> = Cell::new(DirtyCacheState::default()),
    edit_session: crate::symbol_edit::SymbolEditSession = crate::symbol_edit::SymbolEditSession::for_scene(root.clone(), 0.75, Vec2::ZERO),
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
    debugger: crate::debugger::Debugger = Default::default(),
    actions_ui: crate::panels::actions_sidebar::ActionsUi = Default::default(),
    output: crate::output_pipeline::OutputPipeline = Default::default(),
    test_movie_preview_open: bool = false,
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
    ai_session: crate::panels::ai_dialogs::AiDocumentSession = Default::default(),
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
        let replaced_session_key = self.clipboard.session.clone();
        self.cancel_edits();
        self.prepare_session_switch(ctx);
        self.clear_active_recovery();
        let recovery = self.recovery.for_document(path.as_deref());
        let mut incoming =
            DocumentSession::new(project, path, self.workspace.undo_levels as usize, recovery);
        [incoming.onion_before, incoming.onion_after] = self.workspace.onion_range;
        incoming.stage_view.prefs = self.stage_view.prefs.clone();
        if !saved {
            incoming.saved_project = None;
        }
        incoming.exchange(self);
        if self.sessions.is_empty() {
            self.sessions.push(None);
            self.active_session = 0;
        }
        self.session_installed(ctx);
        if replaced_session_key != self.clipboard.session {
            self.remove_closed_session_aux_state(&replaced_session_key);
        }
        self.sync_session_tabs_to_prefs();
    }

    /// Synchronize `WorkspacePrefs::general.{session_files, active_session_file}` with the
    /// currently open document sessions in left-to-right tab order (`T-231`).
    pub(crate) fn sync_session_tabs_to_prefs(&mut self) {
        if self.sessions.is_empty() {
            self.workspace.general.set_session_files(&[], None);
            return;
        }
        let mut paths = Vec::with_capacity(self.sessions.len());
        for index in 0..self.sessions.len() {
            let tracked = if index == self.active_session {
                self.tracked_disk_path().map(Path::to_path_buf)
            } else {
                self.sessions[index]
                    .as_ref()
                    .and_then(DocumentSession::tracked_disk_path)
                    .map(Path::to_path_buf)
            };
            if let Some(p) = tracked {
                paths.push(p);
            }
        }
        let preferred_idx = self.quit_origin_session.unwrap_or(self.active_session);
        let active_path = if preferred_idx == self.active_session {
            self.tracked_disk_path().map(Path::to_path_buf)
        } else {
            self.sessions
                .get(preferred_idx)
                .and_then(|s| s.as_ref())
                .and_then(DocumentSession::tracked_disk_path)
                .map(Path::to_path_buf)
                .or_else(|| self.tracked_disk_path().map(Path::to_path_buf))
        };
        self.workspace
            .general
            .set_session_files(&paths, active_path.as_deref());
    }

    pub(crate) fn has_document(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub(crate) fn session_count(&self) -> usize {
        self.sessions.len()
    }

    fn prepare_session_switch(&mut self, ctx: &egui::Context) {
        self.publish_controller.cancel();
        self.bitmap_trace.cancel();
        self.publish_settings = None;
        self.export_lottie = None;
        self.last_lottie_report = None;
        if let Some(d) = &self.export_video {
            d.cancel_worker();
        }
        self.export_video = None;
        self.last_video_report = None;
        self.debugger.inline_edit = None;
        self.finish_edits();
        self.authoring_tools.reset_document_transient();
        self.exit_stage_text_edit();
        self.authoring_modify.distort_session = None;
        self.authoring_modify.envelope_session = None;
        self.authoring_modify.scale_and_rotate_dialog = None;
        self.library_panel.drag = None;
        self.timeline_edit.cancel_gesture();
        self.timeline_motion.paste_special_open = false;
        // Keep document-scoped widget IDs, but never replay delayed clipboard input after a switch.
        let identity = self.clipboard.session.clone();
        self.clipboard = Default::default();
        self.clipboard.session = identity;
        self.scale9_guide_drag = None;
        self.cancel_guide_drag();
        self.set_playing(false);
        self.timeline_audio.stop_all();
        self.timeline_video.follow_preview(None);
        self.timeline_key_drag = None;
        self.document_settings = None;
        self.edit_envelope = None;
        self.find_replace = None;
        self.confirm_revert = false;
        self.symbol_conversion = None;
        self.graphic_drop_position = None;
        self.stage_cursor = None;
        self.shown_stage_cursor = None;
        self.clear_simple_button_state();
        self.clear_live_preview_pointer_state();
        self.cancel_bitmap_properties(ctx);
        self.cancel_export_audio(ctx);
        self.cancel_export_image(ctx);
        self.cancel_import_video(ctx);
        self.cancel_stroke_style(ctx);
        self.cancel_font_embedding(ctx);
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
        self.sync_output_document_id();
        self.bitmap_textures = Default::default();
        self.video_textures = Default::default();
        self.timeline_thumbnails = Default::default();
        self.authored_preview = None;
        crate::panels::publish_settings::sync_document_publish_preferences(
            &mut self.workspace.publish,
            &self.project,
        );
        self.sync_runtime_breakpoints_from_session();
        self.load_project_fonts(ctx);
        self.normalize_selection();
    }

    pub(crate) fn new_document_session(&mut self, ctx: &egui::Context) -> usize {
        let mut project = Project::blank();
        // A newly created file starts with exactly one blank keyframe and one frame of
        // duration. `Project::blank()` remains the broad 120-frame fixture used by the demo and
        // model/editor tests; the product's File ▸ New contract is intentionally narrower.
        if let Some(root) = project.symbol_mut(&project.root.clone()) {
            root.timeline.frame_count = 1;
        }
        let mut defaults = self.workspace.document_defaults.unwrap_or_default();
        defaults.sanitize();
        project.width = defaults.width;
        project.height = defaults.height;
        project.fps = defaults.fps;
        project.background = defaults.background;
        project.script_language = Some(defaults.script_language);
        project.also_compile_compatible_scripts = defaults.also_compile_compatible_scripts;
        crate::timeline_commands::upgrade_all_format_versions(&mut project);
        let previous_count = self.session_count();
        let index = self.open_new_session_tab(ctx, project, None);
        if self.session_count() > previous_count
            && let Some(defaults) = self.workspace.document_defaults
        {
            self.stage_view.prefs.units = defaults.units;
        }
        index
    }

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
        let had_document = self.has_document();
        let recovery = self.recovery.for_document(path.as_deref());
        let mut incoming =
            DocumentSession::new(project, path, self.workspace.undo_levels as usize, recovery);
        [incoming.onion_before, incoming.onion_after] = self.workspace.onion_range;
        incoming.stage_view.prefs = self.workspace.view_defaults.clone().unwrap_or_default();
        incoming.exchange(self);
        if had_document {
            self.sessions[self.active_session] = Some(incoming);
            self.active_session += 1;
            self.sessions.insert(self.active_session, None);
            self.library_panel
                .rebase_session_source_after_insert(self.active_session);
            for session in self.sessions.iter_mut().flatten() {
                session
                    .library_panel
                    .rebase_session_source_after_insert(self.active_session);
            }
        } else {
            self.active_session = 0;
            self.sessions.push(None);
        }
        self.session_installed(ctx);
        self.sync_session_tabs_to_prefs();
        self.active_session
    }

    pub(crate) fn open_path_in_session(&mut self, ctx: &egui::Context, path: PathBuf) -> bool {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return false;
        }
        if let Some(index) = self.session_for_open_path(&path) {
            self.switch_session_tab(ctx, index);
            return true;
        }
        if Self::is_actionscript_source_path(&path) {
            return self.open_standalone_script_session(ctx, path);
        }
        if Self::is_non_native_graphic_document_path(&path) || Self::is_legacy_flash_path(&path) {
            return self.try_import_legacy(path, ctx);
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
                let is_json_ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
                if is_json_ext
                    && let Ok(report) = neoflash_import::inspect(&path)
                    && report.format == neoflash_import::ImportFormat::Figma
                {
                    return self.try_import_legacy(path, ctx);
                }
                self.message(format!("Open failed: {error}"));
                false
            }
        }
    }

    pub(crate) fn session_for_path(&self, path: &Path) -> Option<usize> {
        let same = |other: &Path| same_path(path, other);
        if self.has_document() && self.path.as_deref().is_some_and(same) {
            return Some(self.active_session);
        }
        self.sessions.iter().position(|slot| {
            slot.as_ref()
                .and_then(|s| s.path.as_deref())
                .is_some_and(same)
        })
    }

    /// Open/Import recognizes the source of an imported document without treating it as a save
    /// destination. Save conflicts and recovery continue to use only `session_for_path`.
    pub(crate) fn session_for_open_path(&self, path: &Path) -> Option<usize> {
        self.session_for_path(path).or_else(|| {
            let target = canonical_open_identity(path);
            let same = |other: &Path| canonical_open_identity(other) == target;
            if self.has_document() && self.import_source.as_deref().is_some_and(same) {
                return Some(self.active_session);
            }
            self.sessions.iter().position(|slot| {
                slot.as_ref()
                    .and_then(|session| session.import_source.as_deref())
                    .is_some_and(same)
            })
        })
    }

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
        self.sync_session_tabs_to_prefs();
        true
    }

    /// Dirty targets become active before prompting. Cancel leaves the target selected.
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
        let closed_session_key = self.clipboard.session.clone();
        self.ai_session.cancel_and_clear();
        self.clear_active_recovery();
        let origin = self.close_origin.take().unwrap_or(index);
        if self.sessions.len() <= 1 {
            // Keep a valid inert backing model for existing panel APIs; it is not an open session.
            self.install_document(ctx, Project::blank(), None, true);
            self.sessions.clear();
            self.active_session = 0;
            self.remove_closed_session_aux_state(&closed_session_key);
            self.sync_session_tabs_to_prefs();
            self.message("Document closed.");
            return;
        }
        self.sessions.remove(index);
        self.library_panel.rebase_session_source_after_remove(index);
        for session in self.sessions.iter_mut().flatten() {
            session
                .library_panel
                .rebase_session_source_after_remove(index);
        }
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
        drop(incoming);
        self.active_session = next;
        self.session_installed(ctx);
        self.remove_closed_session_aux_state(&closed_session_key);
        self.sync_session_tabs_to_prefs();
    }

    pub(crate) fn remove_closed_session_aux_state(&self, closed_session_key: &str) {
        self.remove_debugger_authoring_state(closed_session_key);
        crate::layer_controls::prune_closed_session_layer_state(closed_session_key);
    }

    pub(crate) fn cycle_document(&mut self, ctx: &egui::Context, previous: bool) {
        let count = self.session_count();
        if count > 1 {
            let index = (self.active_session + if previous { count - 1 } else { 1 }) % count;
            self.switch_session_tab(ctx, index);
        }
    }

    pub(crate) fn request_close_documents(&mut self, ctx: &egui::Context, others: bool) {
        self.start_close_sequence(ctx, false, others);
    }

    pub(crate) fn request_quit(&mut self, ctx: &egui::Context) {
        self.start_close_sequence(ctx, true, false);
    }

    fn start_close_sequence(&mut self, ctx: &egui::Context, quit: bool, others: bool) {
        if self.confirm.is_some() || self.close_origin.is_some() || self.timeline_edit.dragging() {
            return;
        }
        self.finish_edits();
        self.close_origin = Some(self.active_session);
        if quit {
            self.quit_origin_session = Some(self.active_session);
        }
        self.close_targets = (0..self.session_count())
            .filter(|index| !others || *index != self.active_session)
            .collect();
        self.close_quit = quit;
        self.quit_approved.clear();
        self.continue_close_sequence(ctx);
    }

    pub(crate) fn approve_quit_document(&mut self, ctx: &egui::Context) {
        self.quit_approved.push(self.active_session);
        self.continue_close_sequence(ctx);
    }

    fn continue_close_sequence(&mut self, ctx: &egui::Context) {
        let next = self.close_targets.iter().copied().find(|index| {
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
            self.confirm = Some(if self.close_quit {
                Pending::Close
            } else {
                Pending::CloseSessions
            });
            return;
        }
        // Discard is deferred until every target has a decision: a later Cancel is lossless.
        self.close_origin = None;
        self.quit_approved.clear();
        if self.close_quit {
            self.sync_session_tabs_to_prefs();
            self.clear_all_session_recovery();
            self.close_targets.clear();
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            let targets = std::mem::take(&mut self.close_targets);
            for index in targets.into_iter().rev() {
                self.switch_session(ctx, index);
                self.finish_session_close(ctx, index);
            }
        }
    }

    pub(crate) fn cancel_confirmation(&mut self, _ctx: &egui::Context) {
        self.confirm = None;
        self.quit_approved.clear();
        self.close_targets.clear();
        self.close_quit = false;
        self.close_origin = None;
        self.quit_origin_session = None;
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

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    crate::recovery::canonical_or_normalized_path(a)
        == crate::recovery::canonical_or_normalized_path(b)
}

fn canonical_open_identity(path: &Path) -> PathBuf {
    let canon = crate::recovery::canonical_or_normalized_path(path);
    let is_dom = canon
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("DOMDocument.xml"));
    let is_marker = canon
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("xfl"))
        && canon.is_file()
        && std::fs::File::open(&canon).is_ok_and(|mut file| {
            let mut header = [0; 9];
            file.read_exact(&mut header).is_ok() && &header == b"PROXY-CS5"
        });
    if (is_dom || is_marker)
        && let Some(parent) = canon.parent()
    {
        return parent.to_owned();
    }
    canon
}
