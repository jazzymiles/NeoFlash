//! Original, font-independent editor icons in logical point coordinates.
use eframe::egui::{self, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2, Widget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Select,
    Subselect,
    Transform,
    Pen,
    Pencil,
    Line,
    Brush,
    Bucket,
    Ink,
    Eyedropper,
    Eraser,
    Lasso,
    Hand,
    Gradient,
    Rectangle,
    Ellipse,
    Text,
    Plus,
    Trash,
    ZoomIn,
    ZoomOut,
    Fit,
    Symbol,
    ChevronRight,
    External,
    Rewind,
    Play,
    Pause,
    Lock,
    Unlock,
    Visible,
    Hidden,
}

pub(crate) fn paint_icon(p: &Painter, rect: Rect, icon: Icon, stroke: Stroke) {
    let size = rect.width().min(rect.height());
    if !size.is_finite() || size <= 0. {
        return;
    }
    let origin = rect.center() - Vec2::splat(size / 2.);
    let point = |x: f32, y: f32| origin + Vec2::new(x, y) * (size / 24.);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    let path = |points: &[(f32, f32)], closed: bool, filled: bool| {
        let points = points.iter().map(|&(x, y)| point(x, y)).collect();
        p.add(if filled {
            Shape::convex_polygon(points, stroke.color, Stroke::NONE)
        } else if closed {
            Shape::closed_line(points, stroke)
        } else {
            Shape::line(points, stroke)
        });
    };
    let box_at = |x: f32, y: f32, w: f32, h: f32| {
        p.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            0,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Select => {
            path(&[(4., 3.), (5., 19.), (19., 12.)], true, true);
            path(&[(8., 12.), (13., 21.), (16., 19.), (12., 12.)], true, true);
        }
        Icon::Subselect => path(
            &[
                (4., 3.),
                (5., 19.),
                (9., 15.),
                (13., 21.),
                (16., 19.),
                (12., 13.),
                (19., 12.),
            ],
            true,
            false,
        ),
        Icon::Transform => {
            box_at(6., 6., 12., 12.);
            for (x, y) in [(3., 3.), (17., 3.), (3., 17.), (17., 17.)] {
                box_at(x, y, 4., 4.);
            }
        }
        Icon::Pen => {
            path(
                &[(12., 3.), (20., 14.), (15., 20.), (9., 20.), (4., 14.)],
                true,
                false,
            );
            line((12., 3.), (12., 12.));
            p.circle_stroke(point(12., 14.), 2. * size / 24., stroke);
            line((8., 22.), (16., 22.));
        }
        Icon::Pencil => {
            path(
                &[
                    (4., 20.),
                    (6., 13.),
                    (16., 3.),
                    (21., 8.),
                    (11., 18.),
                    (4., 20.),
                ],
                false,
                false,
            );
            line((6., 13.), (11., 18.));
            line((14., 5.), (19., 10.));
            line((8., 15.), (17., 6.));
        }
        Icon::Line => line((4., 20.), (20., 4.)),
        Icon::Brush => {
            path(&[(11., 14.), (17., 3.), (21., 5.), (15., 16.)], true, false);
            path(
                &[
                    (11., 14.),
                    (7., 14.),
                    (6., 18.),
                    (3., 21.),
                    (10., 21.),
                    (14., 17.),
                ],
                true,
                true,
            );
        }
        Icon::Bucket => {
            path(&[(4., 9.), (11., 3.), (18., 11.), (11., 19.)], true, false);
            line((3., 9.), (17., 9.));
            path(
                &[(19., 13.), (16., 18.), (18., 21.), (21., 18.)],
                true,
                true,
            );
        }
        Icon::Ink => {
            box_at(8., 3., 8., 4.);
            path(
                &[
                    (8., 8.),
                    (5., 11.),
                    (5., 20.),
                    (19., 20.),
                    (19., 11.),
                    (16., 8.),
                ],
                true,
                false,
            );
            line((7., 15.), (17., 15.));
        }
        Icon::Eyedropper => {
            path(
                &[(6., 15.), (15., 6.), (18., 9.), (9., 18.), (4., 20.)],
                true,
                false,
            );
            path(&[(12., 5.), (16., 2.), (22., 8.), (19., 12.)], true, true);
        }
        Icon::Eraser => {
            path(
                &[(3., 14.), (13., 4.), (21., 12.), (13., 20.), (9., 20.)],
                true,
                false,
            );
            line((8., 9.), (17., 17.));
            line((9., 21.), (22., 21.));
        }
        Icon::Lasso => {
            p.add(Shape::ellipse_stroke(
                point(12., 10.),
                Vec2::new(9., 6.) * size / 24.,
                stroke,
            ));
            path(
                &[(7., 15.), (5., 20.), (9., 22.), (12., 19.), (8., 15.)],
                false,
                false,
            );
        }
        Icon::Hand => path(
            &[
                (5., 13.),
                (3., 11.),
                (2., 13.),
                (7., 21.),
                (17., 21.),
                (21., 13.),
                (21., 8.),
                (18., 8.),
                (17., 13.),
                (17., 5.),
                (14., 4.),
                (13., 12.),
                (13., 3.),
                (10., 3.),
                (9., 12.),
                (9., 5.),
                (6., 5.),
                (6., 15.),
            ],
            false,
            false,
        ),
        Icon::Gradient => {
            box_at(3., 4., 18., 16.);
            for x in [7., 10., 13., 16., 18.] {
                line((x, 6.), (x, 18.));
            }
            p.circle(point(6., 12.), 2. * size / 24., stroke.color, Stroke::NONE);
        }
        Icon::Rectangle => box_at(3., 5., 18., 14.),
        Icon::Ellipse => {
            p.add(Shape::ellipse_stroke(
                point(12., 12.),
                Vec2::new(9., 7.) * size / 24.,
                stroke,
            ));
        }
        Icon::Text => {
            line((4., 5.), (20., 5.));
            line((12., 5.), (12., 21.));
            line((8., 21.), (16., 21.));
            line((4., 5.), (4., 8.));
            line((20., 5.), (20., 8.));
        }
        Icon::Plus => {
            line((4., 12.), (20., 12.));
            line((12., 4.), (12., 20.));
        }
        Icon::Trash => {
            line((4., 6.), (20., 6.));
            box_at(9., 3., 6., 3.);
            path(&[(6., 6.), (7., 21.), (17., 21.), (18., 6.)], false, false);
            line((10., 10.), (10., 17.));
            line((14., 10.), (14., 17.));
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            p.circle_stroke(point(10., 10.), 7. * size / 24., stroke);
            line((15., 15.), (21., 21.));
            line((6., 10.), (14., 10.));
            if icon == Icon::ZoomIn {
                line((10., 6.), (10., 14.));
            }
        }
        Icon::Fit => {
            for points in [
                &[(9., 4.), (4., 4.), (4., 9.)][..],
                &[(15., 4.), (20., 4.), (20., 9.)],
                &[(4., 15.), (4., 20.), (9., 20.)],
                &[(15., 20.), (20., 20.), (20., 15.)],
            ] {
                path(points, false, false);
            }
        }
        Icon::Symbol => {
            path(&[(12., 3.), (21., 12.), (12., 21.), (3., 12.)], true, false);
            box_at(9., 9., 6., 6.);
        }
        Icon::ChevronRight => path(&[(8., 4.), (16., 12.), (8., 20.)], false, false),
        Icon::External => {
            path(&[(13., 4.), (20., 4.), (20., 11.)], false, false);
            line((20., 4.), (10., 14.));
            path(
                &[(9., 5.), (4., 5.), (4., 20.), (19., 20.), (19., 15.)],
                false,
                false,
            );
        }
        Icon::Rewind => {
            line((5., 4.), (5., 20.));
            path(&[(19., 4.), (8., 12.), (19., 20.)], true, true);
        }
        Icon::Play => path(&[(6., 3.), (21., 12.), (6., 21.)], true, true),
        Icon::Pause => {
            for x in [6., 15.] {
                p.rect_filled(
                    Rect::from_min_max(point(x, 4.), point(x + 3., 20.)),
                    0,
                    stroke.color,
                );
            }
        }
        Icon::Lock | Icon::Unlock => {
            box_at(5., 11., 14., 10.);
            if icon == Icon::Lock {
                path(
                    &[
                        (8., 11.),
                        (8., 6.),
                        (10., 3.),
                        (14., 3.),
                        (16., 6.),
                        (16., 11.),
                    ],
                    false,
                    false,
                );
            } else {
                path(
                    &[(8., 11.), (8., 6.), (10., 3.), (14., 3.), (17., 6.)],
                    false,
                    false,
                );
            }
            line((12., 15.), (12., 18.));
        }
        Icon::Visible | Icon::Hidden => {
            path(
                &[
                    (2., 12.),
                    (6., 7.),
                    (12., 5.),
                    (18., 7.),
                    (22., 12.),
                    (18., 17.),
                    (12., 19.),
                    (6., 17.),
                ],
                true,
                false,
            );
            p.circle_stroke(point(12., 12.), 3. * size / 24., stroke);
            if icon == Icon::Hidden {
                line((3., 3.), (21., 21.));
            }
        }
    }
}

pub(crate) struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    selected: Option<bool>,
    size: Vec2,
    text: bool,
    flat: bool,
    fixed_width: bool,
}
impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            selected: None,
            size: Vec2::new(30., 26.),
            text: false,
            flat: false,
            fixed_width: false,
        }
    }
    pub fn selected(mut self, value: bool) -> Self {
        self.selected = Some(value);
        self
    }
    pub fn size(mut self, value: Vec2) -> Self {
        self.size = value;
        self.fixed_width = true;
        self
    }
    pub fn text(mut self) -> Self {
        self.text = true;
        self
    }
    pub fn flat(mut self) -> Self {
        self.flat = true;
        self
    }
}
impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let galley = self.text.then(|| {
            let width = if self.fixed_width {
                (self.size.x - 34.).max(1.)
            } else {
                f32::INFINITY
            };
            let mut job = egui::text::LayoutJob::simple(
                self.label.into(),
                egui::TextStyle::Button.resolve(ui.style()),
                ui.visuals().text_color(),
                width,
            );
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            ui.painter().layout_job(job)
        });
        let size = Vec2::new(
            self.size
                .x
                .max(galley.as_ref().map_or(0., |g| g.size().x + 34.)),
            self.size
                .y
                .max(galley.as_ref().map_or(0., |g| g.size().y + 8.)),
        );
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            self.selected.map_or_else(
                || egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label),
                |selected| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        selected,
                        self.label,
                    )
                },
            )
        });
        if ui.is_rect_visible(rect) {
            let visuals = ui
                .style()
                .interact_selectable(&response, self.selected.unwrap_or(false));
            if !self.flat
                || self.selected == Some(true)
                || response.hovered()
                || response.has_focus()
            {
                ui.painter().rect(
                    rect,
                    visuals.corner_radius,
                    visuals.weak_bg_fill,
                    visuals.bg_stroke,
                    egui::StrokeKind::Inside,
                );
            }
            let icon_rect = if self.text {
                Rect::from_center_size(
                    Pos2::new(rect.left() + 15., rect.center().y),
                    Vec2::splat(18.),
                )
            } else {
                Rect::from_center_size(
                    rect.center(),
                    Vec2::splat((rect.height() - 10.).clamp(12., 22.)),
                )
            };
            paint_icon(
                ui.painter(),
                icon_rect,
                self.icon,
                Stroke::new(1.5_f32, visuals.fg_stroke.color),
            );
            if let Some(galley) = galley {
                ui.painter().galley_with_override_text_color(
                    Pos2::new(rect.left() + 29., rect.center().y - galley.size().y / 2.),
                    galley,
                    visuals.fg_stroke.color,
                );
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect.shrink(1.),
                    2.,
                    ui.visuals().selection.stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
        response.on_hover_text(self.label)
    }
}

pub(crate) fn inline_icon(ui: &mut egui::Ui, icon: Icon, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_icon(
        ui.painter(),
        rect,
        icon,
        Stroke::new(1.5_f32, ui.visuals().weak_text_color()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;
    #[test]
    fn icons_are_finite_distinct_vector_geometry_without_font_glyphs() {
        let icons = [
            Icon::Select,
            Icon::Subselect,
            Icon::Transform,
            Icon::Pen,
            Icon::Pencil,
            Icon::Rectangle,
            Icon::Ellipse,
            Icon::Text,
            Icon::Plus,
            Icon::Trash,
            Icon::ZoomIn,
            Icon::ZoomOut,
            Icon::Fit,
            Icon::Symbol,
            Icon::ChevronRight,
            Icon::External,
            Icon::Rewind,
            Icon::Play,
            Icon::Pause,
            Icon::Lock,
            Icon::Unlock,
            Icon::Visible,
            Icon::Hidden,
        ];
        let ctx = egui::Context::default();
        let mut signatures = std::collections::HashSet::new();
        for icon in icons {
            let output = ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::background());
                paint_icon(
                    &p,
                    Rect::from_min_size(Pos2::ZERO, Vec2::splat(24.)),
                    icon,
                    Stroke::new(1.5_f32, Color32::BLACK),
                );
            });
            assert!(!output.shapes.is_empty());
            for shape in &output.shapes {
                assert!(!matches!(shape.shape, Shape::Text(_)));
                let bounds = shape.shape.visual_bounding_rect();
                assert!(bounds.is_finite());
                assert!(
                    Rect::from_min_max(Pos2::new(-1., -1.), Pos2::new(25., 25.))
                        .contains_rect(bounds)
                );
            }
            assert!(signatures.insert(format!(
                "{:?}",
                output.shapes.iter().map(|s| &s.shape).collect::<Vec<_>>()
            )));
        }
    }
}
