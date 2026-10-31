//! `NF2/Gradient Stop` (12 × 16, Selected) and `NF2/Gradient Bar` (232 × 40): gradient preview with colour
//! stops. Drag a stop to move it, click the bar to add one, drag a stop off (down) to remove it.
//!
//! Stop colours are document colour data; the chrome uses palette tokens only.
use super::{State, a11y};
use crate::theme::{cr, palette};
use crate::tokens::radius;
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget};

/// Size of `NF2/Gradient Stop`.
pub const GRADIENT_STOP: egui::Vec2 = egui::vec2(12.0, 16.0);
/// Height of `NF2/Gradient Bar` (bar 20 + 4 gap + stop 16).
pub const GRADIENT_BAR: f32 = 40.0;
const BAR_H: f32 = 20.0;
/// Distance below the bar after which a dragged stop is removed.
const REMOVE_DRAG: f32 = 24.0;

/// `NF2/Gradient Stop` — pointer with the stop colour. `Selected` = the stop the mixer edits.
pub struct GradientStop {
    pub color: Color32,
    pub selected: bool,
}

impl GradientStop {
    /// Paint a stop whose pointer tip is at the top centre of `rect` (12 × 16).
    pub fn paint(&self, ui: &Ui, rect: Rect, hovered: bool) {
        let p = palette(ui.ctx());
        let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
        let cx = rect.center().x;
        let outline = vec![
            egui::pos2(cx, t),
            egui::pos2(r, t + 5.0),
            egui::pos2(r, b),
            egui::pos2(l, b),
            egui::pos2(l, t + 5.0),
        ];
        let (fill, edge) = if self.selected {
            (p.accent_subtle, p.accent_default)
        } else if hovered {
            (p.bg_hover, p.border_strong)
        } else {
            (p.bg_raised, p.border_strong)
        };
        ui.painter().add(egui::Shape::convex_polygon(
            outline,
            fill,
            Stroke::new(1.0_f32, edge),
        ));
        let chip = Rect::from_center_size(egui::pos2(cx, b - 5.0), egui::vec2(6.0, 6.0));
        super::display::paint_swatch(ui, chip, self.color, 1.0);
    }
}

impl Widget for GradientStop {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(GRADIENT_STOP, Sense::click());
        let s = State::of(ui, &response);
        self.paint(ui, rect, s.hovered);
        a11y(
            &response,
            egui::WidgetType::Button,
            "Gradient stop",
            Some(self.selected),
        );
        response
    }
}

/// `NF2/Gradient Bar`. `stops` are `(position 0…1, colour)` sorted by position.
pub struct GradientBar<'a> {
    pub stops: &'a mut Vec<(f32, Color32)>,
    pub selected: &'a mut usize,
    pub width: f32,
}

impl GradientBar<'_> {
    /// Colour of the gradient at `t` (linear interpolation in sRGB, like the stage preview).
    pub fn sample(stops: &[(f32, Color32)], t: f32) -> Color32 {
        let Some(first) = stops.first() else {
            return Color32::TRANSPARENT;
        };
        if t <= first.0 {
            return first.1;
        }
        for pair in stops.windows(2) {
            let ((a, ca), (b, cb)) = (pair[0], pair[1]);
            if t <= b {
                let k = if b > a { (t - a) / (b - a) } else { 0.0 };
                return lerp(ca, cb, k);
            }
        }
        stops[stops.len() - 1].1
    }
}

fn lerp(a: Color32, b: Color32, k: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

impl Widget for GradientBar<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, mut response) =
            ui.allocate_exact_size(egui::vec2(self.width, GRADIENT_BAR), Sense::hover());
        let bar = Rect::from_min_size(rect.min, egui::vec2(rect.width(), BAR_H));
        // Stops travel on the bar inset by half a stop so the end stops stay inside the component.
        let track = bar.x_range().shrink(GRADIENT_STOP.x / 2.0);
        let x_of = |t: f32| track.min + t * track.span();
        let t_of = |x: f32| ((x - track.min) / track.span()).clamp(0.0, 1.0);

        let bar_r = ui.interact(bar, response.id.with("bar"), Sense::click());
        if bar_r.clicked()
            && let Some(pos) = bar_r.interact_pointer_pos()
        {
            let t = t_of(pos.x);
            let color = GradientBar::sample(self.stops, t);
            let i = self.stops.partition_point(|s| s.0 < t);
            self.stops.insert(i, (t, color));
            *self.selected = i;
            response.mark_changed();
        }

        // Bar: checkerboard-free gradient (stops are opaque in the preview) with a subtle border.
        let mut mesh = egui::Mesh::default();
        let mut cuts: Vec<(f32, Color32)> = vec![(0.0, GradientBar::sample(self.stops, 0.0))];
        for (t, c) in self.stops.iter() {
            cuts.push((x_of(*t) - bar.left(), *c));
        }
        cuts.push((bar.width(), GradientBar::sample(self.stops, 1.0)));
        for (x, c) in &cuts {
            mesh.colored_vertex(egui::pos2(bar.left() + x, bar.top()), *c);
            mesh.colored_vertex(egui::pos2(bar.left() + x, bar.bottom()), *c);
        }
        for k in 0..cuts.len() as u32 - 1 {
            let a = 2 * k;
            mesh.add_triangle(a, a + 2, a + 3);
            mesh.add_triangle(a, a + 3, a + 1);
        }
        ui.painter().with_clip_rect(bar.shrink(1.0)).add(mesh);
        ui.painter().rect_stroke(
            bar,
            cr(radius::SM),
            Stroke::new(1.0_f32, p.border_default),
            StrokeKind::Inside,
        );

        let mut remove = None;
        let mut moved = None;
        for i in 0..self.stops.len() {
            let (t, color) = self.stops[i];
            let stop = Rect::from_min_size(
                egui::pos2(
                    x_of(t) - GRADIENT_STOP.x / 2.0,
                    rect.bottom() - GRADIENT_STOP.y,
                ),
                GRADIENT_STOP,
            );
            let r = ui.interact(stop, response.id.with(("stop", i)), Sense::click_and_drag());
            if r.clicked() || r.drag_started() {
                *self.selected = i;
            }
            if r.dragged()
                && let Some(pos) = r.interact_pointer_pos()
            {
                moved = Some((i, t_of(pos.x)));
            }
            if r.drag_stopped()
                && let Some(pos) = r.interact_pointer_pos()
                && pos.y > rect.bottom() + REMOVE_DRAG
                && self.stops.len() > 2
            {
                remove = Some(i);
            }
            GradientStop {
                color,
                selected: *self.selected == i,
            }
            .paint(ui, stop, State::of(ui, &r).hovered);
            a11y(
                &r,
                egui::WidgetType::Button,
                "Gradient stop",
                Some(*self.selected == i),
            );
        }
        if let Some((i, t)) = moved {
            self.stops[i].0 = t;
            let selected = self.stops[i];
            self.stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            *self.selected = self.stops.iter().position(|s| *s == selected).unwrap_or(i);
            response.mark_changed();
        }
        if let Some(i) = remove {
            self.stops.remove(i);
            *self.selected = (*self.selected).min(self.stops.len() - 1);
            response.mark_changed();
        }
        a11y(&response, egui::WidgetType::Other, "Gradient", None);
        response
    }
}
