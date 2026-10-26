use crate::evaluate::{Evaluation, Node, TransformPatch, snapshot};
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

const SCRIPT_BUDGET: usize = 4096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptEvent {
    Load,
    Click,
    EnterFrame,
    Frame(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptDispatch {
    pub target_path: Vec<Id>,
    pub target_lifetime: u64,
    pub event: ScriptEvent,
    pub script: Script,
}

#[derive(Clone, Copy, Debug)]
pub enum PlaybackCommand {
    Play,
    Stop,
    GotoAndPlay(u32),
    GotoAndStop(u32),
}

#[derive(Clone, Debug)]
struct Clock {
    symbol_id: Id,
    frame: u32,
    playing: bool,
}

/// Stateful playback. Scripts are queued as source, never evaluated by authoring code.
///
/// Dispatch order: newly placed objects' Load (preorder), entered timeline frame
/// scripts (root first), then object EnterFrame (preorder). Click bubbles from the
/// leaf through its ancestors. These are NeoFlash's defined initial semantics.
pub struct Player {
    clocks: HashMap<Vec<Id>, Clock>,
    overrides: HashMap<Vec<Id>, TransformPatch>,
    previous: HashMap<Vec<Id>, Node>,
    lifetimes: HashMap<Vec<Id>, u64>,
    next_lifetime: u64,
    scripts: VecDeque<ScriptDispatch>,
    playing: bool,
    budget: usize,
    overflowed: bool,
}

impl Player {
    pub fn new(project: &Project) -> Self {
        let mut player = Self {
            clocks: HashMap::new(),
            overrides: HashMap::new(),
            previous: HashMap::new(),
            lifetimes: HashMap::new(),
            next_lifetime: 1,
            scripts: VecDeque::new(),
            playing: true,
            budget: SCRIPT_BUDGET,
            overflowed: false,
        };
        player.reset(project);
        player
    }

    pub fn reset(&mut self, project: &Project) {
        self.clocks.clear();
        self.overrides.clear();
        self.previous.clear();
        self.lifetimes.clear();
        self.scripts.clear();
        self.playing = true;
        self.budget = SCRIPT_BUDGET;
        self.overflowed = false;
        self.clocks.insert(
            vec![],
            Clock {
                symbol_id: project.root.clone(),
                frame: 0,
                playing: true,
            },
        );
        self.refresh(project, false, &HashSet::new());
    }

    pub fn frame(&self) -> u32 {
        self.clocks.get(&Vec::new()).map_or(0, |c| c.frame)
    }
    pub fn is_playing(&self) -> bool {
        self.playing
    }
    /// Global transport pause. Script stop() uses control(..., Stop) instead.
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }
    pub fn script_overflowed(&self) -> bool {
        self.overflowed
    }

    pub fn tick(&mut self, project: &Project) {
        if !self.playing {
            return;
        }
        self.budget = SCRIPT_BUDGET;
        self.overflowed = false;
        let mut entered = HashSet::new();
        for (path, clock) in &mut self.clocks {
            if clock.playing
                && let Some(symbol) = project.symbol(&clock.symbol_id)
            {
                clock.frame = (clock.frame + 1) % symbol.timeline.frame_count.max(1);
                entered.insert(path.clone());
            }
        }
        self.refresh(project, true, &entered);
    }

    /// Explicit preview seek resets nested runtime state. Authoring scrub uses evaluate().
    pub fn seek(&mut self, project: &Project, frame: u32) {
        self.clocks.clear();
        self.overrides.clear();
        self.previous.clear();
        self.lifetimes.clear();
        self.scripts.clear();
        self.budget = SCRIPT_BUDGET;
        self.overflowed = false;
        let count = project
            .symbol(&project.root)
            .map_or(1, |s| s.timeline.frame_count.max(1));
        self.clocks.insert(
            vec![],
            Clock {
                symbol_id: project.root.clone(),
                frame: frame.min(count - 1),
                playing: true,
            },
        );
        self.refresh(project, false, &HashSet::new());
    }

    fn snapshot(&self, project: &Project) -> Evaluation {
        snapshot(
            project,
            &project.root,
            self.frame(),
            &|path| Some(self.clocks.get(path).map_or(0, |c| c.frame)),
            &|path| self.overrides.get(path).cloned(),
        )
    }

    pub fn evaluate(&self, project: &Project) -> Vec<DrawItem> {
        self.snapshot(project).items
    }

    fn stabilized(&mut self, project: &Project) -> Evaluation {
        // At most one pass per nesting level is required when a key swaps a symbol.
        for _ in 0..=crate::evaluate::MAX_DEPTH {
            let scene = self.snapshot(project);
            let mut changed = false;
            let mut alive = HashSet::new();
            for node in &scene.nodes {
                if !node.independent {
                    continue;
                }
                let Some(symbol_id) = &node.symbol_id else {
                    continue;
                };
                alive.insert(node.path.clone());
                match self.clocks.get_mut(&node.path) {
                    Some(clock) if clock.symbol_id == *symbol_id => {
                        let count = project
                            .symbol(symbol_id)
                            .map_or(1, |s| s.timeline.frame_count.max(1));
                        if clock.frame >= count {
                            clock.frame = count - 1;
                            changed = true;
                        }
                    }
                    _ => {
                        self.clocks.insert(
                            node.path.clone(),
                            Clock {
                                symbol_id: symbol_id.clone(),
                                frame: 0,
                                playing: true,
                            },
                        );
                        changed = true;
                    }
                }
            }
            self.clocks.retain(|path, _| alive.contains(path));
            let visible: HashSet<_> = scene.nodes.iter().map(|n| n.path.clone()).collect();
            self.overrides.retain(|path, _| visible.contains(path));
            if !changed {
                return scene;
            }
        }
        self.snapshot(project)
    }

    fn enqueue(&mut self, target_path: Vec<Id>, event: ScriptEvent, script: Script) {
        if script.source.trim().is_empty() {
            return;
        }
        if self.budget == 0 || self.scripts.len() >= SCRIPT_BUDGET {
            self.overflowed = true;
            return;
        }
        self.budget -= 1;
        self.scripts.push_back(ScriptDispatch {
            target_lifetime: self.lifetimes.get(&target_path).copied().unwrap_or(0),
            target_path,
            event,
            script,
        });
    }

    fn refresh(&mut self, project: &Project, enter_frame: bool, forced: &HashSet<Vec<Id>>) {
        let scene = self.stabilized(project);
        for node in &scene.nodes {
            let is_new = self
                .previous
                .get(&node.path)
                .is_none_or(|old| old.symbol_id != node.symbol_id);
            if is_new {
                self.lifetimes.insert(node.path.clone(), self.next_lifetime);
                self.next_lifetime = self.next_lifetime.wrapping_add(1);
                for handler in &node.scripts {
                    if handler.event == ObjectEvent::Load {
                        self.enqueue(node.path.clone(), ScriptEvent::Load, handler.script.clone());
                    }
                }
            }
        }
        for node in &scene.nodes {
            if let Some(symbol_id) = &node.symbol_id {
                let entered = forced.contains(&node.path)
                    || self.previous.get(&node.path).is_none_or(|old| {
                        old.frame != node.frame || old.symbol_id != node.symbol_id
                    });
                if entered && let Some(symbol) = project.symbol(symbol_id) {
                    for script in &symbol.timeline.scripts {
                        if script.frame == node.frame {
                            self.enqueue(
                                node.path.clone(),
                                ScriptEvent::Frame(node.frame),
                                script.script.clone(),
                            );
                        }
                    }
                }
            }
        }
        if enter_frame {
            for node in &scene.nodes {
                for handler in &node.scripts {
                    if handler.event == ObjectEvent::EnterFrame {
                        self.enqueue(
                            node.path.clone(),
                            ScriptEvent::EnterFrame,
                            handler.script.clone(),
                        );
                    }
                }
            }
        }
        self.previous = scene
            .nodes
            .into_iter()
            .map(|node| (node.path.clone(), node))
            .collect();
        self.lifetimes
            .retain(|path, _| self.previous.contains_key(path));
    }

    /// Changes when a placement is recreated; stays stable across continuous keys.
    pub fn lifetime_id(&self, path: &[Id]) -> Option<u64> {
        self.lifetimes.get(path).copied()
    }

    /// Control an independent MovieClip/root; synchronized Graphic instances follow their parent.
    pub fn control(&mut self, project: &Project, path: &[Id], command: PlaybackCommand) -> bool {
        let Some(clock) = self.clocks.get_mut(path) else {
            return false;
        };
        let count = project
            .symbol(&clock.symbol_id)
            .map_or(1, |s| s.timeline.frame_count.max(1));
        match command {
            PlaybackCommand::Play => clock.playing = true,
            PlaybackCommand::Stop => clock.playing = false,
            PlaybackCommand::GotoAndPlay(frame) | PlaybackCommand::GotoAndStop(frame) => {
                if frame >= count {
                    return false;
                }
                clock.frame = frame;
                clock.playing = matches!(command, PlaybackCommand::GotoAndPlay(_));
                let mut forced = HashSet::new();
                forced.insert(path.to_vec());
                self.refresh(project, false, &forced);
            }
        }
        true
    }

    pub fn dispatch_click(&mut self, project: &Project, path: &[Id]) {
        self.budget = SCRIPT_BUDGET;
        self.overflowed = false;
        let scene = self.snapshot(project);
        if !scene.nodes.iter().any(|n| n.path == path) {
            return;
        }
        for length in (1..=path.len()).rev() {
            if let Some(node) = scene.nodes.iter().find(|n| n.path == path[..length]) {
                for handler in &node.scripts {
                    if handler.event == ObjectEvent::Click {
                        self.enqueue(
                            node.path.clone(),
                            ScriptEvent::Click,
                            handler.script.clone(),
                        );
                    }
                }
            }
        }
    }

    pub fn drain_scripts(&mut self) -> Vec<ScriptDispatch> {
        self.scripts.drain(..).collect()
    }

    /// Hosts check each drained dispatch immediately before execution. A prior
    /// handler may have navigated away from this frame or removed this object.
    pub fn is_dispatch_current(&self, project: &Project, dispatch: &ScriptDispatch) -> bool {
        if self.lifetime_id(&dispatch.target_path) != Some(dispatch.target_lifetime) {
            return false;
        }
        let scene = self.snapshot(project);
        let Some(node) = scene
            .nodes
            .iter()
            .find(|node| node.path == dispatch.target_path)
        else {
            return false;
        };
        match dispatch.event {
            ScriptEvent::Frame(frame) => {
                if node.frame != frame {
                    return false;
                }
                node.symbol_id
                    .as_ref()
                    .and_then(|id| project.symbol(id))
                    .is_some_and(|symbol| {
                        symbol
                            .timeline
                            .scripts
                            .iter()
                            .any(|script| script.frame == frame && script.script == dispatch.script)
                    })
            }
            ScriptEvent::Load | ScriptEvent::Click | ScriptEvent::EnterFrame => {
                let event = match dispatch.event {
                    ScriptEvent::Load => ObjectEvent::Load,
                    ScriptEvent::Click => ObjectEvent::Click,
                    ScriptEvent::EnterFrame => ObjectEvent::EnterFrame,
                    ScriptEvent::Frame(_) => unreachable!(),
                };
                node.scripts
                    .iter()
                    .any(|handler| handler.event == event && handler.script == dispatch.script)
            }
        }
    }

    pub fn set_property(&mut self, path: &[Id], property: &str, value: f64) -> bool {
        if path.is_empty() || !value.is_finite() || !self.previous.contains_key(path) {
            return false;
        }
        if property == "alpha" && !(0.0..=1.0).contains(&value) {
            return false;
        }
        if !matches!(
            property,
            "x" | "y" | "scale_x" | "scale_y" | "rotation" | "alpha"
        ) {
            return false;
        }
        let patch = self.overrides.entry(path.to_vec()).or_default();
        match property {
            "x" => patch.x = Some(value),
            "y" => patch.y = Some(value),
            "scale_x" => patch.scale_x = Some(value),
            "scale_y" => patch.scale_y = Some(value),
            "rotation" => patch.rotation = Some(value),
            "alpha" => patch.alpha = Some(value),
            _ => unreachable!(),
        }
        true
    }

    pub fn property(&self, project: &Project, path: &[Id], property: &str) -> Option<f64> {
        let scene = self.snapshot(project);
        let t = scene.nodes.iter().find(|n| n.path == path)?.transform;
        match property {
            "x" => Some(t.x),
            "y" => Some(t.y),
            "scale_x" => Some(t.scale_x),
            "scale_y" => Some(t.scale_y),
            "rotation" => Some(t.rotation),
            "alpha" => Some(t.alpha),
            _ => None,
        }
    }

    pub fn clip_frame(&self, path: &[Id]) -> Option<u32> {
        self.previous
            .get(path)
            .filter(|n| n.symbol_id.is_some())
            .map(|n| n.frame)
    }

    pub fn clip_frame_count(&self, project: &Project, path: &[Id]) -> Option<u32> {
        let id = self.previous.get(path)?.symbol_id.as_ref()?;
        Some(project.symbol(id)?.timeline.frame_count)
    }

    pub fn resolve_frame(&self, project: &Project, path: &[Id], label: &str) -> Option<u32> {
        let id = self.previous.get(path)?.symbol_id.as_ref()?;
        project
            .symbol(id)?
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .find(|k| k.label == label && !label.is_empty())
            .map(|k| k.frame)
    }

    pub fn child_path(&self, project: &Project, parent: &[Id], name: &str) -> Option<Vec<Id>> {
        self.snapshot(project)
            .nodes
            .into_iter()
            .find(|n| {
                n.path.len() == parent.len() + 1 && n.path.starts_with(parent) && n.name == name
            })
            .map(|n| n.path)
    }
}
