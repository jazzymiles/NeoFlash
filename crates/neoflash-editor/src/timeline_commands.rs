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
                    let Some(keyed) = &mut key.keyed_properties else {
                        continue;
                    };
                    let reference = match old.keyframes.iter().find(|k| k.frame == key.frame) {
                        Some(old_key) if old_key.keyed_properties.is_some() => {
                            old_key.elements.clone()
                        }
                        _ if keyed.is_empty() => sample_layer_at_fps(old, key.frame, fps),
                        _ => continue,
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
                let shown = sample_layer_at_fps(&snapshot, key.frame, fps);
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
                crate::timeline_motion_curve::rebase_motion_paths(before, project);
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
                // Motion tracks are keyed by stable element ID, so every object on the layer can
                // carry an independent transform through the same span. Keeping the old
                // single-object restriction here made the UI reject a graph the evaluator and
                // property-key recorder already support.
                if key.elements.is_empty() {
                    self.message("A motion tween needs at least one object on its keyframe.");
                    return;
                }
            }
            TweenKind::Classic => {}
        }
        let tween_label = match kind {
            TweenKind::Classic => "Classic tween",
            TweenKind::Shape => "Shape tween",
            TweenKind::Motion => "Motion tween",
        };
        let classic_ease = self.workspace.default_ease;
        self.transact_named(tween_label, |s| {
            let l = s.layer;
            let layer = &mut s.symbol_mut().timeline.layers[l];
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
                        key.property_eases.clear();
                        key.sync_symbols = false;
                        key.snap_to_guide = false;
                    }
                } else if in_span && key.frame > start && key.keyed_properties.is_some() {
                    if kind != TweenKind::Motion {
                        key.keyed_properties = None;
                        key.tween_kind = kind;
                        key.property_eases.clear();
                        key.sync_symbols = false;
                        key.snap_to_guide = false;
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
        self.transact_named("Remove tween", |s| {
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
                key.orient_to_path = false;
                key.property_eases.clear();
                key.sync_symbols = false;
                key.snap_to_guide = false;
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
        self.transact_named("Clear keyframe", |s| {
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

    /// The same transient solo projection for a frozen parent timeline while a
    /// nested symbol is active. Solo state is keyed by symbol, not by the current
    /// breadcrumb level, so parent context must not accidentally use the child's set.
    pub(crate) fn project_for_symbol_stage_eval(
        &self,
        symbol_id: &str,
    ) -> std::borrow::Cow<'_, Project> {
        let Some(symbol) = self.project.symbol(symbol_id) else {
            return std::borrow::Cow::Borrowed(&self.project);
        };
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
            return std::borrow::Cow::Borrowed(&self.project);
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
        let mut copy = self.project.clone();
        if let Some(sym) = copy.symbol_mut(symbol_id) {
            for layer in &mut sym.timeline.layers {
                if !soloed_ids.contains(&layer.id) && !needed_ids.contains(&layer.id) {
                    layer.visible = false;
                }
            }
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
        let (key_idx, _) = self.shape_hint_span()?;
        let layer_idx = self.layer;
        let key = &self.symbol().timeline.layers[layer_idx].keyframes[key_idx];
        let label = ('a'..='z').find(|&ch| !key.shape_hints.iter().any(|h| h.label == ch))?;
        let step = (key.shape_hints.len() as f64 + 1.0) / 27.0;
        let hint = ShapeHint {
            label,
            start_uv: [step, 0.5],
            end_uv: [step, 0.5],
        };
        self.transact_named("Add shape hint", |editor| {
            editor.symbol_mut().timeline.layers[layer_idx].keyframes[key_idx]
                .shape_hints
                .push(hint);
        });
        self.timeline_shape_hints.show = true;
        Some(label)
    }

    /// Remove both endpoints of every hint on the current shape span in one Undo step.
    pub(crate) fn remove_shape_hints(&mut self) -> bool {
        self.finish_edits();
        let Some((key_idx, _)) = self.shape_hint_span() else {
            return false;
        };
        let layer_idx = self.layer;
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
        let Some(key) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.keyframes.iter().find(|k| k.frame == frame))
        else {
            return false;
        };
        let is_pos_key = key
            .keyed_properties
            .as_ref()
            .is_some_and(|kp| kp.contains(&TweenProperty::X) || kp.contains(&TweenProperty::Y));
        if !is_pos_key {
            return false;
        }
        let next_roving = !key.roving;
        self.transact_named("Toggle roving keyframe", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(k) = layer.keyframes.iter_mut().find(|k| k.frame == frame) {
                k.roving = next_roving;
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
                k.set_asset_warp(rig);
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
        key.video = video;
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
}
