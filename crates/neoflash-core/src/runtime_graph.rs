//! Runtime identity and dynamic display graph. Authored documents stay immutable.
use crate::runtime_graphics::{GraphicsState, MAX_GRAPHICS_COMMANDS, MAX_GRAPHICS_OPS};
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
mod as2_display;
pub use as2_display::{As2AccProps, As2AccUpdateEntry};
mod as2_input;
mod as2_render;
mod as2_snapshot;
mod bitmap;
pub(crate) mod clipping;
pub use clipping::{
    AlphaClipComponent, ClipComponent, MaskBitmapMapping, MaskResourceBitmap, RuntimeAlphaMask,
    RuntimeClip, ScrollRect,
};
mod filters;
pub use filters::RuntimeFilterGroup;
mod hierarchy;
mod input;
pub use input::DisplayInputState;
mod linked;
mod loader;
pub use loader::{LoaderMutation, MountedPublishResult};
mod text;
mod video;
pub use video::VideoState;
mod transform;
pub use transform::ColorTransform;
mod buttons_hit_timeline;
mod mutations;
mod snapshot;

pub const MAX_DYNAMIC_OBJECTS: usize = 4096;
const MAX_GRAPH_DEPTH: usize = 64;
const MAX_DISPLAY_EVENTS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectRef {
    pub id: String,
    pub generation: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicKind {
    Sprite,
    Shape,
    MovieClip,
    Bitmap,
    TextField,
    Video,
    Loader,
    Button,
    TextLine,
    MorphShape,
    Avm1Movie,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum DisplayMutation {
    Add {
        parent: ObjectRef,
        child: ObjectRef,
        index: Option<usize>,
    },
    Remove {
        parent: ObjectRef,
        child: ObjectRef,
    },
    SetIndex {
        parent: ObjectRef,
        child: ObjectRef,
        index: usize,
    },
    Swap {
        parent: ObjectRef,
        first: ObjectRef,
        second: ObjectRef,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum StageMutation {
    Add {
        child: ObjectRef,
        index: Option<usize>,
    },
    Remove {
        child: ObjectRef,
    },
    SetIndex {
        child: ObjectRef,
        index: usize,
    },
    Swap {
        first: ObjectRef,
        second: ObjectRef,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayEvent {
    pub kind: String,
    pub target_ref: ObjectRef,
    /// Captured target-first route, independent of later mutation/reparenting.
    pub route: Vec<ObjectRef>,
    /// Whether the captured route ends at Stage, which has no graph ObjectRef.
    #[serde(default)]
    pub stage_route: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MutationResult {
    pub events: Vec<DisplayEvent>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeObject {
    #[serde(flatten)]
    pub display: DisplayNode,
    pub object_ref: ObjectRef,
    pub parent_ref: Option<ObjectRef>,
    pub children: Vec<ObjectRef>,
    pub on_stage: bool,
    pub authored_path: Option<Vec<Id>>,
    pub namespace_ref: Option<ObjectRef>,
    pub source_path: Option<Vec<Id>>,
    /// Own Graphics paints; these are not display children.
    pub graphics: Vec<RuntimeDrawKind>,
    /// Button Hit-frame geometry in button-local coordinates; never painted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub button_hits: Vec<DrawItem>,
    /// True when `button_hits` is authoritative (4-frame authored button or dynamic/overridden `hitTestState`, including explicit `null`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub button_hit_authoritative: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fte_draws: Vec<crate::as3_fte_3d::FteDrawRunOut>,
    pub text: Option<TextFieldState>,
    pub video: Option<VideoState>,
    pub input: Option<DisplayInputState>,
    pub bitmap: Option<PixelDescriptor>,
    pub smoothing: bool,
    pub pixel_snapping: PixelSnapping,
    pub local_matrix: [f64; 6],
    /// Local content transform, including the scrollRect origin offset.
    pub content_local_matrix: [f64; 6],
    pub mask_ref: Option<ObjectRef>,
    pub scroll_rect: Option<ScrollRect>,
    pub clip_ids: Vec<String>,
    pub filters: Vec<FilterSpec>,
    pub color_transform: ColorTransform,
    pub concatenated_color_transform: ColorTransform,
    #[serde(skip)]
    pub(crate) authored_mask_source: bool,
}

impl Serialize for RuntimeObject {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let field_count = 35
            + usize::from(self.display.symbol_id.is_some())
            + usize::from(!self.display.blend_mode.is_normal())
            + usize::from(self.display.cache_as_bitmap)
            + usize::from(!self.button_hits.is_empty())
            + usize::from(self.button_hit_authoritative)
            + usize::from(!self.fte_draws.is_empty());
        let mut s = serializer.serialize_struct("RuntimeObject", field_count)?;
        s.serialize_field("path", &self.display.path)?;
        s.serialize_field("lifetime_id", &self.display.lifetime_id)?;
        s.serialize_field("name", &self.display.name)?;
        s.serialize_field("owner", &self.display.owner)?;
        if self.display.symbol_id.is_some() {
            s.serialize_field("symbol_id", &self.display.symbol_id)?;
        }
        s.serialize_field("class_name", &self.display.class_name)?;
        s.serialize_field("transform", &self.display.transform)?;
        s.serialize_field("world", &self.display.world)?;
        s.serialize_field("visible", &self.display.visible)?;
        s.serialize_field("effective_visible", &self.display.effective_visible)?;
        s.serialize_field("frame", &self.display.frame)?;
        s.serialize_field("total_frames", &self.display.total_frames)?;
        s.serialize_field("independent", &self.display.independent)?;
        if !self.display.blend_mode.is_normal() {
            s.serialize_field("blend_mode", &self.display.blend_mode)?;
        }
        if self.display.cache_as_bitmap {
            s.serialize_field("cache_as_bitmap", &self.display.cache_as_bitmap)?;
        }
        s.serialize_field("kind", &self.display.kind)?;
        s.serialize_field("object_ref", &self.object_ref)?;
        s.serialize_field("parent_ref", &self.parent_ref)?;
        s.serialize_field("children", &self.children)?;
        s.serialize_field("on_stage", &self.on_stage)?;
        s.serialize_field("authored_path", &self.authored_path)?;
        s.serialize_field("namespace_ref", &self.namespace_ref)?;
        s.serialize_field("source_path", &self.source_path)?;
        s.serialize_field("graphics", &self.graphics)?;
        if !self.button_hits.is_empty() {
            s.serialize_field("button_hits", &self.button_hits)?;
        }
        if self.button_hit_authoritative {
            s.serialize_field("button_hit_authoritative", &self.button_hit_authoritative)?;
        }
        if !self.fte_draws.is_empty() {
            s.serialize_field("fte_draws", &self.fte_draws)?;
        }
        s.serialize_field("text", &self.text)?;
        s.serialize_field("video", &self.video)?;
        s.serialize_field("input", &self.input)?;
        s.serialize_field("bitmap", &self.bitmap)?;
        s.serialize_field("smoothing", &self.smoothing)?;
        s.serialize_field("pixel_snapping", &self.pixel_snapping)?;
        s.serialize_field("local_matrix", &self.local_matrix)?;
        s.serialize_field("content_local_matrix", &self.content_local_matrix)?;
        s.serialize_field("mask_ref", &self.mask_ref)?;
        s.serialize_field("scroll_rect", &self.scroll_rect)?;
        s.serialize_field("clip_ids", &self.clip_ids)?;
        s.serialize_field("filters", &self.filters)?;
        s.serialize_field("color_transform", &self.color_transform)?;
        s.serialize_field(
            "concatenated_color_transform",
            &self.concatenated_color_transform,
        )?;
        s.end()
    }
}

/// Runtime pixels have their own identity; authoring resource IDs remain immutable.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeDrawKind {
    VideoFrame {
        width: f64,
        height: f64,
        smoothing: bool,
        color_transform: ColorTransform,
    },
    GradientPath {
        commands: Vec<PathCommand>,
        fill_rule: FillRule,
        fill: Color,
        stroke: Color,
        fill_gradient: Option<GradientPaint>,
        stroke_gradient: Option<GradientPaint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill_bitmap: Option<crate::BitmapPaint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke_bitmap: Option<Box<crate::BitmapPaint>>,
        stroke_width: f64,
        #[serde(default, skip_serializing_if = "LineCaps::is_round")]
        stroke_caps: LineCaps,
        #[serde(default, skip_serializing_if = "LineJoints::is_round")]
        stroke_joints: LineJoints,
        #[serde(
            default = "default_miter_limit",
            skip_serializing_if = "is_default_miter_limit"
        )]
        stroke_miter_limit: f64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stroke_pixel_hinting: bool,
        #[serde(default, skip_serializing_if = "LineScaleMode::is_normal")]
        stroke_scale_mode: LineScaleMode,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stroke_hairline: bool,
    },
    TextField {
        field: Box<TextFieldState>,
    },
    BitmapPixels {
        pixel_ref: PixelRef,
        width: u32,
        height: u32,
        revision: String,
        smoothing: bool,
        pixel_snapping: PixelSnapping,
        color_transform: ColorTransform,
    },
    #[serde(untagged)]
    Element(ElementKind),
}
impl PartialEq<ElementKind> for RuntimeDrawKind {
    fn eq(&self, other: &ElementKind) -> bool {
        match self {
            Self::Element(kind) => kind == other,
            Self::TextField { field } => {
                TextFieldState::from_kind(other).as_ref() == Some(field.as_ref())
            }
            _ => false,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeDrawItem {
    #[serde(flatten)]
    pub item: DrawItem<RuntimeDrawKind>,
    pub owner_ref: ObjectRef,
    pub clip_ids: Vec<String>,
    /// Innermost filtered subtree. Parent links live once in filter_groups.
    pub filter_id: Option<String>,
    /// True when emitted by runtime `MovieClip`/`Shape` `Graphics` drawing commands.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub procedural_graphics: bool,
}

impl Serialize for RuntimeDrawItem {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let field_count = 10
            + usize::from(!self.item.color_transform.is_identity())
            + usize::from(self.item.bitmap_fill.is_some())
            + usize::from(self.procedural_graphics);
        let mut s = serializer.serialize_struct("RuntimeDrawItem", field_count)?;
        s.serialize_field("element_id", &self.item.element_id)?;
        s.serialize_field("name", &self.item.name)?;
        s.serialize_field("instance_path", &self.item.instance_path)?;
        s.serialize_field("layer_id", &self.item.layer_id)?;
        s.serialize_field("transform", &self.item.transform)?;
        s.serialize_field("alpha", &self.item.alpha)?;
        if !self.item.color_transform.is_identity() {
            s.serialize_field("color_transform", &self.item.color_transform)?;
        }
        if self.item.bitmap_fill.is_some() {
            s.serialize_field("bitmap_fill", &self.item.bitmap_fill)?;
        }
        s.serialize_field("kind", &self.item.kind)?;
        s.serialize_field("owner_ref", &self.owner_ref)?;
        s.serialize_field("clip_ids", &self.clip_ids)?;
        s.serialize_field("filter_id", &self.filter_id)?;
        if self.procedural_graphics {
            s.serialize_field("procedural_graphics", &self.procedural_graphics)?;
        }
        s.end()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeAccessibleNode {
    #[serde(flatten)]
    pub node: AccessibleNode,
    pub owner_ref: ObjectRef,
    pub clip_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeScriptDispatch {
    #[serde(flatten)]
    pub dispatch: ScriptDispatch,
    pub target_ref: ObjectRef,
    pub namespace_ref: Option<ObjectRef>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub objects: Vec<RuntimeObject>,
    pub draws: Vec<RuntimeDrawItem>,
    pub semantics: Vec<RuntimeAccessibleNode>,
    pub clips: Vec<RuntimeClip>,
    pub filter_groups: Vec<RuntimeFilterGroup>,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq)]
#[error("{0}")]
pub struct DisplayError(pub String);
fn error(message: &str) -> DisplayError {
    DisplayError(message.into())
}

#[derive(Clone, Debug)]
pub(crate) struct AuthoredPlacement {
    pub path: Vec<Id>,
    pub lifetime: u64,
    pub blend_mode: BlendMode,
    pub cache_as_bitmap: bool,
    pub filters: Vec<FilterSpec>,
}

#[derive(Clone, Debug)]
struct Authored {
    reference: ObjectRef,
    lifetime: u64,
}
#[derive(Clone, Debug)]
struct DynamicObject {
    kind: DynamicKind,
    parent: Option<ObjectRef>,
    transform: Transform,
    visible: bool,
    name: String,
    playing: bool,
    graphics: GraphicsState,
    authored_kind: Option<ElementKind>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ButtonRuntimeState {
    pub active_state: u8,
    pub slots: [Option<Option<ObjectRef>>; 4],
    pub sound_transform: Option<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SourceAddress {
    namespace: ObjectRef,
    path: Vec<Id>,
}
struct LinkedNamespace {
    player: Box<Player>,
    nodes: usize,
    depth: usize,
    mounted_prefix: Option<String>,
}

#[derive(Default)]
pub(crate) struct RuntimeGraph {
    generation: u64,
    next_id: u64,
    retired_published_prefixes: BTreeSet<String>,
    pixels: crate::pixels::PixelArena,
    resource_pixels: HashMap<Id, PixelRef>,
    hydrated_bitmaps: HashMap<Id, crate::resource_image::DecodedImage>,
    bitmap_states: HashMap<ObjectRef, bitmap::BitmapState>,
    bitmap_error: Option<BitmapError>,
    text_states: HashMap<ObjectRef, TextFieldState>,
    fte_draws: HashMap<ObjectRef, Vec<crate::as3_fte_3d::FteDrawRunOut>>,
    video_states: HashMap<ObjectRef, VideoState>,
    input_states: HashMap<ObjectRef, DisplayInputState>,
    pub(crate) button_states: BTreeMap<ObjectRef, ButtonRuntimeState>,
    movieclip_pointer: input::MovieClipPointer,
    text_bytes: usize,
    affines: HashMap<ObjectRef, transform::AffineState>,
    color_transforms: HashMap<ObjectRef, ColorTransform>,
    filters: BTreeMap<ObjectRef, Vec<FilterSpec>>,
    filters_overridden: BTreeSet<ObjectRef>,
    blend_overridden: BTreeSet<ObjectRef>,
    cache_overridden: BTreeSet<ObjectRef>,
    loaders: BTreeMap<ObjectRef, loader::LoaderState>,
    masks: BTreeMap<ObjectRef, ObjectRef>,
    scroll_rects: BTreeMap<ObjectRef, ScrollRect>,
    authored: BTreeMap<Vec<Id>, Authored>,
    paths: HashMap<ObjectRef, Vec<Id>>,
    prefix: HashMap<ObjectRef, Vec<ObjectRef>>,
    suffix: HashMap<ObjectRef, Vec<ObjectRef>>,
    parent_overrides: HashMap<ObjectRef, Option<ObjectRef>>,
    child_order: HashMap<ObjectRef, Vec<ObjectRef>>,
    stage_order: Vec<ObjectRef>,
    dynamic: BTreeMap<ObjectRef, DynamicObject>,
    as2_depths: HashMap<ObjectRef, (ObjectRef, i32)>,
    as2_settings: as2_input::As2Settings,
    as2_input_states: HashMap<ObjectRef, as2_input::As2Input>,
    as2_accessibility: BTreeMap<ObjectRef, as2_display::As2AccProps>,
    as2_render_states: HashMap<ObjectRef, as2_render::As2Render>,
    as2_snapshot_next: u64,
    as2_snapshots: BTreeMap<String, as2_snapshot::TextSnapshot>,
    as2_snapshot_selection: HashMap<ObjectRef, String>,
    events: Vec<DisplayEvent>,
    events_overflowed: bool,
    omit_authored_events: bool,
    lifecycle_mode: bool,
    graphics_cost: usize,
    namespaces: BTreeMap<ObjectRef, LinkedNamespace>,
    linked_authored: BTreeMap<SourceAddress, Authored>,
    linked_paths: HashMap<ObjectRef, SourceAddress>,
}

impl RuntimeGraph {
    pub(crate) fn reset(&mut self) {
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
        self.generation = NEXT_SESSION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("runtime generation exhausted");
        self.pixels.reset();
        self.resource_pixels.clear();
        self.bitmap_states.clear();
        self.bitmap_error = None;
        self.text_states.clear();
        self.fte_draws.clear();
        self.video_states.clear();
        self.input_states.clear();
        self.button_states.clear();
        self.movieclip_pointer = input::MovieClipPointer::default();
        self.text_bytes = 0;
        self.affines.clear();
        self.color_transforms.clear();
        self.filters.clear();
        self.filters_overridden.clear();
        self.blend_overridden.clear();
        self.cache_overridden.clear();
        self.loaders.clear();
        self.masks.clear();
        self.scroll_rects.clear();
        self.authored.clear();
        self.paths.clear();
        self.prefix.clear();
        self.suffix.clear();
        self.parent_overrides.clear();
        self.child_order.clear();
        self.stage_order.clear();
        self.dynamic.clear();
        self.as2_depths.clear();
        self.as2_settings = as2_input::As2Settings::default();
        self.as2_input_states.clear();
        self.as2_accessibility.clear();
        self.as2_render_states.clear();
        self.as2_snapshots.clear();
        self.as2_snapshot_selection.clear();
        self.events.clear();
        self.events_overflowed = false;
        self.graphics_cost = 0;
        self.namespaces.clear();
        self.retired_published_prefixes.clear();
        self.linked_authored.clear();
        self.linked_paths.clear();
    }
    fn allocate(&mut self) -> ObjectRef {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("runtime object identity exhausted");
        ObjectRef {
            id: self.next_id.to_string(),
            generation: self.generation.to_string(),
        }
    }
    pub(crate) fn reconcile(
        &mut self,
        placements: &[AuthoredPlacement],
    ) -> Vec<crate::SoundCommand> {
        let active: HashMap<_, _> = placements
            .iter()
            .map(|p| (p.path.as_slice(), p.lifetime))
            .collect();
        let retired_info: Vec<_> = {
            let mut list: Vec<_> = self
                .authored
                .iter()
                .filter(|(path, node)| active.get(path.as_slice()) != Some(&node.lifetime))
                .map(|(path, node)| {
                    (
                        path.clone(),
                        node.reference.clone(),
                        if self.omit_authored_events {
                            Vec::new()
                        } else {
                            self.route(&node.reference)
                        },
                        !self.omit_authored_events && self.on_stage(&node.reference),
                    )
                })
                .collect();
            // Emit retired children before retiring parents.
            list.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
            list
        };
        let retired: BTreeSet<_> = retired_info
            .iter()
            .map(|(_, reference, _, _)| reference.clone())
            .collect();
        let retired_sound_commands = self.retire_loader_children(&retired);
        self.retire_hierarchy(&retired);
        for (_, reference, route, was_on_stage) in retired_info {
            self.push_timeline_event(DisplayEvent {
                kind: "removed".into(),
                stage_route: was_on_stage,
                target_ref: reference.clone(),
                route: route.clone(),
            });
            if was_on_stage {
                self.push_timeline_event(DisplayEvent {
                    kind: "removed_from_stage".into(),
                    stage_route: true,
                    target_ref: reference,
                    route,
                });
            }
        }
        self.authored
            .retain(|path, node| active.get(path.as_slice()) == Some(&node.lifetime));
        self.paths
            .retain(|reference, _| !retired.contains(reference));
        self.bitmap_states
            .retain(|reference, _| !retired.contains(reference));
        self.fte_draws
            .retain(|reference, _| !retired.contains(reference));
        self.retire_text(&retired);
        self.retire_video(&retired);
        self.retire_input(&retired);
        self.retire_filters(&retired);
        self.retire_clipping(&retired);
        self.affines
            .retain(|reference, _| !retired.contains(reference));
        self.color_transforms
            .retain(|reference, _| !retired.contains(reference));
        let mut added_paths = Vec::new();
        for p in placements {
            if !self.authored.contains_key(&p.path) {
                let reference = self.allocate();
                self.paths.insert(reference.clone(), p.path.clone());
                self.authored.insert(
                    p.path.clone(),
                    Authored {
                        reference,
                        lifetime: p.lifetime,
                    },
                );
                added_paths.push(p.path.clone());
            }
            let reference = self.authored[&p.path].reference.clone();
            self.sync_authored_render(&reference, p.blend_mode, p.cache_as_bitmap, &p.filters);
        }
        self.prefix.retain(|reference, _| {
            !self.paths.contains_key(reference) || self.namespaces.contains_key(reference)
        });
        for p in placements {
            if !p.path.is_empty() {
                let parent = &self.authored[&p.path[..p.path.len() - 1]].reference;
                self.prefix
                    .entry(parent.clone())
                    .or_default()
                    .push(self.authored[&p.path].reference.clone());
            }
        }
        for path in added_paths {
            let Some(node) = self.authored.get(&path) else {
                continue;
            };
            let reference = node.reference.clone();
            if self.omit_authored_events {
                continue;
            }
            let route = self.route(&reference);
            let on_stage = self.on_stage(&reference);
            self.push_timeline_event(DisplayEvent {
                kind: "added".into(),
                stage_route: on_stage,
                target_ref: reference.clone(),
                route: route.clone(),
            });
            if on_stage {
                self.push_timeline_event(DisplayEvent {
                    kind: "added_to_stage".into(),
                    stage_route: true,
                    target_ref: reference,
                    route,
                });
            }
        }
        retired_sound_commands
    }
    pub(crate) fn set_lifecycle_mode(&mut self, enabled: bool) {
        self.lifecycle_mode = enabled;
    }
    pub(crate) fn events_overflowed_flag(&self) -> bool {
        self.events_overflowed
    }
    pub(crate) fn take_lifecycle_events(&mut self) -> Vec<DisplayEvent> {
        std::mem::take(&mut self.events)
    }
    pub(crate) fn prepend_lifecycle_events(&mut self, mut prior: Vec<DisplayEvent>) {
        if prior.is_empty() {
            return;
        }
        prior.extend(std::mem::take(&mut self.events));
        self.events = prior;
    }
    pub(crate) fn push_timeline_event(&mut self, event: DisplayEvent) {
        if self.omit_authored_events {
            return;
        }
        if self.events.len() < MAX_DISPLAY_EVENTS {
            self.events.push(event);
        } else if self.lifecycle_mode {
            self.events_overflowed = true;
        }
    }
    pub(crate) fn object_tree_depth(&self, target: &ObjectRef) -> (bool, usize) {
        let mut depth = 0usize;
        let mut current = target.clone();
        while let Some(parent) = self.parent(&current) {
            depth += 1;
            if depth > MAX_GRAPH_DEPTH {
                break;
            }
            current = parent;
        }
        (!self.on_stage(target), depth)
    }
    fn alive(&self, id: &ObjectRef) -> bool {
        self.paths.contains_key(id)
            || self.dynamic.contains_key(id)
            || self.linked_paths.contains_key(id)
    }
    fn active_button_slot_overridden(&self, id: &ObjectRef) -> bool {
        let Some(state) = self.button_states.get(id) else {
            return false;
        };
        let idx = (state.active_state as usize).min(2);
        state.slots[idx].is_some()
    }
    fn active_button_visual(&self, id: &ObjectRef) -> Option<ObjectRef> {
        let state = self.button_states.get(id)?;
        let idx = (state.active_state as usize).min(2);
        let child = state.slots[idx].as_ref()?.as_ref()?.clone();
        self.alive(&child).then_some(child)
    }
    fn active_button_parent(&self, id: &ObjectRef) -> Option<ObjectRef> {
        self.button_states.iter().find_map(|(btn, _)| {
            (self.active_button_visual(btn).as_ref() == Some(id)).then(|| btn.clone())
        })
    }
    fn parent(&self, id: &ObjectRef) -> Option<ObjectRef> {
        if let Some(btn) = self.active_button_parent(id) {
            return Some(btn);
        }
        if let Some(override_parent) = self.parent_overrides.get(id) {
            return override_parent.clone();
        }
        let src = self.source_parent(id)?;
        if self.active_button_slot_overridden(&src) {
            return None;
        }
        Some(src)
    }
    fn source_parent(&self, id: &ObjectRef) -> Option<ObjectRef> {
        if let Some(dynamic) = self.dynamic.get(id) {
            return dynamic.parent.clone();
        }
        if let Some(address) = self.linked_paths.get(id) {
            if address.path.len() == 1 {
                return Some(address.namespace.clone());
            }
            return self
                .linked_authored
                .get(&SourceAddress {
                    namespace: address.namespace.clone(),
                    path: address.path[..address.path.len() - 1].into(),
                })
                .map(|node| node.reference.clone());
        }
        let path = self.paths.get(id)?;
        if path.is_empty() {
            None
        } else {
            self.authored
                .get(&path[..path.len() - 1])
                .map(|a| a.reference.clone())
        }
    }
    fn children(&self, id: &ObjectRef) -> Vec<ObjectRef> {
        let mut seen = BTreeSet::new();
        let active_visual = self.active_button_visual(id);
        let prefix_children = (!self.active_button_slot_overridden(id))
            .then(|| self.prefix.get(id))
            .flatten();
        self.child_order
            .get(id)
            .into_iter()
            .flatten()
            .chain(prefix_children.into_iter().flatten())
            .chain(active_visual.iter())
            .chain(self.suffix.get(id).into_iter().flatten())
            .filter(|child| {
                self.alive(child)
                    && self.parent(child).as_ref() == Some(id)
                    && seen.insert((*child).clone())
            })
            .cloned()
            .collect()
    }
    fn route(&self, id: &ObjectRef) -> Vec<ObjectRef> {
        let mut route = vec![];
        let mut current = Some(id.clone());
        while let Some(reference) = current {
            current = self.parent(&reference);
            route.push(reference);
        }
        route
    }
    fn is_stage_root(&self, root: &ObjectRef) -> bool {
        self.paths.get(root).is_some_and(Vec::is_empty)
            || (self.alive(root) && self.parent(root).is_none() && self.stage_order.contains(root))
    }
    fn stage_children(&self) -> Vec<ObjectRef> {
        let root = self
            .authored
            .get(&[][..])
            .map(|node| node.reference.clone());
        let mut seen = BTreeSet::new();
        let mut out: Vec<ObjectRef> = self
            .stage_order
            .iter()
            .filter(|id| self.alive(id) && self.parent(id).is_none() && seen.insert((*id).clone()))
            .cloned()
            .collect();
        if let Some(root) = root
            && !seen.contains(&root)
        {
            out.insert(0, root);
        }
        out
    }
    fn on_stage(&self, id: &ObjectRef) -> bool {
        self.route(id)
            .last()
            .is_some_and(|root| self.is_stage_root(root))
    }
    fn subtree(&self, id: &ObjectRef) -> Vec<ObjectRef> {
        let mut result = vec![];
        let mut pending = vec![id.clone()];
        while let Some(reference) = pending.pop() {
            pending.extend(self.children(&reference).into_iter().rev());
            result.push(reference);
        }
        result
    }
}

impl Player {
    /// A host rendering a document without scripts may skip unobserved authored placement events.
    /// Host-driven display mutations still return their own event packets.
    pub fn set_authored_display_events(&mut self, observe: bool) {
        self.graph.omit_authored_events = !observe;
        if !observe {
            self.graph.events.clear();
            self.graph.events_overflowed = false;
        }
    }

    pub fn object_ref(&self, path: &[Id]) -> Option<ObjectRef> {
        self.graph
            .authored
            .get(path)
            .map(|node| node.reference.clone())
    }
    pub fn object_alive(&self, reference: &ObjectRef) -> bool {
        self.graph.alive(reference)
    }
    pub fn create_display(&mut self, kind: DynamicKind) -> Result<ObjectRef, DisplayError> {
        if self.graph.dynamic.len() >= MAX_DYNAMIC_OBJECTS {
            return Err(error(
                "Dynamic display object limit reached (4096, including detached objects)",
            ));
        }
        let reference = self.graph.allocate();
        self.graph.dynamic.insert(
            reference.clone(),
            DynamicObject {
                kind,
                parent: None,
                transform: Transform::default(),
                visible: true,
                name: format!("instance{}", reference.id),
                playing: true,
                graphics: GraphicsState::default(),
                authored_kind: None,
            },
        );
        if kind == DynamicKind::Video {
            self.graph
                .video_states
                .insert(reference.clone(), VideoState::default());
        }
        if kind == DynamicKind::TextField {
            self.graph
                .text_states
                .insert(reference.clone(), TextFieldState::default());
        }
        Ok(reference)
    }
    pub fn set_text_line_draws(
        &mut self,
        reference: &ObjectRef,
        runs: Vec<crate::as3_fte_3d::FteDrawRunOut>,
    ) -> Result<(), DisplayError> {
        let Some(node) = self.graph.dynamic.get(reference) else {
            return Err(error(
                "TextLine display reference is stale or belongs to another player",
            ));
        };
        if node.kind != DynamicKind::TextLine {
            return Err(error(
                "set_text_line_draws requires a TextLine display node",
            ));
        }
        self.graph.fte_draws.insert(reference.clone(), runs);
        Ok(())
    }
    pub fn drain_display_events(&mut self) -> Result<MutationResult, DisplayError> {
        let events = std::mem::take(&mut self.graph.events);
        if std::mem::take(&mut self.graph.events_overflowed) {
            Err(error(
                "Authored retirement exceeded the pending display lifecycle event budget",
            ))
        } else {
            Ok(MutationResult { events })
        }
    }

    pub(crate) fn object_owner(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Option<&'static str> {
        if !self.graph.alive(reference) {
            return None;
        }
        if let Some(path) = self.graph.paths.get(reference) {
            let node = self.previous.get(path)?;
            return Some(Self::node_owner(project, node));
        }
        if let Some(address) = self.graph.linked_paths.get(reference) {
            let scope = self.graph.namespaces.get(&address.namespace)?;
            let node = scope.player.previous.get(&address.path)?;
            return Some(Self::node_owner(project, node));
        }
        if let Some(node) = self.graph.dynamic.get(reference) {
            if let Some(scope) = self.graph.namespaces.get(reference)
                && let Some(root_node) = scope.player.previous.get(&[][..])
            {
                return Some(Self::node_owner(project, root_node));
            }
            return Some(match node.kind {
                DynamicKind::Sprite => "sprite",
                DynamicKind::Shape => "shape",
                DynamicKind::MovieClip => "movie_clip",
                DynamicKind::Bitmap => "bitmap",
                DynamicKind::TextField => "text",
                DynamicKind::Video => "video",
                DynamicKind::Loader => "loader",
                DynamicKind::Button => "button",
                DynamicKind::TextLine => "text_line",
                DynamicKind::MorphShape => "morph_shape",
                DynamicKind::Avm1Movie => "avm1_movie",
            });
        }
        None
    }

    pub(crate) fn object_name(&self, reference: &ObjectRef) -> Option<String> {
        if !self.graph.alive(reference) {
            return None;
        }
        if let Some(path) = self.graph.paths.get(reference) {
            let node = self.previous.get(path)?;
            return Some(
                self.overrides
                    .get(path)
                    .and_then(|p| p.name.clone())
                    .unwrap_or_else(|| node.name.clone()),
            );
        }
        if let Some(address) = self.graph.linked_paths.get(reference) {
            let scope = self.graph.namespaces.get(&address.namespace)?;
            let node = scope.player.previous.get(&address.path)?;
            return Some(
                scope
                    .player
                    .overrides
                    .get(&address.path)
                    .and_then(|p| p.name.clone())
                    .unwrap_or_else(|| node.name.clone()),
            );
        }
        if let Some(node) = self.graph.dynamic.get(reference) {
            return Some(node.name.clone());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_runtime_snapshot_serialize_matches_flattened_wire_contract_and_roundtrips() {
        #[derive(Serialize)]
        struct RefDraw<'a> {
            #[serde(flatten)]
            item: &'a DrawItem<RuntimeDrawKind>,
            owner_ref: &'a ObjectRef,
            clip_ids: &'a [String],
            filter_id: &'a Option<String>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            procedural_graphics: bool,
        }

        let owner = ObjectRef {
            id: "d1".into(),
            generation: "0".into(),
        };
        let draw = RuntimeDrawItem {
            item: DrawItem {
                element_id: "e1".into(),
                name: "box".into(),
                instance_path: vec!["e1".into()],
                layer_id: "l1".into(),
                transform: [1.0, 0.0, 0.0, 1.0, 10.0, 20.0],
                alpha: 0.75,
                color_transform: ColorTransform::default(),
                bitmap_fill: None,
                path_fingerprint: None,
                kind: RuntimeDrawKind::Element(ElementKind::Rectangle {
                    width: 32.0,
                    height: 16.0,
                    fill: [255, 0, 128, 255],
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                }),
            },
            owner_ref: owner.clone(),
            clip_ids: vec![],
            filter_id: None,
            procedural_graphics: true,
        };
        let expected_draw = serde_json::to_value(RefDraw {
            item: &draw.item,
            owner_ref: &draw.owner_ref,
            clip_ids: &draw.clip_ids,
            filter_id: &draw.filter_id,
            procedural_graphics: draw.procedural_graphics,
        })
        .expect("serialize reference draw");
        let actual_draw = serde_json::to_value(&draw).expect("serialize streaming draw");
        assert_eq!(actual_draw, expected_draw);

        let filter = FilterSpec::Blur {
            blur_x: 4.0,
            blur_y: 4.0,
            quality: 1,
        };
        let obj = RuntimeObject {
            display: DisplayNode {
                path: vec![],
                lifetime_id: "d1".into(),
                name: "sprite".into(),
                owner: "sprite".into(),
                symbol_id: Some("sym1".into()),
                class_name: None,
                transform: Transform::default(),
                world: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                visible: true,
                effective_visible: true,
                frame: 0,
                total_frames: 0,
                independent: false,
                blend_mode: BlendMode::Multiply,
                cache_as_bitmap: true,
                filters: vec![filter.clone()],
                kind: None,
            },
            object_ref: owner.clone(),
            parent_ref: None,
            children: vec![],
            on_stage: true,
            authored_path: None,
            namespace_ref: None,
            source_path: None,
            graphics: vec![],
            button_hits: vec![],
            button_hit_authoritative: false,
            fte_draws: vec![],
            text: None,
            video: None,
            input: None,
            bitmap: None,
            smoothing: false,
            pixel_snapping: PixelSnapping::Auto,
            local_matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            content_local_matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            mask_ref: None,
            scroll_rect: None,
            clip_ids: vec![],
            filters: vec![filter],
            color_transform: ColorTransform::default(),
            concatenated_color_transform: ColorTransform::default(),
            authored_mask_source: false,
        };
        let snap = RuntimeSnapshot {
            objects: vec![obj],
            draws: vec![draw],
            semantics: vec![],
            clips: vec![],
            filter_groups: vec![],
        };
        let json = serde_json::to_string(&snap).expect("serialize snapshot");
        let decoded: RuntimeSnapshot =
            serde_json::from_str(&json).expect("roundtrip snapshot without duplicate filters");
        assert_eq!(decoded.objects.len(), 1);
        assert_eq!(decoded.objects[0].filters.len(), 1);
        assert_eq!(decoded.draws.len(), 1);
        assert!(decoded.draws[0].procedural_graphics);
    }
}
