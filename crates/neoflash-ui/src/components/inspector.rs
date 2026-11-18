//! Inspector pieces used by Properties, Align, Transform and Color:
//! * [`DragField`] — `NF2/Field` (numeric) whose value itself scrubs (property inspectors);
//! * [`TypeHeader`] — the one-line kind icon + kind name + object name row at the top of every
//!   Properties variant (`Type header` in the v3 surfaces; not a separate Figma component set).
//!
//! `DragField` paints exactly like [`super::Field`] (frame, prefix, value, unit) but keeps egui's
//! `DragValue` interaction, which inspectors rely on for undo grouping: click to type, drag the value or
//! the prefix to scrub (⇧ = fine), ↑/↓ step while typing, Enter / blur commits, Escape discards the
//! typed text without touching the value. The returned [`Response`] is the value editor itself, so it is
//! focusable and keeps its id when a row re-wraps.
use super::{galley, text_at};
use crate::icons::{self, Icon};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{
    self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget, emath::Numeric,
};
use std::ops::RangeInclusive;

/// `NF2/Field` (numeric, inspector variant): prefix ("W", "α", "FPS"), `DragValue` value, suffix (" %", "°").
#[must_use = "add it with ui.add(…)"]
pub struct DragField<'a, N: Numeric> {
    value: &'a mut N,
    label: &'a str,
    prefix: &'a str,
    suffix: &'a str,
    range: RangeInclusive<N>,
    speed: f64,
    max_decimals: Option<usize>,
    width: Option<f32>,
}

impl<'a, N: Numeric> DragField<'a, N> {
    /// `label` is the accessible name ("Width (px)"); `prefix` is the short visible caption ("W").
    pub fn new(value: &'a mut N, label: &'a str) -> Self {
        Self {
            value,
            label,
            prefix: "",
            suffix: "",
            range: N::MIN..=N::MAX,
            speed: 1.0,
            max_decimals: None,
            width: None,
        }
    }
    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }
    /// Unit appended to the value as typed (" %", "°", " px").
    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }
    pub fn range(mut self, range: RangeInclusive<N>) -> Self {
        self.range = range;
        self
    }
    /// Value change per dragged point.
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }
    pub fn max_decimals(mut self, decimals: usize) -> Self {
        self.max_decimals = Some(decimals);
        self
    }
    /// Fixed width; otherwise the field fills the available width.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

impl<N: Numeric> Widget for DragField<'_, N> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = self
            .width
            .unwrap_or(ui.available_width())
            .max(crate::metrics::control());
        let (rect, outer) =
            ui.allocate_exact_size(egui::vec2(width, crate::metrics::control()), Sense::hover());
        let frame_shape = ui.painter().add(egui::Shape::Noop);
        let enabled = ui.is_enabled();
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());

        // Prefix: drag to scrub (§13.4).
        let mut x = rect.left() + space::s3();
        let mut scrubbed = false;
        let mut scrub_hovered = false;
        if !self.prefix.is_empty() {
            let fg = if enabled {
                p.text_tertiary
            } else {
                p.text_disabled
            };
            let g = galley(ui, self.prefix, TextStyle::Small, fg);
            let prefix_rect = Rect::from_min_max(
                rect.left_top(),
                egui::pos2(x + g.size().x + space::s1(), rect.bottom()),
            );
            let scrub = ui.interact(prefix_rect, outer.id.with("scrub"), Sense::drag());
            scrub_hovered = scrub.hovered();
            if enabled && (scrub.hovered() || scrub.dragged()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if enabled && scrub.dragged() {
                let k = if ui.input(|i| i.modifiers.shift) {
                    0.1
                } else {
                    1.0
                };
                let delta = scrub.drag_delta().x as f64 * self.speed * k;
                if delta != 0.0 {
                    let v = (self.value.to_f64() + delta).clamp(lo, hi);
                    let v = if N::INTEGRAL { v.round() } else { v };
                    *self.value = N::from_f64(v);
                    scrubbed = true;
                }
            }
            text_at(ui, x, rect, g, fg);
            x = prefix_rect.right() + space::s1();
        }

        // Value: a frameless DragValue filling the rest of the field.
        let value_rect = Rect::from_min_max(
            egui::pos2(x, rect.top() + 1.0),
            egui::pos2(rect.right() - space::s2(), rect.bottom() - 1.0),
        );
        let fg = if enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        // A child Ui (not a scope) so the parent's cursor stays after the whole field.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(outer.id.with("value"))
                .max_rect(value_rect)
                .layout(
                    egui::Layout::left_to_right(egui::Align::Center)
                        .with_main_align(egui::Align::Min),
                ),
        );
        #[allow(clippy::redundant_closure_call)]
        // Scoped style tweaks for the inner DragValue only.
        let mut response = (|ui: &mut Ui| {
            let style = ui.style_mut();
            style.drag_value_text_style = egui::TextStyle::Body;
            style.spacing.interact_size = value_rect.size();
            style.spacing.button_padding = egui::Vec2::ZERO;
            style.explanation_tooltips = false;
            let v = &mut style.visuals;
            for w in [
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.bg_fill = Color32::TRANSPARENT;
                w.weak_bg_fill = Color32::TRANSPARENT;
                w.bg_stroke = Stroke::NONE;
                w.expansion = 0.0;
                w.fg_stroke.color = fg;
            }
            v.extreme_bg_color = Color32::TRANSPARENT;
            v.selection.stroke = Stroke::new(0.0_f32, fg);
            v.override_text_color = Some(fg);
            let mut drag = egui::DragValue::new(self.value)
                .range(self.range)
                .clamp_existing_to_range(false)
                .speed(self.speed)
                .update_while_editing(false);
            if !self.suffix.is_empty() {
                drag = drag.suffix(self.suffix);
            }
            if let Some(d) = self.max_decimals {
                drag = drag.max_decimals(d);
            }
            ui.add(drag)
        })(&mut child);
        if scrubbed {
            response.mark_changed();
        }

        // Frame painted underneath, now that focus and hover are known.
        let editing = ui.memory(|m| m.has_focus(response.id));
        let hovered = enabled && (outer.hovered() || response.hovered() || scrub_hovered);
        let (bg, border, w) = if !enabled {
            (p.bg_panel, p.border_subtle, 1.0)
        } else if editing {
            (p.bg_field, p.border_focus, 2.0)
        } else if hovered {
            (p.bg_field, p.border_strong, 1.0)
        } else {
            (p.bg_field, p.border_default, 1.0_f32)
        };
        ui.painter().set(
            frame_shape,
            egui::epaint::RectShape::new(
                rect,
                cr(radius::md()),
                bg,
                Stroke::new(w, border),
                StrokeKind::Inside,
            ),
        );

        let value = self.value.to_f64();
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::DragValue, enabled, self.label);
            info.value = Some(value);
            info
        });
        response
    }
}

/// Properties type header — one `panel_tab()` line: 16 px kind icon, kind name (`NF2/Body Strong`)
/// and the instance / document name in `text/tertiary`, ending with "…" when it does not fit, e.g.
/// "Movie clip  Instance of “Orbit”". The row starts at the left edge of its Ui (the panel pads).
pub struct TypeHeader<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub subtitle: &'a str,
}

impl Widget for TypeHeader<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        type_header(ui, self.icon, self.title, self.subtitle)
    }
}

/// Type header of tool properties (Brush, Pencil). Same one-line form as [`TypeHeader`]; kept as a
/// separate type for its callers.
pub struct CompactTypeHeader<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub subtitle: &'a str,
}

impl Widget for CompactTypeHeader<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        type_header(ui, self.icon, self.title, self.subtitle)
    }
}

fn type_header(ui: &mut Ui, icon: Icon, title: &str, subtitle: &str) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), crate::metrics::panel_tab()),
        Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        let side = crate::metrics::ICON_UI;
        let tile = Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - side / 2.0),
            egui::vec2(side, side),
        );
        icons::paint_sized(ui.painter(), ui.ctx(), tile, icon, side, p.text_secondary);
        let mut x = tile.right() + space::s2();
        let name = super::galley_truncated(
            ui,
            title,
            TextStyle::BodyStrong,
            p.text_primary,
            rect.right() - x,
        );
        let name_w = name.size().x;
        text_at(ui, x, rect, name, p.text_primary);
        x += name_w + space::s2();
        let room = rect.right() - x;
        if !subtitle.is_empty() && room > space::s4() {
            let sub = super::galley_truncated(ui, subtitle, TextStyle::Body, p.text_tertiary, room);
            text_at(ui, x, rect, sub, p.text_tertiary);
        }
    }
    let label = if subtitle.is_empty() {
        title.to_owned()
    } else {
        format!("{title} · {subtitle}")
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    response
}
