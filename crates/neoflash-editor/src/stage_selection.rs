use crate::theme::UiPalette;
use crate::{
    app::{Editor, Tool},
    selection::{Selection, SelectionTarget},
    transform_geometry as geometry,
};
use eframe::egui::{self, Pos2, Rect, Stroke, StrokeKind};
use neoflash_core::*;
use neoflash_ui::components::canvas as cv;
use neoflash_ui::cursors::{self as nf, Cursor};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Scope {
    symbol: Id,
    frame: u32,
    origin: Pos2,
    zoom: f32,
}
impl Scope {
    fn new(editor: &Editor, origin: Pos2) -> Self {
        Self {
            symbol: editor.symbol_id().to_owned(),
            frame: editor.frame,
            origin,
            zoom: editor.zoom,
        }
    }
    fn matches(&self, editor: &Editor) -> bool {
        self.symbol == editor.symbol_id() && self.frame == editor.frame
    }
    fn local(&self, point: Pos2) -> geometry::Point {
        [
            (point.x - self.origin.x) as f64 / self.zoom as f64,
            (point.y - self.origin.y) as f64 / self.zoom as f64,
        ]
    }
}

#[derive(Clone, Copy)]
enum GestureKind {
    Move,
    Pivot {
        initial_world: geometry::Point,
    },
    Resize {
        bounds: geometry::Bounds,
        handle: geometry::ResizeHandle,
    },
    Scale {
        pivot: geometry::Point,
        center: geometry::Point,
    },
    Rotate {
        single: bool,
        pivot: geometry::Point,
        local_bounds: Option<geometry::Bounds>,
    },
    /// Free Transform edge drag: shear along the edge (`horizontal` = top/bottom edge, x shear)
    /// about the opposite edge, or about the transformation point with ⌥. Stage coordinates on the shear's axis.
    Skew {
        horizontal: bool,
        edge: f64,
        opposite: f64,
        center: f64,
    },
    Marquee,
}

/// Free Transform hit zone on the screen (Flash layout): pivot circle, handles scale, the edge
/// between handles skews, just outside a corner rotates.
#[derive(Clone, Copy)]
enum Zone {
    Pivot(Pos2),
    Handle(Rect),
    Edge(Pos2, Pos2),
    Rotate { corner: Pos2, frame: Rect },
}
impl Zone {
    fn contains(self, pos: Pos2) -> bool {
        match self {
            Zone::Pivot(at) => at.distance(pos) <= 7.,
            Zone::Handle(rect) => rect.expand(3.).contains(pos),
            Zone::Edge(a, b) => {
                let ab = b - a;
                let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0., 1.);
                (a + ab * t).distance(pos) <= 4.
            }
            Zone::Rotate { corner, frame } => {
                !frame.expand(2.).contains(pos) && corner.distance(pos) <= 18.
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct SelectionGesture {
    scope: Scope,
    kind: GestureKind,
    press: Pos2,
    latest: Pos2,
    original_selection: Selection,
    targets: Vec<(SelectionTarget, Transform)>,
    /// `(layer_id, keyframe_frame, element_id, original_transform)` for other keyframes in
    /// `onion_range()` when `Edit multiple frames` is active.
    other_keyframes: Vec<(Id, u32, Id, Transform)>,
    hit: Option<SelectionTarget>,
    shift: bool,
    duplicate: bool,
    duplicated: bool,
    moved: bool,
    began: bool,
    changed: bool,
    angle: f64,
    /// Selection bounds (stage) when the move started: the edges that snap (ST-04).
    snap_bounds: Option<Rect>,
    /// Transient group pivot snapshot when a multi-selection gesture started.
    initial_group_pivot: Option<geometry::Point>,
    /// NF2 cursor shown for the whole gesture.
    cursor: Cursor,
}

pub(crate) struct ShapeDraft {
    scope: Scope,
    layer: Id,
    tool: Tool,
    start: Pos2,
}

struct SelectionGeometry {
    local: Option<geometry::Bounds>,
    initial: Option<Transform>,
    world: Rect,
    world_pivot: geometry::Point,
    resizable: bool,
    /// Stage bounds, NF2 selection-box kind, and whether the placement is an ungrouped Merge shape.
    boxes: Vec<(Id, Rect, cv::SelectionKind, bool)>,
}

/// `NF2/Selection Box` kind for a placement: symbols show their registration and transformation
/// points, text shows its sizing handle (point text = auto width, text box = fixed size), and
/// parametric primitives show primitive control corners.
fn selection_kind(element: &Element) -> cv::SelectionKind {
    if element.has_parametric_options() {
        return cv::SelectionKind::Primitive;
    }
    match element.kind {
        ElementKind::Instance { .. } => cv::SelectionKind::Symbol,
        ElementKind::Text { .. } => cv::SelectionKind::TextStatic,
        ElementKind::TextBox { .. } => cv::SelectionKind::TextDynamic,
        _ => cv::SelectionKind::Group,
    }
}

fn bounds(rect: Rect) -> geometry::Bounds {
    geometry::Bounds {
        min: [rect.min.x as f64, rect.min.y as f64],
        max: [rect.max.x as f64, rect.max.y as f64],
    }
}
fn position(point: geometry::Point) -> Pos2 {
    Pos2::new(point[0] as f32, point[1] as f32)
}
fn inverse_matrix(m: [f64; 6]) -> Option<[f64; 6]> {
    let d = m[0] * m[3] - m[1] * m[2];
    if !d.is_finite() || d.abs() < 1e-12 {
        return None;
    }
    Some([
        m[3] / d,
        -m[1] / d,
        -m[2] / d,
        m[0] / d,
        (m[2] * m[5] - m[3] * m[4]) / d,
        (m[1] * m[4] - m[0] * m[5]) / d,
    ])
}
fn transformed_rect(rect: Rect, m: [f64; 6]) -> Rect {
    if !rect.is_finite() {
        return Rect::NOTHING;
    }
    let mut result = Rect::NOTHING;
    for p in [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ] {
        result.extend_with(crate::stage::affine(m, p));
    }
    result
}

/// Groups are independent drawing objects even when their members retain raw Merge geometry.
/// Temporarily exclude those members from planar edits, preserving their flags and stack order.
pub(crate) fn edit_ungrouped_merge_shapes<R>(
    layer: &mut Layer,
    frame: u32,
    edit: impl FnOnce(&mut Vec<Element>) -> R,
) -> Option<R> {
    let grouped: HashSet<_> = layer
        .properties
        .groups
        .iter()
        .flat_map(|group| group.members.iter().cloned())
        .collect();
    let elements = &mut layer.key_at_mut(frame)?.elements;
    let protected: HashSet<_> = elements
        .iter()
        .filter(|element| element.merge && grouped.contains(&element.id))
        .map(|element| element.id.clone())
        .collect();
    for element in elements.iter_mut().filter(|e| protected.contains(&e.id)) {
        element.merge = false;
    }
    let result = edit(elements);
    for element in elements.iter_mut().filter(|e| protected.contains(&e.id)) {
        element.merge = true;
    }
    Some(result)
}

impl Editor {
    pub(crate) fn selection_bounds(
        &self,
        scene: &[DrawItem],
        painter: &egui::Painter,
    ) -> Option<Rect> {
        self.selection_geometry(scene, painter)
            .map(|geometry| geometry.world)
    }
    pub(crate) fn finish_selection_gesture(&mut self) {
        if let Some(gesture) = self.object_gesture.take()
            && gesture.began
        {
            if gesture.changed {
                if !matches!(gesture.kind, GestureKind::Pivot { .. }) {
                    self.remerge_selected_after_move(&gesture);
                }
                self.commit_history();
            } else {
                self.history.cancel(&mut self.project);
            }
        }
    }

    fn remerge_selected_after_move(&mut self, gesture: &SelectionGesture) {
        let frame = self.frame;
        let mut layer_order = Vec::new();
        let mut moved_by_layer: HashMap<Id, Vec<Id>> = HashMap::new();
        for (target, _) in &gesture.targets {
            if !moved_by_layer.contains_key(&target.layer) {
                layer_order.push(target.layer.clone());
            }
            moved_by_layer
                .entry(target.layer.clone())
                .or_default()
                .push(target.element.clone());
        }
        let mut new_selection = Vec::new();
        let mut any_remerged = false;
        for layer_id in layer_order {
            let Some(moved_ids) = moved_by_layer.get(&layer_id) else {
                continue;
            };
            let Some(layer_idx) = self
                .symbol()
                .timeline
                .layers
                .iter()
                .position(|l| l.id == layer_id)
            else {
                continue;
            };
            let grouped: HashSet<Id> = self.symbol().timeline.layers[layer_idx]
                .properties
                .groups
                .iter()
                .filter(|g| g.members.len() >= 2)
                .flat_map(|g| g.members.iter().cloned())
                .collect();
            let merge_eligible: HashSet<&Id> = self.symbol().timeline.layers[layer_idx]
                .key_at(frame)
                .map(|k| {
                    k.elements
                        .iter()
                        .filter(|e| e.merge && !grouped.contains(&e.id))
                        .map(|e| &e.id)
                        .collect()
                })
                .unwrap_or_default();
            let merge_moved: Vec<Id> = moved_ids
                .iter()
                .filter(|id| merge_eligible.contains(id))
                .cloned()
                .collect();
            if merge_moved.is_empty() {
                for id in moved_ids {
                    new_selection.push(SelectionTarget {
                        layer: layer_id.clone(),
                        element: id.clone(),
                    });
                }
                continue;
            }
            any_remerged = true;
            let merge_moved_set: HashSet<&Id> = merge_moved.iter().collect();
            if let Some(remerged_ids) = edit_ungrouped_merge_shapes(
                &mut self.symbol_mut().timeline.layers[layer_idx],
                frame,
                |elements| remerge_moved_elements_in_keyframe(elements, &merge_moved),
            ) {
                for id in moved_ids {
                    if !merge_moved_set.contains(id) {
                        new_selection.push(SelectionTarget {
                            layer: layer_id.clone(),
                            element: id.clone(),
                        });
                    }
                }
                for id in remerged_ids {
                    new_selection.push(SelectionTarget {
                        layer: layer_id.clone(),
                        element: id,
                    });
                }
            }
        }
        if any_remerged {
            self.set_selection(new_selection);
        }
    }
    /// Ensure a keyframe on `layer_id` at `self.frame` for a stage drag/transform (respecting
    /// `auto_keyframe`), and when a new keyframe is created on an untweened in-between frame,
    /// apply `timeline_prefs.tween_on_drag` to the span starting at the previous keyframe.
    fn split_layer_for_stage_transform(&mut self, layer_id: &str) {
        use crate::workspace::TweenOnDrag;
        let frame = self.frame;
        let tween_pref = self.workspace.timeline_prefs.tween_on_drag;
        let prev_untweened = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == layer_id)
            .and_then(|layer| {
                if layer.keyframes.iter().any(|k| k.frame == frame)
                    || motion_span(layer, frame).is_some()
                    || layer.properties.end_frame.is_some_and(|end| frame >= end)
                {
                    return None;
                }
                let prev = layer.key_at(frame)?;
                (prev.frame < frame && prev.tween.is_none()).then(|| {
                    // Every element has a stable ID and an independent transform track. The
                    // motion evaluator therefore supports several objects in one layer; only an
                    // empty keyframe is unsuitable for automatic Motion tween creation.
                    let motion_ok = !prev.elements.is_empty();
                    (prev.frame, motion_ok)
                })
            });

        if tween_pref == TweenOnDrag::Motion
            && self.workspace.timeline_prefs.auto_keyframe
            && let Some((prev_frame, motion_ok)) = prev_untweened
        {
            if motion_ok {
                if let Some(prev) = self
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == prev_frame))
                {
                    prev.tween = Some(Easing::Linear);
                    prev.tween_kind = TweenKind::Motion;
                }
                self.ensure_key_for(layer_id, false);
                self.project.upgrade_format_for_features();
                return;
            }
            self.message("A motion tween needs at least one object on its keyframe.");
        }

        self.ensure_key_for(layer_id, false);

        if tween_pref == TweenOnDrag::Classic
            && let Some((prev_frame, _)) = prev_untweened
        {
            let ease = self.workspace.default_ease;
            let created = self
                .symbol()
                .timeline
                .layers
                .iter()
                .find(|l| l.id == layer_id)
                .is_some_and(|l| l.keyframes.iter().any(|k| k.frame == frame));
            if created {
                if let Some(prev) = self
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                    .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == prev_frame))
                {
                    prev.tween = Some(ease);
                    prev.tween_kind = TweenKind::Classic;
                    prev.property_eases.clear();
                    prev.sync_symbols = false;
                    prev.snap_to_guide = false;
                }
                if matches!(ease, Easing::Flash { .. }) {
                    self.project.format_version = self.project.format_version.max(5);
                }
                self.project.upgrade_format_for_features();
            }
        }
    }
    pub(crate) fn cancel_selection_gesture(&mut self) {
        if let Some(gesture) = self.object_gesture.take() {
            if gesture.began {
                self.history.cancel(&mut self.project);
            }
            self.selection = gesture.original_selection;
        }
    }
    pub(crate) fn cancel_stale_selection_gesture(&mut self) {
        let Some(gesture) = &self.object_gesture else {
            return;
        };
        let scope_changed =
            !gesture.scope.matches(self) || !matches!(self.tool, Tool::Select | Tool::Transform);
        let stale = scope_changed
            || (!gesture.targets.is_empty() && {
                let allowed: HashSet<_> = self.editable_targets().into_iter().collect();
                gesture
                    .targets
                    .iter()
                    .any(|(target, _)| !allowed.contains(target))
            });
        if stale {
            self.cancel_selection_gesture();
        }
    }
    fn selection_geometry(
        &self,
        scene: &[DrawItem],
        p: &egui::Painter,
    ) -> Option<SelectionGeometry> {
        if self.selection.is_empty() {
            return None;
        }
        let single = self.selected_sampled();
        let initial = single.as_ref().map(|element| element.transform);
        let inverse = initial.and_then(|t| inverse_matrix(transform_matrix(t)));
        let selected: HashSet<_> = self
            .selection
            .targets
            .iter()
            .map(|target| &target.element)
            .collect();
        let mut world = Rect::NOTHING;
        let mut local = Rect::NOTHING;
        let mut per_target: Vec<(&Id, Rect)> = Vec::with_capacity(selected.len());
        let mut per_target_idx: HashMap<&Id, usize> = HashMap::with_capacity(selected.len());
        for item in scene {
            let Some(id) = item.instance_path.first() else {
                continue;
            };
            if !selected.contains(id) {
                continue;
            };
            let rect = crate::stage::local_bounds(item, p);
            let item_world = transformed_rect(rect, item.transform);
            world = world.union(item_world);
            if let Some(&idx) = per_target_idx.get(id) {
                per_target[idx].1 = per_target[idx].1.union(item_world);
            } else {
                per_target_idx.insert(id, per_target.len());
                per_target.push((id, item_world));
            }
            if let Some(matrix) = inverse {
                local = local.union(transformed_rect(
                    rect,
                    affine_multiply(matrix, item.transform),
                ));
            }
        }
        if !world.is_finite() {
            return None;
        }
        let local = local.is_finite().then(|| bounds(local));
        let resizable = if let (Some(local), Some(initial)) = (local, initial) {
            let point = geometry::transformed_point(initial, local.max);
            geometry::resize_single(
                initial,
                local,
                geometry::ResizeHandle::SouthEast,
                point,
                point,
                geometry::ResizeOptions::default(),
            )
            .is_some()
        } else {
            self.selection.len() > 1 && world.width() > 1e-6 && world.height() > 1e-6
        };
        let grouped_ids: HashSet<Id> = self
            .symbol()
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.properties.groups)
            .filter(|g| g.members.len() >= 2)
            .flat_map(|g| g.members.iter().cloned())
            .collect();
        let multiple = self.timeline_edit.edit_multiple;
        let (start_f, end_f) = if multiple {
            self.onion_range()
        } else {
            (self.frame, self.frame)
        };
        let mut kinds: HashMap<Id, (cv::SelectionKind, bool)> =
            HashMap::with_capacity(selected.len());
        for layer in &self.symbol().timeline.layers {
            if let Some(key) = layer.key_at(self.frame) {
                for element in &key.elements {
                    if selected.contains(&element.id) {
                        let is_merge = element.merge && !grouped_ids.contains(&element.id);
                        kinds.insert(element.id.clone(), (selection_kind(element), is_merge));
                    }
                }
            }
            if multiple {
                for key in layer
                    .keyframes
                    .iter()
                    .filter(|k| (start_f..=end_f).contains(&k.frame))
                {
                    for element in &key.elements {
                        if selected.contains(&element.id) && !kinds.contains_key(&element.id) {
                            let is_merge = element.merge && !grouped_ids.contains(&element.id);
                            kinds.insert(element.id.clone(), (selection_kind(element), is_merge));
                        }
                    }
                }
            }
        }
        let boxes = per_target
            .into_iter()
            .filter(|(_, rect)| rect.is_finite())
            .map(|(id, rect)| {
                let (kind, is_merge) = kinds
                    .get(id)
                    .copied()
                    .unwrap_or((cv::SelectionKind::Group, false));
                (id.clone(), rect, kind, is_merge)
            })
            .collect();
        let world_pivot = if let (Some(local), Some(initial)) = (local, initial) {
            let local_pivot = initial.pivot.unwrap_or_else(|| local.center());
            geometry::transformed_point(initial, local_pivot)
        } else if let Some((targets, gp)) = &self.authoring_modify.group_pivot
            && targets == &self.selection.targets
        {
            *gp
        } else {
            bounds(world).center()
        };
        Some(SelectionGeometry {
            local,
            initial,
            world,
            world_pivot,
            resizable,
            boxes,
        })
    }
    pub(crate) fn pick_placement(
        &self,
        scene: &[DrawItem],
        p: &egui::Painter,
        point: Pos2,
    ) -> Option<SelectionTarget> {
        let eligible: HashMap<_, _> = self
            .editable_targets()
            .into_iter()
            .map(|target| (target.element.clone(), target))
            .collect();
        let authored = self.presented_interactive_authored_scene(self.show_layer_masking(p.ctx()));
        let pick_with_slop = |slop: f32| {
            scene.iter().rev().find_map(|item| {
                let target = eligible.get(item.instance_path.first()?)?;
                (authored.contains_point(&item.instance_path, [point.x as f64, point.y as f64])
                    && crate::stage::hit_item_with_slop(item, p, point, self.zoom, slop))
                .then(|| target.clone())
            })
        };
        if let Some(exact) = pick_with_slop(0.0) {
            return Some(exact);
        }
        let slop = self.workspace.drawing.click_accuracy.click_slop_px();
        (slop > 0.0).then(|| pick_with_slop(slop)).flatten()
    }

    fn update_selection_gesture(
        &mut self,
        pos: Pos2,
        modifiers: egui::Modifiers,
        scene: &[DrawItem],
        p: &egui::Painter,
    ) {
        self.cancel_stale_selection_gesture();
        let Some(mut gesture) = self.object_gesture.take() else {
            return;
        };
        let current = gesture.scope.local(pos);
        if let GestureKind::Rotate { pivot, .. } = gesture.kind
            && let Some(delta) =
                geometry::rotation_delta(pivot, gesture.scope.local(gesture.latest), current, false)
        {
            gesture.angle += delta;
        }
        gesture.latest = pos;
        gesture.moved |= pos.distance(gesture.press) >= 3.;
        if !gesture.moved {
            self.object_gesture = Some(gesture);
            return;
        }
        p.ctx()
            .data_mut(|d| d.remove::<(f64, Pos2)>(egui::Id::new("nf2-pivot-click")));
        if matches!(gesture.kind, GestureKind::Marquee) {
            let marquee = Rect::from_two_pos(
                position(gesture.scope.local(gesture.press)),
                position(current),
            );
            let mut targets = if gesture.shift {
                gesture.original_selection.targets.clone()
            } else {
                vec![]
            };
            let eligible: HashMap<_, _> = self
                .editable_targets()
                .into_iter()
                .map(|target| (target.element.clone(), target))
                .collect();
            let mut indices = HashMap::<Id, usize>::new();
            let mut candidates: Vec<(SelectionTarget, Rect)> = Vec::new();
            for item in scene {
                let Some(id) = item.instance_path.first() else {
                    continue;
                };
                let Some(target) = eligible.get(id) else {
                    continue;
                };
                let rect = crate::stage::world_bounds(item, p);
                if let Some(&index) = indices.get(id) {
                    candidates[index].1 = candidates[index].1.union(rect);
                } else {
                    indices.insert(id.clone(), candidates.len());
                    candidates.push((target.clone(), rect));
                }
            }
            targets.extend(
                candidates
                    .into_iter()
                    .filter(|(_, rect)| rect.is_finite() && marquee.contains_rect(*rect))
                    .map(|(target, _)| target),
            );
            self.set_selection(targets);
            self.object_gesture = Some(gesture);
            return;
        }
        let originals: Vec<_> = gesture.targets.iter().map(|(_, t)| *t).collect();
        let other_originals: Vec<_> = gesture
            .other_keyframes
            .iter()
            .map(|(_, _, _, t)| *t)
            .collect();
        let press = gesture.scope.local(gesture.press);
        // Snapping (guides, grid): moves snap the selection's edges and centre; handles snap the
        // dragged handle point; scaling snaps the pointer.
        let origin = gesture.scope.origin;
        let current = match gesture.kind {
            GestureKind::Pivot { initial_world } => gesture.scope.local(self.snap_drag(
                origin,
                gesture.press,
                pos,
                position(initial_world),
            )),
            GestureKind::Resize { bounds, handle } => {
                let anchor = position(geometry::transformed_point(
                    originals[0],
                    handle.point(bounds),
                ));
                gesture
                    .scope
                    .local(self.snap_drag(origin, gesture.press, pos, anchor))
            }
            GestureKind::Scale { .. } => gesture.scope.local(self.snap_screen(origin, pos)),
            _ => current,
        };
        let (transforms, other_transforms) = match gesture.kind {
            GestureKind::Move => {
                let mut delta = [current[0] - press[0], current[1] - press[1]];
                let mut lock = [false, false];
                if modifiers.shift {
                    if delta[0].abs() >= delta[1].abs() {
                        delta[1] = 0.;
                        lock[1] = true;
                    } else {
                        delta[0] = 0.;
                        lock[0] = true;
                    }
                }
                let bounds = *gesture.snap_bounds.get_or_insert_with(|| {
                    self.selection_geometry(scene, p)
                        .map_or(Rect::NOTHING, |g| g.world)
                });
                delta = self.snap_move(origin, bounds, position(press), delta, lock);
                if let Some(gp) = gesture.initial_group_pivot {
                    self.authoring_modify.group_pivot = Some((
                        self.selection.targets.clone(),
                        [gp[0] + delta[0], gp[1] + delta[1]],
                    ));
                }
                let translate = |list: &[Transform]| -> Option<Vec<Transform>> {
                    let moved: Vec<_> = list
                        .iter()
                        .map(|t| Transform {
                            x: t.x + delta[0],
                            y: t.y + delta[1],
                            ..*t
                        })
                        .collect();
                    moved
                        .iter()
                        .all(|t| {
                            t.x.is_finite()
                                && t.y.is_finite()
                                && t.x.abs() <= 1e12
                                && t.y.abs() <= 1e12
                        })
                        .then_some(moved)
                };
                let main = translate(&originals);
                let others = if other_originals.is_empty() {
                    None
                } else {
                    translate(&other_originals)
                };
                (main, others)
            }
            GestureKind::Pivot { .. } => {
                let main = if originals.len() == 1 {
                    inverse_matrix(transform_matrix(originals[0])).and_then(|inv| {
                        let lx = inv[0] * current[0] + inv[2] * current[1] + inv[4];
                        let ly = inv[1] * current[0] + inv[3] * current[1] + inv[5];
                        (lx.is_finite() && ly.is_finite() && lx.abs() <= 1e9 && ly.abs() <= 1e9)
                            .then_some(vec![Transform {
                                pivot: Some([lx, ly]),
                                ..originals[0]
                            }])
                    })
                } else {
                    if current[0].is_finite() && current[1].is_finite() {
                        self.authoring_modify.group_pivot =
                            Some((self.selection.targets.clone(), current));
                    }
                    None
                };
                let others = if originals.len() == 1
                    && !other_originals.is_empty()
                    && let Some(ref main_vec) = main
                {
                    let new_pivot = main_vec[0].pivot;
                    Some(
                        other_originals
                            .iter()
                            .map(|t| Transform {
                                pivot: new_pivot,
                                ..*t
                            })
                            .collect(),
                    )
                } else {
                    None
                };
                (main, others)
            }
            GestureKind::Resize { bounds, handle } => {
                let opts = geometry::ResizeOptions {
                    from_center: modifiers.alt,
                    preserve_aspect: modifiers.shift,
                };
                let main = geometry::resize_single_with_pivot(
                    originals[0],
                    bounds,
                    handle,
                    press,
                    current,
                    opts,
                    originals[0].pivot,
                )
                .map(|t| vec![t]);
                let others = if other_originals.is_empty() {
                    None
                } else {
                    other_originals
                        .iter()
                        .map(|&orig| {
                            geometry::resize_single_with_pivot(
                                orig, bounds, handle, press, current, opts, orig.pivot,
                            )
                        })
                        .collect::<Option<Vec<_>>>()
                };
                (main, others)
            }
            GestureKind::Scale { pivot, center } => {
                let anchor = if modifiers.alt { center } else { pivot };
                let main = geometry::uniform_scale_many(&originals, anchor, press, current);
                let others = if other_originals.is_empty() {
                    None
                } else {
                    geometry::uniform_scale_many(&other_originals, anchor, press, current)
                };
                (main, others)
            }
            GestureKind::Rotate {
                single: true,
                pivot,
                local_bounds,
            } => {
                let main = geometry::rotate_single_about_by(
                    originals[0],
                    pivot,
                    gesture.angle,
                    modifiers.shift,
                )
                .map(|t| vec![t]);
                let others = if other_originals.is_empty() {
                    None
                } else {
                    other_originals
                        .iter()
                        .map(|&orig| {
                            let other_pivot = local_bounds
                                .map(|lb| {
                                    let lp = orig.pivot.unwrap_or_else(|| lb.center());
                                    geometry::transformed_point(orig, lp)
                                })
                                .unwrap_or(pivot);
                            geometry::rotate_single_about_by(
                                orig,
                                other_pivot,
                                gesture.angle,
                                modifiers.shift,
                            )
                        })
                        .collect::<Option<Vec<_>>>()
                };
                (main, others)
            }
            GestureKind::Rotate {
                single: false,
                pivot,
                ..
            } => {
                let main =
                    geometry::rotate_many_by(&originals, pivot, gesture.angle, modifiers.shift);
                let others = if other_originals.is_empty() {
                    None
                } else {
                    geometry::rotate_many_by(
                        &other_originals,
                        pivot,
                        gesture.angle,
                        modifiers.shift,
                    )
                };
                (main, others)
            }
            GestureKind::Skew {
                horizontal,
                edge,
                opposite,
                center,
            } => {
                let axis = if horizontal { 0 } else { 1 };
                let anchor = if modifiers.alt { center } else { opposite };
                let delta = current[axis] - press[axis];
                let main = geometry::skew_many(&originals, horizontal, edge, anchor, delta);
                let others = if other_originals.is_empty() {
                    None
                } else {
                    geometry::skew_many(&other_originals, horizontal, edge, anchor, delta)
                };
                (main, others)
            }
            GestureKind::Marquee => unreachable!(),
        };
        if let Some(transforms) = transforms {
            let others_ok = gesture.other_keyframes.is_empty() || other_transforms.is_some();
            if others_ok {
                let other_changed = other_transforms
                    .as_ref()
                    .is_some_and(|ot| *ot != other_originals);
                let want_duplicate = matches!(gesture.kind, GestureKind::Move)
                    && (gesture.duplicate || modifiers.alt);
                gesture.changed = gesture.duplicated || originals != transforms || other_changed;
                if gesture.changed && !gesture.began {
                    self.history.begin(&self.project);
                    if want_duplicate {
                        let sampled_map: HashMap<SelectionTarget, Element> =
                            self.sampled_targets().into_iter().collect();
                        let source_targets: Vec<(SelectionTarget, Element)> = gesture
                            .targets
                            .iter()
                            .filter_map(|(target, orig_t)| {
                                let mut el = sampled_map.get(target)?.clone();
                                el.transform = *orig_t;
                                Some((target.clone(), el))
                            })
                            .collect();
                        if !source_targets.is_empty() {
                            gesture.targets =
                                self.duplicate_targets_with_offset(source_targets, 0.0, 0.0);
                            gesture.other_keyframes.clear();
                            gesture.duplicated = true;
                            gesture.changed = true;
                            if let Some((_, gp)) = &mut self.authoring_modify.group_pivot {
                                let pos = *gp;
                                self.authoring_modify.group_pivot =
                                    Some((self.selection.targets.clone(), pos));
                            }
                        }
                    } else {
                        let multiple = self.timeline_edit.edit_multiple;
                        let (start, end) = if multiple {
                            self.onion_range()
                        } else {
                            (self.frame, self.frame)
                        };
                        let mut layers_to_split = Vec::new();
                        for (target, _) in &gesture.targets {
                            if layers_to_split.contains(&target.layer) {
                                continue;
                            }
                            let should_split = if !multiple {
                                true
                            } else {
                                self.symbol()
                                    .timeline
                                    .layers
                                    .iter()
                                    .find(|l| l.id == target.layer)
                                    .and_then(|l| l.key_at(self.frame))
                                    .is_some_and(|k| {
                                        !(start..=end).contains(&k.frame)
                                            && k.elements.iter().any(|e| e.id == target.element)
                                    })
                            };
                            if should_split {
                                layers_to_split.push(target.layer.clone());
                            }
                        }
                        let apply_tween = !matches!(gesture.kind, GestureKind::Pivot { .. });
                        for layer_id in &layers_to_split {
                            if apply_tween {
                                self.split_layer_for_stage_transform(layer_id);
                            } else {
                                self.ensure_key_for(layer_id, false);
                            }
                        }
                    }
                    gesture.began = true;
                } else if gesture.began && !gesture.duplicated && want_duplicate {
                    // Pressing Option/Alt mid-drag restores originals and switches to moving copies.
                    let frame = self.frame;
                    let orig_map: HashMap<_, _> = gesture
                        .targets
                        .iter()
                        .map(|(target, orig_t)| (target.element.as_str(), *orig_t))
                        .collect();
                    let multiple = self.timeline_edit.edit_multiple;
                    for layer in &mut self.symbol_mut().timeline.layers {
                        let key_opt = if multiple {
                            layer.keyframes.iter_mut().find(|k| k.frame == frame)
                        } else {
                            layer.key_at_mut(frame)
                        };
                        if let Some(key) = key_opt {
                            for element in &mut key.elements {
                                if let Some(orig_t) = orig_map.get(element.id.as_str()) {
                                    element.transform = *orig_t;
                                }
                            }
                        }
                    }
                    for (layer_id, key_frame, elem_id, orig_t) in &gesture.other_keyframes {
                        if let Some(layer) = self
                            .symbol_mut()
                            .timeline
                            .layers
                            .iter_mut()
                            .find(|l| &l.id == layer_id)
                            && let Some(key) =
                                layer.keyframes.iter_mut().find(|k| k.frame == *key_frame)
                            && let Some(elem) = key.elements.iter_mut().find(|e| &e.id == elem_id)
                        {
                            elem.transform = *orig_t;
                        }
                    }
                    let sampled_map: HashMap<SelectionTarget, Element> =
                        self.sampled_targets().into_iter().collect();
                    let source_targets: Vec<(SelectionTarget, Element)> = gesture
                        .targets
                        .iter()
                        .filter_map(|(target, orig_t)| {
                            let mut el = sampled_map.get(target)?.clone();
                            el.transform = *orig_t;
                            Some((target.clone(), el))
                        })
                        .collect();
                    if !source_targets.is_empty() {
                        gesture.targets =
                            self.duplicate_targets_with_offset(source_targets, 0.0, 0.0);
                        gesture.other_keyframes.clear();
                        gesture.duplicated = true;
                        gesture.changed = true;
                        if let Some((_, gp)) = &mut self.authoring_modify.group_pivot {
                            let pos = *gp;
                            self.authoring_modify.group_pivot =
                                Some((self.selection.targets.clone(), pos));
                        }
                    }
                }
                if gesture.began {
                    let frame = self.frame;
                    let all_skew = transforms.iter().any(|t| t.skew != 0.)
                        || (!gesture.duplicated
                            && other_transforms
                                .as_ref()
                                .is_some_and(|ot| ot.iter().any(|t| t.skew != 0.)));
                    let all_pivot = transforms.iter().any(|t| t.pivot.is_some())
                        || (!gesture.duplicated
                            && other_transforms
                                .as_ref()
                                .is_some_and(|ot| ot.iter().any(|t| t.pivot.is_some())));
                    // Affine axes (skew) need document format 5; custom pivots need format 7.
                    if all_skew {
                        self.project.format_version = self.project.format_version.max(5);
                    }
                    if all_pivot {
                        self.project.format_version = self.project.format_version.max(7);
                    }
                    let changed: HashMap<_, _> = gesture
                        .targets
                        .iter()
                        .zip(transforms)
                        .map(|((target, _), transform)| (target.element.as_str(), transform))
                        .collect();
                    let multiple = self.timeline_edit.edit_multiple && !gesture.duplicated;
                    let (edit_start, edit_end) = if multiple {
                        self.onion_range()
                    } else {
                        (frame, frame)
                    };
                    for layer in &mut self.symbol_mut().timeline.layers {
                        let key_opt = if multiple {
                            if layer.keyframes.iter().any(|k| k.frame == frame) {
                                layer.keyframes.iter_mut().find(|k| k.frame == frame)
                            } else if layer
                                .key_at(frame)
                                .is_some_and(|k| !(edit_start..=edit_end).contains(&k.frame))
                            {
                                layer.key_at_mut(frame)
                            } else {
                                None
                            }
                        } else {
                            layer.key_at_mut(frame)
                        };
                        if let Some(key) = key_opt {
                            for element in &mut key.elements {
                                if let Some(transform) = changed.get(element.id.as_str()) {
                                    element.transform = *transform;
                                }
                            }
                        }
                    }
                    if !gesture.duplicated
                        && let Some(other_transforms) = other_transforms
                    {
                        for ((layer_id, key_frame, elem_id, _), new_transform) in
                            gesture.other_keyframes.iter().zip(other_transforms)
                        {
                            if let Some(layer) = self
                                .symbol_mut()
                                .timeline
                                .layers
                                .iter_mut()
                                .find(|l| &l.id == layer_id)
                                && let Some(key) =
                                    layer.keyframes.iter_mut().find(|k| k.frame == *key_frame)
                                && let Some(elem) =
                                    key.elements.iter_mut().find(|e| &e.id == elem_id)
                            {
                                elem.transform = new_transform;
                            }
                        }
                    }
                }
            }
        }
        self.object_gesture = Some(gesture);
    }

    /// Classic Merge Drawing marquee semantics: crossing a raw fill or stroke cuts at the
    /// marquee boundary and selects only the enclosed fragments. Object Drawing placements and
    /// authored groups remain whole objects.
    fn split_marquee_merge_geometry(&mut self, gesture: &SelectionGesture) {
        if !gesture.moved || !matches!(gesture.kind, GestureKind::Marquee) {
            return;
        }
        let from = position(gesture.scope.local(gesture.press));
        let to = position(gesture.scope.local(gesture.latest));
        let rect = Rect::from_two_pos(from, to);
        if rect.width() <= 0.5 || rect.height() <= 0.5 {
            return;
        }
        let polygon = vec![
            PathCommand::MoveTo {
                x: f64::from(rect.min.x),
                y: f64::from(rect.min.y),
            },
            PathCommand::LineTo {
                x: f64::from(rect.max.x),
                y: f64::from(rect.min.y),
            },
            PathCommand::LineTo {
                x: f64::from(rect.max.x),
                y: f64::from(rect.max.y),
            },
            PathCommand::LineTo {
                x: f64::from(rect.min.x),
                y: f64::from(rect.max.y),
            },
            PathCommand::Close,
        ];
        let editable_layers: HashSet<_> = self
            .editable_targets()
            .into_iter()
            .map(|target| target.layer)
            .collect();
        let mut candidate = self.project.clone();
        let symbol_id = self.symbol_id().to_owned();
        let Some(symbol) = candidate.symbol_mut(&symbol_id) else {
            return;
        };
        let mut selected = Vec::new();
        for layer in &mut symbol.timeline.layers {
            if !editable_layers.contains(&layer.id) {
                continue;
            }
            let layer_id = layer.id.clone();
            let result = edit_ungrouped_merge_shapes(layer, self.frame, |elements| {
                split_merge_geometry_by_polygon(elements, &polygon)
            });
            match result {
                Some(Ok(ids)) => selected.extend(ids.into_iter().map(|element| SelectionTarget {
                    layer: layer_id.clone(),
                    element,
                })),
                Some(Err(error)) => {
                    self.message(format!("Marquee could not split drawing geometry: {error}"));
                    return;
                }
                None => {}
            }
        }
        if selected.is_empty() {
            return;
        }
        candidate.upgrade_format_for_features();
        if let Err(error) = candidate.validate() {
            self.message(format!(
                "Marquee could not preserve valid drawing geometry: {error}"
            ));
            return;
        }
        let mut targets = self.selection.targets.clone();
        targets.extend(selected);
        if candidate != self.project {
            self.transact_named("Marquee split geometry", |editor| {
                editor.project = candidate;
            });
        }
        self.set_selection(targets);
    }

    pub(crate) fn selection_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        rect: Rect,
        scene: &[DrawItem],
        response: &egui::Response,
    ) {
        self.cancel_stale_selection_gesture();
        if self
            .object_gesture
            .as_ref()
            .is_some_and(|g| g.scope.origin != origin || g.scope.zoom != self.zoom)
        {
            self.cancel_selection_gesture();
        }
        self.normalize_targets();
        let zoom = self.zoom;
        let screen = |point: Pos2| origin + point.to_vec2() * zoom;
        let geometry = self.selection_geometry(scene, p);
        // Free Transform zones, first match wins: handles, then skew edges, then rotate corners.
        let mut zones: Vec<(Zone, GestureKind, Cursor)> = Vec::new();
        let mut transform_frame = None;
        let mut transform_pivot_screen = None;
        if let Some(g) = &geometry {
            if self.tool == Tool::Transform {
                // `NF2/Transform Box` (Scale mode) on the selection's stage bounds, also for
                // rotated objects (the box itself is never rotated).
                let frame = Rect::from_min_max(screen(g.world.min), screen(g.world.max));
                transform_frame = Some(frame);
                let rects = cv::paint_transform_box(p, p.ctx(), frame, cv::TransformMode::Scale);
                let pivot_world = g.world_pivot;
                let pivot_screen = screen(position(pivot_world));
                transform_pivot_screen = Some(pivot_screen);
                if pivot_screen.distance(frame.center()) > 0.5 {
                    cv::transformation_point(p, p.ctx(), pivot_screen);
                }
                let pivot_first = (frame.width() > 18.0 && frame.height() > 18.0)
                    || pivot_screen.distance(frame.center()) > 0.5;
                if pivot_first {
                    zones.push((
                        Zone::Pivot(pivot_screen),
                        GestureKind::Pivot {
                            initial_world: pivot_world,
                        },
                        nf::MOVE,
                    ));
                }
                let world = bounds(g.world);
                use geometry::ResizeHandle as H;
                // `paint_transform_box` order: corners clockwise from top-left, then edges T R B L.
                const BOX: [H; 8] = [
                    H::NorthWest,
                    H::NorthEast,
                    H::SouthEast,
                    H::SouthWest,
                    H::North,
                    H::East,
                    H::South,
                    H::West,
                ];
                if g.resizable {
                    for (rect, box_handle) in rects.iter().zip(BOX) {
                        let kind = if let (Some(local), Some(initial)) = (g.local, g.initial) {
                            // A single placement resizes in its own axes: the box handle drives the
                            // local handle of the same kind nearest to it on screen.
                            let distance = |h: &H| {
                                screen(position(geometry::transformed_point(
                                    initial,
                                    h.point(local),
                                )))
                                .distance(rect.center())
                            };
                            let handle = H::ALL
                                .into_iter()
                                .filter(|h| h.is_corner() == box_handle.is_corner())
                                .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                                .unwrap_or(box_handle);
                            GestureKind::Resize {
                                bounds: local,
                                handle,
                            }
                        } else {
                            GestureKind::Scale {
                                pivot: box_handle.opposite().point(world),
                                center: pivot_world,
                            }
                        };
                        let cursor =
                            crate::stage_cursor::resize_cursor(rect.center() - frame.center());
                        zones.push((Zone::Handle(*rect), kind, cursor));
                    }
                }
                if !pivot_first {
                    zones.push((
                        Zone::Pivot(pivot_screen),
                        GestureKind::Pivot {
                            initial_world: pivot_world,
                        },
                        nf::MOVE,
                    ));
                }
                let [nw, ne, se, sw, n, e, s, w] = rects.map(|r| r.center());
                let edges = [
                    (nw, n, true, world.min[1], world.max[1]),
                    (n, ne, true, world.min[1], world.max[1]),
                    (sw, s, true, world.max[1], world.min[1]),
                    (s, se, true, world.max[1], world.min[1]),
                    (nw, w, false, world.min[0], world.max[0]),
                    (w, sw, false, world.min[0], world.max[0]),
                    (ne, e, false, world.max[0], world.min[0]),
                    (e, se, false, world.max[0], world.min[0]),
                ];
                for (from, to, horizontal, edge, opposite) in edges {
                    // Leave room around the handles; tiny boxes have no skew zone.
                    let gap = (to - from).normalized() * 9.;
                    if from.distance(to) <= 18. || !gap.is_finite() {
                        continue;
                    }
                    let kind = GestureKind::Skew {
                        horizontal,
                        edge,
                        opposite,
                        center: pivot_world[if horizontal { 1 } else { 0 }],
                    };
                    let cursor = if horizontal { nf::SKEW_H } else { nf::SKEW_V };
                    zones.push((Zone::Edge(from + gap, to - gap), kind, cursor));
                }
                let single = g.local.is_some() && g.initial.is_some();
                for corner in [nw, ne, se, sw] {
                    let kind = GestureKind::Rotate {
                        single,
                        pivot: pivot_world,
                        local_bounds: g.local,
                    };
                    zones.push((Zone::Rotate { corner, frame }, kind, nf::ROTATE));
                }
            } else {
                // `NF2/Selection Box` around Object-mode placements; hatch mesh over Merge shapes.
                let any_merge = g.boxes.iter().any(|(_, _, _, merge)| *merge);
                let has_mask_layers = any_merge
                    && self.show_layer_masking(p.ctx())
                    && self.project.symbols.iter().any(|s| {
                        s.timeline
                            .layers
                            .iter()
                            .any(|l| l.properties.role == LayerRole::Mask)
                    });
                let authored = has_mask_layers.then(|| {
                    self.presented_interactive_authored_scene(self.show_layer_masking(p.ctx()))
                });
                let masks = authored
                    .as_ref()
                    .filter(|scene| !scene.clips.is_empty())
                    .map(|scene| crate::stage_clipping::ClipMeshes::new(&scene.clips, origin, zoom))
                    .transpose();
                let mut items_by_root: HashMap<&Id, Vec<&DrawItem>> = HashMap::new();
                if any_merge {
                    for item in scene {
                        if let Some(root_id) = item.instance_path.first() {
                            items_by_root.entry(root_id).or_default().push(item);
                        }
                    }
                }
                let clips_by_path: HashMap<&[Id], &[Id]> = authored
                    .as_ref()
                    .filter(|scene| !scene.clips.is_empty())
                    .map(|scene| {
                        scene
                            .draws
                            .iter()
                            .map(|draw| {
                                (draw.item.instance_path.as_slice(), draw.clip_ids.as_slice())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                for (id, rect, kind, is_merge) in &g.boxes {
                    let rect = Rect::from_min_max(screen(rect.min), screen(rect.max));
                    if *is_merge {
                        let masks = match &masks {
                            Ok(masks) => masks.as_ref(),
                            Err(error) => {
                                if self.status != *error {
                                    self.message(error.clone());
                                }
                                continue;
                            }
                        };
                        if let Some(items) = items_by_root.get(id) {
                            for item in items {
                                let clip_ids = clips_by_path
                                    .get(item.instance_path.as_slice())
                                    .copied()
                                    .unwrap_or(&[]);
                                if let Err(error) = crate::stage_clipping::paint_shape_selection(
                                    p,
                                    item,
                                    origin,
                                    zoom,
                                    rect,
                                    masks.map(|m| (m, clip_ids)),
                                ) && self.status != error
                                {
                                    self.message(error);
                                }
                            }
                        }
                    } else {
                        cv::paint_selection_box(p, p.ctx(), rect, *kind);
                    }
                }
            }
        }
        if !matches!(self.tool, Tool::Select | Tool::Transform) {
            return;
        }
        let move_cursor = if self.tool == Tool::Transform {
            nf::MOVE
        } else {
            nf::ARROW_MOVE
        };
        if let Some(pos) = ui.input(|input| input.pointer.hover_pos())
            && rect.contains(pos)
        {
            let local = Pos2::ZERO + (pos - origin) / self.zoom;
            self.stage_cursor = Some(if let Some(gesture) = &self.object_gesture {
                gesture.cursor
            } else if let Some((_, _, cursor)) = zones.iter().find(|(zone, ..)| zone.contains(pos))
            {
                *cursor
            } else if transform_frame.is_some_and(|frame| frame.contains(pos))
                || self.pick_placement(scene, p, local).is_some()
            {
                move_cursor
            } else {
                nf::ARROW
            });
        }
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } if rect.contains(pos)
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == response.layer_id) =>
                {
                    self.finish_edits();
                    self.playing = false;
                    let original_selection = self.selection.clone();
                    let local = Pos2::ZERO + (pos - origin) / self.zoom;
                    let zone = zones
                        .iter()
                        .find(|(zone, ..)| zone.contains(pos))
                        .map(|(_, kind, cursor)| (*kind, *cursor));
                    if matches!(zone, Some((GestureKind::Pivot { .. }, _))) {
                        let pivot_click_id = egui::Id::new("nf2-pivot-click");
                        let now = ui.input(|i| i.time);
                        let double = ui
                            .ctx()
                            .data(|d| d.get_temp::<(f64, Pos2)>(pivot_click_id))
                            .is_some_and(|(t, prev)| {
                                now >= t && now - t <= 0.45 && prev.distance(pos) <= 8.0
                            });
                        if double {
                            ui.ctx()
                                .data_mut(|d| d.remove::<(f64, Pos2)>(pivot_click_id));
                            self.reset_transformation_point();
                            continue;
                        }
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(pivot_click_id, (now, pos)));
                    }
                    let hit = self.pick_placement(scene, p, local);
                    // Preferences ▸ Shift-select off: clicking an object adds it without ⇧.
                    let additive =
                        modifiers.shift || (!self.workspace.shift_select && hit.is_some());
                    let mut cursor = move_cursor;
                    let kind = if let Some((kind, zone_cursor)) = zone {
                        cursor = zone_cursor;
                        kind
                    } else if hit
                        .as_ref()
                        .is_none_or(|target| self.selection.targets.contains(target))
                        && transform_frame.is_some_and(|frame| frame.contains(pos))
                    {
                        // Free Transform: dragging anywhere inside the box moves the selection.
                        GestureKind::Move
                    } else if let Some(target) = &hit {
                        if !self.selection.targets.contains(target) {
                            let mut targets = if additive {
                                self.selection.targets.clone()
                            } else {
                                vec![]
                            };
                            targets.push(target.clone());
                            self.set_selection(targets);
                        }
                        GestureKind::Move
                    } else {
                        if !modifiers.shift {
                            self.clear_selection();
                        }
                        cursor = nf::ARROW;
                        GestureKind::Marquee
                    };
                    let targets: Vec<(SelectionTarget, Transform)> = self
                        .sampled_targets()
                        .into_iter()
                        .map(|(target, element)| (target, element.transform))
                        .collect();
                    let other_keyframes = if self.timeline_edit.edit_multiple {
                        let (start, end) = self.onion_range();
                        let selected_set: HashSet<&SelectionTarget> =
                            targets.iter().map(|(t, _)| t).collect();
                        let mut others = Vec::new();
                        let sym = self.symbol();
                        for layer in &sym.timeline.layers {
                            if layer.properties.role == LayerRole::Folder
                                || !layer_effectively_visible(&sym.timeline, layer)
                                || layer_effectively_locked(&sym.timeline, layer)
                            {
                                continue;
                            }
                            for key in &layer.keyframes {
                                if !(start..=end).contains(&key.frame) || key.frame == self.frame {
                                    continue;
                                }
                                for elem in &key.elements {
                                    let candidate = SelectionTarget {
                                        layer: layer.id.clone(),
                                        element: elem.id.clone(),
                                    };
                                    if selected_set.contains(&candidate) {
                                        others.push((
                                            layer.id.clone(),
                                            key.frame,
                                            elem.id.clone(),
                                            elem.transform,
                                        ));
                                    }
                                }
                            }
                        }
                        others
                    } else {
                        vec![]
                    };
                    let initial_group_pivot = self
                        .authoring_modify
                        .group_pivot
                        .as_ref()
                        .filter(|(t, _)| *t == self.selection.targets)
                        .map(|(_, gp)| *gp);
                    self.object_gesture = Some(SelectionGesture {
                        scope: Scope::new(self, origin),
                        kind,
                        press: pos,
                        latest: pos,
                        original_selection,
                        targets,
                        other_keyframes,
                        hit,
                        shift: additive,
                        duplicate: modifiers.alt && matches!(kind, GestureKind::Move),
                        duplicated: false,
                        moved: false,
                        began: false,
                        changed: false,
                        angle: 0.,
                        snap_bounds: None,
                        initial_group_pivot,
                        cursor,
                    });
                }
                egui::Event::PointerMoved(pos) => {
                    self.update_selection_gesture(pos, ui.input(|i| i.modifiers), scene, p)
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                } => {
                    self.update_selection_gesture(pos, modifiers, scene, p);
                    if let Some(gesture) = self.object_gesture.as_ref()
                        && !gesture.moved
                        && matches!(gesture.kind, GestureKind::Move | GestureKind::Marquee)
                    {
                        let mut targets = if gesture.shift {
                            gesture.original_selection.targets.clone()
                        } else {
                            vec![]
                        };
                        if let Some(hit) = gesture.hit.clone() {
                            if gesture.shift && targets.contains(&hit) {
                                let group = self.grouped_targets(&hit);
                                targets.retain(|target| !group.contains(target));
                            } else {
                                targets.push(hit);
                            }
                        }
                        self.set_selection(targets);
                    }
                    if let Some(gesture) = self
                        .object_gesture
                        .as_ref()
                        .filter(|gesture| {
                            gesture.moved && matches!(gesture.kind, GestureKind::Marquee)
                        })
                        .cloned()
                    {
                        self.split_marquee_merge_geometry(&gesture);
                    }
                    self.finish_selection_gesture();
                }
                egui::Event::PointerGone if !ui.input(|i| i.pointer.primary_down()) => {
                    self.cancel_selection_gesture();
                }
                _ => {}
            }
        }
        if let Some(g) = &self.object_gesture
            && matches!(g.kind, GestureKind::Marquee)
            && g.moved
        {
            cv::paint_marquee(p, p.ctx(), Rect::from_two_pos(g.press, g.latest));
        }
        if self.tool == Tool::Transform
            && response.double_clicked()
            && !self.stage_view.guide_pointer
            && let Some(pivot_screen) = transform_pivot_screen
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|pos| pos.distance(pivot_screen) <= 8.0)
        {
            self.reset_transformation_point();
        }
        if self.tool == Tool::Select && response.double_clicked() && !self.stage_view.guide_pointer
        {
            if let Some(Element {
                kind: ElementKind::Instance { symbol_id, .. },
                ..
            }) = self.selected_element().cloned()
            {
                self.enter_symbol(symbol_id);
            } else if self.selected_element().is_some_and(|e| {
                matches!(
                    e.kind,
                    ElementKind::Text { .. } | ElementKind::TextBox { .. }
                )
            }) {
                self.show_panel(crate::workspace::PanelId::Properties);
                self.focus_selected_text_editor(ui.ctx());
            } else if let Some(target) = self.selection.single().cloned() {
                self.expand_merge_double_click_selection(&target);
            }
        }
    }

    /// Double-clicking a Merge-mode fill selects the fill and its adjacent Merge strokes;
    /// double-clicking a Merge-mode stroke selects all connected Merge stroke segments.
    pub(crate) fn expand_merge_double_click_selection(&mut self, target: &SelectionTarget) {
        let Some(layer_idx) = self.target_layer_index(target) else {
            return;
        };
        let Some(key) = self.symbol().timeline.layers[layer_idx].key_at(self.frame) else {
            return;
        };
        let Some(el) = key.elements.iter().find(|e| e.id == target.element) else {
            return;
        };
        if !el.merge {
            return;
        }
        let ids = if is_merge_fill(el) {
            merge_fill_adjacent_stroke_ids(&key.elements, &target.element)
        } else if is_merge_stroke(el) {
            merge_connected_stroke_ids(&key.elements, &target.element)
        } else {
            Vec::new()
        };
        if ids.len() > 1 {
            let targets = ids
                .into_iter()
                .map(|element| SelectionTarget {
                    layer: target.layer.clone(),
                    element,
                })
                .collect();
            self.set_selection(targets);
        }
    }

    /// Reset the Free Transform transformation point (pivot) to the selection's default centre.
    pub(crate) fn reset_transformation_point(&mut self) {
        if let Some(target) = self.selection.single().cloned() {
            if !self.target_editable(&target)
                || self
                    .selected_sampled()
                    .is_none_or(|e| e.transform.pivot.is_none())
            {
                return;
            }
            self.transact(|s| {
                s.ensure_key_for(&target.layer, false);
                if let Some(layer_idx) = s.target_layer_index(&target) {
                    let frame = s.frame;
                    if let Some(el) = s.symbol_mut().timeline.layers[layer_idx]
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    {
                        el.transform.pivot = None;
                    }
                }
            });
        } else if self
            .authoring_modify
            .group_pivot
            .as_ref()
            .is_some_and(|(t, _)| *t == self.selection.targets)
        {
            self.authoring_modify.group_pivot = None;
        }
    }

    pub(crate) fn shape_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        _page: Rect,
        rect: Rect,
    ) {
        let stale = self.shape_draft.as_ref().is_some_and(|draft| {
            !draft.scope.matches(self)
                || draft.scope.origin != origin
                || draft.scope.zoom != self.zoom
                || draft.tool != self.tool
                || self
                    .symbol()
                    .timeline
                    .layers
                    .get(self.layer)
                    .is_none_or(|layer| layer.id != draft.layer || layer.locked || !layer.visible)
        });
        if stale {
            self.shape_draft = None;
        }
        for event in ui.input(|input| input.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if matches!(self.tool, Tool::Rectangle | Tool::Ellipse | Tool::Text)
                    && rect.contains(pos)
                    && self.layer_editable()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id()) =>
                {
                    self.finish_edits();
                    self.playing = false;
                    let start = self.snap_screen(origin, pos);
                    self.shape_draft = Some(ShapeDraft {
                        scope: Scope::new(self, origin),
                        layer: self.symbol().timeline.layers[self.layer].id.clone(),
                        tool: self.tool,
                        start,
                    });
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                } => {
                    if let Some(draft) = self.shape_draft.take() {
                        let pos = self.snap_screen(origin, pos);
                        let bounds = shape_draft_bounds(&draft, pos, modifiers);
                        if bounds.width() > 2. && bounds.height() > 2. {
                            if draft.tool == Tool::Text {
                                self.create_stage_text(ui.ctx(), bounds.min, Some(bounds));
                                continue;
                            }
                            let mut transform = Transform {
                                x: bounds.min.x as f64,
                                y: bounds.min.y as f64,
                                ..Default::default()
                            };
                            let kind = if draft.tool == Tool::Rectangle {
                                if !self.authoring_tools.rect_options.is_default() {
                                    transform.primitive = Some(PrimitiveOptions::Rectangle(
                                        self.authoring_tools.rect_options,
                                    ));
                                }
                                ElementKind::Rectangle {
                                    width: bounds.width() as f64,
                                    height: bounds.height() as f64,
                                    fill: self.fill,
                                    stroke: self.stroke,
                                    stroke_width: self.stroke_width,
                                }
                            } else {
                                if !self.authoring_tools.oval_options.is_default() {
                                    transform.primitive = Some(PrimitiveOptions::Oval(
                                        self.authoring_tools.oval_options,
                                    ));
                                }
                                ElementKind::Ellipse {
                                    width: bounds.width() as f64,
                                    height: bounds.height() as f64,
                                    fill: self.fill,
                                    stroke: self.stroke,
                                    stroke_width: self.stroke_width,
                                }
                            };
                            self.add_element_transformed(kind, transform);
                        }
                    }
                }
                _ => {}
            }
        }
        if self.shape_draft.is_some()
            && let Some(pos) = ui.input(|input| input.pointer.latest_pos())
            && let pos = self.snap_screen(origin, pos)
            && let Some(draft) = &self.shape_draft
        {
            let modifiers = ui.input(|input| input.modifiers);
            let local_bounds = shape_draft_bounds(draft, pos, modifiers);
            let screen = |pt: Pos2| draft.scope.origin + pt.to_vec2() * draft.scope.zoom;
            let preview_rect =
                Rect::from_min_max(screen(local_bounds.min), screen(local_bounds.max));
            let palette = UiPalette::current(p.ctx());
            // Keep the gesture overlay, but follow the same oval contour as the
            // committed primitive (including arcs and its inner radius).
            let oval = (draft.tool == Tool::Ellipse).then(|| {
                oval_path_commands(
                    local_bounds.width() as f64,
                    local_bounds.height() as f64,
                    self.authoring_tools.oval_options,
                )
            });
            for (width, color) in [(3.0_f32, palette.overlay_halo), (1.0_f32, palette.overlay)] {
                if let Some(commands) = &oval {
                    crate::vector::paint_path(
                        p,
                        commands,
                        crate::vector::PathPaint {
                            rule: FillRule::NonZero,
                            fill: egui::Color32::TRANSPARENT,
                            stroke: color,
                            width: (width / draft.scope.zoom) as f64,
                            tolerance: 0.25 / draft.scope.zoom,
                            fingerprint: None,
                        },
                        |point| preview_rect.min + point.to_vec2() * draft.scope.zoom,
                    );
                } else {
                    p.rect_stroke(
                        preview_rect,
                        0.,
                        Stroke::new(width, color),
                        StrokeKind::Inside,
                    );
                }
            }
        }
    }
}

fn shape_draft_bounds(draft: &ShapeDraft, pos: Pos2, modifiers: egui::Modifiers) -> Rect {
    let start = position(draft.scope.local(draft.start));
    let end = position(draft.scope.local(pos));
    let mut delta = end - start;
    if modifiers.shift {
        let side = delta.x.abs().max(delta.y.abs());
        delta.x = if delta.x < 0. { -side } else { side };
        delta.y = if delta.y < 0. { -side } else { side };
    }
    if modifiers.alt {
        Rect::from_center_size(start, egui::vec2(2.0 * delta.x.abs(), 2.0 * delta.y.abs()))
    } else {
        Rect::from_two_pos(start, start + delta)
    }
}
