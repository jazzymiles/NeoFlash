//! Shape-tween correspondence markers. The document keeps the existing normalized hint pair;
//! only visibility and the active pointer gesture live in the editor.
use crate::app::Editor;
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use neoflash_ui::components::canvas::{self as cv, ShapeHintState};

#[derive(Debug)]
pub(crate) struct ShapeHintsState {
    pub show: bool,
    pub drag: Option<ShapeHintDrag>,
    pub hover: Option<char>,
}
impl Default for ShapeHintsState {
    fn default() -> Self {
        Self {
            show: true,
            drag: None,
            hover: None,
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
    geometry: HintGeometry,
}

/// The model's UVs refer to each morph contour's parent-space bounds. The first shape/contour
/// supplies the authoring handle, matching the first contour paired by the morph engine.
#[derive(Clone, Debug)]
struct HintGeometry {
    points: Vec<[f64; 2]>,
    low: [f64; 2],
    high: [f64; 2],
}
impl HintGeometry {
    fn from_key(key: &Keyframe) -> Option<Self> {
        key.elements.iter().find_map(|element| {
            let (commands, _) = element.to_path_commands()?;
            let mut contours = flatten_path(&commands, FlattenOptions::default()).ok()?;
            let contour = contours.first_mut()?;
            let m = transform_matrix(element.transform);
            let mut points: Vec<_> = contour
                .points
                .iter()
                .map(|p| {
                    [
                        m[0] * p[0] + m[2] * p[1] + m[4],
                        m[1] * p[0] + m[3] * p[1] + m[5],
                    ]
                })
                .collect();
            if contour.closed && points.first() != points.last() {
                points.push(*points.first()?);
            }
            if points.len() < 2 {
                return None;
            }
            let low = points
                .iter()
                .fold([f64::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
            let high = points.iter().fold([f64::NEG_INFINITY; 2], |a, p| {
                [a[0].max(p[0]), a[1].max(p[1])]
            });
            Some(Self { points, low, high })
        })
    }
    fn position(&self, uv: [f64; 2]) -> [f64; 2] {
        [
            self.low[0] + (self.high[0] - self.low[0]) * uv[0],
            self.low[1] + (self.high[1] - self.low[1]) * uv[1],
        ]
    }
    fn nearest(&self, p: [f64; 2]) -> [f64; 2] {
        self.points
            .windows(2)
            .map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let d = [b[0] - a[0], b[1] - a[1]];
                let length = d[0] * d[0] + d[1] * d[1];
                let t = if length > 0. {
                    (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0., 1.)
                } else {
                    0.
                };
                [a[0] + d[0] * t, a[1] + d[1] * t]
            })
            .min_by(|a, b| distance(*a, p).total_cmp(&distance(*b, p)))
            .unwrap_or(p)
    }
    fn placed(&self, uv: [f64; 2]) -> bool {
        let p = self.position(uv);
        distance(p, self.nearest(p)) <= 0.5
    }
    fn snapped_uv(&self, pointer: [f64; 2]) -> [f64; 2] {
        let p = self.nearest(pointer);
        std::array::from_fn(|axis| {
            let size = self.high[axis] - self.low[axis];
            if size > 1e-9 {
                ((p[axis] - self.low[axis]) / size).clamp(0., 1.)
            } else {
                0.5
            }
        })
    }
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[derive(Clone, Debug)]
pub(crate) struct ShapeHintMarker {
    pub label: char,
    pub position: [f64; 2],
    pub state: ShapeHintState,
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

    pub(crate) fn can_add_shape_hint(&self) -> bool {
        self.shape_hint_span().is_some_and(|(start, end)| {
            let keys = &self.symbol().timeline.layers[self.layer].keyframes;
            keys[start].shape_hints.len() < ShapeHint::MAX_HINTS
                && HintGeometry::from_key(&keys[start]).is_some()
                && HintGeometry::from_key(&keys[end]).is_some()
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
    }
    pub(crate) fn shape_hint_markers(&self) -> Vec<ShapeHintMarker> {
        if !self.timeline_shape_hints.show || self.playing {
            return vec![];
        }
        let Some((start, end)) = self.shape_hint_span() else {
            return vec![];
        };
        let keys = &self.symbol().timeline.layers[self.layer].keyframes;
        let at_end = self.frame == keys[end].frame;
        if !at_end && self.frame != keys[start].frame {
            return vec![];
        }
        let (Some(a), Some(b)) = (
            HintGeometry::from_key(&keys[start]),
            HintGeometry::from_key(&keys[end]),
        ) else {
            return vec![];
        };
        keys[start]
            .shape_hints
            .iter()
            .map(|hint| {
                let matched = a.placed(hint.start_uv) && b.placed(hint.end_uv);
                ShapeHintMarker {
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
                }
            })
            .collect()
    }
    pub(crate) fn begin_shape_hint_drag(&mut self, label: char) -> bool {
        if !self
            .shape_hint_markers()
            .iter()
            .any(|hint| hint.label == label)
        {
            return false;
        }
        let Some((start, end)) = self.shape_hint_span() else {
            return false;
        };
        let layer = &self.symbol().timeline.layers[self.layer];
        let at_end = self.frame == layer.keyframes[end].frame;
        let geometry =
            HintGeometry::from_key(&layer.keyframes[if at_end { end } else { start }]).unwrap();
        let drag = ShapeHintDrag {
            symbol: self.symbol_id().into(),
            layer: layer.id.clone(),
            start: layer.keyframes[start].frame,
            frame: self.frame,
            label,
            end: at_end,
            began: false,
            press: None,
            geometry,
        };
        self.finish_edits();
        self.playing = false;
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
        let to = drag.geometry.snapped_uv(pointer);
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
    ) {
        let zoom = self.zoom;
        let local = |p: Pos2| {
            [
                (p.x - origin.x) as f64 / zoom as f64,
                (p.y - origin.y) as f64 / zoom as f64,
            ]
        };
        let screen = |p: [f64; 2]| origin + egui::vec2(p[0] as f32, p[1] as f32) * zoom;
        let markers = self.shape_hint_markers();
        let hit = |pos| {
            markers
                .iter()
                .rev()
                .find(|h| cv::shape_hint_hit(screen(h.position), pos))
                .map(|h| h.label)
        };
        let rulers = cv::RulerLayout::new(canvas);
        let mut consumed = false;
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
                    if let Some(label) = hit(pos)
                        && self.begin_shape_hint_drag(label)
                    {
                        self.timeline_shape_hints.drag.as_mut().unwrap().press = Some(local(pos));
                        consumed = true;
                        self.stage_view.guide_pointer = true;
                    }
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
                    if moved && page.contains(pos) {
                        self.drag_shape_hint_to(local(pos));
                    }
                    self.finish_shape_hint_drag(moved && !page.contains(pos));
                    consumed = true;
                    self.stage_view.guide_pointer = true;
                }
                _ => {}
            }
        }
        if consumed {
            ui.ctx().input_mut(|i| {
                i.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::PointerButton {
                            button: egui::PointerButton::Primary,
                            ..
                        } | egui::Event::PointerMoved(_)
                    )
                })
            });
        }
        self.timeline_shape_hints.hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| {
                canvas.contains(*p) && !(self.stage_view.prefs.rulers && rulers.contains(*p))
            })
            .and_then(hit);
    }
    pub(crate) fn paint_shape_hints(&self, painter: &egui::Painter, origin: Pos2) {
        for hint in self.shape_hint_markers() {
            let at =
                origin + egui::vec2(hint.position[0] as f32, hint.position[1] as f32) * self.zoom;
            cv::paint_shape_hint(painter, painter.ctx(), at, hint.label, hint.state);
        }
    }
}
