use crate::{
    app::{Editor, Tool},
    selection::{Selection, SelectionTarget},
    transform_geometry as geometry,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use neoflash_core::*;
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
    Resize {
        bounds: geometry::Bounds,
        handle: geometry::ResizeHandle,
    },
    Scale {
        pivot: geometry::Point,
        center: geometry::Point,
    },
    Rotate {
        bounds: Option<geometry::Bounds>,
        pivot: geometry::Point,
    },
    Marquee,
}

#[derive(Clone)]
pub(crate) struct SelectionGesture {
    scope: Scope,
    kind: GestureKind,
    press: Pos2,
    latest: Pos2,
    original_selection: Selection,
    targets: Vec<(SelectionTarget, Transform)>,
    hit: Option<SelectionTarget>,
    shift: bool,
    moved: bool,
    began: bool,
    changed: bool,
    angle: f64,
}

pub(crate) struct ShapeDraft {
    scope: Scope,
    layer: Id,
    tool: Tool,
    start: Pos2,
}

struct SelectionGeometry {
    corners: [Pos2; 4],
    local: Option<geometry::Bounds>,
    initial: Option<Transform>,
    world: Rect,
    resizable: bool,
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
                self.history.commit(&self.project);
            } else {
                self.history.cancel(&mut self.project);
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
        for item in scene {
            let Some(id) = item.instance_path.first() else {
                continue;
            };
            if !selected.contains(id) {
                continue;
            }
            let rect = crate::stage::local_bounds(item, p);
            world = world.union(transformed_rect(rect, item.transform));
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
        let corners = if let (Some(local), Some(initial)) = (local, initial) {
            [
                geometry::ResizeHandle::NorthWest,
                geometry::ResizeHandle::NorthEast,
                geometry::ResizeHandle::SouthEast,
                geometry::ResizeHandle::SouthWest,
            ]
            .map(|handle| position(geometry::transformed_point(initial, handle.point(local))))
        } else {
            [
                world.left_top(),
                world.right_top(),
                world.right_bottom(),
                world.left_bottom(),
            ]
        };
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
        Some(SelectionGeometry {
            corners,
            local,
            initial,
            world,
            resizable,
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
        scene.iter().rev().find_map(|item| {
            let target = eligible.get(item.instance_path.first()?)?;
            crate::stage::hit_item(item, p, point, self.zoom).then(|| target.clone())
        })
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
        let press = gesture.scope.local(gesture.press);
        let transforms = match gesture.kind {
            GestureKind::Move => {
                let mut delta = [current[0] - press[0], current[1] - press[1]];
                if modifiers.shift {
                    if delta[0].abs() >= delta[1].abs() {
                        delta[1] = 0.;
                    } else {
                        delta[0] = 0.;
                    }
                }
                let moved: Vec<_> = originals
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
                        t.x.is_finite() && t.y.is_finite() && t.x.abs() <= 1e12 && t.y.abs() <= 1e12
                    })
                    .then_some(moved)
            }
            GestureKind::Resize { bounds, handle } => geometry::resize_single(
                originals[0],
                bounds,
                handle,
                press,
                current,
                geometry::ResizeOptions {
                    from_center: modifiers.alt,
                    preserve_aspect: modifiers.shift,
                },
            )
            .map(|t| vec![t]),
            GestureKind::Scale { pivot, center } => geometry::uniform_scale_many(
                &originals,
                if modifiers.alt { center } else { pivot },
                press,
                current,
            ),
            GestureKind::Rotate {
                bounds: Some(bounds),
                ..
            } => geometry::rotate_single_by(originals[0], bounds, gesture.angle, modifiers.shift)
                .map(|t| vec![t]),
            GestureKind::Rotate {
                bounds: None,
                pivot,
            } => geometry::rotate_many_by(&originals, pivot, gesture.angle, modifiers.shift),
            GestureKind::Marquee => unreachable!(),
        };
        if let Some(transforms) = transforms {
            gesture.changed = originals != transforms;
            if gesture.changed && !gesture.began {
                self.history.begin(&self.project);
                let layers: HashSet<_> = gesture
                    .targets
                    .iter()
                    .map(|(target, _)| target.layer.as_str())
                    .collect();
                for layer in layers {
                    self.ensure_key_for(layer, false);
                }
                gesture.began = true;
            }
            if gesture.began {
                let frame = self.frame;
                let changed: HashMap<_, _> = gesture
                    .targets
                    .iter()
                    .zip(transforms)
                    .map(|((target, _), transform)| (target.element.as_str(), transform))
                    .collect();
                for layer in &mut self.symbol_mut().timeline.layers {
                    if let Some(key) = layer.key_at_mut(frame) {
                        for element in &mut key.elements {
                            if let Some(transform) = changed.get(element.id.as_str()) {
                                element.transform = *transform;
                            }
                        }
                    }
                }
            }
        }
        self.object_gesture = Some(gesture);
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
        let screen = |point: Pos2| origin + point.to_vec2() * self.zoom;
        let geometry = self.selection_geometry(scene, p);
        let mut handles = Vec::new();
        if let Some(g) = &geometry {
            let corners = g.corners.map(screen);
            p.add(egui::Shape::closed_line(
                corners.to_vec(),
                Stroke::new(1.0_f32, Color32::from_rgb(36, 104, 190)),
            ));
            if self.tool == Tool::Transform {
                if g.resizable {
                    for handle in geometry::ResizeHandle::ALL {
                        if self.selection.len() > 1 && !handle.is_corner() {
                            continue;
                        }
                        let point = if let (Some(local), Some(initial)) = (g.local, g.initial) {
                            position(geometry::transformed_point(initial, handle.point(local)))
                        } else {
                            position(handle.point(bounds(g.world)))
                        };
                        let point = screen(point);
                        p.rect_filled(
                            Rect::from_center_size(point, Vec2::splat(7.)),
                            0.,
                            Color32::from_rgb(36, 104, 190),
                        );
                        let kind = if let Some(local) = g.local {
                            GestureKind::Resize {
                                bounds: local,
                                handle,
                            }
                        } else {
                            GestureKind::Scale {
                                pivot: handle.opposite().point(bounds(g.world)),
                                center: bounds(g.world).center(),
                            }
                        };
                        handles.push((point, kind));
                    }
                }
                let pivot = if let (Some(local), Some(initial)) = (g.local, g.initial) {
                    geometry::transformed_point(initial, local.center())
                } else {
                    bounds(g.world).center()
                };
                let center = screen(position(pivot));
                let top = corners[0].lerp(corners[1], 0.5);
                let direction = (top - center).normalized();
                let rotate = top
                    + if direction.is_finite() && direction.length_sq() > 0. {
                        direction * 25.
                    } else {
                        Vec2::new(0., -25.)
                    };
                p.line_segment(
                    [top, rotate],
                    Stroke::new(1.0_f32, Color32::from_rgb(36, 104, 190)),
                );
                p.circle_filled(rotate, 4., Color32::from_rgb(36, 104, 190));
                handles.push((
                    rotate,
                    GestureKind::Rotate {
                        bounds: g.local,
                        pivot,
                    },
                ));
            }
        }
        if !matches!(self.tool, Tool::Select | Tool::Transform) {
            return;
        }
        if response.hovered()
            && let Some(pos) = ui.input(|input| input.pointer.hover_pos())
            && handles.iter().any(|(point, _)| point.distance(pos) < 9.)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
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
                    let handle = handles
                        .iter()
                        .rev()
                        .find(|(point, _)| point.distance(pos) < 9.)
                        .map(|(_, kind)| *kind);
                    let hit = self.pick_placement(scene, p, local);
                    let kind = if let Some(handle) = handle {
                        handle
                    } else if let Some(target) = &hit {
                        if !self.selection.targets.contains(target) {
                            let mut targets = if modifiers.shift {
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
                        GestureKind::Marquee
                    };
                    let targets = self
                        .sampled_targets()
                        .into_iter()
                        .map(|(target, element)| (target, element.transform))
                        .collect();
                    self.object_gesture = Some(SelectionGesture {
                        scope: Scope::new(self, origin),
                        kind,
                        press: pos,
                        latest: pos,
                        original_selection,
                        targets,
                        hit,
                        shift: modifiers.shift,
                        moved: false,
                        began: false,
                        changed: false,
                        angle: 0.,
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
                                targets.retain(|target| *target != hit);
                            } else {
                                targets.push(hit);
                            }
                        }
                        self.set_selection(targets);
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
            let marquee = Rect::from_two_pos(g.press, g.latest);
            p.rect_filled(
                marquee,
                0.,
                Color32::from_rgba_unmultiplied(60, 125, 210, 25),
            );
            p.rect_stroke(
                marquee,
                0.,
                Stroke::new(1.0_f32, Color32::from_rgb(36, 104, 190)),
                StrokeKind::Inside,
            );
        }
        if self.tool == Tool::Select
            && response.double_clicked()
            && let Some(Element {
                kind: ElementKind::Instance { symbol_id, .. },
                ..
            }) = self.selected_element().cloned()
        {
            self.enter_symbol(symbol_id);
        }
    }

    pub(crate) fn shape_stage(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        origin: Pos2,
        page: Rect,
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
                } if matches!(self.tool, Tool::Rectangle | Tool::Ellipse)
                    && rect.contains(pos)
                    && page.contains(pos)
                    && self.layer_editable()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id()) =>
                {
                    self.finish_edits();
                    self.playing = false;
                    self.shape_draft = Some(ShapeDraft {
                        scope: Scope::new(self, origin),
                        layer: self.symbol().timeline.layers[self.layer].id.clone(),
                        tool: self.tool,
                        start: pos,
                    });
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    if let Some(draft) = self.shape_draft.take() {
                        let bounds = Rect::from_two_pos(
                            position(draft.scope.local(draft.start)),
                            position(draft.scope.local(pos)),
                        );
                        if bounds.width() > 2. && bounds.height() > 2. {
                            let kind = if draft.tool == Tool::Rectangle {
                                ElementKind::Rectangle {
                                    width: bounds.width() as f64,
                                    height: bounds.height() as f64,
                                    fill: self.fill,
                                    stroke: [30, 50, 70, 255],
                                    stroke_width: 0.,
                                }
                            } else {
                                ElementKind::Ellipse {
                                    width: bounds.width() as f64,
                                    height: bounds.height() as f64,
                                    fill: self.fill,
                                    stroke: [30, 50, 70, 255],
                                    stroke_width: 0.,
                                }
                            };
                            self.add_element(kind, bounds.min.x as f64, bounds.min.y as f64);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(draft) = &self.shape_draft
            && let Some(pos) = ui.input(|input| input.pointer.latest_pos())
        {
            p.rect_stroke(
                Rect::from_two_pos(draft.start, pos),
                0.,
                Stroke::new(1.0_f32, Color32::from_rgb(36, 104, 190)),
                StrokeKind::Inside,
            );
        }
    }
}
