//! Layer relationships shared by authoring and published runtime snapshots.
use crate::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(crate) struct LayerMaskPlan {
    pub id: String,
    pub sources: Vec<Vec<Id>>,
    pub targets: Vec<Vec<Id>>,
}
#[derive(Clone, Debug)]
pub struct AuthoredDraw {
    pub item: DrawItem,
    pub clip_ids: Vec<String>,
    pub outline_color: Option<Color>,
}
#[derive(Clone, Debug, Default)]
pub struct AuthoredScene {
    pub draws: Vec<AuthoredDraw>,
    pub clips: Vec<RuntimeClip>,
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
impl AuthoredScene {
    pub fn contains_point(&self, path: &[Id], point: [f64; 2]) -> bool {
        let Some(draw) = self
            .draws
            .iter()
            .find(|draw| draw.item.instance_path == path)
        else {
            return false;
        };
        draw.clip_ids.iter().all(|id| {
            self.clips
                .iter()
                .find(|clip| &clip.id == id)
                .is_some_and(|clip| {
                    clip.components.iter().any(|component| {
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
                    })
                })
        })
    }
}

pub(crate) fn layer_mask_plans(
    timeline: &Timeline,
    frame: u32,
    parent: &[Id],
) -> Vec<LayerMaskPlan> {
    let paths = |layer: &Layer| -> Vec<Vec<Id>> {
        if !layer_effectively_visible(timeline, layer) {
            return vec![];
        }
        sample_layer(layer, frame)
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

pub fn evaluate_scene(
    project: &Project,
    symbol: &str,
    frame: u32,
    include_guides: bool,
    show_masking: bool,
) -> AuthoredScene {
    let evaluation = crate::evaluate::snapshot_mode(
        project,
        symbol,
        frame,
        &|_| None,
        &|_| None,
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
    let mut scene = AuthoredScene {
        draws: evaluation
            .items
            .into_iter()
            .map(|item| {
                let outline_color = item
                    .instance_path
                    .iter()
                    .find_map(|id| outlines.get(id.as_str()).copied());
                AuthoredDraw {
                    item,
                    clip_ids: vec![],
                    outline_color,
                }
            })
            .collect(),
        clips: vec![],
    };
    if !show_masking {
        return scene;
    }
    let mut consumed = HashSet::new();
    for plan in evaluation.masks {
        let components = scene
            .draws
            .iter()
            .filter(|draw| {
                plan.sources
                    .iter()
                    .any(|p| draw.item.instance_path.starts_with(p))
            })
            .filter_map(|draw| {
                crate::runtime_graph::clipping::vector_path(&draw.item.kind).map(
                    |(commands, fill_rule)| ClipComponent {
                        commands,
                        fill_rule,
                        matrix: draw.item.transform,
                    },
                )
            })
            .collect();
        for draw in &mut scene.draws {
            if plan
                .sources
                .iter()
                .any(|p| draw.item.instance_path.starts_with(p))
            {
                consumed.insert(draw.item.instance_path.clone());
            }
            if plan
                .targets
                .iter()
                .any(|p| draw.item.instance_path.starts_with(p))
            {
                draw.clip_ids.push(plan.id.clone());
            }
        }
        scene.clips.push(RuntimeClip {
            id: plan.id,
            components,
        });
    }
    scene
        .draws
        .retain(|draw| !consumed.contains(&draw.item.instance_path));
    scene
}

pub(crate) struct RuntimeLayerMask {
    pub id: String,
    pub sources: Vec<ObjectRef>,
    pub targets: Vec<ObjectRef>,
}
impl LayerMaskPlan {
    pub fn resolve(
        &self,
        namespace: &str,
        resolve: impl Fn(&[Id]) -> Option<ObjectRef>,
    ) -> RuntimeLayerMask {
        RuntimeLayerMask {
            id: format!("{namespace}:{}", self.id),
            sources: self.sources.iter().filter_map(|p| resolve(p)).collect(),
            targets: self.targets.iter().filter_map(|p| resolve(p)).collect(),
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
    let mut consumed = HashSet::new();
    for plan in plans {
        let components: Vec<_> = snapshot
            .draws
            .iter()
            .filter(|draw| belongs(&draw.owner_ref, &plan.sources))
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
                })
            })
            .collect();
        let empty = components.is_empty();
        for node in &mut snapshot.objects {
            if belongs(&node.object_ref, &plan.sources) {
                consumed.insert(node.object_ref.clone());
                node.display.effective_visible = false;
            }
            if belongs(&node.object_ref, &plan.targets) {
                node.clip_ids.push(plan.id.clone());
                if empty {
                    node.display.effective_visible = false;
                }
            }
        }
        for draw in &mut snapshot.draws {
            if belongs(&draw.owner_ref, &plan.targets) {
                draw.clip_ids.push(plan.id.clone());
            }
        }
        for semantic in &mut snapshot.semantics {
            if belongs(&semantic.owner_ref, &plan.targets) {
                semantic.clip_ids.push(plan.id.clone());
            }
        }
        snapshot.clips.push(RuntimeClip {
            id: plan.id.clone(),
            components,
        });
    }
    snapshot
        .draws
        .retain(|draw| !consumed.contains(&draw.owner_ref));
    snapshot
        .semantics
        .retain(|node| !consumed.contains(&node.owner_ref));
}

pub(crate) fn validate_layers(project: &Project, symbol: &Symbol) -> Vec<String> {
    let mut errors = vec![];
    let layers = &symbol.timeline.layers;
    if project.format_version < 4 && layers.iter().any(|l| !l.properties.is_default()) {
        errors.push("Layer roles, masks and folders require document format 4".into());
    }
    fn vector_symbol(project: &Project, symbol: &Symbol, visiting: &mut HashSet<Id>) -> bool {
        if !visiting.insert(symbol.id.clone()) {
            return false;
        }
        let valid = symbol
            .timeline
            .layers
            .iter()
            .filter(|l| l.properties.role != LayerRole::Guide)
            .all(|l| {
                l.properties.role != LayerRole::Mask
                    && l.keyframes
                        .iter()
                        .flat_map(|k| &k.elements)
                        .all(|e| match &e.kind {
                            ElementKind::Instance { symbol_id, .. } => project
                                .symbol(symbol_id)
                                .is_some_and(|s| vector_symbol(project, s, visiting)),
                            kind => crate::runtime_graph::clipping::vector_path(kind).is_some(),
                        })
            });
        visiting.remove(&symbol.id);
        valid
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
        if layer.properties.role == LayerRole::Mask {
            let valid = layer
                .keyframes
                .iter()
                .flat_map(|k| &k.elements)
                .all(|e| match &e.kind {
                    ElementKind::Instance { symbol_id, .. } => project
                        .symbol(symbol_id)
                        .is_some_and(|s| vector_symbol(project, s, &mut HashSet::new())),
                    kind => crate::runtime_graph::clipping::vector_path(kind).is_some(),
                });
            if !valid {
                errors.push(format!("Mask '{}' supports vector fills and vector symbols; convert text/bitmaps to vector shapes and remove nested mask roles", layer.name));
            }
        }
    }
    errors
}
