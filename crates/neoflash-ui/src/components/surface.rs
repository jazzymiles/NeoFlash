//! Surfaces and window chrome: panel / raised / popover / dialog frames, modal scrim, top bar,
//! status bar, `NF2/Zoom Control`, `NF2/Dock Zone`, `NF2/Dock Compass`, `NF2/Drop Indicator`, `NF2/Splitter`.
//!
//! Depth comes from surface levels (`bg/app → bg/panel → bg/raised → bg/field`); borders are separators only.
use crate::icons::{Icon, i16};
use crate::metrics::{SHADOW_DIALOG, SHADOW_POPOVER};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, Ui};

/// Surface levels as egui frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Window background (`bg/app`): top bar, status bar, gaps between docks.
    App,
    /// Docked panels and the inspector (`bg/panel`), `space::s4()` padding (`pad/panel`).
    Panel,
    /// Docked panel body without padding (timeline, code editor, lists that draw their own rows).
    PanelFlush,
    /// Floating panels (`bg/raised`, `radius::xl()`, border, popover shadow).
    Floating,
    /// Popovers and menus (`bg/raised`, `radius::lg()`, border, popover shadow, `space::s3()` padding).
    Popover,
    /// Modal dialogs (`bg/raised`, `radius::xl()`, dialog shadow, no padding — header/body/footer pad themselves).
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
                .inner_margin(egui::Margin::same(space::s4() as i8)),
            Self::PanelFlush => egui::Frame::new().fill(p.bg_panel),
            Self::Floating => egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(border)
                .corner_radius(cr(radius::xl()))
                .shadow(SHADOW_POPOVER.egui()),
            Self::Popover => egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(border)
                .corner_radius(cr(radius::lg()))
                .shadow(SHADOW_POPOVER.egui())
                .inner_margin(egui::Margin::same(space::s3() as i8)),
            Self::Dialog => egui::Frame::new()
                .fill(p.bg_raised)
                .stroke(border)
                .corner_radius(cr(radius::xl()))
                .shadow(SHADOW_DIALOG.egui()),
            Self::Canvas => egui::Frame::new().fill(p.bg_canvas),
        }
    }
}

/// `shadow` for a surface only `height` points high (Stage toolbar, toast, tooltip, drag ghost).
/// A blur as wide as the surface makes the tessellator draw hairlines from its corners, so the blur
/// stays 4 pt under the height and the offset shrinks with it; taller surfaces get `shadow` unchanged.
pub fn fitted_shadow(shadow: crate::metrics::Shadow, height: f32) -> egui::epaint::Shadow {
    let blur = shadow.blur.min((height - 4.0).max(1.0));
    crate::metrics::Shadow {
        y: shadow.y * blur / shadow.blur,
        blur,
        alpha: shadow.alpha,
    }
    .egui()
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
    ui.add_space(space::s4());
    ui.horizontal(|ui| {
        ui.add_space(space::s5());
        ui.spacing_mut().item_spacing.x = space::s2();
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(space::s5());
            right(ui);
        });
    });
    ui.add_space(space::s4());
}

/// Status bar (`status_bar()` high): state dot + message on the left, shortcut chips on the right.
pub fn status_bar(ui: &mut Ui, ok: bool, message: &str, summary: &str, hints: &[(&str, &str)]) {
    let p = palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        Stroke::new(1.0_f32, p.border_subtle),
    );
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        ui.add_space(space::s3());
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
            ui.add_space(space::s3());
            ui.spacing_mut().item_spacing.x = space::s1();
            for (key, label) in hints.iter().rev() {
                ui.label(crate::typography::rich(*label, TextStyle::Small).color(p.text_tertiary));
                ui.scope(|ui| {
                    ui.spacing_mut().interact_size.y = crate::metrics::kbd();
                    super::display::kbd(ui, key, false)
                });
                ui.add_space(space::s2());
            }
        });
    });
}

/// `NF3/Stage toolbar` — floating bar at the bottom centre of the canvas, `zoom_control()` high with
/// `control()` icon buttons: hand · − · 75 % · + · fit · rulers · guides · grid · snap to guides · view
/// settings. The percentage sits in a fixed [`ZOOM_PERCENT_CELL`], so the bar keeps its width while
/// zooming.
pub struct ZoomControl {
    pub zoom: f32,
    pub hand: bool,
    pub rulers: Option<bool>,
    pub guides: Option<bool>,
    pub grid: Option<bool>,
    pub snap: Option<bool>,
}

/// Width of the zoom percentage cell: fits "6400%" in `NF2/Body`; never scales with the value.
pub const ZOOM_PERCENT_CELL: f32 = 44.0;
/// Gap between the controls of the Stage toolbar.
const ZOOM_GAP: f32 = 2.0;

/// Width of one vertical `Divider` between grouped controls.
fn zoom_divider_width() -> f32 {
    1.0 + space::s2()
}

/// Inner margin that makes the bordered bar exactly `zoom_control()` high around `control()` buttons.
fn zoom_bar_margin() -> egui::Margin {
    let v = (crate::metrics::zoom_control() - crate::metrics::control()) / 2.0 - 1.0;
    egui::Margin::same(v.max(0.0) as i8)
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
    /// Frame of the floating bar: `bg/raised`, border, `radius::lg()`, a shadow fitted to its height
    /// and the margin that makes it exactly `zoom_control()` high around [`ZoomControl::contents`].
    pub fn bar_frame(ctx: &egui::Context) -> egui::Frame {
        Surface::Floating
            .frame(ctx)
            .corner_radius(cr(radius::lg()))
            .shadow(fitted_shadow(
                SHADOW_POPOVER,
                crate::metrics::zoom_control(),
            ))
            .inner_margin(zoom_bar_margin())
    }

    /// Show at `center_bottom` (canvas coordinates) on the foreground layer. Returns the clicked action.
    pub fn show(
        self,
        ctx: &egui::Context,
        id: egui::Id,
        center_bottom: egui::Pos2,
    ) -> (ZoomAction, Option<Response>) {
        self.show_floating(ctx, id, center_bottom, egui::Align2::CENTER_BOTTOM)
            .0
    }

    /// Floating toolbar pinned by `pivot` at `pos` (R-053: centre-bottom when expanded, right-bottom when
    /// it expands from the collapsed pill). Also returns the area response (hover, context menu).
    pub fn show_floating(
        self,
        ctx: &egui::Context,
        id: egui::Id,
        pos: egui::Pos2,
        pivot: egui::Align2,
    ) -> ((ZoomAction, Option<Response>), Response) {
        let inner = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .pivot(pivot)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                Self::bar_frame(ctx).show(ui, |ui| self.contents(ui)).inner
            });
        (inner.inner, inner.response)
    }

    /// The controls in a row: hand · zoom out · % · zoom in · fit · rulers / guides / grid / snap · view
    /// settings. Used by the floating toolbar and by the Stage edit bar (R-053, edge-docked state).
    pub fn contents(self, ui: &mut Ui) -> (ZoomAction, Option<Response>) {
        let mut action = ZoomAction::None;
        let mut settings = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = ZOOM_GAP;
            let b = |ui: &mut Ui, icon: Icon, name: &str, key: Option<&str>, on: bool| {
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
            if super::display::tooltip(
                ui.add(super::Button::ghost(&pct).width(ZOOM_PERCENT_CELL)),
                "Reset to 100 %",
                Some("⌘1"),
            )
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
        (action, settings)
    }

    /// Width of [`Self::contents`] with all four view toggles: nine icon buttons, the percentage
    /// and three dividers, with a gap between each of the thirteen items.
    pub fn row_width() -> f32 {
        9.0 * crate::metrics::control()
            + ZOOM_PERCENT_CELL
            + 3.0 * zoom_divider_width()
            + 12.0 * ZOOM_GAP
    }

    /// Width of the collapsed pill: ‹ and the zoom percentage.
    pub fn pill_width() -> f32 {
        crate::metrics::control() + ZOOM_GAP + ZOOM_PERCENT_CELL
    }

    /// Width of the controls that slide out to the left of the pill: nine icon buttons and two
    /// dividers, each followed by a gap.
    fn pill_extra_width() -> f32 {
        9.0 * crate::metrics::control() + 2.0 * zoom_divider_width() + 11.0 * ZOOM_GAP
    }

    /// Collapsed state (R-053): a pill with ‹ and the zoom percentage, pinned by its right-bottom
    /// corner at `corner`. While `expanded`, the remaining controls are shown to its left in the
    /// same surface. The surface rectangle is computed from metrics and laid out from the right,
    /// so the pill keeps its exact position in both states and on the frame the state changes.
    pub fn show_pill(
        &self,
        ctx: &egui::Context,
        id: egui::Id,
        corner: egui::Pos2,
        expanded: bool,
    ) -> ZoomPillResponse {
        // The floating bar's own frame: the pill is `zoom_control()` high like the full toolbar.
        let frame = Self::bar_frame(ctx);
        let width = Self::pill_width()
            + if expanded {
                Self::pill_extra_width()
            } else {
                0.0
            };
        let content = egui::vec2(width, crate::metrics::control());
        let outer = content + frame.total_margin().sum();
        let mut action = ZoomAction::None;
        let mut settings = None;
        let mut expand = false;
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(corner - outer)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    let (row, _) = ui.allocate_exact_size(content, Sense::hover());
                    let mut ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(row)
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    let ui = &mut ui;
                    ui.spacing_mut().item_spacing.x = ZOOM_GAP;
                    let button = |ui: &mut Ui, icon, name: &str, key: &str, on: bool| {
                        ui.add(
                            super::IconButton::new(icon, name)
                                .shortcut(key)
                                .selected(on),
                        )
                        .clicked()
                    };
                    // Right to left: the pill first, so it is anchored to the fixed right edge.
                    let percent = format!("{:.0}%", self.zoom * 100.0);
                    if super::display::tooltip(
                        ui.add(super::Button::ghost(&percent).width(ZOOM_PERCENT_CELL)),
                        "Reset to 100 %",
                        Some("⌘1"),
                    )
                    .clicked()
                    {
                        action = ZoomAction::ZoomReset;
                    }
                    expand = ui
                        .add(super::IconButton::new(i16::CHEVLEFT, "Show stage toolbar"))
                        .clicked();
                    if !expanded {
                        return;
                    }
                    if button(ui, i16::PLUS, "Zoom in", "⌘=", false) {
                        action = ZoomAction::ZoomIn;
                    }
                    if button(ui, i16::MINUS, "Zoom out", "⌘−", false) {
                        action = ZoomAction::ZoomOut;
                    }
                    super::display::divider(ui, true);
                    let view_settings = ui.add(super::IconButton::new(
                        i16::VIEW_SETTINGS,
                        "Rulers, guides and grid…",
                    ));
                    if view_settings.clicked() {
                        action = ZoomAction::ViewSettings;
                    }
                    settings = Some(view_settings);
                    for (state, icon, name, key, toggle) in [
                        (
                            self.snap,
                            i16::SNAP_GUIDES,
                            "Snap to guides",
                            "⇧⌘;",
                            ZoomAction::Snap,
                        ),
                        (self.grid, i16::GRID, "Grid", "⌘'", ZoomAction::Grid),
                        (self.guides, i16::GUIDES, "Guides", "⌘;", ZoomAction::Guides),
                        (
                            self.rulers,
                            i16::RULER,
                            "Rulers",
                            "⌥⇧⌘R",
                            ZoomAction::Rulers,
                        ),
                    ] {
                        if button(ui, icon, name, key, state.unwrap_or(false)) {
                            action = toggle;
                        }
                    }
                    if button(ui, i16::FIT, "Fit stage in window", "⌘0", false) {
                        action = ZoomAction::Fit;
                    }
                    super::display::divider(ui, true);
                    if button(ui, i16::HAND, "Hand", "H", self.hand) {
                        action = ZoomAction::Hand;
                    }
                });
            });
        ZoomPillResponse {
            action,
            settings,
            expand,
            area: area.response,
        }
    }
}

/// What [`ZoomControl::show_pill`] reports: the clicked action, the view-settings button (only while
/// expanded), whether ‹ was clicked, and the area response (hover, context menu).
pub struct ZoomPillResponse {
    pub action: ZoomAction,
    pub settings: Option<Response>,
    pub expand: bool,
    pub area: Response,
}

/// `NF2/Dock Zone` — accent-tinted drop target with "Dock here".
pub fn dock_zone(painter: &egui::Painter, ctx: &egui::Context, rect: Rect, label: &str) {
    let p = palette(ctx);
    painter.rect_filled(rect, cr(radius::lg()), p.accent_subtle.gamma_multiply(0.85));
    painter.rect_stroke(
        rect,
        cr(radius::lg()),
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
            cr(radius::md()),
            if on { p.accent_default } else { p.bg_raised },
        );
        painter.rect_stroke(
            *r,
            cr(radius::md()),
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
    let line = if active {
        p.accent_default
    } else {
        p.border_subtle
    };
    // The 24 × 3 grip is always visible (R-054: a splitter you can find), accent while active.
    let grip = if active {
        p.accent_default
    } else {
        p.border_strong
    };
    if vertical {
        painter.vline(rect.center().x, rect.y_range(), Stroke::new(1.0_f32, line));
        painter.rect_filled(
            Rect::from_center_size(rect.center(), egui::vec2(3.0, 24.0)),
            cr(1.5),
            grip,
        );
    } else {
        painter.hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, line));
        painter.rect_filled(
            Rect::from_center_size(rect.center(), egui::vec2(24.0, 3.0)),
            cr(1.5),
            grip,
        );
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
                .corner_radius(cr(radius::md()))
                .inner_margin(egui::Margin::symmetric(
                    space::s3() as i8,
                    space::s1() as i8,
                ))
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

/// Normal-density height of the window regions; layout uses `metrics::top_bar()` / `status_bar()`.
pub const TOP_BAR: f32 = crate::metrics::TOP_BAR;
pub const STATUS_BAR: f32 = crate::metrics::STATUS_BAR;
