//! `NF2/Swatches Popover` (quick colours, 300 × 430) and its `NF2/Gradient Swatch` chips.
//!
//! Opens from the fill/stroke chip in Tools, from any colour well and colour field. Header: current colour,
//! `NF2/Color Field` (hex + alpha), no colour, eyedropper, open full mixer. Sections: Recent, Document (+ add),
//! Palette (collapsible), Gradients. Swatch colours are document data, not UI tokens; the default palette is
//! generated ([`default_palette`]) instead of being copied from the reference as hex literals.
use super::{IconButton, SwatchCell};
use crate::icons::{self, Icon, i16};
use crate::metrics;
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, space};
use eframe::egui::{
    self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget, ecolor::Hsva,
};

/// Columns of the palette grid.
pub const PALETTE_COLUMNS: usize = 12;
/// Side of a `NF2/Swatch Cell`.
const CELL: f32 = 18.0;

/// The classic 12 × 9 palette: one grey ramp, then 8 lightness steps of 12 hues (light → dark).
pub fn default_palette() -> Vec<Color32> {
    let mut out = Vec::with_capacity(PALETTE_COLUMNS * 9);
    for i in 0..PALETTE_COLUMNS {
        let v = i as f32 / (PALETTE_COLUMNS - 1) as f32;
        out.push(Hsva::new(0.0, 0.0, v, 1.0).into());
    }
    let rows: [(f32, f32); 8] = [
        (0.22, 1.0),
        (0.42, 0.97),
        (0.62, 0.95),
        (0.82, 0.93),
        (0.95, 0.9),
        (0.95, 0.74),
        (0.95, 0.57),
        (0.95, 0.4),
    ];
    for (s, v) in rows {
        for h in 0..PALETTE_COLUMNS {
            out.push(Hsva::new(h as f32 / PALETTE_COLUMNS as f32, s, v, 1.0).into());
        }
    }
    out
}

/// Two-stop gradient preset (linear or radial).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientSwatch {
    pub from: Color32,
    pub to: Color32,
    pub radial: bool,
}

impl Widget for GradientSwatch {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
        paint_gradient(ui.painter(), rect, self);
        let border = if response.hovered() {
            p.text_primary
        } else {
            p.border_default
        };
        ui.painter().rect_stroke(
            rect,
            cr(3.0),
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
        super::a11y(
            &response,
            egui::WidgetType::ColorButton,
            if self.radial {
                "Radial gradient"
            } else {
                "Linear gradient"
            },
            None,
        );
        response
    }
}

/// Paint a gradient chip (mesh; radial = centre `from`, rim `to`).
pub fn paint_gradient(painter: &egui::Painter, rect: Rect, g: GradientSwatch) {
    let mut mesh = egui::Mesh::default();
    if g.radial {
        let c = rect.center();
        mesh.colored_vertex(c, g.from);
        let n = 24;
        for i in 0..=n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let r = rect.width() * 0.5;
            let pt = c + egui::vec2(a.cos(), a.sin()) * r;
            mesh.colored_vertex(pt.clamp(rect.min, rect.max), g.to);
        }
        for i in 1..=n as u32 {
            mesh.add_triangle(0, i, i + 1);
        }
        painter.rect_filled(rect, cr(3.0), g.to);
    } else {
        mesh.colored_vertex(rect.left_top(), g.from);
        mesh.colored_vertex(rect.right_top(), g.to);
        mesh.colored_vertex(rect.right_bottom(), g.to);
        mesh.colored_vertex(rect.left_bottom(), g.from);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
    }
    painter.add(mesh);
}

/// What the user did with the popover this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwatchesResponse {
    /// A swatch was clicked (the new colour is already written).
    pub picked: bool,
    pub gradient: Option<GradientSwatch>,
    pub no_colour: bool,
    pub eyedropper: bool,
    pub open_mixer: bool,
    pub add_swatch: bool,
}

/// `NF2/Swatches Popover`. Lay it out inside a [`super::Surface::Popover`] frame (300 wide) or a panel body.
pub struct SwatchesPopover<'a> {
    pub color: &'a mut Color32,
    pub recent: &'a [Color32],
    pub document: &'a [Color32],
    pub palette: &'a [Color32],
    pub gradients: &'a [GradientSwatch],
}

impl SwatchesPopover<'_> {
    pub fn show(self, ui: &mut Ui) -> SwatchesResponse {
        ui.vertical(|ui| self.contents(ui)).inner
    }

    fn contents(self, ui: &mut Ui) -> SwatchesResponse {
        let mut out = SwatchesResponse::default();
        ui.spacing_mut().item_spacing.y = space::S2;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let field = ui.available_width() - 3.0 * metrics::CONTROL - 3.0 * space::S2;
            super::ColorField::new(self.color, "Colour")
                .width(field)
                .show(ui);
            out.no_colour = ui.add(IconButton::new(i16::NOCOLOR, "No colour")).clicked();
            out.eyedropper = ui
                .add(IconButton::new(i16::EYEDROPPER16, "Eyedropper").shortcut("I"))
                .clicked();
            out.open_mixer = ui
                .add(IconButton::new(i16::COLORWHEEL, "Open colour mixer"))
                .clicked();
        });
        let current = *self.color;
        let mut grid = |ui: &mut Ui, colors: &[Color32], add: bool| {
            // Fixed 12-column grid: cells keep their column across rows (palette ramps line up).
            let gap = ((ui.available_width() - PALETTE_COLUMNS as f32 * CELL)
                / (PALETTE_COLUMNS - 1) as f32)
                .max(2.0);
            ui.spacing_mut().item_spacing.y = gap;
            let rows = colors.len().div_ceil(PALETTE_COLUMNS).max(usize::from(add));
            for r in 0..rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in colors
                        .iter()
                        .skip(r * PALETTE_COLUMNS)
                        .take(PALETTE_COLUMNS)
                    {
                        if ui
                            .add(SwatchCell {
                                color: *c,
                                selected: *c == current,
                            })
                            .clicked()
                        {
                            *self.color = *c;
                            out.picked = true;
                        }
                    }
                    if add && r + 1 == rows {
                        out.add_swatch = ui
                            .add(IconButton::new(i16::PLUS, "Add swatch").side(CELL))
                            .clicked();
                    }
                });
            }
        };
        caption(ui, "Recent", None);
        grid(ui, self.recent, false);
        caption(ui, "Document", None);
        grid(ui, self.document, true);
        let id = ui.id().with("nf2-swatches-palette");
        let mut open = ui.data_mut(|d| d.get_persisted::<bool>(id)).unwrap_or(true);
        if caption(ui, "Palette", Some(open)) {
            open = !open;
            ui.data_mut(|d| d.insert_persisted(id, open));
        }
        if open {
            grid(ui, self.palette, false);
        }
        caption(ui, "Gradients", None);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 26.0;
            for g in self.gradients {
                if ui.add(*g).clicked() {
                    out.gradient = Some(*g);
                }
            }
        });
        out
    }
}

/// Overline section caption; with `open = Some(..)` a ▾ / ▸ toggle on the right. Returns `true` when toggled.
fn caption(ui: &mut Ui, label: &str, open: Option<bool>) -> bool {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 24.0),
        if open.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let g = super::galley(ui, label, TextStyle::Overline, p.text_tertiary);
    super::text_at(ui, rect.left(), rect, g, p.text_tertiary);
    if let Some(open) = open {
        let icon: Icon = if open { i16::CHEVDOWN } else { i16::CHEVRIGHT };
        let r = Rect::from_center_size(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        icons::paint(
            ui.painter(),
            ui.ctx(),
            r,
            icon,
            if response.hovered() {
                p.text_primary
            } else {
                p.text_secondary
            },
        );
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(
                egui::WidgetType::CollapsingHeader,
                ui.is_enabled(),
                label,
            );
            info.selected = Some(open);
            info
        });
    }
    response.clicked()
}
