//! Surfaces and window chrome: panel / raised / popover / dialog frames, modal scrim, top bar,
//! status bar, `NF2/Zoom Control`, `NF2/Dock Zone`, `NF2/Dock Compass`, `NF2/Drop Indicator`, `NF2/Splitter`.
//!
//! Depth comes from surface levels (`bg/app → bg/panel → bg/raised → bg/field`); borders are separators only.
use crate::icons::{Icon, i16};
use crate::metrics::{self, SHADOW_DIALOG, SHADOW_POPOVER};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, Ui};

/// Surface levels as egui frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Window background (`bg/app`): top bar, status bar, gaps between docks.
    App,
    /// Docked panels and the inspector (`bg/panel`), 12 px padding.
    Panel,
    /// Docked panel body without padding (timeline, code editor, lists that draw their own rows).
    PanelFlush,
    /// Floating panels (`bg/raised`, radius 12, border, popover shadow).
    Floating,
    /// Popovers and menus (`bg/raised`, radius 8, border, popover shadow, 12 px padding).
    Popover,
    /// Modal dialogs (`bg/raised`, radius 12, dialog shadow, no padding — header/body/footer pad themselves).
    Dialog,
    /// Pasteboard around the stage (`bg/canvas`).
    Canvas,
}

impl Surface {
    pub fn frame(self, ctx: &egui::Context) -> egui::Frame {
        let p = palette(ctx);
        let border = Stroke::new(1.0_f32, p.border_subtle);
        match self {
            Self::App => egui::Frame::new().fill(p.bg_app),
            Self::Panel => egui::Frame::new()
                .fill(p.bg_panel)
                .inner_margin(egui::Margin::same(space::S3 as i8)),
            Self::PanelFlush => egui::Frame::new().fill(p.bg_panel),
            Self::Floating => egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(border)
                .corner_radius(cr(radius::XL))
                .shadow(SHADOW_POPOVER.egui()),
            Self::Popover => egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(border)
                .corner_radius(cr(radius::LG))
                .shadow(SHADOW_POPOVER.egui())
                .inner_margin(egui::Margin::same(space::S3 as i8)),
            Self::Dialog => egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(border)
                .corner_radius(cr(radius::XL))
                .shadow(SHADOW_DIALOG.egui()),
            Self::Canvas => egui::Frame::new().fill(p.bg_canvas),
        }
    }
}

/// Paint the modal scrim (`color/overlay`) over the whole window. Call before showing a dialog.
pub fn scrim(ctx: &egui::Context) {
    let p = palette(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("nf2-scrim"),
    ));
    painter.rect_filled(ctx.content_rect(), 0.0, p.overlay);
}

/// Dialog footer row: secondary/danger actions left, primary right, 24 px padding.
pub fn dialog_footer(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    // NF2 dialogs have no separator above the footer (see editor-unsaved-changes-dialog.png).
    ui.add_space(space::S4);
    ui.horizontal(|ui| {
        ui.add_space(space::S5);
        ui.spacing_mut().item_spacing.x = space::S2;
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(space::S5);
            right(ui);
        });
    });
    ui.add_space(space::S4);
}

/// Status bar (24 px): state dot + message on the left, shortcut chips on the right.
pub fn status_bar(ui: &mut Ui, ok: bool, message: &str, summary: &str, hints: &[(&str, &str)]) {
    let p = palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        Stroke::new(1.0_f32, p.border_subtle),
    );
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        ui.add_space(space::S3);
        let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), Sense::hover());
        ui.painter().circle_filled(
            dot.center(),
            3.0,
            if ok {
                p.success_default
            } else {
                p.warning_default
            },
        );
        ui.label(crate::typography::rich(message, TextStyle::Small).color(p.text_secondary));
        ui.label(crate::typography::rich(summary, TextStyle::Small).color(p.text_tertiary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(space::S3);
            ui.spacing_mut().item_spacing.x = space::S1;
            for (key, label) in hints.iter().rev() {
                ui.label(crate::typography::rich(*label, TextStyle::Small).color(p.text_tertiary));
                ui.scope(|ui| {
                    ui.spacing_mut().interact_size.y = 18.0;
                    super::display::kbd(ui, key, false)
                });
                ui.add_space(space::S2);
            }
        });
    });
}

/// `NF2/Zoom Control` — floating pill at the bottom centre of the canvas:
/// hand · − · 75 % · + · fit · rulers · guides · grid · snap to guides · view settings.
pub struct ZoomControl {
    pub zoom: f32,
    pub hand: bool,
    pub rulers: Option<bool>,
    pub guides: Option<bool>,
    pub grid: Option<bool>,
    pub snap: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZoomAction {
    #[default]
    None,
    Hand,
    ZoomOut,
    ZoomIn,
    ZoomReset,
    Fit,
    Rulers,
    Guides,
    Grid,
    Snap,
    ViewSettings,
}

impl ZoomControl {
    /// Show at `center_bottom` (canvas coordinates) on the foreground layer. Returns the clicked action.
    pub fn show(
        self,
        ctx: &egui::Context,
        id: egui::Id,
        center_bottom: egui::Pos2,
    ) -> (ZoomAction, Option<Response>) {
        let mut action = ZoomAction::None;
        let mut settings = None;
        egui::Area::new(id)
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(center_bottom)
            .show(ctx, |ui| {
                Surface::Floating
                    .frame(ctx)
                    .corner_radius(cr(radius::LG))
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let b = |ui: &mut Ui,
                                     icon: Icon,
                                     name: &str,
                                     key: Option<&str>,
                                     on: bool| {
                                let mut btn = super::IconButton::new(icon, name).selected(on);
                                if let Some(k) = key {
                                    btn = btn.shortcut(k);
                                }
                                ui.add(btn).clicked()
                            };
                            if b(ui, i16::HAND, "Hand", Some("H"), self.hand) {
                                action = ZoomAction::Hand;
                            }
                            super::display::divider(ui, true);
                            if b(ui, i16::MINUS, "Zoom out", Some("⌘−"), false) {
                                action = ZoomAction::ZoomOut;
                            }
                            let pct = format!("{:.0}%", self.zoom * 100.0);
                            if ui
                                .add(super::Button::ghost(&pct).width(52.0))
                                .on_hover_text("Reset to 100 % (⌘1)")
                                .clicked()
                            {
                                action = ZoomAction::ZoomReset;
                            }
                            if b(ui, i16::PLUS, "Zoom in", Some("⌘="), false) {
                                action = ZoomAction::ZoomIn;
                            }
                            super::display::divider(ui, true);
                            if b(ui, i16::FIT, "Fit stage in window", Some("⌘0"), false) {
                                action = ZoomAction::Fit;
                            }
                            let toggles = [
                                (
                                    self.rulers,
                                    i16::RULER,
                                    "Rulers",
                                    "⌥⇧⌘R",
                                    ZoomAction::Rulers,
                                ),
                                (self.guides, i16::GUIDES, "Guides", "⌘;", ZoomAction::Guides),
                                (self.grid, i16::GRID, "Grid", "⌘'", ZoomAction::Grid),
                                (
                                    self.snap,
                                    i16::SNAP_GUIDES,
                                    "Snap to guides",
                                    "⇧⌘;",
                                    ZoomAction::Snap,
                                ),
                            ];
                            if toggles.iter().any(|t| t.0.is_some()) {
                                super::display::divider(ui, true);
                            }
                            for (state, icon, name, key, a) in toggles {
                                if let Some(on) = state
                                    && b(ui, icon, name, Some(key), on)
                                {
                                    action = a;
                                }
                            }
                            let r = ui.add(super::IconButton::new(
                                i16::VIEW_SETTINGS,
                                "Rulers, guides and grid…",
                            ));
                            if r.clicked() {
                                action = ZoomAction::ViewSettings;
                            }
                            settings = Some(r);
                        });
                    });
            });
        (action, settings)
    }
}

/// `NF2/Dock Zone` — accent-tinted drop target with "Dock here".
pub fn dock_zone(painter: &egui::Painter, ctx: &egui::Context, rect: Rect, label: &str) {
    let p = palette(ctx);
    painter.rect_filled(rect, cr(radius::LG), p.accent_subtle.gamma_multiply(0.85));
    painter.rect_stroke(
        rect,
        cr(radius::LG),
        Stroke::new(2.0_f32, p.accent_default),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        crate::typography::font(TextStyle::BodyStrong),
        p.accent_default,
    );
}

/// `NF2/Dock Compass` — 5 targets (left, right, top, bottom, centre = add as tab) around `center`.
/// Returns the rects in the order left, right, top, bottom, centre so callers can hit-test.
pub fn dock_compass(
    painter: &egui::Painter,
    ctx: &egui::Context,
    center: egui::Pos2,
    hovered: Option<usize>,
) -> [Rect; 5] {
    let p = palette(ctx);
    let s = 32.0;
    let g = 4.0 + s;
    let rects = [
        Rect::from_center_size(center + egui::vec2(-g, 0.0), egui::vec2(s, s)),
        Rect::from_center_size(center + egui::vec2(g, 0.0), egui::vec2(s, s)),
        Rect::from_center_size(center + egui::vec2(0.0, -g), egui::vec2(s, s)),
        Rect::from_center_size(center + egui::vec2(0.0, g), egui::vec2(s, s)),
        Rect::from_center_size(center, egui::vec2(s, s)),
    ];
    // Arrow direction per target; the centre shows the tab glyph.
    let dirs = [
        egui::vec2(-1.0, 0.0),
        egui::vec2(1.0, 0.0),
        egui::vec2(0.0, -1.0),
        egui::vec2(0.0, 1.0),
    ];
    for (i, r) in rects.iter().enumerate() {
        let on = hovered == Some(i);
        painter.rect_filled(
            *r,
            cr(radius::MD),
            if on { p.accent_default } else { p.bg_raised },
        );
        painter.rect_stroke(
            *r,
            cr(radius::MD),
            Stroke::new(1.0_f32, p.border_default),
            StrokeKind::Inside,
        );
        let color = if on {
            p.text_on_accent
        } else {
            p.text_secondary
        };
        match dirs.get(i) {
            Some(d) => {
                let c = r.center();
                let n = egui::vec2(-d.y, d.x);
                let tip = c + *d * 5.0;
                let base = c - *d * 3.0;
                painter.add(egui::Shape::line(
                    vec![base + n * 5.0, tip, base - n * 5.0],
                    Stroke::new(1.5_f32, color),
                ));
            }
            None => crate::icons::paint(painter, ctx, *r, i16::SIDEBAR, color),
        }
    }
    rects
}

/// `NF2/Drop Indicator` — 2 px accent insertion line with a ring at the start (layers, library, tabs).
pub fn drop_indicator(
    painter: &egui::Painter,
    ctx: &egui::Context,
    from: egui::Pos2,
    to: egui::Pos2,
) {
    let p = palette(ctx);
    painter.line_segment([from, to], Stroke::new(2.0_f32, p.accent_default));
    painter.circle(
        from,
        3.0,
        p.bg_panel,
        Stroke::new(1.5_f32, p.accent_default),
    );
}

/// `NF2/Splitter` — 1 px separator that shows a 3 px accent grip while hovered or dragged.
pub fn splitter(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    vertical: bool,
    response: &Response,
) {
    let p = palette(ctx);
    let active = response.hovered() || response.dragged();
    let c = if active {
        p.accent_default
    } else {
        p.border_subtle
    };
    if vertical {
        painter.vline(rect.center().x, rect.y_range(), Stroke::new(1.0_f32, c));
        if active {
            painter.rect_filled(
                Rect::from_center_size(rect.center(), egui::vec2(3.0, 24.0)),
                cr(1.5),
                c,
            );
        }
    } else {
        painter.hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, c));
        if active {
            painter.rect_filled(
                Rect::from_center_size(rect.center(), egui::vec2(24.0, 3.0)),
                cr(1.5),
                c,
            );
        }
    }
    if active {
        ctx.set_cursor_icon(if vertical {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        });
    }
}

/// `NF2/Drag Ghost` — label chip following the pointer while a panel tab is dragged.
pub fn drag_ghost(ctx: &egui::Context, label: &str) {
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    let p = palette(ctx);
    egui::Area::new(egui::Id::new("nf2-drag-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(12.0, 12.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(Stroke::new(1.0_f32, p.accent_default))
                .corner_radius(cr(radius::MD))
                .inner_margin(egui::Margin::symmetric(space::S3 as i8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        crate::icons::show(ui, i16::GRIP, p.text_tertiary);
                        ui.label(
                            crate::typography::rich(label, TextStyle::Body).color(p.text_primary),
                        );
                    });
                });
        });
}

/// Fixed height of the window regions, for callers laying out egui panels.
pub const TOP_BAR: f32 = metrics::TOP_BAR;
pub const STATUS_BAR: f32 = metrics::STATUS_BAR;
