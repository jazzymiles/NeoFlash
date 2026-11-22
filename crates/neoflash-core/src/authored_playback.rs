//! Stateful authoring transport using the shared timeline clocks. Supports both
//! classic Flash script-ignore transport and live AS2/AS3/JS preview (`trace()`,
//! timeline control, instance handlers and display overrides) while keeping the
//! authored document pure.
use crate::{
    AccessibleRole, AuthoredScene, ElementKind, Id, PlaybackCommand, Player, Project,
    ScriptDispatch, ScriptEvent, ScriptLanguage, SymbolKind, VideoCommand,
};
use std::collections::{HashMap, HashSet};

mod containers;
mod expressions;
mod includes;
mod lexer;
mod statements;
#[cfg(test)]
mod tests;

use self::{containers::*, lexer::*};

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
        let evaluation = crate::evaluate::snapshot_mode_with_overrides(
            project,
            None,
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
            crate::evaluate::EvaluationMode {
                include_guides,
                publishing: live,
                show_masking,
            },
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
                    let expanded_for_check = if handler.script.language.is_actionscript()
                        && handler.script.source.contains("include")
                    {
                        let resources = Self::build_project_include_map(project);
                        Self::expand_script_includes(&handler.script, "", &resources)
                            .map(|(exp, _, _)| exp)
                            .unwrap_or_else(|_| handler.script.clone())
                    } else {
                        handler.script.clone()
                    };
                    if script_is_compiled(project, handler.script.language)
                        && handler.event != crate::ObjectEvent::Load
                        && !crate::runtime::is_as2_legacy_event_wrapper_script(&expanded_for_check)
                        && crate::runtime::contains_uncommented_as2_legacy_event_wrapper(
                            &expanded_for_check,
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
        let Some(expanded_script) = self.expand_preview_script(project, &dispatch.script, &origin)
        else {
            return;
        };
        let local_fns = collect_local_functions(&expanded_script.source);
        let prev_lang = self.current_language;
        self.current_language = expanded_script.language;
        self.execute_block(
            project,
            &dispatch.target_path,
            &dispatch.event,
            &expanded_script.source,
            &origin,
            1,
            &local_fns,
        );
        self.current_language = prev_lang;
    }
}
