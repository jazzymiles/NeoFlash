//! `NF3/Swatches Popover` (quick colours) and its `NF2/Gradient Swatch` chips.
//!
//! Opens from the fill/stroke chip in Tools, from any colour well and colour field. Header: current colour,
//! `NF2/Color Field` (hex + alpha), no colour, eyedropper, open full mixer. Sections: Recent, Document (+ add),
//! Palette (collapsible), Gradients. Swatch colours are document data, not UI tokens; the default palette is
//! generated ([`default_palette`]) instead of being copied from the reference as hex literals.
use super::{IconButton, SwatchCell};
use crate::icons::{self, Icon, i16};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, space};
use eframe::egui::{
    self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget, ecolor::Hsva,
};

/// Columns of the palette grid.
pub const PALETTE_COLUMNS: usize = 12;
/// Side of a `NF2/Swatch Cell` at the current density.
fn cell() -> f32 {
    crate::metrics::swatch()
}

/// Leave the NF2 offset clear around the colour sample, even when a swatch matches the ring
/// colour or already has the separate selected border. Cell size and hit area stay fixed.
pub(super) fn focused_swatch_sample(ui: &Ui, response: &Response) -> Option<Rect> {
    if !response.has_focus() || !crate::theme::keyboard_focus(ui, response) {
        return None;
    }
    crate::theme::reveal_focus_vertically(ui, response);
    Some(
        response
            .rect
            .shrink(crate::metrics::FOCUS_RING + crate::metrics::FOCUS_OFFSET),
    )
}

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
        let (rect, response) = ui.allocate_exact_size(egui::vec2(cell(), cell()), Sense::click());
        let focus = focused_swatch_sample(ui, &response);
        let sample = focus.unwrap_or(rect);
        paint_gradient(ui.painter(), sample, self);
        let border = if response.hovered() {
            p.text_primary
        } else {
            p.border_default
        };
        ui.painter().rect_stroke(
            sample,
            cr(3.0),
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
        if let Some(sample) = focus {
            crate::theme::focus_ring(ui, sample, crate::tokens::radius::sm());
        }
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

/// Smallest gap between two cells of a swatch grid.
const MIN_GAP: f32 = 2.0;
/// Narrowest hex part of the colour field in the header (six mono digits).
const MIN_HEX: f32 = 52.0;

/// Columns and column pitch of a swatch grid `width` wide: [`PALETTE_COLUMNS`] whenever they fit
/// with a [`MIN_GAP`] between cells (always at `metrics::inspector()`), fewer in a narrower column.
/// The first and last column touch the edges, so a row never ends past `width`.
pub fn grid_columns(width: f32) -> (usize, f32) {
    let side = cell();
    let columns =
        (((width + MIN_GAP) / (side + MIN_GAP)).floor() as usize).clamp(1, PALETTE_COLUMNS);
    let gap = if columns > 1 {
        ((width - columns as f32 * side) / (columns - 1) as f32).max(MIN_GAP)
    } else {
        MIN_GAP
    };
    (columns, side + gap)
}

/// How many of the three header icon buttons fit beside the colour field in `width`; the rest of
/// them (all three, in practice) move to a second row.
fn header_icons_inline(width: f32) -> bool {
    let field = crate::metrics::control() + MIN_HEX + crate::metrics::field_num();
    width >= field + 3.0 * (crate::metrics::control() + space::s2())
}

/// `NF3/Swatches Popover`. Lay it out inside a [`super::Surface::Popover`] frame or a panel body;
/// the header and the grids follow the available width.
pub struct SwatchesPopover<'a> {
    pub color: &'a mut Color32,
    pub recent: &'a [Color32],
    pub document: &'a [Color32],
    pub palette: &'a [Color32],
    pub gradients: &'a [GradientSwatch],
}

/// One cell of a swatch grid.
enum GridItem {
    Color(Color32),
    Gradient(GradientSwatch),
    Add,
}

impl SwatchesPopover<'_> {
    pub fn show(self, ui: &mut Ui) -> SwatchesResponse {
        ui.vertical(|ui| self.contents(ui)).inner
    }

    fn contents(self, ui: &mut Ui) -> SwatchesResponse {
        let mut out = SwatchesResponse::default();
        ui.spacing_mut().item_spacing.y = space::s2();
        let width = ui.available_width();
        let inline = header_icons_inline(width);
        let icons = |ui: &mut Ui, out: &mut SwatchesResponse| {
            out.no_colour = ui.add(IconButton::new(i16::NOCOLOR, "No colour")).clicked();
            out.eyedropper = ui
                .add(IconButton::new(i16::EYEDROPPER16, "Eyedropper").shortcut("I"))
                .clicked();
            out.open_mixer = ui
                .add(IconButton::new(i16::COLORWHEEL, "Open colour mixer"))
                .clicked();
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            let field = if inline {
                width - 3.0 * (crate::metrics::control() + space::s2())
            } else {
                width
            };
            super::ColorField::new(self.color, "Colour")
                .width(field)
                .show(ui);
            if inline {
                icons(ui, &mut out);
            }
        });
        if !inline {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::s2();
                icons(ui, &mut out);
            });
        }
        let current = *self.color;
        let mut picked = None;
        // Cells sit at fixed column positions (palette ramps line up) inside one allocated block.
        let mut grid = |ui: &mut Ui, salt: &str, items: &[GridItem]| {
            let width = ui.available_width();
            let (columns, pitch) = grid_columns(width);
            let side = cell();
            let rows = items.len().div_ceil(columns);
            if rows == 0 {
                return;
            }
            let height = rows as f32 * side + (rows - 1) as f32 * (pitch - side);
            let (area, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
            for (i, item) in items.iter().enumerate() {
                let (column, row) = (i % columns, i / columns);
                let x = if columns > 1 && column + 1 == columns {
                    area.right() - side
                } else {
                    area.left() + column as f32 * pitch
                };
                let rect = Rect::from_min_size(
                    egui::pos2(x, area.top() + row as f32 * pitch),
                    egui::vec2(side, side),
                );
                let mut slot = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(("nf-swatch-grid", salt, i))
                        .max_rect(rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Min)),
                );
                match item {
                    GridItem::Color(c) => {
                        if slot
                            .add(SwatchCell {
                                color: *c,
                                selected: *c == current,
                            })
                            .clicked()
                        {
                            picked = Some(*c);
                        }
                    }
                    GridItem::Gradient(g) => {
                        if slot.add(*g).clicked() {
                            out.gradient = Some(*g);
                        }
                    }
                    GridItem::Add => {
                        out.add_swatch = slot
                            .add(IconButton::new(i16::PLUS, "Add swatch").side(side))
                            .clicked();
                    }
                }
            }
        };
        let colors = |list: &[Color32]| {
            list.iter()
                .copied()
                .map(GridItem::Color)
                .collect::<Vec<_>>()
        };
        caption(ui, "Recent", None);
        grid(ui, "recent", &colors(self.recent));
        caption(ui, "Document", None);
        let mut document = colors(self.document);
        document.push(GridItem::Add);
        grid(ui, "document", &document);
        let id = ui.id().with("nf2-swatches-palette");
        let mut open = ui.data_mut(|d| d.get_persisted::<bool>(id)).unwrap_or(true);
        if caption(ui, "Palette", Some(open)) {
            open = !open;
            ui.data_mut(|d| d.insert_persisted(id, open));
        }
        if open {
            grid(ui, "palette", &colors(self.palette));
        }
        caption(ui, "Gradients", None);
        let gradients: Vec<GridItem> = self
            .gradients
            .iter()
            .copied()
            .map(GridItem::Gradient)
            .collect();
        grid(ui, "gradients", &gradients);
        if let Some(c) = picked {
            *self.color = c;
            out.picked = true;
        }
        out
    }
}

/// Overline section caption; with `open = Some(..)` a ▾ / ▸ toggle on the right. Returns `true` when toggled.
fn caption(ui: &mut Ui, label: &str, open: Option<bool>) -> bool {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), crate::metrics::section_header()),
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
        crate::theme::focus_ring_inside(ui, &response, crate::tokens::radius::sm());
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
