//! Layer relationships shared by authoring and published runtime snapshots.
use crate::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(crate) struct LayerMaskPlan {
    pub id: String,
    pub mode: MaskMode,
    pub invert: bool,
    pub sources: Vec<Vec<Id>>,
    pub targets: Vec<Vec<Id>>,
    pub target_groups: Vec<(String, Vec<Vec<Id>>)>,
}
#[derive(Clone, Debug)]
pub(crate) struct AuthoredBlendGroupPlan {
    pub id: String,
    pub depth: usize,
    pub tier: usize,
    pub roots: Vec<Vec<Id>>,
    pub blend_mode: BlendMode,
}
#[derive(Clone, Debug)]
pub struct AuthoredDraw {
    pub item: DrawItem,
    pub clip_ids: Vec<String>,
    pub outline_color: Option<Color>,
    pub mask_group: Option<String>,
    pub blend_mode: BlendMode,
}
#[derive(Clone, Debug)]
pub struct AuthoredMaskGroup {
    pub id: String,
    pub parent_id: Option<String>,
    pub clip_ids: Vec<String>,
    pub blend_mode: BlendMode,
}
#[derive(Clone, Debug, Default)]
pub struct AuthoredScene {
    pub draws: Vec<AuthoredDraw>,
    pub clips: Vec<RuntimeClip>,
    pub mask_groups: Vec<AuthoredMaskGroup>,
}

/// A stable, opaque editor color. Explicit colors survive renaming/reordering;
/// legacy layers need no serialized field until the author changes their color.
pub fn layer_outline_color(layer: &Layer) -> Color {
    let rgb = layer.properties.outline_color.unwrap_or_else(|| {
        const PALETTE: [[u8; 3]; 8] = [
            [62, 153, 235],
            [212, 95, 193],
            [45, 182, 150],
            [229, 137, 54],
            [135, 120, 225],
            [225, 100, 107],
            [68, 171, 185],
            [165, 162, 64],
        ];
        let hash = layer.id.bytes().fold(2_166_136_261u32, |hash, byte| {
            (hash ^ byte as u32).wrapping_mul(16_777_619)
        });
        PALETTE[hash as usize % PALETTE.len()]
    });
    [rgb[0], rgb[1], rgb[2], 255]
}
pub fn layer_effective_outline_color(timeline: &Timeline, layer: &Layer) -> Option<Color> {
    let mut current = Some(layer);
    let mut color = None;
    for _ in 0..=timeline.layers.len() {
        let Some(node) = current else {
            return color;
        };
        if node.properties.outline {
            color = Some(layer_outline_color(node));
        }
        current = node
            .properties
            .parent
            .as_ref()
            .and_then(|id| timeline.layers.iter().find(|l| &l.id == id));
    }
    color
}

pub fn layer_effectively_visible(timeline: &Timeline, layer: &Layer) -> bool {
    let mut current = Some(layer);
    for _ in 0..=timeline.layers.len() {
        let Some(node) = current else {
            return true;
        };
        if !node.visible {
            return false;
        }
        current = node
            .properties
            .parent
            .as_ref()
            .and_then(|id| timeline.layers.iter().find(|l| &l.id == id));
    }
    false
}

pub fn layer_effectively_locked(timeline: &Timeline, layer: &Layer) -> bool {
    let mut current = Some(layer);
    for _ in 0..=timeline.layers.len() {
        let Some(node) = current else {
            return false;
        };
        if node.locked {
            return true;
        }
        current = node
            .properties
            .parent
            .as_ref()
            .and_then(|id| timeline.layers.iter().find(|l| &l.id == id));
    }
    true
}

pub fn layer_effective_blend_mode(timeline: &Timeline, layer: &Layer) -> BlendMode {
    let mut current = Some(layer);
    for _ in 0..=timeline.layers.len() {
        let Some(node) = current else {
            return BlendMode::Normal;
        };
        if !node.properties.blend_mode.is_normal() {
            return node.properties.blend_mode;
        }
        current = node
            .properties
            .parent
            .as_ref()
            .and_then(|id| timeline.layers.iter().find(|l| &l.id == id));
    }
    BlendMode::Normal
}

pub fn layer_effectively_synced(timeline: &Timeline, layer: &Layer) -> bool {
    let mut current = Some(layer);
    for _ in 0..=timeline.layers.len() {
        let Some(node) = current else {
            return false;
        };
        if node.properties.sync_timeline {
            return true;
        }
        current = node
            .properties
            .parent
            .as_ref()
            .and_then(|id| timeline.layers.iter().find(|l| &l.id == id));
    }
    false
}
impl AuthoredScene {
    pub fn has_blend_modes(&self) -> bool {
        self.draws.iter().any(|draw| !draw.blend_mode.is_normal())
            || self
                .mask_groups
                .iter()
                .any(|group| !group.blend_mode.is_normal())
    }

    pub fn contains_point(&self, path: &[Id], point: [f64; 2]) -> bool {
        let Some(draw) = self
            .draws
            .iter()
            .find(|draw| draw.item.instance_path == path)
        else {
            return false;
        };
        fn inside(scene: &AuthoredScene, id: &str, point: [f64; 2], depth: usize) -> bool {
            if depth > 64 {
                return false;
            }
            scene
                .clips
                .iter()
                .find(|clip| clip.id == id)
                .is_some_and(|clip| {
                    let is_inside = clip.components.iter().enumerate().any(|(idx, component)| {
                        if let Some(alpha_comp) = clip
                            .alpha_components
                            .as_ref()
                            .and_then(|list| list.get(idx))
                            && alpha_comp.fill_gradient.is_none()
                            && alpha_comp.fill_bitmap.is_none()
                            && alpha_comp.resource_bitmap.is_none()
                            && alpha_comp.alpha <= 0.0
                        {
                            return false;
                        }
                        let m = component.matrix;
                        let determinant = m[0] * m[3] - m[1] * m[2];
                        if determinant.abs() < 1e-12 {
                            return false;
                        }
                        let x = point[0] - m[4];
                        let y = point[1] - m[5];
                        let local = [
                            (m[3] * x - m[2] * y) / determinant,
                            (-m[1] * x + m[0] * y) / determinant,
                        ];
                        hit_test_path(
                            &component.commands,
                            component.fill_rule,
                            local,
                            true,
                            0.,
                            FlattenOptions::default(),
                        )
                        .unwrap_or(false)
                            && component
                                .clip_ids
                                .iter()
                                .all(|id| inside(scene, id, point, depth + 1))
                    });
                    if clip.invert { !is_inside } else { is_inside }
                })
        }
        draw.clip_ids.iter().all(|id| inside(self, id, point, 0))
    }
}

pub fn bt709_luminance(r: u8, g: u8, b: u8) -> f64 {
    (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)) / 255.0
}

pub(crate) fn layer_mask_plans(
    timeline: &Timeline,
    frame: u32,
    parent: &[Id],
    fps: f64,
) -> Vec<LayerMaskPlan> {
    let paths = |layer: &Layer| -> Vec<Vec<Id>> {
        if !layer_effectively_visible(timeline, layer) {
            return vec![];
        }
        crate::evaluate::sample_layer_at_fps(layer, frame, fps)
            .into_iter()
            .map(|e| {
                let mut path = parent.to_vec();
                path.push(e.id);
                path
            })
            .collect()
    };
    timeline
        .layers
        .iter()
        .filter(|layer| layer.properties.role == LayerRole::Mask)
        .map(|layer| {
            let mut identity = parent.to_vec();
            identity.push(layer.id.clone());
            LayerMaskPlan {
                id: format!(
                    "layer:{}",
                    serde_json::to_string(&identity).unwrap_or_default()
                ),
                target_groups: timeline
                    .layers
                    .iter()
                    .filter(|target| target.properties.mask.as_ref() == Some(&layer.id))
                    .map(|target| {
                        let mut group_identity = parent.to_vec();
                        group_identity.push(target.id.clone());
                        (
                            format!(
                                "layer-group:{}",
                                serde_json::to_string(&group_identity).unwrap_or_default()
                            ),
                            paths(target),
                        )
                    })
                    .collect(),
                mode: layer.properties.mask_mode,
                invert: layer.properties.mask_invert,
                sources: paths(layer),
                targets: timeline
                    .layers
                    .iter()
                    .filter(|target| target.properties.mask.as_ref() == Some(&layer.id))
                    .flat_map(paths)
                    .collect(),
            }
        })
        .collect()
}

pub(crate) fn instance_blend_plan(path: &[Id], blend_mode: BlendMode) -> AuthoredBlendGroupPlan {
    AuthoredBlendGroupPlan {
        id: format!(
            "blend-instance:{}",
            serde_json::to_string(path).unwrap_or_default()
        ),
        depth: path.len(),
        tier: 200,
        roots: vec![path.to_vec()],
        blend_mode,
    }
}

pub(crate) fn layer_blend_plans(
    timeline: &Timeline,
    frame: u32,
    parent: &[Id],
    include_guides: bool,
    fps: f64,
) -> Vec<AuthoredBlendGroupPlan> {
    let is_descendant = |layer: &Layer, folder_id: &str| -> bool {
        let mut current = layer.properties.parent.as_deref();
        for _ in 0..=timeline.layers.len() {
            let Some(id) = current else {
                return false;
            };
            if id == folder_id {
                return true;
            }
            current = timeline
                .layers
                .iter()
                .find(|l| l.id == id)
                .and_then(|l| l.properties.parent.as_deref());
        }
        false
    };
    let folder_depth = |layer: &Layer| -> usize {
        let mut depth = 0usize;
        let mut current = layer.properties.parent.as_deref();
        for _ in 0..=timeline.layers.len() {
            let Some(id) = current else {
                break;
            };
            depth += 1;
            current = timeline
                .layers
                .iter()
                .find(|l| l.id == id)
                .and_then(|l| l.properties.parent.as_deref());
        }
        depth
    };
    let mut plans = Vec::new();
    for folder in &timeline.layers {
        if folder.properties.role != LayerRole::Folder
            || folder.properties.blend_mode != BlendMode::Layer
            || !layer_effectively_visible(timeline, folder)
        {
            continue;
        }
        let mut roots = Vec::new();
        for child in &timeline.layers {
            if !layer_effectively_visible(timeline, child)
                || child.properties.role == LayerRole::Folder
                || child.properties.role == LayerRole::Mask
                || (!include_guides && child.properties.role == LayerRole::Guide)
                || !is_descendant(child, &folder.id)
            {
                continue;
            }
            for e in crate::evaluate::sample_layer_in_at_fps(timeline, child, frame, fps) {
                let mut path = parent.to_vec();
                path.push(e.id);
                roots.push(path);
            }
        }
        if !roots.is_empty() {
            let mut identity = parent.to_vec();
            identity.push(folder.id.clone());
            plans.push(AuthoredBlendGroupPlan {
                id: format!(
                    "blend-folder:{}",
                    serde_json::to_string(&identity).unwrap_or_default()
                ),
                depth: parent.len() + 1,
                tier: folder_depth(folder),
                roots,
                blend_mode: BlendMode::Layer,
            });
        }
    }
    for layer in &timeline.layers {
        if !layer_effectively_visible(timeline, layer)
            || layer.properties.role == LayerRole::Folder
            || layer.properties.role == LayerRole::Mask
            || (!include_guides && layer.properties.role == LayerRole::Guide)
        {
            continue;
        }
        let layer_blend = layer_effective_blend_mode(timeline, layer);
        if layer_blend.is_normal() {
            continue;
        }
        let roots: Vec<Vec<Id>> =
            crate::evaluate::sample_layer_in_at_fps(timeline, layer, frame, fps)
                .into_iter()
                .filter(|e| layer_blend == BlendMode::Layer || e.blend_mode.is_normal())
                .map(|e| {
                    let mut path = parent.to_vec();
                    path.push(e.id);
                    path
                })
                .collect();
        if !roots.is_empty() {
            let mut identity = parent.to_vec();
            identity.push(layer.id.clone());
            plans.push(AuthoredBlendGroupPlan {
                id: format!(
                    "blend-layer:{}",
                    serde_json::to_string(&identity).unwrap_or_default()
                ),
                depth: parent.len() + 1,
                tier: 100,
                roots,
                blend_mode: layer_blend,
            });
        }
    }
    plans
}

pub fn evaluate_scene(
    project: &Project,
    symbol: &str,
    frame: u32,
    include_guides: bool,
    show_masking: bool,
) -> AuthoredScene {
    evaluate_scene_with_buttons(
        project,
        symbol,
        frame,
        include_guides,
        show_masking,
        &|_| None,
    )
}

/// Re-evaluate a live authored timeline while retaining the frame of every
/// materialized independent clip from a paused preview. The returned clip IDs
/// are the masks structurally targeting `target_root`, even when that branch
/// currently has no drawable children (for example an empty frame with onion
/// ghosts on neighbouring frames).
pub fn evaluate_scene_with_frozen_clips(
    project: &Project,
    symbol: &str,
    frame: u32,
    clip_frames: &HashMap<Vec<Id>, u32>,
    target_root: &[Id],
    include_guides: bool,
    show_masking: bool,
) -> (AuthoredScene, Vec<String>) {
    let evaluation = crate::evaluate::snapshot_mode(
        project,
        symbol,
        frame,
        &|path| clip_frames.get(path).copied(),
        &|_| None,
        include_guides,
    );
    let mut target_clip_ids = Vec::new();
    if show_masking {
        for plan in &evaluation.masks {
            for (index, (_, roots)) in plan.target_groups.iter().enumerate() {
                if roots.iter().any(|root| root.as_slice() == target_root) {
                    let id = target_clip_id(&plan.id, plan.target_groups.len(), index);
                    if !target_clip_ids.contains(&id) {
                        target_clip_ids.push(id);
                    }
                }
            }
        }
    }
    (
        scene_from_evaluation(project, evaluation, show_masking, Some(symbol)),
        target_clip_ids,
    )
}

pub fn evaluate_scene_with_buttons(
    project: &Project,
    symbol: &str,
    frame: u32,
    include_guides: bool,
    show_masking: bool,
    button_frame: &dyn Fn(&[Id]) -> Option<u32>,
) -> AuthoredScene {
    let evaluation = crate::evaluate::snapshot_mode(
        project,
        symbol,
        frame,
        &|_| None,
        &|path| {
            button_frame(path).map(|frame| crate::evaluate::DisplayPatch {
                button_frame: Some(frame),
                ..Default::default()
            })
        },
        include_guides,
    );
    scene_from_evaluation(project, evaluation, show_masking, Some(symbol))
}

/// Shared authored geometry/mask assembly. Only the edited symbol's layer
/// preferences affect authoring outlines; transport preview passes no scope.
pub(crate) fn scene_from_evaluation(
    project: &Project,
    evaluation: crate::evaluate::Evaluation,
    show_masking: bool,
    outline_symbol: Option<&str>,
) -> AuthoredScene {
    // Outline is a local editing preference, not inherited published artwork.
    // An outlined layer/folder in the edited timeline colors its whole subtree,
    // while preferences saved inside nested symbols stay local to those symbols.
    let mut outlines = HashMap::new();
    if let Some(symbol) = outline_symbol.and_then(|id| project.symbol(id)) {
        for layer in &symbol.timeline.layers {
            if let Some(color) = layer_effective_outline_color(&symbol.timeline, layer) {
                for element in layer.keyframes.iter().flat_map(|key| &key.elements) {
                    outlines.insert(element.id.as_str(), color);
                }
            }
        }
    }
    let element_blends: HashMap<Vec<Id>, BlendMode> = evaluation
        .nodes
        .iter()
        .map(|node| (node.path.clone(), node.element_blend_mode))
        .collect();
    let mut scene = AuthoredScene {
        draws: evaluation
            .items
            .into_iter()
            .map(|item| {
                let outline_color = item
                    .instance_path
                    .iter()
                    .find_map(|id| outlines.get(id.as_str()).copied());
                let blend_mode = element_blends
                    .get(&item.instance_path)
                    .copied()
                    .unwrap_or(BlendMode::Normal);
                AuthoredDraw {
                    item,
                    clip_ids: vec![],
                    outline_color,
                    mask_group: None,
                    blend_mode,
                }
            })
            .collect(),
        clips: vec![],
        mask_groups: vec![],
    };
    if show_masking {
        // Attach all relationships first: a mask inside a source symbol must clip
        // that source's painted content before it becomes the outer mask.
        for plan in &evaluation.masks {
            for (index, (_, roots)) in plan.target_groups.iter().enumerate() {
                for draw in &mut scene.draws {
                    if roots.iter().any(|p| draw.item.instance_path.starts_with(p)) {
                        draw.clip_ids.push(target_clip_id(
                            &plan.id,
                            plan.target_groups.len(),
                            index,
                        ));
                    }
                }
            }
        }
        let source_owner = |path: &[Id]| {
            evaluation
                .masks
                .iter()
                .enumerate()
                .flat_map(|(index, plan)| {
                    plan.sources
                        .iter()
                        .filter(move |p| path.starts_with(p))
                        .map(move |p| (p.len(), index))
                })
                .max()
                .map(|(_, index)| index)
        };
        for (index, plan) in evaluation.masks.iter().enumerate() {
            // An ancestor target's mask is deferred until that target is composed.
            // It must not also attenuate a mask source inside the target subtree.
            let inherited: HashSet<_> = evaluation
                .masks
                .iter()
                .flat_map(|other| {
                    other
                        .target_groups
                        .iter()
                        .enumerate()
                        .filter(|(_, (_, roots))| {
                            plan.sources
                                .iter()
                                .any(|source| roots.iter().any(|target| source.starts_with(target)))
                        })
                        .map(|(index, _)| {
                            target_clip_id(&other.id, other.target_groups.len(), index)
                        })
                })
                .collect();
            let source_clips = |ids: &[String]| {
                ids.iter()
                    .filter(|id| !inherited.contains(*id))
                    .cloned()
                    .collect()
            };
            let sources: Vec<_> = scene
                .draws
                .iter()
                .filter(|draw| source_owner(&draw.item.instance_path) == Some(index))
                .collect();
            let luminance = plan.mode == MaskMode::Luminance;
            let alpha_components = plan.mode.uses_paint_coverage().then(|| {
                sources
                    .iter()
                    .filter_map(|draw| {
                        alpha_component(
                            &draw.item.kind,
                            draw.item.transform,
                            draw.item.color_transform,
                            source_clips(&draw.clip_ids),
                            luminance,
                        )
                    })
                    .collect::<Vec<_>>()
            });
            let components = if let Some(alpha) = &alpha_components {
                alpha.iter().map(alpha_geometry).collect()
            } else {
                sources
                    .iter()
                    .filter_map(|draw| {
                        crate::runtime_graph::clipping::vector_path(&draw.item.kind).map(
                            |(commands, fill_rule)| ClipComponent {
                                commands,
                                fill_rule,
                                matrix: draw.item.transform,
                                clip_ids: source_clips(&draw.clip_ids),
                            },
                        )
                    })
                    .collect()
            };
            scene.clips.push(RuntimeClip {
                id: plan.id.clone(),
                components,
                alpha_components,
                invert: plan.invert,
            });
        }
        scene
            .draws
            .retain(|draw| source_owner(&draw.item.instance_path).is_none());
        for plan in &evaluation.masks {
            alias_target_clips(&mut scene.clips, &plan.id, plan.target_groups.len());
        }
        crate::mask_composition::factor_alpha_clip_groups(&mut scene.clips);
    }

    struct GroupEntry {
        id: String,
        depth: usize,
        tier: usize,
        roots: Vec<Vec<Id>>,
        clip_ids: Vec<String>,
        blend_mode: BlendMode,
    }
    let mut groups: Vec<GroupEntry> = if show_masking {
        evaluation
            .masks
            .iter()
            .filter(|plan| clip_uses_alpha(&scene.clips, &plan.id, 0))
            .flat_map(|plan| {
                plan.target_groups
                    .iter()
                    .enumerate()
                    .map(move |(index, (id, roots))| GroupEntry {
                        id: id.clone(),
                        depth: roots.first().map_or(0, Vec::len),
                        tier: 150,
                        roots: roots.clone(),
                        clip_ids: vec![target_clip_id(&plan.id, plan.target_groups.len(), index)],
                        blend_mode: BlendMode::Normal,
                    })
            })
            .collect()
    } else {
        Vec::new()
    };
    for plan in &evaluation.blend_groups {
        if plan.tier == 100
            && let Some(existing) = groups
                .iter_mut()
                .find(|g| g.tier == 150 && g.roots == plan.roots && g.blend_mode.is_normal())
        {
            existing.blend_mode = plan.blend_mode;
            continue;
        }
        groups.push(GroupEntry {
            id: plan.id.clone(),
            depth: plan.depth,
            tier: plan.tier,
            roots: plan.roots.clone(),
            clip_ids: vec![],
            blend_mode: plan.blend_mode,
        });
    }
    let matching_groups = |path: &[Id]| {
        let mut matches: Vec<_> = groups
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry
                    .roots
                    .iter()
                    .filter(|root| path.starts_with(root))
                    .map(Vec::len)
                    .max()
                    .map(|depth| ((depth, entry.tier, index), index))
            })
            .collect();
        matches.sort_unstable();
        matches
    };
    for (index, entry) in groups.iter().enumerate() {
        let my_key = (entry.depth, entry.tier, index);
        let parent = entry.roots.first().and_then(|root| {
            matching_groups(root)
                .into_iter()
                .rfind(|(key, _)| *key < my_key)
                .map(|(_, found)| groups[found].id.clone())
        });
        scene.mask_groups.push(AuthoredMaskGroup {
            id: entry.id.clone(),
            parent_id: parent,
            clip_ids: entry.clip_ids.clone(),
            blend_mode: entry.blend_mode,
        });
    }
    for draw in &mut scene.draws {
        draw.mask_group = matching_groups(&draw.item.instance_path)
            .last()
            .map(|(_, index)| groups[*index].id.clone());
    }

    scene
}

pub(crate) struct RuntimeLayerMask {
    pub id: String,
    pub mode: MaskMode,
    pub invert: bool,
    pub sources: Vec<ObjectRef>,
    pub target_groups: Vec<(String, Vec<ObjectRef>)>,
}
impl LayerMaskPlan {
    pub fn resolve(
        &self,
        namespace: &str,
        resolve: impl Fn(&[Id]) -> Option<ObjectRef>,
    ) -> RuntimeLayerMask {
        RuntimeLayerMask {
            id: format!("{namespace}:{}", self.id),
            target_groups: self
                .target_groups
                .iter()
                .map(|(id, roots)| {
                    (
                        format!("{namespace}:{id}"),
                        roots.iter().filter_map(|p| resolve(p)).collect(),
                    )
                })
                .collect(),
            mode: self.mode,
            invert: self.invert,
            sources: self.sources.iter().filter_map(|p| resolve(p)).collect(),
        }
    }
}
pub(crate) fn apply_runtime_masks(snapshot: &mut RuntimeSnapshot, plans: &[RuntimeLayerMask]) {
    if plans.is_empty() {
        return;
    }
    let parents: HashMap<_, _> = snapshot
        .objects
        .iter()
        .map(|node| (node.object_ref.clone(), node.parent_ref.clone()))
        .collect();
    let belongs = |reference: &ObjectRef, roots: &[ObjectRef]| {
        let mut current = Some(reference);
        for _ in 0..=parents.len() {
            let Some(value) = current else {
                return false;
            };
            if roots.contains(value) {
                return true;
            }
            current = parents.get(value).and_then(|p| p.as_ref());
        }
        false
    };
    // Record target clips before capturing any source; nested masks are
    // expressed by component clip IDs, not flattened contour winding.
    for plan in plans {
        for (index, (_, roots)) in plan.target_groups.iter().enumerate() {
            let clip = target_clip_id(&plan.id, plan.target_groups.len(), index);
            for node in &mut snapshot.objects {
                if belongs(&node.object_ref, roots) {
                    node.clip_ids.push(clip.clone());
                }
            }
            for draw in &mut snapshot.draws {
                if belongs(&draw.owner_ref, roots) {
                    draw.clip_ids.push(clip.clone());
                }
            }
            for node in &mut snapshot.semantics {
                if belongs(&node.owner_ref, roots) {
                    node.clip_ids.push(clip.clone());
                }
            }
        }
    }
    let source_owner = |reference: &ObjectRef| {
        // The nearest source ancestor owns this primitive. An outer source
        // excludes its inner mask artwork but includes its clipped targets.
        let mut current = Some(reference);
        for _ in 0..=parents.len() {
            let value = current?;
            if let Some(index) = plans.iter().position(|plan| plan.sources.contains(value)) {
                return Some(index);
            }
            current = parents.get(value).and_then(|p| p.as_ref());
        }
        None
    };
    for (index, plan) in plans.iter().enumerate() {
        let inherited: HashSet<_> = plans
            .iter()
            .flat_map(|other| {
                other
                    .target_groups
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, roots))| {
                        plan.sources.iter().any(|source| belongs(source, roots))
                    })
                    .map(|(index, _)| target_clip_id(&other.id, other.target_groups.len(), index))
            })
            .collect();
        let source_clips = |ids: &[String]| {
            ids.iter()
                .filter(|id| !inherited.contains(*id))
                .cloned()
                .collect()
        };
        let sources: Vec<_> = snapshot
            .draws
            .iter()
            .filter(|draw| source_owner(&draw.owner_ref) == Some(index))
            .collect();
        let luminance = plan.mode == MaskMode::Luminance;
        let alpha_components = plan.mode.uses_paint_coverage().then(|| {
            sources
                .iter()
                .filter_map(|draw| {
                    runtime_alpha_component(draw, luminance).map(|mut component| {
                        component.clip_ids = source_clips(&component.clip_ids);
                        component
                    })
                })
                .collect::<Vec<_>>()
        });
        let components = if let Some(alpha) = &alpha_components {
            alpha.iter().map(alpha_geometry).collect()
        } else {
            sources
                .iter()
                .filter_map(|draw| {
                    let geometry = match &draw.item.kind {
                        RuntimeDrawKind::Element(kind) => {
                            crate::runtime_graph::clipping::vector_path(kind)
                        }
                        RuntimeDrawKind::GradientPath {
                            commands,
                            fill_rule,
                            ..
                        } => Some((commands.clone(), *fill_rule)),
                        _ => None,
                    };
                    geometry.map(|(commands, fill_rule)| ClipComponent {
                        commands,
                        fill_rule,
                        matrix: draw.item.transform,
                        clip_ids: source_clips(&draw.clip_ids),
                    })
                })
                .collect()
        };
        snapshot.clips.push(RuntimeClip {
            id: plan.id.clone(),
            components,
            alpha_components,
            invert: plan.invert,
        });
    }
    for plan in plans {
        alias_target_clips(&mut snapshot.clips, &plan.id, plan.target_groups.len());
    }
    let empty: HashSet<_> = snapshot
        .clips
        .iter()
        .filter(|clip| !clip.invert && clip.components.is_empty())
        .map(|clip| clip.id.as_str())
        .collect();
    for node in &mut snapshot.objects {
        if source_owner(&node.object_ref).is_some() {
            node.authored_mask_source = true;
            node.display.effective_visible = false;
        }
        if node.clip_ids.iter().any(|id| empty.contains(id.as_str())) {
            node.display.effective_visible = false;
        }
    }
    snapshot
        .draws
        .retain(|draw| source_owner(&draw.owner_ref).is_none());
    snapshot
        .semantics
        .retain(|node| source_owner(&node.owner_ref).is_none());
}

fn alpha_geometry(component: &AlphaClipComponent) -> ClipComponent {
    ClipComponent {
        commands: component.commands.clone(),
        fill_rule: component.fill_rule,
        matrix: component.matrix,
        clip_ids: component.clip_ids.clone(),
    }
}

/// Preserve the same paint data in pure authoring and script mask snapshots.
/// `color` already includes placement and ancestor alpha.
pub(crate) fn alpha_component(
    kind: &ElementKind,
    matrix: [f64; 6],
    color: ColorTransform,
    clip_ids: Vec<String>,
    luminance: bool,
) -> Option<AlphaClipComponent> {
    let mut fill_gradient = None;
    let mut resource_bitmap = None;
    let (commands, fill_rule, alpha) = match kind {
        ElementKind::Bitmap {
            resource_id,
            width,
            height,
            smoothing,
        } => {
            resource_bitmap = Some(MaskResourceBitmap {
                resource_id: resource_id.clone(),
                width: *width,
                height: *height,
                smoothing: *smoothing,
                color_transform: color,
            });
            (
                mask_rectangle(f64::from(*width), f64::from(*height)),
                FillRule::NonZero,
                1.,
            )
        }
        ElementKind::Rectangle { fill, .. }
        | ElementKind::Ellipse { fill, .. }
        | ElementKind::Path { fill, .. }
        | ElementKind::PaintedPath { fill, .. } => {
            let (commands, rule) = crate::runtime_graph::clipping::vector_path(kind)?;
            let alpha = if let ElementKind::PaintedPath {
                fill_gradient: Some(gradient),
                ..
            } = kind
            {
                let mut gradient = gradient.clone();
                for stop in &mut gradient.stops {
                    stop.color = color.apply(stop.color);
                }
                fill_gradient = Some(gradient);
                1.
            } else {
                let c = color.apply(*fill);
                let base_alpha = f64::from(c[3]) / 255.;
                if luminance {
                    base_alpha * bt709_luminance(c[0], c[1], c[2])
                } else {
                    base_alpha
                }
            };
            (commands, rule, alpha)
        }
        _ => return None,
    };
    Some(AlphaClipComponent {
        commands,
        fill_rule,
        matrix,
        clip_ids,
        alpha,
        fill_gradient,
        fill_bitmap: None,
        resource_bitmap,
        luminance,
    })
}

fn mask_rectangle(width: f64, height: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo { x: 0., y: 0. },
        PathCommand::LineTo { x: width, y: 0. },
        PathCommand::LineTo {
            x: width,
            y: height,
        },
        PathCommand::LineTo { x: 0., y: height },
        PathCommand::Close,
    ]
}

fn runtime_alpha_component(draw: &RuntimeDrawItem, luminance: bool) -> Option<AlphaClipComponent> {
    match &draw.item.kind {
        RuntimeDrawKind::Element(kind) => alpha_component(
            kind,
            draw.item.transform,
            draw.item.color_transform,
            draw.clip_ids.clone(),
            luminance,
        ),
        RuntimeDrawKind::GradientPath {
            commands,
            fill_rule,
            fill,
            fill_gradient,
            fill_bitmap,
            ..
        } => {
            // Runtime Graphics paints are already color-transformed.
            let alpha = if fill_gradient.is_some() || fill_bitmap.is_some() {
                1.
            } else {
                let base_alpha = f64::from(fill[3]) / 255.;
                if luminance {
                    base_alpha * bt709_luminance(fill[0], fill[1], fill[2])
                } else {
                    base_alpha
                }
            };
            Some(AlphaClipComponent {
                commands: commands.clone(),
                fill_rule: *fill_rule,
                matrix: draw.item.transform,
                clip_ids: draw.clip_ids.clone(),
                alpha,
                fill_gradient: fill_gradient.clone(),
                fill_bitmap: fill_bitmap.clone(),
                resource_bitmap: None,
                luminance,
            })
        }
        RuntimeDrawKind::BitmapPixels {
            pixel_ref,
            width,
            height,
            revision,
            smoothing,
            color_transform,
            ..
        } => Some(AlphaClipComponent {
            commands: mask_rectangle(f64::from(*width), f64::from(*height)),
            fill_rule: FillRule::NonZero,
            matrix: draw.item.transform,
            clip_ids: draw.clip_ids.clone(),
            alpha: 1.,
            fill_gradient: None,
            resource_bitmap: None,
            fill_bitmap: Some(BitmapPaint {
                pixel_ref: pixel_ref.clone(),
                width: *width,
                height: *height,
                revision: revision.clone(),
                matrix: [1., 0., 0., 1., 0., 0.],
                repeat: false,
                smoothing: *smoothing,
                color_transform: *color_transform,
            }),
            luminance,
        }),
        _ => None,
    }
}

// Distinct linked target layers are independent composites even when they use
// the same mask paint. Alias IDs preserve that identity inside nested sources.
fn target_clip_id(id: &str, count: usize, index: usize) -> String {
    if count <= 1 {
        id.into()
    } else {
        format!("{id}:target:{index}")
    }
}
fn alias_target_clips(clips: &mut Vec<RuntimeClip>, id: &str, count: usize) {
    if count <= 1 {
        return;
    }
    if let Some(template) = clips.iter().find(|clip| clip.id == id).cloned() {
        for index in 0..count {
            clips.push(RuntimeClip {
                id: target_clip_id(id, count, index),
                ..template.clone()
            });
        }
    }
}

fn clip_uses_alpha(clips: &[RuntimeClip], id: &str, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    clips.iter().find(|clip| clip.id == id).is_some_and(|clip| {
        clip.alpha_components.is_some()
            || clip.invert
            || clip
                .components
                .iter()
                .flat_map(|c| &c.clip_ids)
                .any(|id| clip_uses_alpha(clips, id, depth + 1))
    })
}

/// Isolate each alpha-masked target layer before its shared coverage is applied.
/// Existing object filter groups remain inside this layer group; ancestor groups
/// remain outside. This prevents overlapping targets multiplying alpha twice.
pub(crate) fn isolate_runtime_alpha_layers(
    snapshot: &mut RuntimeSnapshot,
    plans: &[RuntimeLayerMask],
) {
    let parents: HashMap<_, _> = snapshot
        .objects
        .iter()
        .map(|node| (node.object_ref.clone(), node.parent_ref.clone()))
        .collect();
    let belongs = |reference: &ObjectRef, roots: &[ObjectRef]| {
        let mut current = Some(reference);
        for _ in 0..=parents.len() {
            let Some(value) = current else {
                return false;
            };
            if roots.contains(value) {
                return true;
            }
            current = parents.get(value).and_then(|p| p.as_ref());
        }
        false
    };
    // Evaluation orders outer timelines before nested timelines. Inserting outer
    // groups first lets the nested layer use it as its nearest external parent.
    for plan in plans
        .iter()
        .filter(|plan| clip_uses_alpha(&snapshot.clips, &plan.id, 0))
    {
        for (index, (id, roots)) in plan.target_groups.iter().enumerate() {
            let Some(first) = snapshot
                .draws
                .iter()
                .find(|draw| belongs(&draw.owner_ref, roots))
            else {
                continue;
            };
            let mut parent = first.filter_id.clone();
            while let Some(group) = parent
                .as_ref()
                .and_then(|id| snapshot.filter_groups.iter().find(|g| &g.id == id))
            {
                if !belongs(&group.owner_ref, roots) {
                    break;
                }
                parent = group.parent_id.clone();
            }
            let owner_ref = roots.first().expect("nonempty matched layer").clone();
            let mut clip_ids = Vec::new();
            let mut ancestor = parent.as_ref();
            for _ in 0..=snapshot.filter_groups.len() {
                let Some(group) =
                    ancestor.and_then(|id| snapshot.filter_groups.iter().find(|g| &g.id == id))
                else {
                    break;
                };
                for clip in &group.clip_ids {
                    if !clip_ids.contains(clip) {
                        clip_ids.push(clip.clone());
                    }
                }
                ancestor = group.parent_id.as_ref();
            }
            let own_clip = target_clip_id(&plan.id, plan.target_groups.len(), index);
            if !clip_ids.contains(&own_clip) {
                clip_ids.push(own_clip);
            }
            let owned: HashSet<_> = snapshot
                .filter_groups
                .iter()
                .filter(|g| belongs(&g.owner_ref, roots))
                .map(|g| g.id.clone())
                .collect();
            for group in &mut snapshot.filter_groups {
                if owned.contains(&group.id)
                    && group.parent_id.as_ref().is_none_or(|p| !owned.contains(p))
                {
                    group.parent_id = Some(id.clone());
                }
            }
            for draw in &mut snapshot.draws {
                if belongs(&draw.owner_ref, roots)
                    && draw.filter_id.as_ref().is_none_or(|id| !owned.contains(id))
                {
                    draw.filter_id = Some(id.clone());
                }
            }
            snapshot.filter_groups.push(RuntimeFilterGroup {
                id: id.clone(),
                owner_ref,
                parent_id: parent,
                filters: vec![],
                clip_ids,
                composite_operation: "source-over".into(),
                opaque_background: None,
                cache_as_bitmap: false,
                alpha_masks: vec![],
            });
        }
    }
}

pub(crate) fn validate_layers(project: &Project, symbol: &Symbol) -> Vec<String> {
    let mut errors = vec![];
    let layers = &symbol.timeline.layers;
    if project.format_version < 4 && layers.iter().any(|l| !l.properties.is_default()) {
        errors.push("Layer roles, masks and folders require document format 4".into());
    }
    fn source_kind(
        project: &Project,
        kind: &ElementKind,
        mode: MaskMode,
        visiting: &mut HashSet<Id>,
    ) -> bool {
        match kind {
            ElementKind::Instance { symbol_id, .. } => {
                let Some(symbol) = project.symbol(symbol_id) else {
                    return false;
                };
                if !visiting.insert(symbol.id.clone()) {
                    return false;
                }
                let valid = symbol
                    .timeline
                    .layers
                    .iter()
                    .filter(|l| l.properties.role != LayerRole::Guide)
                    .all(|layer| {
                        let mode = if layer.properties.role == LayerRole::Mask {
                            layer.properties.mask_mode
                        } else {
                            mode
                        };
                        layer
                            .keyframes
                            .iter()
                            .flat_map(|k| &k.elements)
                            .all(|e| source_kind(project, &e.kind, mode, visiting))
                    });
                visiting.remove(&symbol.id);
                valid
            }
            ElementKind::Bitmap { resource_id, .. } if mode.uses_paint_coverage() => project
                .resources
                .iter()
                .find(|resource| &resource.id == resource_id)
                .is_some_and(|resource| resource.decode_image().is_ok()),
            kind => crate::runtime_graph::clipping::vector_path(kind).is_some(),
        }
    }
    for layer in layers {
        if layer.properties.role == LayerRole::Folder
            && layer.keyframes.iter().any(|k| !k.elements.is_empty())
        {
            errors.push(format!(
                "Folder '{}' must not contain artwork; move artwork into a child layer",
                layer.name
            ));
        }
        if let Some(id) = &layer.properties.mask
            && (id == &layer.id
                || !layers
                    .iter()
                    .any(|l| &l.id == id && l.properties.role == LayerRole::Mask)
                || layer.properties.role != LayerRole::Normal)
        {
            errors.push(format!(
                "Layer '{}' must reference another mask layer from a normal layer",
                layer.name
            ));
        }
        if let Some(id) = &layer.properties.motion_guide
            && (id == &layer.id
                || !layers
                    .iter()
                    .any(|l| &l.id == id && l.properties.role == LayerRole::Guide)
                || layer.properties.role != LayerRole::Normal)
        {
            errors.push(format!(
                "Layer '{}' must reference another guide layer as its motion guide from a normal layer",
                layer.name
            ));
        }
        let mut keys: Vec<_> = layer.keyframes.iter().collect();
        keys.sort_by_key(|k| k.frame);
        let mut in_motion = false;
        for key in keys {
            if key.keyed_properties.is_some() {
                if !in_motion {
                    errors.push(format!(
                        "Layer '{}' has a property keyframe at frame {} outside a motion tween",
                        layer.name,
                        key.frame + 1
                    ));
                }
            } else {
                in_motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
            }
        }
        if let Some(id) = &layer.properties.parent {
            let mut seen = HashSet::from([layer.id.clone()]);
            let mut next = Some(id);
            while let Some(id) = next {
                if !seen.insert(id.clone()) {
                    errors.push(format!("Layer '{}' has cyclic folder ancestry", layer.name));
                    break;
                }
                let Some(parent) = layers
                    .iter()
                    .find(|l| &l.id == id && l.properties.role == LayerRole::Folder)
                else {
                    errors.push(format!(
                        "Layer '{}' references a missing folder",
                        layer.name
                    ));
                    break;
                };
                next = parent.properties.parent.as_ref();
            }
        }
        if !layer.properties.mask_mode.is_geometry() && layer.properties.role != LayerRole::Mask {
            errors.push(format!(
                "Layer '{}' can use {} mask mode only with the Mask role",
                layer.name,
                match layer.properties.mask_mode {
                    MaskMode::Alpha => "alpha",
                    MaskMode::Luminance => "luminance",
                    MaskMode::Geometry => "geometry",
                }
            ));
        }
        if layer.properties.mask_invert && layer.properties.role != LayerRole::Mask {
            errors.push(format!(
                "Layer '{}' can invert a mask only with the Mask role",
                layer.name
            ));
        }
        if layer.properties.role == LayerRole::Mask {
            let valid = layer
                .keyframes
                .iter()
                .flat_map(|key| &key.elements)
                .all(|e| {
                    source_kind(
                        project,
                        &e.kind,
                        layer.properties.mask_mode,
                        &mut HashSet::new(),
                    )
                });
            if !valid {
                errors.push(format!("Mask '{}' requires vector fills or nested vector symbols in Geometry mode; Alpha mode (and Luminance mode) also accepts decodable bitmap images and mixed symbols. Convert text/video to supported artwork", layer.name));
            }
        }
    }
    errors
}
