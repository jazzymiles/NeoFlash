//! Shape-tween correspondence markers. The document keeps the existing normalized hint pair;
//! only visibility and the active pointer gesture live in the editor.
use crate::{app::Editor, stage_mapping::StageMapping};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use neoflash_ui::components::canvas::{self as cv, ShapeHintState};

#[derive(Debug)]
pub(crate) struct ShapeHintsState {
    pub show: bool,
    pub drag: Option<ShapeHintDrag>,
    pub hover: Option<char>,
    pub hover_target: Option<(char, u32)>,
    pub context_menu: Option<(char, u32, Pos2)>,
}
impl Default for ShapeHintsState {
    fn default() -> Self {
        Self {
            show: true,
            drag: None,
            hover: None,
            hover_target: None,
            context_menu: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShapeHintDrag {
    symbol: Id,
    layer: Id,
    start: u32,
    frame: u32,
    label: char,
    end: bool,
    began: bool,
    press: Option<[f64; 2]>,
    /// Mapping captured by the stage pointer phase; direct command tests leave it unset.
    mapping: Option<StageMapping>,
    geometry: ShapeHintGeometry,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[derive(Clone, Debug)]
pub(crate) struct ShapeHintMarker {
    pub label: char,
    pub position: [f64; 2],
    pub state: ShapeHintState,
    pub start_frame: u32,
    pub end: bool,
}

impl Editor {
    /// Active span, including its ending key. On chained spans a start carrying hints wins;
    /// otherwise the preceding span's ending hints remain available.
    pub(crate) fn shape_hint_span(&self) -> Option<(usize, usize)> {
        let timeline = &self.symbol().timeline;
        let layer = timeline.layers.get(self.layer)?;
        if layer_effectively_locked(timeline, layer) || !layer_effectively_visible(timeline, layer)
        {
            return None;
        }
        let current = layer
            .keyframes
            .iter()
            .rposition(|key| key.frame <= self.frame)?;
        let shape = |index: usize| {
            let key = &layer.keyframes[index];
            key.tween.is_some()
                && key.tween_kind == TweenKind::Shape
                && layer.keyframes.get(index + 1).is_some()
        };
        if shape(current)
            && (!layer.keyframes[current].shape_hints.is_empty()
                || current == 0
                || layer.keyframes[current].frame != self.frame
                || !shape(current - 1))
        {
            Some((current, current + 1))
        } else if current > 0 && layer.keyframes[current].frame == self.frame && shape(current - 1)
        {
            Some((current - 1, current))
        } else if shape(current) {
            Some((current, current + 1))
        } else {
            None
        }
    }

    /// Span targeted when adding a new Shape Hint (`SH-10`). On a boundary keyframe `K1` where
    /// `K0 -> K1` and `K1 -> K2` are both Shape Tweens, adding a hint targets the outgoing span
    /// `K1 -> K2` unless the timeline frame selection explicitly lies strictly inside `K0..K1`.
    pub(crate) fn shape_hint_add_span(&self) -> Option<(usize, usize)> {
        let timeline = &self.symbol().timeline;
        let layer = timeline.layers.get(self.layer)?;
        if layer_effectively_locked(timeline, layer) || !layer_effectively_visible(timeline, layer)
        {
            return None;
        }
        let current = layer
            .keyframes
            .iter()
            .rposition(|key| key.frame <= self.frame)?;
        let shape = |index: usize| {
            let key = &layer.keyframes[index];
            key.tween.is_some()
                && key.tween_kind == TweenKind::Shape
                && layer.keyframes.get(index + 1).is_some()
        };
        let explicitly_incoming = current > 0
            && layer.keyframes[current].frame == self.frame
            && shape(current - 1)
            && self
                .frame_range()
                .is_some_and(|range| range.end < layer.keyframes[current].frame);
        if explicitly_incoming {
            Some((current - 1, current))
        } else if shape(current) {
            Some((current, current + 1))
        } else if current > 0 && layer.keyframes[current].frame == self.frame && shape(current - 1)
        {
            Some((current - 1, current))
        } else {
            None
        }
    }

    fn shape_hint_visible_spans(&self) -> Vec<(usize, usize, bool)> {
        let timeline = &self.symbol().timeline;
        let Some(layer) = timeline.layers.get(self.layer) else {
            return vec![];
        };
        if layer_effectively_locked(timeline, layer) || !layer_effectively_visible(timeline, layer)
        {
            return vec![];
        }
        let Some(current) = layer
            .keyframes
            .iter()
            .rposition(|key| key.frame <= self.frame)
        else {
            return vec![];
        };
        let shape = |index: usize| {
            let key = &layer.keyframes[index];
            key.tween.is_some()
                && key.tween_kind == TweenKind::Shape
                && layer.keyframes.get(index + 1).is_some()
        };
        let mut spans = Vec::new();
        if current > 0 && layer.keyframes[current].frame == self.frame && shape(current - 1) {
            spans.push((current - 1, current, true));
        }
        if layer.keyframes[current].frame == self.frame && shape(current) {
            spans.push((current, current + 1, false));
        }
        spans
    }

    pub(crate) fn can_add_shape_hint(&self) -> bool {
        self.shape_hint_add_span().is_some_and(|(start, end)| {
            let keys = &self.symbol().timeline.layers[self.layer].keyframes;
            keys[start].shape_hints.len() < ShapeHint::MAX_HINTS
                && ShapeHintGeometry::from_key(&keys[start]).is_some()
                && ShapeHintGeometry::from_key(&keys[end]).is_some()
        })
    }
    pub(crate) fn can_remove_shape_hints(&self) -> bool {
        self.shape_hint_span().is_some_and(|(start, _)| {
            !self.symbol().timeline.layers[self.layer].keyframes[start]
                .shape_hints
                .is_empty()
        })
    }
    pub(crate) fn remove_all_shape_hints(&mut self) -> bool {
        self.remove_shape_hints()
    }
    pub(crate) fn toggle_shape_hints(&mut self) {
        self.cancel_shape_hint_drag();
        self.timeline_shape_hints.show = !self.timeline_shape_hints.show;
        self.timeline_shape_hints.hover = None;
        self.timeline_shape_hints.hover_target = None;
        self.timeline_shape_hints.context_menu = None;
    }
    pub(crate) fn shape_hint_markers(&self) -> Vec<ShapeHintMarker> {
        if !self.timeline_shape_hints.show || self.playing {
            return vec![];
        }
        let spans = self.shape_hint_visible_spans();
        if spans.is_empty() {
            return vec![];
        }
        let keys = &self.symbol().timeline.layers[self.layer].keyframes;
        let mut markers = Vec::new();
        for (start, end, at_end) in spans {
            let (Some(a), Some(b)) = (
                ShapeHintGeometry::from_key(&keys[start]),
                ShapeHintGeometry::from_key(&keys[end]),
            ) else {
                continue;
            };
            let active = active_shape_hint_labels(
                &keys[start].elements,
                &keys[end].elements,
                &keys[start].shape_hints,
            );
            let start_frame = keys[start].frame;
            for hint in &keys[start].shape_hints {
                let matched = a.placed(hint.start_uv)
                    && b.placed(hint.end_uv)
                    && active.contains(&hint.label);
                markers.push(ShapeHintMarker {
                    label: hint.label,
                    position: if at_end {
                        b.position(hint.end_uv)
                    } else {
                        a.position(hint.start_uv)
                    },
                    state: if !matched {
                        ShapeHintState::Unplaced
                    } else if at_end {
                        ShapeHintState::End
                    } else {
                        ShapeHintState::Start
                    },
                    start_frame,
                    end: at_end,
                });
            }
        }
        markers
    }
    #[cfg(test)]
    pub(crate) fn begin_shape_hint_drag(&mut self, label: char) -> bool {
        let Some(marker) = self
            .shape_hint_markers()
            .into_iter()
            .rev()
            .find(|hint| hint.label == label)
        else {
            return false;
        };
        self.begin_shape_hint_drag_in_span(marker.start_frame, marker.end, label)
    }

    pub(crate) fn begin_shape_hint_drag_in_span(
        &mut self,
        start_frame: u32,
        at_end: bool,
        label: char,
    ) -> bool {
        if !self.shape_hint_markers().iter().any(|hint| {
            hint.label == label && hint.start_frame == start_frame && hint.end == at_end
        }) {
            return false;
        }
        let layer = &self.symbol().timeline.layers[self.layer];
        let Some(start) = layer.keyframes.iter().position(|k| k.frame == start_frame) else {
            return false;
        };
        let end = start + 1;
        if end >= layer.keyframes.len() {
            return false;
        }
        let Some(geometry) =
            ShapeHintGeometry::from_key(&layer.keyframes[if at_end { end } else { start }])
        else {
            return false;
        };
        let drag = ShapeHintDrag {
            symbol: self.symbol_id().into(),
            layer: layer.id.clone(),
            start: start_frame,
            frame: self.frame,
            label,
            end: at_end,
            began: false,
            press: None,
            mapping: None,
            geometry,
        };
        self.finish_edits();
        self.playing = false;
        self.timeline_shape_hints.context_menu = None;
        self.timeline_shape_hints.drag = Some(drag);
        true
    }
    pub(crate) fn drag_shape_hint_to(&mut self, pointer: [f64; 2]) {
        let Some(drag) = self.timeline_shape_hints.drag.clone() else {
            return;
        };
        if drag.symbol != self.symbol_id() || drag.frame != self.frame {
            self.cancel_shape_hint_drag();
            return;
        }
        let snap_radius = if drag.mapping.is_some() {
            (DEFAULT_HINT_VERTEX_SNAP_RADIUS / f64::from(self.zoom.max(0.25))).clamp(1.5, 12.0)
        } else {
            DEFAULT_HINT_VERTEX_SNAP_RADIUS
        };
        let to = drag.geometry.snapped_uv_with_snap(pointer, snap_radius);
        let old = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == drag.layer)
            .and_then(|l| l.keyframes.iter().find(|k| k.frame == drag.start))
            .and_then(|k| k.shape_hints.iter().find(|h| h.label == drag.label))
            .map(|h| if drag.end { h.end_uv } else { h.start_uv });
        if old.is_none_or(|old| old == to) {
            return;
        }
        if !drag.began {
            self.history.begin(&self.project);
            self.timeline_shape_hints.drag.as_mut().unwrap().began = true;
        }
        if let Some(hint) = self
            .symbol_mut()
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == drag.layer)
            .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == drag.start))
            .and_then(|k| k.shape_hints.iter_mut().find(|h| h.label == drag.label))
        {
            if drag.end {
                hint.end_uv = to;
            } else {
                hint.start_uv = to;
            }
        }
    }
    pub(crate) fn finish_shape_hint_drag(&mut self, remove: bool) {
        let Some(mut drag) = self.timeline_shape_hints.drag.take() else {
            return;
        };
        if remove && drag.symbol == self.symbol_id() {
            if !drag.began {
                self.history.begin(&self.project);
                drag.began = true;
            }
            if let Some(key) = self
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == drag.layer)
                .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == drag.start))
            {
                key.shape_hints.retain(|h| h.label != drag.label);
            }
        }
        if drag.began {
            self.commit_history();
            self.reset_authored_preview();
        }
    }
    pub(crate) fn cancel_shape_hint_drag(&mut self) {
        if self
            .timeline_shape_hints
            .drag
            .take()
            .is_some_and(|drag| drag.began)
        {
            self.history.cancel(&mut self.project);
            self.reset_authored_preview();
        }
    }

    /// Guides get first refusal; a captured marker consumes the press before any stage tool.
    pub(crate) fn shape_hint_input(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        page: Rect,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        if self
            .timeline_shape_hints
            .drag
            .as_ref()
            .and_then(|drag| drag.mapping)
            .is_some_and(|captured| captured != mapping)
        {
            self.cancel_shape_hint_drag();
        }
        let zoom = self.zoom;
        let local = |p: Pos2| {
            let point = mapping.screen_to_edit(origin, zoom, p);
            [f64::from(point.x), f64::from(point.y)]
        };
        let screen =
            |p: [f64; 2]| mapping.edit_to_screen(origin, zoom, Pos2::new(p[0] as f32, p[1] as f32));
        let markers = self.shape_hint_markers();
        let hit_marker = |pos| {
            markers
                .iter()
                .rev()
                .find(|h| cv::shape_hint_hit(screen(h.position), pos))
                .cloned()
        };
        let rulers = cv::RulerLayout::new(canvas);
        let drop_bounds = page.intersect(canvas);
        let mut consumed = false;
        let mut consumed_secondary = false;
        let mut consumed_delete = false;
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } if self.timeline_shape_hints.drag.is_none()
                    && !self.stage_view.guide_pointer
                    && canvas.contains(pos)
                    && !(self.stage_view.prefs.rulers && rulers.contains(pos))
                    && ui.ctx().layer_id_at(pos).is_none_or(|l| l == ui.layer_id()) =>
                {
                    if let Some(marker) = hit_marker(pos)
                        && self.begin_shape_hint_drag_in_span(
                            marker.start_frame,
                            marker.end,
                            marker.label,
                        )
                    {
                        let drag = self.timeline_shape_hints.drag.as_mut().unwrap();
                        drag.press = Some(local(pos));
                        drag.mapping = Some(mapping);
                        consumed = true;
                        self.stage_view.guide_pointer = true;
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    ..
                } if self.timeline_shape_hints.drag.is_none()
                    && !self.stage_view.guide_pointer
                    && canvas.contains(pos)
                    && !(self.stage_view.prefs.rulers && rulers.contains(pos))
                    && ui.ctx().layer_id_at(pos).is_none_or(|l| l == ui.layer_id()) =>
                {
                    if let Some(marker) = hit_marker(pos) {
                        self.timeline_shape_hints.context_menu =
                            Some((marker.label, marker.start_frame, pos));
                        consumed_secondary = true;
                    } else {
                        self.timeline_shape_hints.context_menu = None;
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed: false,
                    ..
                } if self.timeline_shape_hints.context_menu.is_some()
                    || hit_marker(pos).is_some() =>
                {
                    if self.timeline_shape_hints.context_menu.is_none()
                        && let Some(marker) = hit_marker(pos)
                    {
                        self.timeline_shape_hints.context_menu =
                            Some((marker.label, marker.start_frame, pos));
                    }
                    consumed_secondary = true;
                }
                egui::Event::PointerMoved(pos) if self.timeline_shape_hints.drag.is_some() => {
                    if self.timeline_shape_hints.drag.as_ref().is_some_and(|drag| {
                        drag.began
                            || drag
                                .press
                                .is_none_or(|press| distance(press, local(pos)) > 1e-6)
                    }) {
                        self.drag_shape_hint_to(local(pos));
                    }
                    consumed = true;
                    self.stage_view.guide_pointer = true;
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } if self.timeline_shape_hints.drag.is_some() => {
                    let moved = self.timeline_shape_hints.drag.as_ref().is_some_and(|drag| {
                        drag.began
                            || drag
                                .press
                                .is_none_or(|press| distance(press, local(pos)) > 1e-6)
                    });
                    if moved && drop_bounds.contains(pos) {
                        self.drag_shape_hint_to(local(pos));
                    }
                    self.finish_shape_hint_drag(moved && !drop_bounds.contains(pos));
                    consumed = true;
                    self.stage_view.guide_pointer = true;
                }
                egui::Event::Key {
                    key: egui::Key::Delete | egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    if self.timeline_shape_hints.drag.is_some() {
                        self.finish_shape_hint_drag(true);
                        consumed_delete = true;
                    } else if let Some((label, start_frame)) =
                        self.timeline_shape_hints.hover_target.or_else(|| {
                            ui.input(|i| i.pointer.hover_pos())
                                .filter(|p| {
                                    canvas.contains(*p)
                                        && !(self.stage_view.prefs.rulers && rulers.contains(*p))
                                })
                                .and_then(hit_marker)
                                .map(|m| (m.label, m.start_frame))
                        })
                        && self.remove_shape_hint_in_span(start_frame, label)
                    {
                        self.timeline_shape_hints.hover = None;
                        self.timeline_shape_hints.hover_target = None;
                        self.timeline_shape_hints.context_menu = None;
                        consumed_delete = true;
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if self.timeline_shape_hints.context_menu.is_some() => {
                    self.timeline_shape_hints.context_menu = None;
                }
                _ => {}
            }
        }
        if consumed || consumed_secondary || consumed_delete {
            ui.ctx().input_mut(|i| {
                i.events.retain(|event| {
                    !(consumed
                        && matches!(
                            event,
                            egui::Event::PointerButton {
                                button: egui::PointerButton::Primary,
                                ..
                            } | egui::Event::PointerMoved(_)
                        ))
                        && !(consumed_secondary
                            && matches!(
                                event,
                                egui::Event::PointerButton {
                                    button: egui::PointerButton::Secondary,
                                    ..
                                }
                            ))
                        && !(consumed_delete
                            && matches!(
                                event,
                                egui::Event::Key {
                                    key: egui::Key::Delete | egui::Key::Backspace,
                                    pressed: true,
                                    ..
                                }
                            ))
                })
            });
        }
        let hovered_marker = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| {
                canvas.contains(*p) && !(self.stage_view.prefs.rulers && rulers.contains(*p))
            })
            .and_then(hit_marker);
        self.timeline_shape_hints.hover = hovered_marker.as_ref().map(|m| m.label);
        self.timeline_shape_hints.hover_target = hovered_marker.map(|m| (m.label, m.start_frame));

        if let Some((label, start_frame, menu_pos)) = self.timeline_shape_hints.context_menu {
            let mut close_menu = false;
            let area = egui::Area::new(egui::Id::new("nf2-shape-hint-context-menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(menu_pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| {
                        crate::ui_icons::menu_contents(ui, 220.0, |ui| {
                            let remove_one = format!("Remove Hint '{label}'");
                            if crate::ui_icons::menu_item_enabled(ui, true, &remove_one, "⌫")
                                .clicked()
                            {
                                self.remove_shape_hint_in_span(start_frame, label);
                                close_menu = true;
                            }
                            if crate::ui_icons::menu_item_enabled(
                                ui,
                                self.can_remove_shape_hints(),
                                "Remove All Hints",
                                "",
                            )
                            .clicked()
                            {
                                self.remove_shape_hints_in_span(start_frame);
                                close_menu = true;
                            }
                        });
                    });
                });
            let clicked_outside = !consumed_secondary
                && ui.input(|i| {
                    i.pointer.any_pressed()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|pos| !area.response.rect.contains(pos))
                });
            if close_menu || clicked_outside {
                self.timeline_shape_hints.context_menu = None;
            }
        }
    }
    pub(crate) fn paint_shape_hints(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        for hint in self.shape_hint_markers() {
            let at = mapping.edit_to_screen(
                origin,
                self.zoom,
                Pos2::new(hint.position[0] as f32, hint.position[1] as f32),
            );
            cv::paint_shape_hint(painter, painter.ctx(), at, hint.label, hint.state);
        }
    }
}
