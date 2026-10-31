//! Colour and grouped controls: `NF2/Color Well`, `NF2/Fill Stroke Chip`, `NF2/Swatch Cell`,
//! `NF2/Registration Picker`, `NF2/Icon Group`.
use super::{State, a11y, display::paint_swatch};
use crate::icons::{self, Icon, i16};
use crate::metrics::CONTROL;
use crate::theme::{cr, palette};
use crate::tokens::{radius, space};
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget};

/// `Target` of `NF2/Color Well`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorWellTarget {
    Stroke,
    Fill,
}

/// `NF2/Color Well` — target icon (pencil / bucket), swatch, ▾. `Active` = the target the mixer edits.
/// `color = None` shows the "no colour" slash.
pub struct ColorWell {
    pub target: ColorWellTarget,
    pub color: Option<Color32>,
    pub active: bool,
}

impl Widget for ColorWell {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(74.0, CONTROL), Sense::click());
        let s = State::of(ui, &response);
        let (bg, border) = if self.active {
            (p.accent_subtle, p.accent_default)
        } else if s.hovered {
            (p.bg_hover, p.border_strong)
        } else {
            (p.bg_field, p.border_default)
        };
        ui.painter().rect_filled(rect, cr(radius::MD), bg);
        ui.painter().rect_stroke(
            rect,
            cr(radius::MD),
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
        let icon = match self.target {
            ColorWellTarget::Stroke => i16::PENCIL,
            ColorWellTarget::Fill => i16::BUCKET,
        };
        let ic = if self.active {
            p.accent_default
        } else {
            p.text_secondary
        };
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(
                egui::pos2(rect.left() + 16.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            icon,
            ic,
        );
        let sw = Rect::from_center_size(
            egui::pos2(rect.left() + 38.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        match self.color {
            Some(c) => paint_swatch(ui, sw, c, radius::SM),
            None => paint_none(ui, sw),
        }
        icons::paint(
            ui.painter(),
            ui.ctx(),
            Rect::from_center_size(
                egui::pos2(rect.right() - 14.0, rect.center().y),
                egui::vec2(16.0, 16.0),
            ),
            i16::CHEVDOWN,
            p.text_secondary,
        );
        super::focus(ui, s, rect, radius::MD);
        let name = match self.target {
            ColorWellTarget::Stroke => "Stroke colour",
            ColorWellTarget::Fill => "Fill colour",
        };
        a11y(
            &response,
            egui::WidgetType::ColorButton,
            name,
            Some(self.active),
        );
        super::display::tooltip(response, name, None)
    }
}

/// White chip with a red diagonal = no colour.
pub fn paint_none(ui: &Ui, rect: Rect) {
    let p = palette(ui.ctx());
    ui.painter()
        .rect_filled(rect, cr(radius::SM), Color32::WHITE);
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.5_f32, p.danger_default),
    );
    ui.painter().rect_stroke(
        rect,
        cr(radius::SM),
        Stroke::new(1.0_f32, p.border_default),
        StrokeKind::Inside,
    );
}

/// `NF2/Fill Stroke Chip` — 36 × 36: fill square in front (top-left), stroke square behind.
pub struct FillStrokeChip {
    pub fill: Option<Color32>,
    pub stroke: Option<Color32>,
}

/// Which square was clicked.
pub struct FillStrokeResponse {
    pub response: Response,
    pub fill_clicked: bool,
    pub stroke_clicked: bool,
}

impl FillStrokeChip {
    pub fn show(self, ui: &mut Ui) -> FillStrokeResponse {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::hover());
        let back = Rect::from_min_size(rect.min + egui::vec2(14.0, 14.0), egui::vec2(20.0, 20.0));
        let front = Rect::from_min_size(rect.min + egui::vec2(2.0, 2.0), egui::vec2(20.0, 20.0));
        // Stroke: a hollow square drawn with the stroke colour.
        ui.painter().rect_filled(back, cr(radius::SM), p.bg_field);
        match self.stroke {
            Some(c) => {
                ui.painter().rect_stroke(
                    back,
                    cr(radius::SM),
                    Stroke::new(4.0_f32, c),
                    StrokeKind::Inside,
                );
                ui.painter().rect_stroke(
                    back,
                    cr(radius::SM),
                    Stroke::new(1.0_f32, p.border_default),
                    StrokeKind::Inside,
                );
            }
            None => paint_none(ui, back),
        }
        match self.fill {
            Some(c) => paint_swatch(ui, front, c, radius::SM),
            None => paint_none(ui, front),
        }
        ui.painter().rect_stroke(
            front,
            cr(radius::SM),
            Stroke::new(1.5_f32, p.bg_panel),
            StrokeKind::Outside,
        );
        let stroke_resp = super::display::tooltip(
            ui.interact(back, response.id.with("stroke"), Sense::click()),
            "Stroke colour",
            None,
        );
        let fill_resp = super::display::tooltip(
            ui.interact(front, response.id.with("fill"), Sense::click()),
            "Fill colour",
            None,
        );
        a11y(
            &fill_resp,
            egui::WidgetType::ColorButton,
            "Fill colour",
            None,
        );
        a11y(
            &stroke_resp,
            egui::WidgetType::ColorButton,
            "Stroke colour",
            None,
        );
        FillStrokeResponse {
            response,
            fill_clicked: fill_resp.clicked(),
            stroke_clicked: stroke_resp.clicked(),
        }
    }
}

/// `NF2/Swatch Cell` — 18 px palette cell; selected cell gets a 2 px accent ring.
pub struct SwatchCell {
    pub color: Color32,
    pub selected: bool,
}

impl Widget for SwatchCell {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
        paint_swatch(ui, rect, self.color, 3.0);
        if self.selected {
            ui.painter().rect_stroke(
                rect.expand(1.0),
                cr(4.0),
                Stroke::new(2.0_f32, p.accent_default),
                StrokeKind::Outside,
            );
        } else if response.hovered() {
            ui.painter().rect_stroke(
                rect,
                cr(3.0),
                Stroke::new(1.0_f32, p.text_primary),
                StrokeKind::Outside,
            );
        }
        let [r, g, b, _] = self.color.to_srgba_unmultiplied();
        let name = format!("#{r:02X}{g:02X}{b:02X}");
        a11y(
            &response,
            egui::WidgetType::ColorButton,
            &name,
            Some(self.selected),
        );
        response
    }
}

/// `Point` of `NF2/Registration Picker`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RegistrationPoint {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl RegistrationPoint {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(4)
    }
    pub fn label(self) -> &'static str {
        [
            "Top left",
            "Top",
            "Top right",
            "Left",
            "Center",
            "Right",
            "Bottom left",
            "Bottom",
            "Bottom right",
        ][self.index()]
    }
}

/// `NF2/Registration Picker` — 28 px 3 × 3 dot grid. Click a dot; arrow keys move it.
pub struct RegistrationPicker<'a> {
    pub point: &'a mut RegistrationPoint,
}

impl Widget for RegistrationPicker<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, mut response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
        let s = State::of(ui, &response);
        ui.painter().rect_filled(rect, cr(radius::MD), p.bg_field);
        ui.painter().rect_stroke(
            rect,
            cr(radius::MD),
            Stroke::new(
                1.0_f32,
                if s.hovered {
                    p.border_strong
                } else {
                    p.border_default
                },
            ),
            StrokeKind::Inside,
        );
        let cell = (rect.width() - 8.0) / 3.0;
        let center = |i: usize| {
            rect.min
                + egui::vec2(
                    4.0 + cell * ((i % 3) as f32 + 0.5),
                    4.0 + cell * ((i / 3) as f32 + 0.5),
                )
        };
        if let Some(pos) = response
            .interact_pointer_pos()
            .filter(|_| response.clicked())
        {
            let i = (0..9)
                .min_by(|a, b| {
                    center(*a)
                        .distance(pos)
                        .total_cmp(&center(*b).distance(pos))
                })
                .unwrap_or(4);
            *self.point = RegistrationPoint::ALL[i];
            response.mark_changed();
        }
        if response.has_focus() {
            let i = self.point.index() as i32;
            let (dx, dy) = ui.input(|inp| {
                (
                    inp.key_pressed(egui::Key::ArrowRight) as i32
                        - inp.key_pressed(egui::Key::ArrowLeft) as i32,
                    inp.key_pressed(egui::Key::ArrowDown) as i32
                        - inp.key_pressed(egui::Key::ArrowUp) as i32,
                )
            });
            if dx != 0 || dy != 0 {
                let (x, y) = ((i % 3 + dx).clamp(0, 2), (i / 3 + dy).clamp(0, 2));
                *self.point = RegistrationPoint::ALL[(y * 3 + x) as usize];
                response.mark_changed();
            }
        }
        let sel = self.point.index();
        for i in 0..9 {
            let c = center(i);
            if i == sel {
                ui.painter().rect_filled(
                    Rect::from_center_size(c, egui::vec2(5.0, 5.0)),
                    cr(1.0),
                    p.accent_default,
                );
            } else {
                ui.painter().rect_filled(
                    Rect::from_center_size(c, egui::vec2(3.0, 3.0)),
                    cr(0.5),
                    p.text_tertiary,
                );
            }
        }
        super::focus(ui, s, rect, radius::MD);
        a11y(
            &response,
            egui::WidgetType::RadioGroup,
            &format!("Registration point: {}", self.point.label()),
            None,
        );
        response
    }
}

/// `NF2/Icon Group` — 2–4 icon buttons in one bordered group (align, distribute, caps, joins, text align).
/// Returns the index of the clicked item. `selected` marks toggles (`Type=Selected`).
pub struct IconGroup<'a> {
    pub items: &'a [(Icon, &'a str)],
    pub selected: Option<usize>,
}

impl IconGroup<'_> {
    pub fn show(self, ui: &mut Ui) -> Option<usize> {
        let p = palette(ui.ctx());
        let w = self.items.len() as f32 * 28.0 + 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 34.0), Sense::hover());
        ui.painter().rect_filled(rect, cr(radius::LG), p.bg_field);
        ui.painter().rect_stroke(
            rect,
            cr(radius::LG),
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
        let mut clicked = None;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect.shrink(3.0))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, (icon, name)) in self.items.iter().enumerate() {
                    if ui
                        .add(
                            super::IconButton::new(*icon, name)
                                .side(28.0)
                                .selected(self.selected == Some(i)),
                        )
                        .clicked()
                    {
                        clicked = Some(i);
                    }
                }
            },
        );
        let _ = space::S1;
        clicked
    }
}
