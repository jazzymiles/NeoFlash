//! Global tool preferences. Rail customization is deliberately excluded from workspace snapshots.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PencilMode {
    #[default]
    Straighten,
    Smooth,
    Ink,
}
impl PencilMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Straighten => "Straighten",
            Self::Smooth => "Smooth",
            Self::Ink => "Ink",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPreferences {
    /// Stable tool names; unknown or duplicate entries are removed on load.
    pub slots: Vec<Vec<String>>,
    pub pencil_mode: PencilMode,
    /// Per-tool strength, scaled by the Drawing preference tolerance (50 = preference value).
    pub pencil_smoothing: f64,
}
impl Default for ToolPreferences {
    fn default() -> Self {
        Self {
            slots: default_slots(),
            pencil_mode: PencilMode::Straighten,
            pencil_smoothing: 50.,
        }
    }
}
/// The approved Customize rail contains fourteen slots (R-038 outline).
pub const TOOL_SLOT_COUNT: usize = 14;
pub const TOOL_NAMES: [&str; 19] = [
    "Selection",
    "Subselection",
    "Free Transform",
    "Gradient Transform",
    "Lasso",
    "Pen",
    "Line",
    "Text",
    "Rectangle",
    "Oval",
    "PolyStar",
    "Pencil",
    "Brush",
    "Paint Bucket",
    "Ink Bottle",
    "Eyedropper",
    "Eraser",
    "Hand",
    "Zoom",
];
pub fn default_slots() -> Vec<Vec<String>> {
    [
        vec!["Selection", "Subselection"],
        vec!["Free Transform", "Gradient Transform"],
        vec!["Lasso"],
        vec!["Pen", "Line"],
        vec!["Text"],
        vec!["Rectangle", "Oval", "PolyStar"],
        vec!["Pencil", "Brush"],
        vec!["Paint Bucket", "Ink Bottle"],
        vec!["Eyedropper"],
        vec!["Eraser"],
        vec!["Hand", "Zoom"],
    ]
    .into_iter()
    .map(|slot| slot.into_iter().map(str::to_owned).collect())
    .collect()
}
impl ToolPreferences {
    pub fn sanitize(&mut self) {
        self.pencil_smoothing = if self.pencil_smoothing.is_finite() {
            self.pencil_smoothing.clamp(0., 100.)
        } else {
            50.
        };
        let mut used = std::collections::HashSet::new();
        self.slots.truncate(TOOL_SLOT_COUNT);
        for slot in &mut self.slots {
            slot.retain(|name| TOOL_NAMES.contains(&name.as_str()) && used.insert(name.clone()));
        }
        self.slots.retain(|slot| !slot.is_empty());
    }
}
