//! Symbol authoring and the compact, frame-preserving timeline timing controls.
use crate::{app::Editor, paint_controls::property_row};
use eframe::egui;
use neoflash_core::{
    ElementKind, Id, InstancePlayback, LayerRole, SymbolKind, layer_effectively_locked,
    layer_effectively_visible, new_id, sample_layer_in,
};
use std::collections::HashSet;

pub(crate) const BUTTON_STATES: [&str; 4] = ["Up", "Over", "Down", "Hit"];

#[derive(Clone)]
struct TimingDraft {
    baseline: u32,
    frames: u32,
    /// Fractional while scrubbing the `FPS` prefix; committed rounded.
    fps: f64,
    seconds: f64,
    pending: bool,
}

impl Editor {
    /// Timeline controls bar: `NF2/Field` FPS and/or Duration (frame count stays fixed), or — in the
    /// FPS slot — the Up · Over · Down · Hit segmented states of a button symbol. The bar can hide each
    /// field separately (R-052); both share one draft. One undo step per commit.
    pub(crate) fn timeline_timing_fields(
        &mut self,
        ui: &mut egui::Ui,
        show_fps: bool,
        show_duration: bool,
    ) {
        use neoflash_ui::components as nf;
        use neoflash_ui::{TextStyle, typography};
        if self.symbol().kind == SymbolKind::Button {
            if !show_fps {
                return;
            }
            let count = self.symbol().timeline.frame_count as usize;
            if count < 4 && ui.add(nf::Button::new("Enable four states")).clicked() {
                self.transact(|s| s.symbol_mut().timeline.frame_count = 4);
            }
            let options: Vec<(u32, &str)> = BUTTON_STATES
                .iter()
                .enumerate()
                .filter(|(frame, _)| *frame < count)
                .map(|(frame, label)| (frame as u32, *label))
                .collect();
            let mut state = self.frame;
            nf::Segmented::show(ui, &mut state, &options, false);
            if state != self.frame {
                self.finish_edits();
                self.set_playing(false);
                self.reset_authored_preview();
                self.frame = state;
                self.normalize_selection();
            }
            let p = neoflash_ui::palette(ui.ctx());
            ui.label(
                typography::rich(
                    if self.frame == 3 {
                        "Hit area · invisible in HTML5"
                    } else {
                        "Button state"
                    },
                    TextStyle::Small,
                )
                .color(p.text_tertiary),
            );
            return;
        }
        let frames = self.symbol().timeline.frame_count.max(1);
        let id = egui::Id::new(("timeline-timing", &self.clipboard.session, self.symbol_id()));
        let mut draft = ui
            .data_mut(|data| data.get_temp::<TimingDraft>(id))
            .filter(|draft| draft.baseline == self.project.fps && draft.frames == frames)
            .unwrap_or(TimingDraft {
                baseline: self.project.fps,
                frames,
                fps: self.project.fps as f64,
                seconds: frames as f64 / self.project.fps.max(1) as f64,
                pending: false,
            });
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if show_fps {
            let fps = ui
                .add(
                    nf::Field::new(&mut draft.fps, "Frame rate")
                        .prefix("FPS")
                        .range(1.0..=1000.0)
                        .step(0.25)
                        .width(92.),
                )
                .on_hover_text("Frame rate. Frame count stays fixed; duration changes.");
            if fps.changed() {
                draft.seconds = frames as f64 / draft.fps.round().max(1.);
                draft.pending = true;
            }
        }
        if show_duration {
            let duration = ui
                .add(
                    nf::Field::new(&mut draft.seconds, "Duration")
                        .prefix("Duration")
                        .unit("s")
                        .range(frames as f64 / 1000. ..=frames as f64)
                        .step(0.02)
                        .decimals(2)
                        .width(140.),
                )
                .on_hover_text(
                    "Total duration. Keeps all frames and sets the nearest whole-number FPS.",
                );
            if duration.changed() {
                draft.fps = (frames as f64 / draft.seconds).round().clamp(1., 1000.);
                draft.pending = true;
            }
        }
        if escape {
            ui.data_mut(|data| data.remove::<TimingDraft>(id));
            return;
        }
        if draft.pending && !ui.input(|i| i.pointer.primary_down()) {
            let next = (draft.fps.round() as u32).clamp(1, 1000);
            if next != self.project.fps {
                self.finish_edits();
                self.transact_named("Frame rate", |s| {
                    s.project.fps = next;
                    s.accumulator = 0.;
                    s.last_tick = std::time::Instant::now();
                });
            }
            draft.baseline = self.project.fps;
            draft.fps = self.project.fps as f64;
            draft.seconds = frames as f64 / self.project.fps as f64;
            draft.pending = false;
        }
        ui.data_mut(|data| data.insert_temp(id, draft));
    }

    /// Playback is a placement property; editing it never changes sibling instances.
    pub(crate) fn instance_playback_ui(
        &mut self,
        ui: &mut egui::Ui,
        symbol_id: &str,
        playback: &mut InstancePlayback,
    ) -> Option<egui::Id> {
        let symbol = self.project.symbol(symbol_id)?;
        let count = symbol.timeline.frame_count.max(1);
        let kind = symbol.kind;
        property_row(ui, "Type", |ui| {
            ui.label(match kind {
                SymbolKind::MovieClip => "Movie Clip",
                SymbolKind::Graphic => "Graphic",
                SymbolKind::Button => "Button",
            })
        });
        if kind == SymbolKind::Button {
            ui.small("Up / Over / Down / Hit");
            ui.small("AS2 callbacks and AS3 events run on this named instance.");
            return None;
        }
        if kind == SymbolKind::MovieClip {
            return None;
        }
        let before = *playback;
        let (mut mode, mut first) = match *playback {
            InstancePlayback::Independent => (0, 0),
            InstancePlayback::Loop => (1, 0),
            InstancePlayback::LoopFrom(f) => (1, f),
            InstancePlayback::PlayOnce => (2, 0),
            InstancePlayback::PlayOnceFrom(f) => (2, f),
            InstancePlayback::SingleFrame(f) => (3, f),
        };
        let labels = ["Independent", "Loop", "Play once", "Single frame"];
        let mut control = None;
        let response = property_row(ui, "Playback", |ui| {
            egui::ComboBox::from_id_salt("instance-playback")
                .width(ui.available_width().min(140.))
                .selected_text(labels[mode])
                .show_ui(ui, |ui| {
                    for (index, name) in labels.iter().enumerate() {
                        ui.selectable_value(&mut mode, index, *name);
                    }
                })
                .response
        });
        let mut displayed = first.saturating_add(1).min(count);
        let field = property_row(ui, "First frame", |ui| {
            ui.add_sized(
                [76., 22.],
                egui::DragValue::new(&mut displayed)
                    .range(1..=count)
                    .update_while_editing(false),
            )
        });
        first = displayed - 1;
        if field.changed() {
            if mode == 0 && first > 0 {
                mode = 1;
            }
            control = Some(field.id);
        }
        *playback = match mode {
            0 => InstancePlayback::Independent,
            1 if first == 0 => InstancePlayback::Loop,
            1 => InstancePlayback::LoopFrom(first),
            2 if first == 0 => InstancePlayback::PlayOnce,
            2 => InstancePlayback::PlayOnceFrom(first),
            _ => InstancePlayback::SingleFrame(first),
        };
        if before != *playback {
            control.get_or_insert(response.id);
        }
        control
    }

    /// R-042 Movie clip `Timeline` section (expanded above `Display` / `Colour effect`):
    /// `Start at frame` Stepper (`1..=frame_count`) + tertiary note.
    pub(crate) fn movie_clip_timeline_section_ui(
        ui: &mut egui::Ui,
        count: u32,
        playback: &mut InstancePlayback,
    ) -> Option<egui::Id> {
        use neoflash_ui::{TextStyle, components as nf, typography::rich};
        let count = count.max(1);
        let first = match *playback {
            InstancePlayback::Independent | InstancePlayback::Loop | InstancePlayback::PlayOnce => {
                0
            }
            InstancePlayback::LoopFrom(f)
            | InstancePlayback::PlayOnceFrom(f)
            | InstancePlayback::SingleFrame(f) => f,
        };
        let mut start_at = (first.saturating_add(1).min(count)) as f64;
        let width = ui.available_width();
        let field = ui.add(
            nf::Field::new(&mut start_at, "Start at frame")
                .prefix("Start at frame")
                .range(1.0..=count as f64)
                .step(1.0)
                .decimals(0)
                .stepper()
                .width(width),
        );
        let p = neoflash_ui::palette(ui.ctx());
        ui.add(
            egui::Label::new(
                rich(
                    "Scripts that call gotoAndPlay take over after it starts.",
                    TextStyle::Small,
                )
                .color(p.text_tertiary),
            )
            .wrap(),
        );
        if field.changed() {
            let next_first = (start_at.round() as u32).clamp(1, count) - 1;
            *playback = match (*playback, next_first) {
                (InstancePlayback::Independent, 0) => InstancePlayback::Independent,
                (InstancePlayback::Loop | InstancePlayback::LoopFrom(_), 0) => {
                    InstancePlayback::Loop
                }
                (
                    InstancePlayback::Independent
                    | InstancePlayback::Loop
                    | InstancePlayback::LoopFrom(_),
                    f,
                ) => InstancePlayback::LoopFrom(f),
                (InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_), 0) => {
                    InstancePlayback::PlayOnce
                }
                (InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_), f) => {
                    InstancePlayback::PlayOnceFrom(f)
                }
                (InstancePlayback::SingleFrame(_), f) => InstancePlayback::SingleFrame(f),
            };
            return Some(field.id);
        }
        None
    }

    /// Set the 1-based `First frame` (`1..=frame_count`) on all editable selected `Graphic` or
    /// `MovieClip` instances at the current frame in one Undo step. If an `Independent` MovieClip
    /// is given a `First frame > 1`, switches it to `LoopFrom(first_frame - 1)` so its start offset
    /// takes effect on the timeline.
    #[cfg(test)]
    pub(crate) fn set_selected_instance_first_frame(&mut self, first_frame: u32) -> bool {
        let targets: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter_map(|(target, element)| {
                if !self.target_editable(&target) {
                    return None;
                }
                let ElementKind::Instance {
                    symbol_id,
                    playback,
                } = &element.kind
                else {
                    return None;
                };
                let sym = self.project.symbol(symbol_id)?;
                if sym.kind == SymbolKind::Button {
                    return None;
                }
                let max_first = sym.timeline.frame_count.max(1).saturating_sub(1);
                let first = first_frame.saturating_sub(1).min(max_first);
                let next = match *playback {
                    InstancePlayback::Independent if first == 0 => InstancePlayback::Independent,
                    InstancePlayback::Independent
                    | InstancePlayback::Loop
                    | InstancePlayback::LoopFrom(_) => {
                        if first == 0 {
                            InstancePlayback::Loop
                        } else {
                            InstancePlayback::LoopFrom(first)
                        }
                    }
                    InstancePlayback::PlayOnce | InstancePlayback::PlayOnceFrom(_) => {
                        if first == 0 {
                            InstancePlayback::PlayOnce
                        } else {
                            InstancePlayback::PlayOnceFrom(first)
                        }
                    }
                    InstancePlayback::SingleFrame(_) => InstancePlayback::SingleFrame(first),
                };
                (next != *playback).then_some((target, next))
            })
            .collect();
        if targets.is_empty() {
            return false;
        }
        self.finish_edits();
        self.transact_named("Set instance first frame", |s| {
            for (target, next_playback) in targets {
                s.ensure_key_for(&target.layer, false);
                let frame = s.frame;
                if let Some(layer) = s
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == target.layer)
                    && let Some(element) = layer
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    && let ElementKind::Instance { playback, .. } = &mut element.kind
                {
                    *playback = next_playback;
                }
            }
        });
        true
    }

    /// Duplicate a non-scene Library symbol with fresh symbol, layer, element and group IDs,
    /// unique `"<name> copy"` naming and cleared class/export linkage. One Undo step.
    pub(crate) fn duplicate_symbol(&mut self, symbol_id: &str) -> Option<Id> {
        if self.project.is_scene(symbol_id) {
            return None;
        }
        let source = self.project.symbol(symbol_id).cloned()?;
        self.finish_edits();
        let existing: HashSet<String> = self
            .project
            .symbols
            .iter()
            .map(|s| s.name.to_lowercase())
            .collect();
        let mut name = format!("{} copy", source.name);
        let mut n = 2_usize;
        while existing.contains(&name.to_lowercase()) {
            name = format!("{} copy {n}", source.name);
            n += 1;
        }
        let id = new_id("symbol");
        let mut copy = source;
        copy.id = id.clone();
        copy.name = name;
        copy.class_name = None;
        copy.linkage_id = None;
        crate::panels::scene::refresh_timeline_ids(&mut copy.timeline);
        self.transact_named("Duplicate symbol", |s| {
            s.project.symbols.push(copy);
        });
        self.library_selected = Some(id.clone());
        Some(id)
    }

    /// Delete a non-scene Library symbol, removing any placed instances across all symbol timelines
    /// and exiting symbol-edit mode if currently inside it. One Undo step.
    pub(crate) fn delete_symbol(&mut self, symbol_id: &str) -> bool {
        if symbol_id == self.project.root || self.project.is_scene(symbol_id) {
            self.message("Scene symbols cannot be deleted from the Library.");
            return false;
        }
        let Some(name) = self.project.symbol(symbol_id).map(|s| s.name.clone()) else {
            return false;
        };
        self.finish_edits();
        let removed_instances: usize = self
            .project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .filter(|e| matches!(&e.kind, ElementKind::Instance { symbol_id: sid, .. } if sid == symbol_id))
            .count();

        self.transact_named("Delete symbol", |s| {
            for owner in &mut s.project.symbols {
                for layer in &mut owner.timeline.layers {
                    for key in &mut layer.keyframes {
                        key.elements.retain(|el| {
                            !matches!(&el.kind, ElementKind::Instance { symbol_id: sid, .. } if sid == symbol_id)
                        });
                    }
                    layer.normalize_motion_keys();
                }
            }
            s.project.symbols.retain(|symbol| symbol.id != symbol_id);
        });

        if let Some(pos) = self.editing.iter().position(|id| id == symbol_id) {
            self.editing.truncate(pos);
            if self.editing.is_empty() {
                self.editing.push(self.project.root.clone());
            }
            self.frame = 0;
            self.layer = 0;
            self.clear_selection();
        }
        if self.library_selected.as_deref() == Some(symbol_id) {
            self.library_selected = None;
        }
        self.normalize_selection();
        if removed_instances > 0 {
            self.message(format!(
                "Deleted symbol “{name}” and {removed_instances} placed instance{}.",
                if removed_instances == 1 { "" } else { "s" }
            ));
        } else {
            self.message(format!("Deleted symbol “{name}”."));
        }
        true
    }

    /// Whether the editable selection contains at least one symbol instance that can be swapped to
    /// `target_symbol_id` without introducing a cyclic symbol reference.
    pub(crate) fn can_swap_instance_symbol(&self, target_symbol_id: &str) -> bool {
        if self.project.is_scene(target_symbol_id)
            || self.project.symbol(target_symbol_id).is_none()
        {
            return false;
        }
        if crate::app::reaches(
            &self.project,
            target_symbol_id,
            self.symbol_id(),
            &mut HashSet::new(),
        ) {
            return false;
        }
        self.sampled_targets().into_iter().any(|(target, element)| {
            self.target_editable(&target)
                && matches!(
                    &element.kind,
                    ElementKind::Instance { symbol_id, .. } if symbol_id != target_symbol_id
                )
        })
    }

    /// Swap editable selected symbol instances to `target_symbol_id`, preserving transform,
    /// instance name, scripts, accessibility and compatible playback settings. Rejects cycles.
    /// One Undo step.
    pub(crate) fn swap_instance_symbol(&mut self, target_symbol_id: &str) -> bool {
        if self.project.is_scene(target_symbol_id) {
            return false;
        }
        let Some((target_kind, target_frames)) = self
            .project
            .symbol(target_symbol_id)
            .map(|s| (s.kind, s.timeline.frame_count.max(1)))
        else {
            return false;
        };
        let swappable: Vec<_> = self
            .sampled_targets()
            .into_iter()
            .filter_map(|(target, element)| {
                if !self.target_editable(&target) {
                    return None;
                }
                match &element.kind {
                    ElementKind::Instance { symbol_id, .. } if symbol_id != target_symbol_id => {
                        let old_kind = self.project.symbol(symbol_id).map(|s| s.kind);
                        Some((target, old_kind))
                    }
                    _ => None,
                }
            })
            .collect();
        if swappable.is_empty() {
            return false;
        }
        if crate::app::reaches(
            &self.project,
            target_symbol_id,
            self.symbol_id(),
            &mut HashSet::new(),
        ) {
            self.message("Cannot swap to this symbol: it would create a recursive symbol.");
            return false;
        }
        self.finish_edits();
        self.transact_named("Swap symbol", |s| {
            for (target, old_kind) in swappable {
                s.ensure_key_for(&target.layer, false);
                let frame = s.frame;
                if let Some(layer) = s
                    .symbol_mut()
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == target.layer)
                    && let Some(element) = layer
                        .key_at_mut(frame)
                        .and_then(|k| k.elements.iter_mut().find(|e| e.id == target.element))
                    && let ElementKind::Instance {
                        symbol_id,
                        playback,
                    } = &mut element.kind
                {
                    *playback =
                        adapt_swapped_playback(*playback, old_kind, target_kind, target_frames);
                    *symbol_id = target_symbol_id.to_owned();
                }
            }
        });
        true
    }

    /// Whether `layer_index` on the active timeline is editable and contains at least one symbol
    /// instance (at the playhead or across its keyframes) that can be replaced with
    /// `target_symbol_id` without introducing a cyclic symbol reference.
    pub(crate) fn can_swap_layer_symbol(&self, layer_index: usize, target_symbol_id: &str) -> bool {
        if self.project.is_scene(target_symbol_id)
            || self.project.symbol(target_symbol_id).is_none()
        {
            return false;
        }
        if crate::app::reaches(
            &self.project,
            target_symbol_id,
            self.symbol_id(),
            &mut HashSet::new(),
        ) {
            return false;
        }
        let timeline = &self.symbol().timeline;
        let Some(layer) = timeline.layers.get(layer_index) else {
            return false;
        };
        if layer.properties.role == LayerRole::Folder
            || layer_effectively_locked(timeline, layer)
            || !layer_effectively_visible(timeline, layer)
        {
            return false;
        }
        let sampled = sample_layer_in(timeline, layer, self.frame);
        let has_at_frame = sampled.iter().any(|el| {
            matches!(
                &el.kind,
                ElementKind::Instance { symbol_id, .. } if symbol_id != target_symbol_id
            )
        });
        if has_at_frame {
            return true;
        }
        layer.keyframes.iter().flat_map(|k| &k.elements).any(|el| {
            matches!(
                &el.kind,
                ElementKind::Instance { symbol_id, .. } if symbol_id != target_symbol_id
            )
        })
    }

    /// AE-style symbol replacement on `layer_index` (or the current selection when `layer_index ==
    /// self.layer` and swappable instances are explicitly selected): replaces the symbol instance(s)
    /// on the layer across their keyframe track while preserving transform, instance name, scripts,
    /// filters, blend mode and compatible playback settings. One Undo step.
    pub(crate) fn swap_layer_or_selected_symbol(
        &mut self,
        layer_index: usize,
        target_symbol_id: &str,
    ) -> bool {
        if layer_index == self.layer && self.can_swap_instance_symbol(target_symbol_id) {
            return self.swap_instance_symbol(target_symbol_id);
        }
        if self.project.is_scene(target_symbol_id) {
            return false;
        }
        let Some((target_kind, target_frames)) = self
            .project
            .symbol(target_symbol_id)
            .map(|s| (s.kind, s.timeline.frame_count.max(1)))
        else {
            return false;
        };
        if crate::app::reaches(
            &self.project,
            target_symbol_id,
            self.symbol_id(),
            &mut HashSet::new(),
        ) {
            self.message("Cannot swap to this symbol: it would create a recursive symbol.");
            return false;
        }
        if !self.can_swap_layer_symbol(layer_index, target_symbol_id) {
            return false;
        }
        let (layer_id, target_element_ids, ensure_at_playhead) = {
            let timeline = &self.symbol().timeline;
            let layer = &timeline.layers[layer_index];
            let sampled_ids: HashSet<Id> = sample_layer_in(timeline, layer, self.frame)
                .into_iter()
                .filter_map(|el| match &el.kind {
                    ElementKind::Instance { symbol_id, .. } if symbol_id != target_symbol_id => {
                        Some(el.id)
                    }
                    _ => None,
                })
                .collect();
            if !sampled_ids.is_empty() {
                (layer.id.clone(), sampled_ids, true)
            } else {
                let all_ids: HashSet<Id> = layer
                    .keyframes
                    .iter()
                    .flat_map(|k| &k.elements)
                    .filter_map(|el| match &el.kind {
                        ElementKind::Instance { symbol_id, .. }
                            if symbol_id != target_symbol_id =>
                        {
                            Some(el.id.clone())
                        }
                        _ => None,
                    })
                    .collect();
                (layer.id.clone(), all_ids, false)
            }
        };
        if target_element_ids.is_empty() {
            return false;
        }
        let symbol_kinds: std::collections::HashMap<Id, SymbolKind> = self
            .project
            .symbols
            .iter()
            .map(|s| (s.id.clone(), s.kind))
            .collect();
        self.finish_edits();
        self.transact_named("Swap symbol", |s| {
            if ensure_at_playhead {
                s.ensure_key_for(&layer_id, false);
            }
            if let Some(layer) = s
                .symbol_mut()
                .timeline
                .layers
                .iter_mut()
                .find(|l| l.id == layer_id)
            {
                for key in &mut layer.keyframes {
                    for element in &mut key.elements {
                        if target_element_ids.contains(&element.id)
                            && let ElementKind::Instance {
                                symbol_id,
                                playback,
                            } = &mut element.kind
                            && symbol_id != target_symbol_id
                        {
                            let old_kind = symbol_kinds.get(symbol_id).copied();
                            *playback = adapt_swapped_playback(
                                *playback,
                                old_kind,
                                target_kind,
                                target_frames,
                            );
                            *symbol_id = target_symbol_id.to_owned();
                        }
                    }
                }
                layer.normalize_motion_keys();
            }
        });
        true
    }

    /// Return the `(zero_based_frame, display_label)` entries for the selected Graphic symbol
    /// instance's Frame Picker (`T-109`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn graphic_frame_picker_entries(&self) -> Vec<(u32, String)> {
        let Some(selected_id) = self.selection.single().map(|t| &t.element) else {
            return Vec::new();
        };
        let Some(sym_id) = self
            .symbol()
            .timeline
            .layers
            .get(self.layer)
            .and_then(|l| l.key_at(self.frame))
            .and_then(|k| k.elements.iter().find(|e| &e.id == selected_id))
            .and_then(|e| match &e.kind {
                ElementKind::Instance { symbol_id, .. } => Some(symbol_id.as_str()),
                _ => None,
            })
        else {
            return Vec::new();
        };
        let Some(sym) = self
            .project
            .symbol(sym_id)
            .filter(|s| s.kind == SymbolKind::Graphic)
        else {
            return Vec::new();
        };
        let count = sym.timeline.frame_count.max(1);
        (0..count)
            .map(|f| {
                let label = sym
                    .timeline
                    .layers
                    .iter()
                    .find_map(|l| {
                        l.keyframes
                            .iter()
                            .find(|k| k.frame == f && !k.label.is_empty())
                            .map(|k| k.label.clone())
                    })
                    .unwrap_or_else(|| format!("Frame {}", f + 1));
                (f, label)
            })
            .collect()
    }

    /// Pick a specific pose frame (`0..count`) for the selected Graphic symbol instance at the
    /// current playhead frame (`T-109`), automatically inserting a keyframe if the playhead is on a
    /// held frame and setting `InstancePlayback::SingleFrame(zero_based_frame)` in one Undo step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pick_graphic_frame(&mut self, zero_based_frame: u32) -> bool {
        self.finish_edits();
        if !self.layer_editable() {
            return false;
        }
        let Some(selected_id) = self.selection.single().map(|t| t.element.clone()) else {
            return false;
        };
        let layer_idx = self.layer;
        let frame = self.frame;
        let Some((sym_id, base_elements)) = self
            .symbol()
            .timeline
            .layers
            .get(layer_idx)
            .and_then(|l| l.key_at(frame))
            .and_then(|k| {
                let el = k.elements.iter().find(|e| e.id == selected_id)?;
                match &el.kind {
                    ElementKind::Instance { symbol_id, .. } => {
                        Some((symbol_id.clone(), k.elements.clone()))
                    }
                    _ => None,
                }
            })
        else {
            return false;
        };
        let Some(count) = self
            .project
            .symbol(&sym_id)
            .filter(|s| s.kind == SymbolKind::Graphic)
            .map(|s| s.timeline.frame_count.max(1))
        else {
            return false;
        };
        let target_pose = zero_based_frame.min(count - 1);
        self.transact_named("Pick graphic frame", |editor| {
            let layer = &mut editor.symbol_mut().timeline.layers[layer_idx];
            if let Some(existing) = layer.keyframes.iter_mut().find(|k| k.frame == frame) {
                if let Some(el) = existing.elements.iter_mut().find(|e| e.id == selected_id) {
                    el.kind = ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::SingleFrame(target_pose),
                    };
                }
            } else {
                let mut elems = base_elements;
                if let Some(el) = elems.iter_mut().find(|e| e.id == selected_id) {
                    el.kind = ElementKind::Instance {
                        symbol_id: sym_id,
                        playback: InstancePlayback::SingleFrame(target_pose),
                    };
                }
                layer.keyframes.push(neoflash_core::Keyframe {
                    frame,
                    elements: elems,
                    ..neoflash_core::Keyframe::default()
                });
            }
            layer.normalize_motion_keys();
        });
        true
    }
}

fn adapt_swapped_playback(
    playback: InstancePlayback,
    old_kind: Option<SymbolKind>,
    target_kind: SymbolKind,
    target_frames: u32,
) -> InstancePlayback {
    let max_frame = target_frames.saturating_sub(1);
    let clamp = |p: InstancePlayback| match p {
        InstancePlayback::SingleFrame(f) => InstancePlayback::SingleFrame(f.min(max_frame)),
        InstancePlayback::LoopFrom(f) => {
            let f = f.min(max_frame);
            if f == 0 {
                InstancePlayback::Loop
            } else {
                InstancePlayback::LoopFrom(f)
            }
        }
        InstancePlayback::PlayOnceFrom(f) => {
            let f = f.min(max_frame);
            if f == 0 {
                InstancePlayback::PlayOnce
            } else {
                InstancePlayback::PlayOnceFrom(f)
            }
        }
        other => other,
    };
    if old_kind == Some(target_kind) {
        return clamp(playback);
    }
    match target_kind {
        SymbolKind::MovieClip => InstancePlayback::Independent,
        SymbolKind::Button => InstancePlayback::SingleFrame(0),
        SymbolKind::Graphic => match playback {
            InstancePlayback::Independent => InstancePlayback::Loop,
            other => clamp(other),
        },
    }
}
