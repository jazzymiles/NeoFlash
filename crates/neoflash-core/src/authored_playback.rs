//! Stateful authoring transport using the shared timeline clocks. Supports both
//! classic Flash script-ignore transport and live AS2/AS3/JS preview (`trace()`,
//! timeline control, instance handlers and display overrides) while keeping the
//! authored document pure.
use crate::{
    AccessibleRole, AuthoredScene, ElementKind, Id, PlaybackCommand, Player, Project,
    ScriptDispatch, ScriptEvent, ScriptLanguage, SymbolKind, VideoCommand,
};
use std::collections::{HashMap, HashSet};

fn script_is_compiled(project: &Project, language: ScriptLanguage) -> bool {
    project
        .script_disposition_for(
            project.effective_script_language(),
            project.also_compile_compatible_scripts,
            language,
        )
        .is_compiled()
}

/// Typed runtime value for live authoring script preview (`T-266`, `T-368`).
#[derive(Clone, Debug, PartialEq)]
pub enum PreviewValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<PreviewValue>),
    Object(Vec<(String, PreviewValue)>),
}

impl PreviewValue {
    pub fn is_truthy(&self, is_as1: bool) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::String(s) => {
                if is_as1 {
                    !s.is_empty() && s != "0" && s != "false"
                } else {
                    !s.is_empty()
                }
            }
            Self::Array(_) | Self::Object(_) => true,
        }
    }

    pub fn to_number(&self, is_as1: bool) -> f64 {
        match self {
            Self::Undefined => {
                if is_as1 {
                    0.0
                } else {
                    f64::NAN
                }
            }
            Self::Null | Self::Bool(false) => 0.0,
            Self::Bool(true) => 1.0,
            Self::Number(n) => *n,
            Self::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    if is_as1 { 0.0 } else { f64::NAN }
                } else if let Some(hex) = trimmed
                    .strip_prefix("0x")
                    .or_else(|| trimmed.strip_prefix("0X"))
                {
                    u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64)
                } else {
                    trimmed
                        .parse::<f64>()
                        .unwrap_or(if is_as1 { 0.0 } else { f64::NAN })
                }
            }
            Self::Array(items) => match items.as_slice() {
                [] => 0.0,
                [single] => single.to_number(is_as1),
                _ => {
                    if is_as1 {
                        0.0
                    } else {
                        f64::NAN
                    }
                }
            },
            Self::Object(_) => {
                if is_as1 {
                    0.0
                } else {
                    f64::NAN
                }
            }
        }
    }

    fn to_compound_lhs_number(&self, is_as1: bool) -> f64 {
        match self {
            Self::Undefined | Self::Null => 0.0,
            other => {
                let n = other.to_number(is_as1);
                if n.is_nan() { 0.0 } else { n }
            }
        }
    }

    pub fn to_u32(&self, is_as1: bool) -> u32 {
        let n = self.to_number(is_as1);
        if !n.is_finite() || n == 0.0 {
            return 0;
        }
        n.trunc().rem_euclid(4_294_967_296.0) as u32
    }

    pub fn to_i32(&self, is_as1: bool) -> i32 {
        self.to_u32(is_as1) as i32
    }

    pub fn type_of_str(&self, lang: ScriptLanguage) -> &'static str {
        match self {
            Self::Undefined => "undefined",
            Self::Null => {
                if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    "null"
                } else {
                    "object"
                }
            }
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) | Self::Object(_) => "object",
        }
    }

    pub fn to_es_string(&self, is_as1: bool) -> String {
        match self {
            Self::Undefined => {
                if is_as1 {
                    String::new()
                } else {
                    "undefined".to_owned()
                }
            }
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Number(n) => format_number(*n),
            Self::String(s) => s.clone(),
            Self::Array(items) => {
                let lang = if is_as1 {
                    ScriptLanguage::ActionScript1
                } else {
                    ScriptLanguage::ActionScript3
                };
                Self::join_array_elements(items, ",", lang)
            }
            Self::Object(_) => "[object Object]".to_owned(),
        }
    }

    pub fn to_es_string_for(&self, lang: ScriptLanguage) -> String {
        match self {
            Self::Undefined => {
                if lang == ScriptLanguage::ActionScript1 {
                    String::new()
                } else {
                    "undefined".to_owned()
                }
            }
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Number(n) => format_number(*n),
            Self::String(s) => s.clone(),
            Self::Array(items) => Self::join_array_elements(items, ",", lang),
            Self::Object(_) => "[object Object]".to_owned(),
        }
    }

    pub fn join_array_elements(items: &[Self], sep: &str, lang: ScriptLanguage) -> String {
        let is_avm1 = matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
        items
            .iter()
            .map(|v| match v {
                Self::Undefined => {
                    if is_avm1 && lang == ScriptLanguage::ActionScript2 {
                        "undefined".to_owned()
                    } else {
                        String::new()
                    }
                }
                Self::Null => {
                    if is_avm1 {
                        "null".to_owned()
                    } else {
                        String::new()
                    }
                }
                Self::Array(nested) => Self::join_array_elements(nested, ",", lang),
                other => other.to_es_string_for(lang),
            })
            .collect::<Vec<_>>()
            .join(sep)
    }

    fn is_nested_container(&self) -> bool {
        match self {
            Self::Array(items) => !items.is_empty(),
            Self::Object(entries) => !entries.is_empty(),
            _ => false,
        }
    }

    fn format_trace_inner(&self, top_level: bool, depth: usize) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Number(n) => format_number(*n),
            Self::String(s) => {
                if top_level {
                    s.clone()
                } else {
                    format!("{s:?}")
                }
            }
            Self::Array(items) => {
                if items.is_empty() {
                    return "[]".to_owned();
                }
                if depth >= 10 {
                    return format!("[Array({})]", items.len());
                }
                let has_nested = items.iter().any(Self::is_nested_container);
                let rendered: Vec<String> = items
                    .iter()
                    .take(200)
                    .map(|v| v.format_trace_inner(false, depth + 1))
                    .collect();
                let has_newline = rendered.iter().any(|s| s.contains('\n'));
                if !has_nested && !has_newline && rendered.len() <= 24 {
                    let joined = rendered.join(", ");
                    if joined.len() + 2 <= 88 {
                        return format!("[{joined}]");
                    }
                }
                let child_indent = "  ".repeat(depth + 1);
                let close_indent = "  ".repeat(depth);
                let body = rendered
                    .into_iter()
                    .map(|s| format!("{child_indent}{s}"))
                    .collect::<Vec<_>>()
                    .join(",\n");
                format!("[\n{body}\n{close_indent}]")
            }
            Self::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_owned();
                }
                if depth >= 10 {
                    return "{...}".to_owned();
                }
                let has_nested = entries.iter().any(|(_, v)| v.is_nested_container());
                let rendered: Vec<String> = entries
                    .iter()
                    .take(200)
                    .map(|(k, v)| {
                        let key_fmt = if is_valid_identifier(k) {
                            k.clone()
                        } else {
                            format!("{k:?}")
                        };
                        format!("{key_fmt}: {}", v.format_trace_inner(false, depth + 1))
                    })
                    .collect();
                let has_newline = rendered.iter().any(|s| s.contains('\n'));
                if !has_nested && !has_newline && rendered.len() <= 6 {
                    let joined = rendered.join(", ");
                    if joined.len() + 4 <= 88 {
                        return format!("{{ {joined} }}");
                    }
                }
                let child_indent = "  ".repeat(depth + 1);
                let close_indent = "  ".repeat(depth);
                let body = rendered
                    .into_iter()
                    .map(|s| format!("{child_indent}{s}"))
                    .collect::<Vec<_>>()
                    .join(",\n");
                format!("{{\n{body}\n{close_indent}}}")
            }
        }
    }

    pub fn to_display_string(&self) -> String {
        self.format_trace_inner(true, 0)
    }

    pub fn strict_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined, Self::Undefined) | (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            _ => false,
        }
    }

    pub fn loose_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undefined | Self::Null, Self::Undefined | Self::Null) => true,
            (Self::Undefined | Self::Null, _) | (_, Self::Undefined | Self::Null) => false,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::Bool(b), rhs) => Self::Number(if *b { 1.0 } else { 0.0 }).loose_eq(rhs),
            (lhs, Self::Bool(b)) => lhs.loose_eq(&Self::Number(if *b { 1.0 } else { 0.0 })),
            (Self::Number(n), Self::String(s)) | (Self::String(s), Self::Number(n)) => {
                let s_num = Self::String(s.clone()).to_number(false);
                *n == s_num
            }
            _ => false,
        }
    }

    pub fn compare_rel(&self, other: &Self, is_as1: bool) -> Option<std::cmp::Ordering> {
        if let (Self::String(a), Self::String(b)) = (self, other) {
            return Some(a.encode_utf16().cmp(b.encode_utf16()));
        }
        let l = self.to_number(is_as1);
        let r = other.to_number(is_as1);
        l.partial_cmp(&r)
    }
}

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
    language: ScriptLanguage,
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
    play_all_scenes: bool,
    live_scripts: bool,
    script_halted_root: bool,
    current_language: ScriptLanguage,
    traces: Vec<String>,
    diagnostics: Vec<String>,
    reported_diagnostics: HashSet<String>,
    vars: HashMap<(Vec<Id>, String), PreviewValue>,
    registered_handlers: Vec<RegisteredHandler>,
    initialized_wrapper_placements: HashSet<(Vec<Id>, u64)>,
    executed_load_blocks: HashSet<(Vec<Id>, u64, usize)>,
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
            play_all_scenes: false,
            live_scripts,
            script_halted_root: false,
            current_language: ScriptLanguage::ActionScript2,
            traces: Vec::new(),
            diagnostics: Vec::new(),
            reported_diagnostics: HashSet::new(),
            vars: HashMap::new(),
            registered_handlers: Vec::new(),
            initialized_wrapper_placements: HashSet::new(),
            executed_load_blocks: HashSet::new(),
            call_depth: 0,
            steps_remaining: MAX_PREVIEW_STEPS_PER_PASS,
        };
        preview.process_scripts(project);
        preview
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn set_play_all_scenes(&mut self, play_all_scenes: bool) {
        if play_all_scenes && !self.play_all_scenes && !self.script_halted_root {
            self.player.set_playing(true);
        }
        self.play_all_scenes = play_all_scenes;
    }

    pub fn is_play_all_scenes(&self) -> bool {
        self.play_all_scenes
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
        let play_all_scenes = self.play_all_scenes;
        let mut prior_traces = std::mem::take(&mut self.traces);
        let mut prior_diagnostics = std::mem::take(&mut self.diagnostics);
        let mut reset = Self::with_live_scripts(project, &self.symbol, frame, live_scripts);
        reset.looping = looping;
        reset.play_all_scenes = play_all_scenes;
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

    fn switch_root_scene(
        &mut self,
        project: &Project,
        target_symbol: Id,
        target_frame: u32,
        playing: bool,
    ) {
        let mut player = Player::with_source_root(project, Some(target_symbol.clone()));
        if target_frame != 0 {
            player.drain_sound_commands();
            player.drain_video_commands();
            player.drain_scripts();
            player.runtime_scripts.clear();
            player.seek(project, target_frame);
        }
        player.set_playing(playing);
        self.symbol = target_symbol.clone();
        self.player = player;
        self.button_root = project
            .symbol(&target_symbol)
            .is_some_and(|s| s.kind == SymbolKind::Button);
        self.script_halted_root = !playing;
        // Retain root-scoped (`path.is_empty()`) listeners and variables across scene transitions,
        // while pruning child placement handlers from the departed scene.
        let root_lifetime = self.player.lifetime_id(&[]);
        self.registered_handlers.retain(|h| {
            if h.target_path.is_empty() {
                root_lifetime.is_some()
            } else {
                false
            }
        });
        if let Some(lt) = root_lifetime {
            for h in &mut self.registered_handlers {
                if h.target_path.is_empty() {
                    h.target_lifetime = lt;
                }
            }
        }
        self.vars.retain(|(path, _), _| path.is_empty());
        self.initialized_wrapper_placements.clear();
        self.executed_load_blocks.clear();
    }

    pub fn tick(&mut self, project: &Project) {
        self.call_depth = 0;
        self.steps_remaining = MAX_PREVIEW_STEPS_PER_PASS;
        // Button Up/Over/Down/Hit are selected authoring states, not a timeline
        // animation that should advance automatically through Hit.
        if !self.button_root {
            let total_frames = project
                .symbol(&self.symbol)
                .map_or(1, |s| s.timeline.frame_count.max(1));
            let scenes = project.scene_ids();
            if self.play_all_scenes
                && scenes.len() > 1
                && !self.script_halted_root
                && self.player.is_playing()
                && self.frame() + 1 >= total_frames
                && let Some(scene_idx) = scenes.iter().position(|id| id == &self.symbol)
            {
                if let Some(next_scene) = scenes.get(scene_idx + 1).cloned() {
                    self.switch_root_scene(project, next_scene, 0, true);
                    self.process_scripts(project);
                    if self.live_scripts {
                        self.run_registered_enter_frame(project);
                    }
                    return;
                } else if self.looping {
                    let first_scene = scenes[0].clone();
                    self.switch_root_scene(project, first_scene, 0, true);
                    self.process_scripts(project);
                    if self.live_scripts {
                        self.run_registered_enter_frame(project);
                    }
                    return;
                } else {
                    self.player.set_playing(false);
                    self.process_scripts(project);
                    if self.live_scripts {
                        self.run_registered_enter_frame(project);
                    }
                    return;
                }
            }
            if !self.looping && !self.script_halted_root && self.frame() + 1 >= total_frames {
                self.player.set_playing(false);
                self.process_scripts(project);
                if self.live_scripts {
                    self.run_registered_enter_frame(project);
                }
                return;
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
            let effective_kind = node.symbol_kind.or_else(|| {
                node.symbol_id
                    .as_deref()
                    .and_then(|id| project.symbol(id))
                    .map(|s| s.kind)
            });
            if effective_kind == Some(SymbolKind::Button) {
                return Some(prefix.to_vec());
            }
            if effective_kind == Some(SymbolKind::Graphic) {
                continue;
            }
            if node
                .scripts
                .iter()
                .any(|os| script_is_compiled(project, os.script.language))
            {
                return Some(prefix.to_vec());
            }
            let lifetime = self.player.lifetime_id(prefix);
            if self.registered_handlers.iter().any(|h| {
                h.event == RegisteredEvent::Click
                    && h.target_path == prefix
                    && lifetime == Some(h.target_lifetime)
                    && script_is_compiled(project, h.language)
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
            if node.symbol_kind.unwrap_or(sym.kind) != SymbolKind::Button {
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
            .filter(|h| {
                h.event == RegisteredEvent::EnterFrame && script_is_compiled(project, h.language)
            })
            .cloned()
            .collect();
        for handler in snapshot {
            if self.player.lifetime_id(&handler.target_path) != Some(handler.target_lifetime) {
                continue;
            }
            let prev_lang = self.current_language;
            self.current_language = handler.language;
            self.execute_block(
                project,
                &handler.target_path,
                &ScriptEvent::EnterFrame,
                &handler.body,
                &handler.origin,
                handler.base_line,
                &HashMap::new(),
            );
            self.current_language = prev_lang;
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
            .filter(|h| {
                h.event == event
                    && h.target_path == target_path
                    && script_is_compiled(project, h.language)
            })
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
            let prev_lang = self.current_language;
            self.current_language = handler.language;
            self.execute_block(
                project,
                &handler.target_path,
                &script_event,
                &handler.body,
                &handler.origin,
                handler.base_line,
                &HashMap::new(),
            );
            self.current_language = prev_lang;
            self.process_scripts(project);
        }
    }

    fn process_scripts(&mut self, project: &Project) {
        let dispatches = self.player.drain_scripts();
        self.player.runtime_scripts.clear();
        if !self.live_scripts {
            return;
        }
        // Ensure `onClipEvent(load)` / `on(...)` wrappers attached inside `ObjectEvent::EnterFrame`
        // or `ObjectEvent::Click` also run their initial Load/registration pass once per placement lifetime.
        let mut extra_load_dispatches = Vec::new();
        let mut paths: Vec<Vec<Id>> = self.player.previous.keys().cloned().collect();
        paths.sort();
        for path in paths {
            let Some(lifetime) = self.player.lifetime_id(&path) else {
                continue;
            };
            if !self
                .initialized_wrapper_placements
                .insert((path.clone(), lifetime))
            {
                continue;
            }
            if let Some(node) = self.player.previous.get(&path) {
                for handler in &node.scripts {
                    if script_is_compiled(project, handler.script.language)
                        && handler.event != crate::ObjectEvent::Load
                        && !crate::runtime::is_as2_legacy_event_wrapper_script(&handler.script)
                        && crate::runtime::contains_uncommented_as2_legacy_event_wrapper(
                            &handler.script,
                        )
                    {
                        extra_load_dispatches.push(ScriptDispatch {
                            target_path: path.clone(),
                            target_lifetime: lifetime,
                            event: ScriptEvent::Load,
                            script: handler.script.clone(),
                            visit_id: 0,
                        });
                    }
                }
            }
        }
        for dispatch in dispatches {
            if !self.player.is_dispatch_current(project, &dispatch) {
                continue;
            }
            self.execute_preview_dispatch(project, &dispatch);
        }
        for dispatch in extra_load_dispatches {
            if self.player.lifetime_id(&dispatch.target_path) != Some(dispatch.target_lifetime) {
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
            language: self.current_language,
            body,
            origin,
            base_line,
        });
    }

    fn has_registered_handler(
        &self,
        target_path: &[Id],
        event: RegisteredEvent,
        body: &str,
    ) -> bool {
        let Some(target_lifetime) = self.player.lifetime_id(target_path) else {
            return false;
        };
        self.registered_handlers.iter().any(|h| {
            h.target_path == target_path
                && h.target_lifetime == target_lifetime
                && h.event == event
                && h.body == body
        })
    }

    fn clear_registered_handler(&mut self, target_path: &[Id], event: RegisteredEvent) {
        let lifetime = self.player.lifetime_id(target_path);
        self.registered_handlers.retain(|h| {
            !(h.target_path == target_path
                && Some(h.target_lifetime) == lifetime
                && h.event == event)
        });
    }

    fn is_as1(&self) -> bool {
        self.current_language == ScriptLanguage::ActionScript1
    }

    fn execute_preview_dispatch(&mut self, project: &Project, dispatch: &ScriptDispatch) {
        if !script_is_compiled(project, dispatch.script.language) {
            return;
        }
        let origin = self.describe_origin(project, dispatch);
        let local_fns = collect_local_functions(&dispatch.script.source);
        let prev_lang = self.current_language;
        self.current_language = dispatch.script.language;
        self.execute_block(
            project,
            &dispatch.target_path,
            &dispatch.event,
            &dispatch.script.source,
            &origin,
            1,
            &local_fns,
        );
        self.current_language = prev_lang;
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
                            if !self.has_registered_handler(path, RegisteredEvent::Click, body) {
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
                    let lifetime = self.player.lifetime_id(path).unwrap_or(0);
                    if matches!(current_event, ScriptEvent::Load | ScriptEvent::Frame(_))
                        && self
                            .executed_load_blocks
                            .insert((path.to_vec(), lifetime, line))
                    {
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
                            if !self.has_registered_handler(path, RegisteredEvent::EnterFrame, body)
                            {
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
                    } else if matches!(current_event, ScriptEvent::Click)
                        && !self.has_registered_handler(path, RegisteredEvent::Click, body)
                    {
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
            // 0d. `delete <target>` (callbacks, array elements, object properties, dynamic variables)
            if let Some(rest) = stmt.strip_prefix("delete ") {
                let target_expr = rest.trim();
                if self
                    .try_apply_delete_target(project, path, target_expr)
                    .is_some()
                {
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
            // 1. Variable declaration: var/let/const name[:Type] [= expr] (supports comma-separated declarations)
            if let Some(rest) = stmt
                .strip_prefix("var ")
                .or_else(|| stmt.strip_prefix("let "))
                .or_else(|| stmt.strip_prefix("const "))
            {
                let scope_path = self.effective_scope_path(project, path);
                for decl in split_top_level_comma(rest) {
                    if let Some((lhs, rhs)) = split_top_level_assign(decl) {
                        let name = lhs.split(':').next().unwrap_or(lhs).trim();
                        let rhs_trimmed = rhs.trim();
                        if let Some(del_target) = rhs_trimmed.strip_prefix("delete ")
                            && let Some(removed) = self.try_apply_delete_target(
                                project,
                                &scope_path,
                                del_target.trim(),
                            )
                        {
                            if !name.is_empty() {
                                self.vars.insert(
                                    (scope_path.clone(), name.to_owned()),
                                    PreviewValue::Bool(removed),
                                );
                            }
                            continue;
                        }
                        if !self.is_supported_rhs(rhs_trimmed) {
                            let snippet = truncate_snippet(stmt, 56);
                            self.record_diagnostic(
                                origin,
                                line,
                                &format!(
                                    "statement `{snippet}` is outside the bounded native stage preview subset"
                                ),
                            );
                        }
                        if !name.is_empty() {
                            let val = if self.is_supported_rhs(rhs_trimmed) {
                                self.eval_preview_value(project, &scope_path, rhs_trimmed)
                            } else {
                                PreviewValue::Undefined
                            };
                            self.vars.insert((scope_path.clone(), name.to_owned()), val);
                        }
                    } else {
                        let name = decl.split(':').next().unwrap_or(decl).trim();
                        if !name.is_empty() {
                            self.vars
                                .entry((scope_path.clone(), name.to_owned()))
                                .or_insert(PreviewValue::Undefined);
                        }
                    }
                }
                continue;
            }
            // 2. trace(...)
            if let Some(inner) = extract_call_arg(stmt, "trace") {
                let args = split_top_level_comma(inner);
                let mut rendered = Vec::with_capacity(args.len());
                for a in args {
                    if !self.is_supported_rhs(a) {
                        let snippet = truncate_snippet(a, 56);
                        self.record_diagnostic(
                            origin,
                            line,
                            &format!(
                                "expression `{snippet}` is outside the bounded native stage preview subset"
                            ),
                        );
                    }
                    rendered.push(self.eval_preview_expr(project, path, a));
                }
                self.traces.push(rendered.join(" "));
                continue;
            }
            // 2b. AS1/AS2 `set(path, value)` named assignment
            if let Some(inner) = extract_call_arg(stmt, "set")
                && !local_fns.contains_key("set")
            {
                if self.current_language == ScriptLanguage::ActionScript3 {
                    self.record_diagnostic(
                        origin,
                        line,
                        "ActionScript 3.0 removed set(); use property assignment instead",
                    );
                    continue;
                }
                if matches!(
                    self.current_language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    let args = split_top_level_comma(inner);
                    if args.len() == 2
                        && self.is_supported_rhs(args[0])
                        && self.is_supported_rhs(args[1])
                    {
                        let scope_path = self.effective_scope_path(project, path);
                        let path_val = self.eval_preview_value(project, &scope_path, args[0]);
                        let path_str = path_val.to_es_string_for(self.current_language);
                        let rhs_val = self.eval_preview_value(project, &scope_path, args[1]);
                        if self.apply_eval_path_write(project, &scope_path, &path_str, rhs_val) {
                            continue;
                        }
                    }
                }
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
            // 4b. Container mutating method call (`arr.push(x, y)`)
            if self.try_apply_container_method_call(project, path, stmt) {
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
            "stop"
                | "play"
                | "nextFrame"
                | "prevFrame"
                | "nextScene"
                | "prevScene"
                | "gotoAndStop"
                | "gotoAndPlay"
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
            "nextScene" if arg.is_empty() => {
                if effective_path.is_empty() {
                    self.apply_scene_step(project, 1);
                }
                true
            }
            "prevScene" if arg.is_empty() => {
                if effective_path.is_empty() {
                    self.apply_scene_step(project, -1);
                }
                true
            }
            "gotoAndStop" => {
                self.apply_goto(project, path, &effective_path, arg, false);
                true
            }
            "gotoAndPlay" => {
                self.apply_goto(project, path, &effective_path, arg, true);
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
            .and_then(|n| {
                n.symbol_kind.or_else(|| {
                    n.symbol_id
                        .as_deref()
                        .and_then(|id| project.symbol(id))
                        .map(|s| s.kind)
                })
            })
            .is_some_and(|kind| kind == SymbolKind::Button)
    }

    /// Classic Flash (`T-240`): `on(...)` on a `Button` executes in its parent MovieClip's timeline
    /// scope, whereas `on(...)` and `onClipEvent(...)` on a `MovieClip` execute in that MovieClip's
    /// own instance scope across frames.
    fn effective_scope_path(&self, project: &Project, path: &[Id]) -> Vec<Id> {
        if !path.is_empty() && self.is_button_instance(project, path) {
            path[..path.len() - 1].to_vec()
        } else {
            path.to_vec()
        }
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

    fn dispatch_switched_scene_scripts(&mut self, project: &Project) {
        if self.live_scripts && self.call_depth < MAX_PREVIEW_CALL_DEPTH && self.steps_remaining > 0
        {
            self.call_depth += 1;
            self.process_scripts(project);
            self.call_depth -= 1;
        } else {
            self.player.drain_scripts();
            self.player.runtime_scripts.clear();
        }
    }

    fn apply_scene_step(&mut self, project: &Project, direction: i32) {
        let scenes = project.scene_ids();
        if scenes.len() > 1 {
            if let Some(idx) = scenes.iter().position(|id| id == &self.symbol) {
                let target_idx = if direction >= 0 {
                    idx.checked_add(1).filter(|&i| i < scenes.len())
                } else {
                    idx.checked_sub(1)
                };
                if let Some(next_idx) = target_idx {
                    let next_sym = scenes[next_idx].clone();
                    self.switch_root_scene(project, next_sym, 0, false);
                    self.dispatch_switched_scene_scripts(project);
                }
            }
            return;
        }
        if let Some(target_frame) = crate::scenes::scene_step(
            self.player.scenes(),
            self.player.frame(),
            if direction >= 0 { 1 } else { -1 },
        ) {
            self.player
                .control(project, &[], PlaybackCommand::GotoAndStop(target_frame));
            self.script_halted_root = true;
        }
    }

    fn apply_goto(
        &mut self,
        project: &Project,
        scope_path: &[Id],
        effective_path: &[Id],
        arg: &str,
        play: bool,
    ) {
        let parts = split_top_level_comma(arg);
        if parts.is_empty() {
            return;
        }

        // Root timeline supports multi-scene navigation:
        // - AS1/AS2: `gotoAndPlay("Scene 2", 1)` (`(scene, frame)`)
        // - AS3/JS/TS: `gotoAndPlay(1, "Scene 2")` (`(frame, scene)`)
        // - Single-arg label: `gotoAndStop("intro_end")` across all project scenes
        if effective_path.is_empty() {
            let is_as2_order = matches!(
                self.current_language,
                ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
            );
            let (frame_val, explicit_scene): (String, Option<String>) = if parts.len() >= 2 {
                let first = self.eval_preview_expr(project, scope_path, parts[0]);
                let second = self.eval_preview_expr(project, scope_path, parts[1]);
                let clean_scene = |raw: String| -> Option<String> {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" {
                        None
                    } else {
                        Some(trimmed.to_owned())
                    }
                };
                if is_as2_order {
                    // AS2 canonical order is `(scene, frame)`, but if arg 0 is numeric and arg 1 is
                    // a non-numeric scene name, also accept `(frame, scene)` cleanly.
                    if first.trim().parse::<f64>().is_ok() && second.trim().parse::<f64>().is_err()
                    {
                        (first, clean_scene(second))
                    } else {
                        (second, clean_scene(first))
                    }
                } else {
                    (first, clean_scene(second))
                }
            } else {
                (self.eval_preview_expr(project, scope_path, parts[0]), None)
            };

            let scenes = project.scene_ids();
            if scenes.len() > 1 {
                if let Some(scene_name) = explicit_scene {
                    let Some(target_sym_id) = scenes.iter().find(|sid| {
                        project
                            .symbol(sid)
                            .is_some_and(|s| s.name == scene_name || s.id == scene_name)
                    }) else {
                        return;
                    };
                    if let Some(target_frame) =
                        Self::resolve_frame_in_symbol(project, target_sym_id, &frame_val)
                    {
                        if target_sym_id == &self.symbol {
                            let cmd = if play {
                                PlaybackCommand::GotoAndPlay(target_frame)
                            } else {
                                PlaybackCommand::GotoAndStop(target_frame)
                            };
                            self.player.control(project, &[], cmd);
                            self.script_halted_root = !play;
                        } else {
                            let target_sym = target_sym_id.clone();
                            self.switch_root_scene(project, target_sym, target_frame, play);
                            self.dispatch_switched_scene_scripts(project);
                        }
                    }
                    return;
                }

                // Single-arg on multi-scene project: check current scene first, then cross-scene labels.
                if let Some(local_frame) =
                    Self::resolve_frame_in_symbol(project, &self.symbol, &frame_val)
                {
                    let cmd = if play {
                        PlaybackCommand::GotoAndPlay(local_frame)
                    } else {
                        PlaybackCommand::GotoAndStop(local_frame)
                    };
                    self.player.control(project, &[], cmd);
                    self.script_halted_root = !play;
                    return;
                }
                let label = frame_val.trim();
                if !label.is_empty() && label.parse::<f64>().is_err() {
                    for sid in &scenes {
                        if sid == &self.symbol {
                            continue;
                        }
                        if let Some(found_frame) = Self::find_label_in_symbol(project, sid, label) {
                            self.switch_root_scene(project, sid.clone(), found_frame, play);
                            self.dispatch_switched_scene_scripts(project);
                            return;
                        }
                    }
                }
                return;
            }

            // Flattened `SceneRange` fallback when `player.scenes()` is populated.
            if !self.player.scenes().is_empty()
                && let Some(root_sym) = project.symbol(&self.symbol)
            {
                let labels = root_sym.timeline.navigable_labels();
                let frame = match frame_val.trim().parse::<f64>() {
                    Ok(num) => crate::scenes::GotoFrame::Number(num),
                    Err(_) => crate::scenes::GotoFrame::Label(frame_val.trim().to_owned()),
                };
                let request = crate::scenes::GotoRequest {
                    frame,
                    scene: explicit_scene,
                    as2: is_as2_order,
                };
                if let Ok(Some(target_frame)) = crate::scenes::resolve_goto(
                    self.player.scenes(),
                    &labels,
                    self.player.frame(),
                    &request,
                ) {
                    let cmd = if play {
                        PlaybackCommand::GotoAndPlay(target_frame)
                    } else {
                        PlaybackCommand::GotoAndStop(target_frame)
                    };
                    self.player.control(project, &[], cmd);
                    self.script_halted_root = !play;
                }
                return;
            }

            if let Some(target_frame) =
                Self::resolve_frame_in_symbol(project, &self.symbol, &frame_val)
            {
                let cmd = if play {
                    PlaybackCommand::GotoAndPlay(target_frame)
                } else {
                    PlaybackCommand::GotoAndStop(target_frame)
                };
                self.player.control(project, &[], cmd);
                self.script_halted_root = !play;
            }
            return;
        }

        // Child MovieClip target (`effective_path` non-empty):
        let evaluated = self.eval_preview_expr(project, scope_path, parts[0]);
        let Some(symbol_id) = self
            .player
            .previous
            .get(effective_path)
            .and_then(|n| n.symbol_id.as_deref())
        else {
            return;
        };
        if let Some(target_frame) = Self::resolve_frame_in_symbol(project, symbol_id, &evaluated) {
            let cmd = if play {
                PlaybackCommand::GotoAndPlay(target_frame)
            } else {
                PlaybackCommand::GotoAndStop(target_frame)
            };
            self.player.control(project, effective_path, cmd);
        }
    }

    fn find_label_in_symbol(project: &Project, symbol_id: &str, label: &str) -> Option<u32> {
        let symbol = project.symbol(symbol_id)?;
        symbol
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find(|k| k.is_navigable_label() && k.label.as_str() == label)
            .map(|k| k.frame)
    }

    fn resolve_frame_in_symbol(project: &Project, symbol_id: &str, evaluated: &str) -> Option<u32> {
        let trimmed = evaluated.trim();
        if let Ok(one_based) = trimmed.parse::<f64>() {
            if !one_based.is_finite() {
                return None;
            }
            let total = project
                .symbol(symbol_id)
                .map_or(1, |s| s.timeline.frame_count.max(1));
            let idx = (one_based.round() as i64 - 1).clamp(0, i64::from(total - 1)) as u32;
            return Some(idx);
        }
        Self::find_label_in_symbol(project, symbol_id, trimmed)
    }

    fn try_apply_property_or_var_mutation(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
    ) -> bool {
        let scope_path = self.effective_scope_path(project, path);
        // Postfix and prefix increment / decrement shorthand: `x++`, `++x`, `this._x++`, `--count`
        if let Some(lhs) = stmt.strip_suffix("++").or_else(|| stmt.strip_prefix("++")) {
            let rewritten = format!("{} += 1", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, &scope_path, &rewritten);
        }
        if let Some(lhs) = stmt.strip_suffix("--").or_else(|| stmt.strip_prefix("--")) {
            let rewritten = format!("{} -= 1", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, &scope_path, &rewritten);
        }

        let (lhs, op, rhs) = if let Some((l, r)) = split_compound_op(stmt, ">>>=") {
            (l, ">>>=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "<<=") {
            (l, "<<=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, ">>=") {
            (l, ">>=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "&=") {
            (l, "&=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "|=") {
            (l, "|=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "^=") {
            (l, "^=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "+=") {
            (l, "+=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "-=") {
            (l, "-=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "*=") {
            (l, "*=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "/=") {
            (l, "/=", r)
        } else if let Some((l, r)) = split_compound_op(stmt, "%=") {
            (l, "%=", r)
        } else if let Some((l, r)) = split_top_level_assign(stmt) {
            (l, "=", r)
        } else {
            return false;
        };

        let (recv_opt, prop_raw) = match split_receiver_member(lhs) {
            Some((r, p)) => (Some(r), p.trim()),
            None => (None, lhs.trim()),
        };

        let is_as1 = self.is_as1();
        let lang = self.current_language;

        if op == "="
            && let Some(del_target) = rhs.trim().strip_prefix("delete ")
            && let Some(removed) =
                self.try_apply_delete_target(project, &scope_path, del_target.trim())
        {
            let bool_rhs = if removed { "true" } else { "false" };
            let rewritten = format!("{} = {bool_rhs}", lhs.trim());
            return self.try_apply_property_or_var_mutation(project, &scope_path, &rewritten);
        }

        // Check if mutating a nested array index or object property (`fig[1][2] = 9`, `state.score += 10`, `grid[r][c] |= 1`)
        if self.try_apply_nested_container_mutation(project, &scope_path, lhs, op, rhs, lang) {
            return true;
        }

        // Check if mutating an existing script variable (`score += 10`, `mask |= 4`, `count = 2`)
        if recv_opt.is_none()
            && self
                .vars
                .contains_key(&(scope_path.clone(), prop_raw.to_owned()))
        {
            if !self.is_supported_rhs(rhs) {
                return false;
            }
            let cur_val = self
                .vars
                .get(&(scope_path.clone(), prop_raw.to_owned()))
                .cloned()
                .unwrap_or(PreviewValue::Undefined);
            let rhs_val = self.eval_preview_value(project, &scope_path, rhs);
            let Some(next_val) = apply_compound_var_op(op, &cur_val, &rhs_val, lang) else {
                return false;
            };
            self.vars
                .insert((scope_path, prop_raw.to_owned()), next_val);
            return true;
        }

        let Some(target_path) =
            self.resolve_receiver_path(project, &scope_path, recv_opt.unwrap_or("this"))
        else {
            return false;
        };

        if matches!(prop_raw, "_visible" | "visible") && op == "=" {
            let val = self.eval_preview_value(project, &scope_path, rhs);
            let visible = val.is_truthy(is_as1);
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
                // Allow dynamic custom field/variable storage on target MovieClip scope
                // (`this.vy = 4`, `vy = 4`, `hero.hp += 10`, `flags |= 2`)
                if is_valid_identifier(prop_raw) && self.is_supported_rhs(rhs) {
                    let cur_val = self
                        .vars
                        .get(&(target_path.clone(), prop_raw.to_owned()))
                        .cloned()
                        .unwrap_or(PreviewValue::Undefined);
                    let rhs_val = self.eval_preview_value(project, &scope_path, rhs);
                    let Some(next_val) = apply_compound_var_op(op, &cur_val, &rhs_val, lang) else {
                        return false;
                    };
                    self.vars
                        .insert((target_path, prop_raw.to_owned()), next_val);
                    return true;
                }
                return false;
            }
        };
        let rhs_eval = self.eval_preview_value(project, &scope_path, rhs);
        let rhs_num = rhs_eval.to_compound_lhs_number(is_as1);
        let cur = self
            .player
            .property(project, &target_path, prop_name)
            .unwrap_or(match prop_name {
                "scale_x" | "scale_y" | "alpha" => 1.0,
                _ => 0.0,
            })
            / scale_factor;
        let cur_pv = PreviewValue::Number(cur);
        let next = match op {
            "+=" => cur + rhs_num,
            "-=" => cur - rhs_num,
            "*=" => cur * rhs_num,
            "/=" => {
                if rhs_num.abs() < 1e-12 {
                    cur
                } else {
                    cur / rhs_num
                }
            }
            "%=" => {
                if rhs_num.abs() < 1e-12 {
                    0.0
                } else {
                    cur % rhs_num
                }
            }
            "&=" => f64::from(cur_pv.to_i32(is_as1) & rhs_eval.to_i32(is_as1)),
            "|=" => f64::from(cur_pv.to_i32(is_as1) | rhs_eval.to_i32(is_as1)),
            "^=" => f64::from(cur_pv.to_i32(is_as1) ^ rhs_eval.to_i32(is_as1)),
            "<<=" => f64::from(
                cur_pv
                    .to_i32(is_as1)
                    .wrapping_shl(rhs_eval.to_u32(is_as1) & 0x1f),
            ),
            ">>=" => f64::from(
                cur_pv
                    .to_i32(is_as1)
                    .wrapping_shr(rhs_eval.to_u32(is_as1) & 0x1f),
            ),
            ">>>=" => f64::from(
                cur_pv
                    .to_u32(is_as1)
                    .wrapping_shr(rhs_eval.to_u32(is_as1) & 0x1f),
            ),
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
        let is_as1 = self.is_as1();
        self.eval_preview_value(project, path, cond)
            .is_truthy(is_as1)
    }

    fn eval_preview_expr(&self, project: &Project, path: &[Id], expr: &str) -> String {
        self.eval_preview_value(project, path, expr)
            .to_display_string()
    }

    pub fn eval_preview_value(&self, project: &Project, path: &[Id], expr: &str) -> PreviewValue {
        let scope_path = self.effective_scope_path(project, path);
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let is_avm1 = matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
        let mut trimmed = expr.trim();
        while trimmed.starts_with('(') && trimmed.ends_with(')') && is_single_paren_group(trimmed) {
            trimmed = trimmed[1..trimmed.len() - 1].trim();
        }
        if trimmed.is_empty() {
            return PreviewValue::String(String::new());
        }
        // 1. Ternary `cond ? yes : no`
        if let Some((cond_s, yes_s, no_s)) = split_top_level_ternary(trimmed) {
            return if self.eval_preview_condition(project, &scope_path, cond_s) {
                self.eval_preview_value(project, &scope_path, yes_s)
            } else {
                self.eval_preview_value(project, &scope_path, no_s)
            };
        }
        // 2. Logical OR (`||` and AS1/AS2 `or`)
        let or_ops: &[&str] = if is_avm1 { &["||", "or"] } else { &["||"] };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, or_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            if op == "or" {
                let l_ok = lv.is_truthy(is_as1);
                let rv = self.eval_preview_value(project, &scope_path, rhs_s);
                return PreviewValue::Bool(l_ok || rv.is_truthy(is_as1));
            }
            if lv.is_truthy(is_as1) {
                return lv;
            }
            return self.eval_preview_value(project, &scope_path, rhs_s);
        }
        // 3. Logical AND (`&&` and AS1/AS2 `and`)
        let and_ops: &[&str] = if is_avm1 { &["&&", "and"] } else { &["&&"] };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, and_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            if op == "and" {
                let l_ok = lv.is_truthy(is_as1);
                let rv = self.eval_preview_value(project, &scope_path, rhs_s);
                return PreviewValue::Bool(l_ok && rv.is_truthy(is_as1));
            }
            if !lv.is_truthy(is_as1) {
                return lv;
            }
            return self.eval_preview_value(project, &scope_path, rhs_s);
        }
        // 4. Bitwise OR (`|`)
        let bitor_parts = split_top_level_op(trimmed, "|");
        if bitor_parts.len() > 1 {
            let mut iter = bitor_parts.into_iter().map(|p| {
                self.eval_preview_value(project, &scope_path, p)
                    .to_i32(is_as1)
            });
            let first = iter.next().unwrap_or(0);
            return PreviewValue::Number(f64::from(iter.fold(first, |acc, r| acc | r)));
        }
        // 5. Bitwise XOR (`^`)
        let bitxor_parts = split_top_level_op(trimmed, "^");
        if bitxor_parts.len() > 1 {
            let mut iter = bitxor_parts.into_iter().map(|p| {
                self.eval_preview_value(project, &scope_path, p)
                    .to_i32(is_as1)
            });
            let first = iter.next().unwrap_or(0);
            return PreviewValue::Number(f64::from(iter.fold(first, |acc, r| acc ^ r)));
        }
        // 6. Bitwise AND (`&`)
        let bitand_parts = split_top_level_op(trimmed, "&");
        if bitand_parts.len() > 1 {
            let mut iter = bitand_parts.into_iter().map(|p| {
                self.eval_preview_value(project, &scope_path, p)
                    .to_i32(is_as1)
            });
            let first = iter.next().unwrap_or(0);
            return PreviewValue::Number(f64::from(iter.fold(first, |acc, r| acc & r)));
        }
        // 7. Equality (`===`, `!==`, `==`, `!=`, plus AS1/AS2 `<>`, `eq`, `ne`)
        let eq_ops: &[&str] = if is_avm1 {
            &["===", "!==", "<>", "==", "!=", "eq", "ne"]
        } else {
            &["===", "!==", "==", "!="]
        };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, eq_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            let res = match op {
                "===" => lv.strict_eq(&rv),
                "!==" => !lv.strict_eq(&rv),
                "==" => lv.loose_eq(&rv),
                "!=" => !lv.loose_eq(&rv),
                "<>" => lv.to_number(is_as1) != rv.to_number(is_as1),
                "eq" => lv.to_es_string_for(lang) == rv.to_es_string_for(lang),
                "ne" => lv.to_es_string_for(lang) != rv.to_es_string_for(lang),
                _ => false,
            };
            return PreviewValue::Bool(res);
        }
        // 8. Relational & Membership / Type (`<=`, `>=`, `<`, `>`, AS1/AS2 `lt`..`ge`, `in`, `instanceof`, AS3 `is`/`as`)
        let rel_ops: &[&str] = if is_avm1 {
            &[
                "<=",
                ">=",
                "<",
                ">",
                "lt",
                "le",
                "gt",
                "ge",
                "instanceof",
                "in",
            ]
        } else if lang == ScriptLanguage::ActionScript3 {
            &["<=", ">=", "<", ">", "instanceof", "in", "is", "as"]
        } else {
            &["<=", ">=", "<", ">", "instanceof", "in"]
        };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, rel_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            if matches!(op, "instanceof" | "is" | "as") {
                let ty = rhs_s.trim();
                let matched = match ty {
                    "Array" => matches!(lv, PreviewValue::Array(_)),
                    "Object" => {
                        matches!(lv, PreviewValue::Array(_) | PreviewValue::Object(_))
                            || (op != "instanceof"
                                && !matches!(lv, PreviewValue::Undefined | PreviewValue::Null))
                    }
                    "String" => op != "instanceof" && matches!(lv, PreviewValue::String(_)),
                    "Boolean" => op != "instanceof" && matches!(lv, PreviewValue::Bool(_)),
                    "Number" => op != "instanceof" && matches!(lv, PreviewValue::Number(_)),
                    "int" => {
                        op != "instanceof"
                            && matches!(
                                lv,
                                PreviewValue::Number(n)
                                    if n.is_finite()
                                        && (n - n.trunc()).abs() < 1e-9
                                        && n >= f64::from(i32::MIN)
                                        && n <= f64::from(i32::MAX)
                            )
                    }
                    "uint" => {
                        op != "instanceof"
                            && matches!(
                                lv,
                                PreviewValue::Number(n)
                                    if n.is_finite()
                                        && (n - n.trunc()).abs() < 1e-9
                                        && n >= 0.0
                                        && n <= f64::from(u32::MAX)
                            )
                    }
                    "*" => op != "instanceof",
                    _ => false,
                };
                if op == "as" {
                    return if matched { lv } else { PreviewValue::Null };
                }
                return PreviewValue::Bool(matched);
            }
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            if op == "in" {
                let has = match &rv {
                    PreviewValue::Array(items) => {
                        let key_str = lv.to_es_string_for(lang);
                        if key_str == "length" {
                            true
                        } else if let Ok(idx) = key_str.trim().parse::<usize>() {
                            items
                                .get(idx)
                                .is_some_and(|v| !matches!(v, PreviewValue::Undefined))
                        } else {
                            false
                        }
                    }
                    PreviewValue::Object(entries) => {
                        let key_str = lv.to_es_string_for(lang);
                        entries.iter().any(|(k, _)| k == &key_str)
                    }
                    _ => false,
                };
                return PreviewValue::Bool(has);
            }
            if matches!(op, "lt" | "le" | "gt" | "ge") {
                let ls = lv.to_es_string_for(lang);
                let rs = rv.to_es_string_for(lang);
                let ord = ls.encode_utf16().cmp(rs.encode_utf16());
                let res = match op {
                    "lt" => ord.is_lt(),
                    "le" => !ord.is_gt(),
                    "gt" => ord.is_gt(),
                    "ge" => !ord.is_lt(),
                    _ => false,
                };
                return PreviewValue::Bool(res);
            }
            let ord = lv.compare_rel(&rv, is_as1);
            let res = match op {
                "<" => ord == Some(std::cmp::Ordering::Less),
                "<=" => matches!(
                    ord,
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ),
                ">" => ord == Some(std::cmp::Ordering::Greater),
                ">=" => matches!(
                    ord,
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                ),
                _ => false,
            };
            return PreviewValue::Bool(res);
        }
        // 9. Bitwise Shifts (`>>>`, `<<`, `>>` — left-to-right)
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, &[">>>", "<<", ">>"]) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            let shift = rv.to_u32(is_as1) & 0x1f;
            let res = match op {
                "<<" => f64::from(lv.to_i32(is_as1).wrapping_shl(shift)),
                ">>" => f64::from(lv.to_i32(is_as1).wrapping_shr(shift)),
                ">>>" => f64::from(lv.to_u32(is_as1).wrapping_shr(shift)),
                _ => 0.0,
            };
            return PreviewValue::Number(res);
        }
        // 10. Additive (`+`, binary `-`, and AS1/AS2 `add` — left-to-right)
        let add_ops: &[&str] = if is_avm1 {
            &["+", "-", "add"]
        } else {
            &["+", "-"]
        };
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, add_ops) {
            let lv = self.eval_preview_value(project, &scope_path, lhs_s);
            let rv = self.eval_preview_value(project, &scope_path, rhs_s);
            return match op {
                "+" => eval_add_values(lv, rv, lang),
                "-" => PreviewValue::Number(lv.to_number(is_as1) - rv.to_number(is_as1)),
                "add" => {
                    let mut s = lv.to_es_string_for(lang);
                    s.push_str(&rv.to_es_string_for(lang));
                    PreviewValue::String(s)
                }
                _ => PreviewValue::Undefined,
            };
        }
        // 11. Multiplicative (`*`, `/`, `%` — left-to-right)
        if let Some((lhs_s, op, rhs_s)) = rfind_top_level_ops(trimmed, &["*", "/", "%"]) {
            let lv = self
                .eval_preview_value(project, &scope_path, lhs_s)
                .to_number(is_as1);
            let rv = self
                .eval_preview_value(project, &scope_path, rhs_s)
                .to_number(is_as1);
            return match op {
                "*" => PreviewValue::Number(lv * rv),
                "/" => {
                    if lv.is_nan() || rv.is_nan() {
                        PreviewValue::Number(f64::NAN)
                    } else if rv.abs() < 1e-12 {
                        PreviewValue::Number(0.0)
                    } else {
                        PreviewValue::Number(lv / rv)
                    }
                }
                "%" => {
                    if lv.is_nan() || rv.is_nan() {
                        PreviewValue::Number(f64::NAN)
                    } else if rv.abs() < 1e-12 {
                        PreviewValue::Number(0.0)
                    } else {
                        PreviewValue::Number(lv % rv)
                    }
                }
                _ => PreviewValue::Undefined,
            };
        }
        self.eval_preview_atom_value(project, &scope_path, trimmed)
    }

    fn eval_preview_atom_value(&self, project: &Project, path: &[Id], atom: &str) -> PreviewValue {
        let s = atom.trim();
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
            || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
        {
            return PreviewValue::String(unescape_string_literal(&s[1..s.len() - 1]));
        }
        match s {
            "true" => return PreviewValue::Bool(true),
            "false" => return PreviewValue::Bool(false),
            "null" => return PreviewValue::Null,
            "undefined" => return PreviewValue::Undefined,
            "NaN" => return PreviewValue::Number(f64::NAN),
            "Infinity" => return PreviewValue::Number(f64::INFINITY),
            "-Infinity" => return PreviewValue::Number(f64::NEG_INFINITY),
            "newline"
                if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) =>
            {
                return PreviewValue::String("\n".to_owned());
            }
            _ => {}
        }
        if let Some(inner) = s.strip_prefix('!') {
            return PreviewValue::Bool(!self.eval_preview_condition(project, path, inner));
        }
        if matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        ) && let Some(inner) = strip_unary_keyword(s, "not")
        {
            return PreviewValue::Bool(!self.eval_preview_condition(project, path, inner));
        }
        if let Some(inner) = s.strip_prefix('~')
            && !inner.trim().is_empty()
        {
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::Number(f64::from(!val.to_i32(is_as1)));
        }
        if let Some(inner) = strip_unary_keyword(s, "typeof") {
            let trimmed_inner = inner.trim();
            if self
                .resolve_receiver_path(project, path, trimmed_inner)
                .is_some()
                && !self
                    .vars
                    .contains_key(&(path.to_vec(), trimmed_inner.to_owned()))
            {
                let mc_ty = if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    "movieclip"
                } else {
                    "object"
                };
                return PreviewValue::String(mc_ty.to_owned());
            }
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::String(val.type_of_str(lang).to_owned());
        }
        if let Some(inner) = strip_unary_keyword(s, "void") {
            let _ = self.eval_preview_value(project, path, inner);
            return PreviewValue::Undefined;
        }
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
            && let Ok(v) = u64::from_str_radix(hex, 16)
        {
            return PreviewValue::Number(v as f64);
        }
        if let Ok(n) = s.parse::<f64>() {
            return PreviewValue::Number(n);
        }
        if let Some(inner) = s.strip_prefix('-')
            && !inner.trim().is_empty()
        {
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::Number(-val.to_number(is_as1));
        }
        if let Some(inner) = s.strip_prefix('+')
            && !inner.trim().is_empty()
        {
            let val = self.eval_preview_value(project, path, inner);
            return PreviewValue::Number(val.to_number(is_as1));
        }
        if let Some(arr_val) = self.eval_array_or_object_literal(project, path, s) {
            return arr_val;
        }
        if let Some(builtin_val) = self.eval_builtin_pure_call(project, path, s) {
            return builtin_val;
        }
        if let Some(indexed_val) = self.eval_postfix_container_access(project, path, s) {
            return indexed_val;
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
                PreviewValue::Number(f64::from(f + 1))
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
                PreviewValue::Number(f64::from(count))
            }
            "_name" | "name" => PreviewValue::String(
                target_path
                    .last()
                    .map(|id| {
                        let parent = &target_path[..target_path.len() - 1];
                        self.element_display_name(project, parent, id)
                    })
                    .unwrap_or_else(|| "root".into()),
            ),
            "_visible" | "visible" => {
                let vis = self
                    .player
                    .overrides
                    .get(&target_path)
                    .and_then(|o| o.visible)
                    .unwrap_or(true);
                PreviewValue::Bool(vis)
            }
            "x" | "_x" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "x")
                    .unwrap_or(0.0),
            ),
            "y" | "_y" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "y")
                    .unwrap_or(0.0),
            ),
            "rotation" | "_rotation" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "rotation")
                    .unwrap_or(0.0),
            ),
            "scaleX" | "scale_x" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_x")
                    .unwrap_or(1.0),
            ),
            "_xscale" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_x")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "scaleY" | "scale_y" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_y")
                    .unwrap_or(1.0),
            ),
            "_yscale" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "scale_y")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "alpha" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "alpha")
                    .unwrap_or(1.0),
            ),
            "_alpha" => PreviewValue::Number(
                self.player
                    .property(project, &target_path, "alpha")
                    .map_or(100.0, |a| a * 100.0),
            ),
            _ => PreviewValue::Undefined,
        }
    }

    fn eval_builtin_pure_call(
        &self,
        project: &Project,
        path: &[Id],
        s: &str,
    ) -> Option<PreviewValue> {
        let (head, args_raw) = parse_qualified_call(s.trim())?;
        if !is_supported_builtin_pure_call_head_for(head, self.current_language) {
            return None;
        }
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let args: Vec<PreviewValue> = split_top_level_comma(args_raw)
            .into_iter()
            .map(|a| self.eval_preview_value(project, path, a))
            .collect();
        match head {
            "eval" => {
                let first = args.first()?;
                if matches!(first, PreviewValue::Array(_) | PreviewValue::Object(_)) {
                    return Some(first.clone());
                }
                if matches!(
                    lang,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                ) {
                    let PreviewValue::String(ref path_str) = *first else {
                        return Some(PreviewValue::Undefined);
                    };
                    return Some(self.resolve_eval_path_value(project, path, path_str, is_as1));
                }
                if matches!(
                    lang,
                    ScriptLanguage::JavaScript | ScriptLanguage::TypeScript
                ) {
                    let PreviewValue::String(ref code_str) = *first else {
                        return Some(first.clone());
                    };
                    if code_str.contains("eval") {
                        return Some(PreviewValue::Undefined);
                    }
                    return Some(self.eval_preview_value(project, path, code_str));
                }
                None
            }
            "chr" | "mbchr" => {
                let unit = args.first().map_or(0, |v| v.to_i32(is_as1)) as u16;
                if unit == 0 {
                    Some(PreviewValue::String(String::new()))
                } else {
                    let ch = char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER);
                    Some(PreviewValue::String(ch.to_string()))
                }
            }
            "ord" | "mbord" => {
                let st = args
                    .first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang));
                let unit = st.encode_utf16().next().unwrap_or(0);
                let code = if unit == 0 {
                    0.0
                } else {
                    f64::from(u32::from(
                        char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER),
                    ))
                };
                Some(PreviewValue::Number(code))
            }
            "length" | "mblength" => {
                let st = args
                    .first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang));
                Some(PreviewValue::Number(st.encode_utf16().count() as f64))
            }
            "substring" | "mbsubstring" => {
                let st = args
                    .first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang));
                let units: Vec<u16> = st.encode_utf16().collect();
                let start_1 = args.get(1).map_or(0, |v| v.to_i32(is_as1));
                let start = if start_1 >= 1 {
                    (start_1 as usize).saturating_sub(1).min(units.len())
                } else {
                    0
                };
                let count = args
                    .get(2)
                    .and_then(|v| usize::try_from(v.to_i32(is_as1)).ok());
                let end = count
                    .and_then(|c| start.checked_add(c))
                    .filter(|e| *e <= units.len())
                    .unwrap_or(units.len());
                Some(PreviewValue::String(String::from_utf16_lossy(
                    &units[start.min(end)..end],
                )))
            }
            "String" => Some(PreviewValue::String(
                args.first()
                    .map_or_else(String::new, |v| v.to_es_string_for(lang)),
            )),
            "Number" => Some(PreviewValue::Number(
                args.first().map_or(0.0, |v| v.to_number(is_as1)),
            )),
            "Boolean" => Some(PreviewValue::Bool(
                args.first().is_some_and(|v| v.is_truthy(is_as1)),
            )),
            "int" => Some(PreviewValue::Number(f64::from(
                args.first().map_or(0, |v| v.to_i32(is_as1)),
            ))),
            "uint" => Some(PreviewValue::Number(f64::from(
                args.first().map_or(0, |v| v.to_u32(is_as1)),
            ))),
            "parseFloat" => {
                let st = args
                    .first()
                    .map(|v| v.to_es_string_for(lang))
                    .unwrap_or_default();
                let trimmed = st.trim_start();
                let end = trimmed
                    .char_indices()
                    .take_while(|&(idx, c)| {
                        c.is_ascii_digit()
                            || c == '.'
                            || ((c == '+' || c == '-') && idx == 0)
                            || c == 'e'
                            || c == 'E'
                    })
                    .map(|(idx, c)| idx + c.len_utf8())
                    .last()
                    .unwrap_or(0);
                let parsed = trimmed[..end].parse::<f64>().unwrap_or(f64::NAN);
                Some(PreviewValue::Number(parsed))
            }
            "parseInt" => {
                let st = args
                    .first()
                    .map(|v| v.to_es_string_for(lang))
                    .unwrap_or_default();
                let radix_raw = args.get(1).map_or(0.0, |v| v.to_number(is_as1));
                let mut trimmed = st.trim_start();
                let mut sign = 1.0f64;
                if let Some(rest) = trimmed.strip_prefix('-') {
                    sign = -1.0;
                    trimmed = rest;
                } else if let Some(rest) = trimmed.strip_prefix('+') {
                    trimmed = rest;
                }
                let mut radix = if radix_raw.is_finite() && radix_raw != 0.0 {
                    radix_raw.trunc() as i32
                } else {
                    0
                };
                if (radix == 0 || radix == 16)
                    && let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                {
                    trimmed = hex;
                    radix = 16;
                } else if radix == 0 {
                    radix = 10;
                }
                if !(2..=36).contains(&radix) {
                    return Some(PreviewValue::Number(f64::NAN));
                }
                let r_u32 = radix as u32;
                let digits: String = trimmed.chars().take_while(|c| c.is_digit(r_u32)).collect();
                if digits.is_empty() {
                    return Some(PreviewValue::Number(f64::NAN));
                }
                let val = i128::from_str_radix(&digits, r_u32)
                    .map(|v| (v as f64) * sign)
                    .unwrap_or(f64::NAN);
                Some(PreviewValue::Number(val))
            }
            "Math.abs" => Some(PreviewValue::Number(
                args.first().map_or(f64::NAN, |v| v.to_number(is_as1).abs()),
            )),
            "Math.floor" => Some(PreviewValue::Number(
                args.first()
                    .map_or(f64::NAN, |v| v.to_number(is_as1).floor()),
            )),
            "Math.ceil" => Some(PreviewValue::Number(
                args.first()
                    .map_or(f64::NAN, |v| v.to_number(is_as1).ceil()),
            )),
            "Math.round" => Some(PreviewValue::Number(args.first().map_or(f64::NAN, |v| {
                let n = v.to_number(is_as1);
                if n.is_finite() { (n + 0.5).floor() } else { n }
            }))),
            "Math.sqrt" => Some(PreviewValue::Number(
                args.first()
                    .map_or(f64::NAN, |v| v.to_number(is_as1).sqrt()),
            )),
            "Math.pow" => {
                let a = args.first().map_or(f64::NAN, |v| v.to_number(is_as1));
                let b = args.get(1).map_or(f64::NAN, |v| v.to_number(is_as1));
                Some(PreviewValue::Number(a.powf(b)))
            }
            "Math.min" => {
                if args.is_empty() {
                    return Some(PreviewValue::Number(f64::INFINITY));
                }
                let mut acc = f64::INFINITY;
                for v in &args {
                    let n = v.to_number(is_as1);
                    if n.is_nan() {
                        return Some(PreviewValue::Number(f64::NAN));
                    }
                    if n < acc {
                        acc = n;
                    }
                }
                Some(PreviewValue::Number(acc))
            }
            "Math.max" => {
                if args.is_empty() {
                    return Some(PreviewValue::Number(f64::NEG_INFINITY));
                }
                let mut acc = f64::NEG_INFINITY;
                for v in &args {
                    let n = v.to_number(is_as1);
                    if n.is_nan() {
                        return Some(PreviewValue::Number(f64::NAN));
                    }
                    if n > acc {
                        acc = n;
                    }
                }
                Some(PreviewValue::Number(acc))
            }
            _ => None,
        }
    }

    fn eval_array_or_object_literal(
        &self,
        project: &Project,
        path: &[Id],
        s: &str,
    ) -> Option<PreviewValue> {
        let trimmed = s.trim();
        if trimmed.starts_with('[')
            && find_matching_delim(trimmed, b'[', b']') == Some(trimmed.len() - 1)
        {
            let inner = trimmed[1..trimmed.len() - 1].trim();
            if inner.is_empty() {
                return Some(PreviewValue::Array(Vec::new()));
            }
            let mut parts = split_top_level_comma(inner);
            if parts.last() == Some(&"") {
                parts.pop();
            }
            let items = parts
                .into_iter()
                .map(|part| {
                    if part.trim().is_empty() {
                        PreviewValue::Undefined
                    } else {
                        self.eval_preview_value(project, path, part)
                    }
                })
                .collect();
            return Some(PreviewValue::Array(items));
        }

        let array_call_args = if trimmed == "new Array" {
            Some("")
        } else if let Some(rest) = trimmed.strip_prefix("new ") {
            extract_call_arg(rest.trim_start(), "Array")
        } else {
            extract_call_arg(trimmed, "Array")
        };
        if let Some(arg_str) = array_call_args {
            let args = split_top_level_comma(arg_str);
            if args.is_empty() {
                return Some(PreviewValue::Array(Vec::new()));
            }
            if args.len() == 1 {
                let first = self.eval_preview_value(project, path, args[0]);
                if let PreviewValue::Number(n) = first
                    && n.is_finite()
                    && n >= 0.0
                    && (n - n.trunc()).abs() < 1e-9
                    && (n as usize) <= 10_000
                {
                    return Some(PreviewValue::Array(vec![
                        PreviewValue::Undefined;
                        n as usize
                    ]));
                }
                return Some(PreviewValue::Array(vec![first]));
            }
            let items = args
                .into_iter()
                .map(|a| self.eval_preview_value(project, path, a))
                .collect();
            return Some(PreviewValue::Array(items));
        }

        if trimmed == "new Object"
            || trimmed
                .strip_prefix("new ")
                .and_then(|r| extract_call_arg(r.trim_start(), "Object"))
                == Some("")
        {
            return Some(PreviewValue::Object(Vec::new()));
        }

        if trimmed.starts_with('{')
            && find_matching_delim(trimmed, b'{', b'}') == Some(trimmed.len() - 1)
        {
            let inner = trimmed[1..trimmed.len() - 1].trim();
            if inner.is_empty() {
                return Some(PreviewValue::Object(Vec::new()));
            }
            let mut parts = split_top_level_comma(inner);
            if parts.last() == Some(&"") {
                parts.pop();
            }
            let mut entries: Vec<(String, PreviewValue)> = Vec::with_capacity(parts.len());
            for entry_str in parts {
                let (raw_key, raw_val) = split_top_level_colon(entry_str)?;
                let key = parse_object_literal_key(raw_key)?;
                let val = self.eval_preview_value(project, path, raw_val);
                if let Some((_, slot)) = entries.iter_mut().find(|(k, _)| k == &key) {
                    *slot = val;
                } else {
                    entries.push((key, val));
                }
            }
            return Some(PreviewValue::Object(entries));
        }

        None
    }

    fn resolve_container_chain<'a>(
        &self,
        project: &Project,
        path: &[Id],
        base: &'a str,
        tokens: &[PostfixToken<'a>],
    ) -> Option<(PreviewValue, usize)> {
        if base.starts_with('(')
            || base.starts_with('[')
            || base.starts_with('{')
            || base.starts_with('"')
            || base.starts_with('\'')
            || base.starts_with("new ")
            || base.ends_with(')')
        {
            return Some((self.eval_preview_value(project, path, base), 0));
        }
        if let Some(val) = self.vars.get(&(path.to_vec(), base.to_owned())) {
            return Some((val.clone(), 0));
        }
        // Check receiver-qualified container variable, e.g. `this.fig[0]`, `_root.board[1][2]`, `mc.state.hp`
        let mut recv_prefix = base.to_owned();
        for (idx, tok) in tokens.iter().enumerate() {
            let PostfixToken::Prop(prop_name) = tok else {
                break;
            };
            if let Some(recv_path) = self.resolve_receiver_path(project, path, &recv_prefix)
                && let Some(val) = self.vars.get(&(recv_path, (*prop_name).to_owned()))
                && (idx + 1 < tokens.len()
                    || matches!(val, PreviewValue::Array(_) | PreviewValue::Object(_)))
            {
                return Some((val.clone(), idx + 1));
            }
            recv_prefix.push('.');
            recv_prefix.push_str(prop_name);
        }
        None
    }

    fn resolve_mutable_container_root<'a>(
        &self,
        project: &Project,
        scope_path: &[Id],
        base: &'a str,
        tokens: &[PostfixToken<'a>],
        allow_empty_remaining: bool,
    ) -> Option<(Vec<Id>, String, usize)> {
        if let Some(PreviewValue::Array(_) | PreviewValue::Object(_)) =
            self.vars.get(&(scope_path.to_vec(), base.to_owned()))
            && (allow_empty_remaining || !tokens.is_empty())
        {
            return Some((scope_path.to_vec(), base.to_owned(), 0));
        }
        let mut recv_prefix = base.to_owned();
        for (idx, tok) in tokens.iter().enumerate() {
            let PostfixToken::Prop(prop_name) = tok else {
                break;
            };
            if let Some(recv_path) = self.resolve_receiver_path(project, scope_path, &recv_prefix)
                && let Some(PreviewValue::Array(_) | PreviewValue::Object(_)) =
                    self.vars.get(&(recv_path.clone(), (*prop_name).to_owned()))
                && (allow_empty_remaining || idx + 1 < tokens.len())
            {
                return Some((recv_path, (*prop_name).to_owned(), idx + 1));
            }
            recv_prefix.push('.');
            recv_prefix.push_str(prop_name);
        }
        None
    }

    fn eval_postfix_container_access(
        &self,
        project: &Project,
        path: &[Id],
        s: &str,
    ) -> Option<PreviewValue> {
        let (base, tokens) = parse_postfix_steps(s.trim())?;
        if tokens.is_empty() {
            return None;
        }
        let (mut cur, start_idx) = self.resolve_container_chain(project, path, base, &tokens)?;
        if start_idx >= tokens.len() {
            return Some(cur);
        }
        let is_as1 = self.is_as1();
        for tok in &tokens[start_idx..] {
            cur = match tok {
                PostfixToken::Prop(prop) => match &cur {
                    PreviewValue::Array(items) if *prop == "length" => {
                        PreviewValue::Number(items.len() as f64)
                    }
                    PreviewValue::String(st) if *prop == "length" => {
                        PreviewValue::Number(st.encode_utf16().count() as f64)
                    }
                    PreviewValue::Object(entries) => entries
                        .iter()
                        .find(|(k, _)| k == prop)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(PreviewValue::Undefined),
                    _ => PreviewValue::Undefined,
                },
                PostfixToken::Index(idx_expr) => {
                    let key_val = self.eval_preview_value(project, path, idx_expr);
                    match (&cur, &key_val) {
                        (PreviewValue::Array(items), PreviewValue::Number(n))
                            if n.is_finite() && *n >= 0.0 && (*n - n.trunc()).abs() < 1e-9 =>
                        {
                            items
                                .get(*n as usize)
                                .cloned()
                                .unwrap_or(PreviewValue::Undefined)
                        }
                        (PreviewValue::Array(items), PreviewValue::String(st)) => {
                            if st == "length" {
                                PreviewValue::Number(items.len() as f64)
                            } else if let Ok(idx) = st.trim().parse::<usize>() {
                                items.get(idx).cloned().unwrap_or(PreviewValue::Undefined)
                            } else {
                                PreviewValue::Undefined
                            }
                        }
                        (PreviewValue::Object(entries), _) => {
                            let key = key_val.to_es_string(is_as1);
                            entries
                                .iter()
                                .find(|(k, _)| k == &key)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(PreviewValue::Undefined)
                        }
                        _ => PreviewValue::Undefined,
                    }
                }
                PostfixToken::Call(method, args_raw) => {
                    self.eval_postfix_method(project, path, &cur, method, args_raw)?
                }
            };
        }
        Some(cur)
    }

    fn eval_postfix_method(
        &self,
        project: &Project,
        path: &[Id],
        receiver: &PreviewValue,
        method: &str,
        args_raw: &str,
    ) -> Option<PreviewValue> {
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let args: Vec<PreviewValue> = split_top_level_comma(args_raw)
            .into_iter()
            .map(|a| self.eval_preview_value(project, path, a))
            .collect();

        match (receiver, method) {
            (PreviewValue::Array(items), "join") => {
                let sep = match args.first() {
                    None | Some(PreviewValue::Undefined) => ",".to_owned(),
                    Some(v) => v.to_es_string_for(lang),
                };
                Some(PreviewValue::String(PreviewValue::join_array_elements(
                    items, &sep, lang,
                )))
            }
            (PreviewValue::Array(items), "toString") if args.is_empty() => Some(
                PreviewValue::String(PreviewValue::join_array_elements(items, ",", lang)),
            ),
            (PreviewValue::Array(items), "slice") => {
                let len = items.len() as i64;
                let rel = |raw: Option<&PreviewValue>, default: i64| -> usize {
                    let Some(v) = raw else {
                        return default.clamp(0, len) as usize;
                    };
                    if matches!(v, PreviewValue::Undefined) {
                        return default.clamp(0, len) as usize;
                    }
                    let n = v.to_number(is_as1);
                    if n.is_nan() || n == 0.0 {
                        0
                    } else if n == f64::INFINITY {
                        len as usize
                    } else if n == f64::NEG_INFINITY {
                        0
                    } else {
                        let i = n.trunc() as i64;
                        (if i < 0 { (len + i).max(0) } else { i.min(len) }) as usize
                    }
                };
                let start = rel(args.first(), 0);
                let end = rel(args.get(1), len);
                let out = if end > start {
                    items[start..end].to_vec()
                } else {
                    Vec::new()
                };
                Some(PreviewValue::Array(out))
            }
            (PreviewValue::Array(items), "concat") => {
                let mut out = items.clone();
                for arg in args {
                    if let PreviewValue::Array(extra) = arg {
                        let room = 10_000usize.saturating_sub(out.len());
                        out.extend(extra.into_iter().take(room));
                    } else if out.len() < 10_000 {
                        out.push(arg);
                    }
                }
                Some(PreviewValue::Array(out))
            }
            (PreviewValue::Array(items), "indexOf") => {
                let needle = args.first().unwrap_or(&PreviewValue::Undefined);
                let len = items.len() as i64;
                let from_raw = args.get(1).map_or(0.0, |v| v.to_number(is_as1));
                let mut start = if from_raw.is_nan() || from_raw == 0.0 {
                    0
                } else if from_raw.is_infinite() {
                    if from_raw.is_sign_positive() { len } else { 0 }
                } else {
                    let i = from_raw.trunc() as i64;
                    if i < 0 { (len + i).max(0) } else { i }
                };
                if start < 0 {
                    start = 0;
                }
                let found = (start as usize..items.len())
                    .find(|&idx| items[idx].strict_eq(needle))
                    .map_or(-1.0, |idx| idx as f64);
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::Array(items), "lastIndexOf") => {
                let needle = args.first().unwrap_or(&PreviewValue::Undefined);
                if items.is_empty() {
                    return Some(PreviewValue::Number(-1.0));
                }
                let len = items.len() as i64;
                let from_raw = args
                    .get(1)
                    .map_or((len - 1) as f64, |v| v.to_number(is_as1));
                let end_idx = if from_raw.is_nan() {
                    0i64
                } else if from_raw.is_infinite() {
                    if from_raw.is_sign_positive() {
                        len - 1
                    } else {
                        -1
                    }
                } else {
                    let i = from_raw.trunc() as i64;
                    if i < 0 { len + i } else { i.min(len - 1) }
                };
                if end_idx < 0 {
                    return Some(PreviewValue::Number(-1.0));
                }
                let found = (0..=end_idx as usize)
                    .rev()
                    .find(|&idx| items[idx].strict_eq(needle))
                    .map_or(-1.0, |idx| idx as f64);
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::String(st), "split") => {
                let limit = args
                    .get(1)
                    .and_then(|v| {
                        if matches!(v, PreviewValue::Undefined) {
                            None
                        } else {
                            let n = v.to_number(is_as1);
                            (n.is_finite() && n >= 0.0).then_some((n.trunc() as usize).min(10_000))
                        }
                    })
                    .unwrap_or(10_000);
                if limit == 0 {
                    return Some(PreviewValue::Array(Vec::new()));
                }
                let out: Vec<PreviewValue> = match args.first() {
                    None | Some(PreviewValue::Undefined) => vec![PreviewValue::String(st.clone())],
                    Some(sep_val) => {
                        let sep = sep_val.to_es_string_for(lang);
                        if sep.is_empty() {
                            st.chars()
                                .take(limit)
                                .map(|c| PreviewValue::String(c.to_string()))
                                .collect()
                        } else {
                            st.split(&sep)
                                .take(limit)
                                .map(|part| PreviewValue::String(part.to_owned()))
                                .collect()
                        }
                    }
                };
                Some(PreviewValue::Array(out))
            }
            (PreviewValue::String(st), "charAt") => {
                let n = args.first().map_or(0.0, |v| v.to_number(is_as1));
                let idx = if n.is_nan() || n == 0.0 {
                    0isize
                } else if !n.is_finite() {
                    -1
                } else {
                    n.trunc() as isize
                };
                let ch = if idx < 0 {
                    String::new()
                } else {
                    st.chars()
                        .nth(idx as usize)
                        .map_or_else(String::new, |c| c.to_string())
                };
                Some(PreviewValue::String(ch))
            }
            (PreviewValue::String(st), "charCodeAt") => {
                let n = args.first().map_or(0.0, |v| v.to_number(is_as1));
                let idx = if n.is_nan() || n == 0.0 {
                    0isize
                } else if !n.is_finite() {
                    -1
                } else {
                    n.trunc() as isize
                };
                let code = if idx < 0 {
                    f64::NAN
                } else {
                    st.encode_utf16()
                        .nth(idx as usize)
                        .map_or(f64::NAN, f64::from)
                };
                Some(PreviewValue::Number(code))
            }
            (PreviewValue::String(st), "indexOf") => {
                let needle = args
                    .first()
                    .map_or_else(|| "undefined".to_owned(), |v| v.to_es_string_for(lang));
                let chars: Vec<char> = st.chars().collect();
                let from_raw = args.get(1).map_or(0.0, |v| v.to_number(is_as1));
                let start_char = if !from_raw.is_finite() {
                    if from_raw.is_sign_positive() {
                        chars.len()
                    } else {
                        0
                    }
                } else {
                    (from_raw.trunc() as isize).clamp(0, chars.len() as isize) as usize
                };
                let byte_start = st
                    .char_indices()
                    .nth(start_char)
                    .map_or(st.len(), |(b, _)| b);
                let found = st[byte_start..].find(&needle).map_or(-1.0, |rel_b| {
                    st[..byte_start + rel_b].chars().count() as f64
                });
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::String(st), "lastIndexOf") => {
                let needle = args
                    .first()
                    .map_or_else(|| "undefined".to_owned(), |v| v.to_es_string_for(lang));
                let found = st
                    .rfind(&needle)
                    .map_or(-1.0, |b| st[..b].chars().count() as f64);
                Some(PreviewValue::Number(found))
            }
            (PreviewValue::String(st), "slice" | "substring" | "substr") => {
                let chars: Vec<char> = st.chars().collect();
                let len = chars.len() as i64;
                let (s_idx, e_idx) = if method == "substr" {
                    let s_raw = args.first().map_or(0.0, |v| v.to_number(is_as1));
                    let start = if s_raw.is_nan() || s_raw == 0.0 {
                        0
                    } else if !s_raw.is_finite() {
                        if s_raw.is_sign_positive() { len } else { 0 }
                    } else {
                        let i = s_raw.trunc() as i64;
                        if i < 0 { (len + i).max(0) } else { i.min(len) }
                    };
                    let count = match args.get(1) {
                        None | Some(PreviewValue::Undefined) => len - start,
                        Some(v) => {
                            let c = v.to_number(is_as1);
                            if c.is_nan() || c <= 0.0 {
                                0
                            } else if c.is_infinite() {
                                len - start
                            } else {
                                (c.trunc() as i64).min(len - start)
                            }
                        }
                    };
                    (start as usize, (start + count) as usize)
                } else if method == "substring" {
                    let norm = |raw: Option<&PreviewValue>, def: i64| -> i64 {
                        let Some(v) = raw else { return def };
                        if matches!(v, PreviewValue::Undefined) {
                            return def;
                        }
                        let n = v.to_number(is_as1);
                        if n.is_nan() || n <= 0.0 {
                            0
                        } else if n.is_infinite() {
                            len
                        } else {
                            (n.trunc() as i64).clamp(0, len)
                        }
                    };
                    let a = norm(args.first(), 0);
                    let b = norm(args.get(1), len);
                    (a.min(b) as usize, a.max(b) as usize)
                } else {
                    let norm = |raw: Option<&PreviewValue>, def: i64| -> i64 {
                        let Some(v) = raw else { return def };
                        if matches!(v, PreviewValue::Undefined) {
                            return def;
                        }
                        let n = v.to_number(is_as1);
                        if n.is_nan() || n == 0.0 {
                            0
                        } else if n.is_infinite() {
                            if n.is_sign_positive() { len } else { 0 }
                        } else {
                            let i = n.trunc() as i64;
                            if i < 0 { (len + i).max(0) } else { i.min(len) }
                        }
                    };
                    let a = norm(args.first(), 0);
                    let b = norm(args.get(1), len);
                    (a as usize, b.max(a) as usize)
                };
                Some(PreviewValue::String(
                    chars[s_idx..e_idx].iter().collect::<String>(),
                ))
            }
            (PreviewValue::String(st), "toUpperCase") if args.is_empty() => {
                Some(PreviewValue::String(st.to_uppercase()))
            }
            (PreviewValue::String(st), "toLowerCase") if args.is_empty() => {
                Some(PreviewValue::String(st.to_lowercase()))
            }
            (PreviewValue::String(st), "trim") if args.is_empty() => {
                Some(PreviewValue::String(st.trim().to_owned()))
            }
            (val, "toString")
                if args.is_empty()
                    && !matches!(val, PreviewValue::Undefined | PreviewValue::Null) =>
            {
                Some(PreviewValue::String(val.to_es_string_for(lang)))
            }
            (val, "valueOf")
                if args.is_empty()
                    && !matches!(val, PreviewValue::Undefined | PreviewValue::Null) =>
            {
                Some(val.clone())
            }
            _ => None,
        }
    }

    fn concrete_access_steps(
        &self,
        project: &Project,
        scope_path: &[Id],
        tokens: &[PostfixToken<'_>],
        is_as1: bool,
    ) -> Option<Vec<ConcreteAccessStep>> {
        let mut out = Vec::with_capacity(tokens.len());
        for tok in tokens {
            let step = match tok {
                PostfixToken::Call(_, _) => return None,
                PostfixToken::Prop(p) => ConcreteAccessStep::Prop((*p).to_owned()),
                PostfixToken::Index(idx_expr) => {
                    let kv = self.eval_preview_value(project, scope_path, idx_expr);
                    match kv {
                        PreviewValue::Number(n)
                            if n.is_finite()
                                && n >= 0.0
                                && (n - n.trunc()).abs() < 1e-9
                                && (n as usize) <= 10_000 =>
                        {
                            ConcreteAccessStep::Index(n as usize)
                        }
                        PreviewValue::String(ref st) => {
                            if let Ok(idx) = st.trim().parse::<usize>()
                                && idx <= 10_000
                            {
                                ConcreteAccessStep::Index(idx)
                            } else {
                                ConcreteAccessStep::Prop(st.clone())
                            }
                        }
                        other => ConcreteAccessStep::Prop(other.to_es_string(is_as1)),
                    }
                }
            };
            out.push(step);
        }
        Some(out)
    }

    fn is_supported_rhs(&self, rhs: &str) -> bool {
        is_supported_preview_rhs_for(rhs, self.current_language)
    }

    fn find_var_key_in_scope(
        &self,
        target_path: &[Id],
        name: &str,
        is_as1: bool,
    ) -> Option<String> {
        if self
            .vars
            .contains_key(&(target_path.to_vec(), name.to_owned()))
        {
            return Some(name.to_owned());
        }
        if is_as1 {
            return self
                .vars
                .keys()
                .find(|(p, k)| p.as_slice() == target_path && k.eq_ignore_ascii_case(name))
                .map(|(_, k)| k.clone());
        }
        None
    }

    fn find_named_child_id_matching(
        &self,
        project: &Project,
        parent_path: &[Id],
        instance_name: &str,
        is_as1: bool,
    ) -> Option<Id> {
        if let Some(exact) = self.find_named_child_id(project, parent_path, instance_name) {
            return Some(exact);
        }
        if !is_as1 {
            return None;
        }
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
                    && (el.name.eq_ignore_ascii_case(instance_name)
                        || el.id.eq_ignore_ascii_case(instance_name))
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

    fn resolve_eval_path_value(
        &self,
        project: &Project,
        scope_path: &[Id],
        raw_path: &str,
        is_as1: bool,
    ) -> PreviewValue {
        let Ok(parts) = crate::as2_object::reference_path(raw_path) else {
            return PreviewValue::Undefined;
        };
        if parts.is_empty() {
            return PreviewValue::Undefined;
        }
        let match_kw = |s: &str, kw: &str| {
            if is_as1 {
                s.eq_ignore_ascii_case(kw)
            } else {
                s == kw
            }
        };
        if parts.len() == 1
            && parts[0]
                .strip_prefix("_level")
                .or_else(|| {
                    is_as1
                        .then(|| {
                            let low = parts[0].to_ascii_lowercase();
                            low.starts_with("_level").then_some(&parts[0][6..])
                        })
                        .flatten()
                })
                .is_some_and(|rest| rest != "0" && rest.chars().all(|c| c.is_ascii_digit()))
        {
            return PreviewValue::Undefined;
        }
        let mut cur_mc: Option<Vec<Id>> = Some(scope_path.to_vec());
        let mut cur_val: Option<PreviewValue> = None;

        for (idx, seg) in parts.iter().enumerate() {
            let is_last = idx + 1 == parts.len();
            if let Some(val) = cur_val.take() {
                let next_val = match val {
                    PreviewValue::Array(items) => {
                        if match_kw(seg, "length") {
                            PreviewValue::Number(items.len() as f64)
                        } else if let Ok(i) = seg.parse::<usize>() {
                            items.get(i).cloned().unwrap_or(PreviewValue::Undefined)
                        } else {
                            return PreviewValue::Undefined;
                        }
                    }
                    PreviewValue::Object(entries) => entries
                        .into_iter()
                        .find(|(k, _)| k == seg || (is_as1 && k.eq_ignore_ascii_case(seg)))
                        .map(|(_, v)| v)
                        .unwrap_or(PreviewValue::Undefined),
                    _ => return PreviewValue::Undefined,
                };
                if is_last {
                    return next_val;
                }
                cur_val = Some(next_val);
                continue;
            }

            let Some(mut mc_path) = cur_mc.take() else {
                return PreviewValue::Undefined;
            };
            if idx == 0 && (match_kw(seg, "_root") || match_kw(seg, "_level0")) {
                mc_path.clear();
                if is_last {
                    return PreviewValue::String("root".to_owned());
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if match_kw(seg, "this") {
                if is_last {
                    return self.eval_mc_builtin_prop(project, &mc_path, "_name");
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if match_kw(seg, "_parent") {
                if mc_path.is_empty() {
                    return PreviewValue::Undefined;
                }
                mc_path.pop();
                if is_last {
                    return self.eval_mc_builtin_prop(project, &mc_path, "_name");
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if let Some(var_key) = self.find_var_key_in_scope(&mc_path, seg, is_as1)
                && let Some(v) = self.vars.get(&(mc_path.clone(), var_key)).cloned()
            {
                if is_last {
                    return v;
                }
                cur_val = Some(v);
                continue;
            }
            if let Some(child_id) =
                self.find_named_child_id_matching(project, &mc_path, seg, is_as1)
            {
                mc_path.push(child_id);
                if is_last {
                    return self.eval_mc_builtin_prop(project, &mc_path, "_name");
                }
                cur_mc = Some(mc_path);
                continue;
            }
            if is_last {
                let canon = if is_as1 {
                    seg.to_ascii_lowercase()
                } else {
                    seg.clone()
                };
                return self.eval_mc_builtin_prop(project, &mc_path, &canon);
            }
            return PreviewValue::Undefined;
        }
        PreviewValue::Undefined
    }

    fn eval_mc_builtin_prop(
        &self,
        project: &Project,
        target_path: &[Id],
        prop: &str,
    ) -> PreviewValue {
        match prop {
            "_currentframe" | "currentFrame" => {
                let f = self
                    .player
                    .clip_frame(target_path)
                    .unwrap_or_else(|| self.player.frame());
                PreviewValue::Number(f64::from(f + 1))
            }
            "_totalframes" | "totalFrames" => {
                let sym_id = if target_path.is_empty() {
                    Some(self.symbol.as_str())
                } else {
                    self.player
                        .previous
                        .get(target_path)
                        .and_then(|n| n.symbol_id.as_deref())
                };
                let count = sym_id
                    .and_then(|id| project.symbol(id))
                    .map_or(1, |sym| sym.timeline.frame_count.max(1));
                PreviewValue::Number(f64::from(count))
            }
            "_name" | "name" => PreviewValue::String(
                target_path
                    .last()
                    .map(|id| {
                        let parent = &target_path[..target_path.len() - 1];
                        self.element_display_name(project, parent, id)
                    })
                    .unwrap_or_else(|| "root".into()),
            ),
            "_visible" | "visible" => {
                let vis = self
                    .player
                    .overrides
                    .get(target_path)
                    .and_then(|o| o.visible)
                    .unwrap_or(true);
                PreviewValue::Bool(vis)
            }
            "x" | "_x" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "x")
                    .unwrap_or(0.0),
            ),
            "y" | "_y" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "y")
                    .unwrap_or(0.0),
            ),
            "rotation" | "_rotation" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "rotation")
                    .unwrap_or(0.0),
            ),
            "scaleX" | "scale_x" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_x")
                    .unwrap_or(1.0),
            ),
            "_xscale" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_x")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "scaleY" | "scale_y" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_y")
                    .unwrap_or(1.0),
            ),
            "_yscale" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "scale_y")
                    .map_or(100.0, |v| v * 100.0),
            ),
            "alpha" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "alpha")
                    .unwrap_or(1.0),
            ),
            "_alpha" => PreviewValue::Number(
                self.player
                    .property(project, target_path, "alpha")
                    .map_or(100.0, |a| a * 100.0),
            ),
            _ => PreviewValue::Undefined,
        }
    }

    fn apply_eval_path_write(
        &mut self,
        project: &Project,
        scope_path: &[Id],
        raw_path: &str,
        rhs_val: PreviewValue,
    ) -> bool {
        let Ok(parts) = crate::as2_object::reference_path(raw_path) else {
            return false;
        };
        if parts.is_empty() {
            return false;
        }
        let is_as1 = self.is_as1();
        let lang = self.current_language;
        let match_kw = |s: &str, kw: &str| {
            if is_as1 {
                s.eq_ignore_ascii_case(kw)
            } else {
                s == kw
            }
        };
        let mut mc_path = scope_path.to_vec();
        for (idx, seg) in parts[..parts.len() - 1].iter().enumerate() {
            if idx == 0 && (match_kw(seg, "_root") || match_kw(seg, "_level0")) {
                mc_path.clear();
                continue;
            }
            if match_kw(seg, "this") {
                continue;
            }
            if match_kw(seg, "_parent") {
                if mc_path.is_empty() {
                    return false;
                }
                mc_path.pop();
                continue;
            }
            if let Some(var_key) = self.find_var_key_in_scope(&mc_path, seg, is_as1)
                && matches!(
                    self.vars.get(&(mc_path.clone(), var_key.clone())),
                    Some(PreviewValue::Array(_) | PreviewValue::Object(_))
                )
            {
                let steps: Vec<ConcreteAccessStep> = parts[idx + 1..]
                    .iter()
                    .map(|p| {
                        if let Ok(i) = p.parse::<usize>()
                            && i <= 10_000
                        {
                            ConcreteAccessStep::Index(i)
                        } else {
                            ConcreteAccessStep::Prop(p.clone())
                        }
                    })
                    .collect();
                let Some(slot) = self.vars.get_mut(&(mc_path, var_key)) else {
                    return false;
                };
                return mutate_preview_container(slot, &steps, "=", &rhs_val, lang);
            }
            let Some(child_id) = self.find_named_child_id_matching(project, &mc_path, seg, is_as1)
            else {
                return false;
            };
            mc_path.push(child_id);
        }
        let last = &parts[parts.len() - 1];
        let canon = if is_as1 {
            last.to_ascii_lowercase()
        } else {
            last.clone()
        };
        if matches!(canon.as_str(), "_visible" | "visible") {
            self.player.set_visible(&mc_path, rhs_val.is_truthy(is_as1));
            return true;
        }
        let built_in = match canon.as_str() {
            "x" | "_x" => Some(("x", 1.0)),
            "y" | "_y" => Some(("y", 1.0)),
            "rotation" | "_rotation" => Some(("rotation", 1.0)),
            "scaleX" | "scale_x" => Some(("scale_x", 1.0)),
            "_xscale" => Some(("scale_x", 0.01)),
            "scaleY" | "scale_y" => Some(("scale_y", 1.0)),
            "_yscale" => Some(("scale_y", 0.01)),
            "alpha" => Some(("alpha", 1.0)),
            "_alpha" => Some(("alpha", 0.01)),
            _ => None,
        };
        if let Some((prop_name, scale_factor)) = built_in {
            let mut next = rhs_val.to_compound_lhs_number(is_as1) * scale_factor;
            if prop_name == "alpha" {
                next = next.clamp(0.0, 1.0);
            }
            self.player.set_property(&mc_path, prop_name, next);
            return true;
        }
        let key = self
            .find_var_key_in_scope(&mc_path, last, is_as1)
            .unwrap_or_else(|| last.clone());
        self.vars.insert((mc_path, key), rhs_val);
        true
    }

    fn try_apply_delete_target(
        &mut self,
        project: &Project,
        path: &[Id],
        target_expr: &str,
    ) -> Option<bool> {
        let scope_path = self.effective_scope_path(project, path);
        let is_as1 = self.is_as1();
        let is_avm1 = matches!(
            self.current_language,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
        if let Some((recv, prop)) = split_receiver_member(target_expr)
            && let Some(ev) = callback_prop_to_event(prop)
            && let Some(target_path) = self.resolve_receiver_path(project, &scope_path, recv)
        {
            self.clear_registered_handler(&target_path, ev);
            return Some(true);
        }
        if let Some((base, tokens)) = parse_postfix_steps(target_expr.trim())
            && !tokens.is_empty()
            && let Some((owner_path, root_var, start_idx)) =
                self.resolve_mutable_container_root(project, &scope_path, base, &tokens, false)
            && let Some(steps) =
                self.concrete_access_steps(project, &scope_path, &tokens[start_idx..], is_as1)
            && !steps.is_empty()
            && let Some(root_slot) = self.vars.get_mut(&(owner_path, root_var))
        {
            return delete_preview_container(root_slot, &steps, is_avm1);
        }
        let (recv_opt, prop_raw) = match split_receiver_member(target_expr) {
            Some((r, p)) => (Some(r), p.trim()),
            None => (None, target_expr.trim()),
        };
        if !is_valid_identifier(prop_raw) {
            return None;
        }
        let target_path =
            self.resolve_receiver_path(project, &scope_path, recv_opt.unwrap_or("this"))?;
        if let Some(key) = self.find_var_key_in_scope(&target_path, prop_raw, is_as1) {
            self.vars.remove(&(target_path, key));
            return Some(true);
        }
        Some(!is_avm1)
    }

    fn try_apply_nested_container_mutation(
        &mut self,
        project: &Project,
        scope_path: &[Id],
        lhs: &str,
        op: &str,
        rhs: &str,
        lang: ScriptLanguage,
    ) -> bool {
        if !is_supported_preview_rhs_for(rhs, lang) {
            return false;
        }
        let is_as1 = lang == ScriptLanguage::ActionScript1;
        let Some((base, tokens)) = parse_postfix_steps(lhs.trim()) else {
            return false;
        };
        if tokens.is_empty() {
            return false;
        }
        let Some((owner_path, root_var, start_idx)) =
            self.resolve_mutable_container_root(project, scope_path, base, &tokens, false)
        else {
            return false;
        };
        let Some(steps) =
            self.concrete_access_steps(project, scope_path, &tokens[start_idx..], is_as1)
        else {
            return false;
        };
        if steps.is_empty() {
            return false;
        }
        let rhs_val = self.eval_preview_value(project, scope_path, rhs);
        let Some(root_slot) = self.vars.get_mut(&(owner_path, root_var)) else {
            return false;
        };
        mutate_preview_container(root_slot, &steps, op, &rhs_val, lang)
    }

    fn try_apply_container_method_call(
        &mut self,
        project: &Project,
        path: &[Id],
        stmt: &str,
    ) -> bool {
        let Some((target_expr, method, args_str)) = parse_postfix_method_call(stmt.trim()) else {
            return false;
        };
        if !matches!(method, "push" | "pop" | "shift" | "unshift" | "reverse") {
            return false;
        }
        let scope_path = self.effective_scope_path(project, path);
        let (base, tokens) = match parse_postfix_steps(target_expr) {
            Some(parsed) => parsed,
            None if is_valid_identifier(target_expr) => (target_expr, Vec::new()),
            None => return false,
        };
        let Some((owner_path, root_var, start_idx)) =
            self.resolve_mutable_container_root(project, &scope_path, base, &tokens, true)
        else {
            return false;
        };
        let is_as1 = self.is_as1();
        let Some(steps) =
            self.concrete_access_steps(project, &scope_path, &tokens[start_idx..], is_as1)
        else {
            return false;
        };
        let arg_vals: Vec<PreviewValue> = split_top_level_comma(args_str)
            .into_iter()
            .map(|a| self.eval_preview_value(project, &scope_path, a))
            .collect();
        if matches!(method, "pop" | "shift" | "reverse") && !arg_vals.is_empty() {
            return false;
        }
        let Some(root_slot) = self.vars.get_mut(&(owner_path, root_var)) else {
            return false;
        };
        let Some(PreviewValue::Array(items)) = navigate_preview_container_mut(root_slot, &steps)
        else {
            return false;
        };
        match method {
            "push" => {
                for v in arg_vals {
                    if items.len() < 10_000 {
                        items.push(v);
                    }
                }
                true
            }
            "pop" => {
                items.pop();
                true
            }
            "shift" => {
                if !items.is_empty() {
                    items.remove(0);
                }
                true
            }
            "unshift" => {
                for (i, v) in arg_vals.into_iter().enumerate() {
                    if items.len() < 10_000 {
                        items.insert(i, v);
                    }
                }
                true
            }
            "reverse" => {
                items.reverse();
                true
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
enum PostfixToken<'a> {
    Index(&'a str),
    Prop(&'a str),
    Call(&'a str, &'a str),
}

#[derive(Clone, Debug)]
enum ConcreteAccessStep {
    Index(usize),
    Prop(String),
}

fn navigate_preview_container_mut<'a>(
    mut cur: &'a mut PreviewValue,
    steps: &[ConcreteAccessStep],
) -> Option<&'a mut PreviewValue> {
    for step in steps {
        cur = match (cur, step) {
            (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => items.get_mut(*idx)?,
            (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
                &mut entries.iter_mut().find(|(k, _)| k == key)?.1
            }
            _ => return None,
        };
    }
    Some(cur)
}

fn delete_preview_container(
    cur: &mut PreviewValue,
    steps: &[ConcreteAccessStep],
    is_avm1: bool,
) -> Option<bool> {
    let (first, rest) = steps.split_first()?;
    if rest.is_empty() {
        return match (cur, first) {
            (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
                if *idx >= items.len() {
                    Some(!is_avm1)
                } else if is_avm1 && matches!(items[*idx], PreviewValue::Undefined) {
                    Some(false)
                } else {
                    items[*idx] = PreviewValue::Undefined;
                    Some(true)
                }
            }
            (PreviewValue::Array(_), ConcreteAccessStep::Prop(key)) if key == "length" => {
                Some(false)
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
                if let Some(pos) = entries.iter().position(|(k, _)| k == key) {
                    entries.remove(pos);
                    Some(true)
                } else {
                    Some(!is_avm1)
                }
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
                let key = idx.to_string();
                if let Some(pos) = entries.iter().position(|(k, _)| k == &key) {
                    entries.remove(pos);
                    Some(true)
                } else {
                    Some(!is_avm1)
                }
            }
            _ => None,
        };
    }
    match (cur, first) {
        (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
            delete_preview_container(items.get_mut(*idx)?, rest, is_avm1)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
            let child = &mut entries.iter_mut().find(|(k, _)| k == key)?.1;
            delete_preview_container(child, rest, is_avm1)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
            let key = idx.to_string();
            let child = &mut entries.iter_mut().find(|(k, _)| k == &key)?.1;
            delete_preview_container(child, rest, is_avm1)
        }
        _ => None,
    }
}

fn mutate_preview_container(
    cur: &mut PreviewValue,
    steps: &[ConcreteAccessStep],
    op: &str,
    rhs_val: &PreviewValue,
    lang: ScriptLanguage,
) -> bool {
    let Some((first, rest)) = steps.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return match (cur, first) {
            (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
                if *idx > 10_000 {
                    return false;
                }
                if *idx >= items.len() {
                    items.resize(*idx + 1, PreviewValue::Undefined);
                }
                let Some(next) = apply_compound_var_op(op, &items[*idx], rhs_val, lang) else {
                    return false;
                };
                items[*idx] = next;
                true
            }
            (PreviewValue::Array(items), ConcreteAccessStep::Prop(key))
                if key == "length" && op == "=" =>
            {
                if let PreviewValue::Number(n) = rhs_val
                    && n.is_finite()
                    && *n >= 0.0
                    && (*n - n.trunc()).abs() < 1e-9
                    && (*n as usize) <= 10_000
                {
                    items.resize(*n as usize, PreviewValue::Undefined);
                    true
                } else {
                    false
                }
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
                if let Some((_, slot)) = entries.iter_mut().find(|(k, _)| k == key) {
                    let Some(next) = apply_compound_var_op(op, slot, rhs_val, lang) else {
                        return false;
                    };
                    *slot = next;
                } else {
                    let Some(next) =
                        apply_compound_var_op(op, &PreviewValue::Undefined, rhs_val, lang)
                    else {
                        return false;
                    };
                    entries.push((key.clone(), next));
                }
                true
            }
            (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
                let key = idx.to_string();
                if let Some((_, slot)) = entries.iter_mut().find(|(k, _)| k == &key) {
                    let Some(next) = apply_compound_var_op(op, slot, rhs_val, lang) else {
                        return false;
                    };
                    *slot = next;
                } else {
                    let Some(next) =
                        apply_compound_var_op(op, &PreviewValue::Undefined, rhs_val, lang)
                    else {
                        return false;
                    };
                    entries.push((key, next));
                }
                true
            }
            _ => false,
        };
    }

    match (cur, first) {
        (PreviewValue::Array(items), ConcreteAccessStep::Index(idx)) => {
            let Some(child) = items.get_mut(*idx) else {
                return false;
            };
            mutate_preview_container(child, rest, op, rhs_val, lang)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Prop(key)) => {
            let Some((_, child)) = entries.iter_mut().find(|(k, _)| k == key) else {
                return false;
            };
            mutate_preview_container(child, rest, op, rhs_val, lang)
        }
        (PreviewValue::Object(entries), ConcreteAccessStep::Index(idx)) => {
            let key = idx.to_string();
            let Some((_, child)) = entries.iter_mut().find(|(k, _)| k == &key) else {
                return false;
            };
            mutate_preview_container(child, rest, op, rhs_val, lang)
        }
        _ => false,
    }
}

fn split_top_level_colon(entry: &str) -> Option<(&str, &str)> {
    let parts = split_top_level_op(entry, ":");
    if parts.len() >= 2 {
        let colon_pos = entry.find(':')?;
        let _ = colon_pos;
        // Rejoin rhs in case value contains ternary `:` by splitting only at the first top-level `:`
        let bytes = entry.as_bytes();
        let mut i = 0;
        let mut in_s = false;
        let mut in_d = false;
        let mut paren = 0usize;
        let mut brace = 0usize;
        let mut bracket = 0usize;
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
                b':' if paren == 0 && brace == 0 && bracket == 0 => {
                    return Some((entry[..i].trim(), entry[i + 1..].trim()));
                }
                _ => {}
            }
            i += 1;
        }
    }
    None
}

fn parse_object_literal_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
    {
        return Some(unescape_string_literal(&trimmed[1..trimmed.len() - 1]));
    }
    if is_valid_identifier(trimmed) || trimmed.parse::<i64>().is_ok() {
        return Some(trimmed.to_owned());
    }
    None
}

fn parse_postfix_steps(expr: &str) -> Option<(&str, Vec<PostfixToken<'_>>)> {
    let s = expr.trim();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let base_end = match bytes[0] {
        b'(' => find_matching_delim(s, b'(', b')')? + 1,
        b'[' => find_matching_delim(s, b'[', b']')? + 1,
        b'{' => find_matching_delim(s, b'{', b'}')? + 1,
        b'\'' | b'"' => {
            let quote = bytes[0];
            let mut i = 1usize;
            let mut end = None;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    end = Some(i + 1);
                    break;
                }
                i += 1;
            }
            end?
        }
        b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
            let mut i = 1usize;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            if &s[..i] == "new" && bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                let mut k = i;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                let ctor_start = k;
                while k < bytes.len()
                    && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_' || bytes[k] == b'$')
                {
                    k += 1;
                }
                let ctor_name = &s[ctor_start..k];
                if !matches!(ctor_name, "Array" | "Object") {
                    return None;
                }
                let mut m = k;
                while m < bytes.len() && bytes[m].is_ascii_whitespace() {
                    m += 1;
                }
                if m < bytes.len() && bytes[m] == b'(' {
                    let close = find_matching_delim(&s[m..], b'(', b')')?;
                    m + close + 1
                } else {
                    k
                }
            } else {
                let mut m = i;
                while m < bytes.len() && bytes[m].is_ascii_whitespace() {
                    m += 1;
                }
                if m < bytes.len()
                    && bytes[m] == b'('
                    && (&s[..i] == "Array" || is_supported_builtin_pure_call_head(&s[..i]))
                {
                    let close = find_matching_delim(&s[m..], b'(', b')')?;
                    m + close + 1
                } else {
                    i
                }
            }
        }
        _ => return None,
    };

    let base = s[..base_end].trim();
    let mut rest = s[base_end..].trim_start();
    let mut tokens = Vec::new();
    while !rest.is_empty() {
        if rest.starts_with('[') {
            let close = find_matching_delim(rest, b'[', b']')?;
            tokens.push(PostfixToken::Index(rest[1..close].trim()));
            rest = rest[close + 1..].trim_start();
        } else {
            let after_dot = rest.strip_prefix('.')?.trim_start();
            let ab = after_dot.as_bytes();
            if ab.is_empty() || !(ab[0].is_ascii_alphabetic() || ab[0] == b'_' || ab[0] == b'$') {
                return None;
            }
            let mut i = 1usize;
            while i < ab.len() && (ab[i].is_ascii_alphanumeric() || ab[i] == b'_' || ab[i] == b'$')
            {
                i += 1;
            }
            let prop = &after_dot[..i];
            let tail = after_dot[i..].trim_start();
            if tail.starts_with('(') {
                let close = find_matching_delim(tail, b'(', b')')?;
                tokens.push(PostfixToken::Call(prop, tail[1..close].trim()));
                rest = tail[close + 1..].trim_start();
            } else {
                tokens.push(PostfixToken::Prop(prop));
                rest = tail;
            }
        }
    }
    Some((base, tokens))
}

fn parse_postfix_method_call(stmt: &str) -> Option<(&str, &str, &str)> {
    let s = stmt.trim();
    if !s.ends_with(')') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut in_s = false;
    let mut in_d = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut call_open = None;
    let mut i = 0usize;
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
            b'(' => {
                if paren == 0 && brace == 0 && bracket == 0 {
                    call_open = Some(i);
                }
                paren += 1;
            }
            b')' => paren = paren.saturating_sub(1),
            b'{' => brace += 1,
            b'}' => brace = brace.saturating_sub(1),
            b'[' => bracket += 1,
            b']' => bracket = bracket.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    let open = call_open?;
    if find_matching_delim(&s[open..], b'(', b')') != Some(s.len() - 1 - open) {
        return None;
    }
    let callee = s[..open].trim();
    let cb = callee.as_bytes();
    let mut last_dot = None;
    let mut in_s = false;
    let mut in_d = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut j = 0usize;
    while j < cb.len() {
        let b = cb[j];
        if in_s {
            if b == b'\\' {
                j += 2;
                continue;
            }
            if b == b'\'' {
                in_s = false;
            }
            j += 1;
            continue;
        }
        if in_d {
            if b == b'\\' {
                j += 2;
                continue;
            }
            if b == b'"' {
                in_d = false;
            }
            j += 1;
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
            b'.' if paren == 0 && brace == 0 && bracket == 0 => last_dot = Some(j),
            _ => {}
        }
        j += 1;
    }
    let dot = last_dot?;
    let target = callee[..dot].trim();
    let method = callee[dot + 1..].trim();
    if target.is_empty() || !is_valid_identifier(method) {
        return None;
    }
    Some((target, method, s[open + 1..s.len() - 1].trim()))
}

fn eval_add_values(lhs: PreviewValue, rhs: PreviewValue, lang: ScriptLanguage) -> PreviewValue {
    let is_as1 = lang == ScriptLanguage::ActionScript1;
    if matches!(lhs, PreviewValue::String(_) | PreviewValue::Array(_))
        || matches!(rhs, PreviewValue::String(_) | PreviewValue::Array(_))
    {
        let mut s = lhs.to_es_string_for(lang);
        s.push_str(&rhs.to_es_string_for(lang));
        PreviewValue::String(s)
    } else {
        PreviewValue::Number(lhs.to_number(is_as1) + rhs.to_number(is_as1))
    }
}

fn callback_prop_to_event(prop: &str) -> Option<RegisteredEvent> {
    match prop.trim() {
        "onRelease" | "onPress" | "onclick" => Some(RegisteredEvent::Click),
        "onEnterFrame" => Some(RegisteredEvent::EnterFrame),
        _ => None,
    }
}

fn is_valid_identifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && !bytes[0].is_ascii_digit()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'$')
}

fn is_supported_builtin_pure_call_head(head: &str) -> bool {
    matches!(
        head,
        "eval"
            | "chr"
            | "ord"
            | "mbchr"
            | "mbord"
            | "length"
            | "mblength"
            | "substring"
            | "mbsubstring"
            | "String"
            | "Number"
            | "Boolean"
            | "int"
            | "uint"
            | "parseInt"
            | "parseFloat"
            | "Math.abs"
            | "Math.floor"
            | "Math.ceil"
            | "Math.round"
            | "Math.sqrt"
            | "Math.pow"
            | "Math.min"
            | "Math.max"
    )
}

fn is_supported_builtin_pure_call_head_for(head: &str, lang: ScriptLanguage) -> bool {
    if head == "eval" {
        return lang != ScriptLanguage::ActionScript3;
    }
    if matches!(
        head,
        "chr" | "ord" | "mbchr" | "mbord" | "length" | "mblength" | "substring" | "mbsubstring"
    ) {
        return matches!(
            lang,
            ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
        );
    }
    is_supported_builtin_pure_call_head(head)
}

fn is_supported_postfix_value_method(method: &str) -> bool {
    matches!(
        method,
        "join"
            | "toString"
            | "valueOf"
            | "slice"
            | "concat"
            | "indexOf"
            | "lastIndexOf"
            | "split"
            | "charAt"
            | "charCodeAt"
            | "substring"
            | "substr"
            | "toUpperCase"
            | "toLowerCase"
            | "trim"
    )
}

fn is_supported_preview_rhs_for(rhs: &str, lang: ScriptLanguage) -> bool {
    let mut trimmed = rhs.trim();
    while trimmed.starts_with('(') && trimmed.ends_with(')') && is_single_paren_group(trimmed) {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }
    if trimmed.is_empty() || extract_function_literal_body(trimmed).is_some() {
        return false;
    }
    if let Some(rest) = trimmed.strip_prefix("new ") {
        let ctor = rest.trim_start();
        return ctor.starts_with("Array(")
            || ctor == "Array"
            || ctor.starts_with("Object(")
            || ctor == "Object";
    }
    if let Some((head, _)) = parse_qualified_call(trimmed) {
        if head == "Array" || is_supported_builtin_pure_call_head_for(head, lang) {
            return true;
        }
        if let Some((_, method)) = split_receiver_member(head) {
            return is_supported_postfix_value_method(method);
        }
        return false;
    }
    if let Some((base, tokens)) = parse_postfix_steps(trimmed) {
        if let Some((head, _)) = parse_qualified_call(base)
            && head != "Array"
            && !is_supported_builtin_pure_call_head_for(head, lang)
        {
            return false;
        }
        if tokens
            .iter()
            .any(|t| matches!(t, PostfixToken::Call(m, _) if !is_supported_postfix_value_method(m)))
        {
            return false;
        }
    }
    true
}

fn apply_compound_var_op(
    op: &str,
    cur_val: &PreviewValue,
    rhs_val: &PreviewValue,
    lang: ScriptLanguage,
) -> Option<PreviewValue> {
    let is_as1 = lang == ScriptLanguage::ActionScript1;
    match op {
        "=" => Some(rhs_val.clone()),
        "+=" => {
            if matches!(cur_val, PreviewValue::String(_) | PreviewValue::Array(_))
                || matches!(rhs_val, PreviewValue::String(_) | PreviewValue::Array(_))
            {
                let mut s = cur_val.to_es_string_for(lang);
                s.push_str(&rhs_val.to_es_string_for(lang));
                Some(PreviewValue::String(s))
            } else {
                let a = cur_val.to_compound_lhs_number(is_as1);
                let b = rhs_val.to_number(is_as1);
                Some(PreviewValue::Number(a + b))
            }
        }
        "-=" => Some(PreviewValue::Number(
            cur_val.to_compound_lhs_number(is_as1) - rhs_val.to_number(is_as1),
        )),
        "*=" => Some(PreviewValue::Number(
            cur_val.to_compound_lhs_number(is_as1) * rhs_val.to_number(is_as1),
        )),
        "/=" => {
            let a = cur_val.to_compound_lhs_number(is_as1);
            let b = rhs_val.to_number(is_as1);
            if b.is_nan() {
                Some(PreviewValue::Number(f64::NAN))
            } else if b.abs() < 1e-12 {
                Some(PreviewValue::Number(0.0))
            } else {
                Some(PreviewValue::Number(a / b))
            }
        }
        "%=" => {
            let a = cur_val.to_compound_lhs_number(is_as1);
            let b = rhs_val.to_number(is_as1);
            if b.is_nan() {
                Some(PreviewValue::Number(f64::NAN))
            } else if b.abs() < 1e-12 {
                Some(PreviewValue::Number(0.0))
            } else {
                Some(PreviewValue::Number(a % b))
            }
        }
        "&=" => Some(PreviewValue::Number(f64::from(
            cur_val.to_i32(is_as1) & rhs_val.to_i32(is_as1),
        ))),
        "|=" => Some(PreviewValue::Number(f64::from(
            cur_val.to_i32(is_as1) | rhs_val.to_i32(is_as1),
        ))),
        "^=" => Some(PreviewValue::Number(f64::from(
            cur_val.to_i32(is_as1) ^ rhs_val.to_i32(is_as1),
        ))),
        "<<=" => Some(PreviewValue::Number(f64::from(
            cur_val
                .to_i32(is_as1)
                .wrapping_shl(rhs_val.to_u32(is_as1) & 0x1f),
        ))),
        ">>=" => Some(PreviewValue::Number(f64::from(
            cur_val
                .to_i32(is_as1)
                .wrapping_shr(rhs_val.to_u32(is_as1) & 0x1f),
        ))),
        ">>>=" => Some(PreviewValue::Number(f64::from(
            cur_val
                .to_u32(is_as1)
                .wrapping_shr(rhs_val.to_u32(is_as1) & 0x1f),
        ))),
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
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        }
    } else if (n - n.round()).abs() < 1e-9 && n.abs() < (i64::MAX as f64) {
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
                        && !after_trimmed.starts_with(',')
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
                if !matches!(
                    prev,
                    b'!' | b'='
                        | b'<'
                        | b'>'
                        | b'+'
                        | b'-'
                        | b'*'
                        | b'/'
                        | b'%'
                        | b'&'
                        | b'|'
                        | b'^'
                ) && next != b'='
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

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn strip_unary_keyword<'a>(expr: &'a str, kw: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(kw)?;
    let first = rest.as_bytes().first().copied()?;
    if is_ident_byte(first) {
        return None;
    }
    let trimmed = rest.trim_start();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn op_has_boundary_conflict(expr: &str, bytes: &[u8], i: usize, op: &str) -> bool {
    let op_bytes = op.as_bytes();
    let prev = if i > 0 { bytes[i - 1] } else { 0 };
    let next = bytes.get(i + op_bytes.len()).copied().unwrap_or(0);

    if op_bytes[0].is_ascii_alphabetic() {
        return prev == b'.' || is_ident_byte(prev) || is_ident_byte(next);
    }

    match op {
        "===" | "!==" | ">>>=" | "<<=" => false,
        ">=" => prev == b'>',
        "<=" => prev == b'<',
        ">>=" => prev == b'>',
        "==" => prev == b'=' || prev == b'!' || next == b'=',
        "!=" => next == b'=',
        "<>" => false,
        ">>>" => next == b'=',
        "<<" => next == b'=',
        ">>" => prev == b'>' || next == b'>' || next == b'=',
        "<" => prev == b'<' || next == b'<' || next == b'=' || next == b'>',
        ">" => prev == b'>' || prev == b'<' || prev == b'=' || next == b'>' || next == b'=',
        "&&" => next == b'=',
        "||" => next == b'=',
        "&" => prev == b'&' || next == b'&' || next == b'=',
        "|" => prev == b'|' || next == b'|' || next == b'=',
        "^" => next == b'=',
        "*" | "/" | "%" => next == b'=',
        "+" => {
            if prev == b'+' || next == b'+' || next == b'=' {
                return true;
            }
            if matches!(prev, b'e' | b'E')
                && i >= 2
                && (bytes[i - 2].is_ascii_digit() || bytes[i - 2] == b'.')
            {
                return true;
            }
            let prev_non_ws = expr[..i].trim_end().as_bytes().last().copied();
            !prev_non_ws.is_some_and(|p| {
                p.is_ascii_alphanumeric() || matches!(p, b')' | b']' | b'"' | b'\'' | b'_' | b'$')
            })
        }
        "-" => {
            if prev == b'-' || next == b'-' || next == b'=' || next == b'>' {
                return true;
            }
            if matches!(prev, b'e' | b'E')
                && i >= 2
                && (bytes[i - 2].is_ascii_digit() || bytes[i - 2] == b'.')
            {
                return true;
            }
            let prev_non_ws = expr[..i].trim_end().as_bytes().last().copied();
            !prev_non_ws.is_some_and(|p| {
                p.is_ascii_alphanumeric() || matches!(p, b')' | b']' | b'"' | b'\'' | b'_' | b'$')
            })
        }
        _ => false,
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
                && &bytes[i..i + op_bytes.len()] == op_bytes
                && !op_has_boundary_conflict(expr, bytes, i, op) =>
            {
                parts.push(expr[start..i].trim());
                start = i + op_bytes.len();
                i = start;
                continue;
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

fn rfind_top_level_ops<'a, 'b>(
    expr: &'a str,
    ops: &[&'b str],
) -> Option<(&'a str, &'b str, &'a str)> {
    let bytes = expr.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut last_match: Option<(usize, &'b str)> = None;
    let mut i = 0usize;
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
            b'\'' => {
                in_single = true;
                i += 1;
                continue;
            }
            b'"' => {
                in_double = true;
                i += 1;
                continue;
            }
            b'(' => {
                paren += 1;
                i += 1;
                continue;
            }
            b')' => {
                paren = paren.saturating_sub(1);
                i += 1;
                continue;
            }
            b'{' => {
                brace += 1;
                i += 1;
                continue;
            }
            b'}' => {
                brace = brace.saturating_sub(1);
                i += 1;
                continue;
            }
            b'[' => {
                bracket += 1;
                i += 1;
                continue;
            }
            b']' => {
                bracket = bracket.saturating_sub(1);
                i += 1;
                continue;
            }
            _ => {}
        }
        if paren == 0 && brace == 0 && bracket == 0 {
            let mut matched = None;
            for &op in ops {
                let op_bytes = op.as_bytes();
                if i + op_bytes.len() <= bytes.len()
                    && &bytes[i..i + op_bytes.len()] == op_bytes
                    && !op_has_boundary_conflict(expr, bytes, i, op)
                {
                    matched = Some(op);
                    break;
                }
            }
            if let Some(op) = matched {
                let lhs = expr[..i].trim();
                let rhs = expr[i + op.len()..].trim();
                if !lhs.is_empty() && !rhs.is_empty() {
                    last_match = Some((i, op));
                }
                i += op.len();
                continue;
            }
        }
        i += 1;
    }
    let (pos, op) = last_match?;
    Some((expr[..pos].trim(), op, expr[pos + op.len()..].trim()))
}

fn split_top_level_ternary(expr: &str) -> Option<(&str, &str, &str)> {
    let bytes = expr.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut paren = 0usize;
    let mut brace = 0usize;
    let mut bracket = 0usize;
    let mut q_pos = None;
    let mut nested_q = 0usize;
    let mut i = 0usize;
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
            b'?' if paren == 0 && brace == 0 && bracket == 0 => {
                let next = bytes.get(i + 1).copied().unwrap_or(0);
                if next != b'?' && next != b'.' {
                    if q_pos.is_none() {
                        q_pos = Some(i);
                    } else {
                        nested_q += 1;
                    }
                }
            }
            b':' if paren == 0 && brace == 0 && bracket == 0 && q_pos.is_some() => {
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let next = bytes.get(i + 1).copied().unwrap_or(0);
                if prev != b':' && next != b':' {
                    if nested_q == 0 {
                        let q = q_pos?;
                        let cond = expr[..q].trim();
                        let yes = expr[q + 1..i].trim();
                        let no = expr[i + 1..].trim();
                        if !cond.is_empty() && !yes.is_empty() && !no.is_empty() {
                            return Some((cond, yes, no));
                        }
                        return None;
                    }
                    nested_q -= 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
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
                layer: None,
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
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::JavaScript,
                    source: "var count = 1; this.x = 12; stop(); trace(\"live\");".into(),
                },
            },
            crate::FrameScript {
                layer: None,
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
            layer: None,
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
            layer: None,
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

    #[test]
    fn on_clip_event_load_persists_into_enter_frame_and_compound_ops_mutate_stage_preview() {
        let mut project = Project::blank();
        let root = project.root.clone();
        let mut mc_sym = project.symbol(&root).unwrap().clone();
        mc_sym.id = "spinner-mc".into();
        mc_sym.name = "SpinnerClip".into();
        mc_sym.kind = SymbolKind::MovieClip;
        mc_sym.timeline.frame_count = 1;
        mc_sym.timeline.layers[0].id = "spinner-layer".into();
        project.symbols.push(mc_sym);

        let root_sym = project.symbol_mut(&root).unwrap();
        root_sym.timeline.frame_count = 4;
        root_sym.timeline.layers[0].keyframes[0].elements = vec![crate::Element {
            id: "spinner-inst".into(),
            name: "spinner".into(),
            kind: crate::ElementKind::Instance {
                symbol_id: "spinner-mc".into(),
                playback: crate::InstancePlayback::Independent,
            },
            scripts: vec![crate::ObjectScript {
                event: ObjectEvent::EnterFrame,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript2,
                    source: "onClipEvent (load) {\n    var vx = 20, bonus;\n    omega = 15;\n    _x = 100;\n    _y = 50;\n    _rotation = 10;\n    _xscale = 120;\n    _yscale = 80;\n    _alpha = 90;\n}\nonClipEvent (enterFrame) {\n    bonus += 3;\n    vx /= 2;\n    omega %= 10;\n    _x += vx + bonus;\n    ++_x;\n    _y -= 5;\n    --_y;\n    _rotation += omega;\n    _rotation++;\n    _xscale *= 1.5;\n    _yscale /= 2;\n    _alpha -= 15;\n}\non (release) {\n    vx += 4;\n    trace(\"spinner vx=\" + vx + \" x=\" + _x + \" rot=\" + _rotation);\n}"
                        .into(),
                },
            }],
            ..crate::Element::default()
        }];
        project.validate().unwrap();

        let spinner_path = vec!["spinner-inst".to_owned()];
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert!(preview.drain_diagnostics().is_empty());
        assert_eq!(
            preview.player.property(&project, &spinner_path, "x"),
            Some(100.0)
        );
        assert_eq!(
            preview.player.property(&project, &spinner_path, "rotation"),
            Some(10.0)
        );

        // Frame 1 tick:
        // bonus: undefined -> 0 + 3 = 3
        // vx: 20 /= 2 -> 10
        // omega: 15 %= 10 -> 5
        // _x: 100 + (10 + 3) = 113, ++_x -> 114
        // _y: 50 - 5 = 45, --_y -> 44
        // _rotation: 10 + 5 = 15, _rotation++ -> 16
        // _xscale: 120 *= 1.5 -> 180 (scale_x = 1.8)
        // _yscale: 80 /= 2 -> 40 (scale_y = 0.4)
        // _alpha: 90 -= 15 -> 75 (alpha = 0.75)
        preview.tick(&project);
        assert!(preview.drain_diagnostics().is_empty());
        assert_eq!(
            preview.player.property(&project, &spinner_path, "x"),
            Some(114.0)
        );
        assert_eq!(
            preview.player.property(&project, &spinner_path, "y"),
            Some(44.0)
        );
        assert_eq!(
            preview.player.property(&project, &spinner_path, "rotation"),
            Some(16.0)
        );
        assert!(
            (preview
                .player
                .property(&project, &spinner_path, "scale_x")
                .unwrap()
                - 1.8)
                .abs()
                < 1e-6
        );
        assert!(
            (preview
                .player
                .property(&project, &spinner_path, "scale_y")
                .unwrap()
                - 0.4)
                .abs()
                < 1e-6
        );
        assert!(
            (preview
                .player
                .property(&project, &spinner_path, "alpha")
                .unwrap()
                - 0.75)
                .abs()
                < 1e-6
        );

        // Clicking `spinner` mutates the same instance scope (`vx`: 10 += 4 -> 14)
        preview.click(&project, &spinner_path);
        assert_eq!(
            preview.drain_traces(),
            vec!["spinner vx=14 x=114 rot=16".to_owned()]
        );

        // Frame 2 tick uses updated `vx = 14` (`14 /= 2 -> 7`) and `bonus` (`3 += 3 -> 6`):
        // `_x`: 114 + (7 + 6) + 1 = 128
        preview.tick(&project);
        assert_eq!(
            preview.player.property(&project, &spinner_path, "x"),
            Some(128.0)
        );

        // Verify `evaluate_scene_with_symbol_overrides` evaluates single-symbol solo overrides
        // without cloning `Project`.
        let mut solo_sym = project.symbol(&root).unwrap().clone();
        solo_sym.timeline.layers[0].visible = false;
        let solo_overrides = std::collections::HashMap::from([(root.clone(), solo_sym)]);
        let hidden_scene = crate::evaluate_scene_with_symbol_overrides(
            &project,
            &solo_overrides,
            &root,
            0,
            true,
            true,
        );
        assert!(hidden_scene.draws.is_empty());
    }

    #[test]
    fn t266_typed_preview_value_strict_vs_loose_equality_string_plus_and_null_undefined() {
        let mut project = Project::blank();
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript3,
                source: r#"
                    trace("5" === 5, "5" == 5, "5" !== 5, "5" != 5);
                    trace(null == undefined, null === undefined, null == 0, undefined == 0);
                    trace("null" == null, "undefined" == undefined);
                    var sn = "null", su = "undefined", n_val = null, u_val;
                    trace(sn == n_val, su == u_val, n_val == u_val, n_val === u_val);
                    var a = "5";
                    var b = a + 1;
                    var s = "hello ";
                    var w = s + "world";
                    var n = 2;
                    var m = n + 3;
                    trace(b, b === "51", b === 51, w, m, m === 5);
                    a += 2;
                    trace(a, a === "52");
                    if ("5" === 5) {
                        trace("BUG_STRICT_EQ");
                    }
                    if ("5" == 5 && "5" !== 5 && null == undefined && !(null === undefined)) {
                        trace("COND_OK");
                    }
                "#
                .into(),
            },
        }];
        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert!(preview.drain_diagnostics().is_empty());
        assert_eq!(
            preview.drain_traces(),
            vec![
                "false true true false".to_owned(),
                "true false false false".to_owned(),
                "false false".to_owned(),
                "false false true false".to_owned(),
                "51 true false hello world 5 true".to_owned(),
                "52 true".to_owned(),
                "COND_OK".to_owned(),
            ]
        );
    }

    #[test]
    fn t266_null_and_undefined_arithmetic_and_relational_across_as1_as2_and_as3() {
        let run_for_lang = |language: crate::ScriptLanguage| -> Vec<String> {
            let mut project = Project::blank();
            let root = project.root.clone();
            project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language,
                    source: r#"
                        var u;
                        var nl = null;
                        trace(nl + 5, nl - 2, nl * 10, nl < 5, nl >= 0, nl > 0);
                        trace(u + 5, u - 2, u < 5, u > 5, u <= 0, u >= 0);
                        trace("ab" < "ac", "b" < "a");
                    "#
                    .into(),
                },
            }];
            let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
            assert!(preview.drain_diagnostics().is_empty());
            preview.drain_traces()
        };

        let as1_traces = run_for_lang(crate::ScriptLanguage::ActionScript1);
        assert_eq!(
            as1_traces,
            vec![
                "5 -2 0 true true false".to_owned(),
                "5 -2 true false true true".to_owned(),
                "true false".to_owned(),
            ]
        );

        for lang in [
            crate::ScriptLanguage::ActionScript2,
            crate::ScriptLanguage::ActionScript3,
        ] {
            let traces = run_for_lang(lang);
            assert_eq!(
                traces,
                vec![
                    "5 -2 0 true true false".to_owned(),
                    "NaN NaN false false false false".to_owned(),
                    "true false".to_owned(),
                ]
            );
        }
    }

    #[test]
    fn r102_skipped_foreign_scripts_do_not_execute_or_make_placements_interactive() {
        let mut project = Project::blank();
        project.format_version = project.format_version.max(6);
        project.script_language = Some(crate::ScriptLanguage::ActionScript3);
        project.also_compile_compatible_scripts = false;
        let root = project.root.clone();

        let clip_id: Id = "mc_foreign".into();
        let mut mc_sym = project.symbol(&root).unwrap().clone();
        mc_sym.id = clip_id.clone();
        mc_sym.name = "ForeignClip".into();
        mc_sym.kind = crate::SymbolKind::MovieClip;
        mc_sym.timeline.frame_count = 1;
        mc_sym.timeline.layers[0].id = "mc_layer".into();
        project.symbols.push(mc_sym);

        let foreign_elem_id: Id = "inst_foreign".into();
        let primary_elem_id: Id = "inst_primary".into();
        {
            let root_sym = project.symbol_mut(&root).unwrap();
            root_sym.timeline.frame_count = 2;
            root_sym.timeline.scripts = vec![
                crate::FrameScript {
                    layer: None,
                    frame: 0,
                    script: crate::Script {
                        language: crate::ScriptLanguage::ActionScript3,
                        source: r#"trace("AS3_FRAME_RAN");"#.into(),
                    },
                },
                crate::FrameScript {
                    layer: None,
                    frame: 0,
                    script: crate::Script {
                        language: crate::ScriptLanguage::ActionScript2,
                        source: r#"trace("SKIPPED_AS2_FRAME");"#.into(),
                    },
                },
                crate::FrameScript {
                    layer: None,
                    frame: 0,
                    script: crate::Script {
                        language: crate::ScriptLanguage::JavaScript,
                        source: r#"trace("SKIPPED_JS_FRAME");"#.into(),
                    },
                },
            ];
            root_sym.timeline.layers[0].keyframes[0].elements = vec![
                crate::Element {
                    id: foreign_elem_id.clone(),
                    name: "foreignMc".into(),
                    kind: crate::ElementKind::Instance {
                        symbol_id: clip_id.clone(),
                        playback: crate::InstancePlayback::Independent,
                    },
                    scripts: vec![
                        crate::ObjectScript {
                            event: ObjectEvent::Load,
                            script: crate::Script {
                                language: crate::ScriptLanguage::ActionScript2,
                                source: r#"trace("SKIPPED_AS2_LOAD");"#.into(),
                            },
                        },
                        crate::ObjectScript {
                            event: ObjectEvent::EnterFrame,
                            script: crate::Script {
                                language: crate::ScriptLanguage::ActionScript2,
                                source: r#"onClipEvent(load) { trace("SKIPPED_WRAPPER_LOAD"); } on(release) { trace("SKIPPED_WRAPPER_CLICK"); } trace("SKIPPED_AS2_ENTER");"#.into(),
                            },
                        },
                        crate::ObjectScript {
                            event: ObjectEvent::Click,
                            script: crate::Script {
                                language: crate::ScriptLanguage::JavaScript,
                                source: r#"trace("SKIPPED_JS_CLICK");"#.into(),
                            },
                        },
                    ],
                    ..crate::Element::default()
                },
                crate::Element {
                    id: primary_elem_id.clone(),
                    name: "primaryMc".into(),
                    kind: crate::ElementKind::Instance {
                        symbol_id: clip_id,
                        playback: crate::InstancePlayback::Independent,
                    },
                    scripts: vec![crate::ObjectScript {
                        event: ObjectEvent::Click,
                        script: crate::Script {
                            language: crate::ScriptLanguage::ActionScript3,
                            source: r#"trace("PRIMARY_AS3_CLICK");"#.into(),
                        },
                    }],
                    ..crate::Element::default()
                },
            ];
        }
        project.validate().unwrap();

        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert_eq!(preview.drain_traces(), vec!["AS3_FRAME_RAN".to_owned()]);
        assert!(
            !preview.is_interactive_target(&project, std::slice::from_ref(&foreign_elem_id)),
            "placement with only SkippedForeign scripts must not be clickable"
        );
        assert_eq!(
            preview.interactive_ancestor_path(&project, std::slice::from_ref(&primary_elem_id)),
            Some(vec![primary_elem_id.clone()]),
        );

        preview.tick(&project);
        preview.click(&project, std::slice::from_ref(&foreign_elem_id));
        assert!(
            preview.drain_traces().is_empty(),
            "SkippedForeign EnterFrame/Click handlers must never execute"
        );

        preview.click(&project, std::slice::from_ref(&primary_elem_id));
        assert_eq!(preview.drain_traces(), vec!["PRIMARY_AS3_CLICK".to_owned()]);

        // Enabling `also_compile_compatible_scripts` on a JS primary project compiles TS
        // (`CompiledCompatibleWithWarning`) while still skipping AS2 (`SkippedForeign`).
        project.script_language = Some(crate::ScriptLanguage::JavaScript);
        project.also_compile_compatible_scripts = true;
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![
            crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::TypeScript,
                    source: r#"trace("COMPAT_TS_FRAME");"#.into(),
                },
            },
            crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript2,
                    source: r#"trace("STILL_SKIPPED_AS2");"#.into(),
                },
            },
        ];
        let mut compat_preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        assert_eq!(
            compat_preview.drain_traces(),
            vec!["COMPAT_TS_FRAME".to_owned()]
        );
    }

    #[test]
    fn t320_multi_scene_auto_advance_scene_step_and_dialect_aware_goto() {
        let mut project = Project::blank();
        project.format_version = 6;
        project.script_language = Some(crate::ScriptLanguage::ActionScript3);
        let s1_id = project.root.clone();
        let s2_id: Id = "scene-2".into();

        {
            let s1 = project.symbol_mut(&s1_id).unwrap();
            s1.name = "Scene 1".into();
            s1.timeline.frame_count = 2;
            s1.timeline.layers[0].keyframes[0].label = "s1_start".into();
        }
        let mut s2 = project.symbol(&s1_id).unwrap().clone();
        s2.id = s2_id.clone();
        s2.name = "Scene 2".into();
        s2.timeline.frame_count = 2;
        s2.timeline.layers[0].id = "s2-layer".into();
        s2.timeline.layers[0].keyframes[0].label = "s2_entry".into();
        s2.timeline.layers[0].keyframes.push(crate::Keyframe {
            frame: 1,
            label: "s2_final".into(),
            ..crate::Keyframe::default()
        });
        s2.timeline.scripts = vec![
            crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript3,
                    source: r#"trace("S2_F1");"#.into(),
                },
            },
            crate::FrameScript {
                layer: None,
                frame: 1,
                script: crate::Script {
                    language: crate::ScriptLanguage::ActionScript3,
                    source: r#"trace("S2_F2");"#.into(),
                },
            },
        ];
        project.symbols.push(s2);
        project.scenes = vec![s1_id.clone(), s2_id.clone()];
        project.upgrade_format_for_features();
        project.validate().unwrap();

        // 0. Flattened `Player` (used by HTML5/WASM) auto-advances unstopped Scene 1 -> Scene 2.
        let (flat_project, scene_ranges) = crate::flatten_scenes_with_ranges(&project);
        let mut runtime_player = crate::Player::new(&flat_project);
        runtime_player
            .set_scenes(&flat_project, scene_ranges)
            .unwrap();
        assert_eq!(runtime_player.frame(), 0);
        runtime_player.tick(&flat_project);
        assert_eq!(runtime_player.frame(), 1);
        runtime_player.tick(&flat_project);
        assert_eq!(runtime_player.frame(), 2); // Scene 2 frame 0 in flattened timeline

        // 1. `AuthoredPlayback` with `set_play_all_scenes(true)` auto-advances Scene 1 (frames 0..1) -> Scene 2 frame 0 -> Scene 2 frame 1 -> wraps to Scene 1 frame 0.
        let mut pb = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
        assert!(!pb.is_play_all_scenes());
        pb.set_play_all_scenes(true);
        assert!(pb.is_play_all_scenes());
        assert_eq!(pb.symbol(), s1_id);
        assert_eq!(pb.frame(), 0);

        pb.tick(&project);
        assert_eq!(pb.symbol(), s1_id);
        assert_eq!(pb.frame(), 1);
        pb.vars.insert(
            (Vec::new(), "root_state".into()),
            PreviewValue::String("keep".into()),
        );
        pb.vars.insert(
            (vec!["departed-child".into()], "child_state".into()),
            PreviewValue::String("discard".into()),
        );

        pb.tick(&project);
        assert_eq!(pb.symbol(), s2_id);
        assert_eq!(pb.frame(), 0);
        assert!(pb.vars.contains_key(&(Vec::new(), "root_state".into())));
        assert!(
            !pb.vars
                .contains_key(&(vec!["departed-child".into()], "child_state".into()))
        );
        assert_eq!(pb.drain_traces(), vec!["S2_F1".to_owned()]);

        pb.tick(&project);
        assert_eq!(pb.symbol(), s2_id);
        assert_eq!(pb.frame(), 1);
        assert_eq!(pb.drain_traces(), vec!["S2_F2".to_owned()]);

        pb.tick(&project);
        assert_eq!(pb.symbol(), s1_id);
        assert_eq!(pb.frame(), 0);

        // 2. Single-scene mode (`set_play_all_scenes(false)`) loops inside Scene 1 without crossing.
        pb.set_play_all_scenes(false);
        pb.tick(&project);
        assert_eq!((pb.symbol(), pb.frame()), (s1_id.as_str(), 1));
        pb.tick(&project);
        assert_eq!((pb.symbol(), pb.frame()), (s1_id.as_str(), 0));
        pb.set_play_all_scenes(true);

        // 3. AS3 `nextScene()` / `prevScene()`, 2-arg `(frame, scene)`, and cross-scene 1-arg label.
        project.symbol_mut(&s1_id).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript3,
                source: r#"gotoAndStop("s2_final");"#.into(),
            },
        }];
        let mut pb_label = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
        assert_eq!((pb_label.symbol(), pb_label.frame()), (s2_id.as_str(), 1));
        assert!(pb_label.is_root_halted_by_script());
        assert_eq!(pb_label.drain_traces(), vec!["S2_F2".to_owned()]);

        project.symbol_mut(&s1_id).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript3,
                source: r#"nextScene();"#.into(),
            },
        }];
        let mut pb_next = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
        assert_eq!((pb_next.symbol(), pb_next.frame()), (s2_id.as_str(), 0));
        assert_eq!(pb_next.drain_traces(), vec!["S2_F1".to_owned()]);

        project.symbol_mut(&s2_id).unwrap().timeline.scripts[0] = crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript3,
                source: r#"gotoAndPlay(2, "Scene 1");"#.into(),
            },
        };
        let pb_as3_2arg = AuthoredPlayback::with_live_scripts(&project, &s2_id, 0, true);
        assert_eq!(
            (pb_as3_2arg.symbol(), pb_as3_2arg.frame()),
            (s1_id.as_str(), 1)
        );
        assert!(!pb_as3_2arg.is_root_halted_by_script());

        // 4. AS2 2-arg `gotoAndStop("Scene 2", 2)` uses `(scene, frame)` argument order!
        project.script_language = Some(crate::ScriptLanguage::ActionScript2);
        project.symbol_mut(&s1_id).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript2,
                source: r#"gotoAndStop("Scene 2", 2);"#.into(),
            },
        }];
        project.symbol_mut(&s2_id).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 1,
            script: crate::Script {
                language: crate::ScriptLanguage::ActionScript2,
                source: r#"trace("AS2_S2_F2");"#.into(),
            },
        }];
        let mut pb_as2 = AuthoredPlayback::with_live_scripts(&project, &s1_id, 0, true);
        assert_eq!((pb_as2.symbol(), pb_as2.frame()), (s2_id.as_str(), 1));
        assert!(pb_as2.is_root_halted_by_script());
        assert_eq!(pb_as2.drain_traces(), vec!["AS2_S2_F2".to_owned()]);
    }

    #[test]
    fn t320_runtime_display_graph_applies_camera_pose_parallax_tint_and_pinned_hud() {
        let mut project = Project::blank();
        project.width = 400;
        project.height = 300;
        project.format_version = 10;
        let root_id = project.root.clone();

        let box_sym_id: Id = "box-mc".into();
        let mut box_sym = project.symbol(&root_id).unwrap().clone();
        box_sym.id = box_sym_id.clone();
        box_sym.name = "BoxMc".into();
        box_sym.kind = crate::SymbolKind::MovieClip;
        box_sym.timeline.frame_count = 1;
        box_sym.timeline.layers[0].id = "box-layer".into();
        box_sym.timeline.layers[0].keyframes[0].elements = vec![crate::Element {
            id: "box-rect".into(),
            name: String::new(),
            kind: crate::ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill: [200, 100, 50, 255],
                stroke: [0, 0, 0, 0],
                stroke_width: 0.0,
            },
            ..crate::Element::default()
        }];
        project.symbols.push(box_sym);

        {
            let root = project.symbol_mut(&root_id).unwrap();
            root.timeline.frame_count = 1;
            // Layer 0: active Camera layer panning to (+50, 0) with 2x zoom and 0.5 red tint.
            let mut cam_layer = root.timeline.layers[0].clone();
            cam_layer.id = "cam-layer".into();
            cam_layer.name = "Camera".into();
            cam_layer.keyframes[0].elements.clear();
            cam_layer.keyframes[0].camera = Some(crate::CameraPose {
                x: 50.0,
                y: 0.0,
                zoom: 2.0,
                rotation: 0.0,
                color_transform: crate::ColorTransform {
                    multipliers: [0.5, 1.0, 1.0, 1.0],
                    offsets: [0.0; 4],
                },
                ..crate::CameraPose::default()
            });

            // Layer 1: normal world layer (z_depth = 0) with clip at stage center (200, 150).
            let mut world_layer = root.timeline.layers[0].clone();
            world_layer.id = "world-layer".into();
            world_layer.name = "World".into();
            world_layer.keyframes[0].elements = vec![crate::Element {
                id: "world-inst".into(),
                name: "worldMc".into(),
                transform: crate::Transform {
                    x: 200.0,
                    y: 150.0,
                    ..crate::Transform::default()
                },
                kind: crate::ElementKind::Instance {
                    symbol_id: box_sym_id.clone(),
                    playback: crate::InstancePlayback::Independent,
                },
                ..crate::Element::default()
            }];

            // Layer 2: parallax layer (z_depth = 200 -> focal_length/(focal_length+z_depth)) at (200, 150).
            let mut parallax_layer = world_layer.clone();
            parallax_layer.id = "parallax-layer".into();
            parallax_layer.name = "Parallax".into();
            parallax_layer.properties.z_depth = 200.0;
            parallax_layer.keyframes[0].elements[0].id = "parallax-inst".into();
            parallax_layer.keyframes[0].elements[0].name = "parallaxMc".into();

            // Layer 3: HUD layer pinned to camera (`attach_to_camera = true`) at (25, 15).
            let mut hud_layer = world_layer.clone();
            hud_layer.id = "hud-layer".into();
            hud_layer.name = "HUD".into();
            hud_layer.properties.attach_to_camera = true;
            hud_layer.keyframes[0].elements[0].id = "hud-inst".into();
            hud_layer.keyframes[0].elements[0].name = "hudMc".into();
            hud_layer.keyframes[0].elements[0].transform = crate::Transform {
                x: 25.0,
                y: 15.0,
                ..crate::Transform::default()
            };

            root.timeline.layers = vec![cam_layer, world_layer, parallax_layer, hud_layer];
        }
        project.upgrade_format_for_features();
        project.validate().unwrap();

        let player = crate::Player::new(&project);
        let snap = player.runtime_snapshot(&project);

        let world_node = snap
            .objects
            .iter()
            .find(|n| n.display.name == "worldMc")
            .expect("worldMc node");
        let parallax_node = snap
            .objects
            .iter()
            .find(|n| n.display.name == "parallaxMc")
            .expect("parallaxMc node");
        let hud_node = snap
            .objects
            .iter()
            .find(|n| n.display.name == "hudMc")
            .expect("hudMc node");

        // Authored local matrices stay pure (unmutated by camera).
        assert!((world_node.local_matrix[0] - 1.0).abs() < 1e-6);
        assert!((world_node.local_matrix[4] - 200.0).abs() < 1e-6);
        assert!((world_node.local_matrix[5] - 150.0).abs() < 1e-6);

        // World clip at (200, 150) with camera (+50, 0) zoom 2 around (200, 150) -> scaled 2x and shifted by -50*2 = -100 -> x = 100, y = 150.
        assert!((world_node.display.world[0] - 2.0).abs() < 1e-6);
        assert!((world_node.display.world[3] - 2.0).abs() < 1e-6);
        assert!((world_node.display.world[4] - 100.0).abs() < 1e-6);
        assert!((world_node.display.world[5] - 150.0).abs() < 1e-6);

        // Parallax clip (`z_depth = 200.0`) has distinct perspective scale (< 2.0) and pan (> 100.0).
        assert!(parallax_node.display.world[0] < 1.9 && parallax_node.display.world[0] > 0.5);
        assert!(parallax_node.display.world[4] > 105.0 && parallax_node.display.world[4] < 200.0);

        // HUD clip (`attach_to_camera = true`) ignores camera transform completely!
        assert!((hud_node.display.world[0] - 1.0).abs() < 1e-6);
        assert!((hud_node.display.world[4] - 25.0).abs() < 1e-6);
        assert!((hud_node.display.world[5] - 15.0).abs() < 1e-6);

        let world_child = snap
            .objects
            .iter()
            .find(|n| n.parent_ref.as_ref() == Some(&world_node.object_ref))
            .expect("worldMc child shape");
        let hud_child = snap
            .objects
            .iter()
            .find(|n| n.parent_ref.as_ref() == Some(&hud_node.object_ref))
            .expect("hudMc child shape");
        let world_draw = snap
            .draws
            .iter()
            .find(|d| d.owner_ref == world_child.object_ref)
            .expect("worldMc draw");
        let hud_draw = snap
            .draws
            .iter()
            .find(|d| d.owner_ref == hud_child.object_ref)
            .expect("hudMc draw");
        let crate::runtime_graph::RuntimeDrawKind::Element(crate::ElementKind::Rectangle {
            fill: world_fill,
            ..
        }) = &world_draw.item.kind
        else {
            panic!("expected rectangle draw for worldMc");
        };
        let crate::runtime_graph::RuntimeDrawKind::Element(crate::ElementKind::Rectangle {
            fill: hud_fill,
            ..
        }) = &hud_draw.item.kind
        else {
            panic!("expected rectangle draw for hudMc");
        };
        assert_eq!(
            world_fill[0], 100,
            "camera color_transform (0.5 red) must tint non-attached layers (200 -> 100)"
        );
        assert_eq!(
            hud_fill[0], 200,
            "attach_to_camera HUD layer must not receive camera color_transform"
        );
    }

    #[test]
    fn t368_authored_playback_traces_multidimensional_arrays_objects_and_container_mutations() {
        let mut project = Project::blank();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let root = project.root.clone();
        project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
            layer: None,
            frame: 0,
            script: crate::Script {
                language: ScriptLanguage::ActionScript2,
                source: [
                    "stop();",
                    "var row = [0, 1, 0];",
                    "trace(row);",
                    "var TFigure = [[0, 1, 0], [1, 1, 1], [0, 0, 0]];",
                    "trace(TFigure);",
                    "var AllFigures = new Array(TFigure, [[1, 1, 1, 1]]);",
                    "trace(AllFigures);",
                    "var sized = new Array(3);",
                    "var wrapped = new Array(row);",
                    "trace(sized.length, wrapped);",
                    "TFigure[0][2] = 9;",
                    "TFigure.push([2, 2, 2]);",
                    "trace(TFigure);",
                    "var state = { title: \"Tetris\", score: 100, board: [[1, 0], [0, 1]] };",
                    "state.score += 50;",
                    "state.board[1][0] = 7;",
                    "trace(state);",
                    "trace(\"csv=\" + row + \"|obj=\" + state);",
                    "new Sound();",
                ]
                .join("\n"),
            },
        }];

        let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
        let traces = preview.drain_traces();
        assert_eq!(
            traces,
            vec![
                "[0, 1, 0]",
                "[\n  [0, 1, 0],\n  [1, 1, 1],\n  [0, 0, 0]\n]",
                "[\n  [\n    [0, 1, 0],\n    [1, 1, 1],\n    [0, 0, 0]\n  ],\n  [\n    [1, 1, 1, 1]\n  ]\n]",
                "3 [\n  [0, 1, 0]\n]",
                "[\n  [0, 1, 9],\n  [1, 1, 1],\n  [0, 0, 0],\n  [2, 2, 2]\n]",
                "{\n  title: \"Tetris\",\n  score: 150,\n  board: [\n    [1, 0],\n    [7, 1]\n  ]\n}",
                "csv=0,1,0|obj=[object Object]",
            ]
        );
        let diags = preview.drain_diagnostics();
        assert!(
            diags.iter().any(|d| d.contains("new Sound()")
                && d.contains("outside the bounded native stage preview subset")),
            "unsupported constructor statements must continue emitting diagnostics: {diags:?}"
        );
    }

    #[test]
    fn t369_authored_playback_array_join_nested_matrices_chained_string_methods_and_unsupported_call_diagnostics()
     {
        let run = |lang: ScriptLanguage, src: &str| -> (Vec<String>, Vec<String>) {
            let mut project = Project::blank();
            project.script_language = Some(lang);
            let root = project.root.clone();
            project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: lang,
                    source: src.to_owned(),
                },
            }];
            let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
            (preview.drain_traces(), preview.drain_diagnostics())
        };

        // Exact user snippet + chained flatten + builtin Math/String/Array value methods + unsupported call diagnostic
        let (as2_traces, as2_diags) = run(
            ScriptLanguage::ActionScript2,
            &[
                "stop();",
                "lFigure = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);",
                "f=lFigure.join(\"\");",
                "trace ( f );",
                "trace ( lFigure );",
                "flat = lFigure.join(\"\").split(\",\").join(\"\");",
                "trace ( flat, flat.length, flat.slice(1, 5), Math.floor(3.9), parseInt(\"0x10\") );",
                "avm1Join = [\"zero\", undefined, null, \"five\"].join(\"/\");",
                "avm1Hole = new Array(2).toString();",
                "trace ( avm1Join, avm1Hole );",
                "bad = lFigure.length();",
                "trace ( bad );",
            ]
            .join("\n"),
        );
        assert_eq!(
            as2_traces,
            vec![
                "0,1,00,1,00,1,00,1,0",
                "[\n  [0, 1, 0],\n  [0, 1, 0],\n  [0, 1, 0],\n  [0, 1, 0]\n]",
                "010010010010 12 1001 3 16",
                "zero/undefined/null/five undefined,undefined",
                "undefined",
            ]
        );
        assert!(
            as2_diags.iter().any(|d| d.contains("lFigure.length()")
                && d.contains("outside the bounded native stage preview subset")),
            "unsupported call `.length()` must emit a preview diagnostic instead of storing raw source text: {as2_diags:?}"
        );

        let (as1_traces, as1_diags) = run(
            ScriptLanguage::ActionScript1,
            "stop();\ntrace([\"zero\", undefined, null, \"five\"].join(\"/\"), new Array(2).toString());",
        );
        assert!(as1_diags.is_empty());
        assert_eq!(as1_traces, vec!["zero//null/five ,"]);

        let (as3_traces, as3_diags) = run(
            ScriptLanguage::ActionScript3,
            "stop();\nvar lFigure:Array = new Array([0,1,0],[0,1,0],[0,1,0],[0,1,0]);\nvar f:String = lFigure.join(\"\");\ntrace(f, [\"zero\", undefined, null, \"five\"].join(\"/\"), new Array(2).toString());",
        );
        assert!(as3_diags.is_empty());
        assert_eq!(as3_traces, vec!["0,1,00,1,00,1,00,1,0 zero///five ,"]);
    }

    #[test]
    fn t370_authored_playback_string_array_eval_set_and_bitwise_operators_conformance() {
        let run = |lang: ScriptLanguage, src: &str| -> (Vec<String>, Vec<String>) {
            let mut project = Project::blank();
            project.script_language = Some(lang);
            let root = project.root.clone();
            project.symbol_mut(&root).unwrap().timeline.scripts = vec![crate::FrameScript {
                layer: None,
                frame: 0,
                script: crate::Script {
                    language: lang,
                    source: src.to_owned(),
                },
            }];
            let mut preview = AuthoredPlayback::with_live_scripts(&project, &root, 0, true);
            (preview.drain_traces(), preview.drain_diagnostics())
        };

        // 1. AS2: String ops (+, +=, add/eq/ne/lt/gt/<>, Flash 4 chr/ord/length/substring),
        // Array ops (sparse literals, compound bitwise/arith on elements, delete hole, in, instanceof),
        // eval/set (named & slash/colon & container paths, object identity, safe malformed/expr paths),
        // and Bitwise ops (&, |, ^, ~, <<, >>, >>>, &=, |=, ^=, <<=, >>=, >>>=, >32-bit wrapping).
        let (as2_traces, as2_diags) = run(
            ScriptLanguage::ActionScript2,
            &[
                "stop();",
                // String operators & dialect coercions:
                "var s = \"\" + [undefined, null, 1];",
                "var pairConcat = [1, 2] + [3, 4];",
                "var legAdd = (undefined add \"X\") + \"|\" + (\"10\" lt \"2\") + \"|\" + (undefined eq \"undefined\") + \"|\" + (3 <> 4);",
                "var legFns = chr(65) + \"|\" + ord(\"B\") + \"|\" + length(\"neo\") + \"|\" + substring(\"abcdef\", 2, 3);",
                "var leftAssoc = String((\"a\" + 5) - 2) + \"|\" + (10 - 3 + 2) + \"|\" + (12 / 3 * 2);",
                "trace(s, pairConcat, legAdd, legFns, leftAssoc);",
                // Array operators:
                "var sp = [, 10, , 30,];",
                "var hasBefore = (0 in sp) + \",\" + (1 in sp) + \",\" + (\"length\" in sp) + \",\" + (sp instanceof Array);",
                "var arr = [1, 20, 30];",
                "arr[0] <<= 3; arr[0] |= 3; arr[0] &= 10; arr[0] ^= 1; arr[0] >>= 1; arr[0] >>>= 1;",
                "var del1 = delete arr[1]; var delAgain = delete arr[1]; var delLen = delete arr.length;",
                "trace(sp.length, hasBefore, arr[0], del1, delAgain, delLen, 1 in arr, arr.length);",
                // eval & set:
                "set(\"dynScore\", 42);",
                "var lFigure = new Array([0, 1, 0], [0, 1, 0]);",
                "set(\"lFigure.0.2\", 9);",
                "var idx = 0;",
                "trace(eval(\"dyn\" + \"Score\"), eval(\":dynScore\"), eval(\"lFigure.\" + idx), eval(lFigure).join(\"\"), eval(\"1 + 2\"), eval(\"a..b\"), eval(\"\"));",
                // Bitwise operators & precedence:
                "var bw1 = ((undefined | 1) == 1) && ((null ^ 1) == 1) && ((4294967297 | 0) == 1) && ((~0) == -1) && ((1 << 33) == 2);",
                "var bw2 = (-8 >> 1) + \"|\" + (-1 >>> 0) + \"|\" + (-3 >>> 1) + \"|\" + (1 + 2 << 3) + \"|\" + (4 | 2 & 1);",
                "var grid = [[0, 1], [2, 0]];",
                "grid[0][0] |= (1 << 2);",
                "this._x = 4; this._x |= 2;",
                "trace(bw1, bw2, grid[0][0], this._x, typeof null, (grid[0][0] & 4) ? \"HIT\" : \"MISS\");",
            ]
            .join("\n"),
        );
        assert!(
            as2_diags.is_empty(),
            "unexpected AS2 diagnostics: {as2_diags:?}"
        );
        assert_eq!(
            as2_traces,
            vec![
                "undefined,null,1 1,23,4 undefinedX|true|true|true A|66|3|bcd NaN|9|8",
                "4 false,true,true,true 2 true false false false 3",
                "42 42 [0, 1, 9] 0,1,90,1,0 undefined undefined undefined",
                "true -4|4294967295|2147483646|24|4 4 6 null HIT",
            ]
        );

        // 2. AS1: SWF6 `undefined` -> `""` in `+`/`add`/`eq`/`String()`, `undefined < 1 == true`, and case-insensitive `eval`/`set`.
        let (as1_traces, as1_diags) = run(
            ScriptLanguage::ActionScript1,
            &[
                "stop();",
                "set(\"/:RootScore\", 77);",
                "var s1 = \"\" + [undefined, null, 1];",
                "var leg1 = (undefined add \"Z\") + \"|\" + (undefined eq \"\") + \"|\" + (undefined < 1) + \"|\" + String(undefined);",
                "trace(eval(\"/:ROOTSCORE\"), s1, leg1, (1 and 1), (0 or 1), (not 0));",
            ]
            .join("\n"),
        );
        assert!(
            as1_diags.is_empty(),
            "unexpected AS1 diagnostics: {as1_diags:?}"
        );
        assert_eq!(as1_traces, vec!["77 ,null,1 Z|true|true| true true true"]);

        // 3. AS3: `"" + [undefined, null, 1] == ",,1"`, `is`/`as`, `typeof null == "object"`, `delete arr[99] == true`,
        // and explicit diagnostics when removed `eval()` / `set()` are called.
        let (as3_traces, as3_diags) = run(
            ScriptLanguage::ActionScript3,
            &[
                "stop();",
                "var arr:Array = [undefined, null, 1];",
                "var u:uint = uint(-1 >>> 0);",
                "var delMissing:Boolean = delete arr[99];",
                "trace(\"\" + arr, arr is Array, (arr as Array).length, typeof null, delMissing, u);",
                "var badEval:* = eval(\"arr\");",
                "set(\"x\", 1);",
            ]
            .join("\n"),
        );
        assert_eq!(as3_traces, vec![",,1 true 3 object true 4294967295"]);
        assert!(
            as3_diags.iter().any(|d| d.contains("eval(\"arr\")"))
                && as3_diags.iter().any(|d| d.contains("removed set()")),
            "AS3 must diagnose removed eval() and set(): {as3_diags:?}"
        );

        // 4. JavaScript: bitwise ops, array ops, short-circuit `||`, and pure expression `eval("6 * 7")`.
        let (js_traces, js_diags) = run(
            ScriptLanguage::JavaScript,
            "stop();\nlet flags = 1; flags <<= 3; flags |= 3;\ntrace(flags, \"\" || \"fallback\", eval(\"6 * 7\"), (-1 >>> 1));",
        );
        assert!(
            js_diags.is_empty(),
            "unexpected JS diagnostics: {js_diags:?}"
        );
        assert_eq!(js_traces, vec!["11 fallback 42 2147483647"]);
    }
}
