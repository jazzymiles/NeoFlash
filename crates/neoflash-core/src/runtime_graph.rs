//! Runtime identity and dynamic display graph. Authored documents stay immutable.
use crate::runtime_graphics::{GraphicsState, MAX_GRAPHICS_COMMANDS, MAX_GRAPHICS_OPS};
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
mod as2_display;
mod as2_input;
mod as2_render;
mod as2_snapshot;
mod bitmap;
pub(crate) mod clipping;
pub use clipping::{ClipComponent, RuntimeClip, ScrollRect};
mod filters;
pub use filters::RuntimeFilterGroup;
mod hierarchy;
mod input;
pub use input::DisplayInputState;
mod linked;
mod loader;
pub use loader::LoaderMutation;
mod text;
mod video;
pub use video::VideoState;
mod transform;
pub use transform::ColorTransform;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayEvent {
    pub kind: String,
    pub target_ref: ObjectRef,
    /// Captured target-first route, independent of later mutation/reparenting.
    pub route: Vec<ObjectRef>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MutationResult {
    pub events: Vec<DisplayEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

/// Runtime pixels have their own identity; authoring resource IDs remain immutable.
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
        stroke_width: f64,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeDrawItem {
    #[serde(flatten)]
    pub item: DrawItem<RuntimeDrawKind>,
    pub owner_ref: ObjectRef,
    pub clip_ids: Vec<String>,
    /// Innermost filtered subtree. Parent links live once in filter_groups.
    pub filter_id: Option<String>,
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
}

#[derive(Default)]
pub(crate) struct RuntimeGraph {
    generation: u64,
    next_id: u64,
    pixels: crate::pixels::PixelArena,
    resource_pixels: HashMap<Id, PixelRef>,
    bitmap_states: HashMap<ObjectRef, bitmap::BitmapState>,
    bitmap_error: Option<BitmapError>,
    text_states: HashMap<ObjectRef, TextFieldState>,
    video_states: HashMap<ObjectRef, VideoState>,
    input_states: HashMap<ObjectRef, DisplayInputState>,
    movieclip_pointer: input::MovieClipPointer,
    text_bytes: usize,
    affines: HashMap<ObjectRef, transform::AffineState>,
    color_transforms: HashMap<ObjectRef, ColorTransform>,
    filters: BTreeMap<ObjectRef, Vec<FilterSpec>>,
    loaders: BTreeMap<ObjectRef, loader::LoaderState>,
    masks: BTreeMap<ObjectRef, ObjectRef>,
    scroll_rects: BTreeMap<ObjectRef, ScrollRect>,
    authored: BTreeMap<Vec<Id>, Authored>,
    paths: HashMap<ObjectRef, Vec<Id>>,
    prefix: HashMap<ObjectRef, Vec<ObjectRef>>,
    suffix: HashMap<ObjectRef, Vec<ObjectRef>>,
    parent_overrides: HashMap<ObjectRef, Option<ObjectRef>>,
    child_order: HashMap<ObjectRef, Vec<ObjectRef>>,
    dynamic: BTreeMap<ObjectRef, DynamicObject>,
    as2_depths: HashMap<ObjectRef, (ObjectRef, i32)>,
    as2_settings: as2_input::As2Settings,
    as2_input_states: HashMap<ObjectRef, as2_input::As2Input>,
    as2_render_states: HashMap<ObjectRef, as2_render::As2Render>,
    as2_snapshot_next: u64,
    as2_snapshots: BTreeMap<String, as2_snapshot::TextSnapshot>,
    as2_snapshot_selection: HashMap<ObjectRef, String>,
    events: Vec<DisplayEvent>,
    events_overflowed: bool,
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
        self.video_states.clear();
        self.input_states.clear();
        self.movieclip_pointer = input::MovieClipPointer::default();
        self.text_bytes = 0;
        self.affines.clear();
        self.color_transforms.clear();
        self.filters.clear();
        self.loaders.clear();
        self.masks.clear();
        self.scroll_rects.clear();
        self.authored.clear();
        self.paths.clear();
        self.prefix.clear();
        self.suffix.clear();
        self.parent_overrides.clear();
        self.child_order.clear();
        self.dynamic.clear();
        self.as2_depths.clear();
        self.as2_settings = as2_input::As2Settings::default();
        self.as2_input_states.clear();
        self.as2_render_states.clear();
        self.as2_snapshots.clear();
        self.as2_snapshot_selection.clear();
        self.events.clear();
        self.events_overflowed = false;
        self.graphics_cost = 0;
        self.namespaces.clear();
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
    pub(crate) fn reconcile(&mut self, placements: &[(Vec<Id>, u64)]) {
        let active: HashMap<_, _> = placements.iter().map(|(p, l)| (p.as_slice(), *l)).collect();
        let retired: BTreeSet<_> = self
            .authored
            .iter()
            .filter(|(path, node)| active.get(path.as_slice()) != Some(&node.lifetime))
            .map(|(_, node)| node.reference.clone())
            .collect();
        self.retire_loader_children(&retired);
        self.retire_hierarchy(&retired);
        self.authored
            .retain(|path, node| active.get(path.as_slice()) == Some(&node.lifetime));
        self.paths
            .retain(|reference, _| !retired.contains(reference));
        self.bitmap_states
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
        for (path, lifetime) in placements {
            if !self.authored.contains_key(path) {
                let reference = self.allocate();
                self.paths.insert(reference.clone(), path.clone());
                self.authored.insert(
                    path.clone(),
                    Authored {
                        reference,
                        lifetime: *lifetime,
                    },
                );
            }
        }
        self.prefix
            .retain(|reference, _| !self.paths.contains_key(reference));
        for (path, _) in placements {
            if !path.is_empty() {
                let parent = &self.authored[&path[..path.len() - 1]].reference;
                self.prefix
                    .entry(parent.clone())
                    .or_default()
                    .push(self.authored[path].reference.clone());
            }
        }
    }
    fn alive(&self, id: &ObjectRef) -> bool {
        self.paths.contains_key(id)
            || self.dynamic.contains_key(id)
            || self.linked_paths.contains_key(id)
    }
    fn parent(&self, id: &ObjectRef) -> Option<ObjectRef> {
        self.parent_overrides
            .get(id)
            .cloned()
            .unwrap_or_else(|| self.source_parent(id))
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
        self.child_order
            .get(id)
            .into_iter()
            .flatten()
            .chain(self.prefix.get(id).into_iter().flatten())
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
    fn on_stage(&self, id: &ObjectRef) -> bool {
        self.route(id)
            .last()
            .is_some_and(|root| self.paths.get(root).is_some_and(Vec::is_empty))
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

    pub fn runtime_snapshot(&self, project: &Project) -> RuntimeSnapshot {
        self.snapshot_with_root(project, None)
    }

    /// Capture the current display subtree using the normal scene evaluator.
    /// The source placement/ancestor transforms are ignored. No timelines,
    /// scripts, references, buffers or display properties are mutated.
    pub fn bitmap_draw_snapshot(
        &self,
        project: &Project,
        source: &ObjectRef,
        matrix: [f64; 6],
        color: ColorTransform,
    ) -> Result<RuntimeSnapshot, BitmapError> {
        if !self.graph.alive(source) {
            return Err(BitmapError("draw source is stale or foreign".into()));
        }
        if !matrix
            .into_iter()
            .enumerate()
            .all(|(i, v)| v.is_finite() && v.abs() <= if i < 4 { 1e4 } else { 1e12 })
        {
            return Err(BitmapError(
                "draw matrix exceeds finite affine bounds".into(),
            ));
        }
        color.validate().map_err(|e| BitmapError(e.to_string()))?;
        let included: BTreeSet<_> = self.graph.subtree(source).into_iter().collect();
        if self
            .graph
            .masks
            .iter()
            .any(|(target, mask)| included.contains(target) && !included.contains(mask))
        {
            return Err(BitmapError("draw currently requires mask sources inside the captured subtree; external mask dependencies are unsupported".into()));
        }
        let result = self.snapshot_with_root(project, Some((source, matrix, color)));
        if result
            .objects
            .iter()
            .any(|node| node.bitmap.as_ref().is_some_and(|b| b.disposed))
        {
            return Err(BitmapError(
                "draw subtree contains a disposed BitmapData".into(),
            ));
        }
        Ok(result)
    }

    fn snapshot_with_root(
        &self,
        project: &Project,
        capture: Option<(&ObjectRef, [f64; 6], ColorTransform)>,
    ) -> RuntimeSnapshot {
        let mut authored: HashMap<_, _> = self
            .display_nodes(project)
            .into_iter()
            .filter_map(|node| self.object_ref(&node.path).map(|id| (id, node)))
            .collect();
        let base = self.authored_snapshot(project);
        let mut layer_masks: Vec<_> = base
            .masks
            .iter()
            .map(|plan| plan.resolve("root", |path| self.object_ref(path)))
            .collect();
        let mut items: HashMap<_, _> = base
            .items
            .into_iter()
            .filter_map(|item| self.object_ref(&item.instance_path).map(|id| (id, item)))
            .collect();
        let mut button_symbols: HashMap<_, _> = base
            .nodes
            .iter()
            .filter_map(|node| {
                let symbol = node.symbol_id.as_ref().and_then(|id| project.symbol(id))?;
                (symbol.kind == SymbolKind::Button)
                    .then(|| {
                        self.object_ref(&node.path)
                            .map(|id| (id, symbol.id.clone()))
                    })
                    .flatten()
            })
            .collect();
        let mut metadata: HashMap<_, _> = base
            .nodes
            .into_iter()
            .filter_map(|node| {
                node.accessibility.and_then(|accessibility| {
                    self.object_ref(&node.path).map(|id| (id, accessibility))
                })
            })
            .collect();
        let mut linked_roots = HashMap::new();
        for (namespace, scope) in &self.graph.namespaces {
            for node in scope.player.display_nodes(project) {
                if node.path.is_empty() {
                    linked_roots.insert(namespace.clone(), node);
                } else if let Some(reference) = self.graph.linked_ref(namespace, &node.path) {
                    authored.insert(reference, node);
                }
            }
            let source = scope.player.authored_snapshot(project);
            layer_masks.extend(source.masks.iter().map(|plan| {
                plan.resolve(&format!("{:?}", namespace), |path| {
                    self.graph.linked_ref(namespace, path)
                })
            }));
            for item in source.items {
                if let Some(reference) = self.graph.linked_ref(namespace, &item.instance_path) {
                    items.insert(reference, item);
                }
            }
            for node in source.nodes {
                if let Some(symbol) = node.symbol_id.as_ref().and_then(|id| project.symbol(id))
                    && symbol.kind == SymbolKind::Button
                    && let Some(reference) = self.graph.linked_ref(namespace, &node.path)
                {
                    button_symbols.insert(reference, symbol.id.clone());
                }
                if let Some(accessibility) = node.accessibility
                    && let Some(reference) = self.graph.linked_ref(namespace, &node.path)
                {
                    metadata.insert(reference, accessibility);
                }
            }
        }
        let mut result = RuntimeSnapshot::default();
        let root = self.object_ref(&[]);
        let mut roots: Vec<_> = capture.map_or_else(
            || root.iter().cloned().collect(),
            |(source, _, _)| vec![source.clone()],
        );
        if capture.is_none() {
            roots.extend(
                self.graph
                    .dynamic
                    .iter()
                    .filter(|(_, node)| node.parent.is_none())
                    .map(|(id, _)| id.clone()),
            );
            roots.extend(
                self.graph
                    .parent_overrides
                    .iter()
                    .filter(|(id, parent)| {
                        parent.is_none() && self.graph.alive(id) && root.as_ref() != Some(*id)
                    })
                    .map(|(id, _)| id.clone()),
            );
        }
        let mut pending: Vec<_> = roots
            .into_iter()
            .rev()
            .map(|id| {
                (
                    id,
                    [1., 0., 0., 1., 0., 0.],
                    ColorTransform::default(),
                    true,
                    false,
                    false,
                    false,
                )
            })
            .collect();
        while let Some((
            reference,
            parent_world,
            parent_color,
            parent_visible,
            parent_attached,
            parent_disabled,
            parent_singular,
        )) = pending.pop()
        {
            let (mut display, authored_path, namespace_ref, source_path, graphics) =
                if let Some(node) = authored.get(&reference) {
                    let namespace = self
                        .graph
                        .linked_paths
                        .get(&reference)
                        .map(|address| address.namespace.clone());
                    (
                        node.clone(),
                        if namespace.is_none() {
                            Some(node.path.clone())
                        } else {
                            None
                        },
                        namespace,
                        Some(node.path.clone()),
                        vec![],
                    )
                } else if let Some(node) = self.graph.dynamic.get(&reference) {
                    let owner = match node.kind {
                        DynamicKind::Sprite => "sprite",
                        DynamicKind::Shape => "shape",
                        DynamicKind::MovieClip => "movie_clip",
                        DynamicKind::Bitmap => "bitmap",
                        DynamicKind::TextField => "text",
                        DynamicKind::Video => "video",
                        DynamicKind::Loader => "loader",
                    };
                    let linked = linked_roots.get(&reference);
                    let movie = node.kind == DynamicKind::MovieClip;
                    (
                        DisplayNode {
                            path: vec![],
                            lifetime_id: reference.id.clone(),
                            name: node.name.clone(),
                            owner: linked
                                .map_or_else(|| owner.into(), |source| source.owner.clone()),
                            class_name: linked.and_then(|source| source.class_name.clone()),
                            transform: node.transform,
                            world: [0.; 6],
                            visible: node.visible,
                            effective_visible: true,
                            frame: linked.map_or(0, |source| source.frame),
                            total_frames: linked
                                .map_or(if movie { 1 } else { 0 }, |source| source.total_frames),
                            independent: linked
                                .map_or(movie, |source| source.owner == "movie_clip"),
                            kind: None,
                        },
                        None,
                        linked.map(|_| reference.clone()),
                        linked.map(|_| vec![]),
                        node.graphics.paints(),
                    )
                } else {
                    continue;
                };
            if self.graph.loaded_as2(&reference) {
                display.frame = 0;
                display.total_frames = 1;
                display.kind = None;
            }
            let text = self.graph.text_state(&reference, display.kind.as_ref());
            let video = self.graph.video_state(&reference, display.kind.as_ref());
            let mut bitmap_state = self.graph.bitmap_state(&reference, display.kind.as_ref());
            bitmap_state.smoothing |= self.graph.loaded_as2_smoothing(&reference);
            let bitmap = bitmap_state
                .pixel_ref
                .as_ref()
                .and_then(|id| self.graph.pixels.info(id).ok());
            let capture_root = capture.filter(|(source, _, _)| *source == &reference);
            let local_matrix = if let Some((_, matrix, _)) = capture_root {
                matrix
            } else if let Some(affine) = self.graph.affines.get(&reference) {
                affine.update_display(&mut display);
                affine.matrix
            } else {
                transform_matrix(display.transform)
            };
            let color_transform = capture_root.map_or_else(
                || {
                    self.graph
                        .color_transforms
                        .get(&reference)
                        .copied()
                        .unwrap_or_else(|| {
                            display
                                .transform
                                .color_transform
                                .with_alpha(display.transform.alpha)
                        })
                },
                |(_, _, color)| color,
            );
            display.transform.alpha = color_transform.multipliers[3];
            let concatenated_color_transform = color_transform.then(parent_color);
            let scroll_rect = self.graph.scroll_rects.get(&reference).copied();
            let content_local_matrix = scroll_rect.map_or(local_matrix, |rect| {
                affine_multiply(local_matrix, [1., 0., 0., 1., -rect.x, -rect.y])
            });
            display.world = affine_multiply(parent_world, content_local_matrix);
            display.effective_visible =
                parent_visible && (capture_root.is_some() || display.visible);
            let on_stage =
                capture_root.is_some() || parent_attached || root.as_ref() == Some(&reference);
            let singular = parent_singular
                || local_matrix[0] * local_matrix[3] - local_matrix[1] * local_matrix[2] == 0.;
            let own_accessibility = metadata.get(&reference);
            let disabled = parent_disabled
                || own_accessibility
                    .is_some_and(|a| a.role == AccessibleRole::Button && a.disabled);
            if on_stage
                && display.effective_visible
                && !singular
                && (display.world[0] * display.world[3] - display.world[1] * display.world[2]).abs()
                    >= 1e-12
                && let Some(accessibility) = own_accessibility
            {
                let mut accessibility = accessibility.clone();
                if parent_disabled {
                    accessibility.disabled = true;
                }
                result.semantics.push(RuntimeAccessibleNode {
                    node: AccessibleNode {
                        instance_path: source_path.clone().unwrap_or_default(),
                        lifetime_id: display.lifetime_id.clone(),
                        accessibility,
                    },
                    owner_ref: reference.clone(),
                    clip_ids: vec![],
                });
            }
            let children = self.graph.children(&reference);
            if on_stage && display.effective_visible {
                if let Some(source) = items.get(&reference) {
                    let mut item = source.clone().map_kind(RuntimeDrawKind::Element);
                    item.transform = display.world;
                    item.alpha = 1.;
                    item.color_transform = ColorTransform::default();
                    if let RuntimeDrawKind::Element(kind) = &mut item.kind {
                        concatenated_color_transform.transform_kind(kind);
                    }
                    if !matches!(
                        source.kind,
                        ElementKind::Bitmap { .. }
                            | ElementKind::Video { .. }
                            | ElementKind::Text { .. }
                            | ElementKind::TextBox { .. }
                    ) {
                        result.draws.push(RuntimeDrawItem {
                            item,
                            owner_ref: reference.clone(),
                            clip_ids: vec![],
                            filter_id: None,
                        });
                    }
                }
                if let Some(video) = video {
                    result.draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
                        clip_ids: vec![],
                        filter_id: None,
                        item: DrawItem {
                            element_id: format!("runtime-{}-video", reference.id),
                            name: display.name.clone(),
                            instance_path: source_path.clone().unwrap_or_default(),
                            layer_id: items
                                .get(&reference)
                                .map_or_else(String::new, |item| item.layer_id.clone()),
                            transform: display.world,
                            alpha: 1.,
                            color_transform: ColorTransform::default(),
                            kind: RuntimeDrawKind::VideoFrame {
                                width: video.width,
                                height: video.height,
                                smoothing: video.smoothing,
                                color_transform: concatenated_color_transform,
                            },
                        },
                    });
                }
                if let Some(field) = &text {
                    for (index, ([x, y, width, height], color)) in self
                        .graph
                        .as2_selection_rectangles(&reference, &field.revision)
                        .into_iter()
                        .enumerate()
                    {
                        let mut kind = ElementKind::Rectangle {
                            width,
                            height,
                            fill: [0, 0, 0, 0],
                            stroke: [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255],
                            stroke_width: 1.,
                        };
                        concatenated_color_transform.transform_kind(&mut kind);
                        result.draws.push(RuntimeDrawItem {
                            owner_ref: reference.clone(),
                            clip_ids: vec![],
                            filter_id: None,
                            item: DrawItem {
                                element_id: format!("as2-selection-{}-{index}", reference.id),
                                name: display.name.clone(),
                                instance_path: source_path.clone().unwrap_or_default(),
                                layer_id: items
                                    .get(&reference)
                                    .map_or_else(String::new, |item| item.layer_id.clone()),
                                transform: affine_multiply(display.world, [1., 0., 0., 1., x, y]),
                                alpha: 1.,
                                color_transform: ColorTransform::default(),
                                kind: RuntimeDrawKind::Element(kind),
                            },
                        });
                    }
                    result.draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
                        clip_ids: vec![],
                        filter_id: None,
                        item: DrawItem {
                            element_id: items.get(&reference).map_or_else(
                                || format!("runtime-{}-text", reference.id),
                                |item| item.element_id.clone(),
                            ),
                            name: display.name.clone(),
                            instance_path: source_path.clone().unwrap_or_default(),
                            layer_id: items
                                .get(&reference)
                                .map_or_else(String::new, |item| item.layer_id.clone()),
                            transform: display.world,
                            alpha: 1.,
                            color_transform: ColorTransform::default(),
                            kind: RuntimeDrawKind::TextField {
                                field: Box::new(concatenated_color_transform.transform_text(field)),
                            },
                        },
                    });
                }
                if let Some(pixel) = &bitmap
                    && !pixel.disposed
                {
                    result.draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
                        clip_ids: vec![],
                        filter_id: None,
                        item: DrawItem {
                            element_id: items.get(&reference).map_or_else(
                                || format!("runtime-{}-bitmap", reference.id),
                                |item| item.element_id.clone(),
                            ),
                            name: display.name.clone(),
                            instance_path: source_path.clone().unwrap_or_default(),
                            layer_id: items
                                .get(&reference)
                                .map_or_else(String::new, |item| item.layer_id.clone()),
                            transform: display.world,
                            alpha: 1.,
                            color_transform: ColorTransform::default(),
                            kind: RuntimeDrawKind::BitmapPixels {
                                pixel_ref: pixel.pixel_ref.clone(),
                                width: pixel.width,
                                height: pixel.height,
                                revision: pixel.revision.clone(),
                                smoothing: bitmap_state.smoothing,
                                pixel_snapping: bitmap_state.pixel_snapping,
                                color_transform: concatenated_color_transform,
                            },
                        },
                    });
                }
                let colored_graphics = self
                    .graph
                    .dynamic
                    .get(&reference)
                    .map(|node| {
                        node.graphics
                            .paints_with_color(concatenated_color_transform)
                    })
                    .unwrap_or_default();
                for (index, kind) in colored_graphics.into_iter().enumerate() {
                    result.draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
                        clip_ids: vec![],
                        filter_id: None,
                        item: DrawItem {
                            element_id: format!("runtime-{}-{index}", reference.id),
                            name: display.name.clone(),
                            instance_path: vec![],
                            layer_id: String::new(),
                            transform: display.world,
                            alpha: 1.,
                            color_transform: ColorTransform::default(),
                            kind,
                        },
                    });
                }
            }
            for child in children.iter().rev() {
                pending.push((
                    child.clone(),
                    display.world,
                    concatenated_color_transform,
                    display.effective_visible,
                    on_stage,
                    disabled,
                    singular,
                ));
            }
            result.objects.push(RuntimeObject {
                input: self.graph.input_state(&reference, &display.owner),
                display,
                object_ref: reference.clone(),
                parent_ref: if capture_root.is_some() {
                    None
                } else {
                    self.graph.parent(&reference)
                },
                children,
                on_stage,
                authored_path,
                namespace_ref,
                source_path,
                graphics,
                button_hits: button_symbols
                    .get(&reference)
                    .map_or_else(Vec::new, |symbol| {
                        let count = project.symbol(symbol).map_or(0, |s| s.timeline.frame_count);
                        if count >= 4 {
                            crate::evaluate(project, symbol, 3)
                        } else {
                            vec![]
                        }
                    }),
                text,
                video,
                bitmap,
                smoothing: bitmap_state.smoothing,
                pixel_snapping: bitmap_state.pixel_snapping,
                local_matrix,
                content_local_matrix,
                scroll_rect,
                mask_ref: self.graph.masks.get(&reference).cloned(),
                clip_ids: vec![],
                filters: if capture.is_some() {
                    vec![]
                } else {
                    self.graph
                        .filters
                        .get(&reference)
                        .cloned()
                        .unwrap_or_default()
                },
                color_transform,
                concatenated_color_transform,
            });
        }
        RuntimeGraph::sanitize_input(&mut result);
        self.graph.apply_clipping(&mut result);
        crate::authored_layers::apply_runtime_masks(&mut result, &layer_masks);
        self.graph.apply_filters(&mut result);
        // Eligibility follows final descendant paint alpha. A signed parent
        // transform may hide 0..255 itself while a child's unclamped transform
        // produces a visible concatenation, so never prune whole subtrees early.
        // Each ancestor is visited once; no per-semantic-node scene scan.
        let mut painted = BTreeSet::new();
        for draw in &result.draws {
            let visible = match &draw.item.kind {
                RuntimeDrawKind::Element(
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
                    },
                ) => fill[3] > 0 || (*stroke_width > 0. && stroke[3] > 0),
                RuntimeDrawKind::TextField { field } => field.alpha > 0 && !field.text.is_empty(),
                RuntimeDrawKind::GradientPath {
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    stroke_width,
                    ..
                }
                | RuntimeDrawKind::Element(ElementKind::PaintedPath {
                    fill,
                    stroke,
                    fill_gradient,
                    stroke_gradient,
                    stroke_width,
                    ..
                }) => {
                    fill_gradient
                        .as_ref()
                        .map_or(fill[3] > 0, |g| g.stops.iter().any(|s| s.color[3] > 0))
                        || (*stroke_width > 0.
                            && stroke_gradient
                                .as_ref()
                                .map_or(stroke[3] > 0, |g| g.stops.iter().any(|s| s.color[3] > 0)))
                }
                // The browser's revision cache supplies actual pixel alpha
                // bounds. Core avoids rescanning bitmap buffers each snapshot.
                RuntimeDrawKind::BitmapPixels {
                    color_transform, ..
                }
                | RuntimeDrawKind::VideoFrame {
                    color_transform, ..
                } => color_transform.can_be_visible(),
                _ => false,
            };
            if visible {
                let mut current = Some(draw.owner_ref.clone());
                while let Some(reference) = current {
                    if !painted.insert(reference.clone()) {
                        break;
                    }
                    current = self.graph.parent(&reference);
                }
            }
        }
        result
            .semantics
            .retain(|node| painted.contains(&node.owner_ref));
        result
            .semantics
            .sort_by_key(|node| node.node.accessibility.tab_order);
        result
    }

    pub fn display_mutate(
        &mut self,
        project: &Project,
        mutation: DisplayMutation,
    ) -> Result<MutationResult, DisplayError> {
        let (parent, child) = match &mutation {
            DisplayMutation::Add { parent, child, .. }
            | DisplayMutation::Remove { parent, child }
            | DisplayMutation::SetIndex { parent, child, .. } => (parent, child),
            DisplayMutation::Swap { parent, first, .. } => (parent, first),
        };
        if !self.graph.alive(parent) || !self.graph.alive(child) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        self.graph.validate_loader_mutation(&mutation)?;
        let snapshot = self.runtime_snapshot(project);
        let container = snapshot
            .objects
            .iter()
            .find(|node| &node.object_ref == parent)
            .expect("live parent");
        if !matches!(
            container.display.owner.as_str(),
            "sprite" | "movie_clip" | "graphic" | "button"
        ) {
            return Err(error("This object cannot contain children"));
        }
        if self.graph.paths.get(child).is_some_and(Vec::is_empty) {
            return Err(error("The source root cannot be moved or removed"));
        }
        self.validate_authored_hierarchy_change(project, child)?;
        let current = self.graph.children(parent);
        let mut result = MutationResult::default();
        match mutation {
            DisplayMutation::Add {
                parent,
                child,
                index,
            } => {
                let old_parent = self.graph.parent(&child);
                let same = old_parent.as_ref() == Some(&parent);
                let length = current.len();
                let index = index.unwrap_or(if same {
                    length.saturating_sub(1)
                } else {
                    length
                });
                let maximum = if same {
                    length.saturating_sub(1)
                } else {
                    length
                };
                if index > maximum {
                    return Err(error("Child index is outside the container"));
                }
                let subtree = self.graph.subtree(&child);
                if subtree.contains(&parent) {
                    return Err(error("Display reparenting would create a cycle"));
                }
                let depth = self.graph.route(&parent).len();
                let height = subtree
                    .iter()
                    .map(|id| {
                        self.graph.route(id).len() - self.graph.route(&child).len()
                            + self.graph.namespaces.get(id).map_or(1, |scope| scope.depth)
                    })
                    .max()
                    .unwrap_or(1);
                if depth + height > MAX_GRAPH_DEPTH {
                    return Err(error("Display graph nesting exceeds 64 objects"));
                }
                self.graph.validate_clipping(
                    project,
                    &snapshot,
                    &self.graph.masks,
                    &self.graph.scroll_rects,
                    Some((&child, &parent)),
                )?;
                if same {
                    let mut siblings = current;
                    let old = siblings
                        .iter()
                        .position(|id| id == &child)
                        .expect("same parent");
                    siblings.remove(old);
                    siblings.insert(index, child);
                    self.graph.set_child_order(&parent, siblings);
                    return Ok(result);
                }
                let was_stage = self.graph.on_stage(&child);
                let will_stage = self.graph.on_stage(&parent);
                let count = usize::from(old_parent.is_some())
                    + 1
                    + if was_stage != will_stage {
                        subtree.len()
                    } else {
                        0
                    };
                if count > MAX_DISPLAY_EVENTS {
                    return Err(error("Display mutation exceeds its lifecycle event budget"));
                }
                if old_parent.is_some() {
                    result.events.push(DisplayEvent {
                        kind: "removed".into(),
                        target_ref: child.clone(),
                        route: self.graph.route(&child),
                    });
                }
                if was_stage && !will_stage {
                    for id in &subtree {
                        result.events.push(DisplayEvent {
                            kind: "removed_from_stage".into(),
                            target_ref: id.clone(),
                            route: self.graph.route(id),
                        });
                    }
                }
                self.graph.attach_child(&parent, &child, index);
                result.events.push(DisplayEvent {
                    kind: "added".into(),
                    target_ref: child.clone(),
                    route: self.graph.route(&child),
                });
                if !was_stage && will_stage {
                    for id in subtree {
                        result.events.push(DisplayEvent {
                            kind: "added_to_stage".into(),
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                }
            }
            DisplayMutation::Remove { parent, child } => {
                if self.graph.parent(&child).as_ref() != Some(&parent) {
                    return Err(error("Object is not a child of this container"));
                }
                let subtree = self.graph.subtree(&child);
                let attached = self.graph.on_stage(&child);
                if 1 + if attached { subtree.len() } else { 0 } > MAX_DISPLAY_EVENTS {
                    return Err(error("Display mutation exceeds its lifecycle event budget"));
                }
                result.events.push(DisplayEvent {
                    kind: "removed".into(),
                    target_ref: child.clone(),
                    route: self.graph.route(&child),
                });
                if attached {
                    for id in subtree {
                        result.events.push(DisplayEvent {
                            kind: "removed_from_stage".into(),
                            target_ref: id.clone(),
                            route: self.graph.route(&id),
                        });
                    }
                }
                self.graph.detach_child(&child);
            }
            DisplayMutation::SetIndex {
                parent,
                child,
                index,
            } => {
                let position = current
                    .iter()
                    .position(|id| id == &child)
                    .ok_or_else(|| error("Object is not a child of this container"))?;
                if index >= current.len() {
                    return Err(error("Child index is outside the container"));
                }
                let mut siblings = current;
                siblings.remove(position);
                siblings.insert(index, child);
                self.graph.set_child_order(&parent, siblings);
            }
            DisplayMutation::Swap {
                parent,
                first,
                second,
            } => {
                if !self.graph.alive(&second) {
                    return Err(error(
                        "Display reference is stale or belongs to another player",
                    ));
                }
                self.validate_authored_hierarchy_change(project, &second)?;
                let a = current
                    .iter()
                    .position(|id| id == &first)
                    .ok_or_else(|| error("First object is not a child"))?;
                let b = current
                    .iter()
                    .position(|id| id == &second)
                    .ok_or_else(|| error("Second object is not a child"))?;
                let mut siblings = current;
                siblings.swap(a, b);
                self.graph.set_child_order(&parent, siblings);
            }
        }
        Ok(result)
    }

    /// Detach an inclusive child range after validating the complete request.
    /// This bounded active-source-lifetime profile delivers lifecycle events after the
    /// batch; native reentrant per-child removal callbacks are not modeled.
    pub fn object_remove_children(
        &mut self,
        project: &Project,
        parent: &ObjectRef,
        begin: i32,
        end: i32,
    ) -> Result<MutationResult, DisplayError> {
        if !self.graph.alive(parent) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        let snapshot = self.runtime_snapshot(project);
        let container = snapshot
            .objects
            .iter()
            .find(|node| &node.object_ref == parent)
            .ok_or_else(|| error("Container is absent from the display graph"))?;
        if container.display.owner == "loader" {
            return Err(error(
                "Loader owns its content child; use load/unload instead of child-list mutation",
            ));
        }
        if !matches!(
            container.display.owner.as_str(),
            "sprite" | "movie_clip" | "graphic"
        ) {
            return Err(error("removeChildren requires a DisplayObjectContainer"));
        }
        let children = self.graph.children(parent);
        if begin < 0
            || end < 0
            || begin > end
            || (end != i32::MAX
                && (begin as usize >= children.len() || end as usize >= children.len()))
        {
            return Err(error(
                "removeChildren index range is outside the child list",
            ));
        }
        // The default maximum means through the last child, including an empty
        // list or a begin position beyond its end (pinned Ruffle remove_children).
        let selected: Vec<_> = children
            .into_iter()
            .take(end as usize + 1)
            .skip(begin as usize)
            .collect();
        let mut result = MutationResult::default();
        for child in &selected {
            self.validate_authored_hierarchy_change(project, child)?;
            self.graph
                .validate_loader_mutation(&DisplayMutation::Remove {
                    parent: parent.clone(),
                    child: child.clone(),
                })?;
            let subtree = self.graph.subtree(child);
            let attached = self.graph.on_stage(child);
            let cost = 1 + if attached { subtree.len() } else { 0 };
            if result.events.len().saturating_add(cost) > MAX_DISPLAY_EVENTS {
                return Err(error("Display mutation exceeds its lifecycle event budget"));
            }
            result.events.push(DisplayEvent {
                kind: "removed".into(),
                target_ref: child.clone(),
                route: self.graph.route(child),
            });
            if attached {
                for reference in subtree {
                    result.events.push(DisplayEvent {
                        kind: "removed_from_stage".into(),
                        route: self.graph.route(&reference),
                        target_ref: reference,
                    });
                }
            }
        }
        for child in selected {
            self.graph.detach_child(&child);
        }
        Ok(result)
    }

    /// Stop this container's local MovieClip clocks and those in its current
    /// subtree, including detached linked subtrees. Media playback is separate.
    pub fn object_stop_all_movie_clips(
        &mut self,
        project: &Project,
        target: &ObjectRef,
    ) -> Result<(), DisplayError> {
        if !self.graph.alive(target) {
            return Err(error(
                "Display reference is stale or belongs to another player",
            ));
        }
        let snapshot = self.runtime_snapshot(project);
        let container = snapshot
            .objects
            .iter()
            .find(|node| &node.object_ref == target)
            .ok_or_else(|| error("Container is absent from the display graph"))?;
        if !matches!(
            container.display.owner.as_str(),
            "sprite" | "movie_clip" | "graphic" | "loader"
        ) {
            return Err(error("stopAllMovieClips requires a DisplayObjectContainer"));
        }
        let clips: BTreeSet<_> = snapshot
            .objects
            .iter()
            .filter(|node| node.display.owner == "movie_clip")
            .map(|node| node.object_ref.clone())
            .collect();
        // The receiver is part of subtree(). Source MovieClips in synchronized
        // authoring modes have no independent clock; object_control leaves them
        // to their parent timeline instead of inventing playback state.
        for reference in self.graph.subtree(target) {
            if clips.contains(&reference) && !self.graph.loaded_as2(&reference) {
                self.object_control(project, &reference, PlaybackCommand::Stop);
            }
        }
        Ok(())
    }

    pub fn graphics_apply(
        &mut self,
        reference: &ObjectRef,
        operations: &[GraphicsOp],
    ) -> Result<(), DisplayError> {
        let object = self.graph.dynamic.get(reference).ok_or_else(|| {
            error("Graphics currently requires a live runtime-created Sprite, Shape or MovieClip")
        })?;
        if matches!(
            object.kind,
            DynamicKind::Bitmap | DynamicKind::TextField | DynamicKind::Video | DynamicKind::Loader
        ) {
            return Err(error(
                "Bitmap, Video, TextField and Loader do not own Graphics",
            ));
        }
        if operations.len() > MAX_GRAPHICS_OPS {
            return Err(error("Graphics batch exceeds 4096 operations"));
        }
        let old = object.graphics.cost();
        let mut reservation = old;
        let mut has_stroke = object.graphics.has_stroke();
        for op in operations {
            op.validate().map_err(error)?;
            match op {
                GraphicsOp::Clear => has_stroke = false,
                GraphicsOp::LineStyle { width, .. } => has_stroke = width.is_some(),
                GraphicsOp::LineGradientStyle { .. } if !has_stroke => {
                    return Err(error(
                        "lineGradientStyle requires an active positive-width lineStyle",
                    ));
                }
                _ => {}
            }
            if matches!(op, GraphicsOp::Clear) {
                reservation = 0;
            } else {
                reservation = reservation.saturating_add(op.reservation());
            }
            if self.graph.graphics_cost - old + reservation > MAX_GRAPHICS_COMMANDS {
                return Err(error(
                    "Runtime Graphics storage exceeds 100000 commands/runs",
                ));
            }
        }
        let object = self
            .graph
            .dynamic
            .get_mut(reference)
            .expect("validated object");
        for op in operations {
            object.graphics.apply(op);
        }
        self.graph.graphics_cost = self.graph.graphics_cost - old + object.graphics.cost();
        Ok(())
    }

    /// Snapshot all Graphics state, including active styles and the pen. The
    /// destination subsequently evolves independently; display properties do not copy.
    pub fn graphics_copy_from(
        &mut self,
        target: &ObjectRef,
        source: &ObjectRef,
    ) -> Result<(), DisplayError> {
        let valid = |node: &DynamicObject| {
            !matches!(
                node.kind,
                DynamicKind::Bitmap
                    | DynamicKind::TextField
                    | DynamicKind::Video
                    | DynamicKind::Loader
            )
        };
        let destination = self
            .graph
            .dynamic
            .get(target)
            .filter(|n| valid(n))
            .ok_or_else(|| {
                error("Graphics copy destination must be a live runtime Sprite, Shape or MovieClip")
            })?;
        let source = self
            .graph
            .dynamic
            .get(source)
            .filter(|n| valid(n))
            .ok_or_else(|| {
                error("Graphics copy source must be a live runtime Sprite, Shape or MovieClip")
            })?;
        let old = destination.graphics.cost();
        let next = source.graphics.cost();
        if self.graph.graphics_cost - old + next > MAX_GRAPHICS_COMMANDS {
            return Err(error(
                "Graphics copy exceeds the publication storage budget",
            ));
        }
        let copy = source.graphics.clone();
        self.graph
            .dynamic
            .get_mut(target)
            .expect("validated destination")
            .graphics = copy;
        self.graph.graphics_cost = self.graph.graphics_cost - old + next;
        Ok(())
    }

    pub fn object_set_property(&mut self, reference: &ObjectRef, name: &str, value: f64) -> bool {
        let valid = value.is_finite()
            && match name {
                "x" | "y" => value.abs() <= 1e12,
                "scale_x" | "scale_y" => value.abs() <= 1e4,
                "rotation" => value.abs() <= 1e9,
                "alpha" => (0.0..=1.0).contains(&value),
                _ => false,
            };
        if !valid {
            return false;
        }
        if let Some(result) = self.set_transform_property_override(reference, name, value) {
            return result;
        }
        if let Some(address) = self.graph.linked_paths.get(reference).cloned() {
            return self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .set_property(&address.path, name, value);
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.set_property(&path, name, value);
        }
        let Some(object) = self.graph.dynamic.get_mut(reference) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        match name {
            "x" if value.abs() <= 1e12 => object.transform.x = value,
            "y" if value.abs() <= 1e12 => object.transform.y = value,
            "scale_x" if value.abs() <= 1e4 => object.transform.scale_x = value,
            "scale_y" if value.abs() <= 1e4 => object.transform.scale_y = value,
            "rotation" if value.abs() <= 1e9 => object.transform.rotation = value,
            "alpha" if (0.0..=1.0).contains(&value) => object.transform.alpha = value,
            _ => return false,
        }
        true
    }
    pub fn object_property(
        &self,
        project: &Project,
        reference: &ObjectRef,
        name: &str,
    ) -> Option<f64> {
        if let Some(value) = self.transform_property_override(reference, name) {
            return Some(value);
        }
        if let Some(address) = self.graph.linked_paths.get(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .property(project, &address.path, name);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.property(project, path, name);
        }
        let transform = self.graph.dynamic.get(reference)?.transform;
        match name {
            "x" => Some(transform.x),
            "y" => Some(transform.y),
            "scale_x" => Some(transform.scale_x),
            "scale_y" => Some(transform.scale_y),
            "rotation" => Some(transform.rotation),
            "alpha" => Some(transform.alpha),
            _ => None,
        }
    }
    pub fn object_set_visible(&mut self, reference: &ObjectRef, value: bool) -> bool {
        if let Some(address) = self.graph.linked_paths.get(reference).cloned() {
            return self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .set_visible(&address.path, value);
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.set_visible(&path, value);
        }
        let Some(object) = self.graph.dynamic.get_mut(reference) else {
            return false;
        };
        object.visible = value;
        true
    }
    pub fn object_visible(&self, project: &Project, reference: &ObjectRef) -> Option<bool> {
        if let Some(address) = self.graph.linked_paths.get(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .visible(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.visible(project, path);
        }
        self.graph.dynamic.get(reference).map(|node| node.visible)
    }
    pub fn object_set_name(&mut self, reference: &ObjectRef, name: &str) -> bool {
        if let Some(address) = self.graph.linked_paths.get(reference).cloned() {
            return self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .set_name(&address.path, name);
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.set_name(&path, name);
        }
        if name.len() > 4096 {
            return false;
        }
        let Some(object) = self.graph.dynamic.get_mut(reference) else {
            return false;
        };
        object.name = name.into();
        true
    }
    pub fn object_button_state(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        state: u32,
    ) -> bool {
        if let Some(address) = self.graph.source_address(reference) {
            let changed = self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .button_state(project, &address.path, state);
            if changed {
                self.sync_linked(project);
                self.collect_linked_scripts(&address.namespace);
            }
            return changed;
        }
        self.graph
            .paths
            .get(reference)
            .cloned()
            .is_some_and(|path| self.button_state(project, &path, state))
    }

    pub fn object_control(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        command: PlaybackCommand,
    ) -> bool {
        if self.graph.loaded_as2(reference) {
            return matches!(
                command,
                PlaybackCommand::Play
                    | PlaybackCommand::Stop
                    | PlaybackCommand::GotoAndPlay(0)
                    | PlaybackCommand::GotoAndStop(0)
            );
        }
        if let Some(address) = self.graph.source_address(reference) {
            let result = self
                .graph
                .namespaces
                .get_mut(&address.namespace)
                .expect("namespace")
                .player
                .control(project, &address.path, command);
            if result {
                self.sync_linked(project);
                self.collect_linked_scripts(&address.namespace);
            }
            return result;
        }
        if let Some(path) = self.graph.paths.get(reference).cloned() {
            return self.control(project, &path, command);
        }
        let Some(object) = self
            .graph
            .dynamic
            .get_mut(reference)
            .filter(|node| node.kind == DynamicKind::MovieClip)
        else {
            return false;
        };
        match command {
            PlaybackCommand::Play | PlaybackCommand::GotoAndPlay(0) => object.playing = true,
            PlaybackCommand::Stop | PlaybackCommand::GotoAndStop(0) => object.playing = false,
            _ => return false,
        }
        true
    }
    pub fn object_metadata(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Option<TimelineMetadata> {
        if !self.graph.alive(reference) || self.graph.loaded_as2(reference) {
            return None;
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_metadata(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_metadata(project, path);
        }
        self.graph
            .dynamic
            .get(reference)
            .filter(|node| node.kind == DynamicKind::MovieClip)
            .map(|_| TimelineMetadata {
                current_label: None,
                current_frame_label: None,
                labels: vec![],
                frames_loaded: 1,
                total_frames: 1,
                scene_name: String::new(),
                scene_frame: 1,
                scenes: vec![SceneMetadata {
                    name: String::new(),
                    num_frames: 1,
                    labels: vec![],
                }],
                // Empty MovieClip instances have one frame; explicit play
                // cannot put them into the AS3 isPlaying state.
                is_playing: false,
            })
    }
    pub fn object_frame(&self, reference: &ObjectRef) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return Some(0);
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_frame(&address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_frame(path);
        }
        self.graph
            .dynamic
            .get(reference)
            .filter(|node| node.kind == DynamicKind::MovieClip)
            .map(|_| 0)
    }
    pub fn object_frame_count(&self, project: &Project, reference: &ObjectRef) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return Some(1);
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_frame_count(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_frame_count(project, path);
        }
        self.object_frame(reference).map(|_| 1)
    }
    pub fn object_resolve_frame(
        &self,
        project: &Project,
        reference: &ObjectRef,
        label: &str,
        as2: bool,
    ) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return None;
        }
        if let Some(address) = self.graph.source_address(reference) {
            let scope = &self.graph.namespaces.get(&address.namespace)?.player;
            return if as2 {
                scope.resolve_frame_as2(project, &address.path, label)
            } else {
                scope.resolve_frame(project, &address.path, label)
            };
        }
        let path = self.graph.paths.get(reference)?;
        if as2 {
            self.resolve_frame_as2(project, path, label)
        } else {
            self.resolve_frame(project, path, label)
        }
    }
    /// Scene count of a live timeline object; dynamic and loaded AS2 clips have one.
    pub fn object_scene_count(&self, project: &Project, reference: &ObjectRef) -> Option<u32> {
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .clip_scene_count(project, &address.path);
        }
        if let Some(path) = self.graph.paths.get(reference) {
            return self.clip_scene_count(project, path);
        }
        self.object_frame(reference).map(|_| 1)
    }
    /// Scene-aware goto resolution for a live timeline object.
    pub fn object_resolve_goto(
        &self,
        project: &Project,
        reference: &ObjectRef,
        request: &GotoRequest,
    ) -> Result<Option<u32>, String> {
        if !self.graph.loaded_as2(reference)
            && let Some(address) = self.graph.source_address(reference)
        {
            return self
                .graph
                .namespaces
                .get(&address.namespace)
                .ok_or("Display object is no longer active.")?
                .player
                .resolve_goto(project, &address.path, request);
        }
        if !self.graph.loaded_as2(reference)
            && let Some(path) = self.graph.paths.get(reference)
        {
            return self.resolve_goto(project, path, request);
        }
        if self.object_frame(reference).is_none() {
            return Err("Timeline controls apply to symbols and the root scene.".into());
        }
        // Empty runtime MovieClips and loaded AS2 content expose one frame.
        let single = [crate::SceneRange {
            name: String::new(),
            start: 0,
            frames: 1,
        }];
        crate::resolve_goto(&single, &[], 0, request)
    }
    pub fn object_scene_step(
        &self,
        project: &Project,
        reference: &ObjectRef,
        step: i32,
    ) -> Option<u32> {
        if self.graph.loaded_as2(reference) {
            return None;
        }
        if let Some(address) = self.graph.source_address(reference) {
            return self
                .graph
                .namespaces
                .get(&address.namespace)?
                .player
                .resolve_scene_step(project, &address.path, step);
        }
        let path = self.graph.paths.get(reference)?;
        self.resolve_scene_step(project, path, step)
    }
}
