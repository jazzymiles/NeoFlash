//! Motion on the stage and between objects: the motion tween path of the selected instance with
//! draggable key points (`NF2/Motion Path`, R-005), and Copy motion / Paste motion (frame menu).
//! Every document change is one Undo step through `Editor::commit_history`.
use crate::app::{Editor, Tool};
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
            .map(|(_, id)| id);
        let pick = |key: &Keyframe| -> Option<Id> {
            selected
                .clone()
                .filter(|id| key.elements.iter().any(|e| &e.id == id))
                .or_else(|| (key.elements.len() == 1).then(|| key.elements[0].id.clone()))
                .or_else(|| {
                    key.elements
                        .iter()
                        .find(|e| matches!(e.kind, ElementKind::Instance { .. }))
                        .map(|e| e.id.clone())
                })
        };
        let pose = |key: &Keyframe, id: &str| {
            key.elements
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.transform)
        };
        if let Some((start, properties, end)) = motion_span(layer, self.frame) {
            let id = pick(start).ok_or("The motion tween has no object to copy.")?;
            let last = properties.last().map_or(start.frame, |k| k.frame);
            let end = end
                .unwrap_or(timeline.frame_count)
                .max(last + 1)
                .max(start.frame + 1);
            let mut keys = vec![MotionKey {
                offset: 0,
                keyed: None,
                pose: pose(start, &id).unwrap(),
            }];
            for key in properties {
                if let Some(pose) = pose(key, &id) {
                    keys.push(MotionKey {
                        offset: key.frame - start.frame,
                        keyed: key.keyed_properties.clone(),
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
                ease: start.tween.unwrap_or(Easing::Linear),
                property_eases: start.property_eases.clone(),
                orient_to_path: start.orient_to_path,
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
        let id = pick(key).ok_or("The tween has no object to copy.")?;
        let (Some(first), Some(last)) = (pose(key, &id), pose(next, &id)) else {
            return Err("The tweened object is not on the end keyframe.");
        };
        Ok(MotionClip {
            kind: TweenKind::Classic,
            ease,
            property_eases: vec![],
            orient_to_path: key.orient_to_path,
            length: next.frame - key.frame,
            keys: vec![
                MotionKey {
                    offset: 0,
                    keyed: None,
                    pose: first,
                },
                MotionKey {
                    offset: next.frame - key.frame,
                    keyed: None,
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
        let has_curve = source_start.is_some_and(|key| key.motion_path.is_some());
        let source_group = source_start.is_some_and(|key| key.elements.len() > 1);
        let target_group = self.symbol().timeline.layers[index]
            .keyframes
            .iter()
            .any(|key| key.elements.len() > 1);
        if has_curve && (source_group || target_group) {
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
            clip.source.layers[clip.source_layer]
                .key_at_mut(clip.source_start)
                .unwrap()
                .motion_path = None;
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
                key.tween = Some(clip.ease);
                key.property_eases = if key.tween_kind == TweenKind::Motion {
                    clip.property_eases.clone()
                } else {
                    vec![]
                };
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
            .chain(properties)
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
    pub(crate) fn begin_motion_key_drag(&mut self, frame: u32, press: [f64; 2]) -> bool {
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
                && start.snap_to_guide
                && let Some(guide_id) = layer.properties.motion_guide.as_deref()
                && let Some(guide) = timeline
                    .layers
                    .iter()
                    .find(|l| l.id == guide_id && l.properties.role == LayerRole::Guide)
                && let Some(path) = guide_polyline(guide, start.frame)
            {
                let snapped = path.point_at(path.nearest(to));
                if (snapped[0] - to[0]).hypot(snapped[1] - to[1]) <= 12.0 {
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
        if let Some(keyed) = &mut key.keyed_properties {
            for property in [TweenProperty::X, TweenProperty::Y] {
                if !keyed.contains(&property) {
                    keyed.push(property);
                }
            }
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
    pub(crate) fn motion_path_input(&mut self, ui: &egui::Ui, canvas: Rect, origin: Pos2) {
        let zoom = self.zoom;
        let to_local = |pos: Pos2| {
            [
                ((pos.x - origin.x) / zoom) as f64,
                ((pos.y - origin.y) / zoom) as f64,
            ]
        };
        let screen = |p: [f64; 2]| origin + egui::vec2(p[0] as f32, p[1] as f32) * zoom;
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
                        && self.begin_motion_key_drag(frame, to_local(pos))
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
    pub(crate) fn paint_motion_path(&self, painter: &egui::Painter, origin: Pos2) {
        if !self.workspace.timeline.show_motion_path {
            return;
        }
        let Some(path) = self.motion_path() else {
            return;
        };
        let zoom = self.zoom;
        let screen = |p: [f64; 2]| origin + egui::vec2(p[0] as f32, p[1] as f32) * zoom;
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
        self.paint_motion_handles(painter, origin);
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
