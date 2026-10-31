//! Passive components: `NF2/Badge`, `NF2/Kbd`, `NF2/Tooltip`, `NF2/Swatch`, `NF2/Divider`, `NF2/Logo`,
//! `NF2/Spinner`, `NF2/Progress`, `NF2/Toast`, `NF2/Empty State`.
use super::{fill, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{BADGE, KBD};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, Ui, Widget};

/// `Type` of `NF2/Badge`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeType {
    #[default]
    Neutral,
    Accent,
    /// Unsaved changes, pending state.
    Warning,
}

/// `NF2/Badge` — pill with a leading dot ("Unsaved", "HTML5 · Rust / WebAssembly", "1 warning").
pub struct Badge<'a> {
    label: &'a str,
    kind: BadgeType,
}

impl<'a> Badge<'a> {
    pub fn new(label: &'a str, kind: BadgeType) -> Self {
        Self { label, kind }
    }
}

impl Widget for Badge<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let (bg, fg) = match self.kind {
            BadgeType::Neutral => (p.bg_hover, p.text_secondary),
            BadgeType::Accent => (p.accent_subtle, p.accent_default),
            BadgeType::Warning => (p.warning_subtle, p.warning_default),
        };
        let g = galley(ui, self.label, TextStyle::Small, fg);
        let w = space::S2 + 6.0 + space::S1 + 2.0 + g.size().x + space::S2;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(w, BADGE), Sense::hover());
        fill(ui, rect, radius::FULL, bg, None);
        ui.painter().circle_filled(
            egui::pos2(rect.left() + space::S2 + 3.0, rect.center().y),
            3.0,
            fg,
        );
        text_at(
            ui,
            rect.left() + space::S2 + 6.0 + space::S1 + 2.0,
            rect,
            g,
            fg,
        );
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, self.label));
        response
    }
}

/// `NF2/Kbd` — shortcut chip ("F6", "⌘↵"). `inverse` = drawn inside a tooltip.
pub fn kbd(ui: &mut Ui, key: &str, inverse: bool) -> Response {
    let p = palette(ui.ctx());
    let (bg, border, fg) = if inverse {
        (p.text_primary, p.text_primary, p.text_inverse)
    } else {
        (Color32::TRANSPARENT, p.border_default, p.text_secondary)
    };
    let g = galley(ui, key, TextStyle::Small, fg);
    let w = (g.size().x + 2.0 * 6.0).max(20.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, KBD), Sense::hover());
    fill(ui, rect, radius::SM, bg, Some(border));
    let x = rect.center().x - g.size().x / 2.0;
    text_at(ui, x, rect, g, fg);
    response
}

/// `NF2/Tooltip` — `tooltip/bg` + `text/inverse`, label and optional shortcut chip.
/// Use instead of `on_hover_text` for every icon-only control.
pub fn tooltip(response: Response, label: &str, shortcut: Option<&str>) -> Response {
    let ctx = response.ctx.clone();
    if !response.hovered() || label.is_empty() || ctx.input(|i| i.pointer.any_down()) {
        return response;
    }
    let delay = ctx.style().interaction.tooltip_delay as f64;
    let still = ctx.input(|i| i.pointer.time_since_last_movement()) as f64;
    if still < delay {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - still + 0.01));
        return response;
    }
    let p = palette(&ctx);
    let anchor = response.rect.center_bottom() + egui::vec2(0.0, space::S2);
    egui::Area::new(response.id.with("nf2-tooltip"))
        .order(egui::Order::Tooltip)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(anchor)
        .interactable(false)
        .constrain(true)
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(p.tooltip_bg)
                .corner_radius(cr(radius::MD))
                .inner_margin(egui::Margin::symmetric(10, 5))
                .shadow(crate::metrics::SHADOW_POPOVER.egui())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::S3;
                        ui.label(
                            crate::typography::rich(label, TextStyle::Body).color(p.text_inverse),
                        );
                        if let Some(k) = shortcut {
                            kbd(ui, k, true);
                        }
                    });
                });
        });
    response
}

/// `NF2/Swatch` — 22 px colour chip (checkerboard behind translucent colours).
pub fn swatch(ui: &mut Ui, color: Color32, side: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
    paint_swatch(ui, rect, color, radius::SM);
    response
}

/// Paint a colour chip with a checkerboard for alpha and a subtle border.
pub fn paint_swatch(ui: &Ui, rect: Rect, color: Color32, r: f32) {
    let p = palette(ui.ctx());
    let painter = ui.painter();
    if color.a() < 255 {
        painter.rect_filled(rect, cr(r), Color32::WHITE);
        let cell = (rect.width() / 4.0).max(3.0);
        let mut y = rect.top();
        let mut row = 0;
        while y < rect.bottom() {
            let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { cell };
            while x < rect.right() {
                let c =
                    Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell, cell)).intersect(rect);
                painter.rect_filled(c, 0.0, Color32::from_gray(204));
                x += cell * 2.0;
            }
            y += cell;
            row += 1;
        }
    }
    painter.rect_filled(rect, cr(r), color);
    painter.rect_stroke(
        rect,
        cr(r),
        Stroke::new(1.0_f32, p.border_default),
        egui::StrokeKind::Inside,
    );
}

/// `NF2/Divider` — 1 px `border/subtle` line; `vertical` = between grouped controls (20 px tall).
pub fn divider(ui: &mut Ui, vertical: bool) -> Response {
    let p = palette(ui.ctx());
    let size = if vertical {
        egui::vec2(1.0 + space::S2, 20.0)
    } else {
        egui::vec2(ui.available_width(), 1.0 + space::S2)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let stroke = Stroke::new(1.0_f32, p.border_subtle);
    if vertical {
        ui.painter().vline(rect.center().x, rect.y_range(), stroke);
    } else {
        ui.painter().hline(rect.x_range(), rect.center().y, stroke);
    }
    response
}

/// `NF2/Logo` — 28 px accent tile with the NeoFlash mark.
pub fn logo(ui: &mut Ui, side: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
    let k = side / 28.0;
    ui.painter()
        .rect_filled(rect, cr(radius::LG * k), p.accent_default);
    let c = rect.center();
    ui.painter()
        .circle_stroke(c, 8.0 * k, Stroke::new(2.0 * k, p.text_on_accent));
    ui.painter().circle_filled(c, 4.0 * k, p.text_on_accent);
    response
}

/// `NF2/Spinner` — 20 px indeterminate ring.
pub fn spinner(ui: &mut Ui) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let c = rect.center();
    ui.painter()
        .circle_stroke(c, 8.0, Stroke::new(2.0_f32, p.border_default));
    let start = t * 5.0;
    let pts: Vec<_> = (0..=16)
        .map(|i| {
            let a = start + i as f32 / 16.0 * std::f32::consts::FRAC_PI_2 * 1.5;
            c + 8.0 * egui::vec2(a.cos(), a.sin())
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        pts,
        Stroke::new(2.0_f32, p.accent_default),
    ));
    ui.ctx().request_repaint();
    response
}

/// `NF2/Progress` — label row + 4 px bar. `fraction = None` = indeterminate.
pub fn progress(ui: &mut Ui, label: &str, fraction: Option<f32>, width: f32) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::hover());
    let label_rect = Rect::from_min_size(rect.min, egui::vec2(width, 16.0));
    text_at(
        ui,
        rect.left(),
        label_rect,
        galley(ui, label, TextStyle::Small, p.text_secondary),
        p.text_secondary,
    );
    if let Some(f) = fraction {
        let pct = format!("{:.0} %", f * 100.0);
        let g = galley(ui, &pct, TextStyle::MonoSmall, p.text_tertiary);
        text_at(
            ui,
            rect.right() - g.size().x,
            label_rect,
            g,
            p.text_tertiary,
        );
    }
    let bar = Rect::from_min_size(
        egui::pos2(rect.left(), rect.bottom() - 4.0),
        egui::vec2(width, 4.0),
    );
    ui.painter().rect_filled(bar, cr(2.0), p.bg_hover);
    let filled = match fraction {
        Some(f) => Rect::from_min_size(bar.min, egui::vec2(width * f.clamp(0.0, 1.0), 4.0)),
        None => {
            let t = (ui.input(|i| i.time) as f32 * 0.6).fract();
            ui.ctx().request_repaint();
            Rect::from_min_size(
                egui::pos2(bar.left() + (width * 1.3) * t - width * 0.3, bar.top()),
                egui::vec2(width * 0.3, 4.0),
            )
            .intersect(bar)
        }
    };
    ui.painter().rect_filled(filled, cr(2.0), p.accent_default);
    response
}

/// `Type` of `NF2/Toast`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastType {
    Info,
    Success,
    Warning,
    Error,
}

/// `NF2/Toast` — 46 px notification with a coloured stripe, icon, message, optional action and close.
pub struct Toast<'a> {
    pub kind: ToastType,
    pub message: &'a str,
    pub action: Option<&'a str>,
}

/// What the user did with a toast this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToastResponse {
    pub action: bool,
    pub close: bool,
}

impl Toast<'_> {
    pub fn show(self, ui: &mut Ui, width: f32) -> ToastResponse {
        let p = palette(ui.ctx());
        let (color, icon) = match self.kind {
            ToastType::Info => (p.accent_default, i16::INFO),
            ToastType::Success => (p.success_default, i16::CHECK),
            ToastType::Warning => (p.warning_default, i16::WARNING),
            ToastType::Error => (p.danger_default, i16::ERROR),
        };
        let mut out = ToastResponse::default();
        egui::Frame::new()
            .fill(p.bg_raised)
            .stroke(Stroke::new(1.0_f32, p.border_subtle))
            .corner_radius(cr(radius::LG))
            .inner_margin(egui::Margin::symmetric(space::S3 as i8, 7))
            .shadow(crate::metrics::SHADOW_POPOVER.egui())
            .show(ui, |ui| {
                ui.set_width(width - 2.0 * space::S3);
                ui.horizontal(|ui| {
                    let (stripe, _) = ui.allocate_exact_size(egui::vec2(3.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(stripe, cr(1.5), color);
                    icons::show(ui, icon, color);
                    ui.label(
                        crate::typography::rich(self.message, TextStyle::Body)
                            .color(p.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        out.close = ui
                            .add(super::IconButton::new(i16::X, "Dismiss").side(24.0))
                            .clicked();
                        if let Some(a) = self.action {
                            out.action = ui.add(super::Button::ghost(a)).clicked();
                        }
                    });
                });
            });
        out
    }
}

/// `NF2/Empty State` — icon in a circle, title, one-sentence explanation, optional action.
pub struct EmptyState<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub body: &'a str,
    pub action: Option<&'a str>,
}

impl EmptyState<'_> {
    /// Returns `true` when the action button was clicked.
    pub fn show(self, ui: &mut Ui) -> bool {
        let p = palette(ui.ctx());
        let mut clicked = false;
        ui.vertical_centered(|ui| {
            ui.add_space(space::S4);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 18.0, p.bg_hover);
            icons::paint(ui.painter(), ui.ctx(), rect, self.icon, p.text_secondary);
            ui.add_space(space::S2);
            ui.label(
                crate::typography::rich(self.title, TextStyle::BodyStrong).color(p.text_primary),
            );
            ui.label(crate::typography::rich(self.body, TextStyle::Small).color(p.text_tertiary));
            if let Some(a) = self.action {
                ui.add_space(space::S2);
                clicked = ui.add(super::Button::new(a)).clicked();
            }
            ui.add_space(space::S4);
        });
        clicked
    }
}
