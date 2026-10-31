//! `NF2/Anchor Point` (12 × 12, Selected) and the Edit envelope channel lane: sound waveform (`NF2/TL Span`
//! Type=Sound) with the volume envelope and its draggable anchor points (`dialogs-sound-stroke.png`).
use super::timeline::{SpanType, paint_span};
use super::{State, a11y};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget};

/// Hit size of `NF2/Anchor Point`.
pub const ANCHOR_POINT: f32 = 12.0;
/// Painted square inside the hit area.
const ANCHOR_MARK: f32 = 7.0;

/// `NF2/Anchor Point` — path / envelope point. Hover is runtime state; `selected` is the prop.
pub struct AnchorPoint {
    pub selected: bool,
}

impl AnchorPoint {
    /// Paint centred at `center`.
    pub fn paint(&self, ui: &Ui, center: egui::Pos2, hovered: bool) {
        let p = palette(ui.ctx());
        let mark = Rect::from_center_size(center, egui::vec2(ANCHOR_MARK, ANCHOR_MARK));
        let fill = if self.selected {
            p.accent_default
        } else if hovered {
            p.accent_subtle
        } else {
            p.bg_raised
        };
        ui.painter().rect_filled(mark, cr(1.0), fill);
        ui.painter().rect_stroke(
            mark,
            cr(1.0),
            Stroke::new(1.0_f32, p.accent_default),
            StrokeKind::Inside,
        );
    }
}

impl Widget for AnchorPoint {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ANCHOR_POINT, ANCHOR_POINT),
            Sense::click_and_drag(),
        );
        let s = State::of(ui, &response);
        self.paint(ui, rect.center(), s.hovered);
        a11y(
            &response,
            egui::WidgetType::Button,
            "Anchor point",
            Some(self.selected),
        );
        response
    }
}

/// Edit envelope channel lane: two waveform rows, envelope polyline and anchor points.
/// `points` are `(time 0…1, volume 0…1)`; drag a point to move it, click the lane to add one.
pub struct EnvelopeLane<'a> {
    pub channel: &'a str,
    pub points: &'a mut Vec<(f32, f32)>,
    pub selected: Option<usize>,
    pub height: f32,
}

impl Widget for EnvelopeLane<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, mut response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), self.height),
            Sense::click(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, cr(radius::MD), p.bg_field);
        let half = rect.height() / 2.0;
        for row in 0..2 {
            let r = Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + row as f32 * half),
                egui::vec2(rect.width(), half),
            );
            paint_span(
                painter,
                ui.ctx(),
                r.shrink2(egui::vec2(1.0, 0.0)),
                SpanType::Sound,
                true,
            );
        }
        painter.hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1.0_f32, p.border_subtle),
        );
        painter.rect_stroke(
            rect,
            cr(radius::MD),
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
        painter.text(
            rect.left_top() + egui::vec2(space::S2, space::S1),
            egui::Align2::LEFT_TOP,
            self.channel,
            crate::typography::font(TextStyle::Small),
            p.text_secondary,
        );
        let inner = rect.shrink(ANCHOR_POINT / 2.0);
        let at = |(t, v): (f32, f32)| {
            egui::pos2(
                inner.left() + t * inner.width(),
                inner.bottom() - v * inner.height(),
            )
        };
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let t = ((pos.x - inner.left()) / inner.width()).clamp(0.0, 1.0);
            let v = ((inner.bottom() - pos.y) / inner.height()).clamp(0.0, 1.0);
            let i = self.points.partition_point(|q| q.0 < t);
            self.points.insert(i, (t, v));
            response.mark_changed();
        }
        let line: Vec<egui::Pos2> = self.points.iter().map(|q| at(*q)).collect();
        ui.painter().add(egui::Shape::line(
            line,
            Stroke::new(1.5_f32, p.accent_default),
        ));
        for i in 0..self.points.len() {
            let c = at(self.points[i]);
            let hit = Rect::from_center_size(c, egui::vec2(ANCHOR_POINT, ANCHOR_POINT));
            let r = ui.interact(hit, response.id.with(("point", i)), Sense::drag());
            if r.dragged() {
                let d = r.drag_delta();
                let (t, v) = self.points[i];
                let lo = if i == 0 { 0.0 } else { self.points[i - 1].0 };
                let hi = if i + 1 == self.points.len() {
                    1.0
                } else {
                    self.points[i + 1].0
                };
                self.points[i] = (
                    (t + d.x / inner.width()).clamp(lo, hi),
                    (v - d.y / inner.height()).clamp(0.0, 1.0),
                );
                response.mark_changed();
            }
            AnchorPoint {
                selected: self.selected == Some(i),
            }
            .paint(ui, c, State::of(ui, &r).hovered);
        }
        a11y(&response, egui::WidgetType::Other, self.channel, None);
        response
    }
}
