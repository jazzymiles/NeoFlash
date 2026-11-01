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
    /// Authoring-only ruler guides of this timeline (document version 6). Never evaluated or
    /// published; HTML5 export strips them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
}

/// Direction of a ruler guide. A horizontal guide is a line at `y = position`, a vertical guide a
/// line at `x = position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideAxis {
    Horizontal,
    Vertical,
}

/// A ruler guide in timeline coordinates (stage pixels from the scene's top-left corner, or from a
/// symbol's registration point).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub axis: GuideAxis,
    pub position: f64,
}

impl Guide {
    /// Largest number of guides one timeline may hold.
    pub const LIMIT: usize = 1000;
    /// Largest absolute guide position in stage pixels.
    pub const MAX_POSITION: f64 = 1.0e6;
    /// First document version that may contain guides.
    pub const FORMAT_VERSION: u32 = 6;
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
    /// Document features introduced with format 6 (scenes, tween kinds, motion and ruler guides,
    /// frame sounds).
    pub fn uses_format_6(&self) -> bool {
        !self.scenes.is_empty()
            || self.symbols.iter().any(|s| !s.timeline.guides.is_empty())
            || self
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .any(|layer| {
                    layer.properties.motion_guide.is_some()
                        || layer.keyframes.iter().any(|k| {
                            !k.tween_kind.is_classic()
                                || k.keyed_properties.is_some()
                                || k.orient_to_path
                                || k.sound.is_some()
                        })
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

    /// Raise the format version after an edit that uses a format 6 or 7 feature.
    pub fn upgrade_format_for_features(&mut self) {
        if self.uses_format_6() {
            self.format_version = self.format_version.max(6);
        }
        if self.uses_format_7() {
            self.format_version = self.format_version.max(7);
        }
    }

    /// Document features introduced with format 7 (parametric Rectangle/Oval options and custom
    /// authoring transformation points).
    pub fn uses_format_7(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .any(|e| e.transform.pivot.is_some() || e.transform.primitive.is_some())
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
    /// Rotate tweened objects to follow the motion guide path.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub orient_to_path: bool,
    /// Timeline sound started (or stopped) when the playhead enters this keyframe
    /// (document version 6). The layer then shows as a Sound layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<FrameSound>,
}

/// A library audio resource attached to a keyframe (Flash frame sound).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameSound {
    /// `ResourceUnit` ID of an `audio/*` resource.
    pub resource: Id,
    #[serde(default, skip_serializing_if = "SoundSync::is_event")]
    pub sync: SoundSync,
    /// Number of plays (Flash "Repeat"); at least 1.
    #[serde(
        default = "FrameSound::one",
        skip_serializing_if = "FrameSound::is_one"
    )]
    pub loops: u32,
    /// Flash "Loop": repeat until stopped (Event/Start) or until the span ends (Stream).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub loop_forever: bool,
    #[serde(default, skip_serializing_if = "SoundEffect::is_none")]
    pub effect: SoundEffect,
}

impl FrameSound {
    /// Largest repeat count, as in Flash.
    pub const MAX_LOOPS: u32 = 10_000;

    pub fn new(resource: impl Into<Id>) -> Self {
        Self {
            resource: resource.into(),
            sync: SoundSync::Event,
            loops: 1,
            loop_forever: false,
            effect: SoundEffect::None,
        }
    }
    fn one() -> u32 {
        1
    }
    fn is_one(value: &u32) -> bool {
        *value == 1
    }
}

/// How a frame sound relates to the timeline (Flash "Sync").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundSync {
    /// Play to the end on every entry, overlapping earlier instances.
    #[default]
    Event,
    /// Like Event, but not while an instance of the same sound is still playing.
    Start,
    /// Stop every playing instance of the sound.
    Stop,
    /// Locked to the timeline: plays only while the playhead runs through the span.
    Stream,
}

impl SoundSync {
    pub const ALL: [Self; 4] = [Self::Event, Self::Start, Self::Stop, Self::Stream];
    pub fn is_event(&self) -> bool {
        *self == Self::Event
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Event => "Event",
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Stream => "Stream",
        }
    }
}

/// Flash preset volume envelopes of a frame sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundEffect {
    #[default]
    None,
    LeftChannel,
    RightChannel,
    FadeLeftToRight,
    FadeRightToLeft,
    FadeIn,
    FadeOut,
}

impl SoundEffect {
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::LeftChannel,
        Self::RightChannel,
        Self::FadeLeftToRight,
        Self::FadeRightToLeft,
        Self::FadeIn,
        Self::FadeOut,
    ];
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::LeftChannel => "Left channel",
            Self::RightChannel => "Right channel",
            Self::FadeLeftToRight => "Fade left to right",
            Self::FadeRightToLeft => "Fade right to left",
            Self::FadeIn => "Fade in",
            Self::FadeOut => "Fade out",
        }
    }
    /// Volume envelope as `(position, left, right)` points; `position` is the fraction
    /// 0..=1 of the played length, levels are 0..=1 and linear between points.
    pub fn envelope(self) -> &'static [(f64, f64, f64)] {
        match self {
            Self::None => &[(0.0, 1.0, 1.0)],
            Self::LeftChannel => &[(0.0, 1.0, 0.0)],
            Self::RightChannel => &[(0.0, 0.0, 1.0)],
            Self::FadeLeftToRight => &[(0.0, 1.0, 0.0), (1.0, 0.0, 1.0)],
            Self::FadeRightToLeft => &[(0.0, 0.0, 1.0), (1.0, 1.0, 0.0)],
            Self::FadeIn => &[(0.0, 0.0, 0.0), (1.0, 1.0, 1.0)],
            Self::FadeOut => &[(0.0, 1.0, 1.0), (1.0, 0.0, 0.0)],
        }
    }
    /// Left and right gain at `position` (0..=1) of the played length.
    pub fn gains_at(self, position: f64) -> (f64, f64) {
        let points: Vec<[f64; 3]> = self
            .envelope()
            .iter()
            .map(|&(position, left, right)| [position, left, right])
            .collect();
        crate::timeline_sound::envelope_gains(&points, position)
    }
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

impl Element {
    pub fn pivot(&self) -> Option<[f64; 2]> {
        self.transform.pivot
    }

    pub fn set_pivot(&mut self, pivot: Option<[f64; 2]>) {
        self.transform.pivot = pivot;
    }

    pub fn rectangle_options(&self) -> RectangleOptions {
        match self.transform.primitive {
            Some(PrimitiveOptions::Rectangle(options)) => options,
            _ => RectangleOptions::default(),
        }
    }

    pub fn set_rectangle_options(&mut self, options: RectangleOptions) {
        self.transform.primitive = (!options.is_default()).then_some(PrimitiveOptions::Rectangle(options));
    }

    pub fn oval_options(&self) -> OvalOptions {
        match self.transform.primitive {
            Some(PrimitiveOptions::Oval(options)) => options,
            _ => OvalOptions::default(),
        }
    }

    pub fn set_oval_options(&mut self, options: OvalOptions) {
        self.transform.primitive = (!options.is_default()).then_some(PrimitiveOptions::Oval(options));
    }

    /// True when this element is a `Rectangle` or `Ellipse` with active parametric geometry.
    pub fn has_parametric_options(&self) -> bool {
        match &self.kind {
            ElementKind::Rectangle { .. } => self.rectangle_options().has_rounded_corners(),
            ElementKind::Ellipse { .. } => !self.oval_options().is_default(),
            _ => false,
        }
    }

    /// Local vector contour and fill rule for `Rectangle`, `Ellipse`, `Path` or `PaintedPath`.
    pub fn to_path_commands(&self) -> Option<(Vec<PathCommand>, FillRule)> {
        match &self.kind {
            ElementKind::Rectangle { width, height, .. } => {
                if *width <= 0.0 || *height <= 0.0 {
                    return None;
                }
                Some((
                    crate::geometry::rectangle_path_commands(
                        *width,
                        *height,
                        self.rectangle_options(),
                    ),
                    FillRule::NonZero,
                ))
            }
            ElementKind::Ellipse { width, height, .. } => {
                if *width <= 0.0 || *height <= 0.0 {
                    return None;
                }
                Some((
                    crate::geometry::oval_path_commands(*width, *height, self.oval_options()),
                    FillRule::NonZero,
                ))
            }
            ElementKind::Path {
                commands,
                fill_rule,
                ..
            }
            | ElementKind::PaintedPath {
                commands,
                fill_rule,
                ..
            } => Some((commands.clone(), *fill_rule)),
            _ => None,
        }
    }

    /// Evaluated `ElementKind::Path` when a `Rectangle` or `Ellipse` uses parametric options;
    /// `None` for plain shapes so existing `Rectangle`/`Ellipse` rendering stays unchanged.
    pub fn parametric_path_kind(&self) -> Option<ElementKind> {
        if !self.has_parametric_options() {
            return None;
        }
        let (commands, fill_rule) = self.to_path_commands()?;
        match &self.kind {
            ElementKind::Rectangle {
                fill,
                stroke,
                stroke_width,
                ..
            } => Some(ElementKind::Path {
                commands,
                fill_rule,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
            }),
            ElementKind::Ellipse {
                fill,
                stroke,
                stroke_width,
                ..
            } => {
                let close_path = self.oval_options().close_path;
                Some(ElementKind::Path {
                    commands,
                    fill_rule,
                    fill: if close_path { *fill } else { [0, 0, 0, 0] },
                    stroke: *stroke,
                    stroke_width: *stroke_width,
                })
            }
            _ => None,
        }
    }
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

/// Parametric options for a `Rectangle` primitive: corner radii `[top_left, top_right,
/// bottom_left, bottom_right]` matching the NF2 `↖`, `↗`, `↙`, `↘` fields, plus corner linking.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RectangleOptions {
    #[serde(default, skip_serializing_if = "is_zero_corners")]
    pub corner_radii: [f64; 4],
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub corners_locked: bool,
}

impl Default for RectangleOptions {
    fn default() -> Self {
        Self {
            corner_radii: [0.0; 4],
            corners_locked: true,
        }
    }
}

impl RectangleOptions {
    pub fn is_default(&self) -> bool {
        self.corner_radii == [0.0; 4] && self.corners_locked
    }
    pub fn has_rounded_corners(&self) -> bool {
        self.corner_radii.iter().any(|r| *r > 0.0)
    }
}

/// Parametric options for an `Ellipse` (Oval) primitive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OvalOptions {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start_angle: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub end_angle: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inner_radius: f64,
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub close_path: bool,
}

impl Default for OvalOptions {
    fn default() -> Self {
        Self {
            start_angle: 0.0,
            end_angle: 0.0,
            inner_radius: 0.0,
            close_path: true,
        }
    }
}

impl OvalOptions {
    pub fn is_default(&self) -> bool {
        self.start_angle == 0.0
            && self.end_angle == 0.0
            && self.inner_radius == 0.0
            && self.close_path
    }
    pub fn is_partial_sweep(&self) -> bool {
        let sweep = (self.end_angle - self.start_angle).rem_euclid(360.0);
        sweep > 1e-6 && (360.0 - sweep) > 1e-6
    }
}

/// Format-7 parametric shape options attached to a `Rectangle` or `Ellipse` placement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrimitiveOptions {
    Rectangle(RectangleOptions),
    Oval(OvalOptions),
}

impl PrimitiveOptions {
    pub fn is_default(&self) -> bool {
        match self {
            Self::Rectangle(r) => r.is_default(),
            Self::Oval(o) => o.is_default(),
        }
    }
}

fn is_zero_corners(corners: &[f64; 4]) -> bool {
    *corners == [0.0; 4]
}
const fn bool_true() -> bool {
    true
}
fn is_true(value: &bool) -> bool {
    *value
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
    /// Custom authoring transformation point in the element's local space (format 7).
    /// `None` uses the element's local geometric center.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<[f64; 2]>,
    /// Parametric options for `Rectangle` and `Ellipse` primitives (format 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primitive: Option<PrimitiveOptions>,
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
            pivot: None,
            primitive: None,
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
