//! Runtime-only, shared clipping geometry. A clip unions its components; a
//! node's clip IDs intersect those unions. No renderer owns clipping state.
use super::*;

const MAX_CLIP_ENTRIES: usize = 256;
const MAX_AUTHORED_MASK_COMMANDS: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl ScrollRect {
    fn validate(self) -> Result<(), DisplayError> {
        if [self.x, self.y, self.width, self.height]
            .into_iter()
            .any(|v| !v.is_finite() || v.abs() > 1e9 || v.fract() != 0.)
            || self.width < 0.
            || self.height < 0.
        {
            return Err(error(
                "scrollRect requires integer coordinates within +/-1e9 and nonnegative dimensions; fractional/render-delayed scrolling is not supported",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipComponent {
    pub commands: Vec<PathCommand>,
    pub fill_rule: FillRule,
    pub matrix: [f64; 6],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeClip {
    pub id: String,
    pub components: Vec<ClipComponent>,
}

impl Player {
    pub fn object_mask(&self, target: &ObjectRef) -> Result<Option<ObjectRef>, DisplayError> {
        self.graph.require_clip_object(target)?;
        Ok(self.graph.masks.get(target).cloned())
    }
    pub fn object_set_mask(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        source: Option<ObjectRef>,
    ) -> Result<(), DisplayError> {
        self.graph.require_clip_object(target)?;
        let mut masks = self.graph.masks.clone();
        masks.remove(target);
        if let Some(source) = source {
            self.graph.require_clip_object(&source)?;
            // A source belongs to one target. Reassignment is committed only
            // after the complete candidate graph passes validation.
            masks.retain(|_, current| current != &source);
            masks.insert(target.clone(), source);
        }
        self.graph.validate_clipping(
            project,
            &self.runtime_snapshot(project),
            &masks,
            &self.graph.scroll_rects,
            None,
        )?;
        self.graph.masks = masks;
        Ok(())
    }
    pub fn object_scroll_rect(
        &self,
        target: &ObjectRef,
    ) -> Result<Option<ScrollRect>, DisplayError> {
        self.graph.require_clip_object(target)?;
        Ok(self.graph.scroll_rects.get(target).copied())
    }
    pub fn object_set_scroll_rect(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        rect: Option<ScrollRect>,
    ) -> Result<(), DisplayError> {
        self.graph.require_clip_object(target)?;
        let mut rectangles = self.graph.scroll_rects.clone();
        if let Some(rect) = rect {
            rect.validate()?;
            rectangles.insert(target.clone(), rect);
        } else {
            rectangles.remove(target);
        }
        self.graph.validate_clipping(
            project,
            &self.runtime_snapshot(project),
            &self.graph.masks,
            &rectangles,
            None,
        )?;
        self.graph.scroll_rects = rectangles;
        Ok(())
    }
}

impl RuntimeGraph {
    fn require_clip_object(&self, target: &ObjectRef) -> Result<(), DisplayError> {
        if self.alive(target) {
            Ok(())
        } else {
            Err(error(
                "Clipping reference is stale or belongs to another player",
            ))
        }
    }
    pub(super) fn retire_clipping(&mut self, retired: &BTreeSet<ObjectRef>) {
        self.masks
            .retain(|target, source| !retired.contains(target) && !retired.contains(source));
        self.scroll_rects
            .retain(|target, _| !retired.contains(target));
    }
    /// Validate both assignments and hypothetical reparenting before changing
    /// any graph state. Dynamic source geometry already shares the global
    /// 100k Graphics budget; authored mask geometry receives a separate cap.
    pub(super) fn validate_clipping(
        &self,
        project: &Project,
        snapshot: &RuntimeSnapshot,
        masks: &BTreeMap<ObjectRef, ObjectRef>,
        rectangles: &BTreeMap<ObjectRef, ScrollRect>,
        reparent: Option<(&ObjectRef, &ObjectRef)>,
    ) -> Result<(), DisplayError> {
        if masks.len() + rectangles.len() > MAX_CLIP_ENTRIES {
            return Err(error(
                "Runtime clipping exceeds 256 active mask/scrollRect entries",
            ));
        }
        if masks.is_empty() && rectangles.is_empty() {
            return Ok(());
        }
        let sources: BTreeSet<_> = masks.values().collect();
        let parent_of = |id: &ObjectRef| {
            if let Some((child, parent)) = reparent
                && child == id
            {
                Some(parent.clone())
            } else {
                self.parent(id)
            }
        };
        let objects: HashMap<_, _> = snapshot
            .objects
            .iter()
            .map(|n| (&n.object_ref, n))
            .collect();
        let mut masked = BTreeSet::new();
        for node in &snapshot.objects {
            let mut current = Some(node.object_ref.clone());
            let mut source_count = 0;
            let mut has_target = false;
            let mut clip_count = 0;
            let mut steps = 0;
            while let Some(id) = current {
                source_count += usize::from(sources.contains(&id));
                has_target |= masks.contains_key(&id);
                clip_count += usize::from(masks.contains_key(&id))
                    + usize::from(rectangles.contains_key(&id));
                if clip_count > 32 {
                    return Err(error(
                        "Inherited clipping exceeds 32 mask/scrollRect entries",
                    ));
                }
                current = parent_of(&id);
                steps += 1;
                if steps > MAX_GRAPH_DEPTH {
                    return Err(error("Clipping graph contains an invalid ancestry cycle"));
                }
            }
            if source_count == 0 {
                continue;
            }
            if source_count > 1 || has_target {
                return Err(error(
                    "Mask source subtrees cannot overlap, contain their target, or themselves be masked",
                ));
            }
            if rectangles.contains_key(&node.object_ref) {
                return Err(error("scrollRect inside a mask source is not supported"));
            }
            if node.display.total_frames > 1
                || matches!(node.display.owner.as_str(), "text" | "bitmap" | "video")
            {
                return Err(error(
                    "Mask sources currently support vector fills and one-frame vector containers; text, bitmap, video and animated authored timelines are pending",
                ));
            }
            if let Some(kind) = &node.display.kind {
                vector_cost(kind)?;
            }
            masked.insert(&node.object_ref);
        }
        // Reserve each outer authored branch once. Its static symbol closure
        // includes all descendants (also hidden layers), without double-counting
        // nested instance geometry or unrelated placements of the same symbol.
        let mut authored_counts = BTreeMap::<&str, usize>::new();
        for reference in &masked {
            let node = objects[reference];
            let Some(id) = node.source_path.as_ref().and_then(|p| p.last()) else {
                continue;
            };
            let authored_parent = parent_of(reference).as_ref().is_some_and(|parent| {
                masked.contains(parent)
                    && objects
                        .get(parent)
                        .is_some_and(|n| n.source_path.as_ref().is_some_and(|p| !p.is_empty()))
            });
            if !authored_parent {
                *authored_counts.entry(id).or_default() += 1;
            }
        }
        // A continuing authored ID may change in a later outer keyframe.
        // Validate every occurrence and reserve its largest possible closure.
        let mut future_cost = BTreeMap::<&str, usize>::new();
        let mut symbol_costs = BTreeMap::new();
        for element in project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
        {
            if authored_counts.contains_key(element.id.as_str()) {
                let cost = static_kind_cost(project, &element.kind, 0, &mut symbol_costs)?;
                let entry = future_cost.entry(&element.id).or_default();
                *entry = (*entry).max(cost);
            }
        }
        let authored_cost = authored_counts.iter().fold(0usize, |sum, (id, count)| {
            sum.saturating_add(
                future_cost
                    .get(id)
                    .copied()
                    .unwrap_or_default()
                    .saturating_mul(*count),
            )
        });
        if authored_cost > MAX_AUTHORED_MASK_COMMANDS {
            return Err(error(
                "Authored mask geometry exceeds the shared 100000-command clipping budget",
            ));
        }
        Ok(())
    }

    pub(super) fn apply_clipping(&self, snapshot: &mut RuntimeSnapshot) {
        if self.masks.is_empty() && self.scroll_rects.is_empty() {
            return;
        }
        let objects: HashMap<_, _> = snapshot
            .objects
            .iter()
            .map(|n| (&n.object_ref, n))
            .collect();
        // Exact singular local transforms stay empty even if floating-point
        // world multiplication would leave a spurious nonzero determinant.
        let mut singular = BTreeSet::new();
        for node in &snapshot.objects {
            if !nonsingular(node.local_matrix)
                || node
                    .parent_ref
                    .as_ref()
                    .is_some_and(|parent| singular.contains(parent))
            {
                singular.insert(node.object_ref.clone());
            }
        }
        let mut own_clips = BTreeMap::<ObjectRef, Vec<String>>::new();
        let mut consumed = BTreeSet::new();
        for (target, rect) in &self.scroll_rects {
            let Some(node) = objects.get(target) else {
                continue;
            };
            let id = format!("scroll:{}:{}", target.generation, target.id);
            let matrix = affine_multiply(node.display.world, [1., 0., 0., 1., rect.x, rect.y]);
            let components = if rect.width > 0.
                && rect.height > 0.
                && !singular.contains(target)
                && nonsingular(matrix)
            {
                vec![ClipComponent {
                    commands: rectangle(rect.width, rect.height),
                    fill_rule: FillRule::NonZero,
                    matrix,
                }]
            } else {
                vec![]
            };
            snapshot.clips.push(RuntimeClip {
                id: id.clone(),
                components,
            });
            own_clips.entry(target.clone()).or_default().push(id);
        }
        for (target, source) in &self.masks {
            let id = format!("mask:{}:{}", source.generation, source.id);
            let mut components = vec![];
            for reference in self.subtree(source) {
                consumed.insert(reference.clone());
                let Some(node) = objects.get(&reference) else {
                    continue;
                };
                let matrix = node.display.world;
                if singular.contains(&reference) || !nonsingular(matrix) {
                    continue;
                }
                // Alpha and visible are deliberately ignored for uncached
                // vector masks. Actual fill presence, not RGBA, selects paths.
                if let Some(dynamic) = self.dynamic.get(&reference) {
                    components.extend(dynamic.graphics.mask_paths().map(
                        |(commands, fill_rule)| ClipComponent {
                            commands: commands.to_vec(),
                            fill_rule,
                            matrix,
                        },
                    ));
                }
                if let Some(kind) = &node.display.kind
                    && let Some((commands, fill_rule)) = vector_path(kind)
                {
                    components.push(ClipComponent {
                        commands,
                        fill_rule,
                        matrix,
                    });
                }
            }
            snapshot.clips.push(RuntimeClip {
                id: id.clone(),
                components,
            });
            own_clips.entry(target.clone()).or_default().push(id);
        }
        let empty: BTreeSet<_> = snapshot
            .clips
            .iter()
            .filter(|clip| clip.components.is_empty())
            .map(|clip| clip.id.as_str())
            .collect();
        let mut inherited = HashMap::<ObjectRef, Vec<String>>::new();
        for node in &mut snapshot.objects {
            let mut ids = node
                .parent_ref
                .as_ref()
                .and_then(|p| inherited.get(p))
                .cloned()
                .unwrap_or_default();
            ids.extend(own_clips.remove(&node.object_ref).unwrap_or_default());
            if consumed.contains(&node.object_ref)
                || ids.iter().any(|id| empty.contains(id.as_str()))
            {
                node.display.effective_visible = false;
            }
            node.clip_ids = ids.clone();
            inherited.insert(node.object_ref.clone(), ids);
        }
        snapshot.draws.retain_mut(|draw| {
            draw.clip_ids = inherited.get(&draw.owner_ref).cloned().unwrap_or_default();
            !consumed.contains(&draw.owner_ref)
                && !draw.clip_ids.iter().any(|id| empty.contains(id.as_str()))
        });
        snapshot.semantics.retain_mut(|node| {
            node.clip_ids = inherited.get(&node.owner_ref).cloned().unwrap_or_default();
            !consumed.contains(&node.owner_ref)
                && !node.clip_ids.iter().any(|id| empty.contains(id.as_str()))
        });
    }
}

fn nonsingular(m: [f64; 6]) -> bool {
    let determinant = m[0] * m[3] - m[1] * m[2];
    determinant.is_finite() && determinant != 0.
}
fn vector_cost(kind: &ElementKind) -> Result<usize, DisplayError> {
    match kind {
        ElementKind::Rectangle { .. } => Ok(5),
        ElementKind::Ellipse { .. } => Ok(6),
        ElementKind::Path { commands, .. } => Ok(commands.len()),
        ElementKind::Instance { .. } => Ok(0),
        _ => Err(error(
            "Text/bitmap/video mask sources require an unsupported raster/alpha-mask profile",
        )),
    }
}
fn static_kind_cost(
    project: &Project,
    kind: &ElementKind,
    depth: usize,
    costs: &mut BTreeMap<Id, usize>,
) -> Result<usize, DisplayError> {
    if depth > MAX_GRAPH_DEPTH {
        return Err(error("Mask source symbol nesting exceeds 64"));
    }
    if let ElementKind::Instance { symbol_id, .. } = kind {
        if let Some(cost) = costs.get(symbol_id) {
            return Ok(*cost);
        }
        let symbol = project
            .symbol(symbol_id)
            .ok_or_else(|| error("Mask source symbol is missing"))?;
        if symbol.timeline.frame_count != 1 {
            return Err(error(
                "Animated authored mask timelines are not supported yet",
            ));
        }
        let mut cost = 0usize;
        for element in symbol
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
        {
            cost = cost.saturating_add(static_kind_cost(project, &element.kind, depth + 1, costs)?);
            if cost > MAX_AUTHORED_MASK_COMMANDS {
                return Err(error("Authored mask geometry exceeds 100000 commands"));
            }
        }
        costs.insert(symbol_id.clone(), cost);
        Ok(cost)
    } else {
        vector_cost(kind)
    }
}
fn rectangle(width: f64, height: f64) -> Vec<PathCommand> {
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
pub(crate) fn vector_path(kind: &ElementKind) -> Option<(Vec<PathCommand>, FillRule)> {
    let commands = match kind {
        ElementKind::Path {
            commands,
            fill_rule,
            ..
        } => return Some((commands.clone(), *fill_rule)),
        ElementKind::Rectangle { width, height, .. } if *width > 0. && *height > 0. => {
            rectangle(*width, *height)
        }
        ElementKind::Ellipse { width, height, .. } if *width > 0. && *height > 0. => {
            let (x, y) = (width / 2., height / 2.);
            let k = 0.552_284_749_830_793_6;
            vec![
                PathCommand::MoveTo { x: *width, y },
                PathCommand::CubicTo {
                    cx1: *width,
                    cy1: y + k * y,
                    cx2: x + k * x,
                    cy2: *height,
                    x,
                    y: *height,
                },
                PathCommand::CubicTo {
                    cx1: x - k * x,
                    cy1: *height,
                    cx2: 0.,
                    cy2: y + k * y,
                    x: 0.,
                    y,
                },
                PathCommand::CubicTo {
                    cx1: 0.,
                    cy1: y - k * y,
                    cx2: x - k * x,
                    cy2: 0.,
                    x,
                    y: 0.,
                },
                PathCommand::CubicTo {
                    cx1: x + k * x,
                    cy1: 0.,
                    cx2: *width,
                    cy2: y - k * y,
                    x: *width,
                    y,
                },
                PathCommand::Close,
            ]
        }
        _ => return None,
    };
    Some((commands, FillRule::NonZero))
}
