//! `NF2/Field` multi-line variant (Accessibility description, notes): the field frame around a
//! wrapped `TextEdit`, `NF2/Body`, optional hint and character limit.
use super::{State, a11y};
use crate::metrics::CONTROL;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Color32, Response, Sense, Stroke, StrokeKind, Ui, Widget};

/// Multi-line text field. Height = `rows` × `NF2/Body` line height + 16 px padding (≥ 32).
pub struct TextArea<'a> {
    text: &'a mut String,
    label: &'a str,
    hint: &'a str,
    rows: usize,
    char_limit: usize,
}

impl<'a> TextArea<'a> {
    /// `label` is the accessible name.
    pub fn new(text: &'a mut String, label: &'a str) -> Self {
        Self {
            text,
            label,
            hint: "",
            rows: 3,
            char_limit: usize::MAX,
        }
    }
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }
    pub fn char_limit(mut self, limit: usize) -> Self {
        self.char_limit = limit;
        self
    }
}

impl Widget for TextArea<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let line = TextStyle::Body.spec().line_height;
        let height = (self.rows as f32 * line + space::S2 * 2.0).max(CONTROL);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
        let edit_id = response.id.with("edit");
        let editing = ui.memory(|m| m.has_focus(edit_id));
        let s = State::of(ui, &response);
        let (bg, border, w) = if !s.enabled {
            (p.bg_panel, p.border_subtle, 1.0)
        } else if editing {
            (p.bg_field, p.border_focus, 2.0)
        } else if s.hovered {
            (p.bg_field, p.border_strong, 1.0)
        } else {
            (p.bg_field, p.border_default, 1.0_f32)
        };
        ui.painter().rect_filled(rect, cr(radius::MD), bg);
        ui.painter().rect_stroke(
            rect,
            cr(radius::MD),
            Stroke::new(w, border),
            StrokeKind::Inside,
        );
        let fg = if s.enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        let inner = rect.shrink2(egui::vec2(space::S3, space::S2));
        let edit = ui
            .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                let v = ui.visuals_mut();
                v.override_text_color = Some(fg);
                v.extreme_bg_color = Color32::TRANSPARENT;
                v.widgets.inactive.bg_stroke = Stroke::NONE;
                v.widgets.hovered.bg_stroke = Stroke::NONE;
                v.widgets.active.bg_stroke = Stroke::NONE;
                v.selection.stroke = Stroke::NONE;
                egui::ScrollArea::vertical()
                    .id_salt(edit_id.with("scroll"))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(self.text)
                                .id(edit_id)
                                .frame(false)
                                .char_limit(self.char_limit)
                                .hint_text(
                                    crate::typography::rich(self.hint, TextStyle::Body)
                                        .color(p.text_tertiary),
                                )
                                .font(crate::typography::font(TextStyle::Body))
                                .margin(egui::Margin::ZERO)
                                .desired_rows(self.rows)
                                .desired_width(inner.width()),
                        )
                    })
                    .inner
            })
            .inner;
        let response = response | edit;
        a11y(&response, egui::WidgetType::TextEdit, self.label, None);
        response
    }
}
