//! `NF3/Checkbox`, `NF3/Switch`, `NF3/Radio`, `NF3/Slider` — every size follows the density.
use super::{State, a11y, focus, galley, text_at};
use crate::icons::{self, i16};
use crate::metrics::{
    check_box, field_m, field_num, slider_thumb, switch_height, switch_width, toggle_row,
};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Response, Sense, Stroke, StrokeKind, Ui, Widget};

fn label_width(ui: &Ui, label: &str) -> f32 {
    if label.is_empty() {
        0.0
    } else {
        space::s2()
            + galley(ui, label, TextStyle::Body, egui::Color32::WHITE)
                .size()
                .x
    }
}

/// Vertical centre of a toggle mark, snapped so the box sits on whole points in its row.
fn mark_rect(rect: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let top = rect.top() + ((rect.height() - size.y) / 2.0).round();
    egui::Rect::from_min_size(egui::pos2(rect.left(), top), size)
}

/// `NF3/Checkbox` — row `toggle_row()`, box `check_box()` with a 12 px check (or minus when mixed).
pub struct Checkbox<'a> {
    checked: &'a mut bool,
    label: &'a str,
    mixed: bool,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, label: &'a str) -> Self {
        Self {
            checked,
            label,
            mixed: false,
        }
    }
    /// `Value=Mixed`: the selection disagrees; drawn as a filled box with a minus.
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.mixed = mixed;
        self
    }
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let side = check_box();
        let w = side + label_width(ui, self.label);
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(w, toggle_row()), Sense::click());
        if response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }
        let s = State::of(ui, &response);
        let bx = mark_rect(rect, egui::vec2(side, side));
        let painter = ui.painter();
        if *self.checked || self.mixed {
            let (bg, fg) = if s.enabled {
                (p.accent_default, p.text_on_accent)
            } else {
                (p.bg_hover, p.text_disabled)
            };
            painter.rect_filled(bx, cr(radius::sm()), bg);
            let mark = if self.mixed && !*self.checked {
                i16::MINUS
            } else {
                i16::CHECK
            };
            icons::paint_sized(painter, ui.ctx(), bx, mark, 12.0, fg);
        } else {
            let (bg, b) = if s.enabled {
                (p.bg_field, p.border_strong)
            } else {
                (p.bg_panel, p.border_subtle)
            };
            painter.rect_filled(bx, cr(radius::sm()), bg);
            painter.rect_stroke(
                bx,
                cr(radius::sm()),
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
                bx.right() + space::s2(),
                rect,
                galley(ui, self.label, TextStyle::Body, fg),
                fg,
            );
        }
        focus(ui, s, bx, radius::sm());
        let checked = *self.checked;
        let enabled = response.enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, checked, self.label)
        });
        response
    }
}

/// `NF3/Switch` — `switch_width()` × `switch_height()` pill with a knob 2 pt inside the track; for an
/// application state that applies at once (Autosave, Snapping).
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
        let size = egui::vec2(switch_width(), switch_height());
        let w = size.x + label_width(ui, self.label);
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(w, toggle_row()), Sense::click());
        if response.clicked() {
            *self.on = !*self.on;
            response.mark_changed();
        }
        let s = State::of(ui, &response);
        let track = mark_rect(rect, size);
        let t = ui.ctx().animate_bool_responsive(response.id, *self.on);
        let bg = if !s.enabled {
            p.bg_hover
        } else if *self.on {
            p.accent_default
        } else {
            p.border_strong
        };
        let half = size.y / 2.0;
        ui.painter().rect_filled(track, cr(half), bg);
        let x = egui::lerp(track.left() + half..=track.right() - half, t);
        let knob = if s.enabled {
            p.text_on_accent
        } else {
            p.text_disabled
        };
        ui.painter()
            .circle_filled(egui::pos2(x, track.center().y), half - 2.0, knob);
        if !self.label.is_empty() {
            let fg = if s.enabled {
                p.text_primary
            } else {
                p.text_disabled
            };
            text_at(
                ui,
                track.right() + space::s2(),
                rect,
                galley(ui, self.label, TextStyle::Body, fg),
                fg,
            );
        }
        focus(ui, s, track, half);
        let on = *self.on;
        let enabled = response.enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, self.label)
        });
        response
    }
}

/// `NF3/Radio` — `check_box()` ring with a 6 pt dot; for 2–4 mutually exclusive options that need labels.
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
        let side = check_box();
        let w = side + label_width(ui, self.label);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, toggle_row()), Sense::click());
        let s = State::of(ui, &response);
        let ring = mark_rect(rect, egui::vec2(side, side));
        let c = ring.center();
        let (bg, stroke) = match (self.selected, s.enabled) {
            (true, true) => (p.bg_field, Stroke::new(1.5_f32, p.accent_default)),
            (true, false) => (p.bg_panel, Stroke::new(1.5_f32, p.border_strong)),
            (false, true) => (p.bg_field, Stroke::new(1.0_f32, p.border_strong)),
            (false, false) => (p.bg_panel, Stroke::new(1.0_f32, p.border_subtle)),
        };
        ui.painter()
            .circle(c, side / 2.0 - stroke.width / 2.0, bg, stroke);
        if self.selected {
            let dot = if s.enabled {
                p.accent_default
            } else {
                p.text_disabled
            };
            ui.painter().circle_filled(c, 3.0, dot);
        }
        let fg = if s.enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        text_at(
            ui,
            ring.right() + space::s2(),
            rect,
            galley(ui, self.label, TextStyle::Body, fg),
            fg,
        );
        focus(ui, s, ring, side / 2.0);
        a11y(
            &response,
            egui::WidgetType::RadioButton,
            self.label,
            Some(self.selected),
        );
        response
    }
}

/// How a [`Slider`] shows its value (R-038, the exported compositions of `NF2/Slider`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SliderValue {
    /// Slider only (a separate label or icon names it).
    #[default]
    None,
    /// Slider + [`slider_gap`] + numeric `NF3/Field` ([`slider_field`] wide, unit suffix) bound to the
    /// same value — Brush Size, Smoothing, Trace bitmap parameters.
    Field,
    /// Slider + 8 pt gap + the value as `NF2/Mono Small` `text/secondary` text (read-only).
    Inline,
}

/// Normal-density width of the numeric field in [`SliderValue::Field`]; layout code uses [`slider_field`].
pub const SLIDER_FIELD: f32 = crate::metrics::FIELD_NUM;

/// Width of the numeric field in [`SliderValue::Field`] at the current density (`width/num`).
pub fn slider_field() -> f32 {
    field_num()
}

/// Gap between the rail and the numeric field in [`SliderValue::Field`] (`gap/control`).
pub fn slider_gap() -> f32 {
    space::s2()
}

/// `NF3/Slider` — 2 pt `border/strong` rail, accent filled part, `slider_thumb()` thumb with a 1.5 pt
/// accent ring on `bg/raised`. Drag, arrows step, ⇧ = ×10.
pub struct Slider<'a> {
    value: &'a mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
    width: f32,
    label: &'a str,
    show: SliderValue,
    unit: &'a str,
    decimals: usize,
}

impl<'a> Slider<'a> {
    /// `label` is the accessible name (sliders are usually preceded by a visible label or icon).
    pub fn new(value: &'a mut f64, range: std::ops::RangeInclusive<f64>, label: &'a str) -> Self {
        Self {
            value,
            range,
            step: 1.0,
            width: field_m(),
            label,
            show: SliderValue::None,
            unit: "",
            decimals: 0,
        }
    }
    /// Value presentation next to the rail; `width` stays the rail width.
    pub fn value(mut self, show: SliderValue) -> Self {
        self.show = show;
        self
    }
    /// Unit after the value in `Field` / `Inline` ("px", "%").
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }
    /// Decimals of the shown value (default 0).
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
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
        match self.show {
            SliderValue::None => self.rail(ui),
            SliderValue::Field => {
                let Self {
                    range,
                    step,
                    label,
                    unit,
                    decimals,
                    ..
                } = self;
                let (lo, hi) = (*range.start(), *range.end());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = slider_gap();
                    let rail = Slider {
                        value: &mut *self.value,
                        range: range.clone(),
                        step,
                        width: self.width,
                        label,
                        show: SliderValue::None,
                        unit,
                        decimals,
                    }
                    .rail(ui);
                    let mut field = super::Field::new(self.value, label)
                        .range(lo..=hi)
                        .step(step)
                        .decimals(decimals)
                        .width(slider_field());
                    if !unit.is_empty() {
                        field = field.unit(unit);
                    }
                    rail | ui.add(field)
                })
                .inner
            }
            SliderValue::Inline => {
                let text = slider_value_text(*self.value, self.decimals, self.unit);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::s2();
                    let rail = self.rail(ui);
                    let p = palette(ui.ctx());
                    let fg = if ui.is_enabled() {
                        p.text_secondary
                    } else {
                        p.text_disabled
                    };
                    let g = galley(ui, &text, TextStyle::MonoSmall, fg);
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(g.size().x.max(24.0), toggle_row()),
                        Sense::hover(),
                    );
                    text_at(ui, rect.left(), rect, g, fg);
                    rail
                })
                .inner
            }
        }
    }
}

/// Text of [`SliderValue::Inline`]: the value with `decimals` and an optional unit ("12 px", "50 %").
pub fn slider_value_text(value: f64, decimals: usize, unit: &str) -> String {
    if unit.is_empty() {
        format!("{value:.decimals$}")
    } else {
        format!("{value:.decimals$} {unit}")
    }
}

impl Slider<'_> {
    fn rail(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, mut response) = ui.allocate_exact_size(
            egui::vec2(self.width, toggle_row()),
            Sense::click_and_drag(),
        );
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let thumb = slider_thumb();
        let rail = rect.shrink2(egui::vec2(thumb / 2.0, 0.0));
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
        let y = rect.top() + (rect.height() / 2.0).round();
        let x = rail.left() + rail.width() * t;
        let track = egui::Rect::from_min_max(
            egui::pos2(rail.left(), y - 1.0),
            egui::pos2(rail.right(), y + 1.0),
        );
        // R-101: the empty part of the rail is `border/strong`; `bg/hover` vanished on panels.
        ui.painter().rect_filled(track, cr(1.0), p.border_strong);
        let on = if s.enabled {
            p.accent_default
        } else {
            p.border_strong
        };
        ui.painter().rect_filled(
            egui::Rect::from_min_max(track.min, egui::pos2(x, track.bottom())),
            cr(1.0),
            on,
        );
        ui.painter().circle(
            egui::pos2(x, y),
            thumb / 2.0 - 0.75,
            p.bg_raised,
            Stroke::new(1.5_f32, on),
        );
        focus(
            ui,
            s,
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(thumb, thumb)),
            thumb / 2.0,
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
