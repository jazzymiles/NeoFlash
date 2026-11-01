//! Properties — Motion tween (NF2 `Panel / Properties — Motion tween`, PR-10 / PP-03; reference
//! `motion-panels.png`) and the motion-span edits it shares with the Motion editor.
//!
//! Values of a motion tween live in its keys: the span start key holds the ease and Orient to path;
//! rotation turns are the difference between the start rotation and the last Rotation ◆ key; the path
//! box is the bounds of the X / Y keys. Edits go through one Undo step (or one per field drag).
use super::ease::TweenSpan;
use crate::app::Editor;
use crate::workspace::PanelId;
use eframe::egui::{self, Ui};
use neoflash_core::*;
use neoflash_ui::components as c;
use neoflash_ui::tokens::space;

/// Value of a motion property in the units the panels show: px, degrees, or percent for scale and
/// alpha.
pub(crate) fn to_display(property: TweenProperty, value: f64) -> f64 {
    match property {
        TweenProperty::ScaleX | TweenProperty::ScaleY | TweenProperty::Alpha => value * 100.,
        _ => value,
    }
}

/// Inverse of [`to_display`], clamped to the document's valid transform range.
pub(crate) fn from_display(property: TweenProperty, value: f64) -> f64 {
    match property {
        TweenProperty::ScaleX | TweenProperty::ScaleY => (value / 100.).clamp(-1e4, 1e4),
        TweenProperty::Alpha => (value / 100.).clamp(0., 1.),
        TweenProperty::Rotation | TweenProperty::Skew => value.clamp(-1e9, 1e9),
        _ => value.clamp(-1e12, 1e12),
    }
}

fn set_value(transform: &mut Transform, property: TweenProperty, value: f64) {
    match property {
        TweenProperty::X => transform.x = value,
        TweenProperty::Y => transform.y = value,
        TweenProperty::ScaleX => transform.scale_x = value,
        TweenProperty::ScaleY => transform.scale_y = value,
        TweenProperty::Rotation => transform.rotation = value,
        TweenProperty::Skew => transform.skew = value,
        TweenProperty::Alpha => transform.alpha = value,
        TweenProperty::Color => {}
    }
}

/// Ease field value (−100 in … +100 out, Flash acceleration) and its unit; curves without an
/// acceleration equivalent show 0 "(custom)".
fn ease_amount(easing: Easing) -> (f64, &'static str) {
    let value = match easing {
        Easing::Linear => Some(0.),
        Easing::EaseIn => Some(-100.),
        Easing::EaseOut => Some(100.),
        Easing::Flash { acceleration } => Some(acceleration),
        Easing::EaseInOut | Easing::CubicBezier { .. } => None,
    };
    match value {
        Some(v) if v < 0. => (v, "(in)"),
        Some(v) if v > 0. => (v, "(out)"),
        Some(v) => (v, ""),
        None => (0., "(custom)"),
    }
}

impl Editor {
    /// Motion tween span under the playhead on the current layer.
    pub(crate) fn motion_tween_span(&self) -> Option<TweenSpan> {
        self.tween_span()
            .filter(|span| span.kind == TweenKind::Motion)
    }

    fn span_layer(&self, span: &TweenSpan) -> Option<&Layer> {
        self.symbol()
            .timeline
            .layers
            .iter()
            .find(|l| l.id == span.layer_id)
    }

    fn span_key_mut(&mut self, span: &TweenSpan, frame: u32) -> Option<&mut Keyframe> {
        self.symbol_mut()
            .timeline
            .layers
            .iter_mut()
            .find(|l| l.id == span.layer_id)?
            .keyframes
            .iter_mut()
            .find(|k| k.frame == frame)
    }

    /// Frames keying `property` in the span: the start key, then the ◆ keys that key it.
    pub(crate) fn motion_key_frames(&self, span: &TweenSpan, property: TweenProperty) -> Vec<u32> {
        let Some(layer) = self.span_layer(span) else {
            return vec![];
        };
        let mut frames: Vec<u32> = layer
            .keyframes
            .iter()
            .filter(|k| {
                k.frame == span.start
                    || (k.frame > span.start
                        && k.frame < span.end
                        && k.keyed_properties
                            .as_ref()
                            .is_some_and(|keyed| keyed.contains(&property)))
            })
            .map(|k| k.frame)
            .collect();
        frames.sort_unstable();
        frames
    }

    /// Value of `property` (display units) of the tweened object at `frame`.
    pub(crate) fn motion_value(
        &self,
        span: &TweenSpan,
        property: TweenProperty,
        frame: u32,
    ) -> Option<f64> {
        let layer = self.span_layer(span)?;
        sample_layer(layer, frame)
            .first()
            .map(|e| to_display(property, property_value(&e.transform, property)))
    }

    /// Stored value of `property` (display units) on the key at `frame`.
    pub(crate) fn motion_key_value(
        &self,
        span: &TweenSpan,
        property: TweenProperty,
        frame: u32,
    ) -> Option<f64> {
        let key = self
            .span_layer(span)?
            .keyframes
            .iter()
            .find(|k| k.frame == frame)?;
        key.elements
            .first()
            .map(|e| to_display(property, property_value(&e.transform, property)))
    }

    /// Run a motion edit as one Undo step, or — with `control` — inside the property-edit session
    /// of that control, so a field or curve drag is one Undo step.
    pub(crate) fn motion_edit(&mut self, control: Option<egui::Id>, f: impl FnOnce(&mut Self)) {
        match control {
            Some(control) => {
                self.begin_property_edit(egui::Id::new("motion-tween-edit"), control);
                f(self);
            }
            None => self.transact(f),
        }
    }

    /// Set `property` of the tweened object at `frame` (display units). Inside the span this keys the
    /// property on a ◆ key (created when the frame has none); the start frame edits the start key.
    pub(crate) fn set_motion_value(
        &mut self,
        span: &TweenSpan,
        property: TweenProperty,
        frame: u32,
        value: f64,
        control: Option<egui::Id>,
    ) {
        if frame < span.start || frame >= span.end || !self.layer_editable() {
            return;
        }
        let value = from_display(property, value);
        let span = span.clone();
        self.motion_edit(control, |s| {
            if frame != span.start {
                s.ensure_key_at(&span.layer_id, frame, false);
            }
            if let Some(key) = s.span_key_mut(&span, frame) {
                for element in &mut key.elements {
                    set_value(&mut element.transform, property, value);
                }
                if let Some(keyed) = &mut key.keyed_properties
                    && !keyed.contains(&property)
                {
                    keyed.push(property);
                }
            }
        });
    }

    /// Add the ◆ key of `property` at the playhead, or remove it when it exists (a ◆ key that keys
    /// nothing else is removed). The span's first frame always keys every property. One Undo step.
    pub(crate) fn toggle_motion_key(&mut self, span: &TweenSpan, property: TweenProperty) {
        let frame = self.frame;
        if frame == span.start {
            self.message("The first frame of a motion tween keys every property.");
            return;
        }
        if frame < span.start || frame >= span.end || !self.layer_editable() {
            return;
        }
        let keyed = self.motion_key_frames(span, property).contains(&frame);
        let span = span.clone();
        self.finish_edits();
        self.transact(|s| {
            if keyed {
                let remove = s.span_key_mut(&span, frame).is_some_and(|key| {
                    if let Some(list) = &mut key.keyed_properties {
                        list.retain(|p| *p != property);
                        list.is_empty()
                    } else {
                        false
                    }
                });
                if remove
                    && let Some(layer) = s
                        .symbol_mut()
                        .timeline
                        .layers
                        .iter_mut()
                        .find(|l| l.id == span.layer_id)
                {
                    layer.keyframes.retain(|k| k.frame != frame);
                }
            } else {
                s.ensure_key_at(&span.layer_id, frame, false);
                if let Some(list) = s
                    .span_key_mut(&span, frame)
                    .and_then(|key| key.keyed_properties.as_mut())
                    && !list.contains(&property)
                {
                    list.push(property);
                }
            }
        });
    }

    /// Rotation turns of the span: whole turns between the start rotation and the last Rotation
    /// ◆ key, and whether they run clockwise.
    pub(crate) fn motion_rotation(&self, span: &TweenSpan) -> (u32, bool) {
        let frames = self.motion_key_frames(span, TweenProperty::Rotation);
        let start = self.motion_key_value(span, TweenProperty::Rotation, span.start);
        let end = frames
            .last()
            .filter(|f| **f != span.start)
            .and_then(|f| self.motion_key_value(span, TweenProperty::Rotation, *f));
        match (start, end) {
            (Some(start), Some(end)) => {
                let delta = end - start;
                ((delta.abs() / 360. + 1e-9).floor() as u32, delta >= 0.)
            }
            _ => (0, true),
        }
    }

    /// Set the rotation turns and direction: the last Rotation ◆ key (created on the span's last
    /// frame when there is none) becomes start ± (360 × turns + the remaining angle).
    pub(crate) fn set_motion_rotation(
        &mut self,
        span: &TweenSpan,
        turns: u32,
        clockwise: bool,
        control: Option<egui::Id>,
    ) {
        let Some(start) = self.motion_key_value(span, TweenProperty::Rotation, span.start) else {
            return;
        };
        let frames = self.motion_key_frames(span, TweenProperty::Rotation);
        let target = frames
            .last()
            .copied()
            .filter(|f| *f != span.start)
            .unwrap_or(span.end - 1);
        if target <= span.start {
            self.message("Extend the motion tween to more than one frame to rotate it.");
            return;
        }
        let end = self
            .motion_key_value(span, TweenProperty::Rotation, target)
            .filter(|_| frames.contains(&target))
            .unwrap_or(start);
        let (old_turns, _) = self.motion_rotation(span);
        let rest = ((end - start).abs() - 360. * old_turns as f64).max(0.);
        let sign = if clockwise { 1. } else { -1. };
        let value = start + sign * (360. * turns as f64 + rest);
        self.set_motion_value(span, TweenProperty::Rotation, target, value, control);
    }

    /// Orient to path of the span (stored on its start key). One Undo step.
    pub(crate) fn set_orient_to_path(&mut self, span: &TweenSpan, on: bool) {
        if !self.layer_editable() {
            return;
        }
        let span = span.clone();
        self.finish_edits();
        self.transact(|s| {
            if let Some(key) = s.span_key_mut(&span, span.start) {
                key.orient_to_path = on;
            }
        });
    }

    /// Bounds `[x, y, w, h]` of the motion path: the X and Y key values of the span.
    pub(crate) fn motion_path_bounds(&self, span: &TweenSpan) -> Option<[f64; 4]> {
        let axis = |property| {
            let values: Vec<f64> = self
                .motion_key_frames(span, property)
                .into_iter()
                .filter_map(|f| self.motion_key_value(span, property, f))
                .collect();
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            min.is_finite().then_some((min, max - min))
        };
        let (x, w) = axis(TweenProperty::X)?;
        let (y, h) = axis(TweenProperty::Y)?;
        Some([x, y, w, h])
    }

    /// Move / scale the motion path to the bounds `[x, y, w, h]`: every X and Y key is mapped from
    /// the current bounds (a zero-size axis only moves).
    pub(crate) fn set_motion_path_bounds(
        &mut self,
        span: &TweenSpan,
        bounds: [f64; 4],
        control: Option<egui::Id>,
    ) {
        let Some(old) = self.motion_path_bounds(span) else {
            return;
        };
        if !self.layer_editable() || !bounds.iter().all(|v| v.is_finite()) {
            return;
        }
        let axes = [
            (TweenProperty::X, old[0], old[2], bounds[0], bounds[2]),
            (TweenProperty::Y, old[1], old[3], bounds[1], bounds[3]),
        ];
        let keys: Vec<(TweenProperty, u32)> = axes
            .iter()
            .flat_map(|(p, ..)| {
                self.motion_key_frames(span, *p)
                    .into_iter()
                    .map(|f| (*p, f))
            })
            .collect();
        let span = span.clone();
        self.motion_edit(control, |s| {
            for (property, frame) in keys {
                let (_, min, size, new_min, new_size) =
                    axes.into_iter().find(|a| a.0 == property).unwrap();
                let scale = if size > 1e-9 {
                    new_size.max(0.) / size
                } else {
                    1.
                };
                if let Some(key) = s.span_key_mut(&span, frame) {
                    for element in &mut key.elements {
                        let v = property_value(&element.transform, property);
                        set_value(
                            &mut element.transform,
                            property,
                            new_min + (v - min) * scale,
                        );
                    }
                }
            }
        });
    }

    /// Properties — Motion tween sections (moved from the planned preview), shown in the frame
    /// Properties while the playhead is inside a motion tween.
    pub(crate) fn motion_tween_properties(&mut self, ui: &mut Ui, span: &TweenSpan) {
        let half = (ui.available_width() - space::S2) / 2.0;
        let editable = self.layer_editable();
        ui.add_enabled_ui(editable, |ui| {
            c::SectionHeader::new("Ease")
                .id_salt("motion-tween-ease")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (mut value, unit) = ease_amount(span.easing);
                        let response = ui.add(
                            c::Field::new(&mut value, "Ease")
                                .prefix("Ease")
                                .unit(unit)
                                .decimals(0)
                                .range(-100.0..=100.0)
                                .stepper()
                                .width(160.0),
                        );
                        if response.changed() {
                            let span = span.clone();
                            self.motion_edit(Some(response.id), |s| {
                                if let Some(key) = s.span_key_mut(&span, span.start) {
                                    key.tween = Some(Easing::Flash {
                                        acceleration: value.round(),
                                    });
                                }
                                s.project.format_version = s.project.format_version.max(5);
                            });
                        }
                        if ui.add(c::Button::ghost("Edit curve…")).clicked() {
                            self.show_panel(PanelId::Ease);
                        }
                    });
                });
            c::SectionHeader::new("Rotation")
                .id_salt("motion-tween-rotation")
                .show(ui, |ui| {
                    let (turns, clockwise) = self.motion_rotation(span);
                    ui.horizontal(|ui| {
                        let mut value = turns as f64;
                        let response = ui.add(
                            c::Field::new(&mut value, "Rotate")
                                .prefix("Rotate")
                                .unit("×")
                                .decimals(0)
                                .range(0.0..=1000.0)
                                .stepper()
                                .width(half),
                        );
                        if response.changed() {
                            self.set_motion_rotation(
                                span,
                                value.round() as u32,
                                clockwise,
                                Some(response.id),
                            );
                        }
                        let label = if clockwise {
                            "Direction: CW"
                        } else {
                            "Direction: CCW"
                        };
                        let mut picked = None;
                        c::Select::new("motion-tween-direction", label)
                            .width(half)
                            .show(ui, |ui| {
                                for (cw, name) in [(true, "CW"), (false, "CCW")] {
                                    if ui
                                        .add(c::MenuItem::new(name).checked(cw == clockwise))
                                        .clicked()
                                    {
                                        picked = Some(cw);
                                    }
                                }
                            });
                        if let Some(cw) = picked.filter(|cw| *cw != clockwise) {
                            self.set_motion_rotation(span, turns, cw, None);
                        }
                    });
                    let mut orient = self
                        .span_layer(span)
                        .and_then(|l| l.keyframes.iter().find(|k| k.frame == span.start))
                        .is_some_and(|k| k.orient_to_path);
                    if ui
                        .add(c::Checkbox::new(&mut orient, "Orient to path"))
                        .changed()
                    {
                        self.set_orient_to_path(span, orient);
                    }
                });
            c::SectionHeader::new("Path")
                .id_salt("motion-tween-path")
                .show(ui, |ui| {
                    let mut bounds = self.motion_path_bounds(span).unwrap_or_default();
                    let mut control = None;
                    for (row, labels) in [[0, 1], [2, 3]].into_iter().zip([["X", "Y"], ["W", "H"]])
                    {
                        ui.horizontal(|ui| {
                            for (index, label) in row.into_iter().zip(labels) {
                                let range = if index < 2 { -1e6..=1e6 } else { 0.0..=1e6 };
                                let response = ui.add(
                                    c::Field::new(&mut bounds[index], label)
                                        .prefix(label)
                                        .decimals(1)
                                        .range(range)
                                        .stepper()
                                        .width(half),
                                );
                                if response.changed() {
                                    control = Some(response.id);
                                }
                            }
                        });
                    }
                    if let Some(control) = control {
                        self.set_motion_path_bounds(span, bounds, Some(control));
                    }
                    ui.horizontal(|ui| {
                        // The stage draws the path whenever the tweened instance is selected; hiding the
                        // path or its frame dots is not wired yet (WIRING-PLAN §7.4).
                        for label in ["Show path", "Show frame dots"] {
                            let mut off = false;
                            let response = ui.add_enabled(false, c::Checkbox::new(&mut off, label));
                            c::not_available(response, label);
                        }
                    });
                });
            c::SectionHeader::new("Options")
                .id_salt("motion-tween-options")
                .show(ui, |ui| {
                    for label in ["Sync graphic symbols", "Snap to motion guide"] {
                        let mut off = false;
                        let response = ui.add_enabled(false, c::Checkbox::new(&mut off, label));
                        c::not_available(response, label);
                    }
                });
        });
    }
}
