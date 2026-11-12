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

/// A frame number alone is structural; every other non-default keyframe field is authored
/// content and must keep its layer alive during cleanup.
fn keyframe_has_authored_content(key: &Keyframe) -> bool {
    let empty = Keyframe {
        frame: key.frame,
        ..Keyframe::default()
    };
    key != &empty
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
    let fps = f64::from(project.fps);
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
                    if key.keyed_properties.is_none() {
                        continue;
                    }
                    let reference = match old.keyframes.iter().find(|k| k.frame == key.frame) {
                        Some(old_key) if old_key.keyed_properties.is_some() => {
                            old_key.elements.clone()
                        }
                        _ if key.keyed_properties.as_ref().is_some_and(Vec::is_empty) => {
                            sample_layer_at_fps(old, key.frame, fps)
                        }
                        _ => continue,
                    };
                    let mut changed = Vec::<(Id, Vec<TweenProperty>)>::new();
                    for element in &key.elements {
                        let Some(previous) = reference.iter().find(|e| e.id == element.id) else {
                            continue;
                        };
                        let properties = TweenProperty::ALL
                            .into_iter()
                            .filter(|property| {
                                property_changed(&element.transform, &previous.transform, *property)
                            })
                            .collect::<Vec<_>>();
                        if !properties.is_empty() {
                            changed.push((element.id.clone(), properties));
                        }
                    }
                    if changed.is_empty() {
                        continue;
                    }
                    key.materialize_motion_keys();
                    for (element_id, properties) in changed {
                        let object_key = key.ensure_motion_property_key(&element_id);
                        for property in TweenProperty::ALL {
                            if properties.contains(&property)
                                && !object_key.properties.contains(&property)
                            {
                                object_key.properties.push(property);
                            }
                        }
                    }
                    key.sync_motion_key_union();
                }
            }
            let snapshot = layer.clone();
            for key in &mut layer.keyframes {
                if key.keyed_properties.is_none() {
                    continue;
                }
                let shown = sample_layer_at_fps(&snapshot, key.frame, fps);
                let keyed_by_element: Vec<(Id, Vec<TweenProperty>)> = key
                    .elements
                    .iter()
                    .map(|element| {
                        (
                            element.id.clone(),
                            key.motion_key_properties_for(&element.id).to_vec(),
                        )
                    })
                    .collect();
                for element in &mut key.elements {
                    if let Some(pose) = shown.iter().find(|e| e.id == element.id) {
                        let keyed = keyed_by_element
                            .iter()
                            .find(|(element_id, _)| element_id == &element.id)
                            .map_or(&[][..], |(_, properties)| properties.as_slice());
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
                crate::timeline_motion_curve::rebase_motion_paths(before, project);
                record_motion_property_keys(before, project);
                project.prune_motion_guides();
                project.upgrade_format_for_features();
            })
    }

    /// Collect `(layer_index, span_start_frames)` for all editable spans intersecting the active
    /// timeline frame range (or the playhead frame when no range is selected).
    pub(crate) fn target_tween_spans(&self) -> Vec<(usize, Vec<u32>)> {
        let timeline = &self.symbol().timeline;
        let Some(range) = self.frame_range_or_current() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for layer_id in &range.layers {
            let Some(layer_idx) = timeline.layers.iter().position(|l| &l.id == layer_id) else {
                continue;
            };
            let layer = &timeline.layers[layer_idx];
            if layer.properties.role == LayerRole::Folder
                || neoflash_core::layer_effectively_locked(timeline, layer)
                || !neoflash_core::layer_effectively_visible(timeline, layer)
                || !self.is_layer_id_effectively_soloed(&layer.id)
            {
                continue;
            }
            let ord_frames: Vec<u32> = layer
                .keyframes
                .iter()
                .filter(|k| k.keyed_properties.is_none())
                .map(|k| k.frame)
                .collect();
            if ord_frames.is_empty() {
                continue;
            }
            let layer_end = layer
                .properties
                .end_frame
                .unwrap_or(timeline.frame_count)
                .min(timeline.frame_count)
                .saturating_sub(1);
            let mut starts = Vec::new();
            for (idx, &kf) in ord_frames.iter().enumerate() {
                let span_end = ord_frames
                    .get(idx + 1)
                    .map_or(layer_end, |next_f| next_f.saturating_sub(1))
                    .max(kf);
                if kf <= range.end && span_end >= range.start {
                    // When selecting a range that ends on a 1-frame closing keyframe at the end
                    // of the layer, do not treat that closing endpoint as a new span start if an
                    // earlier span in the selection already precedes it.
                    let is_single_frame_tail_cap = idx + 1 == ord_frames.len()
                        && kf == span_end
                        && !starts.is_empty()
                        && range.start < kf;
                    // Also when selecting exact keyframe-to-keyframe `[f_prev..=kf]` where `range.end == kf`
                    // and `range.start == starts[0]`, `kf` is the closing keyframe of the previous span.
                    let is_exact_closing_key =
                        !starts.is_empty() && range.end == kf && range.start == starts[0];
                    if !is_single_frame_tail_cap && !is_exact_closing_key {
                        starts.push(kf);
                    }
                }
            }
            if !starts.is_empty() {
                out.push((layer_idx, starts));
            }
        }
        out
    }

    fn span_tween_kind(layer: &Layer, start_frame: u32) -> Option<TweenKind> {
        let key = layer.keyframes.iter().find(|k| k.frame == start_frame)?;
        key.tween.map(|_| key.tween_kind)
    }

    /// Tween kind of the span(s) under the selection or playhead; `Some(kind)` when all targeted
    /// spans share `kind`, otherwise `None`.
    pub(crate) fn current_tween(&self) -> Option<TweenKind> {
        let targets = self.target_tween_spans();
        if targets.is_empty() {
            let layer = self.symbol().timeline.layers.get(self.layer)?;
            if motion_span(layer, self.frame).is_some() {
                return Some(TweenKind::Motion);
            }
            let key = layer.key_at(self.frame)?;
            return key.tween.map(|_| key.tween_kind);
        }
        let timeline = &self.symbol().timeline;
        let mut common: Option<TweenKind> = None;
        for (layer_idx, starts) in targets {
            let layer = &timeline.layers[layer_idx];
            for start in starts {
                let kind = Self::span_tween_kind(layer, start)?;
                match common {
                    None => common = Some(kind),
                    Some(prev) if prev == kind => {}
                    Some(_) => return None,
                }
            }
        }
        common
    }

    /// First frame of the span under the playhead (◆ property keys belong to their motion span).
    fn span_start(&self) -> Option<u32> {
        let layer = self.symbol().timeline.layers.get(self.layer)?;
        if let Some((start, ..)) = motion_span(layer, self.frame) {
            return Some(start.frame);
        }
        layer.key_at(self.frame).map(|key| key.frame)
    }

    /// Whether a tween of `kind` can be created on the span(s) under the selection or playhead.
    pub(crate) fn can_create_tween(&self) -> bool {
        !self.target_tween_spans().is_empty()
            || (self.layer_editable() && self.span_start().is_some())
    }

    /// Create a tween of `kind` on all spans intersecting the selected frame range (or the span
    /// under the playhead), or switch their kind. One Undo step.
    /// Switching away from a motion tween keeps each ◆ pose as an ordinary keyframe.
    pub(crate) fn create_tween(&mut self, kind: TweenKind) {
        self.finish_edits();
        let mut targets = self.target_tween_spans();
        if targets.is_empty() {
            if !self.layer_editable() {
                return;
            }
            let Some(start) = self.span_start() else {
                self.message("Insert a keyframe (F6) before creating a tween.");
                return;
            };
            targets.push((self.layer, vec![start]));
        }

        let timeline = &self.symbol().timeline;
        match kind {
            TweenKind::Shape => {
                let shapes = |key: &Keyframe| key.elements.iter().filter(|e| is_shape(e)).count();
                let mut valid_targets = Vec::new();
                let mut any_non_shape = false;
                for (layer_idx, starts) in targets {
                    let layer = &timeline.layers[layer_idx];
                    let mut valid_starts = Vec::new();
                    for start in starts {
                        let Some(key) = layer.keyframes.iter().find(|k| k.frame == start) else {
                            continue;
                        };
                        let next = layer
                            .keyframes
                            .iter()
                            .filter(|k| k.frame > start && k.keyed_properties.is_none())
                            .min_by_key(|k| k.frame);
                        if shapes(key) > 0 && next.is_some_and(|next| shapes(next) > 0) {
                            if key.elements.iter().any(|e| !is_shape(e)) {
                                any_non_shape = true;
                            }
                            valid_starts.push(start);
                        }
                    }
                    if !valid_starts.is_empty() {
                        valid_targets.push((layer_idx, valid_starts));
                    }
                }
                if valid_targets.is_empty() {
                    self.message("Shape tweens need shapes on both keyframes. Break symbols and text apart (⌘B), or use a classic tween.");
                    return;
                }
                if any_non_shape {
                    self.message("Only shapes morph in a shape tween; symbols, text and bitmaps on this layer hold still.");
                }
                targets = valid_targets;
            }
            TweenKind::Motion => {
                let mut valid_targets = Vec::new();
                for (layer_idx, starts) in targets {
                    let layer = &timeline.layers[layer_idx];
                    let valid_starts: Vec<u32> = starts
                        .into_iter()
                        .filter(|&start| {
                            layer
                                .keyframes
                                .iter()
                                .find(|k| k.frame == start)
                                .is_some_and(|k| !k.elements.is_empty())
                        })
                        .collect();
                    if !valid_starts.is_empty() {
                        valid_targets.push((layer_idx, valid_starts));
                    }
                }
                if valid_targets.is_empty() {
                    self.message("A motion tween needs at least one object on its keyframe.");
                    return;
                }
                targets = valid_targets;
            }
            TweenKind::Classic => {}
        }

        let tween_label = match kind {
            TweenKind::Classic => "Classic tween",
            TweenKind::Shape => "Shape tween",
            TweenKind::Motion => "Motion tween",
        };
        let classic_ease = self.workspace.default_ease;
        self.transact_named(tween_label, move |s| {
            let layers = &mut s.symbol_mut().timeline.layers;
            for (layer_idx, starts) in &targets {
                let Some(layer) = layers.get_mut(*layer_idx) else {
                    continue;
                };
                for &start in starts {
                    let mut in_span = false;
                    for key in &mut layer.keyframes {
                        if key.frame == start {
                            in_span = true;
                            let default = match kind {
                                TweenKind::Classic => classic_ease,
                                _ => Easing::Linear,
                            };
                            key.tween = Some(key.tween.unwrap_or(default));
                            key.tween_kind = kind;
                            if kind != TweenKind::Motion {
                                key.motion_tracks.clear();
                                key.motion_keys = None;
                                key.property_eases.clear();
                                key.motion_path = None;
                                key.orient_to_path = false;
                                key.sync_symbols = false;
                                key.snap_to_guide = false;
                                key.roving = false;
                            } else {
                                key.classic_rotate_turns = 0;
                                key.classic_rotate_dir = ClassicRotateDirection::Auto;
                                key.tween_scale = true;
                            }
                        } else if in_span && key.frame > start && key.keyed_properties.is_some() {
                            if kind != TweenKind::Motion {
                                key.keyed_properties = None;
                                key.motion_keys = None;
                                key.motion_tracks.clear();
                                key.tween_kind = kind;
                                key.property_eases.clear();
                                key.motion_path = None;
                                key.orient_to_path = false;
                                key.sync_symbols = false;
                                key.snap_to_guide = false;
                                key.roving = false;
                            }
                        } else if key.frame > start {
                            in_span = false;
                        }
                    }
                }
                layer.keyframes.sort_by_key(|k| k.frame);
            }
            s.project.upgrade_format_for_features();
        });
    }

    /// Remove the tween of all spans intersecting the selected frame range (or under the playhead);
    /// a motion tween also drops its ◆ keys.
    pub(crate) fn remove_tween(&mut self) {
        self.finish_edits();
        let mut targets = self.target_tween_spans();
        if targets.is_empty() {
            if !self.layer_editable() {
                return;
            }
            let Some(start) = self.span_start() else {
                return;
            };
            targets.push((self.layer, vec![start]));
        }
        self.transact_named("Remove tween", move |s| {
            let layers = &mut s.symbol_mut().timeline.layers;
            for (layer_idx, starts) in &targets {
                let Some(layer) = layers.get_mut(*layer_idx) else {
                    continue;
                };
                layer.keyframes.sort_by_key(|k| k.frame);
                for &start in starts {
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
                        key.motion_tracks.clear();
                        key.motion_keys = None;
                        key.orient_to_path = false;
                        key.property_eases.clear();
                        key.motion_path = None;
                        key.sync_symbols = false;
                        key.snap_to_guide = false;
                        key.roving = false;
                    }
                }
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
    /// Like Flash, any normal layer can get a new motion guide, also when it is locked or hidden
    /// (its artwork is not edited) or already guided (the new guide replaces the link).
    pub(crate) fn can_add_motion_guide_to(&self, index: usize) -> bool {
        self.symbol()
            .timeline
            .layers
            .get(index)
            .is_some_and(|layer| layer.properties.role == LayerRole::Normal)
    }

    /// Insert a guide layer "Guide: <name>" above the current layer and guide the current layer
    /// by it. The new guide becomes the current layer so its path can be drawn. One Undo step.
    pub(crate) fn add_motion_guide(&mut self) {
        self.finish_edits();
        if !self.can_add_motion_guide_to(self.layer) {
            self.message(
                "Select a normal layer (not a folder, guide or mask) to add a motion guide.",
            );
            return;
        }
        self.transact_named("Add motion guide", |s| {
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

    /// Clear keyframe (⇧F6): the keyframes in the selected frame range (or under the playhead)
    /// become ordinary frames holding the previous keyframe's content (the first keyframe of a
    /// layer becomes blank).
    pub(crate) fn clear_keyframe(&mut self) {
        self.finish_edits();
        let Some(range) = self.frame_range_or_current() else {
            self.message("Select a keyframe to clear.");
            return;
        };
        let timeline = &self.symbol().timeline;
        let editable_layers: Vec<usize> = range
            .layers
            .iter()
            .filter_map(|id| timeline.layers.iter().position(|l| &l.id == id))
            .filter(|&idx| {
                let l = &timeline.layers[idx];
                l.properties.role != LayerRole::Folder
                    && !neoflash_core::layer_effectively_locked(timeline, l)
                    && neoflash_core::layer_effectively_visible(timeline, l)
            })
            .collect();
        if editable_layers.is_empty()
            || !editable_layers.iter().any(|&idx| {
                timeline.layers[idx]
                    .keyframes
                    .iter()
                    .any(|k| k.frame >= range.start && k.frame <= range.end)
            })
        {
            self.message("Select a keyframe to clear.");
            return;
        }
        let (start, end) = (range.start, range.end);
        self.transact_named("Clear keyframe", |s| {
            for &l in &editable_layers {
                let layer = &mut s.symbol_mut().timeline.layers[l];
                if !layer
                    .keyframes
                    .iter()
                    .any(|k| k.frame >= start && k.frame <= end)
                {
                    continue;
                }
                if layer.keyframes.iter().any(|k| k.frame < start) {
                    layer.keyframes.retain(|k| k.frame < start || k.frame > end);
                } else if let Some(first_frame) = layer.keyframes.first().map(|k| k.frame) {
                    layer
                        .keyframes
                        .retain(|k| k.frame == first_frame || k.frame > end);
                    if let Some(key) = layer.keyframes.iter_mut().find(|k| k.frame == first_frame) {
                        *key = Keyframe {
                            frame: first_frame,
                            ..Default::default()
                        };
                    }
                }
                layer.normalize_motion_keys();
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

    /// Synchronise symbols is available on an editable layer (it works on the selected frames or the
    /// frame under the playhead).
    pub(crate) fn can_sync_symbols(&self) -> bool {
        self.frame_range().is_some() || self.layer_editable()
    }

    /// Synchronise symbols (Frame menu, Edit / Modify ▸ Timeline): graphic instances on the selected
    /// keyframes continue the nested animation of the keyframe before them. One Undo step.
    pub(crate) fn sync_symbols(&mut self) {
        self.finish_edits();
        if let Some(range) = self.frame_range_or_current() {
            self.apply_frame_edit(TimelineEdit::SyncSymbols { range });
        }
    }

    pub(crate) fn layer_blend_mode(&self, layer_index: usize) -> BlendMode {
        let timeline = &self.symbol().timeline;
        timeline
            .layers
            .get(layer_index)
            .map_or(BlendMode::Normal, |l| {
                neoflash_core::layer_effective_blend_mode(timeline, l)
            })
    }

    pub(crate) fn set_layer_blend_mode(
        &mut self,
        layer_index: usize,
        blend_mode: BlendMode,
    ) -> bool {
        let Some(layer) = self.symbol().timeline.layers.get(layer_index) else {
            return false;
        };
        if layer.properties.role == LayerRole::Guide {
            return false;
        }
        let targets = self.target_layer_indices(layer_index);
        let any_changed = targets.iter().any(|&i| {
            self.symbol().timeline.layers.get(i).is_some_and(|l| {
                l.properties.role != LayerRole::Guide
                    && (l.properties.blend_mode != blend_mode
                        || l.keyframes
                            .iter()
                            .flat_map(|kf| &kf.elements)
                            .any(|el| !el.blend_mode.is_normal()))
            })
        });
        if !any_changed {
            return true;
        }
        self.finish_edits();
        self.transact(move |editor| {
            let layers = &mut editor.symbol_mut().timeline.layers;
            for i in targets {
                if let Some(layer) = layers.get_mut(i)
                    && layer.properties.role != LayerRole::Guide
                {
                    layer.properties.blend_mode = blend_mode;
                    for kf in &mut layer.keyframes {
                        for el in &mut kf.elements {
                            el.blend_mode = BlendMode::Normal;
                        }
                    }
                }
            }
            if !blend_mode.is_normal() {
                editor.project.format_version = editor.project.format_version.max(8);
            }
        });
        true
    }

    pub(crate) fn layer_sync_timeline(&self, layer_index: usize) -> bool {
        self.symbol()
            .timeline
            .layers
            .get(layer_index)
            .is_some_and(|l| l.properties.sync_timeline)
    }

    pub(crate) fn set_layer_sync_timeline(&mut self, layer_index: usize, sync: bool) -> bool {
        let Some(layer) = self.symbol().timeline.layers.get(layer_index) else {
            return false;
        };
        if layer.properties.role == LayerRole::Guide {
            return false;
        }
        let targets = self.target_layer_indices(layer_index);
        let any_changed = targets.iter().any(|&i| {
            self.symbol().timeline.layers.get(i).is_some_and(|l| {
                l.properties.role != LayerRole::Guide && l.properties.sync_timeline != sync
            })
        });
        if !any_changed {
            return true;
        }
        self.finish_edits();
        self.transact(move |editor| {
            let layers = &mut editor.symbol_mut().timeline.layers;
            for i in targets {
                if let Some(layer) = layers.get_mut(i)
                    && layer.properties.role != LayerRole::Guide
                {
                    layer.properties.sync_timeline = sync;
                }
            }
            if sync {
                editor.project.format_version = editor.project.format_version.max(8);
            }
        });
        true
    }

    /// Active soloed layer IDs on the current symbol timeline (`R-045`), ignoring deleted layers.
    pub(crate) fn active_solo_layer_ids(&self) -> std::collections::BTreeSet<Id> {
        let Some(raw) = self.solo_layers.get(self.symbol_id()) else {
            return std::collections::BTreeSet::new();
        };
        let layers = &self.symbol().timeline.layers;
        raw.iter()
            .filter(|id| layers.iter().any(|l| &l.id == *id))
            .cloned()
            .collect()
    }

    pub(crate) fn has_any_solo_layers(&self) -> bool {
        !self.active_solo_layer_ids().is_empty()
    }

    pub(crate) fn is_layer_soloed(&self, layer_index: usize) -> bool {
        let Some(layer) = self.symbol().timeline.layers.get(layer_index) else {
            return false;
        };
        self.solo_layers
            .get(self.symbol_id())
            .is_some_and(|set| set.contains(&layer.id))
    }

    pub(crate) fn is_layer_id_effectively_soloed(&self, layer_id: &str) -> bool {
        let solo = self.active_solo_layer_ids();
        if solo.is_empty() {
            return true;
        }
        let timeline = &self.symbol().timeline;
        let mut seen = std::collections::HashSet::from([layer_id]);
        let mut current = timeline.layers.iter().find(|l| l.id == layer_id);
        while let Some(layer) = current {
            if solo.contains(&layer.id) {
                return true;
            }
            let Some(parent_id) = layer.properties.parent.as_deref() else {
                break;
            };
            if !seen.insert(parent_id) {
                break;
            }
            current = timeline.layers.iter().find(|l| l.id == parent_id);
        }
        false
    }

    #[cfg(test)]
    pub(crate) fn is_layer_effectively_soloed(&self, layer_index: usize) -> bool {
        self.symbol()
            .timeline
            .layers
            .get(layer_index)
            .is_some_and(|l| self.is_layer_id_effectively_soloed(&l.id))
    }

    /// Toggle a layer's solo state (`R-045`): click toggles `layer_index` in the solo set;
    /// `⌥`-click (`exclusive_alt = true`) solos only `layer_index` (or clears solo when it is
    /// already the sole soloed layer). Transient view state: never mutates `project` or Undo.
    pub(crate) fn toggle_layer_solo(&mut self, layer_index: usize, exclusive_alt: bool) -> bool {
        let Some(layer_id) = self
            .symbol()
            .timeline
            .layers
            .get(layer_index)
            .map(|l| l.id.clone())
        else {
            return false;
        };
        let existing_ids: std::collections::HashSet<Id> = self
            .symbol()
            .timeline
            .layers
            .iter()
            .map(|l| l.id.clone())
            .collect();
        let symbol_id = self.symbol_id().to_owned();
        let set = self.solo_layers.entry(symbol_id).or_default();
        set.retain(|id| existing_ids.contains(id));
        if exclusive_alt {
            if set.len() == 1 && set.contains(&layer_id) {
                set.clear();
            } else {
                set.clear();
                set.insert(layer_id);
            }
        } else if !set.remove(&layer_id) {
            set.insert(layer_id);
        }
        self.finish_edits();
        self.normalize_targets();
        true
    }

    pub(crate) fn clear_all_solo_layers(&mut self) {
        let symbol_id = self.symbol_id().to_owned();
        if let Some(set) = self.solo_layers.get_mut(&symbol_id) {
            set.clear();
        }
    }

    /// Ephemeral `Project` view for Stage evaluation when any layer on the active timeline is
    /// soloed (`R-045`). Non-soloed layers are hidden (`visible = false`) on the copy while
    /// ancestor folders and referenced masks/guides of soloed layers retain their visibility.
    pub(crate) fn project_for_stage_eval(&self) -> std::borrow::Cow<'_, Project> {
        self.project_for_symbol_stage_eval(self.symbol_id())
    }

    /// Ephemeral single-`Symbol` override when any layer on `symbol_id` is soloed (`R-045`).
    /// Clones only the single target `Symbol` (never `self.project.resources` or other symbols)
    /// and hides non-soloed layers while preserving ancestor folders and referenced masks/guides.
    pub(crate) fn solo_symbol_override(
        &self,
        symbol_id: &str,
    ) -> Option<(Id, neoflash_core::Symbol)> {
        let symbol = self.project.symbol(symbol_id)?;
        let timeline = &symbol.timeline;
        let solo: std::collections::BTreeSet<Id> = self
            .solo_layers
            .get(symbol_id)
            .into_iter()
            .flatten()
            .filter(|id| timeline.layers.iter().any(|layer| &layer.id == *id))
            .cloned()
            .collect();
        if solo.is_empty() {
            return None;
        }
        let effectively_soloed = |layer_id: &str| {
            let mut seen = std::collections::HashSet::from([layer_id]);
            let mut current = timeline.layers.iter().find(|layer| layer.id == layer_id);
            while let Some(layer) = current {
                if solo.contains(&layer.id) {
                    return true;
                }
                let Some(parent_id) = layer.properties.parent.as_deref() else {
                    break;
                };
                if !seen.insert(parent_id) {
                    break;
                }
                current = timeline
                    .layers
                    .iter()
                    .find(|candidate| candidate.id == parent_id);
            }
            false
        };
        let mut soloed_ids = std::collections::HashSet::new();
        let mut needed_ids = std::collections::HashSet::new();
        for layer in &timeline.layers {
            if effectively_soloed(&layer.id) {
                soloed_ids.insert(layer.id.clone());
                if let Some(mask_id) = &layer.properties.mask {
                    needed_ids.insert(mask_id.clone());
                }
                if let Some(guide_id) = &layer.properties.motion_guide {
                    needed_ids.insert(guide_id.clone());
                }
                let mut seen = std::collections::HashSet::from([layer.id.as_str()]);
                let mut cur = Some(layer);
                while let Some(l) = cur {
                    let Some(pid) = l.properties.parent.as_deref() else {
                        break;
                    };
                    if !seen.insert(pid) {
                        break;
                    }
                    needed_ids.insert(pid.to_owned());
                    cur = timeline.layers.iter().find(|cand| cand.id == pid);
                }
            }
        }
        let mut sym_copy = symbol.clone();
        for layer in &mut sym_copy.timeline.layers {
            if !soloed_ids.contains(&layer.id) && !needed_ids.contains(&layer.id) {
                layer.visible = false;
            }
        }
        Some((symbol.id.clone(), sym_copy))
    }

    /// Active symbol solo override map for `evaluate_scene_with_symbol_overrides` (zero `Project::clone()`).
    pub(crate) fn solo_symbol_overrides(
        &self,
    ) -> std::collections::HashMap<Id, neoflash_core::Symbol> {
        self.solo_symbol_override(self.symbol_id())
            .into_iter()
            .collect()
    }

    /// The same transient solo projection for a frozen parent timeline while a
    /// nested symbol is active. Solo state is keyed by symbol, not by the current
    /// breadcrumb level, so parent context must not accidentally use the child's set.
    pub(crate) fn project_for_symbol_stage_eval(
        &self,
        symbol_id: &str,
    ) -> std::borrow::Cow<'_, Project> {
        let Some((id, sym_override)) = self.solo_symbol_override(symbol_id) else {
            return std::borrow::Cow::Borrowed(&self.project);
        };
        let mut copy = self.project.clone();
        if let Some(sym) = copy.symbol_mut(&id) {
            *sym = sym_override;
        }
        std::borrow::Cow::Owned(copy)
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
        self.edit_session
            .levels
            .first()
            .map_or(self.project.root.as_str(), |level| level.symbol.as_str())
    }

    /// Leave symbol editing and show scene `id` at its first frame.
    pub(crate) fn switch_scene(&mut self, id: &str) {
        if !self.project.is_scene(id) {
            return;
        }
        self.reset_symbol_edit_session(id.to_owned());
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
        self.transact_named("New scene", |s| {
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
    /// Scene panel ▸ Delete scene. One Undo step.
    pub(crate) fn delete_scene(&mut self) {
        let scenes = self.project.scene_ids();
        if scenes.len() < 2 {
            self.message("A document needs at least one scene.");
            return;
        }
        let current = self.current_scene().to_owned();
        let index = scenes.iter().position(|id| id == &current).unwrap_or(0);
        self.finish_edits();
        self.transact_named("Delete scene", |s| {
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
        if self.editing_scene() && self.timeline_edit.play_all_scenes && at_end {
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
        // Leave the last frame visible for its tick before advancing to another scene.
        // Loop off stops only when the final scene (or the current scene in single-scene mode) ends.
        let has_next_scene = self.editing_scene()
            && self.timeline_edit.play_all_scenes
            && self
                .project
                .scene_ids()
                .windows(2)
                .any(|pair| pair[0] == self.current_scene());
        if !self.workspace.timeline.loop_playback
            && !has_next_scene
            && (wrapped || self.frame == last)
        {
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
    #[cfg(test)]
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

    /// Edit multiple frames: apply a scale factor and rotation delta to the selected objects across
    /// every keyframe in `onion_range()` (or only the current frame when `edit_multiple` is off) in
    /// one Undo step.
    #[cfg(test)]
    pub(crate) fn scale_and_rotate_selected_in_edit_range(
        &mut self,
        scale_factor: [f64; 2],
        rotation_delta_deg: f64,
    ) {
        self.finish_edits();
        let targets = self.sampled_targets();
        if targets.is_empty() {
            return;
        }
        let multiple = self.timeline_edit.edit_multiple;
        let (start, end) = if multiple {
            self.onion_range()
        } else {
            (self.frame, self.frame)
        };
        let selected: std::collections::HashSet<(Id, Id)> = targets
            .into_iter()
            .map(|(t, _)| (t.layer, t.element))
            .collect();
        self.transact_named("Transform", |editor| {
            let frame = editor.frame;
            if !multiple {
                let layers: Vec<Id> = selected.iter().map(|(l, _)| l.clone()).collect();
                for layer_id in layers {
                    editor.ensure_key_for(&layer_id, false);
                }
            }
            for layer in &mut editor.symbol_mut().timeline.layers {
                let layer_id = layer.id.clone();
                if multiple {
                    for key in &mut layer.keyframes {
                        if !(start..=end).contains(&key.frame) {
                            continue;
                        }
                        for element in &mut key.elements {
                            if selected.contains(&(layer_id.clone(), element.id.clone())) {
                                element.transform.scale_x *= scale_factor[0];
                                element.transform.scale_y *= scale_factor[1];
                                element.transform.rotation += rotation_delta_deg;
                            }
                        }
                    }
                } else if let Some(key) = layer.key_at_mut(frame) {
                    for element in &mut key.elements {
                        if selected.contains(&(layer_id.clone(), element.id.clone())) {
                            element.transform.scale_x *= scale_factor[0];
                            element.transform.scale_y *= scale_factor[1];
                            element.transform.rotation += rotation_delta_deg;
                        }
                    }
                }
            }
        });
    }

    /// Add a shape hint to the active shape span, including its ending keyframe.
    pub(crate) fn add_shape_hint(&mut self) -> Option<char> {
        self.finish_edits();
        if !self.can_add_shape_hint() {
            self.message(
                "Shape hints require a shape tween with artwork and at most 26 hints (a–z).",
            );
            return None;
        }
        let (key_idx, end_idx) = self.shape_hint_add_span()?;
        let layer_idx = self.layer;
        let start_frame = self.symbol().timeline.layers[layer_idx].keyframes[key_idx].frame;
        let end_frame = self.symbol().timeline.layers[layer_idx].keyframes[end_idx].frame;
        let key = &self.symbol().timeline.layers[layer_idx].keyframes[key_idx];
        let label = ('a'..='z').find(|&ch| !key.shape_hints.iter().any(|h| h.label == ch))?;
        let uv = default_shape_hint_uv(label);
        let hint = ShapeHint {
            label,
            start_uv: uv,
            end_uv: uv,
        };
        self.transact_named("Add shape hint", |editor| {
            editor.symbol_mut().timeline.layers[layer_idx].keyframes[key_idx]
                .shape_hints
                .push(hint);
        });
        if self.frame != start_frame && self.frame != end_frame {
            self.frame = start_frame;
            self.normalize_targets();
            self.reset_authored_preview();
        }
        self.timeline_shape_hints.show = true;
        Some(label)
    }

    /// Remove a single shape hint by label on the active shape span in one Undo step.
    #[cfg(test)]
    pub(crate) fn remove_shape_hint(&mut self, label: char) -> bool {
        self.finish_edits();
        let Some((key_idx, _)) = self.shape_hint_span() else {
            return false;
        };
        let start_frame = self.symbol().timeline.layers[self.layer].keyframes[key_idx].frame;
        self.remove_shape_hint_in_span(start_frame, label)
    }

    /// Remove a single shape hint by label on the span starting at `start_frame` in one Undo step.
    pub(crate) fn remove_shape_hint_in_span(&mut self, start_frame: u32, label: char) -> bool {
        self.finish_edits();
        let layer_idx = self.layer;
        let Some(key_idx) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.keyframes.iter().position(|k| k.frame == start_frame))
        else {
            return false;
        };
        if !self.symbol().timeline.layers[layer_idx].keyframes[key_idx]
            .shape_hints
            .iter()
            .any(|h| h.label == label)
        {
            return false;
        }
        self.transact_named("Remove shape hint", |editor| {
            editor.symbol_mut().timeline.layers[layer_idx].keyframes[key_idx]
                .shape_hints
                .retain(|h| h.label != label);
        });
        true
    }

    /// Remove both endpoints of every hint on the current shape span in one Undo step.
    pub(crate) fn remove_shape_hints(&mut self) -> bool {
        self.finish_edits();
        let Some((key_idx, _)) = self.shape_hint_span() else {
            return false;
        };
        let start_frame = self.symbol().timeline.layers[self.layer].keyframes[key_idx].frame;
        self.remove_shape_hints_in_span(start_frame)
    }

    /// Remove all shape hints on the span starting at `start_frame` in one Undo step.
    pub(crate) fn remove_shape_hints_in_span(&mut self, start_frame: u32) -> bool {
        self.finish_edits();
        let layer_idx = self.layer;
        let Some(key_idx) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.keyframes.iter().position(|k| k.frame == start_frame))
        else {
            return false;
        };
        if self.symbol().timeline.layers[layer_idx].keyframes[key_idx]
            .shape_hints
            .is_empty()
        {
            return false;
        }
        self.transact_named("Remove shape hints", |editor| {
            editor.symbol_mut().timeline.layers[layer_idx].keyframes[key_idx]
                .shape_hints
                .clear();
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_classic_tween_rotate(
        &mut self,
        turns: u32,
        dir: neoflash_core::ClassicRotateDirection,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let turns = turns.min(Keyframe::MAX_ROTATE_TURNS);
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if key.tween.is_none() || key.tween_kind != TweenKind::Classic {
            return false;
        }
        if key.classic_rotate_turns == turns && key.classic_rotate_dir == dir {
            return false;
        }
        let key_frame = key.frame;
        self.transact_named("Set classic tween rotation", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.classic_rotate_turns = turns;
                k.classic_rotate_dir = dir;
            }
            layer.normalize_motion_keys();
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_classic_tween_scale(&mut self, tween_scale: bool) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if key.tween.is_none()
            || key.tween_kind != TweenKind::Classic
            || key.tween_scale == tween_scale
        {
            return false;
        }
        let key_frame = key.frame;
        self.transact_named("Set classic tween scale", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.tween_scale = tween_scale;
            }
            layer.normalize_motion_keys();
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_classic_tween_sync_symbols(&mut self, sync: bool) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if key.tween.is_none() || key.tween_kind != TweenKind::Classic || key.sync_symbols == sync {
            return false;
        }
        let key_frame = key.frame;
        self.transact_named("Sync classic tween symbols", |editor| {
            let sym_id = editor.symbol_id().to_owned();
            let project_snapshot = editor.project.clone();
            if let Some(symbol) = editor.project.symbol_mut(&sym_id) {
                let frame_count = symbol.timeline.frame_count;
                let layer = &mut symbol.timeline.layers[layer_idx];
                if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                    k.sync_symbols = sync;
                }
                layer.normalize_motion_keys();
                if sync {
                    let end_frame = layer
                        .keyframes
                        .iter()
                        .filter(|k| k.frame > key_frame)
                        .map(|k| k.frame)
                        .min()
                        .unwrap_or_else(|| frame_count.saturating_sub(1));
                    neoflash_core::sync_layer_symbols(
                        &project_snapshot,
                        layer,
                        key_frame,
                        end_frame,
                    );
                }
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn toggle_roving_keyframe(&mut self) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        if self.selection.symbol != self.symbol_id() {
            return false;
        }
        let Some(target) = self.selection.single() else {
            return false;
        };
        if self.target_layer_index(target) != Some(layer_idx) {
            return false;
        }
        let element_id = target.element.clone();
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.keyframes.iter().find(|k| k.frame == frame))
        else {
            return false;
        };
        let keyed = key.motion_key_properties_for(&element_id);
        let is_pos_key = keyed.contains(&TweenProperty::X) || keyed.contains(&TweenProperty::Y);
        if !is_pos_key {
            return false;
        }
        let next_roving = !key.motion_key_roving_for(&element_id);
        self.transact_named("Toggle roving keyframe", move |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == frame) {
                k.ensure_motion_property_key(&element_id).roving = next_roving;
                k.sync_motion_key_union();
            }
            layer.normalize_motion_keys();
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_instance_property_override(
        &mut self,
        ov: neoflash_core::InstancePropertyOverride,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set instance property override", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.set_instance_override(ov);
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_element_asset_warp(&mut self, rig: neoflash_core::AssetWarpRig) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set Asset Warp", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                if rig.pins.is_empty() {
                    k.asset_warps.retain(|w| w.element_id != rig.element_id);
                } else {
                    k.set_asset_warp(rig);
                }
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reset_element_asset_warp(&mut self, element_id: &str) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if key.asset_warp(element_id).is_none() {
            return false;
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        self.transact_named("Reset Asset Warp", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.reset_asset_warp(&eid);
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn bake_element_asset_warp(&mut self, element_id: &str) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if key.asset_warp(element_id).is_none() {
            return false;
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        self.transact_named("Bake Asset Warp", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.bake_asset_warp(&eid);
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_layer_z_depth(&mut self, z_depth: f64) -> bool {
        self.finish_edits();
        let layer_idx = self.layer;
        if layer_idx >= self.symbol().timeline.layers.len() {
            return false;
        }
        let clamped = z_depth.clamp(-10_000.0, 10_000.0);
        self.transact_named("Set layer Z depth", |editor| {
            editor.symbol_mut().timeline.layers[layer_idx]
                .properties
                .z_depth = clamped;
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_layer_attach_to_camera(&mut self, attach: bool) -> bool {
        self.finish_edits();
        let layer_idx = self.layer;
        if layer_idx >= self.symbol().timeline.layers.len() {
            return false;
        }
        self.transact_named("Set attach layer to camera", |editor| {
            editor.symbol_mut().timeline.layers[layer_idx]
                .properties
                .attach_to_camera = attach;
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_keyframe_camera_pose(
        &mut self,
        pose: Option<neoflash_core::CameraPose>,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set camera pose", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.camera = pose;
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reparent_element_preserving_world(
        &mut self,
        child_id: &str,
        new_parent_id: Option<&str>,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let mut test_key = key.clone();
        if test_key
            .reparent_preserving_world_transform(child_id, new_parent_id)
            .is_err()
        {
            return false;
        }
        let key_frame = key.frame;
        let cid = child_id.to_owned();
        let pid = new_parent_id.map(str::to_owned);
        self.transact_named("Reparent element", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                let _ = k.reparent_preserving_world_transform(&cid, pid.as_deref());
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_element_controller_flag(&mut self, element_id: &str, is_ctrl: bool) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if !key.elements.iter().any(|e| e.id == element_id) {
            return false;
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        self.transact_named("Set controller element", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.controller_elements.retain(|id| id != &eid);
                if is_ctrl {
                    k.controller_elements.push(eid);
                }
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_keyframe_ik_armature(&mut self, armature: neoflash_core::IkArmature) -> bool {
        self.set_keyframe_ik_armature_with_specs(armature, Vec::new())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_keyframe_ik_armature_with_specs(
        &mut self,
        armature: neoflash_core::IkArmature,
        joint_specs: Vec<neoflash_core::IkJointSpec>,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set IK armature", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                neoflash_core::solve_ik_armature_fabrik_ccd(
                    &mut k.elements,
                    &armature,
                    &joint_specs,
                );
                if let Some(slot) = k.ik_armatures.iter_mut().find(|a| a.id == armature.id) {
                    *slot = armature;
                } else {
                    k.ik_armatures.push(armature);
                }
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn bind_shape_to_armature_command(
        &mut self,
        shape_element_id: &str,
        armature_id: &str,
    ) -> Option<neoflash_core::IkShapeSkin> {
        self.finish_edits();
        if !self.layer_editable() {
            return None;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let key = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))?;
        let shape = key.elements.iter().find(|e| e.id == shape_element_id)?;
        let armature = key.ik_armatures.iter().find(|a| a.id == armature_id)?;
        let binding = neoflash_core::auto_bind_shape_to_armature(shape, armature, &key.elements)?;
        let key_frame = key.frame;
        let shape_id = shape_element_id.to_owned();
        let arm_clone = armature.clone();
        let binding_clone = binding.clone();
        self.transact_named("Bind shape to IK armature", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                let posed_elements = k.elements.clone();
                if let Some(el) = k.elements.iter_mut().find(|e| e.id == shape_id) {
                    *el = neoflash_core::deform_shape_with_ik_skin(
                        el,
                        &binding_clone,
                        &arm_clone,
                        &posed_elements,
                    );
                }
            }
        });
        Some(binding)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply_asset_warp_pins_command(
        &mut self,
        element_id: &str,
        pins: Vec<neoflash_core::WarpPin>,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let Some(el) = key.elements.iter().find(|e| e.id == element_id) else {
            return false;
        };
        let rig = neoflash_core::AssetWarpRig {
            element_id: element_id.to_owned(),
            pins,
            envelop_subdivisions: 4,
        };
        if neoflash_core::build_asset_warp_mesh(el, &rig).is_none() {
            return false;
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        let mut applied = false;
        self.transact_named("Puppet asset warp", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                if let Some(slot) = k.asset_warps.iter_mut().find(|r| r.element_id == eid) {
                    *slot = rig.clone();
                } else {
                    k.asset_warps.push(rig.clone());
                }
                if let Some(target_el) = k.elements.iter_mut().find(|e| e.id == eid) {
                    neoflash_core::apply_asset_warp_to_element(target_el, &rig);
                }
                applied = true;
            }
        });
        applied
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply_3d_pose_to_element_command(
        &mut self,
        element_id: &str,
        pose: neoflash_core::Display3dPose,
        focal_length: f64,
        projection_center: [f64; 2],
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if !key.elements.iter().any(|e| e.id == element_id) {
            return false;
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        self.transact_named("Apply 3D transform", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame)
                && let Some(target_el) = k.elements.iter_mut().find(|e| e.id == eid)
            {
                *target_el = neoflash_core::project_element_3d(
                    target_el,
                    &pose,
                    focal_length,
                    projection_center,
                );
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn bake_layer_ik_span(&mut self, start_frame: u32, end_frame: u32) -> usize {
        self.finish_edits();
        if !self.layer_editable() || end_frame < start_frame {
            return 0;
        }
        let layer_idx = self.layer;
        let fps = f64::from(self.project.fps);
        let mut baked = 0usize;
        self.transact_named("Bake IK span to keyframes", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            baked = layer.bake_ik_span_to_keyframes_at_fps(start_frame, end_frame, fps);
        });
        baked
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_element_appearance(
        &mut self,
        appearance: neoflash_core::ElementAppearance,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set element appearance", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.set_appearance(appearance);
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn expand_element_appearance_command(&mut self, element_id: &str) -> Vec<Id> {
        self.finish_edits();
        if !self.layer_editable() {
            return Vec::new();
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return Vec::new();
        };
        if key.appearance(element_id).is_none() {
            return Vec::new();
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        let mut expanded_ids = Vec::new();
        self.transact_named("Expand appearance", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                expanded_ids = k.expand_element_appearance(&eid);
            }
        });
        if let Some(first) = expanded_ids.first() {
            self.select_single_id(first.clone());
        }
        expanded_ids
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn update_global_swatch(
        &mut self,
        swatch_id: &str,
        new_color: neoflash_core::Color,
    ) -> usize {
        self.finish_edits();
        let sid = swatch_id.to_owned();
        let mut updated = 0usize;
        self.transact_named("Update global swatch", |editor| {
            updated = editor.project.update_global_swatch_color(&sid, new_color);
        });
        updated
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply_vector_style(&mut self, style_id: &str) -> usize {
        self.finish_edits();
        let sid = style_id.to_owned();
        let mut updated = 0usize;
        self.transact_named("Apply vector style", |editor| {
            updated = editor.project.apply_vector_style_to_project(&sid);
        });
        updated
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_keyframe_video_span(
        &mut self,
        video: Option<neoflash_core::TimelineVideoSpan>,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        if key.video == video {
            return false;
        }
        let symbol_id = self.symbol_id().to_owned();
        let mut candidate = self.project.clone();
        let Some(layer) = candidate
            .symbol_mut(&symbol_id)
            .and_then(|symbol| symbol.timeline.layers.get_mut(layer_idx))
        else {
            return false;
        };
        let Some(key) = layer
            .keyframes
            .iter_mut()
            .find(|key| key.frame == key_frame)
        else {
            return false;
        };
        if video.is_some() {
            key.keyed_properties = None;
        }
        key.video = video;
        layer.normalize_motion_keys();
        candidate.upgrade_format_for_features();
        if let Err(error) = candidate.validate() {
            self.message(format!("Could not set timeline video span: {error}"));
            return false;
        }
        self.transact_named("Set timeline video span", |editor| {
            editor.project = candidate;
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_document_accessibility_settings(
        &mut self,
        settings: neoflash_core::DocumentAccessibility,
    ) {
        self.finish_edits();
        self.transact_named("Set document accessibility", |editor| {
            editor.project.set_document_accessibility(&settings);
        });
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_element_extended_accessibility(
        &mut self,
        ext: neoflash_core::ExtendedAccessibility,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set extended accessibility", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.set_extended_accessibility(ext);
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn resolve_current_accessibility_tree(
        &self,
    ) -> Vec<neoflash_core::AccessibilityTreeNode> {
        neoflash_core::resolve_frame_accessibility_tree(
            &self.project,
            &self.symbol().timeline,
            self.frame,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn validate_current_accessibility_tree(
        &self,
    ) -> Vec<neoflash_core::AccessibilityDiagnostic> {
        neoflash_core::validate_accessibility_tree(
            &self.project,
            &self.symbol().timeline,
            self.frame,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_element_vector_effects(
        &mut self,
        effects: neoflash_core::ElementVectorEffects,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set vector effects", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                k.set_vector_effects(effects);
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn expand_element_vector_effects_command(&mut self, element_id: &str) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        if key.vector_effects(element_id).is_none() {
            return false;
        }
        let key_frame = key.frame;
        let eid = element_id.to_owned();
        let mut expanded = false;
        self.transact_named("Expand vector effects", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                expanded = k.expand_element_vector_effects(&eid);
            }
        });
        expanded
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_timeline_perspective_grid(
        &mut self,
        grid: Option<neoflash_core::PerspectiveGridSpec>,
    ) {
        self.finish_edits();
        self.transact_named("Set perspective grid", |editor| {
            editor.symbol_mut().timeline.set_perspective_grid(grid);
        });
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_keyframe_property_expression(
        &mut self,
        expr: neoflash_core::PropertyExpression,
    ) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let frame = self.frame;
        let layer_idx = self.layer;
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
        else {
            return false;
        };
        let key_frame = key.frame;
        self.transact_named("Set property expression", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == key_frame) {
                if let Some(slot) = k
                    .property_expressions
                    .iter_mut()
                    .find(|p| p.property == expr.property)
                {
                    *slot = expr;
                } else {
                    k.property_expressions.push(expr);
                }
            }
        });
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply_motion_sketch_to_element(
        &mut self,
        element_id: &str,
        samples: &[neoflash_core::MotionSketchSample],
        smoothing_tolerance: f64,
        smooth_strength: f64,
    ) -> usize {
        self.finish_edits();
        if !self.layer_editable() || samples.is_empty() {
            return 0;
        }
        let fps = self.project.fps as f64;
        let start_frame = self.frame;
        let raw_pts = neoflash_core::capture_motion_sketch_to_keyframes(
            samples,
            fps,
            start_frame,
            smoothing_tolerance,
        );
        let pts = neoflash_core::smooth_motion_keyframes(&raw_pts, smooth_strength);
        if pts.is_empty() {
            return 0;
        }
        let layer_idx = self.layer;
        let eid = element_id.to_owned();
        let mut count = 0usize;
        self.transact_named("Motion sketch capture", |editor| {
            let max_frame = pts.iter().map(|(f, _)| *f).max().unwrap_or(start_frame);
            let sym = editor.symbol_mut();
            sym.timeline.frame_count = sym.timeline.frame_count.max(max_frame + 1);
            let layer = &mut sym.timeline.layers[layer_idx];
            for &(f, pos) in &pts {
                let mut elements = neoflash_core::sample_layer_at_fps(layer, f, fps);
                if let Some(el) = elements.iter_mut().find(|e| e.id == eid) {
                    el.transform.x = pos[0];
                    el.transform.y = pos[1];
                }
                if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == f) {
                    existing.elements = elements;
                } else {
                    layer.keyframes.push(neoflash_core::Keyframe {
                        frame: f,
                        elements,
                        ..Default::default()
                    });
                }
                count += 1;
            }
            layer.normalize_motion_keys();
        });
        count
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn apply_wiggler_to_element(
        &mut self,
        element_id: &str,
        start_frame: u32,
        end_frame: u32,
        config: &neoflash_core::WigglerConfig,
    ) -> usize {
        self.finish_edits();
        if !self.layer_editable() || end_frame < start_frame {
            return 0;
        }
        let layer_idx = self.layer;
        let fps = self.project.fps as f64;
        let eid = element_id.to_owned();
        let cfg = config.clone();
        let mut modified = 0usize;
        self.transact_named("Apply Wiggler", |editor| {
            let sym = editor.symbol_mut();
            sym.timeline.frame_count = sym.timeline.frame_count.max(end_frame + 1);
            let layer = &mut sym.timeline.layers[layer_idx];
            modified = neoflash_core::apply_wiggler_to_keyframes(
                layer,
                &eid,
                start_frame,
                end_frame,
                fps,
                &cfg,
            );
        });
        modified
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_timeline_work_area(
        &mut self,
        work_area: Option<neoflash_core::PreviewWorkArea>,
    ) {
        self.finish_edits();
        self.transact_named("Set preview work area", |editor| {
            editor.symbol_mut().timeline.set_work_area(work_area);
        });
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_timeline_markers(&mut self, markers: Vec<neoflash_core::TimelineMarker>) {
        self.finish_edits();
        self.transact_named("Set timeline markers", |editor| {
            editor.symbol_mut().timeline.set_markers(markers);
        });
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn capture_stage_snapshot_ab(&self, label: &str) -> neoflash_core::StageSnapshotAB {
        let mut elements = Vec::new();
        let tl = &self.symbol().timeline;
        for layer in &tl.layers {
            if layer.visible {
                elements.extend(neoflash_core::sample_layer_in_at_fps(
                    tl,
                    layer,
                    self.frame,
                    f64::from(self.project.fps),
                ));
            }
        }
        neoflash_core::StageSnapshotAB {
            label: label.to_owned(),
            frame: self.frame,
            elements,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn import_act_palette_into_swatches(
        &mut self,
        bytes: &[u8],
        prefix: &str,
    ) -> Result<usize, neoflash_import::ImportError> {
        let imported = neoflash_import::import_act_palette(bytes, prefix)?;
        let count = imported.len();
        self.finish_edits();
        self.transact_named("Import ACT palette", |editor| {
            let mut swatches = editor.project.global_swatches();
            swatches.extend(imported);
            editor.project.set_global_swatches(&swatches);
        });
        Ok(count)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn import_clr_palette_into_swatches(
        &mut self,
        bytes: &[u8],
        prefix: &str,
    ) -> Result<usize, neoflash_import::ImportError> {
        let imported = neoflash_import::import_clr_palette(bytes, prefix)?;
        let count = imported.len();
        self.finish_edits();
        self.transact_named("Import CLR palette", |editor| {
            let mut swatches = editor.project.global_swatches();
            swatches.extend(imported);
            editor.project.set_global_swatches(&swatches);
        });
        Ok(count)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn import_ase_palette_into_swatches(
        &mut self,
        bytes: &[u8],
    ) -> Result<usize, neoflash_import::ImportError> {
        let imported = neoflash_import::import_ase_palette(bytes)?;
        let count = imported.len();
        self.finish_edits();
        self.transact_named("Import ASE palette", |editor| {
            let mut swatches = editor.project.global_swatches();
            swatches.extend(imported);
            editor.project.set_global_swatches(&swatches);
        });
        Ok(count)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn import_fxg_xml_command(
        &mut self,
        xml_text: &str,
        source_name: &str,
    ) -> Result<neoflash_import::ImportReport, neoflash_import::ImportError> {
        self.finish_edits();
        let mut trial = self.project.clone();
        let report = neoflash_import::import_fxg_into_project(&mut trial, xml_text, source_name)?;
        self.transact_named("Import FXG 2.0", |editor| {
            editor.project = trial;
        });
        Ok(report)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn import_flash_screens_xml_command(
        &mut self,
        xml_text: &str,
        source_name: &str,
    ) -> Result<neoflash_import::ImportReport, neoflash_import::ImportError> {
        self.finish_edits();
        let mut trial = self.project.clone();
        let report = neoflash_import::import_flash_screens_xml_into_project(
            &mut trial,
            xml_text,
            source_name,
        )?;
        self.transact_named("Import Flash Screens XML", |editor| {
            editor.project = trial;
        });
        Ok(report)
    }

    pub fn move_key(&mut self, layer: usize, from: u32, to: u32, copy: bool) -> bool {
        let symbol_id = self.symbol_id().to_owned();
        let timeline = &self.symbol().timeline;
        let Some(l) = timeline.layers.get(layer) else {
            return false;
        };
        let layer_editable =
            !layer_effectively_locked(timeline, l) && layer_effectively_visible(timeline, l);
        let is_button = self.symbol().kind == SymbolKind::Button;
        let max_frames = if is_button {
            timeline.frame_count
        } else {
            1_000_000
        };
        if !layer_editable
            || l.properties.role == LayerRole::Folder
            || to >= max_frames
            || from == to
        {
            if self.frame == to && from != to && from < timeline.frame_count {
                self.frame = from;
                self.reset_authored_preview();
            }
            return false;
        }
        if l.keyframes.iter().any(|k| k.frame == to) {
            if self.frame == to && from < timeline.frame_count {
                self.frame = from;
                self.reset_authored_preview();
            }
            self.message("A keyframe already exists at the destination.");
            return false;
        }
        if !l.keyframes.iter().any(|k| k.frame == from) {
            if self.frame == to && from < timeline.frame_count {
                self.frame = from;
                self.reset_authored_preview();
            }
            return false;
        }
        if timeline.key_carries_frame_script(layer, from) && timeline.has_frame_script(to) {
            if self.frame == to && from < timeline.frame_count {
                self.frame = from;
                self.reset_authored_preview();
            }
            self.message(
                "A frame script already exists at the destination. The key and script were not moved.",
            );
            return false;
        }
        let step_label = if copy {
            "Copy keyframe"
        } else {
            "Move keyframe"
        };
        self.transact_named(step_label, |s| {
            let _ = s
                .project
                .move_keyframe(&symbol_id, layer, from, to, copy, true);
            s.frame = to;
            s.layer = layer;
            s.clear_selection();
        });
        self.reset_authored_preview();
        true
    }

    pub(crate) fn can_distribute_to_keyframes(&self) -> bool {
        if self.selection.symbol != self.symbol_id() {
            return false;
        }
        let editable: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter(|(target, _)| self.target_editable(target))
            .collect();
        editable.len() >= 2
            && editable
                .iter()
                .all(|(target, _)| target.layer == editable[0].0.layer)
    }

    /// `Modify ▸ Timeline ▸ Distribute to keyframes`: takes the 2+ selected Stage elements in their
    /// current selection/layer order, keeps the first element on `self.frame`, removes the 2nd..Nth
    /// selected elements from the starting keyframe at `self.frame`, and places each onto its own
    /// consecutive keyframe (`start_frame + 1`, `start_frame + 2`, ...) on that layer, extending
    /// `timeline.frame_count` if needed, in one Undo step.
    pub fn distribute_selection_to_keyframes(&mut self) -> bool {
        self.finish_edits();
        if !self.can_distribute_to_keyframes() {
            return false;
        }
        let sampled_map: std::collections::HashMap<crate::selection::SelectionTarget, Element> =
            self.sampled_targets()
                .into_iter()
                .filter(|(target, _)| self.target_editable(target))
                .collect();
        let mut ordered: Vec<(crate::selection::SelectionTarget, Element)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for target in &self.selection.targets {
            if seen.insert(target.clone())
                && let Some(el) = sampled_map.get(target)
            {
                ordered.push((target.clone(), el.clone()));
            }
        }
        if ordered.len() < 2 {
            return false;
        }
        let target_layer_id = ordered[0].0.layer.clone();
        if ordered
            .iter()
            .any(|(target, _)| target.layer != target_layer_id)
        {
            return false;
        }
        let start_frame = self.frame;
        self.playing = false;
        self.transact_named("Distribute to keyframes", move |editor| {
            let mut touched_layers: Vec<Id> =
                ordered.iter().map(|(t, _)| t.layer.clone()).collect();
            touched_layers.sort();
            touched_layers.dedup();
            for lid in &touched_layers {
                editor.ensure_key_for(lid, false);
            }
            let removed_from_start: std::collections::HashSet<(Id, Id)> = ordered
                .iter()
                .skip(1)
                .map(|(t, _)| (t.layer.clone(), t.element.clone()))
                .collect();
            for lid in &touched_layers {
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| &l.id == lid)
                {
                    if let Some(key) = layer.key_at_mut(start_frame) {
                        key.elements
                            .retain(|e| !removed_from_start.contains(&(lid.clone(), e.id.clone())));
                    }
                    layer.normalize_motion_keys();
                }
            }

            let last_frame = start_frame + (ordered.len() as u32 - 1);
            let old_count = editor.symbol().timeline.frame_count;
            let new_count = old_count.max(last_frame + 1);
            {
                let timeline = &mut editor.symbol_mut().timeline;
                timeline.frame_count = new_count;
                for layer in &mut timeline.layers {
                    if layer.id == target_layer_id {
                        if let Some(end) = &mut layer.properties.end_frame {
                            *end = (*end).max(last_frame + 1);
                        }
                    } else if layer.properties.role == LayerRole::Folder
                        && let Some(end) = &mut layer.properties.end_frame
                    {
                        *end = (*end).max(new_count);
                    }
                }
            }

            let mut new_selection = Vec::with_capacity(ordered.len());
            let mut needs_format_5 = false;
            if let Some(target_layer) = editor
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == target_layer_id)
            {
                new_selection.push(crate::selection::SelectionTarget {
                    layer: target_layer_id.clone(),
                    element: ordered[0].1.id.clone(),
                });
                for (offset, (_, mut el)) in ordered.into_iter().enumerate().skip(1) {
                    let dest_frame = start_frame + offset as u32;
                    if el.transform.skew != 0.0
                        || !el.transform.color_transform.is_identity()
                        || matches!(el.kind, ElementKind::PaintedPath { .. })
                    {
                        needs_format_5 = true;
                    }
                    if target_layer
                        .keyframes
                        .iter()
                        .any(|k| k.frame != dest_frame && k.elements.iter().any(|e| e.id == el.id))
                    {
                        el.id = new_id("object");
                    }
                    new_selection.push(crate::selection::SelectionTarget {
                        layer: target_layer_id.clone(),
                        element: el.id.clone(),
                    });
                    if let Some(existing) = target_layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == dest_frame)
                    {
                        if let Some(slot) = existing
                            .elements
                            .iter_mut()
                            .find(|existing| existing.id == el.id)
                        {
                            *slot = el;
                        } else {
                            existing.elements.push(el);
                        }
                    } else {
                        target_layer.keyframes.push(Keyframe {
                            frame: dest_frame,
                            elements: vec![el],
                            ..Default::default()
                        });
                    }
                }
                target_layer.keyframes.sort_by_key(|k| k.frame);
                target_layer.normalize_motion_keys();
            }
            if needs_format_5 {
                editor.project.format_version = editor.project.format_version.max(5);
            }
            editor.prune_authoring_groups();
            editor.set_selection(new_selection);
        });
        true
    }

    fn stepped_keyframe_ranges(&self) -> Vec<(usize, u32, u32)> {
        let timeline = &self.symbol().timeline;
        if let Some(range) = self.frame_range()
            && range.end > range.start
        {
            return range
                .layers
                .iter()
                .filter_map(|id| timeline.layers.iter().position(|l| &l.id == id))
                .filter(|&idx| {
                    let l = &timeline.layers[idx];
                    l.properties.role != LayerRole::Folder
                        && !neoflash_core::layer_effectively_locked(timeline, l)
                        && neoflash_core::layer_effectively_visible(timeline, l)
                })
                .map(|idx| (idx, range.start, range.end))
                .collect();
        }

        let mut spans = self.target_tween_spans();
        if spans.is_empty()
            && self.layer_editable()
            && let Some(start) = self.span_start()
        {
            spans.push((self.layer, vec![start]));
        }
        let mut out = Vec::new();
        for (layer_idx, starts) in spans {
            let Some(layer) = timeline.layers.get(layer_idx) else {
                continue;
            };
            let layer_end = layer
                .properties
                .end_frame
                .unwrap_or(timeline.frame_count)
                .min(timeline.frame_count)
                .saturating_sub(1);
            let ord_frames: Vec<u32> = layer
                .keyframes
                .iter()
                .filter(|k| k.keyed_properties.is_none())
                .map(|k| k.frame)
                .collect();
            for start in starts {
                let end = ord_frames
                    .iter()
                    .copied()
                    .find(|&f| f > start)
                    .unwrap_or(layer_end)
                    .max(start);
                if end > start {
                    out.push((layer_idx, start, end));
                }
            }
        }
        out
    }

    pub(crate) fn can_convert_to_keyframes_stepped(&self) -> bool {
        !self.stepped_keyframe_ranges().is_empty()
    }

    fn apply_stepped_keyframes_on_layer(
        timeline: &Timeline,
        layer: &mut Layer,
        start: u32,
        end: u32,
        step: u32,
        strip_tween: bool,
        fps: f64,
    ) -> usize {
        fn clear_tween_metadata(key: &mut Keyframe) {
            key.tween = None;
            key.tween_kind = TweenKind::Classic;
            key.keyed_properties = None;
            key.orient_to_path = false;
            key.property_eases.clear();
            key.motion_path = None;
            key.motion_tracks.clear();
            key.motion_keys = None;
            key.sync_symbols = false;
            key.snap_to_guide = false;
            key.shape_hints.clear();
            key.classic_rotate_turns = 0;
            key.classic_rotate_dir = ClassicRotateDirection::Auto;
            key.tween_scale = true;
            key.roving = false;
        }

        let step = step.max(1);
        if start > end {
            return 0;
        }
        layer.keyframes.sort_by_key(|key| key.frame);
        let original = layer.clone();
        let effective_end = original
            .properties
            .end_frame
            .unwrap_or(timeline.frame_count)
            .max(end + 1);
        if layer.properties.end_frame.is_some() {
            layer.properties.end_frame = Some(effective_end);
        }

        let mut stepped_frames: Vec<u32> = Vec::new();
        let mut f = start;
        while f <= end {
            stepped_frames.push(f);
            match f.checked_add(step) {
                Some(next) if next > f => f = next,
                _ => break,
            }
        }
        if stepped_frames.last().copied() != Some(end)
            && original
                .keyframes
                .iter()
                .any(|k| k.frame == end && k.keyed_properties.is_none())
        {
            stepped_frames.push(end);
        }

        let active_ease_at = |frame: u32| -> Option<(Easing, TweenKind)> {
            if let Some((span_start, _, span_end)) = motion_span(&original, frame) {
                let end_limit = span_end.unwrap_or(effective_end);
                if frame < end_limit {
                    return span_start.tween.map(|ease| (ease, TweenKind::Classic));
                }
            }
            let idx = original.keyframes.partition_point(|k| k.frame <= frame);
            let key = idx.checked_sub(1).and_then(|i| original.keyframes.get(i))?;
            let next_frame = original
                .keyframes
                .get(idx)
                .map_or(effective_end, |next| next.frame);
            if frame < next_frame {
                key.tween.map(|ease| {
                    let kind = if key.tween_kind == TweenKind::Shape {
                        TweenKind::Shape
                    } else {
                        TweenKind::Classic
                    };
                    (ease, kind)
                })
            } else {
                None
            }
        };

        let mut replacement = Vec::with_capacity(stepped_frames.len());
        for (idx, &frame) in stepped_frames.iter().enumerate() {
            // Exact source frames keep every non-tween authored field. Generated frames start
            // clean so labels, sounds, video, filters, rigs, markers, etc. are never cloned from
            // a neighbouring template keyframe.
            let mut key = original
                .keyframes
                .iter()
                .find(|old| old.frame == frame)
                .cloned()
                .unwrap_or_default();
            key.frame = frame;
            key.elements = sample_layer_in_at_fps(timeline, &original, frame, fps);
            clear_tween_metadata(&mut key);
            if !strip_tween {
                let is_last_in_range = idx + 1 == stepped_frames.len() && frame == end;
                if is_last_in_range {
                    if let Some(old_end) = original
                        .keyframes
                        .iter()
                        .find(|k| k.frame == frame && k.keyed_properties.is_none())
                    {
                        key.tween = old_end.tween;
                        key.tween_kind =
                            if key.tween.is_some() && old_end.tween_kind == TweenKind::Shape {
                                TweenKind::Shape
                            } else {
                                TweenKind::Classic
                            };
                    } else if let Some((ease, kind)) = active_ease_at(frame) {
                        key.tween = Some(ease);
                        key.tween_kind = kind;
                    }
                } else if let Some((ease, kind)) = active_ease_at(frame) {
                    key.tween = Some(ease);
                    key.tween_kind = kind;
                }
            }
            replacement.push(key);
        }

        if strip_tween
            && end + 1 < effective_end
            && !original.keyframes.iter().any(|k| k.frame == end + 1)
            && active_ease_at(end).is_some()
        {
            let mut after_key = Keyframe {
                frame: end + 1,
                elements: sample_layer_in_at_fps(timeline, &original, end + 1, fps),
                ..Default::default()
            };
            if let Some((ease, kind)) = active_ease_at(end + 1) {
                after_key.tween = Some(ease);
                after_key.tween_kind = kind;
            }
            replacement.push(after_key);
        }

        layer
            .keyframes
            .retain(|key| key.frame < start || key.frame > end);
        layer.keyframes.extend(replacement);
        layer.keyframes.sort_by_key(|key| key.frame);
        layer.keyframes.dedup_by_key(|key| key.frame);
        layer.normalize_motion_keys();
        stepped_frames.len()
    }

    /// ToonMonkey `Tween2Keys` (`strip_tween = true`) & Justin Putney `Convert to Keyframes Advanced`
    /// (`strip_tween = false`): samples the evaluated layer poses at every `step` frames across the
    /// selected frame range or active tween span(s), inserts keyframes on those stepped frames,
    /// removes intermediate keyframes, and either clears tween metadata (`strip_tween = true`) so the
    /// animation plays on 2s / 3s or keeps tween easing on each stepped keyframe (`strip_tween = false`).
    pub fn convert_to_keyframes_stepped(&mut self, step: u32, strip_tween: bool) -> bool {
        self.finish_edits();
        let step = step.max(1);
        let ranges = self.stepped_keyframe_ranges();
        if ranges.is_empty() {
            return false;
        }
        let fps = f64::from(self.project.fps);
        let label = if strip_tween {
            "Tween2Keys"
        } else {
            "Convert to keyframes (stepped)"
        };
        let mut changed = false;
        self.transact_named(label, |editor| {
            let timeline_snapshot = editor.symbol().timeline.clone();
            let sym = editor.symbol_mut();
            for &(layer_idx, start, end) in &ranges {
                if let Some(layer) = sym.timeline.layers.get_mut(layer_idx) {
                    let count = Self::apply_stepped_keyframes_on_layer(
                        &timeline_snapshot,
                        layer,
                        start,
                        end,
                        step,
                        strip_tween,
                        fps,
                    );
                    if count > 0 {
                        changed = true;
                    }
                }
            }
        });
        changed
    }

    /// ToonMonkey `Tween2Keys (On 2s)`.
    pub fn tween_to_keys_on_twos(&mut self) -> bool {
        self.convert_to_keyframes_stepped(2, true)
    }

    /// ToonMonkey `Tween2Keys (On 3s)`.
    pub fn tween_to_keys_on_threes(&mut self) -> bool {
        self.convert_to_keyframes_stepped(3, true)
    }

    fn graphic_synced_frame_at(
        project: &Project,
        layer: &Layer,
        parent_frame: u32,
        symbol_id: &str,
        playback: InstancePlayback,
    ) -> Option<u32> {
        let sym = project.symbol(symbol_id)?;
        if sym.kind != SymbolKind::Graphic {
            return None;
        }
        let count = sym.timeline.frame_count.max(1);
        let key_start = layer.key_at(parent_frame).map_or(parent_frame, |k| k.frame);
        let elapsed = parent_frame.saturating_sub(key_start);
        let synced = match playback {
            InstancePlayback::Loop | InstancePlayback::Independent => elapsed % count,
            InstancePlayback::LoopFrom(first) => (first + elapsed) % count,
            InstancePlayback::PlayOnce => elapsed.min(count - 1),
            InstancePlayback::PlayOnceFrom(first) => (first + elapsed).min(count - 1),
            InstancePlayback::SingleFrame(first) => first.min(count - 1),
        };
        Some(synced)
    }

    pub(crate) fn can_enter_graphic_at_current_frame(&self) -> bool {
        let Some(placement) = self.selected_sampled() else {
            return false;
        };
        let ElementKind::Instance { symbol_id, .. } = placement.kind else {
            return false;
        };
        self.project
            .symbol(&symbol_id)
            .is_some_and(|sym| sym.kind == SymbolKind::Graphic)
    }

    /// ToonMonkey `Enter Graphic at Current Frame` (`Edit in Place at Synced Frame`): enters
    /// Edit in Place for the selected `Graphic` symbol instance and jumps `self.frame` directly
    /// to the synchronized child frame displayed at the current parent frame.
    pub fn enter_graphic_at_current_frame(&mut self) -> bool {
        self.finish_edits();
        let Some(target) = self.selection.single().cloned() else {
            return false;
        };
        let Some(placement) = self.selected_sampled() else {
            return false;
        };
        let ElementKind::Instance {
            symbol_id,
            playback,
        } = placement.kind
        else {
            return false;
        };
        let Some(layer) = self
            .symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == target.layer)
        else {
            return false;
        };
        let Some(synced_frame) =
            Self::graphic_synced_frame_at(&self.project, layer, self.frame, &symbol_id, playback)
        else {
            return false;
        };
        if !self.enter_symbol_at(target, symbol_id) {
            return false;
        }
        self.frame = synced_frame;
        if let Some(level) = self.edit_session.levels.last_mut() {
            level.saved_view.frame = synced_frame;
        }
        true
    }

    pub(crate) fn has_selected_graphic_instance(&self) -> bool {
        self.sampled_targets().into_iter().any(|(target, el)| {
            self.target_editable(&target)
                && matches!(
                    el.kind,
                    ElementKind::Instance { ref symbol_id, .. }
                        if self
                            .project
                            .symbol(symbol_id)
                            .is_some_and(|s| s.kind == SymbolKind::Graphic)
                )
        })
    }

    /// ToonMonkey `Set Graphic Playback: Loop / Play Once / Single Frame (Current)`: updates
    /// `playback` on all selected `Graphic` instances (locking `first_frame` to the currently
    /// displayed synced child frame when `lock_current_synced_frame` is `true`) in one Undo step.
    pub fn set_selected_graphic_playback(
        &mut self,
        playback: InstancePlayback,
        lock_current_synced_frame: bool,
    ) -> bool {
        self.finish_edits();
        let frame = self.frame;
        let timeline = &self.symbol().timeline;
        let mut updates: Vec<(crate::selection::SelectionTarget, InstancePlayback)> = Vec::new();
        for (target, el) in self.sampled_targets() {
            if !self.target_editable(&target) {
                continue;
            }
            let ElementKind::Instance {
                ref symbol_id,
                playback: current_playback,
            } = el.kind
            else {
                continue;
            };
            let Some(layer) = timeline.layers.iter().find(|l| l.id == target.layer) else {
                continue;
            };
            let Some(synced) = Self::graphic_synced_frame_at(
                &self.project,
                layer,
                frame,
                symbol_id,
                current_playback,
            ) else {
                continue;
            };
            let next_playback = if lock_current_synced_frame {
                match playback {
                    InstancePlayback::SingleFrame(_) => InstancePlayback::SingleFrame(synced),
                    InstancePlayback::Loop | InstancePlayback::LoopFrom(_) => {
                        if synced == 0 {
                            InstancePlayback::Loop
                        } else {
                            InstancePlayback::LoopFrom(synced)
                        }
                    }
                    InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
                        if synced == 0 {
                            InstancePlayback::PlayOnce
                        } else {
                            InstancePlayback::PlayOnceFrom(synced)
                        }
                    }
                    InstancePlayback::Independent => InstancePlayback::SingleFrame(synced),
                }
            } else {
                playback
            };
            updates.push((target, next_playback));
        }
        if updates.is_empty() {
            return false;
        }
        self.playing = false;
        self.transact_named("Set graphic playback", move |editor| {
            let mut layers: Vec<Id> = updates.iter().map(|(t, _)| t.layer.clone()).collect();
            layers.sort();
            layers.dedup();
            for lid in &layers {
                editor.ensure_key_for(lid, false);
            }
            for (target, next_playback) in updates {
                if let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == target.layer)
                    && let Some(key) = layer.key_at_mut(frame)
                    && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                    && let ElementKind::Instance { playback: slot, .. } = &mut el.kind
                {
                    *slot = next_playback;
                }
            }
        });
        true
    }

    /// ToonMonkey `Swap Multiple Symbols`: swaps `symbol_id` to `new_symbol_id` across all
    /// currently selected instances, and when `across_selected_frame_range` is `true` (or when
    /// multiple keyframes in a timeline range are selected), swaps matching instances across every
    /// keyframe in the selected layer/frame range in one atomic Undo step.
    pub fn swap_symbol_across_selection_or_range(
        &mut self,
        new_symbol_id: &str,
        across_selected_frame_range: bool,
    ) -> usize {
        self.finish_edits();
        if self.project.symbol(new_symbol_id).is_none() || self.project.is_scene(new_symbol_id) {
            return 0;
        }
        let active_symbol = self.symbol_id().to_owned();
        let mut seen_symbols = std::collections::HashSet::new();
        if active_symbol == new_symbol_id
            || crate::app::reaches(
                &self.project,
                new_symbol_id,
                &active_symbol,
                &mut seen_symbols,
            )
        {
            return 0;
        }

        let selected_instances: Vec<(crate::selection::SelectionTarget, Id)> = self
            .sampled_targets()
            .into_iter()
            .filter(|(target, _)| self.target_editable(target))
            .filter_map(|(target, el)| match el.kind {
                ElementKind::Instance { symbol_id, .. } => Some((target, symbol_id)),
                _ => None,
            })
            .collect();

        let range_opt = self.frame_range();
        let multi_key_range = range_opt.as_ref().is_some_and(|r| {
            let timeline = &self.symbol().timeline;
            r.end > r.start
                && r.layers.iter().any(|lid| {
                    timeline
                        .layers
                        .iter()
                        .find(|l| &l.id == lid)
                        .is_some_and(|l| {
                            l.keyframes
                                .iter()
                                .filter(|k| k.frame >= r.start && k.frame <= r.end)
                                .count()
                                >= 2
                        })
                })
        });
        let across_range = across_selected_frame_range || multi_key_range;

        if selected_instances.is_empty() && range_opt.is_none() {
            return 0;
        }

        let new_id_owned = new_symbol_id.to_owned();
        let current_frame = self.frame;
        let mut swapped = 0usize;
        self.playing = false;
        self.transact_named("Swap multiple symbols", |editor| {
            let selected_elem_ids: std::collections::HashSet<Id> = selected_instances
                .iter()
                .map(|(t, _)| t.element.clone())
                .collect();
            let selected_old_symbols: std::collections::HashSet<Id> = selected_instances
                .iter()
                .map(|(_, sym_id)| sym_id.clone())
                .collect();

            if !across_range {
                let mut touched_layers: Vec<Id> = selected_instances
                    .iter()
                    .map(|(t, _)| t.layer.clone())
                    .collect();
                touched_layers.sort();
                touched_layers.dedup();
                for lid in &touched_layers {
                    editor.ensure_key_for(lid, false);
                }
                for (target, _) in &selected_instances {
                    if let Some(layer) = editor
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == target.layer)
                        && let Some(key) = layer.key_at_mut(current_frame)
                        && let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element)
                        && let ElementKind::Instance { symbol_id, .. } = &mut el.kind
                    {
                        *symbol_id = new_id_owned.clone();
                        swapped += 1;
                    }
                }
            } else {
                let (target_layers, start_f, end_f): (Vec<Id>, u32, u32) =
                    if let Some(ref r) = range_opt {
                        (r.layers.clone(), r.start, r.end)
                    } else {
                        let mut lids: Vec<Id> = selected_instances
                            .iter()
                            .map(|(t, _)| t.layer.clone())
                            .collect();
                        lids.sort();
                        lids.dedup();
                        let max_f = editor.symbol().timeline.frame_count.saturating_sub(1);
                        (lids, 0, max_f)
                    };
                let timeline = &mut editor.symbol_mut().timeline;
                for lid in &target_layers {
                    let Some(layer) = timeline.layers.iter_mut().find(|l| &l.id == lid) else {
                        continue;
                    };
                    if layer.properties.role == LayerRole::Folder || layer.locked || !layer.visible
                    {
                        continue;
                    }
                    for key in &mut layer.keyframes {
                        if key.frame < start_f || key.frame > end_f {
                            continue;
                        }
                        for el in &mut key.elements {
                            let matches_target = selected_instances.is_empty()
                                || selected_elem_ids.contains(&el.id)
                                || matches!(
                                    &el.kind,
                                    ElementKind::Instance { symbol_id, .. }
                                        if selected_old_symbols.contains(symbol_id)
                                );
                            if matches_target
                                && let ElementKind::Instance { symbol_id, .. } = &mut el.kind
                            {
                                *symbol_id = new_id_owned.clone();
                                swapped += 1;
                            }
                        }
                    }
                }
            }
        });
        swapped
    }

    pub(crate) fn can_copy_active_keyframe_ease(&self) -> bool {
        if let Some(span) = self.tween_span()
            && let Some(layer) = self
                .symbol()
                .timeline
                .layers
                .iter()
                .find(|l| l.id == span.layer_id)
            && layer
                .keyframes
                .iter()
                .find(|k| k.frame == span.start)
                .and_then(|k| k.tween)
                .is_some()
        {
            return true;
        }
        self.symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|l| l.key_at(self.frame))
            .and_then(|k| k.tween)
            .is_some()
    }

    pub(crate) fn has_copied_keyframe_ease(&self) -> bool {
        COPIED_KEYFRAME_EASE
            .lock()
            .ok()
            .is_some_and(|guard| guard.is_some())
    }

    /// Justin Putney `Copy Ease`: copies the `Easing` curve from the active keyframe or tween span.
    pub fn copy_active_keyframe_ease(&mut self) -> bool {
        let ease = self
            .tween_span()
            .and_then(|span| {
                self.symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == span.layer_id)?
                    .keyframes
                    .iter()
                    .find(|k| k.frame == span.start)?
                    .tween
            })
            .or_else(|| {
                self.symbol()
                    .timeline
                    .layers
                    .get(self.layer)?
                    .key_at(self.frame)?
                    .tween
            });
        let Some(ease) = ease else {
            return false;
        };
        if let Ok(mut slot) = COPIED_KEYFRAME_EASE.lock() {
            *slot = Some(ease);
            true
        } else {
            false
        }
    }

    /// Justin Putney `Apply Ease to Selected Layers / Range`: applies the copied `Easing` across
    /// all selected layers and frame ranges in one Undo step.
    pub fn apply_copied_ease_to_selection(&mut self) -> usize {
        self.finish_edits();
        let Some(ease) = COPIED_KEYFRAME_EASE.lock().ok().and_then(|g| *g) else {
            return 0;
        };
        let mut targets = self.target_tween_spans();
        let selected_rows = self.target_layer_indices(self.layer);
        let timeline = &self.symbol().timeline;
        for row_idx in selected_rows {
            if targets.iter().any(|(idx, _)| *idx == row_idx) {
                continue;
            }
            let Some(layer) = timeline.layers.get(row_idx) else {
                continue;
            };
            if layer.properties.role == LayerRole::Folder
                || neoflash_core::layer_effectively_locked(timeline, layer)
                || !neoflash_core::layer_effectively_visible(timeline, layer)
            {
                continue;
            }
            let start = if let Some((s, ..)) = motion_span(layer, self.frame) {
                Some(s.frame)
            } else {
                layer.key_at(self.frame).map(|k| k.frame)
            };
            if let Some(st) = start {
                targets.push((row_idx, vec![st]));
            }
        }
        if targets.is_empty() {
            return 0;
        }
        let mut applied = 0usize;
        self.transact_named("Apply ease to selection", |editor| {
            let layers = &mut editor.symbol_mut().timeline.layers;
            for (layer_idx, starts) in &targets {
                let Some(layer) = layers.get_mut(*layer_idx) else {
                    continue;
                };
                for &start_frame in starts {
                    if let Some(key) = layer
                        .keyframes
                        .iter_mut()
                        .find(|k| k.frame == start_frame && k.keyed_properties.is_none())
                    {
                        key.tween = Some(ease);
                        if key.tween_kind == TweenKind::Motion {
                            for (_, prop_ease) in &mut key.property_eases {
                                *prop_ease = ease;
                            }
                            for track in &mut key.motion_tracks {
                                track.easing = Some(ease);
                            }
                        }
                        applied += 1;
                    }
                }
            }
            editor.project.upgrade_format_for_features();
        });
        applied
    }

    /// Justin Putney `Pixel Snap (Round Position to Whole Pixels)`: rounds `transform.x` and
    /// `transform.y` (and vertex coordinates for raw `ElementKind::Path` / `PaintedPath` with zero
    /// translation) to whole pixels on all selected Stage elements (or all editable elements on
    /// the current frame if nothing is selected) in one Undo step.
    pub fn pixel_snap_selection(&mut self) -> usize {
        self.finish_edits();
        let targets: Vec<crate::selection::SelectionTarget> = {
            let sampled: Vec<_> = self
                .sampled_targets()
                .into_iter()
                .filter(|(t, _)| self.target_editable(t))
                .map(|(t, _)| t)
                .collect();
            if !sampled.is_empty() {
                sampled
            } else {
                self.editable_targets()
            }
        };
        if targets.is_empty() {
            return 0;
        }
        let frame = self.frame;
        let mut count = 0usize;
        self.playing = false;
        self.transact_named("Pixel snap", |editor| {
            let mut layer_ids: Vec<Id> = targets.iter().map(|t| t.layer.clone()).collect();
            layer_ids.sort();
            layer_ids.dedup();
            for lid in &layer_ids {
                editor.ensure_key_for(lid, false);
            }
            for target in &targets {
                let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == target.layer)
                else {
                    continue;
                };
                let Some(key) = layer.key_at_mut(frame) else {
                    continue;
                };
                let Some(el) = key.elements.iter_mut().find(|e| e.id == target.element) else {
                    continue;
                };
                let zero_translation = el.transform.x == 0.0 && el.transform.y == 0.0;
                el.transform.x = el.transform.x.round();
                el.transform.y = el.transform.y.round();
                if zero_translation {
                    match &mut el.kind {
                        ElementKind::Path { commands, .. }
                        | ElementKind::PaintedPath { commands, .. } => {
                            for cmd in commands {
                                match cmd {
                                    PathCommand::MoveTo { x, y } | PathCommand::LineTo { x, y } => {
                                        *x = x.round();
                                        *y = y.round();
                                    }
                                    PathCommand::QuadraticTo { cx, cy, x, y } => {
                                        *cx = cx.round();
                                        *cy = cy.round();
                                        *x = x.round();
                                        *y = y.round();
                                    }
                                    PathCommand::CubicTo {
                                        cx1,
                                        cy1,
                                        cx2,
                                        cy2,
                                        x,
                                        y,
                                    } => {
                                        *cx1 = cx1.round();
                                        *cy1 = cy1.round();
                                        *cx2 = cx2.round();
                                        *cy2 = cy2.round();
                                        *x = x.round();
                                        *y = y.round();
                                    }
                                    PathCommand::Close => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                count += 1;
            }
        });
        count
    }

    pub(crate) fn can_reverse_selected_elements_order(&self) -> bool {
        if self.selection.symbol != self.symbol_id() {
            return false;
        }
        let mut counts_by_layer: std::collections::HashMap<Id, usize> =
            std::collections::HashMap::new();
        for (target, _) in self.sampled_targets() {
            if self.target_editable(&target) {
                *counts_by_layer.entry(target.layer).or_insert(0) += 1;
            }
        }
        counts_by_layer.values().any(|&c| c >= 2)
    }

    /// Justin Putney `Swap Depth / Reverse Order`: reverses the relative Z-order stacking indices
    /// of the currently selected elements within their keyframe's `elements` vector in one Undo step.
    pub fn reverse_selected_elements_order(&mut self) -> bool {
        self.finish_edits();
        if !self.can_reverse_selected_elements_order() {
            return false;
        }
        let selected: std::collections::HashSet<crate::selection::SelectionTarget> = self
            .sampled_targets()
            .into_iter()
            .filter(|(t, _)| self.target_editable(t))
            .map(|(t, _)| t)
            .collect();
        let frame = self.frame;
        let mut changed = false;
        self.playing = false;
        self.transact_named("Reverse stacking order", |editor| {
            let mut layer_ids: Vec<Id> = selected.iter().map(|t| t.layer.clone()).collect();
            layer_ids.sort();
            layer_ids.dedup();
            for lid in layer_ids {
                let count_on_layer = selected.iter().filter(|t| t.layer == lid).count();
                if count_on_layer < 2 {
                    continue;
                }
                editor.ensure_key_for(&lid, false);
                let Some(layer) = editor
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == lid)
                else {
                    continue;
                };
                let Some(key) = layer.key_at_mut(frame) else {
                    continue;
                };
                let positions: Vec<usize> = key
                    .elements
                    .iter()
                    .enumerate()
                    .filter(|(_, el)| {
                        selected.contains(&crate::selection::SelectionTarget {
                            layer: lid.clone(),
                            element: el.id.clone(),
                        })
                    })
                    .map(|(idx, _)| idx)
                    .collect();
                if positions.len() < 2 {
                    continue;
                }
                let mut picked: Vec<Element> = positions
                    .iter()
                    .map(|&idx| key.elements[idx].clone())
                    .collect();
                picked.reverse();
                for (pos, el) in positions.into_iter().zip(picked) {
                    key.elements[pos] = el;
                }
                changed = true;
            }
        });
        changed
    }

    /// Removes normal unlocked layers that have no elements, no frame scripts, no labels, no
    /// comments, and no sounds/videos, keeping at least 1 normal layer, in one Undo step.
    pub fn delete_empty_layers(&mut self) -> usize {
        self.finish_edits();
        let timeline = &self.symbol().timeline;
        let referenced_ids: std::collections::HashSet<Id> = timeline
            .layers
            .iter()
            .flat_map(|l| {
                [
                    l.properties.parent.clone(),
                    l.properties.mask.clone(),
                    l.properties.motion_guide.clone(),
                ]
                .into_iter()
                .flatten()
            })
            .collect();
        let script_bearing_layers: std::collections::HashSet<usize> = (0..timeline.frame_count)
            .filter_map(|f| timeline.action_layer_for_frame(f))
            .collect();

        let normal_indices: Vec<usize> = timeline
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.properties.role == LayerRole::Normal)
            .map(|(i, _)| i)
            .collect();
        if normal_indices.len() <= 1 {
            return 0;
        }

        let mut candidate_empty: Vec<usize> = timeline
            .layers
            .iter()
            .enumerate()
            .filter(|(idx, l)| {
                l.properties.role == LayerRole::Normal
                    && !l.locked
                    && !neoflash_core::layer_effectively_locked(timeline, l)
                    && !referenced_ids.contains(&l.id)
                    && !script_bearing_layers.contains(idx)
                    && !(l.name.eq_ignore_ascii_case("actions") && !timeline.scripts.is_empty())
                    && l.keyframes
                        .iter()
                        .all(|key| !keyframe_has_authored_content(key))
            })
            .map(|(idx, _)| idx)
            .collect();

        if candidate_empty.len() >= normal_indices.len() {
            // Keep at least 1 normal layer.
            candidate_empty.remove(0);
        }
        if candidate_empty.is_empty() {
            return 0;
        }

        let remove_ids: std::collections::HashSet<Id> = candidate_empty
            .iter()
            .map(|&idx| timeline.layers[idx].id.clone())
            .collect();
        let removed_count = remove_ids.len();
        self.transact_named("Delete empty layers", move |editor| {
            let timeline = &mut editor.symbol_mut().timeline;
            timeline.layers.retain(|l| !remove_ids.contains(&l.id));
            for layer in &mut timeline.layers {
                if layer
                    .properties
                    .linked_layer
                    .as_ref()
                    .is_some_and(|partner| remove_ids.contains(partner))
                {
                    layer.properties.linked_layer = None;
                }
            }
            let len = editor.symbol().timeline.layers.len();
            if editor.layer >= len {
                editor.layer = len.saturating_sub(1);
            }
            editor.normalize_targets();
        });
        removed_count
    }

    /// Renames generic `Layer N` layers after the first symbol instance or named element on that
    /// layer in one Undo step.
    pub fn rename_layers_from_content(&mut self) -> usize {
        self.finish_edits();
        fn is_generic_layer_name(name: &str) -> bool {
            let trimmed = name.trim();
            trimmed
                .strip_prefix("Layer ")
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        }

        let mut renames: Vec<(usize, String)> = Vec::new();
        for (idx, layer) in self.symbol().timeline.layers.iter().enumerate() {
            if !is_generic_layer_name(&layer.name) {
                continue;
            }
            let derived = layer
                .keyframes
                .iter()
                .flat_map(|k| &k.elements)
                .find_map(|el| match &el.kind {
                    ElementKind::Instance { symbol_id, .. } => self
                        .project
                        .symbol(symbol_id)
                        .map(|s| s.name.trim().to_owned())
                        .filter(|n| !n.is_empty())
                        .or_else(|| {
                            let n = el.name.trim();
                            (!n.is_empty()).then(|| n.to_owned())
                        }),
                    _ => None,
                })
                .or_else(|| {
                    layer
                        .keyframes
                        .iter()
                        .flat_map(|k| &k.elements)
                        .find_map(|el| {
                            let n = el.name.trim();
                            (!n.is_empty()).then(|| n.to_owned())
                        })
                });
            if let Some(new_name) = derived
                && new_name != layer.name
            {
                renames.push((idx, new_name));
            }
        }
        if renames.is_empty() {
            return 0;
        }
        let count = renames.len();
        self.transact_named("Rename layers from symbol", move |editor| {
            let layers = &mut editor.symbol_mut().timeline.layers;
            for (idx, new_name) in renames {
                if let Some(layer) = layers.get_mut(idx) {
                    layer.name = new_name;
                }
            }
        });
        count
    }

    /// Creates or reuses a top locked `"actions"` layer and places a `stop();` `FrameScript` on
    /// the last frame (`frame_count - 1`) of the active timeline in one Undo step.
    pub fn add_actions_and_stop_layer(&mut self) -> bool {
        self.finish_edits();
        if self.symbol().kind == SymbolKind::Button {
            return false;
        }
        let script_language = self
            .symbol()
            .timeline
            .scripts
            .first()
            .map(|script| script.script.language)
            .or_else(|| self.project.sources.first().map(|source| source.language))
            .or_else(|| {
                self.project
                    .symbols
                    .iter()
                    .flat_map(|symbol| &symbol.timeline.scripts)
                    .next()
                    .map(|script| script.script.language)
            })
            .unwrap_or(ScriptLanguage::ActionScript3);
        self.transact_named("Add actions & stop() layer", move |editor| {
            let timeline = &mut editor.symbol_mut().timeline;
            let last_frame = timeline.frame_count.max(1) - 1;
            let existing_idx = timeline.layers.iter().position(|layer| {
                layer.name.eq_ignore_ascii_case("actions")
                    && layer.properties == LayerProperties::default()
                    && layer
                        .keyframes
                        .iter()
                        .all(|key| !keyframe_has_authored_content(key))
            });
            let actions_idx = if let Some(idx) = existing_idx {
                let mut layer = timeline.layers.remove(idx);
                layer.locked = true;
                layer.visible = true;
                if layer.keyframes.is_empty() {
                    layer.keyframes.push(Keyframe {
                        frame: 0,
                        ..Default::default()
                    });
                }
                if last_frame > 0 && !layer.keyframes.iter().any(|k| k.frame == last_frame) {
                    layer.keyframes.push(Keyframe {
                        frame: last_frame,
                        ..Default::default()
                    });
                    layer.keyframes.sort_by_key(|k| k.frame);
                }
                timeline.layers.push(layer);
                timeline.layers.len() - 1
            } else {
                let mut layer_name = "actions".to_owned();
                let mut suffix = 2u32;
                while timeline
                    .layers
                    .iter()
                    .any(|layer| layer.name.eq_ignore_ascii_case(&layer_name))
                {
                    layer_name = format!("actions {suffix}");
                    suffix += 1;
                }
                let mut keyframes = vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }];
                if last_frame > 0 {
                    keyframes.push(Keyframe {
                        frame: last_frame,
                        ..Default::default()
                    });
                }
                timeline.layers.push(Layer {
                    id: new_id("layer"),
                    name: layer_name,
                    visible: true,
                    locked: true,
                    keyframes,
                    properties: LayerProperties {
                        role: LayerRole::Normal,
                        ..Default::default()
                    },
                });
                timeline.layers.len() - 1
            };
            let _ = actions_idx;
            if let Some(existing_script) =
                timeline.scripts.iter_mut().find(|s| s.frame == last_frame)
            {
                if existing_script.script.source.trim().is_empty() {
                    existing_script.script.source = "stop();".to_owned();
                } else if !existing_script.script.source.contains("stop()") {
                    existing_script.script.source.push_str("\nstop();");
                }
            } else {
                timeline.scripts.push(FrameScript {
                    frame: last_frame,
                    script: Script {
                        language: script_language,
                        source: "stop();".to_owned(),
                    },
                });
                timeline.scripts.sort_by_key(|s| s.frame);
            }
        });
        true
    }
}

static COPIED_KEYFRAME_EASE: std::sync::LazyLock<std::sync::Mutex<Option<Easing>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
