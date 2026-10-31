//! `NF2/Color Picker` — colour mixer, 248 × 150: saturation / brightness area (204 × 150), hue strip and
//! alpha strip (14 pt each, 10 / 6 pt gaps; geometry from `panel-color-*.png`).
//!
//! The mixer shows the colour being edited; its gradients are document colour data computed from HSV,
//! not UI tokens. Chrome (borders, thumbs) uses palette tokens only.
use super::{State, a11y};
use crate::theme::{cr, palette};
use crate::tokens::radius;
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget};

/// Colour in HSB over sRGB components, as the mixer and Flash's HSB fields use it (not egui's linear `Hsva`):
/// `FF9900` = `hsv(36.0 / 360.0, 1.0, 1.0, 1.0)`. Colour data, never a UI token.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hsb {
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub a: f32,
}

/// Shorthand for [`Hsb`].
pub const fn hsv(h: f32, s: f32, v: f32, a: f32) -> Hsb {
    Hsb { h, s, v, a }
}

impl From<Hsb> for Color32 {
    fn from(c: Hsb) -> Self {
        let h = c.h.rem_euclid(1.0) * 6.0;
        let (s, v) = (c.s.clamp(0.0, 1.0), c.v.clamp(0.0, 1.0));
        let chroma = v * s;
        let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = v - chroma;
        let byte = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgba_unmultiplied(byte(r), byte(g), byte(b), byte_a(c.a))
    }
}

fn byte_a(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Outer size of `NF2/Color Picker`.
pub const COLOR_PICKER: egui::Vec2 = egui::vec2(248.0, 150.0);
const AREA_W: f32 = 204.0;
const STRIP_W: f32 = 14.0;
const AREA_GAP: f32 = 10.0;
const STRIP_GAP: f32 = 6.0;

/// `NF2/Color Picker`. Dragging the area sets saturation / brightness, the strips set hue and alpha.
pub struct ColorPicker<'a> {
    pub color: &'a mut Hsb,
}

impl ColorPicker<'_> {
    /// Rects of the area, hue strip and alpha strip inside `rect`.
    pub fn layout(rect: Rect) -> (Rect, Rect, Rect) {
        let area = Rect::from_min_size(rect.min, egui::vec2(AREA_W, rect.height()));
        let hue = Rect::from_min_size(
            egui::pos2(area.right() + AREA_GAP, rect.top()),
            egui::vec2(STRIP_W, rect.height()),
        );
        let alpha = Rect::from_min_size(
            egui::pos2(hue.right() + STRIP_GAP, rect.top()),
            egui::vec2(STRIP_W, rect.height()),
        );
        (area, hue, alpha)
    }
}

impl Widget for ColorPicker<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, mut response) = ui.allocate_exact_size(COLOR_PICKER, Sense::hover());
        let (area, hue, alpha) = Self::layout(rect);
        let id = response.id;
        let area_r = ui.interact(area, id.with("sv"), Sense::click_and_drag());
        let hue_r = ui.interact(hue, id.with("hue"), Sense::click_and_drag());
        let alpha_r = ui.interact(alpha, id.with("alpha"), Sense::click_and_drag());
        let c = self.color;
        let before = *c;
        if let Some(pos) = area_r.interact_pointer_pos()
            && (area_r.dragged() || area_r.clicked())
        {
            c.s = ((pos.x - area.left()) / area.width()).clamp(0.0, 1.0);
            c.v = 1.0 - ((pos.y - area.top()) / area.height()).clamp(0.0, 1.0);
        }
        if let Some(pos) = hue_r.interact_pointer_pos()
            && (hue_r.dragged() || hue_r.clicked())
        {
            c.h = ((pos.y - hue.top()) / hue.height()).clamp(0.0, 1.0);
        }
        if let Some(pos) = alpha_r.interact_pointer_pos()
            && (alpha_r.dragged() || alpha_r.clicked())
        {
            c.a = 1.0 - ((pos.y - alpha.top()) / alpha.height()).clamp(0.0, 1.0);
        }
        if *c != before {
            response.mark_changed();
        }
        paint(ui, rect, *c, [&area_r, &hue_r, &alpha_r]);
        a11y(
            &response,
            egui::WidgetType::ColorButton,
            "Colour mixer",
            None,
        );
        response
    }
}

/// Paint the picker for `color` (used by the widget and by read-only previews).
fn paint(ui: &Ui, rect: Rect, color: Hsb, parts: [&Response; 3]) {
    let p = palette(ui.ctx());
    let painter = ui.painter();
    let (area, hue, alpha) = ColorPicker::layout(rect);

    // Saturation (x) / brightness (y) area: a vertex-coloured grid, corners chamfered to the radius.
    let (cols, rows) = (24usize, 12usize);
    let r = radius::MD;
    let mut mesh = egui::Mesh::default();
    for j in 0..=rows {
        for i in 0..=cols {
            let (s, v) = (i as f32 / cols as f32, 1.0 - j as f32 / rows as f32);
            let mut pt = egui::pos2(
                area.left() + s * area.width(),
                area.top() + (1.0 - v) * area.height(),
            );
            pt = round_corner(pt, area, r);
            mesh.colored_vertex(pt, hsv(color.h, s, v, 1.0).into());
        }
    }
    let w = cols as u32 + 1;
    for j in 0..rows as u32 {
        for i in 0..cols as u32 {
            let a = j * w + i;
            mesh.add_triangle(a, a + 1, a + w + 1);
            mesh.add_triangle(a, a + w + 1, a + w);
        }
    }
    painter.add(mesh);
    painter.rect_stroke(
        area,
        cr(r),
        Stroke::new(1.0_f32, p.border_subtle),
        StrokeKind::Inside,
    );

    // Hue strip (red at both ends) and alpha strip (opaque → clear over the field colour).
    let hues: Vec<(f32, Color32)> = (0..=12)
        .map(|i| {
            let t = i as f32 / 12.0;
            (t, hsv(t, 1.0, 1.0, 1.0).into())
        })
        .collect();
    capsule(painter, hue, &hues);
    painter.rect_filled(alpha, cr(radius::FULL), p.bg_field);
    let opaque: Color32 = hsv(color.h, color.s, color.v, 1.0).into();
    capsule(
        painter,
        alpha,
        &[(0.0, opaque), (1.0, Color32::TRANSPARENT)],
    );
    for strip in [hue, alpha] {
        painter.rect_stroke(
            strip,
            cr(radius::FULL),
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
    }

    // Thumbs: ring on the area, pills on the strips; `text/on-accent` reads on any colour.
    let ring = egui::pos2(
        area.left() + color.s * area.width(),
        area.top() + (1.0 - color.v) * area.height(),
    );
    painter.circle_stroke(ring, 6.0, Stroke::new(2.0_f32, p.text_on_accent));
    painter.circle_stroke(ring, 7.5, Stroke::new(1.0_f32, p.border_strong));
    for (strip, t, part) in [(hue, color.h, parts[1]), (alpha, 1.0 - color.a, parts[2])] {
        let y = strip.top() + 3.0 + t * (strip.height() - 6.0);
        let pill = Rect::from_center_size(egui::pos2(strip.center().x, y), egui::vec2(18.0, 6.0));
        painter.rect_filled(pill, cr(radius::FULL), p.text_on_accent);
        let edge = if part.hovered() || part.dragged() {
            p.border_focus
        } else {
            p.border_strong
        };
        painter.rect_stroke(
            pill,
            cr(radius::FULL),
            Stroke::new(1.0_f32, edge),
            StrokeKind::Outside,
        );
    }
    let s = State::of(ui, parts[0]);
    if s.focused {
        crate::theme::focus_ring(ui, area, r);
    }
}

/// Move a point that lies in a corner square of `rect` onto the corner arc (radius `r`).
fn round_corner(pt: egui::Pos2, rect: Rect, r: f32) -> egui::Pos2 {
    let cx = pt.x.clamp(rect.left() + r, rect.right() - r);
    let cy = pt.y.clamp(rect.top() + r, rect.bottom() - r);
    let d = pt - egui::pos2(cx, cy);
    if d.x != 0.0 && d.y != 0.0 && d.length() > r {
        egui::pos2(cx, cy) + d.normalized() * r
    } else {
        pt
    }
}

/// Vertical capsule filled with a top → bottom gradient (`stops` at 0…1). Caps take the end colours.
fn capsule(painter: &egui::Painter, rect: Rect, stops: &[(f32, Color32)]) {
    let half = rect.width() / 2.0;
    let (first, last) = (stops[0].1, stops[stops.len() - 1].1);
    let body = Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + half),
        egui::pos2(rect.right(), rect.bottom() - half),
    );
    painter.add(egui::Shape::convex_polygon(
        half_disc(egui::pos2(rect.center().x, body.top()), half, true),
        first,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        half_disc(egui::pos2(rect.center().x, body.bottom()), half, false),
        last,
        Stroke::NONE,
    ));
    let mut mesh = egui::Mesh::default();
    for (t, c) in stops {
        let y = body.top() + t * body.height();
        mesh.colored_vertex(egui::pos2(body.left(), y), *c);
        mesh.colored_vertex(egui::pos2(body.right(), y), *c);
    }
    for k in 0..stops.len() as u32 - 1 {
        let a = 2 * k;
        mesh.add_triangle(a, a + 1, a + 3);
        mesh.add_triangle(a, a + 3, a + 2);
    }
    painter.add(mesh);
}

fn half_disc(center: egui::Pos2, r: f32, top: bool) -> Vec<egui::Pos2> {
    let n = 12;
    (0..=n)
        .map(|i| {
            let a = std::f32::consts::PI * i as f32 / n as f32;
            let y = if top { -a.sin() } else { a.sin() };
            center + egui::vec2(a.cos() * r, y * r)
        })
        .collect()
}
