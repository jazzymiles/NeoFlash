//! Runtime input capabilities and hit-area identities, independent of painting.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayInputState {
    pub mouse_enabled: bool,
    pub mouse_children: bool,
    pub button_mode: bool,
    pub use_hand_cursor: bool,
    pub hit_area: Option<ObjectRef>,
    #[serde(default = "enabled_default")]
    pub movieclip_enabled: bool,
}

fn enabled_default() -> bool {
    true
}

#[derive(Default)]
pub(super) struct MovieClipPointer {
    over: Option<ObjectRef>,
    pressed: Option<ObjectRef>,
}

impl MovieClipPointer {
    fn release(&mut self, target: &ObjectRef) {
        if self.over.as_ref() == Some(target) {
            self.over = None;
        }
        if self.pressed.as_ref() == Some(target) {
            self.pressed = None;
        }
    }
}

fn sprite(owner: &str) -> bool {
    matches!(owner, "sprite" | "movie_clip" | "graphic")
}
fn container(owner: &str) -> bool {
    sprite(owner) || matches!(owner, "button" | "loader")
}
impl DisplayInputState {
    fn for_owner(owner: &str) -> Option<Self> {
        if !container(owner) && owner != "text" {
            return None;
        }
        Some(Self {
            mouse_enabled: true,
            mouse_children: true,
            button_mode: owner == "button",
            use_hand_cursor: true,
            hit_area: None,
            movieclip_enabled: true,
        })
    }
}
impl RuntimeGraph {
    pub(super) fn input_state(
        &self,
        reference: &ObjectRef,
        owner: &str,
    ) -> Option<DisplayInputState> {
        let defaults = DisplayInputState::for_owner(owner)?;
        let mut state = self
            .input_states
            .get(reference)
            .cloned()
            .unwrap_or_else(|| defaults.clone());
        // An authored ID may retain its placement lifetime while its leaf kind
        // changes. Old capabilities must not leak into the new display kind.
        if !container(owner) {
            state.mouse_children = defaults.mouse_children;
        }
        if !sprite(owner) && owner != "button" {
            state.button_mode = defaults.button_mode;
            state.use_hand_cursor = defaults.use_hand_cursor;
        }
        if !sprite(owner) {
            state.hit_area = None;
        }
        if owner != "movie_clip" {
            state.movieclip_enabled = true;
        }
        Some(state)
    }
    pub(super) fn retire_input(&mut self, retired: &BTreeSet<ObjectRef>) {
        self.input_states
            .retain(|reference, _| !retired.contains(reference));
        self.as2_accessibility
            .retain(|reference, _| !retired.contains(reference));
        for reference in retired {
            self.movieclip_pointer.release(reference);
        }
        for state in self.input_states.values_mut() {
            if state
                .hit_area
                .as_ref()
                .is_some_and(|reference| retired.contains(reference))
            {
                state.hit_area = None;
            }
        }
    }
    pub(super) fn sanitize_input(snapshot: &mut RuntimeSnapshot) {
        let eligible: BTreeSet<_> = snapshot
            .objects
            .iter()
            .filter(|node| sprite(&node.display.owner))
            .map(|node| node.object_ref.clone())
            .collect();
        for node in &mut snapshot.objects {
            if let Some(state) = &mut node.input
                && state
                    .hit_area
                    .as_ref()
                    .is_some_and(|reference| !eligible.contains(reference))
            {
                state.hit_area = None;
            }
        }
    }
}
impl Player {
    pub fn object_input_info(
        &self,
        project: &Project,
        target: &ObjectRef,
    ) -> Result<DisplayInputState, DisplayError> {
        if !self.graph.alive(target) {
            return Err(error(
                "Input reference is stale or belongs to another player",
            ));
        }
        self.runtime_snapshot(project).objects.into_iter()
            .find(|node| &node.object_ref == target)
            .and_then(|node| node.input)
            .ok_or_else(|| error("Mouse input state requires an InteractiveObject; Shape, Bitmap and Video are not interactive"))
    }
    pub fn object_input_update(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        state: DisplayInputState,
    ) -> Result<(), DisplayError> {
        if !self.graph.alive(target) {
            return Err(error(
                "Input reference is stale or belongs to another player",
            ));
        }
        let snapshot = self.runtime_snapshot(project);
        let node = snapshot
            .objects
            .iter()
            .find(|node| &node.object_ref == target)
            .ok_or_else(|| error("Input target is absent from the display graph"))?;
        let owner = node.display.owner.as_str();
        let defaults = DisplayInputState::for_owner(owner)
            .ok_or_else(|| error("Mouse input state requires an InteractiveObject; Shape, Bitmap and Video are not interactive"))?;
        if owner != "movie_clip" && !state.movieclip_enabled {
            return Err(error("MovieClip enabled requires a MovieClip instance"));
        }
        if !container(owner) && state.mouse_children != defaults.mouse_children {
            return Err(error("mouseChildren requires a display container"));
        }
        if !sprite(owner)
            && owner != "button"
            && (state.button_mode != defaults.button_mode
                || state.use_hand_cursor != defaults.use_hand_cursor)
        {
            return Err(error(
                "buttonMode/useHandCursor require a Sprite, MovieClip or button",
            ));
        }
        if let Some(source) = &state.hit_area {
            if !sprite(owner) {
                return Err(error("hitArea requires a Sprite or MovieClip target"));
            }
            let source_node = snapshot
                .objects
                .iter()
                .find(|node| &node.object_ref == source)
                .ok_or_else(|| error("hitArea source is stale or belongs to another player"))?;
            if !sprite(&source_node.display.owner) {
                return Err(error("hitArea source must be a Sprite or MovieClip"));
            }
            let inputs: HashMap<_, _> = snapshot
                .objects
                .iter()
                .filter_map(|node| node.input.as_ref().map(|input| (&node.object_ref, input)))
                .collect();
            let mut next = Some(source);
            let mut seen = BTreeSet::new();
            while let Some(current) = next {
                if current == target || !seen.insert(current) {
                    return Err(error("Cyclic/self hitArea assignments are not supported"));
                }
                if seen.len() > MAX_GRAPH_DEPTH {
                    return Err(error("hitArea reference chain exceeds 64 objects"));
                }
                next = inputs
                    .get(current)
                    .and_then(|input| input.hit_area.as_ref());
            }
        }
        if !state.movieclip_enabled || !state.button_mode {
            self.graph.movieclip_pointer.release(target);
        }
        if state == defaults {
            self.graph.input_states.remove(target);
        } else {
            self.graph.input_states.insert(target.clone(), state);
        }
        Ok(())
    }

    /// Mouse observations drive local MovieClip button labels in Rust. This
    /// does not filter normal AS3 mouse/key events or change real buttons.
    pub fn object_movieclip_pointer(
        &mut self,
        project: &Project,
        event: &str,
        target: Option<&ObjectRef>,
    ) -> Result<bool, DisplayError> {
        if !matches!(event, "mouseMove" | "mouseDown" | "mouseUp" | "cancel") {
            return Err(error(
                "MovieClip pointer event must be mouseMove, mouseDown, mouseUp or cancel",
            ));
        }
        if target.is_some_and(|reference| !self.graph.alive(reference)) {
            return Err(error(
                "MovieClip pointer target is stale or belongs to another player",
            ));
        }
        let snapshot = self.runtime_snapshot(project);
        let eligible: BTreeSet<_> = snapshot
            .objects
            .iter()
            .filter(|node| {
                node.on_stage
                    && node.display.owner == "movie_clip"
                    && node
                        .input
                        .as_ref()
                        .is_some_and(|input| input.button_mode && input.movieclip_enabled)
                    && self.object_metadata(project, &node.object_ref).is_some()
            })
            .map(|node| node.object_ref.clone())
            .collect();
        let mut next = None;
        if event != "cancel" {
            let mut candidate = target;
            for _ in 0..=MAX_GRAPH_DEPTH {
                let Some(reference) = candidate else { break };
                let Some(node) = snapshot
                    .objects
                    .iter()
                    .find(|node| &node.object_ref == reference)
                else {
                    break;
                };
                // Real buttons retain their existing independent pointer path.
                if node.display.owner == "button" {
                    break;
                }
                if eligible.contains(reference) {
                    next = Some(reference.clone());
                    break;
                }
                candidate = node.parent_ref.as_ref();
            }
        }
        let state = &mut self.graph.movieclip_pointer;
        if state
            .over
            .as_ref()
            .is_some_and(|reference| !eligible.contains(reference))
        {
            state.over = None;
        }
        if state
            .pressed
            .as_ref()
            .is_some_and(|reference| !eligible.contains(reference))
        {
            state.pressed = None;
        }
        let mut transitions: Vec<(ObjectRef, &'static str)> = vec![];
        let mut transition = |target: &Option<ObjectRef>, label: &'static str| {
            if let Some(target) = target {
                if let Some((_, previous)) = transitions.iter_mut().find(|(id, _)| id == target) {
                    *previous = label;
                } else {
                    transitions.push((target.clone(), label));
                }
            }
        };
        // Semantic reference: Ruffle fe6cfc545405b888ebb478ce1c211d63147ca49b,
        // display_object/movie_clip.rs::event_dispatch. DragOut uses _over;
        // ReleaseOutside/RollOut use _up, while Press/DragOver use _down.
        match event {
            "cancel" => {
                transition(&state.over, "_up");
                transition(&state.pressed, "_up");
                state.over = None;
                state.pressed = None;
            }
            "mouseDown" => {
                if state.over != next {
                    transition(&state.over, "_up");
                }
                transition(&next, "_down");
                state.over = next.clone();
                state.pressed = next;
            }
            "mouseUp" => {
                if state.over != next && state.over != state.pressed {
                    transition(&state.over, "_up");
                }
                transition(
                    &state.pressed,
                    if state.pressed == next {
                        "_over"
                    } else {
                        "_up"
                    },
                );
                if state.pressed != next && (state.pressed.is_some() || state.over != next) {
                    transition(&next, "_over");
                }
                state.over = next;
                state.pressed = None;
            }
            _ => {
                if state.over != next {
                    transition(
                        &state.over,
                        if state.over == state.pressed {
                            "_over"
                        } else {
                            "_up"
                        },
                    );
                    if state.pressed.is_none() || next == state.pressed {
                        transition(
                            &next,
                            if next == state.pressed {
                                "_down"
                            } else {
                                "_over"
                            },
                        );
                    }
                    state.over = next;
                }
            }
        }
        let mut changed = false;
        for (target, label) in transitions {
            if self.graph.alive(&target)
                && let Some(frame) = self.object_resolve_frame(project, &target, label, false)
            {
                changed |=
                    self.object_control(project, &target, PlaybackCommand::GotoAndStop(frame));
            }
        }
        Ok(changed)
    }
}
