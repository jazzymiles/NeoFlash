//! Timeline commands behind the Timeline controls bar, the frame and layer menus and the
//! Insert / Modify ▸ Timeline / Control menus: tween kinds, motion guides, keyframe conversion,
//! reverse, scenes and playback options. Each document change is one Undo step.
use crate::app::Editor;
use neoflash_core::*;

/// Shapes a shape tween morphs; everything else on the layer holds still.
pub(crate) fn is_shape(element: &Element) -> bool {
    matches!(
        element.kind,
        ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
            | ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. }
    )
}

fn copy_property(to: &mut Transform, from: &Transform, property: TweenProperty) {
    match property {
        TweenProperty::X => to.x = from.x,
        TweenProperty::Y => to.y = from.y,
        TweenProperty::ScaleX => to.scale_x = from.scale_x,
        TweenProperty::ScaleY => to.scale_y = from.scale_y,
        TweenProperty::Rotation => to.rotation = from.rotation,
        TweenProperty::Skew => to.skew = from.skew,
        TweenProperty::Alpha => to.alpha = from.alpha,
        TweenProperty::Color => to.color_transform = from.color_transform,
    }
}

fn property_changed(a: &Transform, b: &Transform, property: TweenProperty) -> bool {
    match property {
        TweenProperty::Color => a.color_transform != b.color_transform,
        _ => (property_value(a, property) - property_value(b, property)).abs() > 1e-9,
    }
}

/// Motion tween editing: a change made on a ◆ property keyframe keys exactly the properties
/// that changed; the other tracks keep interpolating. Unkeyed values stored on property keys are
/// then refreshed to the displayed pose so later edits start from what the stage shows.
pub(crate) fn record_motion_property_keys(before: &Project, project: &mut Project) {
    for symbol in &mut project.symbols {
        let old_symbol = before.symbol(&symbol.id);
        for layer in &mut symbol.timeline.layers {
            if !layer.keyframes.iter().any(|k| k.keyed_properties.is_some()) {
                continue;
            }
            layer.normalize_motion_keys();
            if let Some(old) =
                old_symbol.and_then(|s| s.timeline.layers.iter().find(|l| l.id == layer.id))
            {
                for key in &mut layer.keyframes {
                    let Some(keyed) = &mut key.keyed_properties else {
                        continue;
                    };
                    let reference = match old.keyframes.iter().find(|k| k.frame == key.frame) {
                        Some(old_key) if old_key.keyed_properties.is_some() => {
                            old_key.elements.clone()
                        }
                        _ => sample_layer(old, key.frame),
                    };
                    for element in &key.elements {
                        let Some(previous) = reference.iter().find(|e| e.id == element.id) else {
                            continue;
                        };
                        for property in TweenProperty::ALL {
                            if !keyed.contains(&property)
                                && property_changed(
                                    &element.transform,
                                    &previous.transform,
                                    property,
                                )
                            {
                                keyed.push(property);
                            }
                        }
                    }
                }
            }
            let snapshot = layer.clone();
            for key in &mut layer.keyframes {
                let Some(keyed) = key.keyed_properties.clone() else {
                    continue;
                };
                let shown = sample_layer(&snapshot, key.frame);
                for element in &mut key.elements {
                    if let Some(pose) = shown.iter().find(|e| e.id == element.id) {
                        for property in TweenProperty::ALL {
                            if !keyed.contains(&property) {
                                copy_property(&mut element.transform, &pose.transform, property);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Editor {
    /// Commit the open Undo step after document housekeeping that depends on what changed.
    pub(crate) fn commit_history(&mut self) -> bool {
        self.history
            .commit_with(&mut self.project, |before, project| {
                record_motion_property_keys(before, project);
                project.prune_motion_guides();
                project.upgrade_format_for_features();
            })
    }

    /// Tween kind of the span under the playhead on the current layer; `None` = not tweened.
    pub(crate) fn current_tween(&self) -> Option<TweenKind> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        if motion_span(layer, self.frame).is_some() {
            return Some(TweenKind::Motion);
        }
        let key = layer.key_at(self.frame)?;
        key.tween.map(|_| key.tween_kind)
    }

    /// First frame of the span under the playhead (◆ property keys belong to their motion span).
    fn span_start(&self) -> Option<u32> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        if let Some((start, ..)) = motion_span(layer, self.frame) {
            return Some(start.frame);
        }
        layer.key_at(self.frame).map(|key| key.frame)
    }

    /// Whether a tween of `kind` can be created on the span under the playhead.
    pub(crate) fn can_create_tween(&self) -> bool {
        self.layer_editable() && self.span_start().is_some()
    }

    /// Create a tween of `kind` on the span under the playhead, or switch its kind. One Undo step.
    /// Switching away from a motion tween keeps each ◆ pose as an ordinary keyframe.
    pub(crate) fn create_tween(&mut self, kind: TweenKind) {
        self.finish_edits();
        if !self.layer_editable() {
            return;
        }
        let Some(start) = self.span_start() else {
            self.message("Insert a keyframe (F6) before creating a tween.");
            return;
        };
        let layer = &self.symbol().timeline.layers[self.layer];
        let key = layer.keyframes.iter().find(|k| k.frame == start).unwrap();
        match kind {
            TweenKind::Shape => {
                let shapes = |key: &Keyframe| key.elements.iter().filter(|e| is_shape(e)).count();
                let next = layer
                    .keyframes
                    .iter()
                    .filter(|k| k.frame > start && k.keyed_properties.is_none())
                    .min_by_key(|k| k.frame);
                if shapes(key) == 0 || next.is_none_or(|next| shapes(next) == 0) {
                    self.message("Shape tweens need shapes on both keyframes. Break symbols and text apart (⌘B), or use a classic tween.");
                    return;
                }
                if key.elements.iter().any(|e| !is_shape(e)) {
                    self.message("Only shapes morph in a shape tween; symbols, text and bitmaps on this layer hold still.");
                }
            }
            TweenKind::Motion => {
                if key.elements.len() != 1
                    || !matches!(
                        key.elements[0].kind,
                        ElementKind::Instance { .. }
                            | ElementKind::Text { .. }
                            | ElementKind::TextBox { .. }
                    )
                {
                    self.message("A motion tween needs exactly one symbol instance or text block on its keyframe. Convert the artwork to a symbol (F8) first.");
                    return;
                }
            }
            TweenKind::Classic => {}
        }
        self.transact(|s| {
            let l = s.layer;
            let layer = &mut s.symbol_mut().timeline.layers[l];
            let mut in_span = false;
            for key in &mut layer.keyframes {
                if key.frame == start {
                    in_span = true;
                    let default = match kind {
                        TweenKind::Classic => Easing::EaseInOut,
                        _ => Easing::Linear,
                    };
                    key.tween = Some(key.tween.unwrap_or(default));
                    key.tween_kind = kind;
                } else if in_span && key.frame > start && key.keyed_properties.is_some() {
                    if kind != TweenKind::Motion {
                        key.keyed_properties = None;
                        key.tween_kind = kind;
                    }
                } else if key.frame > start {
                    in_span = false;
                }
            }
            layer.keyframes.sort_by_key(|k| k.frame);
            s.project.upgrade_format_for_features();
        });
    }

    /// Remove the tween of the span under the playhead; a motion tween also drops its ◆ keys.
    pub(crate) fn remove_tween(&mut self) {
        self.finish_edits();
        if !self.layer_editable() {
            return;
        }
        let Some(start) = self.span_start() else {
            return;
        };
        self.transact(|s| {
            let l = s.layer;
            let layer = &mut s.symbol_mut().timeline.layers[l];
            layer.keyframes.sort_by_key(|k| k.frame);
            let mut in_span = false;
            layer.keyframes.retain(|key| {
                if key.frame == start {
                    in_span = true;
                    return true;
                }
                if key.keyed_properties.is_none() {
                    in_span = false;
                }
                !(in_span && key.frame > start)
            });
            if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == start) {
                key.tween = None;
                key.tween_kind = TweenKind::Classic;
            }
        });
    }

    /// Tween group buttons and the frame menu toggle: same kind removes, another kind switches.
    pub(crate) fn toggle_tween(&mut self, kind: TweenKind) {
        if self.current_tween() == Some(kind) {
            self.remove_tween();
        } else {
            self.create_tween(kind);
        }
    }

    /// Add motion guide is available for an editable normal layer that has no guide yet.
    pub(crate) fn can_add_motion_guide_to(&self, index: usize) -> bool {
        let timeline = &self.symbol().timeline;
        timeline.layers.get(index).is_some_and(|layer| {
            layer.properties.role == LayerRole::Normal
                && layer.properties.motion_guide.is_none()
                && !layer_effectively_locked(timeline, layer)
                && layer_effectively_visible(timeline, layer)
        })
    }

    /// Insert a guide layer "Guide: <name>" above the current layer and guide the current layer
    /// by it. The new guide becomes the current layer so its path can be drawn. One Undo step.
    pub(crate) fn add_motion_guide(&mut self) {
        self.finish_edits();
        if !self.can_add_motion_guide_to(self.layer) {
            self.message("Select an unlocked, visible normal layer without a motion guide.");
            return;
        }
        self.transact(|s| {
            let l = s.layer;
            let timeline = &mut s.symbol_mut().timeline;
            let guided = &timeline.layers[l];
            let id = new_id("layer");
            let guide = Layer {
                id: id.clone(),
                name: format!("Guide: {}", guided.name),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe::default()],
                properties: LayerProperties {
                    role: LayerRole::Guide,
                    parent: guided.properties.parent.clone(),
                    end_frame: guided.properties.end_frame,
                    ..Default::default()
                },
            };
            timeline.layers[l].properties.motion_guide = Some(id);
            timeline.layers.insert(l + 1, guide);
            s.layer = l + 1;
            s.clear_selection();
            s.project.format_version = s.project.format_version.max(6);
        });
        self.message("Draw the motion path on the guide layer. Classic tweens on the guided layer follow its first stroke.");
    }

    /// Clear keyframe (⇧F6): the keyframe under the playhead becomes an ordinary frame holding the
    /// previous keyframe's content (the first keyframe of a layer becomes blank).
    pub(crate) fn clear_keyframe(&mut self) {
        self.finish_edits();
        let frame = self.frame;
        let has_key = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .is_some_and(|layer| layer.keyframes.iter().any(|k| k.frame == frame));
        if !self.layer_editable() || !has_key {
            self.message("Select a keyframe to clear.");
            return;
        }
        self.transact(|s| {
            let l = s.layer;
            let layer = &mut s.symbol_mut().timeline.layers[l];
            if layer.keyframes.iter().any(|k| k.frame < frame) {
                layer.keyframes.retain(|k| k.frame != frame);
                layer.normalize_motion_keys();
            } else if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == frame) {
                *key = Keyframe {
                    frame,
                    ..Default::default()
                };
            }
            s.clear_selection();
        });
    }

    /// The selected frame range, or the frame under the playhead on the current layer.
    fn frame_range_or_current(&self) -> Option<FrameRange> {
        self.frame_range().or_else(|| {
            let layer = self.symbol().timeline.layers.get(self.layer)?;
            (layer.properties.role != LayerRole::Folder).then(|| FrameRange {
                layers: vec![layer.id.clone()],
                start: self.frame,
                end: self.frame,
            })
        })
    }

    /// Convert to keyframes / Convert to blank keyframes over the selection (or current frame).
    pub(crate) fn convert_to_keyframes(&mut self, blank: bool) {
        self.finish_edits();
        if let Some(range) = self.frame_range_or_current() {
            self.apply_frame_edit(TimelineEdit::ConvertToKeyframes { range, blank });
        }
    }

    pub(crate) fn can_reverse_frames(&self) -> bool {
        self.frame_range()
            .is_some_and(|range| range.end > range.start)
    }

    /// Reverse frames over the selected range (two or more frames).
    pub(crate) fn reverse_frames(&mut self) {
        self.finish_edits();
        match self.frame_range() {
            Some(range) if range.end > range.start => {
                self.apply_frame_edit(TimelineEdit::Reverse { range });
            }
            _ => self.message("Select two or more frames to reverse."),
        }
    }

    /// Scenes in play order with their names; a document without scenes has one.
    pub(crate) fn scene_list(&self) -> Vec<(Id, String)> {
        self.project
            .scene_ids()
            .into_iter()
            .map(|id| {
                let name = self
                    .project
                    .symbol(&id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                (id, name)
            })
            .collect()
    }

    /// The scene being edited (the bottom of the edit path).
    pub(crate) fn current_scene(&self) -> &str {
        self.editing
            .first()
            .map_or(self.project.root.as_str(), String::as_str)
    }

    /// Leave symbol editing and show scene `id` at its first frame.
    pub(crate) fn switch_scene(&mut self, id: &str) {
        if !self.project.is_scene(id) {
            return;
        }
        self.finish_edits();
        self.timeline_key_drag = None;
        self.editing = vec![id.to_owned()];
        self.reset_authored_preview();
        self.frame = 0;
        self.layer = 0;
        self.clear_selection();
    }

    /// Insert ▸ Scene and Scene select ▸ New scene: append "Scene n" and switch to it. One Undo step.
    pub(crate) fn new_scene(&mut self) {
        self.finish_edits();
        let names: std::collections::HashSet<_> = self
            .project
            .symbols
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut n = self.project.scene_ids().len() + 1;
        while names.contains(&format!("Scene {n}")) {
            n += 1;
        }
        let id = new_id("symbol");
        let frame_count = self
            .project
            .symbol(&self.project.root)
            .map_or(1, |s| s.timeline.frame_count);
        let scene = Symbol {
            id: id.clone(),
            name: format!("Scene {n}"),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: Timeline {
                frame_count,
                layers: vec![Layer {
                    id: new_id("layer"),
                    name: "Layer 1".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe::default()],
                    properties: Default::default(),
                }],
                scripts: vec![],
                guides: vec![],
            },
            class_name: None,
            linkage_id: None,
        };
        self.transact(|s| {
            if s.project.scenes.is_empty() {
                s.project.scenes.push(s.project.root.clone());
            }
            s.project.scenes.push(id.clone());
            s.project.symbols.push(scene);
            s.project.format_version = s.project.format_version.max(6);
        });
        self.switch_scene(&id);
    }

    /// Delete the current scene (not the last one). The next scene, or the previous, is shown.
    /// Its entry point is the Scene panel (PP-08), which is still a planned preview.
    #[allow(dead_code)]
    pub(crate) fn delete_scene(&mut self) {
        let scenes = self.project.scene_ids();
        if scenes.len() < 2 {
            self.message("A document needs at least one scene.");
            return;
        }
        let current = self.current_scene().to_owned();
        let index = scenes.iter().position(|id| id == &current).unwrap_or(0);
        self.finish_edits();
        self.transact(|s| {
            s.project.scenes.retain(|id| id != &current);
            s.project.symbols.retain(|symbol| symbol.id != current);
            s.project.root = s.project.scenes[0].clone();
            if s.project.scenes.len() == 1 {
                s.project.scenes.clear();
            }
        });
        let scenes = self.project.scene_ids();
        let next = scenes[index.min(scenes.len() - 1)].clone();
        self.switch_scene(&next);
    }

    /// Editor transport after one tick: end-of-timeline handling for Loop playback and
    /// Play all scenes. `before` is the frame shown before the tick.
    pub(crate) fn after_playback_tick(&mut self, before: u32) {
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        let wrapped = self.frame < before;
        let at_end = before == last && (wrapped || last == 0);
        if self.editing.len() == 1 && self.timeline_edit.play_all_scenes && at_end {
            let scenes = self.project.scene_ids();
            let index = scenes.iter().position(|id| id == self.current_scene());
            let next = index.and_then(|i| scenes.get(i + 1)).cloned();
            let target = match next {
                Some(next) => Some(next),
                None if self.workspace.timeline.loop_playback && scenes.len() > 1 => {
                    Some(scenes[0].clone())
                }
                None => None,
            };
            if let Some(target) = target {
                self.switch_scene(&target);
                self.playing = true;
                return;
            }
        }
        if !self.workspace.timeline.loop_playback && (wrapped || self.frame == last) {
            self.frame = last;
            self.playing = false;
            self.reset_authored_preview();
        }
    }

    /// Play from the start when playback with Loop off would begin on the last frame.
    pub(crate) fn rewind_if_finished(&mut self) {
        let last = self.symbol().timeline.frame_count.saturating_sub(1);
        if !self.workspace.timeline.loop_playback && self.frame >= last && last > 0 {
            self.reset_authored_preview();
            self.frame = 0;
        }
    }

    /// Snap a scrubbed frame to a keyframe of the current layer when the pointer is within
    /// 4 pt of that keyframe's cell (Frame view ▸ Snap playhead to keyframes; ⇧ suspends it).
    pub(crate) fn snap_scrub_frame(
        &self,
        frame: u32,
        pointer_x: f32,
        origin_x: f32,
        cell: f32,
    ) -> u32 {
        let Some(layer) = self.symbol().timeline.layers.get(self.layer) else {
            return frame;
        };
        layer
            .keyframes
            .iter()
            .map(|key| key.frame)
            .filter(|&key| {
                let left = origin_x + key as f32 * cell - 4.;
                let right = origin_x + (key + 1) as f32 * cell + 4.;
                (left..=right).contains(&pointer_x)
            })
            .min_by_key(|&key| key.abs_diff(frame))
            .unwrap_or(frame)
    }

    /// Edit multiple frames: frames of keyframes inside the onion range on the given layers.
    pub(crate) fn edit_multiple_key_frames(&self) -> Vec<u32> {
        let (start, end) = self.onion_range();
        let mut frames: Vec<u32> = self
            .symbol()
            .timeline
            .layers
            .iter()
            .filter(|layer| layer.visible)
            .flat_map(|layer| &layer.keyframes)
            .map(|key| key.frame)
            .filter(|frame| (start..=end).contains(frame))
            .collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// Edit multiple frames: apply a move of selected objects to the same objects on every
    /// keyframe in the onion range (called inside the open Undo step of the move gesture).
    pub(crate) fn move_on_other_keyframes(&mut self, ids: &[(String, [f64; 2])]) {
        if !self.timeline_edit.edit_multiple {
            return;
        }
        let (start, end) = self.onion_range();
        let frame = self.frame;
        for layer in &mut self.symbol_mut().timeline.layers {
            // The current frame's keyframe already moved with the gesture.
            let own = layer.key_at(frame).map(|key| key.frame);
            for key in &mut layer.keyframes {
                if !(start..=end).contains(&key.frame) || Some(key.frame) == own {
                    continue;
                }
                for element in &mut key.elements {
                    if let Some((_, delta)) = ids.iter().find(|(id, _)| id == &element.id) {
                        element.transform.x += delta[0];
                        element.transform.y += delta[1];
                    }
                }
            }
        }
    }
}
