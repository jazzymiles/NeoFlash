//! AS2-only interaction preferences, publication settings and deterministic tab order.
use super::*;
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize)]
pub(super) struct As2Settings {
    pub quality: String,
    pub sound_buf_time: i32,
    pub focus_rect: bool,
    pub redraw_regions: bool,
    pub redraw_region_color: u32,
}
impl Default for As2Settings {
    fn default() -> Self {
        Self {
            quality: "HIGH".into(),
            sound_buf_time: 5,
            focus_rect: true,
            redraw_regions: false,
            redraw_region_color: 0xff0000,
        }
    }
}
impl As2Settings {
    fn high_quality(&self) -> i32 {
        match self.quality.as_str() {
            "BEST" => 2,
            "LOW" => 0,
            _ => 1,
        }
    }
    fn snapshot(&self) -> Value {
        json!({
            "quality": self.quality,
            "high_quality": self.high_quality(),
            "sound_buf_time": self.sound_buf_time,
            "focus_rect": self.focus_rect,
            "redraw_regions": self.redraw_regions,
            "redraw_region_color": self.redraw_region_color,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum SettingsCommand {
    Get,
    SetQuality {
        value: String,
    },
    SetHighQuality {
        value: f64,
    },
    ToggleHighQuality,
    SetSoundBufTime {
        value: f64,
    },
    SetFocusRect {
        value: bool,
    },
    ShowRedrawRegions {
        enable: bool,
        #[serde(default)]
        color: Option<u64>,
    },
}

#[derive(Clone, Debug, Serialize, Default)]
pub(super) struct As2Input {
    pub enabled: Option<bool>,
    pub focus_enabled: Option<bool>,
    pub tab_enabled: Option<bool>,
    pub tab_index: Option<i32>,
    pub tab_children: Option<bool>,
    pub track_as_menu: Option<bool>,
    pub focus_rect: Option<bool>,
    pub force_smoothing: Option<bool>,
    pub lock_root: Option<bool>,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Get,
    Set { name: String, value: Value },
    Focusable { has_button_handler: bool },
    TabOrder { button_targets: Vec<ObjectRef> },
    MenuTransfer { target: ObjectRef },
}
impl RuntimeGraph {
    pub(super) fn as2_input(&self, target: &ObjectRef) -> As2Input {
        self.as2_input_states
            .get(target)
            .cloned()
            .unwrap_or_default()
    }
}
impl Player {
    pub fn as2_root(&self, target: &ObjectRef) -> Result<ObjectRef, DisplayError> {
        if !self.graph.alive(target) {
            return Err(error("AS2 target is stale or foreign"));
        }
        for reference in self.graph.route(target) {
            if self.graph.as2_input(&reference).lock_root.unwrap_or(false) {
                return Ok(reference);
            }
        }
        self.object_ref(&[])
            .ok_or_else(|| error("Publication root is unavailable"))
    }
    pub fn as2_settings_command(&mut self, request: Value) -> Result<String, DisplayError> {
        let command: SettingsCommand = serde_json::from_value(request)
            .map_err(|e| error(&format!("Invalid AS2 settings request: {e}")))?;
        match command {
            SettingsCommand::Get => {}
            SettingsCommand::SetQuality { value } => {
                let normalized = value.to_ascii_uppercase();
                if matches!(normalized.as_str(), "LOW" | "MEDIUM" | "HIGH" | "BEST") {
                    self.graph.as2_settings.quality = normalized;
                }
            }
            SettingsCommand::SetHighQuality { value } => {
                if value.is_finite() && value >= 0.0 {
                    let level = value as i64;
                    self.graph.as2_settings.quality = match level {
                        0 => "LOW",
                        1 => "HIGH",
                        _ => "BEST",
                    }
                    .into();
                }
            }
            SettingsCommand::ToggleHighQuality => {
                self.graph.as2_settings.quality = if self.graph.as2_settings.quality == "LOW" {
                    "HIGH"
                } else {
                    "LOW"
                }
                .into();
            }
            SettingsCommand::SetSoundBufTime { value } => {
                if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                    return Err(error(
                        "AS2 _soundbuftime requires a finite 32-bit integer value",
                    ));
                }
                self.graph.as2_settings.sound_buf_time = value as i32;
            }
            SettingsCommand::SetFocusRect { value } => {
                self.graph.as2_settings.focus_rect = value;
            }
            SettingsCommand::ShowRedrawRegions { enable, color } => {
                if let Some(color) = color {
                    if color > 0xffffff {
                        return Err(error(
                            "showRedrawRegions color requires a 24-bit RGB integer",
                        ));
                    }
                    self.graph.as2_settings.redraw_region_color = color as u32;
                } else if enable {
                    self.graph.as2_settings.redraw_region_color = 0xff0000;
                }
                self.graph.as2_settings.redraw_regions = enable;
            }
        }
        serde_json::to_string(&self.graph.as2_settings.snapshot())
            .map_err(|e| error(&e.to_string()))
    }
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
                state.track_as_menu = Some(state.track_as_menu.unwrap_or(false));
                state.force_smoothing = Some(state.force_smoothing.unwrap_or(false));
                state.lock_root = Some(state.lock_root.unwrap_or(false));
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
                if name == "forceSmoothing" && node.display.owner != "movie_clip" {
                    return Err(error("forceSmoothing belongs to MovieClip"));
                }
                if name == "_lockroot" && node.display.owner != "movie_clip" {
                    return Err(error("_lockroot belongs to MovieClip"));
                }
                if name == "enabled" && node.display.owner == "text" {
                    return Err(error(
                        "AS2 TextField has no enabled switch; use type/selectable",
                    ));
                }
                if matches!(name.as_str(), "trackAsMenu" | "_focusrect")
                    && !matches!(node.display.owner.as_str(), "movie_clip" | "button")
                {
                    return Err(error(
                        "AS2 menu tracking and focus rectangles require MovieClip or Button",
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
                    "enabled" | "tabEnabled" | "focusEnabled" | "tabChildren" | "trackAsMenu"
                    | "_focusrect" | "forceSmoothing" | "_lockroot" => {
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
                            "trackAsMenu" => state.track_as_menu = value,
                            "_focusrect" => state.focus_rect = value,
                            "forceSmoothing" => state.force_smoothing = value,
                            "_lockroot" => state.lock_root = value,
                            _ => state.tab_children = value,
                        }
                    }
                    _ => return Err(error("Unknown AS2 input preference")),
                }
                self.graph.as2_input_states.insert(target.clone(), state);
                encode(&Value::Null)
            }
            Command::MenuTransfer { target: next } => {
                let allowed = self.graph.alive(&next)
                    && self.graph.as2_input(target).track_as_menu.unwrap_or(false)
                    && self.graph.as2_input(&next).track_as_menu.unwrap_or(false)
                    && self.graph.as2_input(&next).enabled.unwrap_or(true);
                encode(&Value::Bool(allowed))
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
