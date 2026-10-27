use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub type Color = [u8; 4];
pub type Id = String;

/// Compound-path filling, matching the Canvas/SVG spellings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillRule {
    #[default]
    #[serde(rename = "nonzero")]
    NonZero,
    #[serde(rename = "evenodd")]
    EvenOdd,
}

/// Absolute coordinates in the element's local space. Each contour begins with
/// MoveTo; drawing after Close requires another MoveTo. Open contours are
/// implicitly closed for filling, but remain open when stroked.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PathCommand {
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    QuadraticTo {
        cx: f64,
        cy: f64,
        x: f64,
        y: f64,
    },
    CubicTo {
        cx1: f64,
        cy1: f64,
        cx2: f64,
        cy2: f64,
        x: f64,
        y: f64,
    },
    Close,
}

/// Only used for editor-created entities; serialized identity survives round trips.
pub fn new_id(prefix: &str) -> Id {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let mut used = allocated_ids().lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let id = format!("{prefix}-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        if used.insert(id.clone()) {
            return id;
        }
    }
}

fn allocated_ids() -> &'static Mutex<HashSet<Id>> {
    static USED: OnceLock<Mutex<HashSet<Id>>> = OnceLock::new();
    USED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Register imported identities so future editor allocations cannot collide.
pub(crate) fn reserve_ids(project: &Project) {
    let mut used = allocated_ids().lock().unwrap_or_else(|e| e.into_inner());
    for source in &project.sources {
        used.insert(source.id.clone());
    }
    for resource in &project.resources {
        used.insert(resource.id.clone());
    }
    for symbol in &project.symbols {
        used.insert(symbol.id.clone());
        for layer in &symbol.timeline.layers {
            used.insert(layer.id.clone());
            for key in &layer.keyframes {
                for element in &key.elements {
                    used.insert(element.id.clone());
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: Color,
    pub root: Id,
    pub symbols: Vec<Symbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<crate::ResourceUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_class: Option<String>,
}

/// A logical project file. Compilation resolves paths within this snapshot and
/// never reads arbitrary filesystem paths from authored source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceUnit {
    pub id: Id,
    pub path: String,
    pub language: ScriptLanguage,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: Id,
    pub name: String,
    pub kind: SymbolKind,
    pub timeline: Timeline,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Explicit runtime export identifier, independent of the library name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linkage_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    MovieClip,
    Graphic,
    Button,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub frame_count: u32,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub scripts: Vec<FrameScript>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub keyframes: Vec<Keyframe>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Internal frame index is zero-based. The editor and goto API display one-based frames.
    pub frame: u32,
    pub elements: Vec<Element>,
    pub tween: Option<Easing>,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: Id,
    pub name: String,
    pub transform: Transform,
    pub kind: ElementKind,
    #[serde(default)]
    pub scripts: Vec<ObjectScript>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessibility: Option<Accessibility>,
}

/// Authored semantics are attached to a placement, including symbol instances.
/// Hosts derive current visibility and lifetime from the shared runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accessibility {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub role: AccessibleRole,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessibleRole {
    Button,
    Image,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation: f64,
    pub alpha: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            scale_x: 1.,
            scale_y: 1.,
            rotation: 0.,
            alpha: 1.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    Bitmap {
        resource_id: Id,
        width: u32,
        height: u32,
        #[serde(default)]
        smoothing: bool,
    },
    Rectangle {
        width: f64,
        height: f64,
        fill: Color,
        stroke: Color,
        stroke_width: f64,
    },
    Ellipse {
        width: f64,
        height: f64,
        fill: Color,
        stroke: Color,
        stroke_width: f64,
    },
    /// Compound vector geometry. Initial stroke semantics are round caps/joins.
    Path {
        commands: Vec<PathCommand>,
        #[serde(default)]
        fill_rule: FillRule,
        fill: Color,
        stroke: Color,
        stroke_width: f64,
    },
    Text {
        text: String,
        font_family: String,
        font_size: f64,
        color: Color,
    },
    Instance {
        symbol_id: Id,
        playback: InstancePlayback,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstancePlayback {
    Independent,
    Loop,
    PlayOnce,
    SingleFrame(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptLanguage {
    JavaScript,
    TypeScript,
    ActionScript2,
    ActionScript3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub language: ScriptLanguage,
    pub source: String,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            language: ScriptLanguage::JavaScript,
            source: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameScript {
    pub frame: u32,
    pub script: Script,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectEvent {
    Load,
    Click,
    EnterFrame,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectScript {
    pub event: ObjectEvent,
    pub script: Script,
}

impl Project {
    pub fn has_program(&self) -> bool {
        !self.sources.is_empty()
            || self.document_class.is_some()
            || self
                .symbols
                .iter()
                .any(|symbol| symbol.class_name.is_some() || symbol.linkage_id.is_some())
    }

    /// Old documents remain version 1 until project-level code is authored.
    pub fn enable_program(&mut self) {
        self.format_version = self.format_version.max(2);
    }

    pub fn symbol(&self, id: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.id == id)
    }
    pub fn symbol_mut(&mut self, id: &str) -> Option<&mut Symbol> {
        self.symbols.iter_mut().find(|s| s.id == id)
    }
}

impl Layer {
    pub fn key_at(&self, frame: u32) -> Option<&Keyframe> {
        self.keyframes
            .iter()
            .filter(|k| k.frame <= frame)
            .max_by_key(|k| k.frame)
    }
    pub fn key_at_mut(&mut self, frame: u32) -> Option<&mut Keyframe> {
        self.keyframes
            .iter_mut()
            .filter(|k| k.frame <= frame)
            .max_by_key(|k| k.frame)
    }
}
