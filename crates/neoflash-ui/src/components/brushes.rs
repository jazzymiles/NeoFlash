//! v2.2 shelf “Brushes, pencil and tool flyouts” (Figma `96:793`, R-038): [`ToolFlyoutItem`],
//! [`BrushPreset`], [`BrushShape`], [`brush_cursor`] and the `NF2 Brush/*` sample strokes
//! ([`brush_stroke_sample`]).
//!
//! The sample strokes are the vector data of the `NF2 Brush/*` components (96 × 20 frames), rendered
//! with resvg in white and tinted at paint time like the icons. The brush cursor is theme-independent
//! (black + white double ring), because it sits on artwork.
use super::{State, a11y, focus, galley, text_at};
use crate::icons::{self, Icon};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{
    self, Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, Ui, Vec2, Widget,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------------------------
// Tool Flyout Item

/// Default width of `NF2/Tool Flyout Item` in a flyout menu.
pub const TOOL_FLYOUT_ITEM_WIDTH: f32 = 260.0;

/// `NF2/Tool Flyout Item` (`96:807`) — row of a tool group flyout: 6 pt ■ mark when `Active` (the tool
/// currently shown on the rail), 20 pt tool icon, `NF2/Body` name, `NF2/Mono Small` shortcut; 28 pt,
/// radius md, hover `bg/hover`. A click closes the surrounding popup; the caller selects the tool.
#[must_use = "add it with ui.add(…)"]
pub struct ToolFlyoutItem<'a> {
    icon: Icon,
    label: &'a str,
    shortcut: &'a str,
    active: bool,
    width: Option<f32>,
}

impl<'a> ToolFlyoutItem<'a> {
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            shortcut: "",
            active: false,
            width: None,
        }
    }
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = shortcut;
        self
    }
    /// `Active=True` — this tool is the one shown on the rail for its group.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// Row width (default: the available width, at least the content).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

impl Widget for ToolFlyoutItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let short = (!self.shortcut.is_empty())
            .then(|| galley(ui, self.shortcut, TextStyle::MonoSmall, p.text_tertiary));
        let short_w = short.as_ref().map_or(0.0, |g| space::S2 + g.size().x);
        let lead = space::S2 + 6.0 + space::S2 + 20.0 + space::S2;
        let label_probe = galley(ui, self.label, TextStyle::Body, p.text_primary);
        let min_w = lead + label_probe.size().x + short_w + 10.0;
        let width = self
            .width
            .unwrap_or_else(|| ui.available_width().max(min_w));
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, crate::metrics::row()), Sense::click());
        let s = State::of(ui, &response);
        if ui.is_rect_visible(rect) {
            if s.hovered {
                ui.painter().rect_filled(rect, cr(radius::MD), p.bg_hover);
            }
            let fg = if s.enabled {
                p.text_primary
            } else {
                p.text_disabled
            };
            let mark = Rect::from_center_size(
                egui::pos2(rect.left() + space::S2 + 3.0, rect.center().y),
                egui::vec2(6.0, 6.0),
            );
            if self.active {
                ui.painter().rect_filled(mark, cr(1.0), fg);
            }
            let icon = Rect::from_center_size(
                egui::pos2(mark.right() + space::S2 + 10.0, rect.center().y),
                egui::vec2(20.0, 20.0),
            );
            let icon_c = if !s.enabled {
                p.text_disabled
            } else if s.hovered {
                p.text_primary
            } else {
                p.text_secondary
            };
            icons::paint(ui.painter(), ui.ctx(), icon, self.icon, icon_c);
            let right = rect.right() - 10.0;
            let x = icon.right() + space::S2;
            let room = (right - short_w - x).max(0.0);
            let label = super::galley_truncated(ui, self.label, TextStyle::Body, fg, room);
            text_at(ui, x, rect, label, fg);
            if let Some(g) = short {
                let c = if s.enabled {
                    p.text_tertiary
                } else {
                    p.text_disabled
                };
                text_at(ui, right - g.size().x, rect, g, c);
            }
            focus(ui, s, rect, radius::MD);
        }
        a11y(
            &response,
            egui::WidgetType::Button,
            self.label,
            Some(self.active),
        );
        if response.clicked() {
            ui.close();
        }
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Sample strokes (`NF2 Brush/*`)

/// Size of the `NF2 Brush/*` sample frames.
pub const BRUSH_STROKE_SIZE: Vec2 = Vec2::new(96.0, 20.0);
/// Brush size (document px) at which [`brush_stroke_sample`] reproduces the Figma samples exactly.
pub const BRUSH_SAMPLE_REFERENCE: f32 = 12.0;

/// The eight `NF2 Brush/*` sample strokes (brush library presets).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BrushStroke {
    #[default]
    Round,
    Flat,
    Chisel,
    Charcoal,
    Ink,
    Watercolour,
    Dots,
    Vine,
}

impl BrushStroke {
    pub const ALL: [Self; 8] = [
        Self::Round,
        Self::Flat,
        Self::Chisel,
        Self::Charcoal,
        Self::Ink,
        Self::Watercolour,
        Self::Dots,
        Self::Vine,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Round => "Round",
            Self::Flat => "Flat",
            Self::Chisel => "Chisel",
            Self::Charcoal => "Charcoal",
            Self::Ink => "Ink",
            Self::Watercolour => "Watercolour",
            Self::Dots => "Dots",
            Self::Vine => "Vine",
        }
    }

    /// SVG of the sample in white at weight `k` (1 = the Figma component). Stroked samples scale
    /// their line width, filled samples their thickness around the centre line, dots their radii.
    pub fn svg(self, k: f32) -> String {
        const DOTS: [(f32, f32, f32); 8] = [
            (8.0, 11.0, 2.5),
            (20.0, 9.0, 2.0),
            (31.0, 12.0, 2.6),
            (43.0, 10.0, 1.8),
            (55.0, 12.0, 2.4),
            (67.0, 9.0, 2.0),
            (79.0, 11.0, 2.6),
            (89.0, 10.0, 1.8),
        ];
        let fill = |d: &str, opacity: f32, centre: f32| {
            format!(
                r##"<g transform="translate(0 {centre}) scale(1 {k}) translate(0 -{centre})"><path d="{d}" fill="#FFFFFF" fill-opacity="{opacity}"/></g>"##
            )
        };
        let stroke = |d: &str, width: f32, opacity: f32| {
            format!(
                r##"<path d="{d}" stroke="#FFFFFF" stroke-opacity="{opacity}" stroke-width="{}" stroke-linecap="round" fill="none"/>"##,
                width * k
            )
        };
        let body = match self {
            Self::Round => stroke("M4 12C18 3 32 17 48 11C64 5 78 5 92 12", 4.0, 1.0),
            Self::Flat => fill(
                "M4 13C18 5 32 5 48 9C64 13 78 16 92 6V10C80 19 62 16 48 13C34 10 18 9 4 16V13Z",
                1.0,
                11.0,
            ),
            Self::Chisel => fill("M4 15L14 7L84 8L92 14L82 18L14 17L4 15Z", 1.0, 12.5),
            Self::Charcoal => stroke(
                "M4 9C26 6 54 10 92 8M4 11C28 9 56 13 92 11M5 13C27 11 55 15 91 13M6 15C26 14 52 16 90 15M8 7C28 6 52 8 88 7",
                1.0,
                0.7,
            ),
            Self::Ink => fill(
                "M4 12C20 9 40 5 60 6C80 7 84 9 92 9C80 11 62 12 44 13C26 14 16 14 4 12Z",
                1.0,
                9.5,
            ),
            Self::Watercolour => fill(
                "M4 11C10 5 24 4 38 7C52 10 66 13 78 8C90 3 92 11 92 13C92 15 84 19 68 18C52 17 38 14 24 16C10 18 2 18 4 11Z",
                0.55,
                11.5,
            ),
            Self::Dots => DOTS
                .iter()
                .map(|(x, y, r)| {
                    format!(r##"<circle cx="{x}" cy="{y}" r="{}" fill="#FFFFFF"/>"##, r * k)
                })
                .collect(),
            Self::Vine => [
                stroke("M4 12C14 4 24 20 34 12C44 4 54 4 64 12C74 20 82 18 92 10", 1.5, 1.0),
                stroke(
                    "M19 8C21 4 25 4 26 6C24 8 21 9 19 8ZM49 15C51 19 55 19 56 17C54 15 51 14 49 15ZM77 7C79 3 83 3 84 5C82 7 79 8 77 7Z",
                    1.5,
                    1.0,
                ),
            ]
            .concat(),
        };
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="20" viewBox="0 0 96 20" fill="none">{body}</svg>"##
        )
    }
}

/// Weight of a sample for brush `size` (document px): 1 at [`BRUSH_SAMPLE_REFERENCE`], clamped to
/// 0.5…2 and quantised to eighths so the texture cache stays small.
pub fn brush_sample_weight(size: f32) -> f32 {
    let k = (size / BRUSH_SAMPLE_REFERENCE).clamp(0.5, 2.0);
    (k * 8.0).round() / 8.0
}

type StrokeKey = (BrushStroke, u32, u32, u32);

#[derive(Clone, Default)]
struct StrokeCache(Arc<Mutex<HashMap<StrokeKey, TextureHandle>>>);

fn rasterize(svg: &str, w: u32, h: u32) -> Option<egui::ColorImage> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let k = (w as f32 / tree.size().width()).min(h as f32 / tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [w as usize, h as usize],
        pixmap.data(),
    ))
}

/// `NF2 Brush/*` sample stroke in `rect` (fitted, aspect kept, centred) for a brush of `size`
/// document px. Painted in `text/primary` (the samples are drawn in the UI colour, not the fill).
pub fn brush_stroke_sample(painter: &Painter, rect: Rect, stroke: BrushStroke, size: f32) {
    let ctx = painter.ctx();
    let color = palette(ctx).text_primary;
    paint_stroke_sample(painter, rect, stroke, size, color);
}

/// [`brush_stroke_sample`] in an explicit colour (disabled rows, previews on artwork).
pub fn paint_stroke_sample(
    painter: &Painter,
    rect: Rect,
    stroke: BrushStroke,
    size: f32,
    color: Color32,
) {
    let scale = (rect.width() / BRUSH_STROKE_SIZE.x).min(rect.height() / BRUSH_STROKE_SIZE.y);
    if scale <= 0.0 {
        return;
    }
    let target = Rect::from_center_size(rect.center(), BRUSH_STROKE_SIZE * scale);
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let (w, h) = (
        (target.width() * ppp).ceil() as u32,
        (target.height() * ppp).ceil() as u32,
    );
    let k = brush_sample_weight(size);
    let key = (stroke, k.to_bits(), w, h);
    let cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<StrokeCache>(egui::Id::new("nf2-brush-strokes"))
            .clone()
    });
    let Ok(mut map) = cache.0.lock() else {
        return;
    };
    let tex = match map.get(&key) {
        Some(t) => t.clone(),
        None => {
            let Some(image) = rasterize(&stroke.svg(k), w, h) else {
                return;
            };
            let t = ctx.load_texture(
                format!("nf2/brush/{}@{k}/{w}x{h}", stroke.label()),
                image,
                TextureOptions::LINEAR,
            );
            map.insert(key, t.clone());
            t
        }
    };
    painter.image(
        tex.id(),
        target,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        color,
    );
}

// ---------------------------------------------------------------------------------------------
// Brush Preset

/// `NF2/Brush Preset` (`96:851`) — brush library row: 96 × 20 sample stroke, `NF2/Body` name and the
/// default size in `NF2/Mono Small` `text/tertiary`; 36 pt, radius md, 10 pt side padding, 12 pt gaps.
/// `Selected` = `bg/selected` + accent name. Click selects, double-click edits (caller).
#[must_use = "add it with ui.add(…)"]
pub struct BrushPreset<'a> {
    label: &'a str,
    size: &'a str,
    stroke: BrushStroke,
    selected: bool,
    width: Option<f32>,
}

/// Row height of `NF2/Brush Preset`.
pub const BRUSH_PRESET: f32 = 36.0;

impl<'a> BrushPreset<'a> {
    /// `size` is the caller-formatted default size ("12").
    pub fn new(label: &'a str, size: &'a str) -> Self {
        Self {
            label,
            size,
            stroke: BrushStroke::Round,
            selected: false,
            width: None,
        }
    }
    /// Row width (default: the available width).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// The `Stroke` swap (`NF2 Brush/*`).
    pub fn stroke(mut self, stroke: BrushStroke) -> Self {
        self.stroke = stroke;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for BrushPreset<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let width = self
            .width
            .unwrap_or_else(|| ui.available_width().max(200.0));
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, BRUSH_PRESET), Sense::click());
        let s = State::of(ui, &response);
        if ui.is_rect_visible(rect) {
            if self.selected {
                ui.painter()
                    .rect_filled(rect, cr(radius::MD), p.bg_selected);
            } else if s.hovered {
                ui.painter().rect_filled(rect, cr(radius::MD), p.bg_hover);
            }
            let sample = Rect::from_min_size(
                egui::pos2(
                    rect.left() + 10.0,
                    rect.center().y - BRUSH_STROKE_SIZE.y / 2.0,
                ),
                BRUSH_STROKE_SIZE,
            );
            let stroke_c = if s.enabled {
                p.text_primary
            } else {
                p.text_disabled
            };
            paint_stroke_sample(
                ui.painter(),
                sample,
                self.stroke,
                BRUSH_SAMPLE_REFERENCE,
                stroke_c,
            );
            let size_c = if s.enabled {
                p.text_tertiary
            } else {
                p.text_disabled
            };
            let size = galley(ui, self.size, TextStyle::MonoSmall, size_c);
            let size_x = rect.right() - 10.0 - size.size().x;
            let x = sample.right() + space::S3;
            let fg = if !s.enabled {
                p.text_disabled
            } else if self.selected {
                p.accent_default
            } else {
                p.text_primary
            };
            let room = (size_x - space::S3 - x).max(0.0);
            let label = super::galley_truncated(ui, self.label, TextStyle::Body, fg, room);
            text_at(ui, x, rect, label, fg);
            text_at(ui, size_x, rect, size, size_c);
            focus(ui, s, rect, radius::MD);
        }
        a11y(
            &response,
            egui::WidgetType::SelectableLabel,
            self.label,
            Some(self.selected),
        );
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Brush Shape

/// `Shape` of `NF2/Brush Shape` and of the brush footprint (Flash brush shapes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BrushShapeKind {
    #[default]
    Round,
    OvalH,
    OvalV,
    Square,
    RectH,
    RectV,
    Slash,
    Backslash,
}

impl BrushShapeKind {
    pub const ALL: [Self; 8] = [
        Self::Round,
        Self::OvalH,
        Self::OvalV,
        Self::Square,
        Self::RectH,
        Self::RectV,
        Self::Slash,
        Self::Backslash,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Round => "Round",
            Self::OvalH => "Oval horizontal",
            Self::OvalV => "Oval vertical",
            Self::Square => "Square",
            Self::RectH => "Rectangle horizontal",
            Self::RectV => "Rectangle vertical",
            Self::Slash => "Slash",
            Self::Backslash => "Backslash",
        }
    }
    /// Footprint of a brush of `diameter` (its longest side) centred at the origin. Proportions of the
    /// Figma glyphs: ovals 13 × 7, rectangles 13 × 5, slanted tips 1 : 11 at ±45°.
    pub fn footprint(self, diameter: f32) -> Footprint {
        let r = diameter / 2.0;
        let (ellipse, half, angle) = match self {
            Self::Round => (true, Vec2::splat(r), 0.0),
            Self::OvalH => (true, egui::vec2(r, r * 7.0 / 13.0), 0.0),
            Self::OvalV => (true, egui::vec2(r * 7.0 / 13.0, r), 0.0),
            Self::Square => (false, Vec2::splat(r), 0.0),
            Self::RectH => (false, egui::vec2(r, r * 5.0 / 13.0), 0.0),
            Self::RectV => (false, egui::vec2(r * 5.0 / 13.0, r), 0.0),
            Self::Slash => (
                false,
                egui::vec2(r, (r / 11.0).max(0.5)),
                -std::f32::consts::FRAC_PI_4,
            ),
            Self::Backslash => (
                false,
                egui::vec2(r, (r / 11.0).max(0.5)),
                std::f32::consts::FRAC_PI_4,
            ),
        };
        Footprint {
            ellipse,
            half,
            angle,
        }
    }
}

/// Brush footprint: an ellipse or a (rotated) rectangle with half extents `half`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub ellipse: bool,
    pub half: Vec2,
    /// Rotation in radians (clockwise on screen, y down).
    pub angle: f32,
}

impl Footprint {
    /// The same footprint grown by `d` points on every side (negative shrinks).
    pub fn grown(self, d: f32) -> Self {
        Self {
            half: (self.half + Vec2::splat(d)).max(Vec2::splat(0.25)),
            ..self
        }
    }
    /// Outline as a closed polyline around `centre`.
    pub fn outline(self, centre: Pos2) -> Vec<Pos2> {
        let rot = egui::emath::Rot2::from_angle(self.angle);
        if self.ellipse {
            let n = ((self.half.x.max(self.half.y) * 1.5) as usize).clamp(24, 160);
            (0..n)
                .map(|i| {
                    let t = i as f32 / n as f32 * std::f32::consts::TAU;
                    centre + rot * egui::vec2(self.half.x * t.cos(), self.half.y * t.sin())
                })
                .collect()
        } else {
            let h = self.half;
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .into_iter()
                .map(|(sx, sy)| centre + rot * egui::vec2(sx * h.x, sy * h.y))
                .collect()
        }
    }
    /// Screen bounds of the footprint around `centre`.
    pub fn bounds(self, centre: Pos2) -> Rect {
        Rect::from_points(&self.outline(centre))
    }
}

/// Side of `NF2/Brush Shape`.
pub const BRUSH_SHAPE: f32 = 24.0;

/// `NF2/Brush Shape` (`96:900`) — 24 pt tip chooser: `bg/field`, 1 pt `border/subtle`, radius sm,
/// glyph in `text/primary`; `Selected` = `bg/selected`, accent border and glyph. Tooltip = shape name.
#[must_use = "add it with ui.add(…)"]
pub struct BrushShape {
    kind: BrushShapeKind,
    selected: bool,
}

impl BrushShape {
    pub fn new(kind: BrushShapeKind) -> Self {
        Self {
            kind,
            selected: false,
        }
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for BrushShape {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(BRUSH_SHAPE), Sense::click());
        let s = State::of(ui, &response);
        if ui.is_rect_visible(rect) {
            let (bg, border, fg) = if !s.enabled {
                (p.bg_field, p.border_subtle, p.text_disabled)
            } else if self.selected {
                (p.bg_selected, p.accent_default, p.accent_default)
            } else if s.hovered {
                (p.bg_field, p.border_strong, p.text_primary)
            } else {
                (p.bg_field, p.border_subtle, p.text_primary)
            };
            ui.painter().rect_filled(rect, cr(radius::SM), bg);
            ui.painter().rect_stroke(
                rect,
                cr(radius::SM),
                Stroke::new(1.0_f32, border),
                StrokeKind::Inside,
            );
            // Glyphs of the Figma variants: round ⌀10, ovals 13 × 7, square 9, rects 13 × 5,
            // slanted 1.4 × 15.6.
            let c = rect.center();
            let painter = ui.painter();
            match self.kind {
                BrushShapeKind::Round => {
                    painter.circle_filled(c, 5.0, fg);
                }
                BrushShapeKind::OvalH | BrushShapeKind::OvalV => {
                    let r = if self.kind == BrushShapeKind::OvalH {
                        egui::vec2(6.5, 3.5)
                    } else {
                        egui::vec2(3.5, 6.5)
                    };
                    painter.add(egui::Shape::ellipse_filled(c, r, fg));
                }
                BrushShapeKind::Square => {
                    painter.rect_filled(Rect::from_center_size(c, Vec2::splat(9.0)), 0.0, fg);
                }
                BrushShapeKind::RectH => {
                    painter.rect_filled(Rect::from_center_size(c, egui::vec2(13.0, 5.0)), 0.0, fg);
                }
                BrushShapeKind::RectV => {
                    painter.rect_filled(Rect::from_center_size(c, egui::vec2(5.0, 13.0)), 0.0, fg);
                }
                BrushShapeKind::Slash | BrushShapeKind::Backslash => {
                    let pts = Footprint {
                        ellipse: false,
                        half: egui::vec2(7.8, 0.7),
                        angle: if self.kind == BrushShapeKind::Slash {
                            -std::f32::consts::FRAC_PI_4
                        } else {
                            std::f32::consts::FRAC_PI_4
                        },
                    }
                    .outline(c);
                    painter.add(egui::Shape::convex_polygon(pts, fg, Stroke::NONE));
                }
            }
            focus(ui, s, rect, radius::SM);
        }
        a11y(
            &response,
            egui::WidgetType::RadioButton,
            self.kind.label(),
            Some(self.selected),
        );
        super::display::tooltip(response, self.kind.label(), None)
    }
}

// ---------------------------------------------------------------------------------------------
// Brush Cursor

/// Smallest drawn footprint (R-038: the cursor follows zoom, min 4 pt).
pub const BRUSH_CURSOR_MIN: f32 = 4.0;
/// Above this screen diameter the footprint is replaced by a crosshair + value chip.
pub const BRUSH_CURSOR_MAX: f32 = 400.0;

/// `State` of `NF2/Brush Cursor` (`96:915`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrushCursorState {
    /// Drawing: solid double ring.
    #[default]
    Idle,
    /// ⌥⌘-drag resize: dashed double ring + size chip.
    Resize,
    /// Eraser footprint (square for square shapes, like the exported variant).
    Eraser,
}

/// What [`brush_cursor`] draws for a screen `diameter`: the clamped footprint size, or `None` above
/// [`BRUSH_CURSOR_MAX`] (crosshair + chip).
pub fn brush_cursor_diameter(diameter: f32) -> Option<f32> {
    (diameter <= BRUSH_CURSOR_MAX).then(|| diameter.max(BRUSH_CURSOR_MIN))
}

/// `NF2/Brush Cursor`: brush or eraser footprint at `centre` with screen `diameter` (document size ×
/// zoom). Black 85 % ring 0.5 pt outside, white 90 % ring 0.5 pt inside, 1.5 pt centre dot. `Resize`
/// draws the rings dashed. `value` ("56 px") shows a chip 4 pt below the footprint (always shown in
/// Resize when given). Above 400 pt only a crosshair and the chip are drawn.
pub fn brush_cursor(
    painter: &Painter,
    centre: Pos2,
    shape: BrushShapeKind,
    diameter: f32,
    state: BrushCursorState,
    value: Option<&str>,
) {
    let dark = Color32::from_black_alpha(217);
    let light = Color32::from_white_alpha(230);
    let Some(d) = brush_cursor_diameter(diameter) else {
        for (w, c) in [(3.0_f32, dark), (1.0_f32, light)] {
            let s = Stroke::new(w, c);
            painter.line_segment(
                [centre - egui::vec2(8.0, 0.0), centre + egui::vec2(8.0, 0.0)],
                s,
            );
            painter.line_segment(
                [centre - egui::vec2(0.0, 8.0), centre + egui::vec2(0.0, 8.0)],
                s,
            );
        }
        if let Some(v) = value {
            value_chip(painter, egui::pos2(centre.x, centre.y + 12.0), v);
        }
        return;
    };
    let foot = shape.footprint(d);
    let dashed = state == BrushCursorState::Resize;
    for (grow, c) in [(0.5, dark), (-0.5, light)] {
        let mut pts = foot.grown(grow).outline(centre);
        let stroke = Stroke::new(1.0_f32, c);
        if dashed {
            pts.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&pts, stroke, 3.0, 2.0));
        } else {
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
    }
    painter.circle(
        centre,
        0.75,
        Color32::WHITE,
        Stroke::new(0.5_f32, Color32::BLACK),
    );
    if let Some(v) = value {
        let bottom = foot.grown(0.5).bounds(centre).bottom();
        value_chip(painter, egui::pos2(centre.x, bottom + 4.0), v);
    }
}

/// Size chip of the brush cursor: `tooltip/bg`, radius sm, 6 × 2 padding, `NF2/Mono Small`
/// `text/inverse`, centred horizontally on `top_centre`.
fn value_chip(painter: &Painter, top_centre: Pos2, text: &str) {
    let p = palette(painter.ctx());
    let g = painter.layout_no_wrap(
        text.to_owned(),
        crate::typography::font(TextStyle::MonoSmall),
        p.text_inverse,
    );
    let size = g.size() + egui::vec2(12.0, 4.0);
    let rect = Rect::from_min_size(egui::pos2(top_centre.x - size.x / 2.0, top_centre.y), size);
    painter.rect_filled(rect, cr(radius::SM), p.tooltip_bg);
    painter.galley(rect.min + egui::vec2(6.0, 2.0), g, p.text_inverse);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_hit_is_the_bottom_right_12pt_of_the_tool_button() {
        let button = Rect::from_min_size(egui::pos2(100.0, 40.0), Vec2::splat(32.0));
        let hit = super::super::ToolButton::corner_hit(button);
        assert_eq!(
            hit,
            Rect::from_min_max(egui::pos2(120.0, 60.0), egui::pos2(132.0, 72.0))
        );
        assert!(hit.contains(egui::pos2(131.0, 71.0)));
        assert!(!hit.contains(button.center()));
    }

    #[test]
    fn footprints_keep_the_figma_proportions() {
        let f = BrushShapeKind::Round.footprint(26.0);
        assert!(f.ellipse && f.half == Vec2::splat(13.0));
        // Idle cursor in Figma: dark ring r 13.5, light ring r 12.5 for a 26 pt brush.
        assert_eq!(f.grown(0.5).half.x, 13.5);
        assert_eq!(f.grown(-0.5).half.x, 12.5);
        let oval = BrushShapeKind::OvalH.footprint(26.0);
        assert!((oval.half.y - 7.0).abs() < 1e-4);
        let rect = BrushShapeKind::RectV.footprint(26.0);
        assert!(!rect.ellipse && (rect.half.x - 5.0).abs() < 1e-4);
        let slash = BrushShapeKind::Slash.footprint(22.0).bounds(Pos2::ZERO);
        assert!((slash.width() - slash.height()).abs() < 1e-3);
        assert_eq!(BrushShapeKind::ALL.len(), 8);
    }

    #[test]
    fn cursor_follows_zoom_with_limits() {
        assert_eq!(brush_cursor_diameter(1.0), Some(BRUSH_CURSOR_MIN));
        assert_eq!(brush_cursor_diameter(56.0), Some(56.0));
        assert_eq!(brush_cursor_diameter(400.0), Some(400.0));
        assert_eq!(brush_cursor_diameter(401.0), None);
    }

    #[test]
    fn every_sample_stroke_renders() {
        for stroke in BrushStroke::ALL {
            for size in [1.0, 12.0, 200.0] {
                let k = brush_sample_weight(size);
                let image = rasterize(&stroke.svg(k), 192, 40).expect("valid svg");
                let ink = image.pixels.iter().filter(|p| p.a() > 0).count();
                assert!(ink > 100, "{stroke:?} at {size} px drew {ink} pixels");
            }
        }
        assert_eq!(brush_sample_weight(12.0), 1.0);
        assert_eq!(brush_sample_weight(1.0), 0.5);
        assert_eq!(brush_sample_weight(200.0), 2.0);
    }
}
