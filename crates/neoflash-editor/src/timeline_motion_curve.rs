//! Editable Bézier motion paths. Geometry is document data; pointer drafts are session state.
use crate::{app::Editor, stage_mapping::StageMapping};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use neoflash_ui::components::canvas::{self as cv, MotionHandleState};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CurveTarget {
    Handle(usize, bool),
    Segment(usize),
}
#[derive(Clone, Debug)]
pub(crate) struct CurveDrag {
    symbol: Id,
    layer: Id,
    start: u32,
    element: Id,
    target: CurveTarget,
    press: [f64; 2],
    path: Vec<PathCommand>,
    /// Mapping captured by the stage pointer phase; direct command tests leave it unset.
    mapping: Option<StageMapping>,
    began: bool,
}
#[derive(Clone)]
struct Curve {
    layer: Id,
    start: u32,
    element: Id,
    path: Vec<PathCommand>,
    offset: [f64; 2],
}
fn endpoint(command: PathCommand) -> [f64; 2] {
    match command {
        PathCommand::MoveTo { x, y }
        | PathCommand::LineTo { x, y }
        | PathCommand::CubicTo { x, y, .. } => [x, y],
        _ => [0., 0.],
    }
}
fn cubic(path: &mut [PathCommand], index: usize) {
    if let PathCommand::LineTo { x, y } = path[index] {
        let [a, b] = endpoint(path[index - 1]);
        path[index] = PathCommand::CubicTo {
            cx1: a + (x - a) / 3.,
            cy1: b + (y - b) / 3.,
            cx2: a + (x - a) * 2. / 3.,
            cy2: b + (y - b) * 2. / 3.,
            x,
            y,
        };
    }
}
fn handle(path: &[PathCommand], index: usize, outgoing: bool) -> Option<([f64; 2], [f64; 2])> {
    if let PathCommand::CubicTo {
        cx1,
        cy1,
        cx2,
        cy2,
        x,
        y,
    } = *path.get(index)?
    {
        Some(if outgoing {
            (endpoint(path[index - 1]), [cx1, cy1])
        } else {
            ([x, y], [cx2, cy2])
        })
    } else {
        None
    }
}
fn set_handle(path: &mut [PathCommand], index: usize, outgoing: bool, at: [f64; 2]) {
    cubic(path, index);
    if let PathCommand::CubicTo {
        cx1, cy1, cx2, cy2, ..
    } = &mut path[index]
    {
        if outgoing {
            *cx1 = at[0];
            *cy1 = at[1];
        } else {
            *cx2 = at[0];
            *cy2 = at[1];
        }
    }
}
/// Translate an anchor and its attached handles while keeping other anchors untouched.
pub(crate) fn move_anchor(path: &mut [PathCommand], index: usize, delta: [f64; 2]) {
    match &mut path[index] {
        PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
            *x += delta[0];
            *y += delta[1];
        }
        PathCommand::CubicTo { cx2, cy2, x, y, .. } => {
            *cx2 += delta[0];
            *cy2 += delta[1];
            *x += delta[0];
            *y += delta[1];
        }
        _ => {}
    }
    if let Some(PathCommand::CubicTo { cx1, cy1, .. }) = path.get_mut(index + 1) {
        *cx1 += delta[0];
        *cy1 += delta[1];
    }
}
/// Preserve authored tangents when an existing position key moves. Explicit path edits own
/// their geometry, so they bypass this reconciliation. Inserting a positional key subdivides
/// the original segment rather than replacing the curve with straight lines.
pub(crate) fn rebase_motion_paths(before: &Project, project: &mut Project) {
    let fps = f64::from(project.fps);
    for symbol in &mut project.symbols {
        let Some(old_symbol) = before.symbol(&symbol.id) else {
            continue;
        };
        for layer in &mut symbol.timeline.layers {
            if let Some(old) = old_symbol
                .timeline
                .layers
                .iter()
                .find(|old| old.id == layer.id)
            {
                rebase_layer_motion_paths(old, layer, fps);
            }
        }
    }
}

pub(crate) fn rebase_layer_motion_paths(before: &Layer, layer: &mut Layer, fps: f64) {
    rebase_object_motion_paths(before, layer, fps);
    let starts: Vec<_> = layer
        .keyframes
        .iter()
        .filter_map(|k| k.motion_path.as_ref().map(|_| k.frame))
        .collect();
    for frame in starts {
        let Some(start_index) = layer.keyframes.iter().position(|key| key.frame == frame) else {
            continue;
        };
        let Some(old_start) = before.keyframes.iter().find(|k| k.frame == frame) else {
            continue;
        };
        let Some(mut path) = old_start.motion_path.clone() else {
            continue;
        };
        if layer.keyframes[start_index].motion_path.as_ref() != Some(&path) {
            continue;
        }
        let Some(reference) = old_start.elements.first() else {
            continue;
        };
        if layer.keyframes[start_index]
            .elements
            .first()
            .is_none_or(|e| e.id != reference.id)
        {
            reanchor_changed_reference(before, layer, start_index, reference, path, fps);
            continue;
        }
        let mut frames: Vec<_> = motion_path_waypoints(before, frame, &reference.id)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        let Some((start, properties, _)) = motion_span(layer, frame) else {
            continue;
        };
        let keys: Vec<_> = std::iter::once(start).chain(properties).collect();
        // Range edits may preserve spatial nodes after removing a temporal key. Follow the
        // existing evaluated point: reuse a matching node or split the retained segment exactly.
        if frames.len() != path.len() {
            for key in keys {
                let Some(to) = key.elements.iter().find(|e| e.id == reference.id) else {
                    continue;
                };
                let Some(old) = before
                    .keyframes
                    .iter()
                    .find(|k| k.frame == key.frame)
                    .and_then(|k| k.elements.iter().find(|e| e.id == reference.id))
                else {
                    continue;
                };
                let delta = [
                    to.transform.x - old.transform.x,
                    to.transform.y - old.transform.y,
                ];
                if delta[0].hypot(delta[1]) <= 1e-9 {
                    continue;
                }
                let Some(shown) = sample_layer_at_fps(before, key.frame, fps)
                    .into_iter()
                    .find(|e| e.id == reference.id)
                else {
                    continue;
                };
                let at = [shown.transform.x, shown.transform.y];
                let index = if key.frame == frame {
                    0
                } else if frames.last() == Some(&key.frame) {
                    path.len() - 1
                } else if let Some(index) = path.iter().position(|command| {
                    let p = endpoint(*command);
                    (p[0] - at[0]).hypot(p[1] - at[1]) < 0.1
                }) {
                    index
                } else {
                    let Some((segment, t, _, _)) = nearest_path_segment(&path, at) else {
                        continue;
                    };
                    let Some((split, index)) = subdivide_path_at(&path, segment, t) else {
                        continue;
                    };
                    path = split;
                    index
                };
                move_anchor(&mut path, index, delta);
            }
            layer.keyframes[start_index].motion_path = Some(path);
            continue;
        }
        for key in keys {
            let Some(to) = key.elements.iter().find(|e| e.id == reference.id) else {
                continue;
            };
            let old_pose = before
                .keyframes
                .iter()
                .find(|k| k.frame == key.frame)
                .and_then(|k| k.elements.iter().find(|e| e.id == reference.id))
                .map(|e| e.transform)
                .or_else(|| {
                    sample_layer_at_fps(before, key.frame, fps)
                        .into_iter()
                        .find(|e| e.id == reference.id)
                        .map(|e| e.transform)
                });
            let Some(old_pose) = old_pose else {
                continue;
            };
            let delta = [to.transform.x - old_pose.x, to.transform.y - old_pose.y];
            let changed = delta[0].hypot(delta[1]) > 1e-9;
            let positional = key
                .keyed_properties
                .as_ref()
                .is_some_and(|p| p.contains(&TweenProperty::X) || p.contains(&TweenProperty::Y));
            let index = if let Some(i) = frames.iter().position(|f| *f == key.frame) {
                i
            } else if changed || positional {
                let index = frames.partition_point(|f| *f < key.frame);
                if index == 0 {
                    continue;
                }
                if index == frames.len() {
                    let [x, y] = endpoint(*path.last().unwrap());
                    path.push(PathCommand::LineTo { x, y });
                } else {
                    let old_position = sample_layer_at_fps(before, key.frame, fps)
                        .into_iter()
                        .find(|e| e.id == reference.id)
                        .map(|e| [e.transform.x, e.transform.y])
                        .unwrap_or([old_pose.x, old_pose.y]);
                    let [x, y] = endpoint(path[index - 1]);
                    let Some((_, t, _, _)) = nearest_path_segment(
                        &[PathCommand::MoveTo { x, y }, path[index]],
                        old_position,
                    ) else {
                        continue;
                    };
                    let Some((split, _)) = subdivide_path_at(&path, index, t) else {
                        continue;
                    };
                    path = split;
                }
                frames.insert(index, key.frame);
                index
            } else {
                continue;
            };
            if changed {
                move_anchor(&mut path, index, delta);
            }
        }
        layer.keyframes[start_index].motion_path = Some(path);
    }
}

/// Reconcile format-10 paths by stable element id. Unlike the legacy path, object paths never
/// depend on element order, so deleting or reordering a neighbour cannot transfer their geometry.
fn rebase_object_motion_paths(before: &Layer, layer: &mut Layer, fps: f64) {
    let previous_pose = |frame: u32, element_id: &str| {
        before
            .keyframes
            .iter()
            .find(|key| key.frame == frame)
            .and_then(|key| key.elements.iter().find(|element| element.id == element_id))
            .map(|element| element.transform)
            .or_else(|| {
                sample_layer_at_fps(before, frame, fps)
                    .into_iter()
                    .find(|element| element.id == element_id)
                    .map(|element| element.transform)
            })
    };
    let starts: Vec<(u32, Id, Vec<PathCommand>)> = before
        .keyframes
        .iter()
        .flat_map(|key| {
            key.motion_tracks.iter().filter_map(move |track| {
                let MotionPathOverride::Path { commands } = track.motion_path.as_ref()? else {
                    return None;
                };
                Some((key.frame, track.element_id.clone(), commands.clone()))
            })
        })
        .collect();
    for (frame, element_id, mut path) in starts {
        let Some(start_index) = layer.keyframes.iter().position(|key| key.frame == frame) else {
            continue;
        };
        let unchanged = layer.keyframes[start_index]
            .motion_track(&element_id)
            .and_then(|track| track.motion_path.as_ref())
            .is_some_and(|current| {
                matches!(current, MotionPathOverride::Path { commands } if commands == &path)
            });
        if !unchanged {
            continue;
        }
        let mut frames: Vec<u32> = motion_path_waypoints(before, frame, &element_id)
            .into_iter()
            .map(|(key_frame, _)| key_frame)
            .collect();
        let Some((start, properties, _)) = motion_span(layer, frame) else {
            continue;
        };
        let keys: Vec<Keyframe> = std::iter::once(start)
            .chain(properties)
            .filter(|key| {
                let already_keys_position = key
                    .motion_key_properties_for(&element_id)
                    .iter()
                    .any(|property| matches!(property, TweenProperty::X | TweenProperty::Y));
                let position_changed = key
                    .elements
                    .iter()
                    .find(|element| element.id == element_id)
                    .zip(previous_pose(key.frame, &element_id))
                    .is_some_and(|(element, previous)| {
                        (element.transform.x - previous.x).hypot(element.transform.y - previous.y)
                            > 1e-9
                    });
                key.frame == frame || already_keys_position || position_changed
            })
            .cloned()
            .collect();
        if frames.len() != path.len() {
            for key in &keys {
                let Some(to) = key.elements.iter().find(|element| element.id == element_id) else {
                    continue;
                };
                let Some(old_pose) = previous_pose(key.frame, &element_id) else {
                    continue;
                };
                let delta = [to.transform.x - old_pose.x, to.transform.y - old_pose.y];
                if delta[0].hypot(delta[1]) <= 1e-9 {
                    continue;
                }
                let Some(shown) = sample_layer_at_fps(before, key.frame, fps)
                    .into_iter()
                    .find(|element| element.id == element_id)
                else {
                    continue;
                };
                let at = [shown.transform.x, shown.transform.y];
                let index = if key.frame == frame {
                    0
                } else if frames.last() == Some(&key.frame) {
                    path.len() - 1
                } else if let Some(index) = path.iter().position(|command| {
                    let point = endpoint(*command);
                    (point[0] - at[0]).hypot(point[1] - at[1]) < 0.1
                }) {
                    index
                } else {
                    let Some((segment, t, _, _)) = nearest_path_segment(&path, at) else {
                        continue;
                    };
                    let Some((split, index)) = subdivide_path_at(&path, segment, t) else {
                        continue;
                    };
                    path = split;
                    index
                };
                move_anchor(&mut path, index, delta);
            }
            if let Some(track) = layer.keyframes[start_index].motion_track_mut(&element_id) {
                track.motion_path = Some(MotionPathOverride::Path { commands: path });
            }
            continue;
        }
        for key in keys {
            let Some(to) = key.elements.iter().find(|element| element.id == element_id) else {
                continue;
            };
            let Some(old_pose) = previous_pose(key.frame, &element_id) else {
                continue;
            };
            let delta = [to.transform.x - old_pose.x, to.transform.y - old_pose.y];
            let changed = delta[0].hypot(delta[1]) > 1e-9;
            let index = if let Some(index) = frames.iter().position(|at| *at == key.frame) {
                index
            } else if changed {
                let index = frames.partition_point(|at| *at < key.frame);
                if index == 0 {
                    continue;
                }
                if index == frames.len() {
                    let [x, y] = endpoint(*path.last().unwrap());
                    path.push(PathCommand::LineTo { x, y });
                } else {
                    let old_position = sample_layer_at_fps(before, key.frame, fps)
                        .into_iter()
                        .find(|element| element.id == element_id)
                        .map(|element| [element.transform.x, element.transform.y])
                        .unwrap_or([old_pose.x, old_pose.y]);
                    let [x, y] = endpoint(path[index - 1]);
                    let Some((_, t, _, _)) = nearest_path_segment(
                        &[PathCommand::MoveTo { x, y }, path[index]],
                        old_position,
                    ) else {
                        continue;
                    };
                    let Some((split, _)) = subdivide_path_at(&path, index, t) else {
                        continue;
                    };
                    path = split;
                }
                frames.insert(index, key.frame);
                index
            } else {
                continue;
            };
            if changed {
                move_anchor(&mut path, index, delta);
            }
        }
        if let Some(track) = layer.keyframes[start_index].motion_track_mut(&element_id) {
            track.motion_path = Some(MotionPathOverride::Path { commands: path });
        }
    }
}

/// The path's reference is implicit in its start element. Reordering/deleting that element
/// must not move surviving artwork. Parallel tracks can retain the editable translated curve;
/// independent tracks need exact frame poses, sampled with the original reference restored.
fn reanchor_changed_reference(
    before: &Layer,
    layer: &mut Layer,
    start_index: usize,
    reference: &Element,
    mut path: Vec<PathCommand>,
    fps: f64,
) {
    let start = &layer.keyframes[start_index];
    let Some(next_reference) = start.elements.first() else {
        layer.keyframes[start_index].motion_path = None;
        return;
    };
    let frame = start.frame;
    let next_id = next_reference.id.clone();
    let order: Vec<_> = start
        .elements
        .iter()
        .map(|element| element.id.clone())
        .collect();
    let Some((_, properties, end)) = motion_span(layer, frame) else {
        return;
    };
    let end = end.unwrap_or_else(|| properties.last().map_or(frame + 1, |key| key.frame + 1));
    let mut sampling = layer.clone();
    let mut uncurved = before.clone();
    for key in &mut uncurved.keyframes {
        key.motion_path = None;
        key.orient_to_path = false;
        if key.tween.is_some() {
            key.tween = Some(Easing::Linear);
        }
        key.property_eases.clear();
    }
    for key in sampling
        .keyframes
        .iter_mut()
        .filter(|key| key.frame >= frame && key.frame < end)
    {
        let restored = if let Some(index) = key
            .elements
            .iter()
            .position(|element| element.id == reference.id)
        {
            key.elements.remove(index)
        } else {
            before
                .keyframes
                .iter()
                .find(|old| old.frame == key.frame)
                .and_then(|old| {
                    old.elements
                        .iter()
                        .find(|element| element.id == reference.id)
                })
                .cloned()
                .or_else(|| {
                    sample_layer_at_fps(&uncurved, key.frame, fps)
                        .into_iter()
                        .find(|element| element.id == reference.id)
                })
                .unwrap_or_else(|| reference.clone())
        };
        key.elements.insert(0, restored);
    }
    let a = motion_path_waypoints(&sampling, frame, &reference.id);
    let b = motion_path_waypoints(&sampling, frame, &next_id);
    let delta = [b[0].1[0] - a[0].1[0], b[0].1[1] - a[0].1[1]];
    if a.iter().zip(&b).all(|((_, a), (_, b))| {
        (b[0] - a[0] - delta[0]).abs() < 1e-6 && (b[1] - a[1] - delta[1]).abs() < 1e-6
    }) {
        for command in &mut path {
            match command {
                PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                    *x += delta[0];
                    *y += delta[1];
                }
                PathCommand::CubicTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => {
                    *cx1 += delta[0];
                    *cy1 += delta[1];
                    *cx2 += delta[0];
                    *cy2 += delta[1];
                    *x += delta[0];
                    *y += delta[1];
                }
                _ => {}
            }
        }
        layer.keyframes[start_index].motion_path = Some(path);
        return;
    }
    let replacement: Vec<_> = (frame..end)
        .map(|at| {
            let mut elements = sample_layer_at_fps(&sampling, at, fps);
            elements.retain(|element| order.contains(&element.id));
            elements.sort_by_key(|element| order.iter().position(|id| id == &element.id));
            let metadata = layer.keyframes.iter().find(|key| key.frame == at);
            Keyframe {
                frame: at,
                elements,
                label: metadata.map_or_else(String::new, |key| key.label.clone()),
                sound: metadata.and_then(|key| key.sound.clone()),
                ..Default::default()
            }
        })
        .collect();
    layer
        .keyframes
        .retain(|key| key.frame < frame || key.frame >= end);
    layer.keyframes.extend(replacement);
    layer.keyframes.sort_by_key(|key| key.frame);
}

impl Editor {
    fn editable_motion_curve(&self) -> Option<Curve> {
        let visible = self.motion_path()?;
        let layer = &self.symbol().timeline.layers[visible.layer];
        if layer.properties.motion_guide.is_some() {
            return None;
        }
        let (start, _, _) = motion_span(layer, self.frame)?;
        let selected = start
            .elements
            .iter()
            .find(|element| element.id == visible.element)?;
        let waypoints = motion_path_waypoints(layer, self.frame, &selected.id);
        if waypoints.len() < 2 {
            return None;
        }
        let straight_path = || {
            waypoints
                .iter()
                .enumerate()
                .map(|(index, (_, point))| {
                    if index == 0 {
                        PathCommand::MoveTo {
                            x: point[0],
                            y: point[1],
                        }
                    } else {
                        PathCommand::LineTo {
                            x: point[0],
                            y: point[1],
                        }
                    }
                })
                .collect()
        };
        let path = match start.motion_path_for(&selected.id) {
            MotionPathSource::Object(path) => path.to_vec(),
            MotionPathSource::Legacy(path) => {
                let first = start.elements.first()?;
                if first.id == selected.id {
                    path.to_vec()
                } else {
                    let reference = motion_path_waypoints(layer, self.frame, &first.id);
                    let offset = [
                        selected.transform.x - first.transform.x,
                        selected.transform.y - first.transform.y,
                    ];
                    if reference.len() == waypoints.len()
                        && reference
                            .iter()
                            .zip(&waypoints)
                            .all(|((_, from), (_, to))| {
                                (to[0] - from[0] - offset[0]).abs() <= 1e-6
                                    && (to[1] - from[1] - offset[1]).abs() <= 1e-6
                            })
                    {
                        crate::timeline_motion::translate_motion_path(path, offset)
                    } else {
                        straight_path()
                    }
                }
            }
            MotionPathSource::None => straight_path(),
        };
        Some(Curve {
            layer: layer.id.clone(),
            start: start.frame,
            element: selected.id.clone(),
            path,
            offset: [0.0, 0.0],
        })
    }
    fn selected_curve_anchor(&self, curve: &Curve) -> usize {
        self.timeline_motion
            .curve_selected
            .unwrap_or_else(|| {
                let layer = self
                    .symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == curve.layer)
                    .unwrap();
                let points = motion_path_waypoints(layer, self.frame, &curve.element);
                let selected_frame = points
                    .iter()
                    .min_by_key(|(f, _)| f.abs_diff(self.frame))
                    .map_or(curve.start, |(f, _)| *f);
                let position =
                    sample_layer_at_fps(layer, selected_frame, f64::from(self.project.fps))
                        .into_iter()
                        .find(|e| e.id == curve.element)
                        .map_or(endpoint(curve.path[0]), |e| [e.transform.x, e.transform.y]);
                curve
                    .path
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let a = endpoint(**a);
                        let b = endpoint(**b);
                        (a[0] - position[0])
                            .hypot(a[1] - position[1])
                            .total_cmp(&(b[0] - position[0]).hypot(b[1] - position[1]))
                    })
                    .map_or(0, |(index, _)| index)
            })
            .min(curve.path.len() - 1)
    }
    fn shown_handles(&self, curve: &Curve) -> Vec<(CurveTarget, [f64; 2], [f64; 2])> {
        let selected = self.selected_curve_anchor(curve);
        let mut result = vec![];
        for index in 1..curve.path.len() {
            if index == selected || index == selected + 1 {
                for outgoing in [true, false] {
                    if let Some((anchor, at)) = handle(&curve.path, index, outgoing) {
                        result.push((CurveTarget::Handle(index, outgoing), anchor, at));
                    }
                }
            }
        }
        result
    }
    pub(crate) fn paint_motion_handles(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        let Some(curve) = self.editable_motion_curve() else {
            return;
        };
        let screen = |p: [f64; 2]| {
            mapping.edit_to_screen(
                origin,
                self.zoom,
                Pos2::new(
                    (p[0] + curve.offset[0]) as f32,
                    (p[1] + curve.offset[1]) as f32,
                ),
            )
        };
        for (target, anchor, at) in self.shown_handles(&curve) {
            let state = if self
                .timeline_motion
                .curve_drag
                .as_ref()
                .is_some_and(|d| d.target == target)
            {
                MotionHandleState::Drag
            } else if self.timeline_motion.curve_hover == Some(target) {
                MotionHandleState::Hover
            } else {
                MotionHandleState::Default
            };
            cv::paint_motion_handle(painter, painter.ctx(), screen(anchor), screen(at), state);
        }
    }
    pub(crate) fn begin_motion_curve_drag(&mut self, target: CurveTarget, press: [f64; 2]) -> bool {
        if !self.motion_paths_interactive() {
            return false;
        }
        let Some(curve) = self.editable_motion_curve() else {
            return false;
        };
        self.finish_edits();
        self.playing = false;
        self.timeline_motion.curve_selected = Some(match target {
            CurveTarget::Handle(i, true) => i - 1,
            CurveTarget::Handle(i, false) | CurveTarget::Segment(i) => i,
        });
        self.timeline_motion.curve_drag = Some(CurveDrag {
            symbol: self.symbol_id().into(),
            layer: curve.layer,
            start: curve.start,
            element: curve.element,
            target,
            press,
            path: curve.path,
            mapping: None,
            began: false,
        });
        true
    }
    pub(crate) fn drag_motion_curve_to(&mut self, pointer: [f64; 2], corner: bool) {
        let Some(drag) = self.timeline_motion.curve_drag.clone() else {
            return;
        };
        if drag.symbol != self.symbol_id() {
            self.cancel_motion_drags();
            return;
        }
        let delta = [pointer[0] - drag.press[0], pointer[1] - drag.press[1]];
        if !drag.began && delta[0].hypot(delta[1]) < 1e-8 {
            return;
        }
        let mut path = drag.path.clone();
        match drag.target {
            CurveTarget::Handle(index, outgoing) => {
                cubic(&mut path, index);
                let (anchor, at) = handle(&path, index, outgoing).unwrap();
                let to = [at[0] + delta[0], at[1] + delta[1]];
                set_handle(&mut path, index, outgoing, to);
                if !corner {
                    let other = if outgoing {
                        index.checked_sub(1).filter(|i| *i > 0).map(|i| (i, false))
                    } else {
                        (index + 1 < path.len()).then_some((index + 1, true))
                    };
                    if let Some((i, out)) = other {
                        set_handle(
                            &mut path,
                            i,
                            out,
                            [2. * anchor[0] - to[0], 2. * anchor[1] - to[1]],
                        );
                    }
                }
            }
            CurveTarget::Segment(index) => {
                cubic(&mut path, index);
                if let PathCommand::CubicTo {
                    cx1, cy1, cx2, cy2, ..
                } = &mut path[index]
                {
                    *cx1 += delta[0] * 4. / 3.;
                    *cy1 += delta[1] * 4. / 3.;
                    *cx2 += delta[0] * 4. / 3.;
                    *cy2 += delta[1] * 4. / 3.;
                }
            }
        }
        if !drag.began {
            self.history.begin(&self.project);
            self.timeline_motion.curve_drag.as_mut().unwrap().began = true;
        }
        if let Some(key) = self
            .symbol_mut()
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == drag.layer)
            .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == drag.start))
        {
            key.ensure_motion_track(&drag.element).motion_path =
                Some(MotionPathOverride::Path { commands: path });
        }
    }
    pub(crate) fn finish_motion_curve_drag(&mut self) {
        if self
            .timeline_motion
            .curve_drag
            .take()
            .is_some_and(|d| d.began)
        {
            self.commit_history();
            self.reset_authored_preview();
        }
    }
    pub(crate) fn cancel_motion_drags(&mut self) {
        let curve = self
            .timeline_motion
            .curve_drag
            .take()
            .is_some_and(|d| d.began);
        let key = self.timeline_motion.drag.take().is_some_and(|d| d.began);
        if curve || key {
            self.history.cancel(&mut self.project);
            self.reset_authored_preview();
        }
    }
    fn toggle_curve_corner(&mut self, index: usize) {
        let Some(mut curve) = self.editable_motion_curve() else {
            return;
        };
        self.finish_edits();
        let anchor = endpoint(curve.path[index]);
        let before = index.checked_sub(1).map(|i| endpoint(curve.path[i]));
        let after = curve.path.get(index + 1).copied().map(endpoint);
        let handles = [(index, false), (index + 1, true)];
        let has_handles = handles
            .iter()
            .filter_map(|&(i, out)| handle(&curve.path, i, out))
            .any(|(_, p)| (p[0] - anchor[0]).hypot(p[1] - anchor[1]) > 1e-8);
        let tangent = match (before, after) {
            (Some(a), Some(b)) => [(b[0] - a[0]) / 6., (b[1] - a[1]) / 6.],
            (None, Some(b)) => [(b[0] - anchor[0]) / 3., (b[1] - anchor[1]) / 3.],
            (Some(a), None) => [(anchor[0] - a[0]) / 3., (anchor[1] - a[1]) / 3.],
            _ => [0., 0.],
        };
        for (i, out) in handles {
            if i > 0 && i < curve.path.len() {
                let direction = if out { 1. } else { -1. };
                set_handle(
                    &mut curve.path,
                    i,
                    out,
                    if has_handles {
                        anchor
                    } else {
                        [
                            anchor[0] + direction * tangent[0],
                            anchor[1] + direction * tangent[1],
                        ]
                    },
                );
            }
        }
        self.timeline_motion.curve_selected = Some(index);
        self.transact_named("Change motion path tangent", |s| {
            if let Some(key) = s
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == curve.layer)
                .and_then(|l| l.keyframes.iter_mut().find(|k| k.frame == curve.start))
            {
                key.ensure_motion_track(&curve.element).motion_path =
                    Some(MotionPathOverride::Path {
                        commands: curve.path,
                    });
            }
        });
    }
    pub(crate) fn motion_curve_input(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        self.timeline_motion.curve_hover = None;
        if self
            .timeline_motion
            .curve_drag
            .as_ref()
            .and_then(|drag| drag.mapping)
            .is_some_and(|captured| captured != mapping)
        {
            self.cancel_motion_drags();
        }
        if !self.motion_paths_interactive() {
            if self.timeline_motion.curve_drag.is_some() {
                self.cancel_motion_drags();
            }
            return;
        }
        let Some(curve) = self.editable_motion_curve() else {
            if self.timeline_motion.curve_drag.is_some() {
                self.cancel_motion_drags();
            }
            return;
        };
        let zoom = self.zoom;
        let screen = |p: [f64; 2]| {
            mapping.edit_to_screen(
                origin,
                zoom,
                Pos2::new(
                    (p[0] + curve.offset[0]) as f32,
                    (p[1] + curve.offset[1]) as f32,
                ),
            )
        };
        let local = |p: Pos2| {
            let point = mapping.screen_to_edit(origin, zoom, p);
            [f64::from(point.x), f64::from(point.y)]
        };
        let handles = self.shown_handles(&curve);
        let anchor_hit = |pos: Pos2| {
            curve
                .path
                .iter()
                .enumerate()
                .find(|(_, c)| cv::motion_key_hit(screen(endpoint(**c)), pos))
                .map(|(i, _)| i)
        };
        let hit = |pos: Pos2| {
            handles
                .iter()
                .rev()
                .find(|(_, _, at)| cv::motion_handle_hit(screen(*at), pos))
                .map(|(t, _, _)| *t)
                .or_else(|| {
                    if anchor_hit(pos).is_some() {
                        return None;
                    }
                    (1..curve.path.len())
                        .find(|&i| {
                            motion_path_segment(endpoint(curve.path[i - 1]), curve.path[i])
                                .is_some_and(|path| {
                                    let at = local(pos);
                                    let at = [at[0] - curve.offset[0], at[1] - curve.offset[1]];
                                    let near = path.point_at(path.nearest(at));
                                    screen(near).distance(pos) <= 4.5
                                })
                        })
                        .map(CurveTarget::Segment)
                })
        };
        let rulers = self.stage_view.prefs.rulers;
        let layout = cv::RulerLayout::new(canvas);
        let mut taken = None;
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                } if canvas.contains(pos)
                    && !(rulers && layout.contains(pos))
                    && self.timeline_motion.drag.is_none()
                    && self.timeline_motion.curve_drag.is_none()
                    && ui.ctx().layer_id_at(pos).is_none_or(|l| l == ui.layer_id()) =>
                {
                    if modifiers.alt
                        && let Some(index) = anchor_hit(pos)
                    {
                        self.toggle_curve_corner(index);
                        taken = Some(pos);
                    } else if let Some(target) = hit(pos)
                        && self.begin_motion_curve_drag(target, local(pos))
                    {
                        self.timeline_motion.curve_drag.as_mut().unwrap().mapping = Some(mapping);
                        taken = Some(pos);
                    } else if let Some(index) = anchor_hit(pos) {
                        self.timeline_motion.curve_selected = Some(index);
                    }
                }
                egui::Event::PointerMoved(pos) if self.timeline_motion.curve_drag.is_some() => {
                    self.drag_motion_curve_to(local(pos), ui.input(|i| i.modifiers.alt))
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                } if self.timeline_motion.curve_drag.is_some() => {
                    self.drag_motion_curve_to(local(pos), modifiers.alt);
                    self.finish_motion_curve_drag();
                }
                _ => {}
            }
        }
        if let Some(pos) = taken {
            self.stage_view.guide_pointer = true;
            ui.ctx().input_mut(|i| {
                i.events.retain(
                    |e| !matches!(e,egui::Event::PointerButton{pos:p,pressed:true,..}if *p==pos),
                )
            });
        }
        self.timeline_motion.curve_hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| canvas.contains(*p))
            .and_then(hit);
    }
}
