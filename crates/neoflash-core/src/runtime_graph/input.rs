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
        Some(state)
    }
    pub(super) fn retire_input(&mut self, retired: &BTreeSet<ObjectRef>) {
        self.input_states
            .retain(|reference, _| !retired.contains(reference));
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
        if state == defaults {
            self.graph.input_states.remove(target);
        } else {
            self.graph.input_states.insert(target.clone(), state);
        }
        Ok(())
    }
}
