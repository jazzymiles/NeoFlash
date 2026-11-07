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

/// Geometric masks preserve legacy fill silhouettes. Alpha masks composite the
/// source's painted opacity, including gradients and decoded bitmap pixels.
/// Luminance masks weight coverage by BT.709 relative luminance (`0.2126 R + 0.7152 G + 0.0722 B`)
/// multiplied by painted opacity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    #[default]
    Geometry,
    Alpha,
    Luminance,
}
impl MaskMode {
    pub const ALL: [Self; 3] = [Self::Geometry, Self::Alpha, Self::Luminance];

    pub fn is_geometry(&self) -> bool {
        *self == Self::Geometry
    }

    pub fn uses_paint_coverage(&self) -> bool {
        matches!(self, Self::Alpha | Self::Luminance)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Geometry => "Geometry",
            Self::Alpha => "Alpha",
            Self::Luminance => "Luminance",
        }
    }
}

/// Authoring relationships use document IDs, never row positions or UI paths.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// Applies to a Mask layer's source, never to its linked target layers.
    #[serde(default, skip_serializing_if = "MaskMode::is_geometry")]
    pub mask_mode: MaskMode,
    /// When true on a Mask layer, target layers are visible outside the mask coverage
    /// (`1 - coverage`) instead of inside (`coverage`), matching After Effects Inverted masks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mask_invert: bool,
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
    /// Layer-level blend mode (AE / Animate CC style). Elements on this layer with
    /// `BlendMode::Normal` inherit the layer's blend mode during evaluation.
    #[serde(default, skip_serializing_if = "BlendMode::is_normal")]
    pub blend_mode: BlendMode,
    /// When enabled, symbol instances on this layer synchronize their internal timeline to the
    /// parent timeline frame (`0`-based `frame` + `First frame` offset) rather than resetting at
    /// each placement span start — matching After Effects layer cropping / trim behavior.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sync_timeline: bool,
    /// Optional 9-slice scaling grid `[x, y, width, height]` in symbol coordinates (format 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale9_grid: Option<[f64; 4]>,
}

impl Symbol {
    /// 9-slice scaling grid `[x, y, width, height]` stored on this symbol's primary layer.
    pub fn scale9_grid(&self) -> Option<[f64; 4]> {
        self.timeline
            .layers
            .iter()
            .find_map(|l| l.properties.scale9_grid)
    }

    /// Set or clear the 9-slice scaling grid `[x, y, width, height]` on this symbol.
    pub fn set_scale9_grid(&mut self, grid: Option<[f64; 4]>) {
        for layer in &mut self.timeline.layers {
            layer.properties.scale9_grid = None;
        }
        if let Some(first) = self.timeline.layers.first_mut() {
            first.properties.scale9_grid = grid;
        }
    }
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

    /// Raise the format version after an edit that uses a newer document feature.
    pub fn upgrade_format_for_features(&mut self) {
        if self.uses_format_6() {
            self.format_version = self.format_version.max(6);
        }
        if self.uses_format_7() {
            self.format_version = self.format_version.max(7);
        }
        if self.uses_format_8() {
            self.format_version = self.format_version.max(8);
        }
        if self.uses_format_9() {
            self.format_version = self.format_version.max(9);
        }
    }

    /// Explicit painted-alpha/luminance/inverted timeline masks, per-resource audio/bitmap export
    /// settings, classic tween rotation/scale controls, roving keyframes, 9-slice guides,
    /// variable-width stroke profiles, bitmap fills, and instance property overrides (format 9).
    pub fn uses_format_9(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .any(|layer| {
                !layer.properties.mask_mode.is_geometry()
                    || layer.properties.mask_invert
                    || layer.properties.scale9_grid.is_some()
                    || layer.keyframes.iter().any(|k| {
                        k.classic_rotate_turns > 0
                            || !k.classic_rotate_dir.is_default()
                            || !k.tween_scale
                            || k.roving
                            || !k.instance_overrides.is_empty()
                            || !k.bitmap_fills.is_empty()
                            || k.elements.iter().any(|e| {
                                matches!(
                                    e.transform.primitive,
                                    Some(
                                        PrimitiveOptions::Scale9Grid(_)
                                            | PrimitiveOptions::StrokeProfile(_)
                                    )
                                )
                            })
                    })
            })
            || self.resources.iter().any(|r| {
                r.audio_export.as_ref().is_some_and(|e| !e.is_default())
                    || r.bitmap_export.as_ref().is_some_and(|e| !e.is_default())
            })
    }

    /// Editable spatial motion curves, classic guide endpoint snapping, layer blend modes and
    /// timeline-synchronized layers (format 8).
    pub fn uses_format_8(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .any(|l| {
                !l.properties.blend_mode.is_normal()
                    || l.properties.sync_timeline
                    || l.keyframes.iter().any(|k| {
                        k.motion_path.is_some()
                            || (k.snap_to_guide && k.tween_kind == TweenKind::Classic)
                    })
            })
    }

    /// Document features introduced with format 7 (parametric Rectangle/Oval options, custom
    /// authoring transformation points, Bounce/Elastic and per-property eases, motion sync/snap
    /// options, custom stereo sound envelopes or trim points, and authored display
    /// filters/blending/visibility).
    pub fn uses_format_7(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .any(|k| {
                matches!(k.tween, Some(Easing::Bounce | Easing::Elastic))
                    || !k.property_eases.is_empty()
                    || !k.shape_hints.is_empty()
                    || k.sync_symbols
                    || k.snap_to_guide
                    || k.sound.as_ref().is_some_and(|s| {
                        s.effect == SoundEffect::Custom
                            || s.envelope.is_some()
                            || s.in_point.is_some()
                            || s.out_point.is_some()
                    })
                    || k.elements.iter().any(|e| {
                        e.transform.pivot.is_some()
                            || e.transform.primitive.is_some()
                            || !e.visible
                            || e.cache_as_bitmap
                            || !e.blend_mode.is_normal()
                            || !e.filters.is_empty()
                            || e.merge
                    })
            })
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

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassicRotateDirection {
    #[default]
    Auto,
    None,
    Cw,
    Ccw,
}

impl ClassicRotateDirection {
    pub const ALL: [Self; 4] = [Self::Auto, Self::None, Self::Cw, Self::Ccw];

    pub fn is_default(&self) -> bool {
        matches!(self, Self::Auto)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::None => "None",
            Self::Cw => "CW",
            Self::Ccw => "CCW",
        }
    }
}

/// Per-instance override of a child element inside a Symbol instance on a Keyframe (format 9).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstancePropertyOverride {
    /// ID or name of the Symbol instance element on this keyframe.
    pub instance_id: Id,
    /// Name of the target child element inside the referenced symbol.
    pub target_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
}

/// Authored bitmap fill with an affine FillTransform matrix attached to a shape element (format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthoredBitmapFill {
    pub element_id: Id,
    pub resource_id: Id,
    /// Affine matrix `[a, b, c, d, tx, ty]` mapping bitmap pixel coordinates to local path space.
    pub matrix: [f64; 6],
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub repeat: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smoothing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Per-property easing overrides on a motion tween start keyframe (document version 7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub property_eases: Vec<(TweenProperty, Easing)>,
    /// Open spatial curve of a motion span, in timeline coordinates (format 8).
    /// The first element supplies the reference position; other elements keep their own
    /// track offsets. MoveTo followed by LineTo/CubicTo; position keys control timing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_path: Option<Vec<PathCommand>>,
    /// Keep nested graphic symbol animations synchronized across this tween span
    /// (document version 7).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sync_symbols: bool,
    /// Snap this tween's path to its layer's motion guide (motion: version 7;
    /// classic endpoint snapping: version 8).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snap_to_guide: bool,
    /// Timeline sound started (or stopped) when the playhead enters this keyframe
    /// (document version 6). The layer then shows as a Sound layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<FrameSound>,
    /// Authoring correspondence hints for a shape tween span starting here (document version 7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_hints: Vec<ShapeHint>,
    /// Additional full rotations added during a classic tween span (format 9).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub classic_rotate_turns: u32,
    /// Rotation interpolation mode for a classic tween span (format 9).
    #[serde(default, skip_serializing_if = "ClassicRotateDirection::is_default")]
    pub classic_rotate_dir: ClassicRotateDirection,
    /// Whether a classic tween interpolates scale and dimensions (default `true`, format 9).
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub tween_scale: bool,
    /// Spatial roving keyframe inside a motion tween span: timing is distributed
    /// proportionally to arc length along the motion path (format 9).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub roving: bool,
    /// Master/Essential property overrides for symbol instances at this keyframe (format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_overrides: Vec<InstancePropertyOverride>,
    /// Authored bitmap fills for shapes on this keyframe (format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bitmap_fills: Vec<AuthoredBitmapFill>,
}

impl Default for Keyframe {
    fn default() -> Self {
        Self {
            frame: 0,
            elements: Vec::new(),
            tween: None,
            label: String::new(),
            tween_kind: TweenKind::Classic,
            keyed_properties: None,
            orient_to_path: false,
            property_eases: Vec::new(),
            motion_path: None,
            sync_symbols: false,
            snap_to_guide: false,
            sound: None,
            shape_hints: Vec::new(),
            classic_rotate_turns: 0,
            classic_rotate_dir: ClassicRotateDirection::Auto,
            tween_scale: true,
            roving: false,
            instance_overrides: Vec::new(),
            bitmap_fills: Vec::new(),
        }
    }
}

impl Keyframe {
    pub const MAX_ROTATE_TURNS: u32 = 1000;

    pub fn bitmap_fill(&self, element_id: &str) -> Option<&AuthoredBitmapFill> {
        self.bitmap_fills
            .iter()
            .find(|bf| bf.element_id == element_id)
    }

    pub fn set_bitmap_fill(&mut self, fill: AuthoredBitmapFill) {
        if let Some(slot) = self
            .bitmap_fills
            .iter_mut()
            .find(|bf| bf.element_id == fill.element_id)
        {
            *slot = fill;
        } else {
            self.bitmap_fills.push(fill);
        }
    }

    pub fn clear_bitmap_fill(&mut self, element_id: &str) {
        self.bitmap_fills.retain(|bf| bf.element_id != element_id);
    }

    pub fn instance_override(
        &self,
        instance_id: &str,
        target_name: &str,
    ) -> Option<&InstancePropertyOverride> {
        self.instance_overrides
            .iter()
            .find(|ov| ov.instance_id == instance_id && ov.target_name == target_name)
    }

    pub fn set_instance_override(&mut self, ov: InstancePropertyOverride) {
        if let Some(slot) = self
            .instance_overrides
            .iter_mut()
            .find(|item| item.instance_id == ov.instance_id && item.target_name == ov.target_name)
        {
            *slot = ov;
        } else {
            self.instance_overrides.push(ov);
        }
    }
}

/// Authoring correspondence marker for a shape tween span (document version 7).
/// Maps a normalized `[0.0..=1.0, 0.0..=1.0]` UV position on the start shape's bounds
/// to a normalized `[0.0..=1.0, 0.0..=1.0]` UV position on the end shape's bounds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeHint {
    pub label: char,
    pub start_uv: [f64; 2],
    pub end_uv: [f64; 2],
}

impl ShapeHint {
    pub const MAX_HINTS: usize = 26;
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
    /// Custom stereo volume envelope points `[position, left, right]` (`0.0..=1.0`, format 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Vec<[f64; 3]>>,
    /// Playback trim start in seconds within the audio resource (format 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_point: Option<f64>,
    /// Playback trim end in seconds within the audio resource (format 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_point: Option<f64>,
}

impl FrameSound {
    /// Largest repeat count, as in Flash.
    pub const MAX_LOOPS: u32 = 10_000;
    /// Largest number of custom stereo envelope control points.
    pub const MAX_ENVELOPE_POINTS: usize = 32;
    /// Largest frame-sound trim timestamp in seconds (24 hours).
    pub const MAX_SOUND_TRIM_SECONDS: f64 = 86_400.0;

    pub fn new(resource: impl Into<Id>) -> Self {
        Self {
            resource: resource.into(),
            sync: SoundSync::Event,
            loops: 1,
            loop_forever: false,
            effect: SoundEffect::None,
            envelope: None,
            in_point: None,
            out_point: None,
        }
    }
    /// Effective stereo volume envelope as `[position, left, right]` points (`0.0..=1.0`).
    pub fn envelope_points(&self) -> Vec<[f64; 3]> {
        if let Some(points) = &self.envelope
            && !points.is_empty()
        {
            return points.clone();
        }
        self.effect
            .envelope()
            .iter()
            .map(|&(position, left, right)| [position, left, right])
            .collect()
    }
    /// Left and right gain at `position` (`0.0..=1.0`) of the played length.
    pub fn gains_at(&self, position: f64) -> (f64, f64) {
        crate::timeline_sound::envelope_gains(&self.envelope_points(), position)
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
    Custom,
}

impl SoundEffect {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::LeftChannel,
        Self::RightChannel,
        Self::FadeLeftToRight,
        Self::FadeRightToLeft,
        Self::FadeIn,
        Self::FadeOut,
        Self::Custom,
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
            Self::Custom => "Custom",
        }
    }
    /// Volume envelope as `(position, left, right)` points; `position` is the fraction
    /// 0..=1 of the played length, levels are 0..=1 and linear between points.
    pub fn envelope(self) -> &'static [(f64, f64, f64)] {
        match self {
            Self::None | Self::Custom => &[(0.0, 1.0, 1.0)],
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
    Bounce,
    Elastic,
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

/// Display-object compositing blend mode (Flash 8+ authoring set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Layer,
    Multiply,
    Screen,
    Lighten,
    Darken,
    Difference,
    Add,
    Subtract,
    Invert,
    Alpha,
    Erase,
    Overlay,
    Hardlight,
}

impl BlendMode {
    pub const ALL: [Self; 14] = [
        Self::Normal,
        Self::Layer,
        Self::Multiply,
        Self::Screen,
        Self::Lighten,
        Self::Darken,
        Self::Difference,
        Self::Add,
        Self::Subtract,
        Self::Invert,
        Self::Alpha,
        Self::Erase,
        Self::Overlay,
        Self::Hardlight,
    ];

    pub fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Layer => "layer",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Lighten => "lighten",
            Self::Darken => "darken",
            Self::Difference => "difference",
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Invert => "invert",
            Self::Alpha => "alpha",
            Self::Erase => "erase",
            Self::Overlay => "overlay",
            Self::Hardlight => "hardlight",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Layer => "Layer",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Lighten => "Lighten",
            Self::Darken => "Darken",
            Self::Difference => "Difference",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Invert => "Invert",
            Self::Alpha => "Alpha",
            Self::Erase => "Erase",
            Self::Overlay => "Overlay",
            Self::Hardlight => "Hard Light",
        }
    }

    /// True when the HTML5 Canvas 2D compositing profile supports this mode natively.
    pub fn has_canvas_compositing(self) -> bool {
        !matches!(
            self,
            Self::Subtract | Self::Invert | Self::Alpha | Self::Erase
        )
    }

    /// Standardized Canvas 2D `globalCompositeOperation` string for this blend mode.
    pub fn canvas_operation(self) -> &'static str {
        match self {
            Self::Normal
            | Self::Layer
            | Self::Subtract
            | Self::Invert
            | Self::Alpha
            | Self::Erase => "source-over",
            Self::Add => "lighter",
            Self::Hardlight => "hard-light",
            other => other.as_str(),
        }
    }
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
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache_as_bitmap: bool,
    #[serde(default, skip_serializing_if = "BlendMode::is_normal")]
    pub blend_mode: BlendMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<crate::FilterSpec>,
    /// True when this element is a raw Merge Drawing shape rather than a self-contained
    /// Object Drawing shape (document version 7).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge: bool,
}

impl Default for Element {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Path {
                commands: Vec::new(),
                fill_rule: FillRule::NonZero,
                fill: [0; 4],
                stroke: [0; 4],
                stroke_width: 0.0,
            },
            scripts: Vec::new(),
            accessibility: None,
            visible: true,
            cache_as_bitmap: false,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            merge: false,
        }
    }
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
        self.transform.primitive =
            (!options.is_default()).then_some(PrimitiveOptions::Rectangle(options));
    }

    pub fn oval_options(&self) -> OvalOptions {
        match self.transform.primitive {
            Some(PrimitiveOptions::Oval(options)) => options,
            _ => OvalOptions::default(),
        }
    }

    pub fn set_oval_options(&mut self, options: OvalOptions) {
        self.transform.primitive =
            (!options.is_default()).then_some(PrimitiveOptions::Oval(options));
    }

    pub fn scale9_grid(&self) -> Option<[f64; 4]> {
        match self.transform.primitive {
            Some(PrimitiveOptions::Scale9Grid(grid)) => Some(grid),
            _ => None,
        }
    }

    pub fn set_scale9_grid(&mut self, grid: Option<[f64; 4]>) {
        self.transform.primitive = grid.map(PrimitiveOptions::Scale9Grid);
    }

    pub fn stroke_width_profile(&self) -> StrokeWidthProfile {
        match self.transform.primitive {
            Some(PrimitiveOptions::StrokeProfile(profile)) => profile,
            _ => StrokeWidthProfile::default(),
        }
    }

    pub fn set_stroke_width_profile(&mut self, profile: StrokeWidthProfile) {
        self.transform.primitive =
            (!profile.is_default()).then_some(PrimitiveOptions::StrokeProfile(profile));
    }

    /// True when this element is a `Rectangle`, `Ellipse`, or stroked `Path` with active parametric geometry.
    pub fn has_parametric_options(&self) -> bool {
        match &self.kind {
            ElementKind::Rectangle { .. } => self.rectangle_options().has_rounded_corners(),
            ElementKind::Ellipse { .. } => !self.oval_options().is_default(),
            ElementKind::Path {
                fill, stroke_width, ..
            }
            | ElementKind::PaintedPath {
                fill, stroke_width, ..
            } => {
                *stroke_width > 0.0
                    && *fill == [0, 0, 0, 0]
                    && !self.stroke_width_profile().is_default()
            }
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
            ElementKind::Path {
                stroke,
                stroke_width,
                ..
            } => {
                let profile = self.stroke_width_profile();
                let expanded = crate::geometry::expand_variable_width_stroke(
                    &commands,
                    *stroke_width,
                    &profile,
                );
                if expanded.is_empty() {
                    None
                } else {
                    Some(ElementKind::Path {
                        commands: expanded,
                        fill_rule: FillRule::NonZero,
                        fill: *stroke,
                        stroke: [0, 0, 0, 0],
                        stroke_width: 0.0,
                    })
                }
            }
            ElementKind::PaintedPath {
                stroke,
                stroke_gradient,
                stroke_width,
                ..
            } => {
                let profile = self.stroke_width_profile();
                let expanded = crate::geometry::expand_variable_width_stroke(
                    &commands,
                    *stroke_width,
                    &profile,
                );
                if expanded.is_empty() {
                    None
                } else {
                    Some(ElementKind::PaintedPath {
                        commands: expanded,
                        fill_rule: FillRule::NonZero,
                        fill: *stroke,
                        stroke: [0, 0, 0, 0],
                        fill_gradient: stroke_gradient.clone(),
                        stroke_gradient: None,
                        stroke_width: 0.0,
                    })
                }
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

/// Preset variable-width stroke profiles (Flash CS5.5+ / Animate width profiles).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeProfilePreset {
    #[default]
    Uniform,
    TaperBoth,
    TaperStart,
    TaperEnd,
    Wave,
    Bell,
    Custom,
}

impl StrokeProfilePreset {
    pub const ALL: [Self; 7] = [
        Self::Uniform,
        Self::TaperBoth,
        Self::TaperStart,
        Self::TaperEnd,
        Self::Wave,
        Self::Bell,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Uniform => "Uniform",
            Self::TaperBoth => "Width Profile 1 (Taper Both)",
            Self::TaperStart => "Width Profile 2 (Taper Start)",
            Self::TaperEnd => "Width Profile 3 (Taper End)",
            Self::Wave => "Width Profile 4 (Wave)",
            Self::Bell => "Width Profile 5 (Bell)",
            Self::Custom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrokeWidthStop {
    pub offset: f64,
    pub scale: f64,
}

/// Variable-width stroke profile with up to 8 control stops in `[0.0..=1.0]` along the stroke.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeWidthProfile {
    #[serde(default)]
    pub preset: StrokeProfilePreset,
    #[serde(default)]
    pub stop_count: u8,
    #[serde(default)]
    pub stops: [StrokeWidthStop; 8],
}

impl Default for StrokeWidthProfile {
    fn default() -> Self {
        Self {
            preset: StrokeProfilePreset::Uniform,
            stop_count: 0,
            stops: [StrokeWidthStop::default(); 8],
        }
    }
}

impl StrokeWidthProfile {
    pub fn from_preset(preset: StrokeProfilePreset) -> Self {
        Self {
            preset,
            stop_count: 0,
            stops: [StrokeWidthStop::default(); 8],
        }
    }

    pub fn from_stops(stops: &[(f64, f64)]) -> Self {
        let mut arr = [StrokeWidthStop::default(); 8];
        let count = stops.len().min(8);
        for (i, &(offset, scale)) in stops.iter().take(count).enumerate() {
            arr[i] = StrokeWidthStop {
                offset: offset.clamp(0.0, 1.0),
                scale: scale.clamp(0.0, 10.0),
            };
        }
        Self {
            preset: StrokeProfilePreset::Custom,
            stop_count: count as u8,
            stops: arr,
        }
    }

    pub fn is_default(&self) -> bool {
        self.preset == StrokeProfilePreset::Uniform && self.stop_count == 0
    }

    /// Evaluate the width multiplier at normalized arc-length parameter `t` in `[0.0..=1.0]`.
    pub fn sample_scale(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if self.stop_count > 0 {
            let count = (self.stop_count as usize).min(8);
            let slice = &self.stops[..count];
            if t <= slice[0].offset {
                return slice[0].scale;
            }
            for pair in slice.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if t <= b.offset {
                    let span = b.offset - a.offset;
                    if span <= 1e-9 {
                        return b.scale;
                    }
                    let u = (t - a.offset) / span;
                    return a.scale + (b.scale - a.scale) * u;
                }
            }
            return slice[count - 1].scale;
        }
        match self.preset {
            StrokeProfilePreset::Uniform | StrokeProfilePreset::Custom => 1.0,
            StrokeProfilePreset::TaperBoth => (4.0 * t * (1.0 - t)).max(0.08),
            StrokeProfilePreset::TaperStart => t.max(0.08),
            StrokeProfilePreset::TaperEnd => (1.0 - t).max(0.08),
            StrokeProfilePreset::Wave => 0.6 + 0.4 * (t * std::f64::consts::TAU * 2.0).sin().abs(),
            StrokeProfilePreset::Bell => {
                let d = (t - 0.5) * 2.0;
                (1.0 - d * d).max(0.15)
            }
        }
    }
}

/// Format-7/9 parametric shape and placement options attached to `Transform::primitive`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrimitiveOptions {
    Rectangle(RectangleOptions),
    Oval(OvalOptions),
    Scale9Grid([f64; 4]),
    StrokeProfile(StrokeWidthProfile),
}

impl PrimitiveOptions {
    pub fn is_default(&self) -> bool {
        match self {
            Self::Rectangle(r) => r.is_default(),
            Self::Oval(o) => o.is_default(),
            Self::Scale9Grid(g) => *g == [0.0; 4],
            Self::StrokeProfile(p) => p.is_default(),
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

    pub const SWATCHES_RESOURCE_PATH: &'static str = "palette/swatches.json";

    /// Document-specific custom colour swatches stored portably in the project.
    pub fn custom_swatches(&self) -> Vec<Color> {
        let Some(unit) = self
            .resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case(Self::SWATCHES_RESOURCE_PATH))
        else {
            return Vec::new();
        };
        let Ok(bytes) = unit.decoded_bytes() else {
            return Vec::new();
        };
        serde_json::from_slice::<Vec<Color>>(&bytes).unwrap_or_default()
    }

    /// Persist document-specific custom colour swatches in the project.
    pub fn set_custom_swatches(&mut self, swatches: &[Color]) {
        if swatches.is_empty() {
            self.resources
                .retain(|r| !r.path.eq_ignore_ascii_case(Self::SWATCHES_RESOURCE_PATH));
            return;
        }
        let Ok(bytes) = serde_json::to_vec(swatches) else {
            return;
        };
        let existing_id = self
            .resources
            .iter()
            .find(|r| r.path.eq_ignore_ascii_case(Self::SWATCHES_RESOURCE_PATH))
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let Ok(unit) = crate::ResourceUnit::from_bytes(
            existing_id,
            Self::SWATCHES_RESOURCE_PATH,
            "application/json",
            &bytes,
        ) else {
            return;
        };
        if let Some(slot) = self
            .resources
            .iter_mut()
            .find(|r| r.path.eq_ignore_ascii_case(Self::SWATCHES_RESOURCE_PATH))
        {
            *slot = unit;
        } else {
            self.resources.push(unit);
        }
        self.format_version = self.format_version.max(3);
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
            if !(key.tween.is_some() && key.tween_kind == TweenKind::Shape) {
                key.shape_hints.clear();
            }
            if key.keyed_properties.is_none() {
                in_motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
                let in_classic = key.tween.is_some() && key.tween_kind == TweenKind::Classic;
                key.roving = false;
                if !in_motion {
                    key.property_eases.clear();
                    key.motion_path = None;
                    if !in_classic {
                        key.sync_symbols = false;
                        key.snap_to_guide = false;
                    }
                }
                if !in_classic {
                    key.classic_rotate_turns = 0;
                    key.classic_rotate_dir = ClassicRotateDirection::Auto;
                    key.tween_scale = true;
                }
            } else {
                key.property_eases.clear();
                key.motion_path = None;
                key.sync_symbols = false;
                key.snap_to_guide = false;
                key.classic_rotate_turns = 0;
                key.classic_rotate_dir = ClassicRotateDirection::Auto;
                key.tween_scale = true;
                let has_pos = key.keyed_properties.as_ref().is_some_and(|props| {
                    props.contains(&TweenProperty::X) || props.contains(&TweenProperty::Y)
                });
                if !has_pos {
                    key.roving = false;
                }
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
