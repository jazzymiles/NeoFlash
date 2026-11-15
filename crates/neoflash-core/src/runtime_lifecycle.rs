//! Rust-owned AS3/AS2 timeline frame lifecycle phases and bounded continuation stack.
use crate::runtime::{PlaybackCommand, Player, ScriptEvent};
use crate::runtime_graph::{DisplayEvent, ObjectRef, RuntimeScriptDispatch};
use crate::{Id, Project};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum nested re-entrant `gotoAndPlay` / `gotoAndStop` / `nextFrame` / `prevFrame` depth.
pub const MAX_LIFECYCLE_DEPTH: usize = 128;

/// Maximum lifecycle steps executed within a single top-level frame / dispatch cycle.
pub const MAX_LIFECYCLE_STEPS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    Construct,
    FrameConstructed,
    FrameScripts,
    ExitFrame,
    Render,
}

/// One deterministic lifecycle step returned to a script host (`WebPlayer` / JS runtime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleStep {
    pub generation: u64,
    pub depth: usize,
    pub inner_goto: bool,
    pub phase: LifecyclePhase,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display_events: Vec<DisplayEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<RuntimeScriptDispatch>,
}

#[derive(Debug, Clone)]
pub(crate) struct LifecycleScope {
    pub(crate) inner_goto: bool,
    pub(crate) next_phase: LifecyclePhase,
    pub(crate) frame_scripts_entered: bool,
    pub(crate) display_events: Vec<DisplayEvent>,
    pub(crate) scripts: VecDeque<RuntimeScriptDispatch>,
}

impl LifecycleScope {
    pub(crate) fn new(
        inner_goto: bool,
        display_events: Vec<DisplayEvent>,
        scripts: VecDeque<RuntimeScriptDispatch>,
    ) -> Self {
        Self {
            inner_goto,
            next_phase: LifecyclePhase::Construct,
            frame_scripts_entered: false,
            display_events,
            scripts,
        }
    }
}

struct StagedPending {
    display_events: Vec<DisplayEvent>,
    scripts: Vec<RuntimeScriptDispatch>,
}

fn overflow_error() -> String {
    "Script event limit exceeded. Check recursive gotoAndPlay/gotoAndStop calls.".into()
}

fn event_rank(event: &ScriptEvent) -> u8 {
    match event {
        ScriptEvent::Load => 0,
        ScriptEvent::Click | ScriptEvent::Frame(_) => 1,
        ScriptEvent::EnterFrame => 2,
    }
}

impl Player {
    /// Enable Rust-owned frame lifecycle state tracking (used by WebPlayer and lifecycle tests).
    pub fn enable_lifecycle(&mut self) {
        if !self.lifecycle_mode {
            self.lifecycle_mode = true;
            self.graph.set_lifecycle_mode(true);
            if self.lifecycle_stack.is_empty() {
                self.push_lifecycle_scope(false);
            }
        }
    }

    /// Monotonically incremented whenever `reset` or `seek` recreates the runtime root.
    pub fn reset_generation(&self) -> u64 {
        self.reset_generation
    }

    /// Current nesting depth of active lifecycle / inner-goto scopes.
    pub fn lifecycle_depth(&self) -> usize {
        self.lifecycle_stack.len()
    }

    fn drain_ordered_runtime_scripts(&mut self) -> Vec<RuntimeScriptDispatch> {
        let raw = self.drain_runtime_scripts();
        if raw.len() <= 1 {
            return raw;
        }
        let mut keyed: Vec<_> = raw
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let (detached, depth) = self.graph.object_tree_depth(&item.target_ref);
                let rank = event_rank(&item.dispatch.event);
                ((rank, detached, depth, index), item)
            })
            .collect();
        keyed.sort_by_key(|(key, _)| *key);
        keyed.into_iter().map(|(_, item)| item).collect()
    }

    pub(crate) fn push_lifecycle_scope(&mut self, inner_goto: bool) {
        let display_events = self.graph.take_lifecycle_events();
        let scripts = VecDeque::from(self.drain_ordered_runtime_scripts());
        self.lifecycle_stack
            .push(LifecycleScope::new(inner_goto, display_events, scripts));
    }

    fn absorb_pending_into_top(&mut self) {
        if self.lifecycle_stack.is_empty() {
            return;
        }
        let events = self.graph.take_lifecycle_events();
        let extra_scripts = self.drain_ordered_runtime_scripts();
        let top = self.lifecycle_stack.last_mut().expect("stack non-empty");
        if !events.is_empty() {
            top.display_events.extend(events);
        }
        if !extra_scripts.is_empty() {
            top.scripts.extend(extra_scripts);
        }
    }

    fn stash_staged(&mut self) -> Option<StagedPending> {
        if self.lifecycle_stack.is_empty() {
            Some(StagedPending {
                display_events: self.graph.take_lifecycle_events(),
                scripts: self.drain_ordered_runtime_scripts(),
            })
        } else {
            self.absorb_pending_into_top();
            None
        }
    }

    fn restore_staged(&mut self, stashed: Option<StagedPending>) {
        let Some(staged) = stashed else {
            return;
        };
        if !staged.display_events.is_empty() {
            self.graph.prepend_lifecycle_events(staged.display_events);
        }
        for item in staged.scripts.into_iter().rev() {
            self.runtime_scripts.push_front(item);
        }
    }

    fn finish_lifecycle_scope(&mut self) {
        let scope = self.lifecycle_stack.pop().expect("active lifecycle scope");
        // EXIT_FRAME callbacks may enqueue linked initialization or deferred
        // legacy navigation after this scope's frame-script phase has finished.
        // Return that work to the host's pending queue instead of dropping it.
        for item in scope.scripts.into_iter().rev() {
            self.runtime_scripts.push_front(item);
        }
    }

    fn check_lifecycle_budget(&mut self) -> Result<(), String> {
        if self.overflowed
            || self.lifecycle_stack.len() >= MAX_LIFECYCLE_DEPTH
            || self.lifecycle_steps >= MAX_LIFECYCLE_STEPS
        {
            self.overflowed = true;
            return Err(overflow_error());
        }
        Ok(())
    }

    /// Execute a timeline navigation on an authored path and start a synchronous inner-goto scope
    /// when jumping frames (`GotoAndPlay` / `GotoAndStop`).
    pub fn begin_inner_goto(
        &mut self,
        project: &Project,
        path: &[Id],
        command: PlaybackCommand,
    ) -> Result<usize, String> {
        if !matches!(
            command,
            PlaybackCommand::GotoAndPlay(_) | PlaybackCommand::GotoAndStop(_)
        ) {
            if !self.control(project, path, command) {
                return Ok(0);
            }
            return Ok(self.lifecycle_stack.len());
        }
        self.check_lifecycle_budget()?;
        let stashed = self.stash_staged();
        if !self.control(project, path, command) {
            self.restore_staged(stashed);
            return Ok(0);
        }
        if self.overflowed {
            self.restore_staged(stashed);
            return Err(overflow_error());
        }
        self.push_lifecycle_scope(true);
        self.restore_staged(stashed);
        Ok(self.lifecycle_stack.len())
    }

    /// Execute a timeline navigation on a display object reference and start a synchronous inner-goto
    /// scope when jumping frames (`GotoAndPlay` / `GotoAndStop`).
    pub fn object_begin_inner_goto(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        command: PlaybackCommand,
    ) -> Result<usize, String> {
        if !matches!(
            command,
            PlaybackCommand::GotoAndPlay(_) | PlaybackCommand::GotoAndStop(_)
        ) {
            if !self.object_control(project, target, command) {
                return Ok(0);
            }
            return Ok(self.lifecycle_stack.len());
        }
        self.check_lifecycle_budget()?;
        let stashed = self.stash_staged();
        if !self.object_control(project, target, command) {
            self.restore_staged(stashed);
            return Ok(0);
        }
        if self.overflowed {
            self.restore_staged(stashed);
            return Err(overflow_error());
        }
        self.push_lifecycle_scope(true);
        self.restore_staged(stashed);
        Ok(self.lifecycle_stack.len())
    }

    /// Advance the active top scope by one lifecycle phase. Returns `Ok(None)` when idle.
    pub fn lifecycle_next_step(&mut self) -> Result<Option<LifecycleStep>, String> {
        if self.overflowed || self.graph.events_overflowed_flag() {
            self.overflowed = true;
            return Err(overflow_error());
        }
        if self.lifecycle_stack.is_empty() {
            return Ok(None);
        }
        self.lifecycle_steps += 1;
        if self.lifecycle_steps > MAX_LIFECYCLE_STEPS {
            self.overflowed = true;
            return Err(overflow_error());
        }
        self.absorb_pending_into_top();
        let depth = self.lifecycle_stack.len();
        let generation = self.reset_generation;
        let top = self.lifecycle_stack.last_mut().expect("stack non-empty");
        let inner_goto = top.inner_goto;
        match top.next_phase {
            LifecyclePhase::Construct => {
                let display_events = std::mem::take(&mut top.display_events);
                top.next_phase = LifecyclePhase::FrameConstructed;
                Ok(Some(LifecycleStep {
                    generation,
                    depth,
                    inner_goto,
                    phase: LifecyclePhase::Construct,
                    display_events,
                    scripts: Vec::new(),
                }))
            }
            LifecyclePhase::FrameConstructed => {
                let display_events = std::mem::take(&mut top.display_events);
                top.next_phase = LifecyclePhase::FrameScripts;
                Ok(Some(LifecycleStep {
                    generation,
                    depth,
                    inner_goto,
                    phase: LifecyclePhase::FrameConstructed,
                    display_events,
                    scripts: Vec::new(),
                }))
            }
            LifecyclePhase::FrameScripts => {
                let display_events = std::mem::take(&mut top.display_events);
                let scripts: Vec<_> = top.scripts.drain(..).collect();
                let has_work = !display_events.is_empty() || !scripts.is_empty();
                if !top.frame_scripts_entered || has_work {
                    top.frame_scripts_entered = true;
                    if !has_work {
                        top.next_phase = LifecyclePhase::ExitFrame;
                    }
                    return Ok(Some(LifecycleStep {
                        generation,
                        depth,
                        inner_goto,
                        phase: LifecyclePhase::FrameScripts,
                        display_events,
                        scripts,
                    }));
                }
                if inner_goto {
                    self.finish_lifecycle_scope();
                } else {
                    top.next_phase = LifecyclePhase::Render;
                }
                Ok(Some(LifecycleStep {
                    generation,
                    depth,
                    inner_goto,
                    phase: LifecyclePhase::ExitFrame,
                    display_events: Vec::new(),
                    scripts: Vec::new(),
                }))
            }
            LifecyclePhase::ExitFrame => {
                let display_events = std::mem::take(&mut top.display_events);
                if inner_goto {
                    self.finish_lifecycle_scope();
                } else {
                    top.next_phase = LifecyclePhase::Render;
                }
                Ok(Some(LifecycleStep {
                    generation,
                    depth,
                    inner_goto,
                    phase: LifecyclePhase::ExitFrame,
                    display_events,
                    scripts: Vec::new(),
                }))
            }
            LifecyclePhase::Render => {
                let display_events = std::mem::take(&mut top.display_events);
                self.finish_lifecycle_scope();
                Ok(Some(LifecycleStep {
                    generation,
                    depth,
                    inner_goto: false,
                    phase: LifecyclePhase::Render,
                    display_events,
                    scripts: Vec::new(),
                }))
            }
        }
    }
}
