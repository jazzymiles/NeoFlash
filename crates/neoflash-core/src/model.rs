use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub use crate::timeline_video::{
    ExternalVideoAsset, VideoDeploymentMode, VideoManifestEntry, VideoSourceSpec,
    collect_external_video_assets, inspect_video_deployment, resolve_video_resource_source,
    resolve_video_source, strip_non_embedded_video_bytes, timeline_video_sources,
    validate_external_video_path, validate_progressive_video_url, video_manifest_entries,
};

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
    /// Layer Z-depth offset from the stage plane (`0.0`); positive is farther from the camera,
    /// negative is closer, producing parallax when a timeline camera moves or zooms (format 9).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub z_depth: f64,
    /// When true, locks this layer (such as a HUD overlay) to the camera viewport so camera pan,
    /// zoom and rotation do not displace it (format 9).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub attach_to_camera: bool,
    /// Layer-level display filters applied to all content on this layer (Flash CS5.5+ / Animate,
    /// format 11). Element-level filters remain a format-7 feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<crate::FilterSpec>,
    /// Optional symmetric link to a partner layer (e.g. linked Video + Audio montage tracks,
    /// format 12). Linked layers split, trim, move and ripple-edit together until unlinked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_layer: Option<Id>,
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

impl Timeline {
    /// Remap all internal layer, group, and element IDs on this timeline using `fresh_id`,
    /// plus optional `symbol_map` (`ElementKind::Instance`) and `resource_map` (`sound`, `video`,
    /// `bitmap_fills`, `ElementKind::Bitmap`) (`T-259` / `T-260`).
    ///
    /// Preserves element identity across keyframes on the same layer and updates every layer
    /// (`parent`, `mask`, `motion_guide`, `linked_layer`, `groups`) and keyframe element-ID
    /// side-table (`motion_tracks`, `motion_keys`, `instance_overrides`, `bitmap_fills`,
    /// `asset_warps`, `element_parents`, `controller_elements`, `ik_armatures`, `appearances`,
    /// `extended_accessibility`, `vector_effects`).
    pub fn remap_internal_ids(
        &mut self,
        mut fresh_id: impl FnMut(&str) -> Id,
        symbol_map: Option<&std::collections::BTreeMap<Id, Id>>,
        resource_map: Option<&std::collections::BTreeMap<Id, Id>>,
    ) {
        let mut layer_map = std::collections::HashMap::with_capacity(self.layers.len());
        for layer in &self.layers {
            layer_map.insert(layer.id.clone(), fresh_id("lyr"));
        }
        for layer in &mut self.layers {
            let mut elem_map = std::collections::HashMap::new();
            for kf in &layer.keyframes {
                for el in &kf.elements {
                    elem_map
                        .entry(el.id.clone())
                        .or_insert_with(|| fresh_id("el"));
                }
            }
            if let Some(lid) = layer_map.get(&layer.id) {
                layer.id = lid.clone();
            }
            layer.properties.parent = layer
                .properties
                .parent
                .as_ref()
                .and_then(|id| layer_map.get(id).cloned());
            layer.properties.mask = layer
                .properties
                .mask
                .as_ref()
                .and_then(|id| layer_map.get(id).cloned());
            layer.properties.motion_guide = layer
                .properties
                .motion_guide
                .as_ref()
                .and_then(|id| layer_map.get(id).cloned());
            layer.properties.linked_layer = layer
                .properties
                .linked_layer
                .as_ref()
                .and_then(|id| layer_map.get(id).cloned());
            for group in &mut layer.properties.groups {
                group.id = fresh_id("grp");
                for member in &mut group.members {
                    if let Some(eid) = elem_map.get(member) {
                        *member = eid.clone();
                    }
                }
            }
            for kf in &mut layer.keyframes {
                kf.remap_all_ids(&elem_map, symbol_map, resource_map);
            }
        }
    }

    /// Remap referenced `ResourceUnit` IDs across `sound`, `video`, `bitmap_fills`, and
    /// `ElementKind::Bitmap` (`T-259` / `T-260`).
    pub fn remap_resource_ids(&mut self, resource_map: &std::collections::HashMap<Id, Id>) {
        for layer in &mut self.layers {
            for kf in &mut layer.keyframes {
                if let Some(snd) = &mut kf.sound
                    && let Some(rid) = resource_map.get(&snd.resource)
                {
                    snd.resource = rid.clone();
                }
                if let Some(vid) = &mut kf.video
                    && let Some(rid) = resource_map.get(&vid.resource)
                {
                    vid.resource = rid.clone();
                }
                for bf in &mut kf.bitmap_fills {
                    if let Some(rid) = resource_map.get(&bf.resource_id) {
                        bf.resource_id = rid.clone();
                    }
                }
                for el in &mut kf.elements {
                    if let ElementKind::Bitmap { resource_id, .. } = &mut el.kind
                        && let Some(rid) = resource_map.get(resource_id)
                    {
                        *resource_id = rid.clone();
                    }
                }
            }
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
            layer.properties.groups.retain_mut(|group| {
                let before_len = group.members.len();
                group.members.retain(|id| members.contains(id.as_str()));
                !group.members.is_empty() && !(before_len > 1 && group.members.len() == 1)
            });
        }
    }

    /// Assign fresh symbol, layer, group, element, and optionally resource IDs across the entire
    /// project while preserving every cross-reference (`root`, `scenes`, `Instance.symbol`,
    /// keyframe side-tables, and resource links) (`T-260`).
    pub fn freshen_all_ids(&mut self, remap_resources: bool, mut fresh_id: impl FnMut(&str) -> Id) {
        let mut sym_map = std::collections::BTreeMap::new();
        for sym in &self.symbols {
            sym_map.insert(sym.id.clone(), fresh_id("sym"));
        }
        let res_map = if remap_resources {
            let mut m = std::collections::BTreeMap::new();
            for res in &mut self.resources {
                let next = fresh_id("res");
                m.insert(res.id.clone(), next.clone());
                res.id = next;
            }
            Some(m)
        } else {
            None
        };
        if let Some(next_root) = sym_map.get(&self.root).cloned() {
            self.root = next_root;
        }
        for scene in &mut self.scenes {
            if let Some(next_scene) = sym_map.get(scene) {
                *scene = next_scene.clone();
            }
        }
        for sym in &mut self.symbols {
            if let Some(next_id) = sym_map.get(&sym.id) {
                sym.id = next_id.clone();
            }
            sym.timeline
                .remap_internal_ids(&mut fresh_id, Some(&sym_map), res_map.as_ref());
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
            let linkable: std::collections::HashSet<Id> = symbol
                .timeline
                .layers
                .iter()
                .filter(|l| l.properties.role != LayerRole::Folder)
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
                if layer.properties.role == LayerRole::Folder
                    || layer
                        .properties
                        .linked_layer
                        .as_ref()
                        .is_some_and(|id| id == &layer.id || !linkable.contains(id))
                {
                    layer.properties.linked_layer = None;
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
        if self.uses_format_10() {
            self.format_version = self.format_version.max(10);
        }
        if self.uses_format_11() {
            self.format_version = self.format_version.max(11);
        }
        if self.uses_format_12() {
            self.format_version = self.format_version.max(12);
        }
    }

    /// Timeline-video out-point trims and linked montage layers (format 12).
    ///
    /// The fields are optional so documents without an authored out-point or linked montage track
    /// remain readable by format-9/11 implementations. A document that stores either feature must
    /// advertise the newer format so an older writer cannot silently discard it on save.
    pub fn uses_format_12(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .any(|layer| {
                layer.properties.linked_layer.is_some()
                    || layer.keyframes.iter().any(|key| {
                        key.video
                            .as_ref()
                            .is_some_and(|video| video.end_offset_ms.is_some())
                    })
            })
    }

    /// Layer/keyframe display-filter stacks and non-embedded timeline-video deployment (format 11).
    pub fn uses_format_11(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .any(|layer| {
                !layer.properties.filters.is_empty()
                    || layer.keyframes.iter().any(|key| {
                        !key.filters.is_empty()
                            || key
                                .video
                                .as_ref()
                                .is_some_and(|video| video.uses_non_embedded_deployment())
                    })
            })
            || self
                .resources
                .iter()
                .any(|resource| resource.is_video() && resource.data_base64.is_empty())
    }

    /// Object-scoped Motion Tween settings and property keys (format 10).
    pub fn uses_format_10(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|symbol| &symbol.timeline.layers)
            .flat_map(|layer| &layer.keyframes)
            .any(|key| !key.motion_tracks.is_empty() || key.motion_keys.is_some())
    }

    /// Explicit painted-alpha/luminance/inverted timeline masks, per-resource audio/bitmap export
    /// settings, classic tween rotation/scale controls, roving keyframes, 9-slice guides,
    /// variable-width stroke profiles, bitmap fills, instance property overrides, asset warp rigs,
    /// camera/depth/parenting/IK rigs, and appearance/modifier stacks (format 9).
    pub fn uses_format_9(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .any(|layer| {
                !layer.properties.mask_mode.is_geometry()
                    || layer.properties.mask_invert
                    || layer.properties.scale9_grid.is_some()
                    || layer.properties.z_depth != 0.0
                    || layer.properties.attach_to_camera
                    || layer.keyframes.iter().any(|k| {
                        k.classic_rotate_turns > 0
                            || !k.classic_rotate_dir.is_default()
                            || !k.tween_scale
                            || k.roving
                            || !k.instance_overrides.is_empty()
                            || !k.bitmap_fills.is_empty()
                            || !k.asset_warps.is_empty()
                            || k.camera.is_some()
                            || !k.element_parents.is_empty()
                            || !k.controller_elements.is_empty()
                            || !k.ik_armatures.is_empty()
                            || !k.appearances.is_empty()
                            || k.video.is_some()
                            || !k.extended_accessibility.is_empty()
                            || !k.vector_effects.is_empty()
                            || !k.property_expressions.is_empty()
                            || k.perspective_grid.is_some()
                            || k.work_area.is_some()
                            || !k.markers.is_empty()
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
    /// options, custom stereo sound envelopes or trim points, and authored element display
    /// filters/blending/visibility).
    pub fn uses_format_7(&self) -> bool {
        self.symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .any(|l| {
                l.keyframes.iter().any(|k| {
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

const fn one_f64() -> f64 {
    1.0
}
fn is_one_f64(v: &f64) -> bool {
    (*v - 1.0).abs() <= 1e-9
}
const fn default_warp_subdivisions() -> u8 {
    4
}
fn is_default_warp_subdivisions(v: &u8) -> bool {
    *v == 4
}
const fn default_focal_length() -> f64 {
    1000.0
}
fn is_default_focal_length(v: &f64) -> bool {
    (*v - 1000.0).abs() <= 1e-6
}

/// Pin constraint mode in an `AssetWarpRig` (T-112, format 9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarpPinKind {
    /// Movable and rotatable deformer pin.
    #[default]
    Open,
    /// Locked anchor pin that holds surrounding geometry near its rest position.
    Fixed,
}

/// Editable deformation pin in element-local coordinates (T-112, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarpPin {
    pub id: Id,
    #[serde(default)]
    pub kind: WarpPinKind,
    pub rest_pos: [f64; 2],
    pub pos: [f64; 2],
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
    pub stiffness: f64,
}

impl WarpPin {
    pub const MAX_PINS: usize = 64;
}

/// Serializable Asset Warp rig attached to a vector or bitmap element on a Keyframe (T-112, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetWarpRig {
    pub element_id: Id,
    pub pins: Vec<WarpPin>,
    #[serde(
        default = "default_warp_subdivisions",
        skip_serializing_if = "is_default_warp_subdivisions"
    )]
    pub envelop_subdivisions: u8,
}

/// 2D camera pose authored on a keyframe (T-113, format 9).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraPose {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub y: f64,
    #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
    pub zoom: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    #[serde(
        default = "default_focal_length",
        skip_serializing_if = "is_default_focal_length"
    )]
    pub focal_length: f64,
    #[serde(default, skip_serializing_if = "crate::ColorTransform::is_identity")]
    pub color_transform: crate::ColorTransform,
}

impl Default for CameraPose {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            rotation: 0.0,
            focal_length: 1000.0,
            color_transform: crate::ColorTransform::default(),
        }
    }
}

/// Parent-child hierarchy link between two elements on the same keyframe (T-113, format 9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementParentLink {
    pub child_id: Id,
    pub parent_id: Id,
}

/// One bone in a bounded Inverse Kinematics (IK) armature (T-113, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkBone {
    pub id: Id,
    pub element_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_bone_id: Option<Id>,
    pub length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_angle: Option<f64>,
    #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
    pub stiffness: f64,
}

impl IkBone {
    pub const MAX_BONES: usize = 64;
}

/// Bounded IK armature rig on a keyframe solved deterministically via CCD (T-113, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IkArmature {
    pub id: Id,
    pub name: String,
    pub bones: Vec<IkBone>,
    pub effector_bone_id: Id,
    pub target_pos: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pole_pos: Option<[f64; 2]>,
}

/// Named global colour swatch linked across document elements (T-114).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalSwatch {
    pub id: Id,
    pub name: String,
    pub color: Color,
}

/// Named reusable vector appearance style (T-114).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorStyle {
    pub id: Id,
    pub name: String,
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<VectorModifier>,
}

/// Pattern or art brush applied along a stroke contour (T-114, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PatternBrushKind {
    DashPattern {
        dash: f64,
        gap: f64,
    },
    ArtStretch {
        stamp_commands: Vec<PathCommand>,
        nominal_length: f64,
    },
    PatternRepeat {
        tile_commands: Vec<PathCommand>,
        spacing: f64,
    },
}

/// Ordered fill or stroke entry in an element's appearance stack (T-114, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppearancePaintEntry {
    Fill {
        color: Color,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gradient: Option<crate::GradientPaint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        swatch_id: Option<Id>,
        #[serde(default)]
        offset: [f64; 2],
        #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
        opacity: f64,
    },
    Stroke {
        color: Color,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gradient: Option<crate::GradientPaint>,
        width: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        swatch_id: Option<Id>,
        #[serde(default, skip_serializing_if = "is_zero")]
        offset_distance: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern_brush: Option<PatternBrushKind>,
        #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
        opacity: f64,
    },
}

/// Non-destructive vector modifier in an element's appearance stack (T-114, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum VectorModifier {
    OffsetContour {
        delta: f64,
    },
    LiveCorners {
        radius: f64,
    },
    Mirror {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis_x: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis_y: Option<f64>,
    },
    Repeater {
        copies: u32,
        offset_x: f64,
        offset_y: f64,
        #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
        scale_step: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        rotation_step: f64,
    },
}

/// Text on Path or multi-column Area Text flow binding (T-114, format 9).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TextPathBinding {
    PathText {
        path_commands: Vec<PathCommand>,
        #[serde(default, skip_serializing_if = "is_zero")]
        start_offset: f64,
    },
    AreaText {
        bounds: [f64; 4],
        #[serde(default = "FrameSound::one")]
        columns: u32,
        #[serde(default, skip_serializing_if = "is_zero")]
        gutter: f64,
    },
}

/// Editable appearance stack, linked style, vector modifiers, and text-path binding for an element (T-114, format 9).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElementAppearance {
    pub element_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_swatch_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_swatch_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<AppearancePaintEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<VectorModifier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_binding: Option<TextPathBinding>,
}

/// An object-specific spatial-path override on a Motion Tween track (format 10).
///
/// Absence of an override keeps the legacy keyframe-level `motion_path`. `Disabled`
/// is therefore distinct from absence: it lets one object opt out of a legacy shared
/// path without changing the other objects on the layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MotionPathOverride {
    Disabled,
    Path { commands: Vec<PathCommand> },
}

/// Sparse object-specific settings for one object in a Motion Tween span (format 10).
///
/// Every optional field falls back to the corresponding legacy setting on the span's
/// start keyframe. `property_eases: Some(vec![])` explicitly removes inherited
/// per-property overrides for this object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionTrackOverride {
    pub element_id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub easing: Option<Easing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property_eases: Option<Vec<(TweenProperty, Easing)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orient_to_path: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap_to_guide: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_symbols: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_path: Option<MotionPathOverride>,
}

impl MotionTrackOverride {
    pub fn new(element_id: impl Into<Id>) -> Self {
        Self {
            element_id: element_id.into(),
            easing: None,
            property_eases: None,
            orient_to_path: None,
            snap_to_guide: None,
            sync_symbols: None,
            motion_path: None,
        }
    }

    pub fn is_noop(&self) -> bool {
        self.easing.is_none()
            && self.property_eases.is_none()
            && self.orient_to_path.is_none()
            && self.snap_to_guide.is_none()
            && self.sync_symbols.is_none()
            && self.motion_path.is_none()
    }
}

/// Properties keyed for one object on a Motion Tween property keyframe (format 10).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionPropertyKey {
    pub element_id: Id,
    pub properties: Vec<TweenProperty>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub roving: bool,
}

/// Resolved spatial path for one object. `Legacy` preserves the format-8 shared
/// first-element-reference behavior; `Object` is authored directly for `element_id`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MotionPathSource<'a> {
    None,
    Legacy(&'a [PathCommand]),
    Object(&'a [PathCommand]),
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
    /// Sparse per-object Motion Tween settings. Empty keeps the legacy shared settings above.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub motion_tracks: Vec<MotionTrackOverride>,
    /// Per-object property keys. `None` keeps legacy `keyed_properties`/`roving` semantics;
    /// `Some` is authoritative even when no object is keyed at this frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_keys: Option<Vec<MotionPropertyKey>>,
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
    /// Serializable Asset Warp rigs for elements on this keyframe (T-112, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_warps: Vec<AssetWarpRig>,
    /// 2D camera pose at this keyframe (T-113, format 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraPose>,
    /// Parent-child hierarchy links between elements on this keyframe (T-113, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub element_parents: Vec<ElementParentLink>,
    /// Element IDs marked as authoring Null/Controller objects on this keyframe (T-113, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controller_elements: Vec<Id>,
    /// Bounded Inverse Kinematics (IK) armatures on this keyframe (T-113, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ik_armatures: Vec<IkArmature>,
    /// Editable appearance/modifier stacks and linked styles for elements on this keyframe (T-114, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appearances: Vec<ElementAppearance>,
    /// Authored timeline video span and cue points on this keyframe (T-118, format 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<crate::TimelineVideoSpan>,
    /// Extended WAI-ARIA role, live region, focus group, and reading-order metadata per element (T-117, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extended_accessibility: Vec<crate::ExtendedAccessibility>,
    /// Non-destructive envelope warps, procedural path effects, blends, freeform gradients, and gradient meshes (T-120, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vector_effects: Vec<crate::ElementVectorEffects>,
    /// Deterministic property expressions modulating transform properties on this keyframe (T-121, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub property_expressions: Vec<crate::PropertyExpression>,
    /// Optional 1/2/3-point perspective grid configuration stored on the timeline's primary keyframe (T-120, format 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perspective_grid: Option<crate::PerspectiveGridSpec>,
    /// Optional preview work area bounds stored on the timeline's primary keyframe (T-121, format 9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_area: Option<crate::PreviewWorkArea>,
    /// Rich timeline span markers stored on the timeline's primary keyframe (T-121, format 9).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<crate::TimelineMarker>,
    /// Keyframe-level display filters applied to content on this keyframe span and interpolated
    /// across tweens (Flash CS5.5+ / Animate, format 11).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<crate::FilterSpec>,
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
            motion_tracks: Vec::new(),
            motion_keys: None,
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
            asset_warps: Vec::new(),
            camera: None,
            element_parents: Vec::new(),
            controller_elements: Vec::new(),
            ik_armatures: Vec::new(),
            appearances: Vec::new(),
            video: None,
            extended_accessibility: Vec::new(),
            vector_effects: Vec::new(),
            property_expressions: Vec::new(),
            perspective_grid: None,
            work_area: None,
            markers: Vec::new(),
            filters: Vec::new(),
        }
    }
}

impl Keyframe {
    pub const MAX_ROTATE_TURNS: u32 = 1000;

    pub fn motion_track(&self, element_id: &str) -> Option<&MotionTrackOverride> {
        self.motion_tracks
            .iter()
            .find(|track| track.element_id == element_id)
    }

    pub fn motion_track_mut(&mut self, element_id: &str) -> Option<&mut MotionTrackOverride> {
        self.motion_tracks
            .iter_mut()
            .find(|track| track.element_id == element_id)
    }

    /// Return the sparse settings entry for `element_id`, creating it when needed.
    pub fn ensure_motion_track(&mut self, element_id: &str) -> &mut MotionTrackOverride {
        if let Some(index) = self
            .motion_tracks
            .iter()
            .position(|track| track.element_id == element_id)
        {
            return &mut self.motion_tracks[index];
        }
        self.motion_tracks
            .push(MotionTrackOverride::new(element_id));
        self.motion_tracks.last_mut().unwrap()
    }

    /// Follow a fresh-ID operation for all element, symbol, and resource references on this keyframe (`T-259` / `T-260`).
    pub fn remap_all_ids(
        &mut self,
        elem_map: &HashMap<Id, Id>,
        symbol_map: Option<&std::collections::BTreeMap<Id, Id>>,
        resource_map: Option<&std::collections::BTreeMap<Id, Id>>,
    ) {
        if let Some(rmap) = resource_map {
            if let Some(snd) = &mut self.sound
                && let Some(rid) = rmap.get(&snd.resource)
            {
                snd.resource = rid.clone();
            }
            if let Some(vid) = &mut self.video
                && let Some(rid) = rmap.get(&vid.resource)
            {
                vid.resource = rid.clone();
            }
        }
        self.remap_motion_element_ids(elem_map);
        for ov in &mut self.instance_overrides {
            if let Some(eid) = elem_map.get(&ov.instance_id) {
                ov.instance_id = eid.clone();
            }
        }
        for bf in &mut self.bitmap_fills {
            if let Some(eid) = elem_map.get(&bf.element_id) {
                bf.element_id = eid.clone();
            }
            if let Some(rmap) = resource_map
                && let Some(rid) = rmap.get(&bf.resource_id)
            {
                bf.resource_id = rid.clone();
            }
        }
        for warp in &mut self.asset_warps {
            if let Some(eid) = elem_map.get(&warp.element_id) {
                warp.element_id = eid.clone();
            }
        }
        for pl in &mut self.element_parents {
            if let Some(eid) = elem_map.get(&pl.child_id) {
                pl.child_id = eid.clone();
            }
            if let Some(pid) = elem_map.get(&pl.parent_id) {
                pl.parent_id = pid.clone();
            }
        }
        for cid in &mut self.controller_elements {
            if let Some(eid) = elem_map.get(cid) {
                *cid = eid.clone();
            }
        }
        for arm in &mut self.ik_armatures {
            for bone in &mut arm.bones {
                if let Some(eid) = elem_map.get(&bone.element_id) {
                    bone.element_id = eid.clone();
                }
            }
        }
        for app in &mut self.appearances {
            if let Some(eid) = elem_map.get(&app.element_id) {
                app.element_id = eid.clone();
            }
        }
        for acc in &mut self.extended_accessibility {
            if let Some(eid) = elem_map.get(&acc.element_id) {
                acc.element_id = eid.clone();
            }
        }
        for fx in &mut self.vector_effects {
            if let Some(eid) = elem_map.get(&fx.element_id) {
                fx.element_id = eid.clone();
            }
        }
        for el in &mut self.elements {
            if let Some(eid) = elem_map.get(&el.id) {
                el.id = eid.clone();
            }
            match &mut el.kind {
                ElementKind::Instance { symbol_id, .. } => {
                    if let Some(smap) = symbol_map
                        && let Some(sid) = smap.get(symbol_id)
                    {
                        *symbol_id = sid.clone();
                    }
                }
                ElementKind::Bitmap { resource_id, .. } => {
                    if let Some(rmap) = resource_map
                        && let Some(rid) = rmap.get(resource_id)
                    {
                        *resource_id = rid.clone();
                    }
                }
                _ => {}
            }
        }
    }

    /// Follow a fresh-ID operation for only the object references owned by Motion data.
    /// IDs absent from `remap` are deliberately retained so partial imports cannot retarget
    /// a track to an unrelated object.
    pub fn remap_motion_element_ids(&mut self, remap: &HashMap<Id, Id>) {
        for track in &mut self.motion_tracks {
            if let Some(fresh) = remap.get(&track.element_id) {
                track.element_id = fresh.clone();
            }
        }
        if let Some(keys) = &mut self.motion_keys {
            for key in keys {
                if let Some(fresh) = remap.get(&key.element_id) {
                    key.element_id = fresh.clone();
                }
            }
        }
    }

    pub fn motion_easing_for(&self, element_id: &str) -> Easing {
        self.motion_track(element_id)
            .and_then(|track| track.easing)
            .or(self.tween)
            .unwrap_or(Easing::Linear)
    }

    pub fn motion_property_ease_for(&self, element_id: &str, property: TweenProperty) -> Easing {
        let overrides = self
            .motion_track(element_id)
            .and_then(|track| track.property_eases.as_deref())
            .unwrap_or(&self.property_eases);
        overrides
            .iter()
            .find(|(candidate, _)| *candidate == property)
            .map(|(_, ease)| *ease)
            .unwrap_or_else(|| self.motion_easing_for(element_id))
    }

    pub fn motion_orient_to_path_for(&self, element_id: &str) -> bool {
        self.motion_track(element_id)
            .and_then(|track| track.orient_to_path)
            .unwrap_or(self.orient_to_path)
    }

    pub fn motion_snap_to_guide_for(&self, element_id: &str) -> bool {
        self.motion_track(element_id)
            .and_then(|track| track.snap_to_guide)
            .unwrap_or(self.snap_to_guide)
    }

    pub fn motion_sync_symbols_for(&self, element_id: &str) -> bool {
        self.motion_track(element_id)
            .and_then(|track| track.sync_symbols)
            .unwrap_or(self.sync_symbols)
    }

    pub fn motion_path_for(&self, element_id: &str) -> MotionPathSource<'_> {
        match self
            .motion_track(element_id)
            .and_then(|track| track.motion_path.as_ref())
        {
            Some(MotionPathOverride::Disabled) => MotionPathSource::None,
            Some(MotionPathOverride::Path { commands }) => {
                MotionPathSource::Object(commands.as_slice())
            }
            None => self
                .motion_path
                .as_deref()
                .map_or(MotionPathSource::None, MotionPathSource::Legacy),
        }
    }

    /// Properties keyed for one object. Explicit format-10 keys are authoritative;
    /// absent format-10 data retains the legacy broadcast semantics.
    pub fn motion_key_properties_for(&self, element_id: &str) -> &[TweenProperty] {
        match &self.motion_keys {
            Some(keys) => keys
                .iter()
                .find(|key| key.element_id == element_id)
                .map_or(&[], |key| key.properties.as_slice()),
            None => self.keyed_properties.as_deref().unwrap_or(&[]),
        }
    }

    pub fn motion_key_roving_for(&self, element_id: &str) -> bool {
        match &self.motion_keys {
            Some(keys) => keys
                .iter()
                .find(|key| key.element_id == element_id)
                .is_some_and(|key| key.roving),
            None => self.roving,
        }
    }

    /// Convert one legacy broadcast property key into explicit per-object entries without
    /// changing its behavior. New object-specific edits can then modify only their entry.
    pub fn materialize_motion_keys(&mut self) {
        if self.motion_keys.is_some() {
            return;
        }
        let properties = self.keyed_properties.clone().unwrap_or_default();
        let roving = self.roving
            && (properties.contains(&TweenProperty::X) || properties.contains(&TweenProperty::Y));
        let keys = if properties.is_empty() {
            Vec::new()
        } else {
            self.elements
                .iter()
                .map(|element| MotionPropertyKey {
                    element_id: element.id.clone(),
                    properties: properties.clone(),
                    roving,
                })
                .collect()
        };
        self.motion_keys = Some(keys);
        self.roving = false;
        self.sync_motion_key_union();
    }

    pub fn ensure_motion_property_key(&mut self, element_id: &str) -> &mut MotionPropertyKey {
        self.materialize_motion_keys();
        let keys = self.motion_keys.as_mut().unwrap();
        if let Some(index) = keys.iter().position(|key| key.element_id == element_id) {
            return &mut keys[index];
        }
        keys.push(MotionPropertyKey {
            element_id: element_id.to_owned(),
            properties: Vec::new(),
            roving: false,
        });
        keys.last_mut().unwrap()
    }

    /// Rebuild the legacy structural union used to identify a Motion property key.
    pub fn sync_motion_key_union(&mut self) {
        let Some(keys) = &self.motion_keys else {
            return;
        };
        let union = TweenProperty::ALL
            .into_iter()
            .filter(|property| keys.iter().any(|key| key.properties.contains(property)))
            .collect();
        self.keyed_properties = Some(union);
    }

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

    pub fn asset_warp(&self, element_id: &str) -> Option<&AssetWarpRig> {
        self.asset_warps.iter().find(|w| w.element_id == element_id)
    }

    pub fn asset_warp_mut(&mut self, element_id: &str) -> Option<&mut AssetWarpRig> {
        self.asset_warps
            .iter_mut()
            .find(|w| w.element_id == element_id)
    }

    pub fn set_asset_warp(&mut self, rig: AssetWarpRig) {
        if let Some(slot) = self
            .asset_warps
            .iter_mut()
            .find(|w| w.element_id == rig.element_id)
        {
            *slot = rig;
        } else {
            self.asset_warps.push(rig);
        }
    }

    /// Reset all movable pins in the element's Asset Warp rig back to their rest positions and zero rotation (T-112).
    pub fn reset_asset_warp(&mut self, element_id: &str) -> bool {
        let Some(rig) = self.asset_warp_mut(element_id) else {
            return false;
        };
        for pin in &mut rig.pins {
            pin.pos = pin.rest_pos;
            pin.rotation = 0.0;
        }
        true
    }

    /// Deterministically bake/expand the element's Asset Warp rig into its geometry and remove the rig (T-112).
    pub fn bake_asset_warp(&mut self, element_id: &str) -> bool {
        let Some(rig) = self.asset_warp(element_id).cloned() else {
            return false;
        };
        let Some(el) = self.elements.iter_mut().find(|e| e.id == element_id) else {
            return false;
        };
        *el = crate::geometry::apply_asset_warp_to_element(el, &rig);
        self.asset_warps.retain(|w| w.element_id != element_id);
        true
    }

    pub fn parent_of(&self, child_id: &str) -> Option<&str> {
        self.element_parents
            .iter()
            .find(|p| p.child_id == child_id)
            .map(|p| p.parent_id.as_str())
    }

    /// Set or clear an element's parent on this keyframe, rejecting self-parenting and cycles (T-113).
    pub fn set_element_parent(
        &mut self,
        child_id: Id,
        parent_id: Option<Id>,
    ) -> Result<(), &'static str> {
        let Some(parent_id) = parent_id else {
            self.element_parents.retain(|p| p.child_id != child_id);
            return Ok(());
        };
        if child_id == parent_id {
            return Err("an element cannot be parented to itself");
        }
        if !self.elements.iter().any(|e| e.id == child_id)
            || !self.elements.iter().any(|e| e.id == parent_id)
        {
            return Err("both child and parent elements must exist on the keyframe");
        }
        let mut cursor = Some(parent_id.as_str());
        let mut steps = 0usize;
        while let Some(cur) = cursor {
            if cur == child_id {
                return Err("parenting cycle detected");
            }
            steps += 1;
            if steps > self.elements.len() + 1 {
                return Err("parenting cycle detected");
            }
            cursor = self.parent_of(cur);
        }
        if let Some(slot) = self
            .element_parents
            .iter_mut()
            .find(|p| p.child_id == child_id)
        {
            slot.parent_id = parent_id;
        } else {
            self.element_parents.push(ElementParentLink {
                child_id,
                parent_id,
            });
        }
        Ok(())
    }

    /// Effective 2D affine matrix `[a, b, c, d, tx, ty]` of `element_id` in layer coordinates after walking its parent chain (T-113).
    pub fn element_world_matrix(&self, element_id: &str) -> Option<[f64; 6]> {
        let el = self.elements.iter().find(|e| e.id == element_id)?;
        let mut m = crate::evaluate::transform_matrix(el.transform);
        let mut cursor = self.parent_of(element_id);
        let mut depth = 0usize;
        while let Some(pid) = cursor {
            if depth >= 64 {
                break;
            }
            let Some(parent) = self.elements.iter().find(|e| e.id == pid) else {
                break;
            };
            m = crate::evaluate::affine_multiply(
                crate::evaluate::transform_matrix(parent.transform),
                m,
            );
            cursor = self.parent_of(pid);
            depth += 1;
        }
        Some(m)
    }

    /// Reparent `child_id` to `new_parent_id` while adjusting `child.transform` so its world matrix on stage is preserved (T-113).
    pub fn reparent_preserving_world_transform(
        &mut self,
        child_id: &str,
        new_parent_id: Option<&str>,
    ) -> Result<(), &'static str> {
        let world = self
            .element_world_matrix(child_id)
            .ok_or("child element does not exist on keyframe")?;
        let new_parent_world = match new_parent_id {
            Some(pid) => self
                .element_world_matrix(pid)
                .ok_or("parent element does not exist on keyframe")?,
            None => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        self.set_element_parent(child_id.to_owned(), new_parent_id.map(str::to_owned))?;
        let inv_parent = crate::geometry::invert_affine_matrix(new_parent_world)
            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let local_matrix = crate::evaluate::affine_multiply(inv_parent, world);
        if let Some(new_t) = Transform::from_matrix(local_matrix)
            && let Some(child) = self.elements.iter_mut().find(|e| e.id == child_id)
        {
            child.transform.x = new_t.x;
            child.transform.y = new_t.y;
            child.transform.scale_x = new_t.scale_x;
            child.transform.scale_y = new_t.scale_y;
            child.transform.rotation = new_t.rotation;
            child.transform.skew = new_t.skew;
        }
        Ok(())
    }

    pub fn appearance(&self, element_id: &str) -> Option<&ElementAppearance> {
        self.appearances.iter().find(|a| a.element_id == element_id)
    }

    pub fn set_appearance(&mut self, appearance: ElementAppearance) {
        if let Some(slot) = self
            .appearances
            .iter_mut()
            .find(|a| a.element_id == appearance.element_id)
        {
            *slot = appearance;
        } else {
            self.appearances.push(appearance);
        }
    }

    /// Deterministically expand/bake the appearance stack and modifiers of `element_id` into concrete elements on this keyframe (T-114).
    pub fn expand_element_appearance(&mut self, element_id: &str) -> Vec<Id> {
        let Some(app) = self.appearance(element_id).cloned() else {
            return Vec::new();
        };
        let Some(idx) = self.elements.iter().position(|e| e.id == element_id) else {
            return Vec::new();
        };
        let base = self.elements[idx].clone();
        let expanded = crate::geometry::evaluate_element_appearance(&base, &app);
        self.appearances.retain(|a| a.element_id != element_id);
        if expanded.is_empty() {
            return Vec::new();
        }
        let ids: Vec<Id> = expanded.iter().map(|e| e.id.clone()).collect();
        self.elements.splice(idx..=idx, expanded);
        ids
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

impl ElementKind {
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. } | Self::TextBox { .. })
    }

    pub fn text_utf16_len(&self) -> Option<usize> {
        crate::TextFieldState::from_kind(self).map(|s| s.utf16_len())
    }

    pub fn format_at_utf16(&self, offset_utf16: usize) -> Option<crate::TextFormat> {
        crate::TextFieldState::from_kind(self).map(|s| s.format_at_utf16_offset(offset_utf16))
    }

    pub fn format_at_utf16_caret(&self, caret_utf16: usize) -> Option<crate::TextFormat> {
        crate::TextFieldState::from_kind(self).map(|s| s.format_at_utf16_caret(caret_utf16))
    }

    pub fn format_for_utf16_selection(
        &self,
        start_utf16: usize,
        end_utf16: usize,
    ) -> Option<crate::TextFormat> {
        crate::TextFieldState::from_kind(self)
            .map(|s| s.format_for_utf16_selection(start_utf16, end_utf16))
    }

    pub fn apply_format_to_utf16_range(
        &mut self,
        start_utf16: usize,
        end_utf16: usize,
        patch: &crate::TextFormatPatch,
    ) -> Result<(), crate::TextError> {
        let mut state = crate::TextFieldState::from_kind(self)
            .ok_or_else(|| crate::TextError("Element is not a text field".into()))?;
        state.apply_format_to_utf16_range(start_utf16, end_utf16, patch)?;
        state.write_to_kind(self);
        Ok(())
    }

    pub fn replace_utf16_range(
        &mut self,
        start_utf16: usize,
        end_utf16: usize,
        inserted: &str,
        typing_style: Option<&crate::TextFormat>,
    ) -> Result<usize, crate::TextError> {
        let mut state = crate::TextFieldState::from_kind(self)
            .ok_or_else(|| crate::TextError("Element is not a text field".into()))?;
        let new_caret =
            state.replace_utf16_range(start_utf16, end_utf16, inserted, typing_style)?;
        state.write_to_kind(self);
        Ok(new_caret)
    }
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

/// Shared Library linkage metadata for a symbol (`T-128`, `editor-symbols :: shared-libraries`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedLibraryLinkage {
    #[serde(default)]
    pub symbol_id: Id,
    pub url: String,
    #[serde(default)]
    pub export_for_runtime: bool,
    #[serde(default)]
    pub import_for_runtime: bool,
    #[serde(default = "bool_true")]
    pub always_update: bool,
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
    pub fn resource(&self, id: &str) -> Option<&crate::ResourceUnit> {
        self.resources.iter().find(|r| r.id == id)
    }
    pub fn resource_mut(&mut self, id: &str) -> Option<&mut crate::ResourceUnit> {
        self.resources.iter_mut().find(|r| r.id == id)
    }
    pub fn collect_external_video_assets(&self) -> Vec<crate::ExternalVideoAsset> {
        crate::collect_external_video_assets(self)
    }
    pub fn video_manifest_entries(&self) -> Vec<crate::VideoManifestEntry> {
        crate::video_manifest_entries(self)
    }
    pub fn strip_non_embedded_video_bytes(&mut self) -> Vec<crate::ExternalVideoAsset> {
        crate::strip_non_embedded_video_bytes(self)
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

    pub const GLOBAL_SWATCHES_RESOURCE_PATH: &'static str = "palette/global_swatches.json";
    pub const VECTOR_STYLES_RESOURCE_PATH: &'static str = "palette/vector_styles.json";
    pub const DOCUMENT_ACCESSIBILITY_RESOURCE_PATH: &'static str = "accessibility/document.json";
    pub const SHARED_LIBRARIES_RESOURCE_PATH: &'static str = "library/shared_libraries.json";

    /// Shared Library linkage entries stored portably in the project (`T-128`).
    pub fn shared_libraries(&self) -> Vec<SharedLibraryLinkage> {
        let Some(unit) = self.resources.iter().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::SHARED_LIBRARIES_RESOURCE_PATH)
        }) else {
            return Vec::new();
        };
        let Ok(bytes) = unit.decoded_bytes() else {
            return Vec::new();
        };
        serde_json::from_slice::<Vec<SharedLibraryLinkage>>(&bytes).unwrap_or_default()
    }

    /// Shared Library linkage metadata for `symbol_id` (`T-128`).
    pub fn shared_library(&self, symbol_id: &str) -> Option<SharedLibraryLinkage> {
        self.shared_libraries()
            .into_iter()
            .find(|entry| entry.symbol_id == symbol_id)
    }

    /// Set or clear Shared Library linkage metadata for `symbol_id` (`T-128`).
    pub fn set_shared_library(&mut self, symbol_id: &str, linkage: Option<SharedLibraryLinkage>) {
        let mut entries = self.shared_libraries();
        entries.retain(|entry| entry.symbol_id != symbol_id);
        if let Some(mut item) = linkage {
            item.symbol_id = symbol_id.to_owned();
            entries.push(item);
        }
        if entries.is_empty() {
            self.resources.retain(|r| {
                !r.path
                    .eq_ignore_ascii_case(Self::SHARED_LIBRARIES_RESOURCE_PATH)
            });
            return;
        }
        let Ok(bytes) = serde_json::to_vec(&entries) else {
            return;
        };
        let existing_id = self
            .resources
            .iter()
            .find(|r| {
                r.path
                    .eq_ignore_ascii_case(Self::SHARED_LIBRARIES_RESOURCE_PATH)
            })
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let Ok(unit) = crate::ResourceUnit::from_bytes(
            existing_id,
            Self::SHARED_LIBRARIES_RESOURCE_PATH,
            "application/json",
            &bytes,
        ) else {
            return;
        };
        if let Some(slot) = self.resources.iter_mut().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::SHARED_LIBRARIES_RESOURCE_PATH)
        }) {
            *slot = unit;
        } else {
            self.resources.push(unit);
        }
        self.format_version = self.format_version.max(3);
    }

    /// Synchronize imported Shared Library symbols in `self` from `donor` (`T-128`, `T-231`).
    ///
    /// Matches symbols with `import_for_runtime == true` against exported symbols in `donor`
    /// (by `linkage_id` or `name`), updating the local symbol's `kind`, `timeline`, and
    /// non-colliding `class_name` while preserving its local `id` so existing instances remain
    /// valid, and transitively importing any nested sub-symbols, resources, global swatches, and
    /// vector styles with collision-safe IDs and paths.
    pub fn resolve_shared_library_symbols(&mut self, donor: &Project) -> usize {
        use std::collections::{BTreeMap, HashMap, HashSet};

        let imports: Vec<SharedLibraryLinkage> = self
            .shared_libraries()
            .into_iter()
            .filter(|entry| entry.import_for_runtime)
            .collect();
        if imports.is_empty() {
            return 0;
        }
        let donor_exports = donor.shared_libraries();
        let mut matched_roots: Vec<(Id, Symbol)> = Vec::new();
        for import_entry in imports {
            let Some(local_sym) = self
                .symbols
                .iter()
                .find(|s| s.id == import_entry.symbol_id)
                .cloned()
            else {
                continue;
            };
            if self.is_scene(&local_sym.id) {
                continue;
            }
            let donor_sym = donor
                .symbols
                .iter()
                .find(|ds| {
                    if donor.is_scene(&ds.id) {
                        return false;
                    }
                    let is_exported = donor_exports
                        .iter()
                        .any(|de| de.symbol_id == ds.id && de.export_for_runtime)
                        || ds.linkage_id.is_some();
                    if !is_exported {
                        return false;
                    }
                    if let (Some(l_link), Some(d_link)) =
                        (local_sym.linkage_id.as_deref(), ds.linkage_id.as_deref())
                        && l_link == d_link
                    {
                        return true;
                    }
                    ds.name == local_sym.name
                        || ds.linkage_id.as_deref() == Some(local_sym.name.as_str())
                })
                .cloned();
            if let Some(ds) = donor_sym
                && !matched_roots.iter().any(|(lid, _)| lid == &local_sym.id)
            {
                matched_roots.push((local_sym.id, ds));
            }
        }
        if matched_roots.is_empty() {
            return 0;
        }

        // Collect transitive symbol, resource, swatch, and style dependencies in `donor`.
        let mut sym_map: BTreeMap<Id, Id> = BTreeMap::new();
        for (local_id, donor_sym) in &matched_roots {
            sym_map.insert(donor_sym.id.clone(), local_id.clone());
        }
        let mut visited_donor_syms: HashSet<Id> = HashSet::new();
        let mut extra_donor_syms: Vec<Symbol> = Vec::new();
        let mut needed_resources: Vec<Id> = Vec::new();
        let mut seen_resources: HashSet<Id> = HashSet::new();
        let mut needed_swatches: Vec<Id> = Vec::new();
        let mut seen_swatches: HashSet<Id> = HashSet::new();
        let mut needed_styles: Vec<Id> = Vec::new();
        let mut seen_styles: HashSet<Id> = HashSet::new();

        let mut stack: Vec<Symbol> = matched_roots.iter().map(|(_, ds)| ds.clone()).collect();
        while let Some(curr) = stack.pop() {
            if !visited_donor_syms.insert(curr.id.clone()) {
                continue;
            }
            if !matched_roots.iter().any(|(_, ds)| ds.id == curr.id) {
                extra_donor_syms.push(curr.clone());
            }
            for layer in &curr.timeline.layers {
                for kf in &layer.keyframes {
                    if let Some(snd) = &kf.sound
                        && seen_resources.insert(snd.resource.clone())
                    {
                        needed_resources.push(snd.resource.clone());
                    }
                    if let Some(vid) = &kf.video
                        && seen_resources.insert(vid.resource.clone())
                    {
                        needed_resources.push(vid.resource.clone());
                    }
                    for bf in &kf.bitmap_fills {
                        if seen_resources.insert(bf.resource_id.clone()) {
                            needed_resources.push(bf.resource_id.clone());
                        }
                    }
                    for app in &kf.appearances {
                        for sw in app.fill_swatch_id.iter().chain(app.stroke_swatch_id.iter()) {
                            if seen_swatches.insert(sw.clone()) {
                                needed_swatches.push(sw.clone());
                            }
                        }
                        if let Some(st) = &app.style_id
                            && seen_styles.insert(st.clone())
                        {
                            needed_styles.push(st.clone());
                        }
                        for entry in &app.entries {
                            let sw_opt = match entry {
                                AppearancePaintEntry::Fill { swatch_id, .. }
                                | AppearancePaintEntry::Stroke { swatch_id, .. } => swatch_id,
                            };
                            if let Some(sw) = sw_opt
                                && seen_swatches.insert(sw.clone())
                            {
                                needed_swatches.push(sw.clone());
                            }
                        }
                    }
                    for el in &kf.elements {
                        match &el.kind {
                            ElementKind::Instance { symbol_id, .. } => {
                                if !donor.is_scene(symbol_id)
                                    && let Some(child_sym) = donor.symbol(symbol_id)
                                {
                                    stack.push(child_sym.clone());
                                }
                            }
                            ElementKind::Bitmap { resource_id, .. }
                                if seen_resources.insert(resource_id.clone()) =>
                            {
                                needed_resources.push(resource_id.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Determine target IDs for nested helper symbols (reusing previously synced helpers when
        // name + kind match, and never overwriting unrelated local symbols).
        let mut replaced_symbol_ids: HashSet<Id> =
            matched_roots.iter().map(|(lid, _)| lid.clone()).collect();
        for helper in &extra_donor_syms {
            if sym_map.contains_key(&helper.id) {
                continue;
            }
            let shared_candidate = format!("shared-{}", helper.id);
            let reusable = self
                .symbols
                .iter()
                .find(|s| {
                    !self.is_scene(&s.id)
                        && !replaced_symbol_ids.contains(&s.id)
                        && (s.id == helper.id || s.id == shared_candidate)
                        && s.name == helper.name
                        && s.kind == helper.kind
                })
                .map(|s| s.id.clone());
            if let Some(existing_id) = reusable {
                replaced_symbol_ids.insert(existing_id.clone());
                sym_map.insert(helper.id.clone(), existing_id);
            }
        }

        // Collect occupied IDs across `self`, excluding internal timeline IDs of symbols whose
        // timelines are about to be replaced.
        let mut occupied_ids: HashSet<Id> = HashSet::new();
        occupied_ids.insert(self.root.clone());
        occupied_ids.extend(self.scenes.iter().cloned());
        for sym in &self.symbols {
            occupied_ids.insert(sym.id.clone());
            if !replaced_symbol_ids.contains(&sym.id) {
                for layer in &sym.timeline.layers {
                    occupied_ids.insert(layer.id.clone());
                    for g in &layer.properties.groups {
                        occupied_ids.insert(g.id.clone());
                    }
                    for kf in &layer.keyframes {
                        for el in &kf.elements {
                            occupied_ids.insert(el.id.clone());
                        }
                    }
                }
            }
        }
        for res in &self.resources {
            occupied_ids.insert(res.id.clone());
        }
        for sw in self.global_swatches() {
            occupied_ids.insert(sw.id);
        }
        for st in self.vector_styles() {
            occupied_ids.insert(st.id);
        }

        let alloc_id = |prefix: &str, preferred: Option<&str>, occupied: &mut HashSet<Id>| -> Id {
            if let Some(pref) = preferred
                && !pref.is_empty()
                && occupied.insert(pref.to_owned())
            {
                return pref.to_owned();
            }
            let stem = preferred.filter(|p| !p.is_empty()).unwrap_or(prefix);
            let first = format!("shared-{stem}");
            if occupied.insert(first.clone()) {
                return first;
            }
            let mut n = 2usize;
            loop {
                let cand = format!("shared-{stem}-{n}");
                if occupied.insert(cand.clone()) {
                    return cand;
                }
                n += 1;
            }
        };

        for helper in &extra_donor_syms {
            if !sym_map.contains_key(&helper.id) {
                let target_id = alloc_id("sym", Some(&helper.id), &mut occupied_ids);
                sym_map.insert(helper.id.clone(), target_id);
            }
        }

        // Import/deduplicate referenced resources.
        let mut occupied_paths: HashSet<String> = self
            .resources
            .iter()
            .map(|r| r.path.to_ascii_lowercase())
            .collect();
        let mut res_map: BTreeMap<Id, Id> = BTreeMap::new();
        for res_id in &needed_resources {
            let Some(donor_res) = donor.resources.iter().find(|r| r.id == *res_id) else {
                continue;
            };
            if donor_res
                .path
                .eq_ignore_ascii_case(Self::SHARED_LIBRARIES_RESOURCE_PATH)
            {
                continue;
            }
            if let Some(existing) = self.resources.iter().find(|r| {
                r.media_type == donor_res.media_type && r.data_base64 == donor_res.data_base64
            }) {
                res_map.insert(res_id.clone(), existing.id.clone());
                continue;
            }
            let new_id = alloc_id("res", Some(&donor_res.id), &mut occupied_ids);
            let new_path = if occupied_paths.insert(donor_res.path.to_ascii_lowercase()) {
                donor_res.path.clone()
            } else {
                let (stem, ext) = match donor_res.path.rsplit_once('.') {
                    Some((s, e)) => (s, format!(".{e}")),
                    None => (donor_res.path.as_str(), String::new()),
                };
                let mut candidate_path = format!("{stem}-{new_id}{ext}");
                let mut n = 2usize;
                while !occupied_paths.insert(candidate_path.to_ascii_lowercase()) {
                    candidate_path = format!("{stem}-{new_id}-{n}{ext}");
                    n += 1;
                }
                candidate_path
            };
            let mut imported = donor_res.clone();
            imported.id = new_id.clone();
            imported.path = new_path;
            self.resources.push(imported);
            res_map.insert(res_id.clone(), new_id);
        }

        // Import/deduplicate referenced global swatches and vector styles.
        let mut swatch_map: HashMap<Id, Id> = HashMap::new();
        if !needed_swatches.is_empty() {
            let donor_swatches = donor.global_swatches();
            let mut local_swatches = self.global_swatches();
            let mut changed_swatches = false;
            for sw_id in &needed_swatches {
                let Some(dsw) = donor_swatches.iter().find(|s| s.id == *sw_id) else {
                    continue;
                };
                if let Some(existing) = local_swatches
                    .iter()
                    .find(|s| s.name == dsw.name && s.color == dsw.color)
                {
                    swatch_map.insert(sw_id.clone(), existing.id.clone());
                    continue;
                }
                let new_id = alloc_id("swatch", Some(sw_id), &mut occupied_ids);
                let mut copy = dsw.clone();
                copy.id = new_id.clone();
                local_swatches.push(copy);
                swatch_map.insert(sw_id.clone(), new_id);
                changed_swatches = true;
            }
            if changed_swatches {
                self.set_global_swatches(&local_swatches);
            }
        }

        let mut style_map: HashMap<Id, Id> = HashMap::new();
        if !needed_styles.is_empty() {
            let donor_styles = donor.vector_styles();
            let mut local_styles = self.vector_styles();
            let mut changed_styles = false;
            for st_id in &needed_styles {
                let Some(dst) = donor_styles.iter().find(|s| s.id == *st_id) else {
                    continue;
                };
                if let Some(existing) = local_styles.iter().find(|s| s == &dst) {
                    style_map.insert(st_id.clone(), existing.id.clone());
                    continue;
                }
                let new_id = alloc_id("style", Some(st_id), &mut occupied_ids);
                let mut copy = dst.clone();
                copy.id = new_id.clone();
                local_styles.push(copy);
                style_map.insert(st_id.clone(), new_id);
                changed_styles = true;
            }
            if changed_styles {
                self.set_vector_styles(&local_styles);
            }
        }

        let remap_timeline = |mut tl: Timeline,
                              occupied: &mut HashSet<Id>,
                              sym_map: &BTreeMap<Id, Id>,
                              res_map: &BTreeMap<Id, Id>,
                              swatch_map: &HashMap<Id, Id>,
                              style_map: &HashMap<Id, Id>|
         -> Timeline {
            let mut layer_map: HashMap<Id, Id> = HashMap::with_capacity(tl.layers.len());
            for layer in &tl.layers {
                let next_lid = alloc_id("layer", Some(&layer.id), occupied);
                layer_map.insert(layer.id.clone(), next_lid);
            }
            for layer in &mut tl.layers {
                let mut elem_map: HashMap<Id, Id> = HashMap::new();
                for kf in &layer.keyframes {
                    for el in &kf.elements {
                        elem_map
                            .entry(el.id.clone())
                            .or_insert_with(|| alloc_id("object", Some(&el.id), occupied));
                    }
                }
                if let Some(lid) = layer_map.get(&layer.id) {
                    layer.id = lid.clone();
                }
                layer.properties.parent = layer
                    .properties
                    .parent
                    .as_ref()
                    .and_then(|id| layer_map.get(id).cloned());
                layer.properties.mask = layer
                    .properties
                    .mask
                    .as_ref()
                    .and_then(|id| layer_map.get(id).cloned());
                layer.properties.motion_guide = layer
                    .properties
                    .motion_guide
                    .as_ref()
                    .and_then(|id| layer_map.get(id).cloned());
                layer.properties.linked_layer = layer
                    .properties
                    .linked_layer
                    .as_ref()
                    .and_then(|id| layer_map.get(id).cloned());
                for group in &mut layer.properties.groups {
                    group.id = alloc_id("group", Some(&group.id), occupied);
                    for member in &mut group.members {
                        if let Some(eid) = elem_map.get(member) {
                            *member = eid.clone();
                        }
                    }
                }
                for kf in &mut layer.keyframes {
                    kf.remap_all_ids(&elem_map, Some(sym_map), Some(res_map));
                    for app in &mut kf.appearances {
                        if let Some(sw) = &mut app.fill_swatch_id
                            && let Some(mapped) = swatch_map.get(sw)
                        {
                            *sw = mapped.clone();
                        }
                        if let Some(sw) = &mut app.stroke_swatch_id
                            && let Some(mapped) = swatch_map.get(sw)
                        {
                            *sw = mapped.clone();
                        }
                        if let Some(st) = &mut app.style_id
                            && let Some(mapped) = style_map.get(st)
                        {
                            *st = mapped.clone();
                        }
                        for entry in &mut app.entries {
                            let sw_opt = match entry {
                                AppearancePaintEntry::Fill { swatch_id, .. }
                                | AppearancePaintEntry::Stroke { swatch_id, .. } => swatch_id,
                            };
                            if let Some(sw) = sw_opt
                                && let Some(mapped) = swatch_map.get(sw)
                            {
                                *sw = mapped.clone();
                            }
                        }
                    }
                }
            }
            tl
        };

        let mut occupied_linkages: HashSet<String> = self
            .symbols
            .iter()
            .filter_map(|s| s.linkage_id.clone())
            .collect();
        let mut occupied_classes: HashSet<String> = self
            .symbols
            .iter()
            .filter_map(|s| s.class_name.clone())
            .collect();

        // Install or update nested helper symbols first so root symbol references validate cleanly.
        for helper in extra_donor_syms.into_iter().rev() {
            let Some(target_id) = sym_map.get(&helper.id).cloned() else {
                continue;
            };
            let remapped_tl = remap_timeline(
                helper.timeline.clone(),
                &mut occupied_ids,
                &sym_map,
                &res_map,
                &swatch_map,
                &style_map,
            );
            if let Some(existing) = self.symbols.iter_mut().find(|s| s.id == target_id) {
                existing.kind = helper.kind;
                existing.timeline = remapped_tl;
            } else {
                let mut imported = helper;
                imported.id = target_id;
                imported.timeline = remapped_tl;
                if let Some(link) = &imported.linkage_id
                    && !occupied_linkages.insert(link.clone())
                {
                    imported.linkage_id = None;
                }
                if let Some(cls) = &imported.class_name
                    && !occupied_classes.insert(cls.clone())
                {
                    imported.class_name = None;
                }
                if !imported.folder.is_empty() {
                    let _ = self.add_library_folder(&imported.folder);
                }
                self.symbols.push(imported);
            }
        }

        // Update matched root symbols while preserving their local symbol IDs.
        let mut updated = 0usize;
        for (local_id, donor_sym) in matched_roots {
            let remapped_tl = remap_timeline(
                donor_sym.timeline,
                &mut occupied_ids,
                &sym_map,
                &res_map,
                &swatch_map,
                &style_map,
            );
            let class_taken_elsewhere = donor_sym.class_name.as_ref().is_some_and(|cls| {
                self.symbols
                    .iter()
                    .any(|s| s.id != local_id && s.class_name.as_ref() == Some(cls))
            });
            if let Some(target_sym) = self.symbols.iter_mut().find(|s| s.id == local_id) {
                target_sym.kind = donor_sym.kind;
                target_sym.timeline = remapped_tl;
                if let Some(cls) = donor_sym.class_name
                    && !class_taken_elsewhere
                {
                    occupied_classes.insert(cls.clone());
                    target_sym.class_name = Some(cls);
                }
                updated += 1;
            }
        }
        if updated > 0 {
            self.format_version = self.format_version.max(donor.format_version).max(3);
            self.upgrade_format_for_features();
        }
        updated
    }

    /// Document-level accessibility settings (auto-label, title, description, reading order, `T-117`).
    pub fn document_accessibility(&self) -> crate::DocumentAccessibility {
        let Some(unit) = self.resources.iter().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH)
        }) else {
            return crate::DocumentAccessibility::default();
        };
        let Ok(bytes) = unit.decoded_bytes() else {
            return crate::DocumentAccessibility::default();
        };
        serde_json::from_slice::<crate::DocumentAccessibility>(&bytes).unwrap_or_default()
    }

    /// Persist document-level accessibility settings in the project (`T-117`).
    pub fn set_document_accessibility(&mut self, doc_acc: &crate::DocumentAccessibility) {
        if doc_acc.is_default() {
            self.resources.retain(|r| {
                !r.path
                    .eq_ignore_ascii_case(Self::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH)
            });
            return;
        }
        let Ok(bytes) = serde_json::to_vec(doc_acc) else {
            return;
        };
        let existing_id = self
            .resources
            .iter()
            .find(|r| {
                r.path
                    .eq_ignore_ascii_case(Self::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH)
            })
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let Ok(unit) = crate::ResourceUnit::from_bytes(
            existing_id,
            Self::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH,
            "application/json",
            &bytes,
        ) else {
            return;
        };
        if let Some(slot) = self.resources.iter_mut().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::DOCUMENT_ACCESSIBILITY_RESOURCE_PATH)
        }) {
            *slot = unit;
        } else {
            self.resources.push(unit);
        }
        self.format_version = self.format_version.max(3);
    }

    /// Document-level named global colour swatches linked to elements (T-114).
    pub fn global_swatches(&self) -> Vec<GlobalSwatch> {
        let Some(unit) = self.resources.iter().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::GLOBAL_SWATCHES_RESOURCE_PATH)
        }) else {
            return Vec::new();
        };
        let Ok(bytes) = unit.decoded_bytes() else {
            return Vec::new();
        };
        serde_json::from_slice::<Vec<GlobalSwatch>>(&bytes).unwrap_or_default()
    }

    /// Persist named global colour swatches in the project (T-114).
    pub fn set_global_swatches(&mut self, swatches: &[GlobalSwatch]) {
        if swatches.is_empty() {
            self.resources.retain(|r| {
                !r.path
                    .eq_ignore_ascii_case(Self::GLOBAL_SWATCHES_RESOURCE_PATH)
            });
            return;
        }
        let Ok(bytes) = serde_json::to_vec(swatches) else {
            return;
        };
        let existing_id = self
            .resources
            .iter()
            .find(|r| {
                r.path
                    .eq_ignore_ascii_case(Self::GLOBAL_SWATCHES_RESOURCE_PATH)
            })
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let Ok(unit) = crate::ResourceUnit::from_bytes(
            existing_id,
            Self::GLOBAL_SWATCHES_RESOURCE_PATH,
            "application/json",
            &bytes,
        ) else {
            return;
        };
        if let Some(slot) = self.resources.iter_mut().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::GLOBAL_SWATCHES_RESOURCE_PATH)
        }) {
            *slot = unit;
        } else {
            self.resources.push(unit);
        }
        self.format_version = self.format_version.max(3);
    }

    /// Update a global swatch's colour and propagate the change to all elements and appearance stacks linked to `swatch_id` (T-114).
    pub fn update_global_swatch_color(&mut self, swatch_id: &str, new_color: Color) -> usize {
        let mut swatches = self.global_swatches();
        if let Some(sw) = swatches.iter_mut().find(|s| s.id == swatch_id) {
            sw.color = new_color;
        } else {
            swatches.push(GlobalSwatch {
                id: swatch_id.to_owned(),
                name: swatch_id.to_owned(),
                color: new_color,
            });
        }
        self.set_global_swatches(&swatches);
        let mut updated = 0usize;
        for symbol in &mut self.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    for app in &mut key.appearances {
                        let fill_linked = app.fill_swatch_id.as_deref() == Some(swatch_id);
                        let stroke_linked = app.stroke_swatch_id.as_deref() == Some(swatch_id);
                        if (fill_linked || stroke_linked)
                            && let Some(el) =
                                key.elements.iter_mut().find(|e| e.id == app.element_id)
                        {
                            match &mut el.kind {
                                ElementKind::Rectangle { fill, stroke, .. }
                                | ElementKind::Ellipse { fill, stroke, .. }
                                | ElementKind::Path { fill, stroke, .. }
                                | ElementKind::PaintedPath { fill, stroke, .. } => {
                                    if fill_linked {
                                        *fill = new_color;
                                        updated += 1;
                                    }
                                    if stroke_linked {
                                        *stroke = new_color;
                                        updated += 1;
                                    }
                                }
                                ElementKind::Text { color, .. } if fill_linked => {
                                    *color = new_color;
                                    updated += 1;
                                }
                                _ => {}
                            }
                        }
                        for entry in &mut app.entries {
                            match entry {
                                AppearancePaintEntry::Fill {
                                    color,
                                    swatch_id: sid,
                                    ..
                                }
                                | AppearancePaintEntry::Stroke {
                                    color,
                                    swatch_id: sid,
                                    ..
                                } if sid.as_deref() == Some(swatch_id) => {
                                    *color = new_color;
                                    updated += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        updated
    }

    /// Document-level reusable vector styles (T-114).
    pub fn vector_styles(&self) -> Vec<VectorStyle> {
        let Some(unit) = self.resources.iter().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::VECTOR_STYLES_RESOURCE_PATH)
        }) else {
            return Vec::new();
        };
        let Ok(bytes) = unit.decoded_bytes() else {
            return Vec::new();
        };
        serde_json::from_slice::<Vec<VectorStyle>>(&bytes).unwrap_or_default()
    }

    /// Persist reusable vector styles in the project (T-114).
    pub fn set_vector_styles(&mut self, styles: &[VectorStyle]) {
        if styles.is_empty() {
            self.resources.retain(|r| {
                !r.path
                    .eq_ignore_ascii_case(Self::VECTOR_STYLES_RESOURCE_PATH)
            });
            return;
        }
        let Ok(bytes) = serde_json::to_vec(styles) else {
            return;
        };
        let existing_id = self
            .resources
            .iter()
            .find(|r| {
                r.path
                    .eq_ignore_ascii_case(Self::VECTOR_STYLES_RESOURCE_PATH)
            })
            .map(|r| r.id.clone())
            .unwrap_or_else(|| new_id("resource"));
        let Ok(unit) = crate::ResourceUnit::from_bytes(
            existing_id,
            Self::VECTOR_STYLES_RESOURCE_PATH,
            "application/json",
            &bytes,
        ) else {
            return;
        };
        if let Some(slot) = self.resources.iter_mut().find(|r| {
            r.path
                .eq_ignore_ascii_case(Self::VECTOR_STYLES_RESOURCE_PATH)
        }) {
            *slot = unit;
        } else {
            self.resources.push(unit);
        }
        self.format_version = self.format_version.max(3);
    }

    /// Propagate a named `VectorStyle` to all elements linked to `style_id` across the project (T-114).
    pub fn apply_vector_style_to_project(&mut self, style_id: &str) -> usize {
        let Some(style) = self.vector_styles().into_iter().find(|s| s.id == style_id) else {
            return 0;
        };
        let mut count = 0usize;
        for symbol in &mut self.symbols {
            for layer in &mut symbol.timeline.layers {
                for key in &mut layer.keyframes {
                    for app in &mut key.appearances {
                        if app.style_id.as_deref() == Some(style_id) {
                            app.modifiers = style.modifiers.clone();
                            if let Some(el) =
                                key.elements.iter_mut().find(|e| e.id == app.element_id)
                            {
                                match &mut el.kind {
                                    ElementKind::Rectangle {
                                        fill,
                                        stroke,
                                        stroke_width,
                                        ..
                                    }
                                    | ElementKind::Ellipse {
                                        fill,
                                        stroke,
                                        stroke_width,
                                        ..
                                    }
                                    | ElementKind::Path {
                                        fill,
                                        stroke,
                                        stroke_width,
                                        ..
                                    }
                                    | ElementKind::PaintedPath {
                                        fill,
                                        stroke,
                                        stroke_width,
                                        ..
                                    } => {
                                        *fill = style.fill;
                                        *stroke = style.stroke;
                                        *stroke_width = style.stroke_width;
                                        count += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
        count
    }
}

impl Layer {
    /// Property keyframes left outside a motion tween by an edit become ordinary
    /// keyframes holding their stored pose (they then start their own span).
    pub fn normalize_motion_keys(&mut self) {
        self.keyframes.sort_by_key(|key| key.frame);
        let mut in_motion = false;
        let mut motion_ids = HashSet::new();
        for key in &mut self.keyframes {
            if key.keyed_properties.is_some() && !in_motion {
                key.keyed_properties = None;
                key.motion_keys = None;
            }
            if !(key.tween.is_some() && key.tween_kind == TweenKind::Shape) {
                key.shape_hints.clear();
            }
            if key.keyed_properties.is_none() {
                in_motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
                let in_classic = key.tween.is_some() && key.tween_kind == TweenKind::Classic;
                key.motion_keys = None;
                key.roving = false;
                motion_ids.clear();
                if in_motion {
                    motion_ids.extend(key.elements.iter().map(|element| element.id.clone()));
                    key.motion_tracks
                        .retain(|track| motion_ids.contains(&track.element_id));
                } else {
                    key.motion_tracks.clear();
                }
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
                key.motion_tracks.clear();
                key.property_eases.clear();
                key.motion_path = None;
                key.sync_symbols = false;
                key.snap_to_guide = false;
                key.classic_rotate_turns = 0;
                key.classic_rotate_dir = ClassicRotateDirection::Auto;
                key.tween_scale = true;
                if let Some(keys) = &mut key.motion_keys {
                    let placed: HashSet<_> = key
                        .elements
                        .iter()
                        .map(|element| element.id.as_str())
                        .collect();
                    for object_key in keys.iter_mut() {
                        object_key.properties = TweenProperty::ALL
                            .into_iter()
                            .filter(|property| object_key.properties.contains(property))
                            .collect();
                        if !object_key.properties.contains(&TweenProperty::X)
                            && !object_key.properties.contains(&TweenProperty::Y)
                        {
                            object_key.roving = false;
                        }
                    }
                    keys.retain(|object_key| {
                        !object_key.properties.is_empty()
                            && motion_ids.contains(&object_key.element_id)
                            && placed.contains(object_key.element_id.as_str())
                    });
                    key.roving = false;
                    key.sync_motion_key_union();
                } else {
                    let has_pos = key.keyed_properties.as_ref().is_some_and(|props| {
                        props.contains(&TweenProperty::X) || props.contains(&TweenProperty::Y)
                    });
                    if !has_pos {
                        key.roving = false;
                    }
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

    /// Bake all IK armatures across `start_frame..=end_frame` into ordinary keyframes with solved bone transforms (T-113).
    pub fn bake_ik_span_to_keyframes(&mut self, start_frame: u32, end_frame: u32) -> usize {
        self.bake_ik_span_to_keyframes_at_fps(start_frame, end_frame, 24.0)
    }

    /// Bake IK armatures using the document frame rate for property-expression evaluation.
    pub fn bake_ik_span_to_keyframes_at_fps(
        &mut self,
        start_frame: u32,
        end_frame: u32,
        fps: f64,
    ) -> usize {
        if end_frame < start_frame {
            return 0;
        }
        let mut sampled_frames = Vec::new();
        for f in start_frame..=end_frame {
            let elements = crate::evaluate::sample_layer_at_fps(self, f, fps);
            sampled_frames.push((f, elements));
        }
        let mut baked = 0usize;
        for (f, elements) in sampled_frames {
            if let Some(existing) = self.keyframes.iter_mut().find(|k| k.frame == f) {
                existing.elements = elements;
                existing.tween = None;
                existing.keyed_properties = None;
                existing.ik_armatures.clear();
            } else {
                self.keyframes.push(Keyframe {
                    frame: f,
                    elements,
                    ..Default::default()
                });
            }
            baked += 1;
        }
        self.normalize_motion_keys();
        baked
    }
}
