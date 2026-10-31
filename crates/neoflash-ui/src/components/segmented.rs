//! `NF2/Segmented Control` made of `NF2/Segment`s (Selection / Tool, Align to Selection / Stage, RGB / HSB).
use super::{State, a11y, galley, text_at};
use crate::metrics::ROW;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Response, Sense, Stroke, StrokeKind, Ui};

pub struct Segmented;

impl Segmented {
    /// Show segments for `options`; returns the response of the clicked segment (if any) and
    /// writes the new value. `full_width` stretches segments to the available width.
    pub fn show<T: PartialEq + Copy>(
        ui: &mut Ui,
        value: &mut T,
        options: &[(T, &str)],
        full_width: bool,
    ) -> Option<Response> {
        let p = palette(ui.ctx());
        let widths: Vec<f32> = options
            .iter()
            .map(|(_, l)| galley(ui, l, TextStyle::Body, p.text_primary).size().x + 2.0 * space::S3)
            .collect();
        let inner: f32 = widths.iter().sum();
        let total = if full_width {
            ui.available_width()
        } else {
            inner + 6.0
        };
        let (outer, _) = ui.allocate_exact_size(egui::vec2(total, ROW + 6.0), Sense::hover());
        ui.painter().rect_filled(outer, cr(radius::LG), p.bg_field);
        ui.painter().rect_stroke(
            outer,
            cr(radius::LG),
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
        let scale = (total - 6.0) / inner;
        let mut x = outer.left() + 3.0;
        let mut clicked = None;
        for ((v, label), w) in options.iter().zip(widths) {
            let w = w * scale;
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, outer.top() + 3.0), egui::vec2(w, ROW));
            let id = ui.id().with(("segment", label));
            let response = ui.interact(rect, id, Sense::click());
            let s = State::of(ui, &response);
            let selected = *v == *value;
            if selected {
                ui.painter().rect_filled(rect, cr(radius::MD), p.bg_raised);
                ui.painter().rect_stroke(
                    rect,
                    cr(radius::MD),
                    Stroke::new(1.0_f32, p.border_default),
                    StrokeKind::Inside,
                );
            } else if s.hovered {
                ui.painter().rect_filled(rect, cr(radius::MD), p.bg_hover);
            }
            let fg = if !s.enabled {
                p.text_disabled
            } else if selected || s.hovered {
                p.text_primary
            } else {
                p.text_secondary
            };
            let g = galley(ui, label, TextStyle::Body, fg);
            text_at(ui, rect.center().x - g.size().x / 2.0, rect, g, fg);
            super::focus(ui, s, rect, radius::MD);
            a11y(
                &response,
                egui::WidgetType::RadioButton,
                label,
                Some(selected),
            );
            if response.clicked() && !selected {
                *value = *v;
                clicked = Some(response);
            }
            x += w;
        }
        clicked
    }
}
