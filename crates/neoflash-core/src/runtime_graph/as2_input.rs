//! AS2-only interaction preferences and deterministic tab order.
use super::*;
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Default)]
pub(super) struct As2Input {
    pub enabled: Option<bool>,
    pub focus_enabled: Option<bool>,
    pub tab_enabled: Option<bool>,
    pub tab_index: Option<i32>,
    pub tab_children: Option<bool>,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Get,
    Set { name: String, value: Value },
    Focusable { has_button_handler: bool },
    TabOrder { button_targets: Vec<ObjectRef> },
}
impl RuntimeGraph {
    fn as2_input(&self, target: &ObjectRef) -> As2Input {
        self.as2_input_states
            .get(target)
            .cloned()
            .unwrap_or_default()
    }
}
impl Player {
    pub fn as2_input_command(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        request: Value,
    ) -> Result<String, DisplayError> {
        if !self.graph.alive(target) {
            return Err(error("AS2 input target is stale or foreign"));
        }
        let command: Command = serde_json::from_value(request)
            .map_err(|e| error(&format!("Invalid AS2 input request: {e}")))?;
        let encode =
            |value: &Value| serde_json::to_string(value).map_err(|e| error(&e.to_string()));
        match command {
            Command::Get => {
                let mut state = self.graph.as2_input(target);
                state.enabled = Some(state.enabled.unwrap_or(true));
                state.tab_children = Some(state.tab_children.unwrap_or(true));
                serde_json::to_string(&state).map_err(|e| error(&e.to_string()))
            }
            Command::Set { name, value } => {
                let snapshot = self.runtime_snapshot(project);
                let node = snapshot
                    .objects
                    .iter()
                    .find(|node| &node.object_ref == target)
                    .ok_or_else(|| error("AS2 input target is unavailable"))?;
                if !matches!(
                    node.display.owner.as_str(),
                    "movie_clip" | "button" | "text"
                ) {
                    return Err(error(
                        "AS2 input preferences require MovieClip, Button or TextField",
                    ));
                }
                if name == "tabChildren" && node.display.owner == "text" {
                    return Err(error("TextField has no tabChildren"));
                }
                if name == "focusEnabled" && node.display.owner != "movie_clip" {
                    return Err(error("focusEnabled belongs to MovieClip"));
                }
                if name == "enabled" && node.display.owner == "text" {
                    return Err(error(
                        "AS2 TextField has no enabled switch; use type/selectable",
                    ));
                }
                let mut state = self.graph.as2_input(target);
                match name.as_str() {
                    "tabIndex" => {
                        state.tab_index = if value.is_null() {
                            None
                        } else {
                            Some(value.as_i64().filter(|n|(0..=i32::MAX as i64).contains(n)).ok_or_else(||error("AS2 tabIndex requires a nonnegative signed integer or undefined"))?as i32)
                        };
                    }
                    "enabled" | "tabEnabled" | "focusEnabled" | "tabChildren" => {
                        let value = if value.is_null() {
                            None
                        } else {
                            Some(value.as_bool().ok_or_else(|| {
                                error("AS2 input flags require Boolean or undefined")
                            })?)
                        };
                        match name.as_str() {
                            "enabled" => state.enabled = value,
                            "tabEnabled" => state.tab_enabled = value,
                            "focusEnabled" => state.focus_enabled = value,
                            _ => state.tab_children = value,
                        }
                    }
                    _ => return Err(error("Unknown AS2 input preference")),
                }
                self.graph.as2_input_states.insert(target.clone(), state);
                encode(&Value::Null)
            }
            Command::Focusable { has_button_handler } => {
                let scene = self.runtime_snapshot(project);
                let node = scene
                    .objects
                    .iter()
                    .find(|node| &node.object_ref == target)
                    .ok_or_else(|| error("AS2 focus target is absent"))?;
                let state = self.graph.as2_input(target);
                let accepted = node.on_stage
                    && node.display.effective_visible
                    && state.enabled.unwrap_or(true)
                    && match node.display.owner.as_str() {
                        "movie_clip" => state.focus_enabled.unwrap_or(false) || has_button_handler,
                        "button" => true,
                        "text" => node.text.as_ref().is_some_and(|field| {
                            field.selectable || field.field_type == TextFieldType::Input
                        }),
                        _ => false,
                    };
                encode(&Value::Bool(accepted))
            }
            Command::TabOrder { button_targets } => {
                if button_targets.len() > MAX_DYNAMIC_OBJECTS {
                    return Err(error("AS2 tab callback list exceeds 4096 targets"));
                }
                let buttons: BTreeSet<_> = button_targets.into_iter().collect();
                let scene = self.runtime_snapshot(project);
                let mut candidates: Vec<_> =
                    scene
                        .objects
                        .iter()
                        .enumerate()
                        .filter_map(|(order, node)| {
                            let state = self.graph.as2_input(&node.object_ref);
                            if !node.on_stage
                                || !node.display.effective_visible
                                || !state.enabled.unwrap_or(true)
                            {
                                return None;
                            }
                            let default = match node.display.owner.as_str() {
                                "button" => true,
                                "movie_clip" => buttons.contains(&node.object_ref),
                                "text" => node
                                    .text
                                    .as_ref()
                                    .is_some_and(|field| field.field_type == TextFieldType::Input),
                                _ => return None,
                            };
                            if !state.tab_enabled.unwrap_or(default) {
                                return None;
                            }
                            if self.graph.route(&node.object_ref).into_iter().skip(1).any(
                                |parent| {
                                    !self.graph.as2_input(&parent).tab_children.unwrap_or(true)
                                },
                            ) {
                                return None;
                            }
                            Some((node, state.tab_index, order))
                        })
                        .collect();
                if candidates.iter().any(|(_, index, _)| index.is_some()) {
                    candidates.retain(|(_, index, _)| index.is_some());
                    candidates.sort_by_key(|(_, index, order)| (index.unwrap_or(0), *order));
                } else {
                    candidates.sort_by(|(a, _, ao), (b, _, bo)| {
                        a.display.world[5]
                            .total_cmp(&b.display.world[5])
                            .then(a.display.world[4].total_cmp(&b.display.world[4]))
                            .then(ao.cmp(bo))
                    });
                }
                let references: Vec<_> = candidates
                    .into_iter()
                    .map(|(node, _, _)| node.object_ref.clone())
                    .collect();
                // Typed serialization retains canonical ObjectRef field order.
                serde_json::to_string(&references).map_err(|e| error(&e.to_string()))
            }
        }
    }
}
