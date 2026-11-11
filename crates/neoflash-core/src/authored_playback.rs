//! Stateful authoring transport using the shared timeline clocks. Supports both
//! classic Flash script-ignore transport and live AS2/AS3/JS preview (`trace()`,
//! timeline control, instance handlers and display overrides) while keeping the
//! authored document pure.
use crate::{
    AccessibleRole, AuthoredScene, ElementKind, Id, PlaybackCommand, Player, Project,
    ScriptDispatch, ScriptEvent, SymbolKind, VideoCommand,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegisteredEvent {
    Click,
    EnterFrame,
}

#[derive(Clone, Debug)]
struct RegisteredHandler {
    target_path: Vec<Id>,
    target_lifetime: u64,
    event: RegisteredEvent,
    body: String,
    origin: String,
    base_line: usize,
}

const MAX_PREVIEW_CALL_DEPTH: usize = 32;
const MAX_PREVIEW_STEPS_PER_PASS: usize = 25_000;

/// Independent MovieClips advance even when their parent has only one frame.
/// Pausing means retaining this value without calling `tick`; recreating it after
/// a scrub/edit starts a fresh placement lifetime. Artwork and source stay pure.
pub struct AuthoredPlayback {
    symbol: Id,
    player: Player,
    button_root: bool,
    looping: bool,
    live_scripts: bool,
    script_halted_root: bool,
    traces: Vec<String>,
    diagnostics: Vec<String>,
    reported_diagnostics: HashSet<String>,
    vars: HashMap<(Vec<Id>, String), String>,
    registered_handlers: Vec<RegisteredHandler>,
    call_depth: usize,
    steps_remaining: usize,
}

impl AuthoredPlayback {
    pub fn new(project: &Project, symbol: &str, start_frame: u32) -> Self {
        Self::with_live_scripts(project, symbol, start_frame, false)
    }

    pub fn with_live_scripts(
        project: &Project,
        symbol: &str,
        start_frame: u32,
        live_scripts: bool,
    ) -> Self {
        // A scoped player shares the runtime's placement clocks but does not
        // prepare bitmap pixels. Native textures consume immutable resources.
        let mut player = Player::with_source_root(project, Some(symbol.to_owned()));
        if start_frame != 0 {
            // Sounds and scripts of frame 0 belong to the discarded start, not to this preview.
            player.drain_sound_commands();
            player.drain_video_commands();
            player.drain_scripts();
            player.runtime_scripts.clear();
            player.seek(project, start_frame);
        }
        let mut preview = Self {
            symbol: symbol.to_owned(),
            player,
            button_root: project
                .symbol(symbol)
                .is_some_and(|symbol| symbol.kind == SymbolKind::Button),
            looping: true,
            live_scripts,
            script_halted_root: false,
            traces: Vec::new(),
            diagnostics: Vec::new(),
            reported_diagnostics: HashSet::new(),
            vars: HashMap::new(),
            registered_handlers: Vec::new(),
            call_depth: 0,
            steps_remaining: MAX_PREVIEW_STEPS_PER_PASS,
        };
        preview.process_scripts(project);
        preview
    }

    pub fn set_live_scripts(&mut self, project: &Project, live_scripts: bool) {
        if self.live_scripts == live_scripts {
            return;
        }
        // A script can pause or seek any nested MovieClip clock. Clearing only the root halt bit,
        // interpreter variables and display overrides leaves those child clocks contaminated.
        // Rebuild the complete scoped player at the same authored root frame so both directions of
        // the toggle start with clean placement lifetimes.
        let frame = self.frame();
        let looping = self.looping;
        let mut prior_traces = std::mem::take(&mut self.traces);
        let mut prior_diagnostics = std::mem::take(&mut self.diagnostics);
        let mut reset = Self::with_live_scripts(project, &self.symbol, frame, live_scripts);
        reset.looping = looping;
        prior_traces.append(&mut reset.traces);
        prior_diagnostics.append(&mut reset.diagnostics);
        reset.traces = prior_traces;
        reset.diagnostics = prior_diagnostics;
        *self = reset;
    }

    pub fn is_live_scripts(&self) -> bool {
        self.live_scripts
    }

    pub fn is_root_halted_by_script(&self) -> bool {
        self.script_halted_root
    }

    pub fn set_looping(&mut self, looping: bool) {
        // Re-enabling loop after a non-looping preview reached its last frame must also resume
        // the transport that the terminal-frame guard paused.
        if looping && !self.looping && !self.script_halted_root {
            self.player.set_playing(true);
        }
        self.looping = looping;
    }

    pub fn is_looping(&self) -> bool {
        self.looping
    }

    pub fn tick(&mut self, project: &Project) {
        self.call_depth = 0;
        self.steps_remaining = MAX_PREVIEW_STEPS_PER_PASS;
        // Button Up/Over/Down/Hit are selected authoring states, not a timeline
        // animation that should advance automatically through Hit.
        if !self.button_root {
            if !self.looping && !self.script_halted_root {
                let total_frames = project
                    .symbol(&self.symbol)
                    .map_or(1, |s| s.timeline.frame_count.max(1));
                if self.frame() + 1 >= total_frames {
                    self.player.set_playing(false);
                    self.process_scripts(project);
                    if self.live_scripts {
                        self.run_registered_enter_frame(project);
                    }
                    return;
                }
            }
            self.player.tick(project);
        }
        self.process_scripts(project);
        if self.live_scripts {
            self.run_registered_enter_frame(project);
        }
    }

    pub fn frame(&self) -> u32 {
        self.player.frame()
    }

    /// Current authored frame of one placed clip in this preview. This keeps
    /// editor navigation aligned with independent clips after playback pauses.
    pub fn clip_frame(&self, path: &[Id]) -> Option<u32> {
        self.player.clip_frame(path)
    }

    /// Frozen frames for the whole currently materialized clip tree. Paths are
    /// relative to this preview's source root, matching authored evaluation.
    pub fn clip_frames(&self) -> HashMap<Vec<Id>, u32> {
        self.player.clip_frames()
    }

    /// Timeline sound commands for the editor's audio preview (see `SoundCommand`).
    pub fn drain_sound_commands(&mut self) -> Vec<crate::SoundCommand> {
        self.player.drain_sound_commands()
    }

    /// Timeline video commands for the editor's video preview (see `VideoCommand`).
    pub fn drain_video_commands(&mut self) -> Vec<VideoCommand> {
        self.player.drain_video_commands()
    }

    /// Update the visual Up (`0`), Over (`1`) or Down (`2`) state of a placed Button during preview.
    pub fn set_button_state(&mut self, project: &Project, path: &[Id], state: u32) -> bool {
        if !self.live_scripts {
            return false;
        }
        self.player.button_state(project, path, state)
    }

    /// Returns the deepest interactive ancestor prefix of `path` (`1..=path.len()`) during live preview:
    /// a `Button` symbol instance, an instance with authored `ObjectScript` handlers, or an
    /// instance with dynamically registered `Click` listeners.
    pub fn interactive_ancestor_path(&self, project: &Project, path: &[Id]) -> Option<Vec<Id>> {
        if !self.live_scripts || path.is_empty() {
            return None;
        }
        for len in 1..=path.len() {
            let prefix = &path[..len];
            let Some(node) = self.player.previous.get(prefix) else {
                continue;
            };
            if !node.effective_visible {
                return None;
            }
            if node
                .accessibility
                .as_ref()
                .is_some_and(|a| a.disabled && a.role == AccessibleRole::Button)
            {
                return None;
            }
        }
        for len in (1..=path.len()).rev() {
            let prefix = &path[..len];
            let Some(node) = self.player.previous.get(prefix) else {
                continue;
            };
            if node
                .symbol_id
                .as_deref()
                .and_then(|id| project.symbol(id))
                .is_some_and(|s| s.kind == SymbolKind::Button)
            {
                return Some(prefix.to_vec());
            }
            if !node.scripts.is_empty() {
                return Some(prefix.to_vec());
            }
            let lifetime = self.player.lifetime_id(prefix);
            if self.registered_handlers.iter().any(|h| {
                h.event == RegisteredEvent::Click
                    && h.target_path == prefix
                    && lifetime == Some(h.target_lifetime)
            }) {
                return Some(prefix.to_vec());
            }
        }
        None
    }

    /// Returns true if `path` (or one of its ancestors) is interactive during live preview.
    pub fn is_interactive_target(&self, project: &Project, path: &[Id]) -> bool {
        self.interactive_ancestor_path(project, path).is_some()
    }

    /// Placed visible, enabled `Button` instances `(path, symbol_id, world_matrix)` for Frame 3 (`Hit`) hit-testing.
    pub fn button_hit_candidates(&self, project: &Project) -> Vec<(Vec<Id>, Id, [f64; 6])> {
        if !self.live_scripts {
            return Vec::new();
        }
        let mut out = Vec::new();
        for node in self.player.previous.values() {
            if node.path.is_empty() || !node.effective_visible {
                continue;
            }
            let Some(sym_id) = node.symbol_id.as_ref() else {
                continue;
            };
            let Some(sym) = project.symbol(sym_id) else {
                continue;
            };
            if sym.kind != SymbolKind::Button {
                continue;
            }
            if (1..=node.path.len()).any(|len| {
                self.player
                    .previous
                    .get(&node.path[..len])
                    .and_then(|anc| anc.accessibility.as_ref())
                    .is_some_and(|a| a.disabled && a.role == AccessibleRole::Button)
            }) {
                continue;
            }
            out.push((node.path.clone(), sym_id.clone(), node.world));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Returns true if root/Stage itself has a registered `Click` handler (`stage.addEventListener("click", ...)`).
    pub fn has_root_click_handler(&self) -> bool {
        if !self.live_scripts {
            return false;
        }
        let root_lifetime = self.player.lifetime_id(&[]);
        self.registered_handlers.iter().any(|h| {
            h.event == RegisteredEvent::Click
                && h.target_path.is_empty()
                && root_lifetime == Some(h.target_lifetime)
        })
    }

    /// Dispatch a click event on a placed element path (or empty slice for Stage/root) during live authoring preview.
    pub fn click(&mut self, project: &Project, path: &[Id]) {
        self.call_depth = 0;
        self.steps_remaining = MAX_PREVIEW_STEPS_PER_PASS;
        if !path.is_empty() {
            let visible_and_enabled = self
                .player
                .previous
                .get(path)
                .is_some_and(|n| n.effective_visible)
                && !(1..=path.len()).any(|len| {
                    self.player
                        .previous
                        .get(&path[..len])
                        .and_then(|node| node.accessibility.as_ref())
                        .is_some_and(|a| a.disabled && a.role == AccessibleRole::Button)
                });
            if !visible_and_enabled {
                return;
            }
            self.player.dispatch_click(project, path);
        }
        self.process_scripts(project);
        if !self.live_scripts {
            return;
        }
        if !path.is_empty() {
            for len in (1..=path.len()).rev() {
                let prefix = path[..len].to_vec();
                self.run_registered_handlers_for(project, &prefix, RegisteredEvent::Click);
            }
        }
        // Stage / root click listeners also receive bubbled or direct Stage clicks.
        self.run_registered_handlers_for(project, &[], RegisteredEvent::Click);
    }

    /// Script `trace(...)` lines produced since the last call.
    pub fn drain_traces(&mut self) -> Vec<String> {
        std::mem::take(&mut self.traces)
    }

    /// Deduplicated located preview diagnostics produced since the last call.
    pub fn drain_diagnostics(&mut self) -> Vec<String> {
        std::mem::take(&mut self.diagnostics)
    }

    pub fn scene(
        &self,
        project: &Project,
        include_guides: bool,
        show_masking: bool,
    ) -> AuthoredScene {
        let live = self.live_scripts;
        let evaluation = crate::evaluate::snapshot_mode(
            project,
            &self.symbol,
            self.frame(),
            &|path| self.player.clip_frame(path),
            &|path| {
                if live {
                    self.player.overrides.get(path).cloned()
                } else {
                    None
                }
            },
            include_guides,
        );
        // Outline is an editor display preference. Playback shows the authored
        // fills/effects while leaving both the document and static scrub intact.
        crate::authored_layers::scene_from_evaluation(project, evaluation, show_masking, None)
    }

    fn prune_stale_handlers(&mut self) {
        self.registered_handlers
            .retain(|h| self.player.lifetime_id(&h.target_path) == Some(h.target_lifetime));
    }

    fn run_registered_enter_frame(&mut self, project: &Project) {
        self.prune_stale_handlers();
        let snapshot: Vec<RegisteredHandler> = self
            .registered_handlers
            .iter()
            .filter(|h| h.event == RegisteredEvent::EnterFrame)
            .cloned()
            .collect();
        for handler in snapshot {
            if self.player.lifetime_id(&handler.target_path) != Some(handler.target_lifetime) {
                continue;
            }
            self.execute_block(
                project,
                &handler.target_path,
                &ScriptEvent::EnterFrame,
                &handler.body,
                &handler.origin,
                handler.base_line,
                &HashMap::new(),
            );
            self.process_scripts(project);
        }
    }

    fn run_registered_handlers_for(
        &mut self,
        project: &Project,
        target_path: &[Id],
        event: RegisteredEvent,
    ) {
        self.prune_stale_handlers();
        let snapshot: Vec<RegisteredHandler> = self
            .registered_handlers
            .iter()
            .filter(|h| h.event == event && h.target_path == target_path)
            .cloned()
            .collect();
        let script_event = match event {
            RegisteredEvent::Click => ScriptEvent::Click,
            RegisteredEvent::EnterFrame => ScriptEvent::EnterFrame,
        };
        for handler in snapshot {
            if self.player.lifetime_id(&handler.target_path) != Some(handler.target_lifetime) {
                continue;
            }
            self.execute_block(
                project,
                &handler.target_path,
                &script_event,
                &handler.body,
                &handler.origin,
                handler.base_line,
                &HashMap::new(),
            );
            self.process_scripts(project);
        }
    }

    fn process_scripts(&mut self, project: &Project) {
        let dispatches = self.player.drain_scripts();
        self.player.runtime_scripts.clear();
        if !self.live_scripts {
            return;
        }
        for dispatch in dispatches {
            if !self.player.is_dispatch_current(project, &dispatch) {
                continue;
            }
            self.execute_preview_dispatch(project, &dispatch);
        }
    }

    fn describe_origin(&self, project: &Project, dispatch: &ScriptDispatch) -> String {
        let root_name = project
            .symbol(&self.symbol)
            .map_or(self.symbol.as_str(), |s| s.name.as_str());
        let target_label = if dispatch.target_path.is_empty() {
            root_name.to_owned()
        } else {
            let names: Vec<String> = dispatch
                .target_path
                .iter()
                .enumerate()
                .map(|(idx, id)| {
                    self.element_display_name(project, &dispatch.target_path[..idx], id)
                })
                .collect();
            format!("{root_name} · {}", names.join("."))
        };
        let ev = match dispatch.event {
            ScriptEvent::Frame(f) => format!("Frame {}", f + 1),
            ScriptEvent::Load => "Load".to_owned(),
            ScriptEvent::EnterFrame => "EnterFrame".to_owned(),
            ScriptEvent::Click => "Click".to_owned(),
        };
        format!("{target_label} ({ev})")
    }

    fn element_display_name(
        &self,
        project: &Project,
        parent_path: &[Id],
        element_id: &str,
    ) -> String {
        let sym_id = if parent_path.is_empty() {
            Some(self.symbol.as_str())
        } else {
            self.player
                .previous
                .get(parent_path)
                .and_then(|n| n.symbol_id.as_deref())
        };
        if let Some(sym) = sym_id.and_then(|id| project.symbol(id)) {
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        if el.id == element_id {
                            if !el.name.trim().is_empty() {
                                return el.name.clone();
                            }
                            return el.id.clone();
                        }
                    }
                }
            }
        }
        element_id.to_owned()
    }

    fn record_diagnostic(&mut self, origin: &str, line: usize, detail: &str) {
        let msg = format!(
            "{origin}, line {line}: {detail} — use Test Movie (Cmd+Return) for full runtime execution."
        );
        if self.reported_diagnostics.insert(msg.clone()) {
            self.diagnostics.push(msg);
        }
    }

    fn register_handler(
        &mut self,
        target_path: Vec<Id>,
        event: RegisteredEvent,
        body: String,
        origin: String,
        base_line: usize,
        replace_existing_kind: bool,
    ) {
        let Some(target_lifetime) = self.player.lifetime_id(&target_path) else {
            return;
        };
        if replace_existing_kind {
            self.registered_handlers.retain(|h| {
                !(h.target_path == target_path
                    && h.target_lifetime == target_lifetime
                    && h.event == event)
            });
        } else if self.registered_handlers.iter().any(|h| {
            h.target_path == target_path
                && h.target_lifetime == target_lifetime
                && h.event == event
                && h.body == body
        }) {
            return;
        }
        self.registered_handlers.push(RegisteredHandler {
            target_path,
            target_lifetime,
            event,
            body,
            origin,
            base_line,
        });
    }

    fn clear_registered_handler(&mut self, target_path: &[Id], event: RegisteredEvent) {
        let lifetime = self.player.lifetime_id(target_path);
        self.registered_handlers.retain(|h| {
            !(h.target_path == target_path
                && Some(h.target_lifetime) == lifetime
                && h.event == event)
        });
    }

    fn execute_preview_dispatch(&mut self, project: &Project, dispatch: &ScriptDispatch) {
        let origin = self.describe_origin(project, dispatch);
        let local_fns = collect_local_functions(&dispatch.script.source);
        self.execute_block(
            project,
            &dispatch.target_path,
            &dispatch.event,
            &dispatch.script.source,
            &origin,
            1,
            &local_fns,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_block(
        &mut self,
        project: &Project,
        path: &[Id],
        current_event: &ScriptEvent,
        source: &str,
        origin: &str,
        base_line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) {
        if self.call_depth >= MAX_PREVIEW_CALL_DEPTH {
            self.record_diagnostic(
                origin,
                base_line,
                &format!("preview script call depth exceeded maximum ({MAX_PREVIEW_CALL_DEPTH})"),
            );
            return;
        }
        self.call_depth += 1;
        self.execute_block_inner(
            project,
            path,
            current_event,
            source,
            origin,
            base_line,
            local_fns,
        );
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_block_inner(
        &mut self,
        project: &Project,
        path: &[Id],
        current_event: &ScriptEvent,
        source: &str,
        origin: &str,
        base_line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) {
        for (rel_line, raw_stmt) in split_script_statements(source) {
            let line = base_line + rel_line.saturating_sub(1);
            if self.steps_remaining == 0 {
                self.record_diagnostic(
                    origin,
                    line,
                    &format!(
                        "preview script execution step budget ({MAX_PREVIEW_STEPS_PER_PASS}) exceeded"
                    ),
                );
                return;
            }
            self.steps_remaining -= 1;
            let stmt = strip_comments_and_trim(raw_stmt);
            let stmt = stmt.trim();
            if stmt.is_empty() || stmt == ";" || stmt == "\"use strict\"" || stmt == "'use strict'"
            {
                continue;
            }
            if stmt.starts_with("import ") {
                continue;
            }
            // Local named function declaration: collected in pass 1, skip at top level.
            if parse_named_function_decl(stmt).is_some() {
                continue;
            }
            // Bare `{ ... }` block
            if stmt.starts_with('{') && stmt.ends_with('}') && is_single_braced_group(stmt) {
                let inner = &stmt[1..stmt.len() - 1];
                self.execute_block(project, path, current_event, inner, origin, line, local_fns);
                continue;
            }
            // 0a. Classic Flash `on (release, press, ...) { ... }` wrapper
            if let Some((events_raw, body)) = parse_wrapper_block(stmt, "on") {
                let events: Vec<String> = events_raw
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                let is_click_like = events
                    .iter()
                    .any(|e| matches!(e.as_str(), "release" | "press" | "click"));
                if is_click_like {
                    match current_event {
                        ScriptEvent::Click => {
                            self.execute_block(
                                project,
                                path,
                                current_event,
                                body,
                                origin,
                                line,
                                local_fns,
                            );
                        }
                        ScriptEvent::Load | ScriptEvent::Frame(_) => {
                            self.register_handler(
                                path.to_vec(),
                                RegisteredEvent::Click,
                                body.to_owned(),
                                origin.to_owned(),
                                line,
                                false,
                            );
                        }
                        ScriptEvent::EnterFrame => {}
                    }
                } else {
                    self.record_diagnostic(
                        origin,
                        line,
                        &format!(
                            "event wrapper on({}) is not simulated in native stage preview",
                            events_raw.trim()
                        ),
                    );
                }
                continue;
            }
            // 0b. Classic Flash `onClipEvent (load | enterFrame | ...) { ... }` wrapper
            if let Some((events_raw, body)) = parse_wrapper_block(stmt, "onClipEvent") {
                let events: Vec<String> = events_raw
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut handled_any = false;
                if events.iter().any(|e| e == "load") {
                    handled_any = true;
                    if matches!(current_event, ScriptEvent::Load | ScriptEvent::Frame(_)) {
                        self.execute_block(
                            project,
                            path,
                            &ScriptEvent::Load,
                            body,
                            origin,
                            line,
                            local_fns,
                        );
                    }
                }
                if events.iter().any(|e| e == "enterframe") {
                    handled_any = true;
                    match current_event {
                        ScriptEvent::EnterFrame => {
                            self.execute_block(
                                project,
                                path,
                                current_event,
                                body,
                                origin,
                                line,
                                local_fns,
                            );
                        }
                        ScriptEvent::Load | ScriptEvent::Frame(_) => {
                            self.register_handler(
                                path.to_vec(),
                                RegisteredEvent::EnterFrame,
                                body.to_owned(),
                                origin.to_owned(),
                                line,
                                false,
                            );
                        }
                        ScriptEvent::Click => {}
                    }
                }
                if events
                    .iter()
                    .any(|e| matches!(e.as_str(), "mousedown" | "mouseup"))
                {
                    handled_any = true;
                    if matches!(current_event, ScriptEvent::Load | ScriptEvent::Frame(_)) {
                        self.register_handler(
                            path.to_vec(),
                            RegisteredEvent::Click,
                            body.to_owned(),
                            origin.to_owned(),
                            line,
                            false,
                        );
                    } else if matches!(current_event, ScriptEvent::Click) {
                        self.execute_block(
                            project,
                            path,
                            current_event,
                            body,
                            origin,
                            line,
                            local_fns,
                        );
                    }
                }
                if !handled_any {
                    self.record_diagnostic(
                        origin,
                        line,
                        &format!(
                            "clip event onClipEvent({}) is not simulated in native stage preview",
                            events_raw.trim()
                        ),
                    );
                }
                continue;
            }
            // 0c. `if (...) { ... } [else ...]`
            if let Some((cond, then_body, else_part)) = parse_if_statement(stmt) {
                if self.eval_preview_condition(project, path, cond) {
                    self.execute_block(
                        project,
                        path,
                        current_event,
                        then_body,
                        origin,
                        line,
                        local_fns,
                    );
                } else if let Some(else_src) = else_part {
                    self.execute_block(
                        project,
                        path,
                        current_event,
                        else_src,
                        origin,
                        line,
                        local_fns,
                    );
                }
                continue;
            }
            // 0d. `delete receiver.onEnterFrame` / `delete receiver.onRelease`
            if let Some(rest) = stmt.strip_prefix("delete ") {
                let target_expr = rest.trim();
                if let Some((recv, prop)) = split_receiver_member(target_expr)
                    && let Some(ev) = callback_prop_to_event(prop)
                    && let Some(target_path) = self.resolve_receiver_path(project, path, recv)
                {
                    self.clear_registered_handler(&target_path, ev);
                    continue;
                }
            }
            // 0e. Callback assignment: `[receiver.]onRelease = function(...) { ... }` or `= null`
            if self
                .try_register_assigned_callback(project, path, stmt, origin, line, local_fns)
                .unwrap_or(false)
            {
                continue;
            }
            // 0f. `addEventListener(...)` / `removeEventListener(...)`
            if self
                .try_register_event_listener(project, path, stmt, origin, line, local_fns)
                .unwrap_or(false)
            {
                continue;
            }
            // 1. Variable declaration: var/let/const name[:Type] [= expr]
            if let Some(rest) = stmt
                .strip_prefix("var ")
                .or_else(|| stmt.strip_prefix("let "))
                .or_else(|| stmt.strip_prefix("const "))
            {
                if let Some((lhs, rhs)) = split_top_level_assign(rest) {
                    let name = lhs.split(':').next().unwrap_or(lhs).trim();
                    if !name.is_empty() {
                        let val = self.eval_preview_expr(project, path, rhs.trim());
                        self.vars.insert((path.to_vec(), name.to_owned()), val);
                    }
                } else {
                    let name = rest.split(':').next().unwrap_or(rest).trim();
                    if !name.is_empty() {
                        self.vars
                            .entry((path.to_vec(), name.to_owned()))
                            .or_default();
                    }
                }
                continue;
            }
            // 2. trace(...)
            if let Some(inner) = extract_call_arg(stmt, "trace") {
                let args = split_top_level_comma(inner);
                let rendered: Vec<String> = args
                    .into_iter()
                    .map(|a| self.eval_preview_expr(project, path, a))
                    .collect();
                self.traces.push(rendered.join(" "));
                continue;
            }
            // 3. Timeline control functions (bare or receiver-qualified: `stop()`, `this.stop()`, `mc.gotoAndStop(2)`, `_root.play()`)
            if self.try_execute_timeline_call(project, path, stmt, origin, line) {
                continue;
            }
            // 4. Local named function call `helper()`
            if let Some((fn_name, arg_str)) = parse_simple_call(stmt)
                && arg_str.is_empty()
                && let Some((fn_line, fn_body)) = local_fns.get(fn_name)
            {
                self.execute_block(
                    project,
                    path,
                    current_event,
                    fn_body,
                    origin,
                    *fn_line,
                    local_fns,
                );
                continue;
            }
            // 5. Property or variable assignments on `this.`, named child, `_root.`, or bare `_prop` / `var`
            if self.try_apply_property_or_var_mutation(project, path, stmt) {
                continue;
            }
            // 6. Unrecognized construct -> emit deduplicated located diagnostic rather than silently dropping!
            let snippet = truncate_snippet(stmt, 56);
            self.record_diagnostic(
                origin,
                line,
                &format!(
                    "statement `{snippet}` is outside the bounded native stage preview subset"
                ),
            );
        }
    }

    fn try_register_assigned_callback(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
        origin: &str,
        line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) -> Option<bool> {
        let (lhs, rhs) = split_top_level_assign(stmt)?;
        let (recv, prop) = match split_receiver_member(lhs) {
            Some((r, p)) => (Some(r), p),
            None => (None, lhs.trim()),
        };
        let ev = callback_prop_to_event(prop)?;
        let Some(target_path) = self.resolve_receiver_path(project, path, recv.unwrap_or("this"))
        else {
            self.record_diagnostic(
                origin,
                line,
                &format!(
                    "cannot resolve instance `{}` for `.{prop}` in native preview",
                    recv.unwrap_or("this")
                ),
            );
            return Some(true);
        };
        let rhs_trimmed = rhs.trim();
        if matches!(rhs_trimmed, "null" | "undefined") {
            self.clear_registered_handler(&target_path, ev);
            return Some(true);
        }
        if let Some(body) = extract_function_literal_body(rhs_trimmed) {
            self.register_handler(
                target_path,
                ev,
                body.to_owned(),
                origin.to_owned(),
                line,
                true,
            );
            return Some(true);
        }
        if let Some((fn_line, fn_body)) = local_fns.get(rhs_trimmed) {
            self.register_handler(
                target_path,
                ev,
                fn_body.clone(),
                origin.to_owned(),
                *fn_line,
                true,
            );
            return Some(true);
        }
        None
    }

    fn try_register_event_listener(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
        origin: &str,
        line: usize,
        local_fns: &HashMap<String, (usize, String)>,
    ) -> Option<bool> {
        let (recv, is_remove, args_raw) =
            if let Some(inner) = extract_call_arg(stmt, "addEventListener") {
                ("this", false, inner)
            } else if let Some(inner) = extract_call_arg(stmt, "removeEventListener") {
                ("this", true, inner)
            } else if let Some((call_head, call_args)) = parse_qualified_call(stmt) {
                if let Some(r) = call_head.strip_suffix(".addEventListener") {
                    (r.trim(), false, call_args)
                } else {
                    let r = call_head.strip_suffix(".removeEventListener")?;
                    (r.trim(), true, call_args)
                }
            } else {
                return None;
            };

        let args = split_top_level_comma(args_raw);
        if args.len() < 2 {
            return None;
        }
        let event_expr = args[0].trim();
        let listener_expr = args[1].trim();
        let ev = match event_expr {
            "\"click\""
            | "'click'"
            | "MouseEvent.CLICK"
            | "flash.events.MouseEvent.CLICK"
            | "\"mouseDown\""
            | "'mouseDown'"
            | "MouseEvent.MOUSE_DOWN" => RegisteredEvent::Click,
            "\"enterFrame\""
            | "'enterFrame'"
            | "Event.ENTER_FRAME"
            | "flash.events.Event.ENTER_FRAME" => RegisteredEvent::EnterFrame,
            other => {
                self.record_diagnostic(
                    origin,
                    line,
                    &format!("event listener for `{other}` requires Test Movie"),
                );
                return Some(true);
            }
        };
        let Some(target_path) = self.resolve_receiver_path(project, path, recv) else {
            self.record_diagnostic(
                origin,
                line,
                &format!("cannot resolve listener target `{recv}` in native preview"),
            );
            return Some(true);
        };
        if is_remove {
            self.clear_registered_handler(&target_path, ev);
            return Some(true);
        }
        if let Some(body) = extract_function_literal_body(listener_expr) {
            self.register_handler(
                target_path,
                ev,
                body.to_owned(),
                origin.to_owned(),
                line,
                false,
            );
            return Some(true);
        }
        if let Some((fn_line, fn_body)) = local_fns.get(listener_expr) {
            self.register_handler(
                target_path,
                ev,
                fn_body.clone(),
                origin.to_owned(),
                *fn_line,
                false,
            );
            return Some(true);
        }
        self.record_diagnostic(
            origin,
            line,
            &format!("listener `{listener_expr}` cannot be resolved in native preview"),
        );
        Some(true)
    }

    fn try_execute_timeline_call(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
        origin: &str,
        line: usize,
    ) -> bool {
        if extract_call_arg(stmt, "stopAllSounds") == Some("")
            || extract_call_arg(stmt, "SoundMixer.stopAll") == Some("")
        {
            self.player.stop_all_sounds();
            return true;
        }
        let Some((head, arg)) = parse_qualified_call(stmt) else {
            return false;
        };
        let (recv_opt, method) = match split_receiver_member(head) {
            Some((r, m)) => (Some(r), m),
            None => (None, head),
        };
        if !matches!(
            method,
            "stop" | "play" | "nextFrame" | "prevFrame" | "gotoAndStop" | "gotoAndPlay"
        ) {
            return false;
        }
        let Some(target_path) =
            self.resolve_receiver_path(project, path, recv_opt.unwrap_or("this"))
        else {
            self.record_diagnostic(
                origin,
                line,
                &format!(
                    "cannot resolve timeline target `{}` in native preview",
                    recv_opt.unwrap_or("this")
                ),
            );
            return true;
        };
        // Classic Flash: unqualified `stop()` / `gotoAndPlay()` inside a Button's `on(release)` targets its parent timeline!
        let effective_path = if recv_opt.is_none() && self.is_button_instance(project, &target_path)
        {
            if target_path.is_empty() {
                Vec::new()
            } else {
                target_path[..target_path.len() - 1].to_vec()
            }
        } else {
            target_path
        };

        match method {
            "stop" if arg.is_empty() => {
                self.player
                    .control(project, &effective_path, PlaybackCommand::Stop);
                if effective_path.is_empty() {
                    self.script_halted_root = true;
                }
                true
            }
            "play" if arg.is_empty() => {
                self.player
                    .control(project, &effective_path, PlaybackCommand::Play);
                if effective_path.is_empty() {
                    self.script_halted_root = false;
                }
                true
            }
            "nextFrame" if arg.is_empty() => {
                let cur = self
                    .player
                    .clip_frame(&effective_path)
                    .unwrap_or_else(|| self.player.frame());
                self.player.control(
                    project,
                    &effective_path,
                    PlaybackCommand::GotoAndStop(cur + 1),
                );
                if effective_path.is_empty() {
                    self.script_halted_root = true;
                }
                true
            }
            "prevFrame" if arg.is_empty() => {
                let cur = self
                    .player
                    .clip_frame(&effective_path)
                    .unwrap_or_else(|| self.player.frame());
                self.player.control(
                    project,
                    &effective_path,
                    PlaybackCommand::GotoAndStop(cur.saturating_sub(1)),
                );
                if effective_path.is_empty() {
                    self.script_halted_root = true;
                }
                true
            }
            "gotoAndStop" => {
                if let Some(target_frame) = self.resolve_frame_arg(project, &effective_path, arg) {
                    self.player.control(
                        project,
                        &effective_path,
                        PlaybackCommand::GotoAndStop(target_frame),
                    );
                    if effective_path.is_empty() {
                        self.script_halted_root = true;
                    }
                }
                true
            }
            "gotoAndPlay" => {
                if let Some(target_frame) = self.resolve_frame_arg(project, &effective_path, arg) {
                    self.player.control(
                        project,
                        &effective_path,
                        PlaybackCommand::GotoAndPlay(target_frame),
                    );
                    if effective_path.is_empty() {
                        self.script_halted_root = false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn is_button_instance(&self, project: &Project, path: &[Id]) -> bool {
        if path.is_empty() {
            return self.button_root;
        }
        self.player
            .previous
            .get(path)
            .and_then(|n| n.symbol_id.as_deref())
            .and_then(|id| project.symbol(id))
            .is_some_and(|s| s.kind == SymbolKind::Button)
    }

    fn resolve_receiver_path(
        &self,
        project: &Project,
        current_path: &[Id],
        recv: &str,
    ) -> Option<Vec<Id>> {
        let trimmed = recv.trim();
        if trimmed.is_empty() || trimmed == "this" {
            return Some(current_path.to_vec());
        }
        if matches!(trimmed, "_root" | "root" | "_level0" | "stage" | "Stage") {
            return Some(Vec::new());
        }
        if matches!(trimmed, "_parent" | "parent") {
            if current_path.is_empty() {
                return Some(Vec::new());
            }
            return Some(current_path[..current_path.len() - 1].to_vec());
        }
        let mut base: Vec<Id> = current_path.to_vec();
        let segments: Vec<&str> = trimmed.split('.').collect();
        let mut idx = 0;
        if segments[0] == "this" {
            idx = 1;
        } else if matches!(segments[0], "_root" | "root" | "_level0") {
            base.clear();
            idx = 1;
        } else if matches!(segments[0], "_parent" | "parent") {
            if !base.is_empty() {
                base.pop();
            }
            idx = 1;
        }
        while idx < segments.len() {
            let seg = segments[idx].trim();
            if matches!(seg, "_parent" | "parent") {
                if !base.is_empty() {
                    base.pop();
                }
            } else if seg != "this" {
                let child_id = self.find_named_child_id(project, &base, seg)?;
                base.push(child_id);
            }
            idx += 1;
        }
        Some(base)
    }

    fn find_named_child_id(
        &self,
        project: &Project,
        parent_path: &[Id],
        instance_name: &str,
    ) -> Option<Id> {
        let sym_id = if parent_path.is_empty() {
            Some(self.symbol.as_str())
        } else {
            self.player
                .previous
                .get(parent_path)
                .and_then(|n| n.symbol_id.as_deref())
        }?;
        let sym = project.symbol(sym_id)?;
        let cur_frame = self
            .player
            .clip_frame(parent_path)
            .unwrap_or_else(|| self.player.frame());
        // Check elements sampled at the active frame first, then any element in the symbol timeline.
        for layer in sym.timeline.layers.iter().rev() {
            for el in crate::evaluate::sample_layer_in_at_fps(
                &sym.timeline,
                layer,
                cur_frame,
                f64::from(project.fps.max(1)),
            )
            .into_iter()
            .rev()
            {
                if matches!(el.kind, ElementKind::Instance { .. })
                    && (el.name == instance_name || el.id == instance_name)
                {
                    let mut candidate = parent_path.to_vec();
                    candidate.push(el.id.clone());
                    if self.player.previous.contains_key(&candidate) {
                        return Some(el.id);
                    }
                }
            }
        }
        None
    }

    fn resolve_frame_arg(&self, project: &Project, path: &[Id], arg: &str) -> Option<u32> {
        // Support two-arg `gotoAndPlay("Scene", frame)` by taking the frame argument when targeting root
        let parts = split_top_level_comma(arg);
        let frame_expr = if parts.len() == 2 && path.is_empty() {
            parts[1]
        } else {
            parts.first().copied().unwrap_or(arg)
        };
        let evaluated = self.eval_preview_expr(project, path, frame_expr);
        if let Ok(one_based) = evaluated.trim().parse::<f64>() {
            let idx = (one_based.round() as i64 - 1).max(0) as u32;
            return Some(idx);
        }
        let symbol_id = if path.is_empty() {
            Some(self.symbol.as_str())
        } else {
            self.player
                .previous
                .get(path)
                .and_then(|n| n.symbol_id.as_deref())
        }?;
        let symbol = project.symbol(symbol_id)?;
        symbol
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find(|k| k.label.as_str() == evaluated.trim())
            .map(|k| k.frame)
    }

    fn try_apply_property_or_var_mutation(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
    ) -> bool {
        // Increment / decrement shorthand: `x++`, `this._x++`, `count--`
        if let Some(lhs) = stmt.strip_suffix("++") {
            let rewritten = format!("{} += 1", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, path, &rewritten);
        }
        if let Some(lhs) = stmt.strip_suffix("--") {
            let rewritten = format!("{} -= 1", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, path, &rewritten);
        }

        let (lhs, op, rhs) = if let Some((l, r)) = split_compound_op(stmt, "+=") {
            (l, "+=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "-=") {
            (l, "-=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "*=") {
            (l, "*=", r)
        } else if let Some((l, r)) = split_top_level_assign(stmt) {
            (l, "=", r)
        } else {
            return false;
        };

        let (recv_opt, prop_raw) = match split_receiver_member(lhs) {
            Some((r, p)) => (Some(r), p.trim()),
            None => (None, lhs.trim()),
        };

        // Check if mutating an existing script variable (`score += 10`, `count = 2`)
        if recv_opt.is_none()
            && self
                .vars
                .contains_key(&(path.to_vec(), prop_raw.to_owned()))
        {
            let cur_val = self
                .vars
                .get(&(path.to_vec(), prop_raw.to_owned()))
                .cloned()
                .unwrap_or_default();
            let rhs_val = self.eval_preview_expr(project, path, rhs);
            let next_val = match op {
                "=" => rhs_val,
                "+=" => match (cur_val.parse::<f64>(), rhs_val.parse::<f64>()) {
                    (Ok(a), Ok(b)) => format_number(a + b),
                    _ => format!("{cur_val}{rhs_val}"),
                },
                "-=" => format_number(
                    cur_val.parse::<f64>().unwrap_or(0.0) - rhs_val.parse::<f64>().unwrap_or(0.0),
                ),
                "*=" => format_number(
                    cur_val.parse::<f64>().unwrap_or(0.0) * rhs_val.parse::<f64>().unwrap_or(0.0),
                ),
                _ => return false,
            };
            self.vars
                .insert((path.to_vec(), prop_raw.to_owned()), next_val);
            return true;
        }

        let Some(target_path) =
            self.resolve_receiver_path(project, path, recv_opt.unwrap_or("this"))
        else {
            return false;
        };

        if matches!(prop_raw, "_visible" | "visible") && op == "=" {
            let val_str = self.eval_preview_expr(project, path, rhs);
            let visible = !matches!(val_str.as_str(), "false" | "0" | "");
            self.player.set_visible(&target_path, visible);
            return true;
        }
        let (prop_name, scale_factor) = match prop_raw {
            "x" | "_x" => ("x", 1.0),
            "y" | "_y" => ("y", 1.0),
            "rotation" | "_rotation" => ("rotation", 1.0),
            "scaleX" | "scale_x" => ("scale_x", 1.0),
            "_xscale" => ("scale_x", 0.01),
            "scaleY" | "scale_y" => ("scale_y", 1.0),
            "_yscale" => ("scale_y", 0.01),
            "alpha" => ("alpha", 1.0),
            "_alpha" => ("alpha", 0.01),
            _ => {
                // Allow dynamic custom field storage on target path (`this.vy = 4`, `hero.hp = 10`)
                if recv_opt.is_some()
                    && prop_raw
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                {
                    let cur_val = self
                        .vars
                        .get(&(target_path.clone(), prop_raw.to_owned()))
                        .cloned()
                        .unwrap_or_else(|| "0".into());
                    let rhs_val = self.eval_preview_expr(project, path, rhs);
                    let next_val = match op {
                        "=" => rhs_val,
                        "+=" => match (cur_val.parse::<f64>(), rhs_val.parse::<f64>()) {
                            (Ok(a), Ok(b)) => format_number(a + b),
                            _ => format!("{cur_val}{rhs_val}"),
                        },
                        "-=" => format_number(
                            cur_val.parse::<f64>().unwrap_or(0.0)
                                - rhs_val.parse::<f64>().unwrap_or(0.0),
                        ),
                        "*=" => format_number(
                            cur_val.parse::<f64>().unwrap_or(0.0)
                                * rhs_val.parse::<f64>().unwrap_or(0.0),
                        ),
                        _ => return false,
                    };
                    self.vars
                        .insert((target_path, prop_raw.to_owned()), next_val);
                    return true;
                }
                return false;
            }
        };
        let Ok(rhs_num) = self.eval_preview_expr(project, path, rhs).parse::<f64>() else {
            return false;
        };
        let cur = self
            .player
            .property(project, &target_path, prop_name)
            .unwrap_or(match prop_name {
                "scale_x" | "scale_y" | "alpha" => 1.0,
                _ => 0.0,
            })
            / scale_factor;
        let next = match op {
            "+=" => cur + rhs_num,
            "-=" => cur - rhs_num,
            "*=" => cur * rhs_num,
            "=" => rhs_num,
            _ => return false,
        } * scale_factor;
        let next = if prop_name == "alpha" {
            next.clamp(0.0, 1.0)
        } else {
            next
        };
        self.player.set_property(&target_path, prop_name, next);
        true
    }

    fn eval_preview_condition(&self, project: &Project, path: &[Id], cond: &str) -> bool {
        let mut s = cond.trim();
        while s.starts_with('(') && s.ends_with(')') && is_single_paren_group(s) {
            s = s[1..s.len() - 1].trim();
        }
        let or_parts = split_top_level_op(s, "||");
        if or_parts.len() > 1 {
            return or_parts
                .into_iter()
                .any(|p| self.eval_preview_condition(project, path, p));
        }
        let and_parts = split_top_level_op(s, "&&");
        if and_parts.len() > 1 {
            return and_parts
                .into_iter()
                .all(|p| self.eval_preview_condition(project, path, p));
        }
        if let Some(inner) = s.strip_prefix('!') {
            return !self.eval_preview_condition(project, path, inner);
        }
        for op in ["===", "!==", "<=", ">=", "==", "!=", "<", ">"] {
            let parts = split_top_level_op(s, op);
            if parts.len() == 2 {
                let lv = self.eval_preview_expr(project, path, parts[0]);
                let rv = self.eval_preview_expr(project, path, parts[1]);
                let num_cmp = match (lv.parse::<f64>(), rv.parse::<f64>()) {
                    (Ok(a), Ok(b)) => a.partial_cmp(&b),
                    _ => None,
                };
                return match op {
                    "===" | "==" => num_cmp.map_or(lv == rv, |o| o.is_eq()),
                    "!==" | "!=" => num_cmp.map_or(lv != rv, |o| !o.is_eq()),
                    "<" => num_cmp.map_or(lv < rv, |o| o.is_lt()),
                    "<=" => num_cmp.map_or(lv <= rv, |o| o.is_le()),
                    ">" => num_cmp.map_or(lv > rv, |o| o.is_gt()),
                    ">=" => num_cmp.map_or(lv >= rv, |o| o.is_ge()),
                    _ => false,
                };
            }
        }
        let val = self.eval_preview_expr(project, path, s);
        !matches!(
            val.as_str(),
            "" | "false" | "0" | "null" | "undefined" | "NaN"
        )
    }

    fn eval_preview_expr(&self, project: &Project, path: &[Id], expr: &str) -> String {
        let mut trimmed = expr.trim();
        while trimmed.starts_with('(') && trimmed.ends_with(')') && is_single_paren_group(trimmed) {
            trimmed = trimmed[1..trimmed.len() - 1].trim();
        }
        if trimmed.is_empty() {
            return String::new();
        }
        let parts = split_top_level_plus(trimmed);
        if parts.len() > 1 {
            let evaluated: Vec<String> = parts
                .into_iter()
                .map(|p| self.eval_preview_expr(project, path, p))
                .collect();
            if evaluated.iter().all(|s| s.parse::<f64>().is_ok()) {
                let sum: f64 = evaluated.iter().filter_map(|s| s.parse::<f64>().ok()).sum();
                return format_number(sum);
            }
            return evaluated.concat();
        }
        let sub_parts = split_top_level_binary_minus(trimmed);
        if let Some((l, r)) = sub_parts {
            let lv = self
                .eval_preview_expr(project, path, l)
                .parse::<f64>()
                .unwrap_or(0.0);
            let rv = self
                .eval_preview_expr(project, path, r)
                .parse::<f64>()
                .unwrap_or(0.0);
            return format_number(lv - rv);
        }
        let mul_parts = split_top_level_op(trimmed, "*");
        if mul_parts.len() == 2 {
            let lv = self
                .eval_preview_expr(project, path, mul_parts[0])
                .parse::<f64>()
                .unwrap_or(0.0);
            let rv = self
                .eval_preview_expr(project, path, mul_parts[1])
                .parse::<f64>()
                .unwrap_or(0.0);
            return format_number(lv * rv);
        }
        self.eval_preview_atom(project, path, trimmed)
    }

    fn eval_preview_atom(&self, project: &Project, path: &[Id], atom: &str) -> String {
        let s = atom.trim();
        if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
            || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        {
            return unescape_string_literal(&s[1..s.len() - 1]);
        }
        if s == "true" || s == "false" {
            return s.to_owned();
        }
        if let Some(inner) = s.strip_prefix('!') {
            return if self.eval_preview_condition(project, path, inner) {
                "false".into()
            } else {
                "true".into()
            };
        }
        if let Ok(n) = s.parse::<f64>() {
            return format_number(n);
        }
        if let Some(val) = self.vars.get(&(path.to_vec(), s.to_owned())) {
            return val.clone();
        }
        let (target_path, prop) = if let Some((recv, member)) = split_receiver_member(s)
            && let Some(resolved) = self.resolve_receiver_path(project, path, recv)
        {
            (resolved, member.trim())
        } else {
            (path.to_vec(), s)
        };
        if let Some(val) = self.vars.get(&(target_path.clone(), prop.to_owned())) {
            return val.clone();
        }
        match prop {
            "_currentframe" | "currentFrame" => {
                let f = self
                    .player
                    .clip_frame(&target_path)
                    .unwrap_or_else(|| self.player.frame());
                (f + 1).to_string()
            }
            "_totalframes" | "totalFrames" => {
                let sym_id = if target_path.is_empty() {
                    Some(self.symbol.as_str())
                } else {
                    self.player
                        .previous
                        .get(&target_path)
                        .and_then(|n| n.symbol_id.as_deref())
                };
                let count = sym_id
                    .and_then(|id| project.symbol(id))
                    .map_or(1, |sym| sym.timeline.frame_count.max(1));
                count.to_string()
            }
            "_name" | "name" => target_path
                .last()
                .map(|id| {
                    let parent = &target_path[..target_path.len() - 1];
                    self.element_display_name(project, parent, id)
                })
                .unwrap_or_else(|| "root".into()),
            "_visible" | "visible" => {
                let vis = self
                    .player
                    .overrides
                    .get(&target_path)
                    .and_then(|o| o.visible)
                    .unwrap_or(true);
                vis.to_string()
            }
            "x" | "_x" => self
                .player
                .property(project, &target_path, "x")
                .map(format_number)
                .unwrap_or_else(|| "0".into()),
            "y" | "_y" => self
                .player
                .property(project, &target_path, "y")
                .map(format_number)
                .unwrap_or_else(|| "0".into()),
            "rotation" | "_rotation" => self
                .player
                .property(project, &target_path, "rotation")
                .map(format_number)
                .unwrap_or_else(|| "0".into()),
            "scaleX" | "scale_x" => self
                .player
                .property(project, &target_path, "scale_x")
                .map(format_number)
                .unwrap_or_else(|| "1".into()),
            "_xscale" => self
                .player
                .property(project, &target_path, "scale_x")
                .map(|v| format_number(v * 100.0))
                .unwrap_or_else(|| "100".into()),
            "scaleY" | "scale_y" => self
                .player
                .property(project, &target_path, "scale_y")
                .map(format_number)
                .unwrap_or_else(|| "1".into()),
            "_yscale" => self
                .player
                .property(project, &target_path, "scale_y")
                .map(|v| format_number(v * 100.0))
                .unwrap_or_else(|| "100".into()),
            "alpha" => self
                .player
                .property(project, &target_path, "alpha")
                .map(format_number)
                .unwrap_or_else(|| "1".into()),
            "_alpha" => self
                .player
                .property(project, &target_path, "alpha")
                .map(|a| format_number(a * 100.0))
                .unwrap_or_else(|| "100".into()),
            _ => s.to_owned(),
        }
    }
}

fn callback_prop_to_event(prop: &str) -> Option<RegisteredEvent> {
    match prop.trim() {
        "onRelease" | "onPress" | "onclick" => Some(RegisteredEvent::Click),
        "onEnterFrame" => Some(RegisteredEvent::EnterFrame),
        _ => None,
    }
}

fn truncate_snippet(s: &str, max_chars: usize) -> String {
    let single_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= max_chars {
        single_line
    } else {
        let head: String = single_line.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

fn format_number(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 {
        format!("{}", n.round() as i64)
    } else {
        format!("{n}")
    }
}

fn unescape_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Strip `//` and `/* ... */` comments outside string literals (`'...'`, `"..."`, `` `...` ``)
/// while preserving arbitrary UTF-8 codepoints verbatim (`T-259`).
fn strip_comments_and_trim(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            out.push(b);
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if b == b'`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    if bytes[i] == b'\n' {
                        out.push(b'\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'`' => in_backtick = true,
            _ => {}
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out)
        .unwrap_or_else(|_| s.to_owned())
        .trim()
        .to_owned()
}

/// Brace-, paren-, bracket-, string- and comment-aware top-level statement splitter.
/// Returns `(1_based_line, statement_slice)`.
fn split_script_statements(source: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut start_line = 1usize;
    let mut cur_line = 1usize;
    let mut saw_token_in_seg = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && saw_token_in_seg {
                    let seg = source[start..i].trim();
                    if !seg.is_empty() {
                        out.push((start_line, seg));
                    }
                    start = i + 1;
                    saw_token_in_seg = false;
                }
                cur_line += 1;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_single {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'\n' {
                    cur_line += 1;
                }
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'\n' {
                    cur_line += 1;
                }
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if in_backtick {
            if b == b'\n' {
                cur_line += 1;
            }
            if b == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'\n' {
                    cur_line += 1;
                }
                i += 2;
                continue;
            }
            if b == b'`' {
                in_backtick = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }
        if !b.is_ascii_whitespace() && !saw_token_in_seg {
            saw_token_in_seg = true;
            start_line = cur_line;
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'`' => in_backtick = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                    // Check if followed by `else` (after optional whitespace/comments) or `;`
                    let after = &source[i + 1..];
                    let after_trimmed = strip_comments_and_trim(after);
                    if !after_trimmed.starts_with("else")
                        && !after_trimmed.starts_with(';')
                        && !after_trimmed.starts_with(')')
                    {
                        let seg = source[start..=i].trim();
                        if !seg.is_empty() {
                            out.push((start_line, seg));
                        }
                        start = i + 1;
                        saw_token_in_seg = false;
                    }
                }
            }
            b';' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                let seg = source[start..i].trim();
                if !seg.is_empty() {
                    out.push((start_line, seg));
                }
                start = i + 1;
                saw_token_in_seg = false;
            }
            b'\n' => {
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && saw_token_in_seg {
                    let after = strip_comments_and_trim(&source[i + 1..]);
                    if !after.starts_with("else") && !after.starts_with('{') {
                        let seg = source[start..i].trim();
                        if !seg.is_empty() {
                            out.push((start_line, seg));
                        }
                        start = i + 1;
                        saw_token_in_seg = false;
                    }
                }
                cur_line += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < source.len() {
        let seg = source[start..].trim();
        if !seg.is_empty() {
            out.push((start_line, seg));
        }
    }
    out
}

fn collect_local_functions(source: &str) -> HashMap<String, (usize, String)> {
    let mut map = HashMap::new();
    for (line, raw_stmt) in split_script_statements(source) {
        let stmt = strip_comments_and_trim(raw_stmt);
        if let Some((name, body)) = parse_named_function_decl(&stmt) {
            map.insert(name.to_owned(), (line, body.to_owned()));
        }
    }
    map
}

fn parse_named_function_decl(stmt: &str) -> Option<(&str, &str)> {
    let rest = stmt.strip_prefix("function")?.trim_start();
    if rest.starts_with('(') {
        return None;
    }
    let open_paren = rest.find('(')?;
    let name = rest[..open_paren].trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let after_name = &rest[open_paren..];
    let close_paren = find_matching_delim(after_name, b'(', b')')?;
    let after_params = after_name[close_paren + 1..].trim_start();
    let brace_idx = after_params.find('{')?;
    // Between `)` and `{` only optional `: ReturnType` annotation is allowed
    let between = after_params[..brace_idx].trim();
    if !between.is_empty() && !between.starts_with(':') {
        return None;
    }
    let body_block = after_params[brace_idx..].trim();
    let close_brace = find_matching_delim(body_block, b'{', b'}')?;
    if close_brace + 1 != body_block.len() {
        return None;
    }
    Some((name, &body_block[1..close_brace]))
}

fn parse_wrapper_block<'a>(stmt: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let rest = stmt.strip_prefix(keyword)?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_delim(rest, b'(', b')')?;
    let events = &rest[1..close_paren];
    let body_part = rest[close_paren + 1..].trim();
    if !body_part.starts_with('{') {
        return None;
    }
    let close_brace = find_matching_delim(body_part, b'{', b'}')?;
    if close_brace + 1 != body_part.len() {
        return None;
    }
    Some((events, &body_part[1..close_brace]))
}

fn parse_if_statement(stmt: &str) -> Option<(&str, &str, Option<&str>)> {
    let rest = stmt.strip_prefix("if")?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_delim(rest, b'(', b')')?;
    let cond = &rest[1..close_paren];
    let after_cond = rest[close_paren + 1..].trim();
    if let Some(close_brace) = after_cond
        .starts_with('{')
        .then(|| find_matching_delim(after_cond, b'{', b'}'))
        .flatten()
    {
        let then_body = &after_cond[1..close_brace];
        let tail = after_cond[close_brace + 1..].trim();
        if tail.is_empty() {
            return Some((cond, then_body, None));
        }
        if let Some(else_rest) = tail.strip_prefix("else")
            && !else_rest
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            let else_trimmed = else_rest.trim();
            if else_trimmed.starts_with('{')
                && let Some(end_b) = find_matching_delim(else_trimmed, b'{', b'}')
                && end_b + 1 == else_trimmed.len()
            {
                return Some((cond, then_body, Some(&else_trimmed[1..end_b])));
            }
            return Some((cond, then_body, Some(else_trimmed)));
        }
        None
    } else {
        // Single-statement `if (cond) stmt`
        Some((cond, after_cond, None))
    }
}

fn extract_function_literal_body(expr: &str) -> Option<&str> {
    let s = expr.trim();
    if let Some(rest) = s.strip_prefix("function") {
        let open_paren = rest.find('(')?;
        let after_open = &rest[open_paren..];
        let close_paren = find_matching_delim(after_open, b'(', b')')?;
        let after_params = after_open[close_paren + 1..].trim_start();
        let brace_idx = after_params.find('{')?;
        let block = after_params[brace_idx..].trim();
        let close_brace = find_matching_delim(block, b'{', b'}')?;
        if close_brace + 1 == block.len() {
            return Some(&block[1..close_brace]);
        }
        return None;
    }
    // Arrow function: `(e) => { ... }` or `() => stmt` or `e => { ... }`
    if let Some(arrow_pos) = find_top_level_arrow(s) {
        let rhs = s[arrow_pos + 2..].trim();
        if rhs.starts_with('{')
            && let Some(close_brace) = find_matching_delim(rhs, b'{', b'}')
            && close_brace + 1 == rhs.len()
        {
            return Some(&rhs[1..close_brace]);
        }
        return Some(rhs);
    }
    None
}

fn find_top_level_arrow(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut in_s = false;
    let mut in_d = false;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'=' if bytes[i + 1] == b'>' && paren == 0 && brace == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_matching_delim(s: &str, open: u8, close: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first().copied() != Some(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut i = 0;
    let mut in_s = false;
    let mut in_d = false;
    let mut in_b = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        if in_b {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'`' {
                in_b = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'`' => in_b = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_single_braced_group(s: &str) -> bool {
    find_matching_delim(s, b'{', b'}') == Some(s.len().saturating_sub(1))
}

fn is_single_paren_group(s: &str) -> bool {
    find_matching_delim(s, b'(', b')') == Some(s.len().saturating_sub(1))
}

fn extract_call_arg<'a>(stmt: &'a str, fn_name: &str) -> Option<&'a str> {
    let rest = stmt.strip_prefix(fn_name)?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close = find_matching_delim(rest, b'(', b')')?;
    if close + 1 != rest.len() {
        return None;
    }
    Some(rest[1..close].trim())
}

fn parse_qualified_call(stmt: &str) -> Option<(&str, &str)> {
    let open = stmt.find('(')?;
    let head = stmt[..open].trim();
    if head.is_empty()
        || !head
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
    {
        return None;
    }
    let args_part = &stmt[open..];
    let close = find_matching_delim(args_part, b'(', b')')?;
    if close + 1 != args_part.len() {
        return None;
    }
    Some((head, args_part[1..close].trim()))
}

fn parse_simple_call(stmt: &str) -> Option<(&str, &str)> {
    let (head, args) = parse_qualified_call(stmt)?;
    if head.contains('.') {
        return None;
    }
    Some((head, args))
}

fn split_receiver_member(expr: &str) -> Option<(&str, &str)> {
    let trimmed = expr.trim();
    let dot = trimmed.rfind('.')?;
    let recv = trimmed[..dot].trim();
    let member = trimmed[dot + 1..].trim();
    if recv.is_empty() || member.is_empty() {
        return None;
    }
    if !recv
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
    {
        return None;
    }
    Some((recv, member))
}

fn split_top_level_assign(stmt: &str) -> Option<(&str, &str)> {
    let bytes = stmt.as_bytes();
    let mut i = 0;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut in_s = false;
    let mut in_d = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            b'=' if paren == 0 && brace == 0 && bracket == 0 => {
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
                if !matches!(prev, b'!' | b'=' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/')
                    && next != b'='
                    && next != b'>'
                {
                    return Some((stmt[..i].trim(), stmt[i + 1..].trim()));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_compound_op<'a>(stmt: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let parts = split_top_level_op(stmt, op);
    if parts.len() == 2 {
        Some((parts[0].trim(), parts[1].trim()))
    } else {
        None
    }
}

fn split_top_level_comma(s: &str) -> Vec<&str> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let parts = split_top_level_op(trimmed, ",");
    if parts.is_empty() {
        vec![trimmed]
    } else {
        parts
    }
}

fn split_top_level_op<'a>(expr: &'a str, op: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let bytes = expr.as_bytes();
    let op_bytes = op.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            _ if paren == 0
                && brace == 0
                && bracket == 0
                && i + op_bytes.len() <= bytes.len()
                && &bytes[i..i + op_bytes.len()] == op_bytes =>
            {
                // Avoid matching `===` when looking for `==`, or `<=` when looking for `<`
                let next = bytes.get(i + op_bytes.len()).copied().unwrap_or(0);
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let conflict = (op == "==" && (prev == b'=' || prev == b'!' || next == b'='))
                    || (op == "!=" && next == b'=')
                    || ((op == "<" || op == ">") && next == b'=')
                    || (op == "*" && next == b'=');
                if !conflict {
                    parts.push(expr[start..i].trim());
                    start = i + op_bytes.len();
                    i = start;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if !parts.is_empty() {
        parts.push(expr[start..].trim());
    }
    parts
}

fn split_top_level_plus(expr: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut paren_depth = 0usize;
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'+' if paren_depth == 0 => {
                let next = bytes.get(i + 1).copied().unwrap_or(0);
                if next != b'+' && next != b'=' {
                    parts.push(expr[start..i].trim());
                    start = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if !parts.is_empty() {
        parts.push(expr[start..].trim());
    }
    parts
}

fn split_top_level_binary_minus(expr: &str) -> Option<(&str, &str)> {
    let bytes = expr.as_bytes();
    let mut in_s = false;
    let mut in_d = false;
    let mut paren = 0usize;
    let mut last_minus = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_s {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            i += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => in_s = true,
            b'"' => in_d = true,
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            b'-' if paren == 0 => {
                let prev_non_ws = expr[..i].trim_end().as_bytes().last().copied();
                let next = bytes.get(i + 1).copied().unwrap_or(0);
                if next != b'-'
                    && next != b'='
                    && prev_non_ws.is_some_and(|p| {
                        p.is_ascii_alphanumeric() || matches!(p, b')' | b'"' | b'\'' | b'_')
                    })
                {
                    last_minus = Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    let pos = last_minus?;
    Some((expr[..pos].trim(), expr[pos + 1..].trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectEvent;

    #[test]
    fn non_looping_preview_stops_at_last_frame_and_can_resume_looping() {
        let mut project = Project::blank();
        project.symbols[0].timeline.frame_count = 3;
        let root = project.root.clone();
        let mut preview = AuthoredPlayback::new(&project, &root, 0);

        preview.set_looping(false);
        preview.tick(&project);
        preview.tick(&project);
        preview.tick(&project);
        assert_eq!(preview.frame(), 2);

        preview.set_looping(true);
        preview.tick(&project);
        assert_eq!(preview.frame(), 0);
    }

    #[test]
    fn demo_playback_emits_traces_and_supports_live_script_mode() {
        let project = Project::demo();
        let root = project.root.clone();
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert_eq!(
            preview.drain_traces(),
            vec!["NeoFlash: the stage is ready".to_string()]
        );

        let orbit_id = project
            .symbol(&root)
            .unwrap()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .find(|e| e.name == "mainOrbit")
            .unwrap()
            .id
            .clone();

        preview.click(&project, std::slice::from_ref(&orbit_id));
        assert_eq!(preview.drain_traces(), vec!["Orbit rotated".to_string()]);
    }

    #[test]
    fn classic_preview_ignores_initial_tick_and_click_scripts() {
        let mut project = Project::demo();
        let root = project.root.clone();
        project
            .symbol_mut(&root)
            .unwrap()
            .timeline
            .scripts
            .push(crate::FrameScript {
                frame: 1,
                script: crate::Script {
                    language: crate::ScriptLanguage::JavaScript,
                    source: "trace(\"tick\"); this.x = 99;".into(),
                },
            });
        let orbit_id = project
            .symbol(&root)
            .unwrap()
            .timeline
            .layers
            .iter()
            .flat_map(|layer| &layer.keyframes)
            .flat_map(|keyframe| &keyframe.elements)
            .find(|element| element.name == "mainOrbit")
            .unwrap()
            .id
            .clone();

        let mut preview = AuthoredPlayback::new(&project, &root, 0);
        assert!(preview.drain_traces().is_empty());
        assert!(preview.vars.is_empty());
        assert!(preview.player.overrides.is_empty());

        preview.click(&project, std::slice::from_ref(&orbit_id));
        assert!(preview.drain_traces().is_empty());
        assert!(preview.vars.is_empty());
        assert!(preview.player.overrides.is_empty());

        preview.tick(&project);
        assert_eq!(preview.frame(), 1);
        assert!(preview.drain_traces().is_empty());
        assert!(preview.vars.is_empty());
        assert!(preview.player.overrides.is_empty());
    }

    #[test]
    fn disabling_live_scripts_clears_runtime_state_and_resumes_root() {
        let mut project = Project::demo();
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![
            crate::FrameScript {
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::JavaScript,
                    source: "var count = 1; this.x = 12; stop(); trace(\"live\");".into(),
                },
            },
            crate::FrameScript {
                frame: 1,
                script: crate::Script {
                    language: crate::ScriptLanguage::JavaScript,
                    source: "trace(\"ignored after toggle\"); this.x = 99;".into(),
                },
            },
        ];

        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert!(preview.is_root_halted_by_script());
        assert!(!preview.vars.is_empty());
        assert!(!preview.player.overrides.is_empty());

        preview.set_live_scripts(&project, false);
        assert!(!preview.is_root_halted_by_script());
        assert!(preview.vars.is_empty());
        assert!(preview.player.overrides.is_empty());
        assert_eq!(preview.drain_traces(), vec!["live".to_string()]);

        preview.tick(&project);
        assert_eq!(preview.frame(), 1);
        assert!(preview.drain_traces().is_empty());
        assert!(preview.vars.is_empty());
        assert!(preview.player.overrides.is_empty());
    }

    #[test]
    fn switching_live_scripts_rebuilds_a_stopped_child_clock() {
        let mut project = Project::blank();
        let root = project.root.clone();
        let mut child = project.symbol(&root).unwrap().clone();
        child.id = "live-toggle-child".into();
        child.name = "Live toggle child".into();
        child.timeline.frame_count = 3;
        child.timeline.layers[0].id = "live-toggle-child-layer".into();
        child.timeline.scripts = vec![crate::FrameScript {
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::JavaScript,
                source: "stop();".into(),
            },
        }];
        project.symbols.push(child);
        project.symbol_mut(&root).unwrap().timeline.frame_count = 1;
        project.symbol_mut(&root).unwrap().timeline.layers[0].keyframes[0].elements =
            vec![crate::Element {
                id: "live-toggle-instance".into(),
                name: "liveToggleClip".into(),
                kind: crate::ElementKind::Instance {
                    symbol_id: "live-toggle-child".into(),
                    playback: crate::InstancePlayback::Independent,
                },
                ..crate::Element::default()
            }];
        project.validate().unwrap();

        let path = vec!["live-toggle-instance".into()];
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        preview.tick(&project);
        assert_eq!(preview.clip_frame(&path), Some(0));

        preview.set_live_scripts(&project, false);
        assert_eq!(preview.frame(), 0, "the authored root playhead is retained");
        preview.tick(&project);
        assert_eq!(
            preview.clip_frame(&path),
            Some(1),
            "classic preview gets a fresh child clock and ignores stop()"
        );
    }

    #[test]
    fn instance_wrappers_assigned_callbacks_conditionals_and_diagnostics_work_in_live_preview() {
        let mut project = Project::blank();
        let root = project.root.clone();
        let mut btn_sym = project.symbol(&root).unwrap().clone();
        btn_sym.id = "play-btn-sym".into();
        btn_sym.name = "PlayButton".into();
        btn_sym.kind = SymbolKind::Button;
        btn_sym.timeline.frame_count = 4;
        btn_sym.timeline.layers[0].id = "btn-layer".into();

        let mut mc_sym = project.symbol(&root).unwrap().clone();
        mc_sym.id = "hero-mc-sym".into();
        mc_sym.name = "HeroClip".into();
        mc_sym.kind = SymbolKind::MovieClip;
        mc_sym.timeline.frame_count = 2;
        mc_sym.timeline.layers[0].id = "mc-layer".into();
        project.symbols.push(btn_sym);
        project.symbols.push(mc_sym);

        let root_sym = project.symbol_mut(&root).unwrap();
        root_sym.timeline.frame_count = 5;
        root_sym.timeline.scripts = vec![crate::FrameScript {
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript2,
                source: "if (false) {\n    stop();\n    trace(\"wrong branch\");\n}\nif (1 + 1 == 2) {\n    stop();\n}\nhero_mc.onRelease = function() {\n    this._x += 25;\n    trace(\"hero clicked at \" + this._x);\n};\nhero_mc.addEventListener(\"enterFrame\", function(e) {\n    this._y += 2;\n});"
                    .into(),
            },
        }];
        root_sym.timeline.layers[0].keyframes[0].elements = vec![
            crate::Element {
                id: "btn-inst".into(),
                name: "go_btn".into(),
                kind: crate::ElementKind::Instance {
                    symbol_id: "play-btn-sym".into(),
                    playback: crate::InstancePlayback::Independent,
                },
                // Stored inside ObjectEvent::Load with classic `on (release)` wrapper, matching FLA/XFL imports!
                scripts: vec![crate::ObjectScript {
                    event: ObjectEvent::Load,
                    script: crate::Script {
                        language: crate::ScriptLanguage::ActionScript2,
                        source: "on (release) {\n    gotoAndStop(4);\n    trace(\"btn jumped root\");\n}\nnew Sound();"
                            .into(),
                    },
                }],
                ..crate::Element::default()
            },
            crate::Element {
                id: "hero-inst".into(),
                name: "hero_mc".into(),
                kind: crate::ElementKind::Instance {
                    symbol_id: "hero-mc-sym".into(),
                    playback: crate::InstancePlayback::Independent,
                },
                scripts: vec![crate::ObjectScript {
                    event: ObjectEvent::Load,
                    script: crate::Script {
                        language: crate::ScriptLanguage::ActionScript2,
                        source: "onClipEvent (load) {\n    this._x = 10;\n}\nonClipEvent (enterFrame) {\n    this._x += 5;\n}"
                            .into(),
                    },
                }],
                ..crate::Element::default()
            },
        ];
        // Extend keyframe span across all 5 frames so instances stay alive on frame 3 (1-based 4)
        project.validate().unwrap();

        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        // `if (false)` did NOT trace "wrong branch", `if (1 + 1 == 2)` halted root, and `hero_mc.onRelease` body did NOT run on assignment!
        assert!(preview.is_root_halted_by_script());
        assert!(preview.drain_traces().is_empty());

        // Unsupported `new Sound()` emitted a located diagnostic once
        let diags = preview.drain_diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].contains("go_btn"));
        assert!(diags[0].contains("new Sound()"));

        // Tick runs both `onClipEvent(enterFrame)` (+5 -> 15) and `addEventListener("enterFrame")` (+2 -> y=2)
        preview.tick(&project);
        let hero_path = vec!["hero-inst".to_owned()];
        assert_eq!(
            preview.player.property(&project, &hero_path, "x"),
            Some(15.0)
        );
        assert_eq!(
            preview.player.property(&project, &hero_path, "y"),
            Some(2.0)
        );

        // Clicking `hero_mc` fires assigned `onRelease` with `this` bound to `hero_mc`!
        preview.click(&project, &hero_path);
        assert_eq!(
            preview.drain_traces(),
            vec!["hero clicked at 40".to_string()]
        );

        // Clicking `go_btn` fires `on (release)` which jumps parent/root timeline to frame 4 (0-based 3)!
        let btn_path = vec!["btn-inst".to_owned()];
        assert!(preview.is_interactive_target(&project, &btn_path));
        preview.click(&project, &btn_path);
        assert_eq!(preview.frame(), 3);
        assert_eq!(preview.drain_traces(), vec!["btn jumped root".to_string()]);
    }
}
