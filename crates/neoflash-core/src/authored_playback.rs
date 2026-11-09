//! Stateful authoring transport using the shared timeline clocks. Supports both
//! classic Flash script-ignore transport and live AS2/AS3/JS preview (`trace()`,
//! timeline control and display overrides) while keeping the authored document pure.
use crate::{
    AuthoredScene, Id, PlaybackCommand, Player, Project, ScriptDispatch, SymbolKind, VideoCommand,
};
use std::collections::HashMap;

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
    vars: HashMap<(Vec<Id>, String), String>,
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
            vars: HashMap::new(),
        };
        preview.process_scripts(project);
        preview
    }

    pub fn set_live_scripts(&mut self, live_scripts: bool) {
        self.live_scripts = live_scripts;
        if !live_scripts {
            self.script_halted_root = false;
        }
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
        // Button Up/Over/Down/Hit are selected authoring states, not a timeline
        // animation that should advance automatically through Hit.
        if !self.button_root {
            if !self.looping {
                let total_frames = project
                    .symbol(&self.symbol)
                    .map_or(1, |s| s.timeline.frame_count.max(1));
                if self.frame() + 1 >= total_frames {
                    self.player.set_playing(false);
                    self.process_scripts(project);
                    return;
                }
            }
            self.player.tick(project);
        }
        self.process_scripts(project);
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

    /// Dispatch a click event on a placed element path during live authoring preview.
    pub fn click(&mut self, project: &Project, path: &[Id]) {
        self.player.dispatch_click(project, path);
        self.process_scripts(project);
    }

    /// Script `trace(...)` lines produced since the last call.
    pub fn drain_traces(&mut self) -> Vec<String> {
        std::mem::take(&mut self.traces)
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

    fn process_scripts(&mut self, project: &Project) {
        let dispatches = self.player.drain_scripts();
        self.player.runtime_scripts.clear();
        for dispatch in dispatches {
            if !self.player.is_dispatch_current(project, &dispatch) {
                continue;
            }
            self.execute_preview_dispatch(project, &dispatch);
        }
    }

    fn execute_preview_dispatch(&mut self, project: &Project, dispatch: &ScriptDispatch) {
        let path = &dispatch.target_path;
        for raw_stmt in split_script_statements(&dispatch.script.source) {
            let stmt = strip_leading_comments(raw_stmt).trim();
            if stmt.is_empty() {
                continue;
            }
            // 1. Variable declaration: var/let/const name[:Type] = expr
            if let Some(rest) = stmt
                .strip_prefix("var ")
                .or_else(|| stmt.strip_prefix("let "))
                .or_else(|| stmt.strip_prefix("const "))
                && let Some((lhs, rhs)) = rest.split_once('=')
            {
                let name = lhs.split(':').next().unwrap_or(lhs).trim();
                if !name.is_empty() {
                    let val = self.eval_preview_expr(project, path, rhs.trim());
                    self.vars.insert((path.clone(), name.to_owned()), val);
                }
                continue;
            }
            // 2. trace(...)
            if let Some(inner) = extract_call_arg(stmt, "trace") {
                let text = self.eval_preview_expr(project, path, inner);
                self.traces.push(text);
                continue;
            }
            if !self.live_scripts {
                continue;
            }
            // 3. Timeline control functions when live_scripts is enabled
            if extract_call_arg(stmt, "stop") == Some("")
                || extract_call_arg(stmt, "this.stop") == Some("")
            {
                self.player.control(project, path, PlaybackCommand::Stop);
                if path.is_empty() {
                    self.script_halted_root = true;
                }
                continue;
            }
            if extract_call_arg(stmt, "play") == Some("")
                || extract_call_arg(stmt, "this.play") == Some("")
            {
                self.player.control(project, path, PlaybackCommand::Play);
                if path.is_empty() {
                    self.script_halted_root = false;
                }
                continue;
            }
            if extract_call_arg(stmt, "stopAllSounds") == Some("") {
                self.player.stop_all_sounds();
                continue;
            }
            if extract_call_arg(stmt, "nextFrame") == Some("")
                || extract_call_arg(stmt, "this.nextFrame") == Some("")
            {
                let cur = self
                    .player
                    .clip_frame(path)
                    .unwrap_or_else(|| self.player.frame());
                self.player
                    .control(project, path, PlaybackCommand::GotoAndStop(cur + 1));
                if path.is_empty() {
                    self.script_halted_root = true;
                }
                continue;
            }
            if extract_call_arg(stmt, "prevFrame") == Some("")
                || extract_call_arg(stmt, "this.prevFrame") == Some("")
            {
                let cur = self
                    .player
                    .clip_frame(path)
                    .unwrap_or_else(|| self.player.frame());
                self.player.control(
                    project,
                    path,
                    PlaybackCommand::GotoAndStop(cur.saturating_sub(1)),
                );
                if path.is_empty() {
                    self.script_halted_root = true;
                }
                continue;
            }
            if let Some(arg) = extract_call_arg(stmt, "gotoAndStop")
                .or_else(|| extract_call_arg(stmt, "this.gotoAndStop"))
            {
                if let Some(target_frame) = self.resolve_frame_arg(project, path, arg) {
                    self.player
                        .control(project, path, PlaybackCommand::GotoAndStop(target_frame));
                    if path.is_empty() {
                        self.script_halted_root = true;
                    }
                }
                continue;
            }
            if let Some(arg) = extract_call_arg(stmt, "gotoAndPlay")
                .or_else(|| extract_call_arg(stmt, "this.gotoAndPlay"))
            {
                if let Some(target_frame) = self.resolve_frame_arg(project, path, arg) {
                    self.player
                        .control(project, path, PlaybackCommand::GotoAndPlay(target_frame));
                    if path.is_empty() {
                        self.script_halted_root = false;
                    }
                }
                continue;
            }
            // 4. Property assignments on `this.` or bare `_prop`
            self.try_apply_property_mutation(project, path, stmt);
        }
    }

    fn resolve_frame_arg(&self, project: &Project, path: &[Id], arg: &str) -> Option<u32> {
        let evaluated = self.eval_preview_expr(project, path, arg);
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

    fn try_apply_property_mutation(&mut self, project: &Project, path: &[Id], stmt: &str) {
        let (lhs, op, rhs) = if let Some((l, r)) = stmt.split_once("+=") {
            (l.trim(), "+=", r.trim())
        } else if let Some((l, r)) = stmt.split_once("-=") {
            (l.trim(), "-=", r.trim())
        } else if let Some((l, r)) = stmt.split_once("*=") {
            (l.trim(), "*=", r.trim())
        } else if let Some((l, r)) = stmt.split_once('=')
            && !l.ends_with('!')
            && !l.ends_with('=')
            && !l.ends_with('<')
            && !l.ends_with('>')
            && !r.starts_with('=')
        {
            (l.trim(), "=", r.trim())
        } else {
            return;
        };
        let prop_raw = lhs.strip_prefix("this.").unwrap_or(lhs).trim();
        if matches!(prop_raw, "_visible" | "visible") && op == "=" {
            let val_str = self.eval_preview_expr(project, path, rhs);
            let visible = !matches!(val_str.as_str(), "false" | "0" | "");
            self.player.set_visible(path, visible);
            return;
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
            _ => return,
        };
        let Ok(rhs_num) = self.eval_preview_expr(project, path, rhs).parse::<f64>() else {
            return;
        };
        let cur = self
            .player
            .property(project, path, prop_name)
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
            _ => return,
        } * scale_factor;
        let next = if prop_name == "alpha" {
            next.clamp(0.0, 1.0)
        } else {
            next
        };
        self.player.set_property(path, prop_name, next);
    }

    fn eval_preview_expr(&self, project: &Project, path: &[Id], expr: &str) -> String {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        let parts = split_top_level_plus(trimmed);
        if parts.len() > 1 {
            let evaluated: Vec<String> = parts
                .into_iter()
                .map(|p| self.eval_preview_atom(project, path, p))
                .collect();
            if evaluated.iter().all(|s| s.parse::<f64>().is_ok()) {
                let sum: f64 = evaluated.iter().filter_map(|s| s.parse::<f64>().ok()).sum();
                return format_number(sum);
            }
            return evaluated.concat();
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
        if let Ok(n) = s.parse::<f64>() {
            return format_number(n);
        }
        if let Some(val) = self.vars.get(&(path.to_vec(), s.to_owned())) {
            return val.clone();
        }
        let prop = s
            .strip_prefix("this.")
            .or_else(|| s.strip_prefix("_root."))
            .unwrap_or(s);
        match prop {
            "_currentframe" | "currentFrame" => {
                let f = self
                    .player
                    .clip_frame(path)
                    .unwrap_or_else(|| self.player.frame());
                (f + 1).to_string()
            }
            "_totalframes" | "totalFrames" => {
                let sym_id = if path.is_empty() {
                    Some(self.symbol.as_str())
                } else {
                    self.player
                        .previous
                        .get(path)
                        .and_then(|n| n.symbol_id.as_deref())
                };
                let count = sym_id
                    .and_then(|id| project.symbol(id))
                    .map_or(1, |sym| sym.timeline.frame_count.max(1));
                count.to_string()
            }
            "_name" | "name" => path.last().cloned().unwrap_or_else(|| "root".into()),
            "x" | "_x" => self
                .player
                .property(project, path, "x")
                .map(format_number)
                .unwrap_or_else(|| "0".into()),
            "y" | "_y" => self
                .player
                .property(project, path, "y")
                .map(format_number)
                .unwrap_or_else(|| "0".into()),
            "rotation" | "_rotation" => self
                .player
                .property(project, path, "rotation")
                .map(format_number)
                .unwrap_or_else(|| "0".into()),
            "alpha" => self
                .player
                .property(project, path, "alpha")
                .map(format_number)
                .unwrap_or_else(|| "1".into()),
            "_alpha" => self
                .player
                .property(project, path, "alpha")
                .map(|a| format_number(a * 100.0))
                .unwrap_or_else(|| "100".into()),
            _ => s.to_owned(),
        }
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

fn strip_leading_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix("//") {
            if let Some(idx) = rest.find('\n') {
                s = &rest[idx + 1..];
            } else {
                return "";
            }
        } else if let Some(rest) = s.strip_prefix("/*") {
            if let Some(idx) = rest.find("*/") {
                s = &rest[idx + 2..];
            } else {
                return "";
            }
        } else {
            return s;
        }
    }
}

fn split_script_statements(source: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut paren_depth = 0usize;
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
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
        match b {
            b'\'' => in_single = true,
            b'"' => in_double = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b';' | b'\n' if paren_depth == 0 => {
                let seg = source[start..i].trim();
                if !seg.is_empty() {
                    out.push(seg);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < source.len() {
        let seg = source[start..].trim();
        if !seg.is_empty() {
            out.push(seg);
        }
    }
    out
}

fn extract_call_arg<'a>(stmt: &'a str, fn_name: &str) -> Option<&'a str> {
    let rest = stmt.strip_prefix(fn_name)?.trim_start();
    let rest = rest.strip_prefix('(')?;
    let rest = rest.strip_suffix(')')?;
    Some(rest.trim())
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
                parts.push(expr[start..i].trim());
                start = i + 1;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
