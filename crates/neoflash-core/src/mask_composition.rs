//! Normalize nested alpha sources once, before either renderer consumes them.
//! Sibling paint coverage is unioned before a shared inner mask is applied.
use crate::{
    AlphaClipComponent, ClipComponent, FillRule, PathBounds, PathCommand, RuntimeClip, path_bounds,
};
use std::collections::{BTreeMap, HashSet};

/// Each synthetic union replaces at least two components, so the generated graph
/// is bounded by the input component count. Its nesting is the existing clip-ID
/// depth; normalization does not evaluate pixels or mutate the document.
pub(crate) fn factor_alpha_clip_groups(clips: &mut Vec<RuntimeClip>) {
    let mut ids: HashSet<_> = clips.iter().map(|clip| clip.id.clone()).collect();
    let original_count = clips.len();
    let mut extra = Vec::new();
    for clip in &mut clips[..original_count] {
        if let Some(components) = clip.alpha_components.take() {
            let mut sequence = 0;
            let components = factor(components, &clip.id, &mut sequence, &mut ids, &mut extra);
            clip.components = components.iter().map(geometry).collect();
            clip.alpha_components = Some(components);
        }
    }
    clips.extend(extra);
}

fn factor(
    components: Vec<AlphaClipComponent>,
    owner: &str,
    sequence: &mut usize,
    ids: &mut HashSet<String>,
    extra: &mut Vec<RuntimeClip>,
) -> Vec<AlphaClipComponent> {
    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (index, component) in components.iter().enumerate() {
        if let Some(id) = component.clip_ids.first() {
            groups.entry(id.clone()).or_default().push(index);
        }
    }
    let mut components: Vec<_> = components.into_iter().map(Some).collect();
    for (shared, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let children: Vec<_> = indices
            .iter()
            .map(|index| {
                let mut component = components[*index].take().expect("unique component index");
                component.clip_ids.remove(0);
                component
            })
            .collect();
        let bounds = union_bounds(&children);
        let children = factor(children, owner, sequence, ids, extra);
        let union_id = loop {
            let candidate = format!("mask-union:{owner}:{}", *sequence);
            *sequence += 1;
            if ids.insert(candidate.clone()) {
                break candidate;
            }
        };
        extra.push(RuntimeClip {
            id: union_id.clone(),
            components: children.iter().map(geometry).collect(),
            alpha_components: Some(children),
        });
        // A conservative world-space bounds rectangle carries the synthetic
        // union and the shared mask. Exact curves/holes remain in its children.
        let commands = bounds
            .map(|b| {
                vec![
                    PathCommand::MoveTo {
                        x: b.x_min,
                        y: b.y_min,
                    },
                    PathCommand::LineTo {
                        x: b.x_max,
                        y: b.y_min,
                    },
                    PathCommand::LineTo {
                        x: b.x_max,
                        y: b.y_max,
                    },
                    PathCommand::LineTo {
                        x: b.x_min,
                        y: b.y_max,
                    },
                    PathCommand::Close,
                ]
            })
            .unwrap_or_default();
        components[indices[0]] = Some(AlphaClipComponent {
            commands,
            fill_rule: FillRule::NonZero,
            matrix: [1., 0., 0., 1., 0., 0.],
            alpha: 1.,
            clip_ids: vec![union_id, shared],
            fill_gradient: None,
            fill_bitmap: None,
            resource_bitmap: None,
        });
    }
    components.into_iter().flatten().collect()
}

fn geometry(component: &AlphaClipComponent) -> ClipComponent {
    ClipComponent {
        commands: component.commands.clone(),
        fill_rule: component.fill_rule,
        matrix: component.matrix,
        clip_ids: component.clip_ids.clone(),
    }
}

fn union_bounds(components: &[AlphaClipComponent]) -> Option<PathBounds> {
    let mut result: Option<PathBounds> = None;
    for component in components {
        let Some(bounds) = path_bounds(&component.commands).ok().flatten() else {
            continue;
        };
        let [a, b, c, d, tx, ty] = component.matrix;
        for [x, y] in [
            [bounds.x_min, bounds.y_min],
            [bounds.x_max, bounds.y_min],
            [bounds.x_max, bounds.y_max],
            [bounds.x_min, bounds.y_max],
        ] {
            let (x, y) = (a * x + c * y + tx, b * x + d * y + ty);
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            if let Some(bounds) = &mut result {
                bounds.x_min = bounds.x_min.min(x);
                bounds.y_min = bounds.y_min.min(y);
                bounds.x_max = bounds.x_max.max(x);
                bounds.y_max = bounds.y_max.max(y);
            } else {
                result = Some(PathBounds {
                    x_min: x,
                    y_min: y,
                    x_max: x,
                    y_max: y,
                });
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MaskCoverage, Project};
    fn component(x: f64, alpha: f64, clip_ids: Vec<String>) -> AlphaClipComponent {
        AlphaClipComponent {
            commands: vec![
                PathCommand::MoveTo { x: 0., y: 0. },
                PathCommand::LineTo { x: 100., y: 0. },
                PathCommand::LineTo { x: 100., y: 100. },
                PathCommand::LineTo { x: 0., y: 100. },
                PathCommand::Close,
            ],
            fill_rule: FillRule::NonZero,
            matrix: [1., 0., 0., 1., x, 0.],
            alpha,
            clip_ids,
            fill_gradient: None,
            fill_bitmap: None,
            resource_bitmap: None,
        }
    }
    #[test]
    fn nested_soft_mask_is_applied_once_to_union_including_holes_and_transforms() {
        let mut hole = component(0., 0.5, vec![]);
        hole.fill_rule = FillRule::EvenOdd;
        hole.commands.extend([
            PathCommand::MoveTo { x: 70., y: 20. },
            PathCommand::LineTo { x: 90., y: 20. },
            PathCommand::LineTo { x: 90., y: 80. },
            PathCommand::LineTo { x: 70., y: 80. },
            PathCommand::Close,
        ]);
        let mut clips = vec![
            RuntimeClip {
                id: "half".into(),
                components: vec![geometry(&hole)],
                alpha_components: Some(vec![hole]),
            },
            RuntimeClip {
                id: "source".into(),
                components: vec![],
                alpha_components: Some(vec![
                    component(0., 1., vec!["half".into()]),
                    component(20., 1., vec!["half".into()]),
                ]),
            },
        ];
        factor_alpha_clip_groups(&mut clips);
        let coverage = MaskCoverage::new(&Project::blank(), &clips).unwrap();
        for (point, expected) in [
            ([10., 50.], 0.5),
            ([40., 50.], 0.5),
            ([80., 50.], 0.),
            ([110., 50.], 0.),
        ] {
            assert_eq!(coverage.alpha(&["source".into()], point), expected);
        }
        let json = serde_json::to_value(&clips).unwrap();
        factor_alpha_clip_groups(&mut clips);
        assert_eq!(
            serde_json::to_value(&clips).unwrap(),
            json,
            "normalization is stable"
        );
    }
}
