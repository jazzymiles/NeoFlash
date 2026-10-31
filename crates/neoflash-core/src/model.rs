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
    /// Portable library organization, including folders that are currently empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_folders: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<crate::ResourceUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_class: Option<String>,
    /// Ordered scene timelines (MovieClip symbol IDs) played one after another as
    /// the main timeline. Empty means one scene, `root`; otherwise the first scene
    /// is `root`. Scene symbols are not library items and are never instantiated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<Id>,
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
    /// Slash-separated logical library path; empty means the library root.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
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
    #[serde(default, skip_serializing_if = "LayerProperties::is_default")]
    pub properties: LayerProperties,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerRole {
    #[default]
    Normal,
    Guide,
    Mask,
    Folder,
}

/// Authoring relationships use document IDs, never row positions or UI paths.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerProperties {
    /// Exclusive authored exposure end; absent in legacy documents means the
    /// containing timeline's end. Zero is an empty layer, not a hidden key store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_frame: Option<u32>,
    #[serde(default)]
    pub role: LayerRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Id>,
    #[serde(default)]
    pub outline: bool,
    /// Authoring-only RGB; None uses the stable layer palette without changing
    /// older documents or published artwork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<[u8; 3]>,
    #[serde(default)]
    pub collapsed: bool,
    /// Authoring groups share selection/transforms without creating a runtime
    /// display object or changing the authored stacking order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<AuthoringGroup>,
    /// Guide layer (same timeline) whose first stroke is the motion path for
    /// classic tweens on this layer. The guide itself is never published artwork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_guide: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringGroup {
    pub id: Id,
    pub name: String,
    pub members: Vec<Id>,
}
impl Project {
    /// Remove obsolete authoring selection links after destructive edits. Artwork,
    /// runtime identities and scripts are never changed by this housekeeping.
    pub fn prune_authoring_groups(&mut self) {
        for layer in self.symbols.iter_mut().flat_map(|s| &mut s.timeline.layers) {
            let members: std::collections::HashSet<_> = layer
                .keyframes
                .iter()
                .flat_map(|k| &k.elements)
                .map(|e| e.id.as_str())
                .collect();
            for group in &mut layer.properties.groups {
                group.members.retain(|id| members.contains(id.as_str()));
            }
            layer
                .properties
                .groups
                .retain(|group| group.members.len() > 1);
        }
    }
}
impl Project {
    /// Document features introduced with format 6.
    pub fn uses_format_6(&self) -> bool {
        !self.scenes.is_empty()
            || self
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .any(|layer| {
                    layer.properties.motion_guide.is_some()
                        || layer
                            .keyframes
                            .iter()
                            .any(|k| !k.tween_kind.is_classic() || k.keyed_properties.is_some())
                })
    }

    /// Drop motion-guide links whose guide layer was deleted or is no longer a guide.
    pub fn prune_motion_guides(&mut self) {
        for symbol in &mut self.symbols {
            let guides: HashSet<Id> = symbol
                .timeline
                .layers
                .iter()
                .filter(|l| l.properties.role == LayerRole::Guide)
                .map(|l| l.id.clone())
                .collect();
            for layer in &mut symbol.timeline.layers {
                if layer.properties.role != LayerRole::Normal
                    || layer
                        .properties
                        .motion_guide
                        .as_ref()
                        .is_some_and(|id| !guides.contains(id))
                {
                    layer.properties.motion_guide = None;
                }
            }
        }
    }

    /// Raise the format version after an edit that uses a format 6 feature.
    pub fn upgrade_format_for_features(&mut self) {
        if self.uses_format_6() {
            self.format_version = self.format_version.max(6);
        }
    }

    /// Scene timelines in play order; a document without scenes has one, `root`.
    pub fn scene_ids(&self) -> Vec<Id> {
        if self.scenes.is_empty() {
            vec![self.root.clone()]
        } else {
            self.scenes.clone()
        }
    }

    pub fn is_scene(&self, id: &str) -> bool {
        id == self.root || self.scenes.iter().any(|scene| scene == id)
    }
}

impl LayerProperties {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Internal frame index is zero-based. The editor and goto API display one-based frames.
    pub frame: u32,
    pub elements: Vec<Element>,
    pub tween: Option<Easing>,
    pub label: String,
    /// How the span starting here interpolates when `tween` is set.
    #[serde(default, skip_serializing_if = "TweenKind::is_classic")]
    pub tween_kind: TweenKind,
    /// Property keyframe (◆) inside a motion tween span: the properties keyed at
    /// this frame. `None` is an ordinary keyframe that starts a new span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyed_properties: Option<Vec<TweenProperty>>,
}

/// Interpolation used by a tweened span.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TweenKind {
    /// Pose interpolation between two keyframes, matching objects by identity.
    #[default]
    Classic,
    /// Morph the layer's shapes between two keyframes, pairing shapes by order.
    Shape,
    /// Object tween with independent property tracks keyed by ◆ property keyframes.
    Motion,
}

impl TweenKind {
    pub fn is_classic(&self) -> bool {
        *self == Self::Classic
    }
}

/// One independently keyed track of a motion tween.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TweenProperty {
    X,
    Y,
    ScaleX,
    ScaleY,
    Rotation,
    Skew,
    Alpha,
    /// Color transform multipliers and offsets as one track.
    Color,
}

impl TweenProperty {
    pub const ALL: [Self; 8] = [
        Self::X,
        Self::Y,
        Self::ScaleX,
        Self::ScaleY,
        Self::Rotation,
        Self::Skew,
        Self::Alpha,
        Self::Color,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    /// Classic Flash acceleration: -100 is quadratic ease-in, +100 ease-out.
    Flash {
        acceleration: f64,
    },
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
    /// Angle of the y axis relative to the x-axis rotation, in degrees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skew: f64,
    #[serde(default, skip_serializing_if = "crate::ColorTransform::is_identity")]
    pub color_transform: crate::ColorTransform,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.
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
            skew: 0.,
            color_transform: crate::ColorTransform::default(),
        }
    }
}

impl Transform {
    /// Preserve both matrix axis angles, including reflection and shear.
    pub fn from_matrix(matrix: [f64; 6]) -> Option<Self> {
        if matrix
            .iter()
            .enumerate()
            .any(|(i, v)| !v.is_finite() || v.abs() > if i < 4 { 1e4 } else { 1e9 })
        {
            return None;
        }
        let [a, b, c, d, x, y] = matrix;
        let rotation = b.atan2(a);
        Some(Self {
            x,
            y,
            scale_x: a.hypot(b),
            scale_y: c.hypot(d),
            rotation: rotation.to_degrees(),
            skew: ((-c).atan2(d) - rotation).to_degrees(),
            ..Self::default()
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    /// Authored video viewport. Decoder/NetStream state belongs to the browser
    /// backend and is never embedded in the authoring document.
    Video { width: f64, height: f64 },
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
    /// Authored path paints share the runtime gradient representation.
    #[serde(rename = "gradient_path")]
    PaintedPath {
        commands: Vec<PathCommand>,
        #[serde(default)]
        fill_rule: FillRule,
        fill: Color,
        stroke: Color,
        fill_gradient: Option<crate::GradientPaint>,
        stroke_gradient: Option<crate::GradientPaint>,
        stroke_width: f64,
    },
    TextBox {
        text: String,
        width: f64,
        height: f64,
        format: crate::TextFormat,
        #[serde(default)]
        word_wrap: bool,
        #[serde(default)]
        multiline: bool,
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
    /// Parent-synchronized graphic playback starting at a zero-based frame.
    LoopFrom(u32),
    PlayOnce,
    PlayOnceFrom(u32),
    SingleFrame(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptLanguage {
    JavaScript,
    TypeScript,
    ActionScript1,
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
    /// Property keyframes left outside a motion tween by an edit become ordinary
    /// keyframes holding their stored pose (they then start their own span).
    pub fn normalize_motion_keys(&mut self) {
        self.keyframes.sort_by_key(|key| key.frame);
        let mut in_motion = false;
        for key in &mut self.keyframes {
            if key.keyed_properties.is_some() && !in_motion {
                key.keyed_properties = None;
            }
            if key.keyed_properties.is_none() {
                in_motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
            }
        }
    }
    pub fn key_at(&self, frame: u32) -> Option<&Keyframe> {
        if self.properties.end_frame.is_some_and(|end| frame >= end) {
            return None;
        }
        self.keyframes
            .iter()
            .filter(|k| k.frame <= frame)
            .max_by_key(|k| k.frame)
    }
    pub fn key_at_mut(&mut self, frame: u32) -> Option<&mut Keyframe> {
        if self.properties.end_frame.is_some_and(|end| frame >= end) {
            return None;
        }
        self.keyframes
            .iter_mut()
            .filter(|k| k.frame <= frame)
            .max_by_key(|k| k.frame)
    }
}
