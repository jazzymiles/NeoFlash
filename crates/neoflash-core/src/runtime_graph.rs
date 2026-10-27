//! Runtime identity and dynamic display graph. Authored documents stay immutable.
use crate::runtime_graphics::{GraphicsState, MAX_GRAPHICS_COMMANDS, MAX_GRAPHICS_OPS};
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
mod bitmap;
mod linked;
mod text;
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
    pub text: Option<TextFieldState>,
    pub bitmap: Option<PixelDescriptor>,
    pub smoothing: bool,
    pub pixel_snapping: PixelSnapping,
    pub local_matrix: [f64; 6],
    pub color_transform: ColorTransform,
    pub concatenated_color_transform: ColorTransform,
}

/// Runtime pixels have their own identity; authoring resource IDs remain immutable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeDrawKind {
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
        field: TextFieldState,
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
            Self::TextField { field } => TextFieldState::from_kind(other).as_ref() == Some(field),
            _ => false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeDrawItem {
    #[serde(flatten)]
    pub item: DrawItem<RuntimeDrawKind>,
    pub owner_ref: ObjectRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeAccessibleNode {
    #[serde(flatten)]
    pub node: AccessibleNode,
    pub owner_ref: ObjectRef,
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
    text_bytes: usize,
    affines: HashMap<ObjectRef, transform::AffineState>,
    color_transforms: HashMap<ObjectRef, ColorTransform>,
    authored: BTreeMap<Vec<Id>, Authored>,
    paths: HashMap<ObjectRef, Vec<Id>>,
    prefix: HashMap<ObjectRef, Vec<ObjectRef>>,
    suffix: HashMap<ObjectRef, Vec<ObjectRef>>,
    dynamic: BTreeMap<ObjectRef, DynamicObject>,
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
        self.text_bytes = 0;
        self.affines.clear();
        self.color_transforms.clear();
        self.authored.clear();
        self.paths.clear();
        self.prefix.clear();
        self.suffix.clear();
        self.dynamic.clear();
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
        for parent in &retired {
            let children = self.suffix.get(parent).cloned().unwrap_or_default();
            for child in children {
                let route = self.route(&child);
                let on_stage = self.on_stage(&child);
                let subtree = self.subtree(&child);
                let stage_routes: Vec<_> = subtree
                    .iter()
                    .map(|id| (id.clone(), self.route(id)))
                    .collect();
                self.dynamic.get_mut(&child).expect("dynamic suffix").parent = None;
                // Retirement must never reattach a child to a replacement path.
                if self.events.len() + 1 + stage_routes.len() <= MAX_DISPLAY_EVENTS {
                    self.events.push(DisplayEvent {
                        kind: "removed".into(),
                        target_ref: child,
                        route,
                    });
                    if on_stage {
                        for (target_ref, route) in stage_routes {
                            self.events.push(DisplayEvent {
                                kind: "removed_from_stage".into(),
                                target_ref,
                                route,
                            });
                        }
                    }
                } else {
                    self.events_overflowed = true;
                }
            }
            self.suffix.remove(parent);
            self.prefix.remove(parent);
        }
        self.authored
            .retain(|path, node| active.get(path.as_slice()) == Some(&node.lifetime));
        self.paths
            .retain(|reference, _| !retired.contains(reference));
        self.bitmap_states
            .retain(|reference, _| !retired.contains(reference));
        self.retire_text(&retired);
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
        self.prefix
            .get(id)
            .into_iter()
            .flatten()
            .chain(self.suffix.get(id).into_iter().flatten())
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
        let mut authored: HashMap<_, _> = self
            .display_nodes(project)
            .into_iter()
            .filter_map(|node| self.object_ref(&node.path).map(|id| (id, node)))
            .collect();
        let base = self.authored_snapshot(project);
        let mut items: HashMap<_, _> = base
            .items
            .into_iter()
            .filter_map(|item| self.object_ref(&item.instance_path).map(|id| (id, item)))
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
            for item in source.items {
                if let Some(reference) = self.graph.linked_ref(namespace, &item.instance_path) {
                    items.insert(reference, item);
                }
            }
            for node in source.nodes {
                if let Some(accessibility) = node.accessibility
                    && let Some(reference) = self.graph.linked_ref(namespace, &node.path)
                {
                    metadata.insert(reference, accessibility);
                }
            }
        }
        let mut result = RuntimeSnapshot::default();
        let root = self.object_ref(&[]);
        let mut roots: Vec<_> = root.iter().cloned().collect();
        roots.extend(
            self.graph
                .dynamic
                .iter()
                .filter(|(_, node)| node.parent.is_none())
                .map(|(id, _)| id.clone()),
        );
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
                    };
                    let linked = linked_roots.get(&reference);
                    let movie = node.kind == DynamicKind::MovieClip;
                    (
                        DisplayNode {
                            path: vec![],
                            lifetime_id: reference.id.clone(),
                            name: node.name.clone(),
                            owner: owner.into(),
                            class_name: linked.and_then(|source| source.class_name.clone()),
                            transform: node.transform,
                            world: [0.; 6],
                            visible: node.visible,
                            effective_visible: true,
                            frame: linked.map_or(0, |source| source.frame),
                            total_frames: linked
                                .map_or(if movie { 1 } else { 0 }, |source| source.total_frames),
                            independent: movie,
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
            let text = self.graph.text_state(&reference, display.kind.as_ref());
            let bitmap_state = self.graph.bitmap_state(&reference, display.kind.as_ref());
            let bitmap = bitmap_state
                .pixel_ref
                .as_ref()
                .and_then(|id| self.graph.pixels.info(id).ok());
            let local_matrix = if let Some(affine) = self.graph.affines.get(&reference) {
                affine.update_display(&mut display);
                affine.matrix
            } else {
                transform_matrix(display.transform)
            };
            let color_transform = self
                .graph
                .color_transforms
                .get(&reference)
                .copied()
                .unwrap_or_else(|| ColorTransform::from_alpha(display.transform.alpha));
            display.transform.alpha = color_transform.multipliers[3];
            let concatenated_color_transform = color_transform.then(parent_color);
            display.world = affine_multiply(parent_world, local_matrix);
            display.effective_visible = parent_visible && display.visible;
            let on_stage = parent_attached || root.as_ref() == Some(&reference);
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
                });
            }
            let children = self.graph.children(&reference);
            if on_stage && display.effective_visible {
                if let Some(source) = items.get(&reference) {
                    let mut item = source.clone().map_kind(RuntimeDrawKind::Element);
                    item.transform = display.world;
                    item.alpha = 1.;
                    if let RuntimeDrawKind::Element(kind) = &mut item.kind {
                        concatenated_color_transform.transform_kind(kind);
                    }
                    if !matches!(
                        source.kind,
                        ElementKind::Bitmap { .. }
                            | ElementKind::Text { .. }
                            | ElementKind::TextBox { .. }
                    ) {
                        result.draws.push(RuntimeDrawItem {
                            item,
                            owner_ref: reference.clone(),
                        });
                    }
                }
                if let Some(field) = &text {
                    result.draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
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
                            kind: RuntimeDrawKind::TextField {
                                field: concatenated_color_transform.transform_text(field),
                            },
                        },
                    });
                }
                if let Some(pixel) = &bitmap
                    && !pixel.disposed
                {
                    result.draws.push(RuntimeDrawItem {
                        owner_ref: reference.clone(),
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
                        item: DrawItem {
                            element_id: format!("runtime-{}-{index}", reference.id),
                            name: display.name.clone(),
                            instance_path: vec![],
                            layer_id: String::new(),
                            transform: display.world,
                            alpha: 1.,
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
                display,
                object_ref: reference.clone(),
                parent_ref: self.graph.parent(&reference),
                children,
                on_stage,
                authored_path,
                namespace_ref,
                source_path,
                graphics,
                text,
                bitmap,
                smoothing: bitmap_state.smoothing,
                pixel_snapping: bitmap_state.pixel_snapping,
                local_matrix,
                color_transform,
                concatenated_color_transform,
            });
        }
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
                } => {
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
        if !self.graph.dynamic.contains_key(child) {
            return Err(error(
                "Moving or removing authored timeline children is not supported",
            ));
        }
        let prefix = self.graph.prefix.get(parent).map_or(0, Vec::len);
        let current = self.graph.suffix.get(parent).cloned().unwrap_or_default();
        let mut result = MutationResult::default();
        match mutation {
            DisplayMutation::Add {
                parent,
                child,
                index,
            } => {
                let old_parent = self.graph.parent(&child);
                let same = old_parent.as_ref() == Some(&parent);
                let length = prefix + current.len();
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
                if index < prefix || index > maximum {
                    return Err(error(
                        "Dynamic child index must stay in the suffix after authored children",
                    ));
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
                if same {
                    let siblings = self.graph.suffix.entry(parent).or_default();
                    let old = siblings
                        .iter()
                        .position(|id| id == &child)
                        .expect("same parent");
                    siblings.remove(old);
                    siblings.insert(index - prefix, child);
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
                if let Some(old) = old_parent {
                    self.graph
                        .suffix
                        .get_mut(&old)
                        .expect("old children")
                        .retain(|id| id != &child);
                }
                self.graph.dynamic.get_mut(&child).expect("dynamic").parent = Some(parent.clone());
                self.graph
                    .suffix
                    .entry(parent)
                    .or_default()
                    .insert(index - prefix, child.clone());
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
                self.graph
                    .suffix
                    .get_mut(&parent)
                    .expect("parent children")
                    .retain(|id| id != &child);
                self.graph.dynamic.get_mut(&child).expect("dynamic").parent = None;
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
                if index < prefix || index >= prefix + current.len() {
                    return Err(error(
                        "Dynamic child index must stay in the suffix after authored children",
                    ));
                }
                let siblings = self.graph.suffix.get_mut(&parent).expect("parent children");
                siblings.remove(position);
                siblings.insert(index - prefix, child);
            }
            DisplayMutation::Swap {
                parent,
                first,
                second,
            } => {
                if !self.graph.dynamic.contains_key(&second) {
                    return Err(error("Only dynamic children can be swapped"));
                }
                let a = current
                    .iter()
                    .position(|id| id == &first)
                    .ok_or_else(|| error("First object is not a child"))?;
                let b = current
                    .iter()
                    .position(|id| id == &second)
                    .ok_or_else(|| error("Second object is not a child"))?;
                self.graph
                    .suffix
                    .get_mut(&parent)
                    .expect("parent children")
                    .swap(a, b);
            }
        }
        Ok(result)
    }

    pub fn graphics_apply(
        &mut self,
        reference: &ObjectRef,
        operations: &[GraphicsOp],
    ) -> Result<(), DisplayError> {
        let object = self.graph.dynamic.get(reference).ok_or_else(|| {
            error("Graphics currently requires a live runtime-created Sprite, Shape or MovieClip")
        })?;
        if matches!(object.kind, DynamicKind::Bitmap | DynamicKind::TextField) {
            return Err(error("Bitmap and TextField do not own Graphics"));
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
            !matches!(node.kind, DynamicKind::Bitmap | DynamicKind::TextField)
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
    pub fn object_control(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        command: PlaybackCommand,
    ) -> bool {
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
    pub fn object_frame(&self, reference: &ObjectRef) -> Option<u32> {
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
}
