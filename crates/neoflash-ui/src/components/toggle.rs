//! `NF2/Checkbox`, `NF2/Switch`, `NF2/Radio`, `NF2/Slider`.
use super::{State, a11y, focus, galley, text_at};
use crate::icons::{self, i16};
use crate::metrics::TOGGLE_ROW;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Response, Sense, Stroke, StrokeKind, Ui, Widget};

fn label_width(ui: &Ui, label: &str) -> f32 {
    if label.is_empty() {
        0.0
    } else {
        space::S2
            + galley(ui, label, TextStyle::Body, egui::Color32::WHITE)
                .size()
                .x
    }
}

/// `NF2/Checkbox` — 18 px box, accent fill with check when `Checked`.
pub struct Checkbox<'a> {
    checked: &'a mut bool,
    label: &'a str,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, label: &'a str) -> Self {
        Self { checked, label }
    }
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let w = 18.0 + label_width(ui, self.label);
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(w, TOGGLE_ROW.max(20.0)), Sense::click());
        if response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }
        let s = State::of(ui, &response);
        let bx = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 9.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        let painter = ui.painter();
        if *self.checked {
            let c = if s.enabled {
                p.accent_default
            } else {
                p.text_disabled
            };
            painter.rect_filled(bx, cr(radius::SM), c);
            icons::paint(painter, ui.ctx(), bx, i16::CHECK, p.text_on_accent);
        } else {
            let b = if !s.enabled {
                p.border_subtle
            } else if s.hovered {
                p.border_strong
            } else {
                p.border_default
            };
            painter.rect_filled(bx, cr(radius::SM), p.bg_field);
            painter.rect_stroke(
                bx,
                cr(radius::SM),
                Stroke::new(1.0_f32, b),
                StrokeKind::Inside,
            );
        }
        if !self.label.is_empty() {
            let fg = if s.enabled {
                p.text_primary
            } else {
                p.text_disabled
            };
            text_at(
                ui,
                bx.right() + space::S2,
                rect,
                galley(ui, self.label, TextStyle::Body, fg),
                fg,
            );
        }
        focus(ui, s, bx, radius::SM);
        let checked = *self.checked;
        let enabled = response.enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, checked, self.label)
        });
        response
    }
}

/// `NF2/Switch` — 32 × 18 pill, for settings that apply immediately (Preferences, View settings).
pub struct Switch<'a> {
    on: &'a mut bool,
    label: &'a str,
}

impl<'a> Switch<'a> {
    pub fn new(on: &'a mut bool, label: &'a str) -> Self {
        Self { on, label }
    }
}

impl Widget for Switch<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let w = 32.0 + label_width(ui, self.label);
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(w, TOGGLE_ROW), Sense::click());
        if response.clicked() {
            *self.on = !*self.on;
            response.mark_changed();
        }
        let s = State::of(ui, &response);
        let track = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - 9.0),
            egui::vec2(32.0, 18.0),
        );
        let t = ui.ctx().animate_bool_responsive(response.id, *self.on);
        let bg = if !s.enabled {
            p.bg_hover
        } else if *self.on {
            p.accent_default
        } else {
            p.border_strong
        };
        ui.painter().rect_filled(track, cr(9.0), bg);
        let x = egui::lerp(track.left() + 9.0..=track.right() - 9.0, t);
        let knob = if *self.on {
            p.text_on_accent
        } else {
            p.bg_panel
        };
        ui.painter()
            .circle_filled(egui::pos2(x, track.center().y), 7.0, knob);
        if !self.label.is_empty() {
            let fg = if s.enabled {
                p.text_primary
            } else {
                p.text_disabled
            };
            text_at(
                ui,
                track.right() + space::S2,
                rect,
                galley(ui, self.label, TextStyle::Body, fg),
                fg,
            );
        }
        focus(ui, s, track, 9.0);
        let on = *self.on;
        let enabled = response.enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, self.label)
        });
        response
    }
}

/// `NF2/Radio` — 16 px circle; use for 2–4 mutually exclusive options that need labels.
pub struct Radio<'a> {
    selected: bool,
    label: &'a str,
}

impl<'a> Radio<'a> {
    pub fn new(selected: bool, label: &'a str) -> Self {
        Self { selected, label }
    }
}

impl Widget for Radio<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let w = 16.0 + label_width(ui, self.label);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, TOGGLE_ROW), Sense::click());
        let s = State::of(ui, &response);
        let c = egui::pos2(rect.left() + 8.0, rect.center().y);
        if self.selected {
            ui.painter().circle_filled(
                c,
                8.0,
                if s.enabled {
                    p.accent_default
                } else {
                    p.text_disabled
                },
            );
            ui.painter().circle_filled(c, 3.0, p.text_on_accent);
        } else {
            let b = if s.hovered {
                p.border_strong
            } else {
                p.border_default
            };
            ui.painter()
                .circle(c, 7.5, p.bg_field, Stroke::new(1.0_f32, b));
        }
        let fg = if s.enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        text_at(
            ui,
            rect.left() + 16.0 + space::S2,
            rect,
            galley(ui, self.label, TextStyle::Body, fg),
            fg,
        );
        focus(
            ui,
            s,
            egui::Rect::from_center_size(c, egui::vec2(16.0, 16.0)),
            8.0,
        );
        a11y(
            &response,
            egui::WidgetType::RadioButton,
            self.label,
            Some(self.selected),
        );
        response
    }
}

/// `NF2/Slider` — 4 px rail, accent fill to the left, 14 px white knob. Drag, arrows step, ⇧ = ×10.
pub struct Slider<'a> {
    value: &'a mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
    width: f32,
    label: &'a str,
}

impl<'a> Slider<'a> {
    /// `label` is the accessible name (sliders are usually preceded by a visible label or icon).
    pub fn new(value: &'a mut f64, range: std::ops::RangeInclusive<f64>, label: &'a str) -> Self {
        Self {
            value,
            range,
            step: 1.0,
            width: 140.0,
            label,
        }
    }
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl Widget for Slider<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(self.width, TOGGLE_ROW), Sense::click_and_drag());
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let rail = rect.shrink2(egui::vec2(7.0, 0.0));
        if let Some(pos) = response.interact_pointer_pos() {
            let t = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0) as f64;
            let v = lo + (hi - lo) * t;
            let v = (v / self.step).round() * self.step;
            if v != *self.value {
                *self.value = v.clamp(lo, hi);
                response.mark_changed();
            }
        }
        if response.has_focus() {
            let (l, r, shift) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowLeft),
                    i.key_pressed(egui::Key::ArrowRight),
                    i.modifiers.shift,
                )
            });
            let k = if shift { 10.0 } else { 1.0 };
            let d = if r {
                self.step * k
            } else if l {
                -self.step * k
            } else {
                0.0
            };
            if d != 0.0 {
                *self.value = (*self.value + d).clamp(lo, hi);
                response.mark_changed();
            }
        }
        let s = State::of(ui, &response);
        let t = if hi > lo {
            ((*self.value - lo) / (hi - lo)) as f32
        } else {
            0.0
        };
        let y = rect.center().y;
        let x = rail.left() + rail.width() * t;
        let track = egui::Rect::from_min_max(
            egui::pos2(rail.left(), y - 2.0),
            egui::pos2(rail.right(), y + 2.0),
        );
        ui.painter().rect_filled(track, cr(2.0), p.bg_hover);
        let on = if s.enabled {
            p.accent_default
        } else {
            p.text_disabled
        };
        ui.painter().rect_filled(
            egui::Rect::from_min_max(track.min, egui::pos2(x, track.bottom())),
            cr(2.0),
            on,
        );
        ui.painter().circle(
            egui::pos2(x, y),
            7.0,
            p.text_primary,
            Stroke::new(1.0_f32, p.border_default),
        );
        focus(
            ui,
            s,
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(14.0, 14.0)),
            7.0,
        );
        let value = *self.value;
        let enabled = response.enabled();
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, enabled, self.label);
            info.value = Some(value);
            info
        });
        response
    }
}
