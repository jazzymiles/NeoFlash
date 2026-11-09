//! Motion on the stage and between objects: the motion tween path of the selected instance with
//! draggable key points (`NF2/Motion Path`, R-005), and Copy motion / Paste motion (frame menu).
//! Every document change is one Undo step through `Editor::commit_history`.
use crate::{
    app::{Editor, Tool},
    stage_mapping::StageMapping,
};
use eframe::egui::{self, Pos2, Rect};
use neoflash_core::*;
use neoflash_ui::components::canvas::{self as cv, MotionKeyState};

/// Session state: the motion clipboard and the key point drag on the stage.
#[derive(Clone, Debug, Default)]
pub(crate) struct MotionState {
    pub clipboard: Option<MotionClip>,
    pub drag: Option<MotionKeyDrag>,
    /// Frame of the key point under the pointer (Hover state).
    pub hover: Option<u32>,
    pub curve_drag: Option<crate::timeline_motion_curve::CurveDrag>,
    pub curve_hover: Option<crate::timeline_motion_curve::CurveTarget>,
    pub curve_selected: Option<usize>,
    pub paste_special_open: bool,
    pub paste_options: PasteMotionOptions,
}

/// Session choices from the approved Paste motion special dialog (R-006).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PasteMotionOptions {
    pub properties: [bool; 9],
    pub preserve_scale: bool,
    pub preserve_rotation: bool,
}
impl Default for PasteMotionOptions {
    fn default() -> Self {
        Self {
            properties: [true; 9],
            preserve_scale: false,
            preserve_rotation: false,
        }
    }
}
impl PasteMotionOptions {
    pub fn any(&self) -> bool {
        self.properties.iter().any(|v| *v)
    }
}

/// One copied tween: its kind and ease, and the poses of one object at the span keys.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MotionClip {
    kind: TweenKind,
    ease: Easing,
    property_eases: Vec<(TweenProperty, Easing)>,
    orient_to_path: bool,
    snap_to_guide: bool,
    sync_symbols: bool,
    /// Object-coordinate editable path. A legacy secondary track with a changing offset is baked
    /// before paste instead of pretending its shared reference curve is independent.
    motion_path: Option<Vec<PathCommand>>,
    /// Frames from the span start to its end key (classic) or span end (motion).
    length: u32,
    /// Offset 0 first, then ◆ property keys in frame order (motion) or the end key (classic).
    keys: Vec<MotionKey>,
    /// Pose shown on the last frame of a motion span (held after it).
    hold: Transform,
    source: Timeline,
    source_fps: f64,
    source_layer: usize,
    source_start: u32,
    source_element: Id,
}

#[derive(Clone, Debug, PartialEq)]
struct MotionKey {
    offset: u32,
    keyed: Option<Vec<TweenProperty>>,
    roving: bool,
    pose: Transform,
}

/// A key point of the motion path being dragged on the stage.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MotionKeyDrag {
    symbol: Id,
    layer: Id,
    element: Id,
    frame: u32,
    /// Key position before the drag and pointer press, in timeline coordinates.
    start: [f64; 2],
    press: [f64; 2],
    /// View transform captured by the stage pointer phase; direct command calls leave it unset.
    mapping: Option<StageMapping>,
    zoom: Option<f32>,
    pub(crate) began: bool,
}

/// The motion tween path of the selected object in timeline coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MotionPath {
    pub layer: usize,
    pub element: Id,
    /// Registration point on every frame of the span.
    pub frames: Vec<[f64; 2]>,
    /// The span start key and its ◆ property keys: frame and position.
    pub keys: Vec<(u32, [f64; 2])>,
}

/// `pose` re-expressed relative to `target`: positions, rotation and skew move by the source's
/// change from `base`; scale and alpha scale by it; a changed colour effect is copied.
fn relative(base: &Transform, pose: &Transform, target: &Transform) -> Transform {
    let ratio = |value: f64, from: f64, to: f64| {
        if from.abs() > 1e-9 {
            to * value / from
        } else {
            value
        }
    };
    Transform {
        x: target.x + pose.x - base.x,
        y: target.y + pose.y - base.y,
        rotation: target.rotation + pose.rotation - base.rotation,
        skew: target.skew + pose.skew - base.skew,
        scale_x: ratio(pose.scale_x, base.scale_x, target.scale_x),
        scale_y: ratio(pose.scale_y, base.scale_y, target.scale_y),
        alpha: ratio(pose.alpha, base.alpha, target.alpha).clamp(0., 1.),
        color_transform: if pose.color_transform == base.color_transform {
            target.color_transform
        } else {
            pose.color_transform
        },
        ..*target
    }
}

/// Layer content at `frame` as a new keyframe (labels and scripts stay where they are).
fn sampled_key(timeline: &Timeline, layer: &Layer, frame: u32, fps: f64) -> Keyframe {
    Keyframe {
        frame,
        elements: sample_layer_in_at_fps(timeline, layer, frame, fps),
        ..Default::default()
    }
}

/// Set `id`'s pose on `key`, adding a copy of `template` when the object is not on that key.
fn place(key: &mut Keyframe, template: &Element, pose: Transform) {
    match key.elements.iter_mut().find(|e| e.id == template.id) {
        Some(element) => element.transform = pose,
        None => key.elements.push(Element {
            transform: pose,
            ..template.clone()
        }),
    }
}

fn copy_motion_properties(
    target: &mut Transform,
    source: &Transform,
    properties: &[TweenProperty],
) {
    for property in properties {
        match property {
            TweenProperty::X => target.x = source.x,
            TweenProperty::Y => target.y = source.y,
            TweenProperty::ScaleX => target.scale_x = source.scale_x,
            TweenProperty::ScaleY => target.scale_y = source.scale_y,
            TweenProperty::Rotation => target.rotation = source.rotation,
            TweenProperty::Skew => target.skew = source.skew,
            TweenProperty::Alpha => target.alpha = source.alpha,
            TweenProperty::Color => target.color_transform = source.color_transform,
        }
    }
}

pub(crate) fn translate_motion_path(path: &[PathCommand], delta: [f64; 2]) -> Vec<PathCommand> {
    let mut translated = path.to_vec();
    for command in &mut translated {
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
    translated
}

fn remove_object_motion_key(key: &mut Keyframe, element_id: &str) {
    if key.keyed_properties.is_none() {
        return;
    }
    key.materialize_motion_keys();
    if let Some(keys) = &mut key.motion_keys {
        keys.retain(|object_key| object_key.element_id != element_id);
    }
    key.sync_motion_key_union();
}

fn remove_object_motion_properties(
    key: &mut Keyframe,
    element_id: &str,
    properties: &[TweenProperty],
) {
    if key.keyed_properties.is_none() {
        return;
    }
    key.materialize_motion_keys();
    if let Some(keys) = &mut key.motion_keys {
        if let Some(object_key) = keys.iter_mut().find(|key| key.element_id == element_id) {
            object_key
                .properties
                .retain(|property| !properties.contains(property));
        }
        keys.retain(|key| !key.properties.is_empty());
    }
    key.sync_motion_key_union();
}

fn ensure_object_motion_key<'a>(
    layer: &'a mut Layer,
    snapshot: &Timeline,
    original: &Layer,
    frame: u32,
    fps: f64,
    easing: Easing,
) -> &'a mut Keyframe {
    if let Some(index) = layer.keyframes.iter().position(|key| key.frame == frame) {
        return &mut layer.keyframes[index];
    }
    let mut key = sampled_key(snapshot, original, frame, fps);
    key.tween = Some(easing);
    key.tween_kind = TweenKind::Motion;
    key.keyed_properties = Some(Vec::new());
    key.motion_keys = Some(Vec::new());
    layer.keyframes.push(key);
    layer.keyframes.sort_by_key(|key| key.frame);
    let index = layer
        .keyframes
        .iter()
        .position(|key| key.frame == frame)
        .unwrap();
    &mut layer.keyframes[index]
}

impl Editor {
    /// The selected object on the current timeline: layer index and element id.
    fn selected_object(&self) -> Option<(usize, Id)> {
        if self.selection.symbol != self.symbol_id() {
            return None;
        }
        let target = self.selection.single()?;
        Some((self.target_layer_index(target)?, target.element.clone()))
    }

    // ── Copy / Paste motion ──

    /// The tween under the playhead on the current layer, recorded for one object.
    fn motion_under_playhead(&self) -> Result<MotionClip, &'static str> {
        let timeline = &self.symbol().timeline;
        let layer = timeline
            .layers
            .get(self.layer)
            .ok_or("Select a tweened frame first.")?;
        let selected = self
            .selected_object()
            .filter(|(index, _)| *index == self.layer)
            .map(|(_, id)| id)
            .ok_or("Select exactly one object on the tweened layer to copy its motion.")?;
        let pose = |key: &Keyframe, id: &str| {
            key.elements
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.transform)
        };
        if let Some((start, properties, end)) = motion_span(layer, self.frame) {
            let id = selected.clone();
            if !start.elements.iter().any(|element| element.id == id) {
                return Err("The selected object is not part of this motion tween.");
            }
            let last = properties.last().map_or(start.frame, |k| k.frame);
            let end = end
                .unwrap_or(timeline.frame_count)
                .max(last + 1)
                .max(start.frame + 1);
            let mut keys = vec![MotionKey {
                offset: 0,
                keyed: None,
                roving: false,
                pose: pose(start, &id).unwrap(),
            }];
            for key in properties {
                let keyed = key.motion_key_properties_for(&id);
                if !keyed.is_empty()
                    && let Some(pose) = pose(key, &id)
                {
                    keys.push(MotionKey {
                        offset: key.frame - start.frame,
                        keyed: Some(keyed.to_vec()),
                        roving: key.motion_key_roving_for(&id),
                        pose,
                    });
                }
            }
            let hold =
                sample_layer_in_at_fps(timeline, layer, end - 1, f64::from(self.project.fps))
                    .into_iter()
                    .find(|e| e.id == id)
                    .map_or(keys[0].pose, |e| e.transform);
            return Ok(MotionClip {
                kind: TweenKind::Motion,
                ease: start.motion_easing_for(&id),
                property_eases: start
                    .motion_track(&id)
                    .and_then(|track| track.property_eases.clone())
                    .unwrap_or_else(|| start.property_eases.clone()),
                orient_to_path: start.motion_orient_to_path_for(&id),
                snap_to_guide: start.motion_snap_to_guide_for(&id),
                sync_symbols: start.motion_sync_symbols_for(&id),
                motion_path: match start.motion_path_for(&id) {
                    MotionPathSource::Object(path) => Some(path.to_vec()),
                    MotionPathSource::Legacy(path)
                        if start
                            .elements
                            .first()
                            .is_some_and(|element| element.id == id) =>
                    {
                        Some(path.to_vec())
                    }
                    MotionPathSource::None | MotionPathSource::Legacy(_) => None,
                },
                length: end - start.frame,
                keys,
                hold,
                source: timeline.clone(),
                source_fps: f64::from(self.project.fps),
                source_layer: self.layer,
                source_start: start.frame,
                source_element: id,
            });
        }
        let key = layer
            .key_at(self.frame)
            .ok_or("Select a frame inside a classic or motion tween.")?;
        let ease = key
            .tween
            .ok_or("Copy motion copies a classic or motion tween. The frame under the playhead is not tweened.")?;
        if key.tween_kind == TweenKind::Shape {
            return Err(
                "Copy motion copies classic and motion tweens; a shape tween morphs shapes instead of moving an object.",
            );
        }
        let next = layer
            .keyframes
            .iter()
            .filter(|k| k.frame > key.frame)
            .min_by_key(|k| k.frame)
            .ok_or("The tween has no end keyframe.")?;
        let id = selected;
        if !key.elements.iter().any(|element| element.id == id) {
            return Err("The selected object is not part of this tween.");
        }
        let (Some(first), Some(last)) = (pose(key, &id), pose(next, &id)) else {
            return Err("The tweened object is not on the end keyframe.");
        };
        Ok(MotionClip {
            kind: TweenKind::Classic,
            ease,
            property_eases: vec![],
            orient_to_path: key.orient_to_path,
            snap_to_guide: key.snap_to_guide,
            sync_symbols: key.sync_symbols,
            motion_path: None,
            length: next.frame - key.frame,
            keys: vec![
                MotionKey {
                    offset: 0,
                    keyed: None,
                    roving: false,
                    pose: first,
                },
                MotionKey {
                    offset: next.frame - key.frame,
                    keyed: None,
                    roving: false,
                    pose: last,
                },
            ],
            hold: last,
            source: timeline.clone(),
            source_fps: f64::from(self.project.fps),
            source_layer: self.layer,
            source_start: key.frame,
            source_element: id,
        })
    }

    pub(crate) fn can_copy_motion(&self) -> bool {
        self.motion_under_playhead().is_ok()
    }

    /// Copy motion: remember the tween under the playhead (kind, ease and the object's poses at its
    /// keys) for Paste motion.
    pub(crate) fn copy_motion(&mut self) {
        match self.motion_under_playhead() {
            Ok(clip) => {
                self.timeline_motion.clipboard = Some(clip);
                self.message("Motion copied. Select an object and choose Paste motion to animate it the same way.");
            }
            Err(error) => self.message(error),
        }
    }

    pub(crate) fn can_paste_motion(&self) -> bool {
        self.timeline_motion.clipboard.is_some()
            && self.selected_object().is_some_and(|(index, _)| {
                self.selection
                    .single()
                    .is_some_and(|target| self.target_editable(target))
                    && self.symbol().timeline.layers[index].properties.role == LayerRole::Normal
            })
    }

    /// Paste motion: animate the selected object from the playhead the way the copied tween moved
    /// its object, relative to the selected object's own pose. Keys inside the pasted span are
    /// replaced; the rest of the layer keeps its content. One Undo step.
    pub(crate) fn paste_motion(&mut self) {
        self.finish_edits();
        let Some(mut clip) = self.timeline_motion.clipboard.clone() else {
            self.message("Copy motion from a tween first.");
            return;
        };
        if !self.can_paste_motion() {
            self.message("Select one object on an unlocked layer to paste motion onto.");
            return;
        }
        let (index, id) = self.selected_object().unwrap();
        let source_start = clip.source.layers[clip.source_layer].key_at(clip.source_start);
        let unresolved_legacy_curve = source_start.is_some_and(|key| {
            matches!(
                key.motion_path_for(&clip.source_element),
                MotionPathSource::Legacy(_)
            ) && clip.motion_path.is_none()
        });
        if unresolved_legacy_curve {
            if clip.length > 10_000 {
                self.message("The pasted motion exceeds the 10,000-frame timeline limit.");
                return;
            }
            // A stored curve is shared by the whole span, relative to its first element.
            // Copying one member of a group instead records that member's visible poses;
            // otherwise the shared displacement would also move unrelated target objects.
            let mut keys = Vec::with_capacity(clip.length as usize);
            for offset in 0..clip.length {
                let Some(element) = sample_layer_in_at_fps(
                    &clip.source,
                    &clip.source.layers[clip.source_layer],
                    clip.source_start + offset,
                    clip.source_fps,
                )
                .into_iter()
                .find(|element| element.id == clip.source_element) else {
                    continue;
                };
                keys.push(MotionKey {
                    offset,
                    keyed: (offset > 0).then(|| TweenProperty::ALL.to_vec()),
                    roving: false,
                    pose: element.transform,
                });
            }
            if let Some(last) = keys.last() {
                clip.hold = last.pose;
            }
            clip.keys = keys;
            // The poses already contain easing and orientation; do not apply either twice.
            clip.ease = Easing::Linear;
            clip.property_eases.clear();
            clip.orient_to_path = false;
            clip.snap_to_guide = false;
            clip.sync_symbols = false;
            clip.motion_path = None;
        }
        if clip.kind == TweenKind::Motion {
            self.paste_object_motion(index, id, clip);
            return;
        }
        let at = self.frame;
        let end = at + clip.length;
        let target_fps = f64::from(self.project.fps);
        self.transact(|s| {
            let timeline = &mut s.symbol_mut().timeline;
            let original = timeline.layers[index].clone();
            let snapshot = timeline.clone();
            let Some(template) = sample_layer_in_at_fps(&snapshot, &original, at, target_fps)
                .into_iter()
                .find(|e| e.id == id)
            else {
                return;
            };
            let target = template.transform;
            let exposure = original
                .properties
                .end_frame
                .unwrap_or(snapshot.frame_count);
            let later_content = exposure > end || original.keyframes.iter().any(|k| k.frame > end);
            let key_at = |frame| {
                original
                    .keyframes
                    .iter()
                    .find(|k| k.frame == frame && k.keyed_properties.is_none())
                    .cloned()
                    .unwrap_or_else(|| sampled_key(&snapshot, &original, frame, target_fps))
            };
            let mut keys = vec![];
            for (i, recorded) in clip.keys.iter().enumerate() {
                let frame = at + recorded.offset;
                // Ordinary keys keep their label and outgoing tween; ◆ keys are new samples.
                let mut key = if i == 0 || clip.kind != TweenKind::Motion {
                    key_at(frame)
                } else {
                    sampled_key(&snapshot, &original, frame, target_fps)
                };
                place(
                    &mut key,
                    &template,
                    relative(&clip.keys[0].pose, &recorded.pose, &target),
                );
                if i == 0 {
                    key.tween = Some(clip.ease);
                    key.tween_kind = clip.kind;
                    key.property_eases = if clip.kind == TweenKind::Motion {
                        clip.property_eases.clone()
                    } else {
                        vec![]
                    };
                    key.orient_to_path = clip.orient_to_path;
                    key.motion_path = clip.source.layers[clip.source_layer]
                        .key_at(clip.source_start)
                        .and_then(|k| k.motion_path.clone())
                        .map(|mut path| {
                            let first = &clip.source.layers[clip.source_layer]
                                .key_at(clip.source_start)
                                .unwrap()
                                .elements[0]
                                .transform;
                            let delta = [target.x - first.x, target.y - first.y];
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
                            path
                        });
                    key.keyed_properties = None;
                } else if clip.kind == TweenKind::Motion {
                    key.tween = Some(clip.ease);
                    key.tween_kind = TweenKind::Motion;
                    key.keyed_properties = recorded.keyed.clone();
                }
                keys.push(key);
            }
            // A motion span ends at the next ordinary key: add one holding the last pose when
            // content follows the pasted span.
            if clip.kind == TweenKind::Motion && later_content {
                let mut key = key_at(end);
                place(
                    &mut key,
                    &template,
                    relative(&clip.keys[0].pose, &clip.hold, &target),
                );
                keys.push(key);
            }
            let layer = &mut timeline.layers[index];
            layer.keyframes.retain(|k| k.frame < at || k.frame > end);
            layer.keyframes.extend(keys);
            layer.keyframes.sort_by_key(|k| k.frame);
            let needed = match clip.kind {
                TweenKind::Motion => end,
                _ => end + 1,
            };
            if let Some(layer_end) = &mut layer.properties.end_frame {
                *layer_end = (*layer_end).max(needed);
            }
            layer.normalize_motion_keys();
            timeline.frame_count = timeline.frame_count.max(needed);
        });
    }

    /// Merge one copied Motion track into the selected object. Physical frames are shared by the
    /// layer, but only this object's format-10 entries and poses are replaced; other objects,
    /// labels, sounds and span timing remain intact.
    fn paste_object_motion(&mut self, index: usize, id: Id, clip: MotionClip) {
        let at = self.frame;
        let Some(desired_end) = at.checked_add(clip.length).filter(|end| *end <= 10_000) else {
            self.message("The pasted motion exceeds the 10,000-frame timeline limit.");
            return;
        };
        let timeline = &self.symbol().timeline;
        let original = &timeline.layers[index];
        let existing_span = motion_span(original, at).map(|(start, _, end)| {
            (
                start.frame,
                end.unwrap_or(timeline.frame_count).max(start.frame + 1),
            )
        });
        let (span_start, effective_end, new_span) = if let Some((start, end)) = existing_span {
            (start, desired_end.min(end).max(at + 1), false)
        } else {
            let next_ordinary = original
                .keyframes
                .iter()
                .filter(|key| key.frame > at && key.keyed_properties.is_none())
                .map(|key| key.frame)
                .min();
            (
                at,
                next_ordinary.map_or(desired_end, |next| desired_end.min(next)),
                true,
            )
        };
        if effective_end <= at {
            return;
        }
        let fps = f64::from(self.project.fps);
        let Some(template) = sample_layer_in_at_fps(timeline, original, at, fps)
            .into_iter()
            .find(|element| element.id == id)
        else {
            return;
        };
        let target = template.transform;
        let base = clip.keys[0].pose;
        self.transact_named("Paste motion", move |s| {
            {
                let timeline = &mut s.symbol_mut().timeline;
                let snapshot = timeline.clone();
                let original = snapshot.layers[index].clone();
                let layer = &mut timeline.layers[index];

                if !layer.keyframes.iter().any(|key| key.frame == span_start) {
                    let key = sampled_key(&snapshot, &original, span_start, fps);
                    layer.keyframes.push(key);
                    layer.keyframes.sort_by_key(|key| key.frame);
                }
                let start_index = layer
                    .keyframes
                    .iter()
                    .position(|key| key.frame == span_start)
                    .unwrap();
                if new_span {
                    let start = &mut layer.keyframes[start_index];
                    start.tween = Some(clip.ease);
                    start.tween_kind = TweenKind::Motion;
                    start.keyed_properties = None;
                    start.motion_keys = None;
                }

                for key in layer
                    .keyframes
                    .iter_mut()
                    .filter(|key| key.frame >= at && key.frame < effective_end)
                {
                    remove_object_motion_key(key, &id);
                }

                if at == span_start {
                    let start = &mut layer.keyframes[start_index];
                    place(start, &template, target);
                    let inherits_legacy_path = start.motion_path.is_some();
                    let path = clip.motion_path.as_ref().map(|path| {
                        translate_motion_path(path, [target.x - base.x, target.y - base.y])
                    });
                    let track = start.ensure_motion_track(&id);
                    track.easing = Some(clip.ease);
                    track.property_eases = Some(clip.property_eases.clone());
                    track.orient_to_path = Some(clip.orient_to_path);
                    track.snap_to_guide = Some(clip.snap_to_guide);
                    track.sync_symbols = Some(clip.sync_symbols);
                    track.motion_path = path.map_or_else(
                        || inherits_legacy_path.then_some(MotionPathOverride::Disabled),
                        |commands| Some(MotionPathOverride::Path { commands }),
                    );
                }

                for recorded in &clip.keys {
                    let frame = at + recorded.offset;
                    if frame >= effective_end {
                        continue;
                    }
                    let pose = relative(&base, &recorded.pose, &target);
                    if frame == span_start {
                        place(&mut layer.keyframes[start_index], &template, pose);
                        continue;
                    }
                    let key = ensure_object_motion_key(
                        layer, &snapshot, &original, frame, fps, clip.ease,
                    );
                    if key.keyed_properties.is_none() {
                        continue;
                    }
                    let properties = recorded
                        .keyed
                        .clone()
                        .unwrap_or_else(|| TweenProperty::ALL.to_vec());
                    if let Some(element) = key.elements.iter_mut().find(|element| element.id == id)
                    {
                        copy_motion_properties(&mut element.transform, &pose, &properties);
                    } else {
                        place(key, &template, pose);
                    }
                    let object_key = key.ensure_motion_property_key(&id);
                    object_key.properties = properties;
                    object_key.roving = recorded.roving;
                    key.sync_motion_key_union();
                }

                if new_span && effective_end < snapshot.frame_count {
                    if !layer.keyframes.iter().any(|key| key.frame == effective_end) {
                        layer
                            .keyframes
                            .push(sampled_key(&snapshot, &original, effective_end, fps));
                    }
                    if let Some(end_key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|key| key.frame == effective_end)
                    {
                        place(end_key, &template, relative(&base, &clip.hold, &target));
                    }
                }
                layer.keyframes.sort_by_key(|key| key.frame);
                layer.normalize_motion_keys();
                timeline.frame_count = timeline.frame_count.max(effective_end);
            }
            s.project.upgrade_format_for_features();
        });
    }

    pub(crate) fn open_paste_motion_special(&mut self) {
        if self.can_paste_motion() {
            self.finish_edits();
            self.timeline_motion.paste_special_open = true;
        }
    }

    /// Selective paste samples both animations before changing the destination. Materialized
    /// frames preserve every unticked animated property, other objects, labels, scripts and sounds.
    /// This also keeps different source/target eases and guide trajectories exact at frame times.
    pub(crate) fn paste_motion_special(&mut self, options: &PasteMotionOptions) {
        if !options.any() || !self.can_paste_motion() {
            return;
        }
        self.finish_edits();
        let clip = self.timeline_motion.clipboard.clone().unwrap();
        let (index, id) = self.selected_object().unwrap();
        let at = self.frame;
        let target_timeline = self.symbol().timeline.clone();
        let target_fps = f64::from(self.project.fps);
        let original = &target_timeline.layers[index];
        // Ease alone edits the existing span's timing without replacing its authored poses.
        if !options.properties[..8].iter().any(|chosen| *chosen) {
            let key = motion_span(original, at)
                .map(|(start, _, _)| start)
                .or_else(|| original.key_at(at).filter(|key| key.tween.is_some()));
            let Some(key) = key else {
                self.message("Select a tweened target frame to paste only its ease.");
                return;
            };
            let start = key.frame;
            self.transact_named("Paste motion special", |s| {
                let key = s.symbol_mut().timeline.layers[index]
                    .keyframes
                    .iter_mut()
                    .find(|key| key.frame == start)
                    .unwrap();
                if key.tween_kind == TweenKind::Motion {
                    let track = key.ensure_motion_track(&id);
                    track.easing = Some(clip.ease);
                    track.property_eases = Some(clip.property_eases.clone());
                } else {
                    key.tween = Some(clip.ease);
                    key.property_eases.clear();
                }
                s.project.upgrade_format_for_features();
            });
            return;
        }
        let last_offset = if clip.kind == TweenKind::Motion {
            clip.length.saturating_sub(1)
        } else {
            clip.length
        };
        let Some(paste_end) = at.checked_add(last_offset).filter(|v| *v < 10000) else {
            self.message("The pasted motion exceeds the 10,000-frame timeline limit.");
            return;
        };
        let Some(target) = sample_layer_in_at_fps(&target_timeline, original, at, target_fps)
            .into_iter()
            .find(|e| e.id == id)
        else {
            return;
        };
        if !options.properties[6]
            && !options.properties[7]
            && self.paste_motion_special_into_object_span(
                &clip,
                options,
                index,
                &id,
                at,
                paste_end,
                &target_timeline,
                target_fps,
                &target,
            )
        {
            return;
        }
        let mut source = clip.source.clone();
        // Ease unchecked uses the destination span's ease for the copied property changes.
        if !options.properties[8] {
            let target_key = motion_span(original, at)
                .map(|(start, _, _)| start)
                .or_else(|| original.key_at(at));
            for key in &mut source.layers[clip.source_layer].keyframes {
                if key.tween.is_some() {
                    key.tween = Some(target_key.and_then(|k| k.tween).unwrap_or(Easing::Linear));
                    key.property_eases = if key.tween_kind == TweenKind::Motion {
                        target_key
                            .map(|k| k.property_eases.clone())
                            .unwrap_or_default()
                    } else {
                        vec![]
                    };
                }
            }
        }
        // Splitting a target tween would change its easing before/after the pasted interval.
        // Materialize just the intersected target spans as well, preserving every displayed pose
        // (including other objects and non-cubic eases) outside the selected interval.
        let target_span = |frame| {
            if let Some((start, _, end)) = motion_span(original, frame) {
                Some((start.frame, end.unwrap_or(target_timeline.frame_count)))
            } else {
                original
                    .key_at(frame)
                    .filter(|key| key.tween.is_some())
                    .map(|key| {
                        let end = original
                            .keyframes
                            .iter()
                            .filter(|next| next.frame > key.frame)
                            .map(|key| key.frame)
                            .min()
                            .unwrap_or(target_timeline.frame_count);
                        (key.frame, end)
                    })
            }
        };
        let bake_start = target_span(at).map_or(at, |(start, _)| start);
        let bake_end = target_span(paste_end)
            .map_or(paste_end, |(_, end)| paste_end.max(end.saturating_sub(1)));
        let first = clip.keys[0].pose;
        let mut keys = Vec::new();
        for frame in bake_start..=bake_end {
            let mut key = original
                .keyframes
                .iter()
                .find(|k| k.frame == frame)
                .cloned()
                .unwrap_or_else(|| sampled_key(&target_timeline, original, frame, target_fps));
            key.elements = sample_layer_in_at_fps(&target_timeline, original, frame, target_fps);
            if (at..=paste_end).contains(&frame)
                && let Some(copied) = sample_layer_in_at_fps(
                    &source,
                    &source.layers[clip.source_layer],
                    clip.source_start + frame - at,
                    clip.source_fps,
                )
                .into_iter()
                .find(|e| e.id == clip.source_element)
            {
                let mut element = key
                    .elements
                    .iter()
                    .find(|e| e.id == id)
                    .cloned()
                    .unwrap_or_else(|| target.clone());
                let delta = relative(&first, &copied.transform, &target.transform);
                let p = &options.properties;
                if p[0] {
                    element.transform.x = delta.x;
                }
                if p[1] {
                    element.transform.y = delta.y;
                }
                if p[2] {
                    element.transform.scale_x = if options.preserve_scale {
                        delta.scale_x
                    } else {
                        copied.transform.scale_x
                    };
                }
                if p[3] {
                    element.transform.scale_y = if options.preserve_scale {
                        delta.scale_y
                    } else {
                        copied.transform.scale_y
                    };
                }
                if p[4] {
                    element.transform.rotation = if options.preserve_rotation {
                        delta.rotation
                    } else {
                        copied.transform.rotation
                    };
                    element.transform.skew = if options.preserve_rotation {
                        delta.skew
                    } else {
                        copied.transform.skew
                    };
                }
                if p[5] {
                    element.transform.alpha = copied.transform.alpha;
                    element.transform.color_transform = copied.transform.color_transform;
                }
                if p[6] {
                    element.filters = copied.filters;
                }
                if p[7] {
                    element.blend_mode = copied.blend_mode;
                }
                if let Some(existing) = key.elements.iter_mut().find(|e| e.id == id) {
                    *existing = element;
                } else {
                    key.elements.push(element);
                }
            }
            key.tween = None;
            key.tween_kind = TweenKind::Classic;
            key.keyed_properties = None;
            key.property_eases.clear();
            key.orient_to_path = false;
            key.snap_to_guide = false;
            key.sync_symbols = false;
            key.shape_hints.clear();
            key.motion_path = None;
            keys.push(key);
        }
        self.transact_named("Paste motion special", |s| {
            let timeline = &mut s.symbol_mut().timeline;
            let layer = &mut timeline.layers[index];
            let continuation = bake_end + 1;
            if continuation < target_timeline.frame_count
                && !layer.keyframes.iter().any(|k| k.frame == continuation)
            {
                keys.push(sampled_key(
                    &target_timeline,
                    original,
                    continuation,
                    target_fps,
                ));
            }
            layer
                .keyframes
                .retain(|k| k.frame < bake_start || k.frame > bake_end);
            layer.keyframes.extend(keys);
            layer.keyframes.sort_by_key(|k| k.frame);
            layer.normalize_motion_keys();
            if let Some(exposure) = &mut layer.properties.end_frame {
                *exposure = (*exposure).max(paste_end + 1);
            }
            timeline.frame_count = timeline.frame_count.max(paste_end + 1);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paste_motion_special_into_object_span(
        &mut self,
        clip: &MotionClip,
        options: &PasteMotionOptions,
        index: usize,
        id: &str,
        at: u32,
        paste_end: u32,
        target_timeline: &Timeline,
        target_fps: f64,
        target: &Element,
    ) -> bool {
        let original = &target_timeline.layers[index];
        let Some((start, _, span_end)) = motion_span(original, at) else {
            return false;
        };
        let span_start = start.frame;
        let end = paste_end.min(
            span_end
                .unwrap_or(target_timeline.frame_count)
                .saturating_sub(1),
        );
        if end < at {
            return false;
        }
        let p = options.properties;
        let properties: Vec<TweenProperty> = [
            (p[0], TweenProperty::X),
            (p[1], TweenProperty::Y),
            (p[2], TweenProperty::ScaleX),
            (p[3], TweenProperty::ScaleY),
            (p[4], TweenProperty::Rotation),
            (p[4], TweenProperty::Skew),
            (p[5], TweenProperty::Alpha),
            (p[5], TweenProperty::Color),
        ]
        .into_iter()
        .filter_map(|(chosen, property)| chosen.then_some(property))
        .collect();
        if properties.is_empty() {
            return false;
        }
        let first = clip.keys[0].pose;
        let mut updates = Vec::with_capacity((end - at + 1) as usize);
        for frame in at..=end {
            let Some(copied) = sample_layer_in_at_fps(
                &clip.source,
                &clip.source.layers[clip.source_layer],
                clip.source_start + frame - at,
                clip.source_fps,
            )
            .into_iter()
            .find(|element| element.id == clip.source_element) else {
                continue;
            };
            let Some(mut element) =
                sample_layer_in_at_fps(target_timeline, original, frame, target_fps)
                    .into_iter()
                    .find(|element| element.id == id)
            else {
                continue;
            };
            let delta = relative(&first, &copied.transform, &target.transform);
            if p[0] {
                element.transform.x = delta.x;
            }
            if p[1] {
                element.transform.y = delta.y;
            }
            if p[2] {
                element.transform.scale_x = if options.preserve_scale {
                    delta.scale_x
                } else {
                    copied.transform.scale_x
                };
            }
            if p[3] {
                element.transform.scale_y = if options.preserve_scale {
                    delta.scale_y
                } else {
                    copied.transform.scale_y
                };
            }
            if p[4] {
                element.transform.rotation = if options.preserve_rotation {
                    delta.rotation
                } else {
                    copied.transform.rotation
                };
                element.transform.skew = if options.preserve_rotation {
                    delta.skew
                } else {
                    copied.transform.skew
                };
            }
            if p[5] {
                element.transform.alpha = copied.transform.alpha;
                element.transform.color_transform = copied.transform.color_transform;
            }
            updates.push((frame, element));
        }
        let id = id.to_owned();
        let properties_for_keys = properties.clone();
        let clip_ease = clip.ease;
        let clip_property_eases = clip.property_eases.clone();
        let paste_ease = p[8];
        self.transact_named("Paste motion special", move |s| {
            {
                let timeline = &mut s.symbol_mut().timeline;
                let snapshot = timeline.clone();
                let original = snapshot.layers[index].clone();
                let layer = &mut timeline.layers[index];
                for key in layer
                    .keyframes
                    .iter_mut()
                    .filter(|key| key.frame >= at && key.frame <= end)
                {
                    remove_object_motion_properties(key, &id, &properties_for_keys);
                }
                for (frame, element) in updates {
                    let key = if frame == span_start {
                        layer
                            .keyframes
                            .iter_mut()
                            .find(|key| key.frame == span_start)
                            .unwrap()
                    } else {
                        ensure_object_motion_key(
                            layer, &snapshot, &original, frame, target_fps, clip_ease,
                        )
                    };
                    if let Some(existing) = key.elements.iter_mut().find(|item| item.id == id) {
                        copy_motion_properties(
                            &mut existing.transform,
                            &element.transform,
                            &properties_for_keys,
                        );
                    } else {
                        key.elements.push(element);
                    }
                    if frame != span_start {
                        let object_key = key.ensure_motion_property_key(&id);
                        for property in &properties_for_keys {
                            if !object_key.properties.contains(property) {
                                object_key.properties.push(*property);
                            }
                        }
                        object_key.roving = false;
                        key.sync_motion_key_union();
                    }
                }
                if paste_ease
                    && let Some(start) = layer
                        .keyframes
                        .iter_mut()
                        .find(|key| key.frame == span_start)
                {
                    let track = start.ensure_motion_track(&id);
                    track.easing = Some(clip_ease);
                    track.property_eases = Some(clip_property_eases);
                }
                layer.normalize_motion_keys();
            }
            s.project.upgrade_format_for_features();
        });
        true
    }

    // ── Motion path on the stage ──

    /// The motion tween path of the selected object while the playhead is in its motion span.
    pub(crate) fn motion_path(&self) -> Option<MotionPath> {
        let (index, element) = self.selected_object()?;
        let timeline = &self.symbol().timeline;
        let layer = timeline.layers.get(index)?;
        let (start, properties, end) = motion_span(layer, self.frame)?;
        let last = properties.last().map_or(start.frame, |k| k.frame);
        let end = end
            .unwrap_or(timeline.frame_count)
            .min(timeline.frame_count)
            .max(last + 1);
        let position = |frame| {
            sample_layer_in_at_fps(timeline, layer, frame, f64::from(self.project.fps))
                .into_iter()
                .find(|e| e.id == element)
                .map(|e| [e.transform.x, e.transform.y])
        };
        let frames: Vec<_> = (start.frame..end).filter_map(position).collect();
        // Key points sit where the object is shown on their frame (a ◆ key that does not key X/Y
        // stores a stale position until its next refresh).
        let keys = std::iter::once(start)
            .chain(
                properties
                    .into_iter()
                    .filter(|key| !key.motion_key_properties_for(&element).is_empty()),
            )
            .filter(|key| key.elements.iter().any(|e| e.id == element))
            .filter_map(|key| position(key.frame).map(|at| (key.frame, at)))
            .collect();
        Some(MotionPath {
            layer: index,
            element,
            frames,
            keys,
        })
    }

    pub(crate) fn motion_paths_interactive(&self) -> bool {
        self.workspace.timeline.show_motion_path
            && matches!(self.tool, Tool::Select | Tool::Transform)
            && self
                .selection
                .single()
                .is_some_and(|target| self.target_editable(target))
    }

    /// Start dragging the key point at `frame` of the selected object's motion path.
    #[cfg(test)]
    pub(crate) fn begin_motion_key_drag(&mut self, frame: u32, press: [f64; 2]) -> bool {
        self.begin_motion_key_drag_with_context(frame, press, None, None)
    }

    /// Start a stage pointer drag and capture the exact view transform used to interpret it.
    pub(crate) fn begin_motion_key_drag_in_stage_context(
        &mut self,
        frame: u32,
        press: [f64; 2],
        mapping: StageMapping,
        zoom: f32,
    ) -> bool {
        self.begin_motion_key_drag_with_context(frame, press, Some(mapping), Some(zoom))
    }

    fn begin_motion_key_drag_with_context(
        &mut self,
        frame: u32,
        press: [f64; 2],
        mapping: Option<StageMapping>,
        zoom: Option<f32>,
    ) -> bool {
        if !self.workspace.timeline.show_motion_path {
            return false;
        }
        let Some(path) = self.motion_path() else {
            return false;
        };
        let Some(&(_, start)) = path.keys.iter().find(|(f, _)| *f == frame) else {
            return false;
        };
        self.finish_edits();
        self.playing = false;
        self.timeline_motion.drag = Some(MotionKeyDrag {
            symbol: self.symbol_id().to_owned(),
            layer: self.symbol().timeline.layers[path.layer].id.clone(),
            element: path.element,
            frame,
            start,
            press,
            mapping,
            zoom,
            began: false,
        });
        true
    }

    /// Move the dragged key point with the pointer (timeline coordinates). The key's X and Y become
    /// keyed properties so the path follows the pointer while dragging.
    pub(crate) fn drag_motion_key_to(&mut self, pointer: [f64; 2]) {
        let Some(drag) = self.timeline_motion.drag.clone() else {
            return;
        };
        if drag.symbol != self.symbol_id() {
            self.timeline_motion.drag = None;
            return;
        }
        let mut to = [
            drag.start[0] + pointer[0] - drag.press[0],
            drag.start[1] + pointer[1] - drag.press[1],
        ];
        {
            let timeline = &self.symbol().timeline;
            if let Some(layer) = timeline.layers.iter().find(|l| l.id == drag.layer)
                && let Some((start, _, _)) = motion_span(layer, drag.frame)
                && start.motion_snap_to_guide_for(&drag.element)
                && let Some(guide_id) = layer.properties.motion_guide.as_deref()
                && let Some(guide) = timeline
                    .layers
                    .iter()
                    .find(|l| l.id == guide_id && l.properties.role == LayerRole::Guide)
                && let Some(path) = guide_polyline(guide, start.frame)
            {
                let snapped = path.point_at(path.nearest(to));
                let distance = if let (Some(mapping), Some(zoom)) = (drag.mapping, drag.zoom) {
                    let at = mapping.edit_to_stage_point(to);
                    let snapped_at = mapping.edit_to_stage_point(snapped);
                    (snapped_at[0] - at[0]).hypot(snapped_at[1] - at[1]) * f64::from(zoom.abs())
                } else {
                    (snapped[0] - to[0]).hypot(snapped[1] - to[1])
                };
                if distance <= 12.0 {
                    to = snapped;
                }
            }
        }
        if !drag.began {
            if (to[0] - drag.start[0]).abs() < 1e-9 && (to[1] - drag.start[1]).abs() < 1e-9 {
                return;
            }
            self.history.begin(&self.project);
            if let Some(d) = &mut self.timeline_motion.drag {
                d.began = true;
            }
        }
        let fps = f64::from(self.project.fps);
        let Some(layer) = self
            .symbol_mut()
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == drag.layer)
        else {
            return;
        };
        let before = layer.clone();
        let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == drag.frame) else {
            return;
        };
        if key.keyed_properties.is_some() {
            let object_key = key.ensure_motion_property_key(&drag.element);
            for property in [TweenProperty::X, TweenProperty::Y] {
                if !object_key.properties.contains(&property) {
                    object_key.properties.push(property);
                }
            }
            key.sync_motion_key_union();
        }
        if let Some(element) = key.elements.iter_mut().find(|e| e.id == drag.element) {
            element.transform.x = to[0];
            element.transform.y = to[1];
        }
        crate::timeline_motion_curve::rebase_layer_motion_paths(&before, layer, fps);
    }

    /// End the key point drag: one Undo step when it moved.
    pub(crate) fn finish_motion_key_drag(&mut self) {
        if let Some(drag) = self.timeline_motion.drag.take()
            && drag.began
        {
            self.commit_history();
            self.reset_authored_preview();
        }
    }

    /// Stage input for motion path key points. Runs before the stage tools read the press; a press
    /// on a key point is consumed and suppresses the stage click handlers like a guide press.
    pub(crate) fn motion_path_input(
        &mut self,
        ui: &egui::Ui,
        canvas: Rect,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        if self.timeline_motion.drag.as_ref().is_some_and(|drag| {
            drag.mapping.is_some_and(|captured| captured != mapping)
                || drag
                    .zoom
                    .is_some_and(|captured| captured.to_bits() != self.zoom.to_bits())
        }) {
            self.cancel_motion_drags();
        }
        let zoom = self.zoom;
        let to_local = |pos: Pos2| {
            let point = mapping.screen_to_edit(origin, zoom, pos);
            [f64::from(point.x), f64::from(point.y)]
        };
        let screen =
            |p: [f64; 2]| mapping.edit_to_screen(origin, zoom, Pos2::new(p[0] as f32, p[1] as f32));
        let path = self
            .motion_paths_interactive()
            .then(|| self.motion_path())
            .flatten();
        let hit = |pos: Pos2| {
            path.as_ref().and_then(|path| {
                path.keys
                    .iter()
                    .rev()
                    .find(|(_, at)| cv::motion_key_hit(screen(*at), pos))
                    .map(|(frame, _)| *frame)
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
                    ..
                } if canvas.contains(pos)
                    && !(rulers && layout.contains(pos))
                    && self.timeline_motion.drag.is_none()
                    && ui
                        .ctx()
                        .layer_id_at(pos)
                        .is_none_or(|layer| layer == ui.layer_id()) =>
                {
                    if let Some(frame) = hit(pos)
                        && self.begin_motion_key_drag_in_stage_context(
                            frame,
                            to_local(pos),
                            mapping,
                            zoom,
                        )
                    {
                        taken = Some(pos);
                        self.stage_view.guide_pointer = true;
                    }
                }
                egui::Event::PointerMoved(pos) if self.timeline_motion.drag.is_some() => {
                    self.drag_motion_key_to(to_local(pos));
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } if self.timeline_motion.drag.is_some() => {
                    self.drag_motion_key_to(to_local(pos));
                    self.finish_motion_key_drag();
                }
                _ => {}
            }
        }
        if let Some(pos) = taken {
            ui.ctx().input_mut(|i| {
                i.events.retain(|event| {
                    !matches!(event, egui::Event::PointerButton { pos: p, pressed: true, .. } if *p == pos)
                })
            });
        }
        self.timeline_motion.hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|pos| canvas.contains(*pos))
            .and_then(hit);
        // Hand cursor over a key point / while dragging one: `Editor::show_stage_cursor`.
    }

    /// `NF2/Motion Path` for the selected motion-tweened object, in screen points.
    pub(crate) fn paint_motion_path(
        &self,
        painter: &egui::Painter,
        origin: Pos2,
        mapping: StageMapping,
    ) {
        if !self.workspace.timeline.show_motion_path {
            return;
        }
        let Some(path) = self.motion_path() else {
            return;
        };
        let zoom = self.zoom;
        let screen =
            |p: [f64; 2]| mapping.edit_to_screen(origin, zoom, Pos2::new(p[0] as f32, p[1] as f32));
        let frames: Vec<Pos2> = path.frames.iter().map(|p| screen(*p)).collect();
        if self.workspace.timeline.show_motion_frame_dots {
            cv::paint_motion_path(painter, painter.ctx(), &frames);
        } else if frames.len() >= 2 {
            let nf = neoflash_ui::palette(painter.ctx());
            painter.add(egui::Shape::line(
                frames,
                egui::Stroke::new(1.0_f32, nf.guide),
            ));
        }
        self.paint_motion_handles(painter, origin, mapping);
        let dragged = self.timeline_motion.drag.as_ref().map(|d| d.frame);
        for (frame, at) in &path.keys {
            let state = if dragged == Some(*frame) {
                MotionKeyState::Selected
            } else if dragged.is_none() && self.timeline_motion.hover == Some(*frame) {
                MotionKeyState::Hover
            } else {
                MotionKeyState::Default
            };
            cv::paint_motion_key(painter, painter.ctx(), screen(*at), state);
        }
    }
}
