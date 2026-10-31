//! Easing: `NF2/Ease Preset`, `NF2/Ease Graph` and the Ease panel preview track.
//!
//! The graph is a display component: it draws a cubic-bezier ease (`X1 Y1 X2 Y2`, CSS convention) over a
//! tween of `frames` frames, with handles, the playhead and — for the Overshoot / Undershoot ranges — the
//! out-of-range bands tinted `warning/subtle` and out-of-range handles drawn amber (§10, `NF2/Ease Graph`).
use super::{State, a11y, galley};
use crate::theme::{cr, palette};
use crate::tokens::{Palette, TextStyle, radius, space};
use eframe::egui::{self, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Widget};

/// `Preset` of `NF2/Ease Preset`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EasePresetKind {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Back,
    Bounce,
    Elastic,
    #[default]
    Custom,
}

impl EasePresetKind {
    pub const ALL: [Self; 8] = [
        Self::Linear,
        Self::EaseIn,
        Self::EaseOut,
        Self::EaseInOut,
        Self::Back,
        Self::Bounce,
        Self::Elastic,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::EaseInOut => "Ease In-Out",
            Self::Back => "Back",
            Self::Bounce => "Bounce",
            Self::Elastic => "Elastic",
            Self::Custom => "Custom",
        }
    }

    /// Progress (0 → 1, may overshoot) at time `t` in 0..=1; used for the preset glyph.
    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t * t,
            Self::EaseOut => 1.0 - (1.0 - t).powi(3),
            Self::EaseInOut => t * t * (3.0 - 2.0 * t),
            Self::Back => {
                let (c1, c3) = (1.70158, 2.70158);
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            Self::Bounce => bounce_out(t),
            Self::Elastic => {
                if t <= 0.0 || t >= 1.0 {
                    t
                } else {
                    2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * std::f32::consts::TAU / 3.0).sin()
                        + 1.0
                }
            }
            Self::Custom => cubic_bezier_ease([0.6, 0.02, 0.4, 0.98], t),
        }
    }
}

fn bounce_out(t: f32) -> f32 {
    let (n1, d1) = (7.5625, 2.75);
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

/// Point of the cubic bezier (0,0) → (x1,y1) → (x2,y2) → (1,1) at parameter `s`.
pub fn cubic_bezier_point(b: [f32; 4], s: f32) -> (f32, f32) {
    let u = 1.0 - s;
    let w1 = 3.0 * u * u * s;
    let w2 = 3.0 * u * s * s;
    let w3 = s * s * s;
    (w1 * b[0] + w2 * b[2] + w3, w1 * b[1] + w2 * b[3] + w3)
}

/// Eased progress at time `t` for a CSS-style cubic bezier (solves x(s) = t by bisection).
pub fn cubic_bezier_ease(b: [f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..32 {
        let mid = 0.5 * (lo + hi);
        if cubic_bezier_point(b, mid).0 < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic_bezier_point(b, 0.5 * (lo + hi)).1
}

/// `NF2/Ease Preset` — 72 × 64 card: curve glyph above the preset name. `Selected` = accent card.
pub struct EasePreset {
    pub preset: EasePresetKind,
    pub selected: bool,
}

impl Widget for EasePreset {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::vec2(72.0, 64.0), Sense::click());
        let s = State::of(ui, &response);
        let (bg, border, curve, fg) = if self.selected {
            (
                p.accent_subtle,
                p.accent_default,
                p.accent_default,
                p.text_primary,
            )
        } else if s.hovered {
            (
                p.bg_hover,
                p.border_strong,
                p.text_secondary,
                p.text_primary,
            )
        } else {
            (
                p.bg_field,
                p.border_subtle,
                p.text_secondary,
                p.text_secondary,
            )
        };
        let (curve, fg) = if s.enabled {
            (curve, fg)
        } else {
            (Palette::tint(curve, 0.6), p.text_disabled)
        };
        ui.painter().rect_filled(rect, cr(radius::LG), bg);
        ui.painter().rect_stroke(
            rect,
            cr(radius::LG),
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
        let glyph = Rect::from_min_size(
            rect.min + egui::vec2(16.0, space::S2 + 2.0),
            egui::vec2(40.0, 24.0),
        );
        let points: Vec<Pos2> = (0..=32)
            .map(|i| {
                let t = i as f32 / 32.0;
                egui::pos2(
                    glyph.left() + t * glyph.width(),
                    glyph.bottom() - self.preset.sample(t) * glyph.height(),
                )
            })
            .collect();
        ui.painter()
            .add(Shape::line(points, Stroke::new(1.5_f32, curve)));
        let g = galley(ui, self.preset.label(), TextStyle::Small, fg);
        let pos = egui::pos2(
            rect.center().x - g.size().x / 2.0,
            rect.bottom() - space::S2 - g.size().y,
        );
        ui.painter().galley(pos, g, fg);
        super::focus(ui, s, rect, radius::LG);
        a11y(
            &response,
            egui::WidgetType::RadioButton,
            self.preset.label(),
            Some(self.selected),
        );
        response
    }
}

/// `Range` of `NF2/Ease Graph`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EaseRange {
    /// `0–100%`.
    #[default]
    Normal,
    /// `Overshoot (above 100%)`.
    Overshoot,
    /// `Undershoot (below 0%)`.
    Undershoot,
}

/// `NF2/Ease Graph` — 336 × 224: progress (Y, %) over frames (X), bezier curve with handles, playhead.
pub struct EaseGraph {
    pub range: EaseRange,
    /// `X1 Y1 X2 Y2`.
    pub bezier: [f32; 4],
    /// Tween length in frames (X axis labels every 5).
    pub frames: u32,
    /// Current frame inside the tween (playhead), if any.
    pub playhead: Option<u32>,
    pub width: f32,
}

impl Default for EaseGraph {
    fn default() -> Self {
        Self {
            range: EaseRange::Normal,
            bezier: [0.6, 0.02, 0.4, 0.98],
            frames: 40,
            playhead: None,
            width: 336.0,
        }
    }
}

impl Widget for EaseGraph {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.width, 224.0), Sense::hover());
        let painter = ui.painter_at(rect.expand(space::S2));
        let label_w = 40.0;
        let plot = Rect::from_min_max(
            egui::pos2(rect.left() + label_w, rect.top() + space::S2),
            egui::pos2(rect.right() - space::S2, rect.bottom() - 24.0),
        );
        let (lo, hi) = match self.range {
            EaseRange::Normal => (0.0_f32, 1.0_f32),
            EaseRange::Overshoot | EaseRange::Undershoot => (-0.3, 1.3),
        };
        let y_of = |v: f32| plot.bottom() - (v - lo) / (hi - lo) * plot.height();
        let x_of = |t: f32| plot.left() + t * plot.width();
        painter.rect_filled(plot, cr(radius::SM), p.bg_field);
        if self.range != EaseRange::Normal {
            let band = p.warning_subtle;
            painter.rect_filled(
                Rect::from_min_max(plot.min, egui::pos2(plot.right(), y_of(1.0))),
                0.0,
                band,
            );
            painter.rect_filled(
                Rect::from_min_max(egui::pos2(plot.left(), y_of(0.0)), plot.max),
                0.0,
                band,
            );
        }
        // Horizontal grid every 20 %, labels in NF2/Mono Small.
        let mut v = if lo < 0.0 { -0.2 } else { 0.0 };
        while v <= hi + 1e-3 {
            let y = y_of(v);
            let strong = v.abs() < 1e-3 || (v - 1.0).abs() < 1e-3;
            painter.hline(
                plot.x_range(),
                y,
                Stroke::new(
                    1.0_f32,
                    if strong {
                        p.border_strong
                    } else {
                        p.border_subtle
                    },
                ),
            );
            let text = format!("{}%", (v * 100.0).round() as i32);
            let g = galley(ui, &text, TextStyle::MonoSmall, p.text_tertiary);
            painter.galley(
                egui::pos2(plot.left() - space::S1 - g.size().x, y - g.size().y / 2.0),
                g,
                p.text_tertiary,
            );
            v += 0.2;
        }
        // Vertical grid every 5 frames.
        let frames = self.frames.max(1);
        let mut f = 0;
        while f <= frames {
            let x = x_of(f as f32 / frames as f32);
            painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, p.border_subtle));
            let g = galley(ui, &f.to_string(), TextStyle::MonoSmall, p.text_tertiary);
            painter.galley(
                egui::pos2(x - g.size().x / 2.0, plot.bottom() + space::S1),
                g,
                p.text_tertiary,
            );
            f += 5;
        }
        if let Some(frame) = self.playhead {
            let x = x_of(frame.min(frames) as f32 / frames as f32);
            painter.vline(x, plot.y_range(), Stroke::new(1.0_f32, p.timeline_playhead));
        }
        let b = self.bezier;
        let start = egui::pos2(x_of(0.0), y_of(0.0));
        let end = egui::pos2(x_of(1.0), y_of(1.0));
        let h1 = egui::pos2(x_of(b[0]), y_of(b[1]));
        let h2 = egui::pos2(x_of(b[2]), y_of(b[3]));
        let handle_line = Stroke::new(1.0_f32, p.text_tertiary);
        painter.line_segment([start, h1], handle_line);
        painter.line_segment([end, h2], handle_line);
        let curve: Vec<Pos2> = (0..=64)
            .map(|i| {
                let (x, y) = cubic_bezier_point(b, i as f32 / 64.0);
                egui::pos2(x_of(x), y_of(y))
            })
            .collect();
        painter.add(Shape::line(curve, Stroke::new(2.0_f32, p.accent_default)));
        for (pos, value) in [(h1, b[1]), (h2, b[3])] {
            let out = !(0.0..=1.0).contains(&value);
            let c = if out {
                p.warning_default
            } else {
                p.accent_default
            };
            painter.circle(pos, 5.0, p.bg_field, Stroke::new(2.0_f32, c));
        }
        for pos in [start, end] {
            painter.rect_filled(
                Rect::from_center_size(pos, egui::vec2(8.0, 8.0)),
                0.0,
                p.text_primary,
            );
        }
        let name = format!("Ease curve {:.2} {:.2} {:.2} {:.2}", b[0], b[1], b[2], b[3]);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &name));
        response
    }
}

/// Ease panel preview track — field-like strip with one dot per sampled frame spaced by the ease;
/// the dot at `progress` (0..=1 of the tween time) is the accent "ball".
pub fn ease_preview_track(ui: &mut Ui, bezier: [f32; 4], progress: f32, width: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, crate::metrics::ROW), Sense::hover());
    ui.painter().rect_filled(rect, cr(radius::MD), p.bg_field);
    ui.painter().rect_stroke(
        rect,
        cr(radius::MD),
        Stroke::new(1.0_f32, p.border_subtle),
        StrokeKind::Inside,
    );
    let lane = rect.shrink2(egui::vec2(space::S4, 0.0));
    let x_of = |t: f32| lane.left() + cubic_bezier_ease(bezier, t).clamp(-0.1, 1.1) * lane.width();
    for i in 0..=10 {
        ui.painter().circle_filled(
            egui::pos2(x_of(i as f32 / 10.0), rect.center().y),
            4.0,
            p.border_strong,
        );
    }
    ui.painter().circle_filled(
        egui::pos2(x_of(progress), rect.center().y),
        7.0,
        if ui.is_enabled() {
            p.accent_default
        } else {
            Palette::tint(p.accent_default, 0.6)
        },
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Ease preview"));
    response
}
