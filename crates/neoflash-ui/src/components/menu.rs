//! `NF2/Menu Item`, `NF2/Menu Section`, `NF2/Menu Separator`, `NF2/Menu Bar Item` and popup helpers.
//!
//! Menus are `bg/raised` popovers (`radius::lg()`, `space::s1()` inner margin, popover shadow), at
//! least `menu_width()` wide. Rows are `row()` high: 16 px check column + `space::s2()`, label
//! (`NF2/Body`), shortcut right-aligned (`NF2/Small`, `text/tertiary`), ▸ for submenus.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::theme::{cr, palette};
use crate::tokens::{TextStyle, radius, space};
use eframe::egui::{self, Rect, Response, Sense, Stroke, Ui, Widget};

/// `NF2/Menu Item`.
#[must_use = "add it with ui.add(…)"]
pub struct MenuItem<'a> {
    label: &'a str,
    shortcut: &'a str,
    checked: bool,
    submenu: bool,
    icon: Option<Icon>,
    danger: bool,
    highlighted: bool,
}

impl<'a> MenuItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            shortcut: "",
            checked: false,
            submenu: false,
            icon: None,
            danger: false,
            highlighted: false,
        }
    }
    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = shortcut;
        self
    }
    /// `State=Checked` — toggles and the current choice of a select.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
    /// `Submenu` boolean — shows ▸ (the row opens a nested menu).
    pub fn submenu(mut self, submenu: bool) -> Self {
        self.submenu = submenu;
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Keyboard-highlighted row of a menu driven by ↑↓ (quick-fix menu, R-033): drawn like hover
    /// (`bg/hover`) without the pointer. The caller owns the highlighted index and handles ↩ / Esc.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }
    /// Destructive command ("Delete layer"). Red only for destruction.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

impl Widget for MenuItem<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = palette(ui.ctx());
        let label_w = galley(ui, self.label, TextStyle::Body, p.text_primary)
            .size()
            .x;
        let short_w = if self.shortcut.is_empty() {
            0.0
        } else {
            galley(
                ui,
                &super::display::platform_shortcut(self.shortcut),
                TextStyle::Small,
                p.text_tertiary,
            )
            .size()
            .x + space::s5()
        };
        let min_w = label_inset()
            + label_w
            + short_w
            + space::s3()
            + if self.submenu {
                MARK + space::s2()
            } else {
                0.0
            };
        let width = ui.available_width().max(min_w);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, crate::metrics::row()), Sense::click());
        let s = State::of(ui, &response);
        let open = self.submenu && response.hovered();
        // R-101: the pointed-at or keyboard-highlighted row is a neutral `bg/hover` fill; the icon,
        // submenu chevron and shortcut on it rise to `text/primary` so nothing on the row is dim.
        let lit = s.enabled && (s.hovered || open || self.highlighted);
        if lit {
            ui.painter().rect_filled(rect, cr(radius::sm()), p.bg_hover);
        }
        let on = |normal: egui::Color32| if lit { p.text_primary } else { normal };
        let fg = if !s.enabled {
            p.text_disabled
        } else if self.danger {
            p.danger_default
        } else {
            p.text_primary
        };
        // 16 px check / icon column after the row padding.
        let col = Rect::from_center_size(
            egui::pos2(
                rect.left() + space::s2() + CHECK_COLUMN / 2.0,
                rect.center().y,
            ),
            egui::vec2(CHECK_COLUMN, CHECK_COLUMN),
        );
        if self.checked {
            icons::paint_sized(ui.painter(), ui.ctx(), col, i16::CHECK, MARK, fg);
        } else if let Some(icon) = self.icon {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                col,
                icon,
                if s.enabled {
                    on(p.text_secondary)
                } else {
                    p.text_disabled
                },
            );
        }
        text_at(
            ui,
            rect.left() + label_inset(),
            rect,
            galley(ui, self.label, TextStyle::Body, fg),
            fg,
        );
        let mut right = rect.right() - space::s3();
        if self.submenu {
            icons::paint_sized(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(right - MARK / 2.0, rect.center().y),
                    egui::vec2(MARK, MARK),
                ),
                i16::CHEVRIGHT,
                MARK,
                on(p.text_secondary),
            );
            right -= MARK + space::s2();
        }
        if !self.shortcut.is_empty() {
            let c = if s.enabled {
                on(p.text_tertiary)
            } else {
                p.text_disabled
            };
            let g = galley(
                ui,
                &super::display::platform_shortcut(self.shortcut),
                TextStyle::Small,
                c,
            );
            text_at(ui, right - g.size().x, rect, g, c);
        }
        let kind = if self.checked {
            egui::WidgetType::Checkbox
        } else {
            egui::WidgetType::Button
        };
        crate::theme::focus_ring_inside(ui, &response, radius::sm());
        a11y(&response, kind, self.label, self.checked.then_some(true));
        if response.clicked() && !self.submenu {
            ui.close();
        }
        response
    }
}

/// Side of the check / icon column of a [`MenuItem`] and of its 12 px marks.
const CHECK_COLUMN: f32 = 16.0;
const MARK: f32 = 12.0;

/// Left edge of a menu label: row padding, check column, control gap (28 / 24 / 24).
fn label_inset() -> f32 {
    space::s2() + CHECK_COLUMN + space::s2()
}

/// `NF2/Menu Section` — overline caption inside menus and popovers ("RECENT", "SCENES · 3").
pub fn menu_section(ui: &mut Ui, label: &str) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), crate::metrics::menu_section()),
        Sense::hover(),
    );
    text_at(
        ui,
        rect.left() + space::s3(),
        rect,
        galley(ui, label, TextStyle::Overline, p.text_tertiary),
        p.text_tertiary,
    );
}

/// `NF3/Menu Separator` — 1 px `border/subtle` centred in `menu_separator()`, inset by the field padding.
pub fn menu_separator(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), crate::metrics::menu_separator()),
        Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range().shrink(space::s3()),
        rect.center().y,
        Stroke::new(1.0_f32, p.border_subtle),
    );
}

/// Menu-like popup under `anchor` (Select, preset menus, ⋯ panel menus). Returns `Some` while open.
pub fn popup_below<R>(
    anchor: &Response,
    min_width: f32,
    content: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    egui::Popup::menu(anchor)
        .width(min_width)
        .gap(space::s1())
        .frame(popover_frame(&anchor.ctx))
        .show(|ui| {
            menu_style(ui.style_mut());
            ui.set_min_width(min_width);
            content(ui)
        })
        .map(|r| r.inner)
}

/// Context menu (right click) with NF2 styling.
pub fn context_menu<R>(response: &Response, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    egui::Popup::context_menu(response)
        .frame(popover_frame(&response.ctx))
        .show(|ui| {
            menu_style(ui.style_mut());
            ui.set_min_width(crate::metrics::menu_width());
            content(ui)
        })
        .map(|r| r.inner)
}

/// Frame of menus and popovers: `bg/raised`, 1 px `border/subtle`, radius 8, popover shadow, 4 px margin.
pub fn popover_frame(ctx: &egui::Context) -> egui::Frame {
    let p = palette(ctx);
    egui::Frame::new()
        .fill(p.bg_raised)
        .stroke(Stroke::new(1.0_f32, p.border_subtle))
        .corner_radius(cr(radius::lg()))
        .inner_margin(egui::Margin::same(space::s1() as i8))
        .shadow(crate::metrics::SHADOW_POPOVER.egui())
}

/// Style for menu contents: no extra item spacing, `row()` rows, NF2 text.
pub fn menu_style(style: &mut egui::Style) {
    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
    style.spacing.button_padding = egui::vec2(space::s2(), space::s1());
    style.spacing.interact_size.y = crate::metrics::row();
    style.spacing.menu_margin = egui::Margin::same(space::s1() as i8);
    style.visuals.menu_corner_radius = cr(radius::lg());
}

/// `NF2/Menu Bar Item` — ghost text button in the top bar; `State=Open` while its menu is shown.
/// Must be called inside [`menu_bar`] so hover switches between open menus.
pub struct MenuBarItem;

impl MenuBarItem {
    pub fn show<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let p = palette(ui.ctx());
        let text = crate::typography::rich(label, TextStyle::Body).color(p.text_secondary);
        ui.menu_button(text, |ui| {
            ui.set_min_width(crate::metrics::menu_width());
            content(ui)
        })
        .inner
    }
}

/// The application menu bar (File · Edit · View · Modify · Insert · Control · Window).
/// Uses egui's `MenuBar` for keyboard and hover switching; looks are NF2.
pub fn menu_bar<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui.ctx());
    ui.scope(|ui| {
        let v = ui.visuals_mut();
        v.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.weak_bg_fill = p.bg_hover;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.open.weak_bg_fill = p.bg_hover;
        v.widgets.open.bg_stroke = Stroke::NONE;
        v.widgets.active.weak_bg_fill = p.bg_hover;
        v.widgets.active.bg_stroke = Stroke::NONE;
        ui.spacing_mut().button_padding = egui::vec2(space::s3() - 2.0, space::s1());
        ui.spacing_mut().item_spacing.x = 0.0;
        egui::MenuBar::new()
            .config(egui::containers::menu::MenuConfig::new().style(menu_style))
            .ui(ui, |ui| {
                // The bar itself keeps NF2 Menu Bar Item spacing; `menu_style` only styles dropdowns.
                // Menu titles: field padding on both sides plus one control gap between them.
                ui.spacing_mut().item_spacing.x = space::s2();
                ui.spacing_mut().button_padding = egui::vec2(space::s3(), space::s1());
                ui.spacing_mut().interact_size.y = crate::metrics::row();
                ui.visuals_mut().override_text_color = Some(p.text_secondary);
                content(ui)
            })
            .inner
    })
    .inner
}

/// Nested submenu row (`Submenu=true`) with its own popup to the right.
pub fn submenu<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let response = ui.add(MenuItem::new(label).submenu(true));
    egui::containers::menu::SubMenu::new()
        .show(ui, &response, |ui| {
            ui.set_min_width(crate::metrics::menu_width());
            content(ui)
        })
        .map(|r| r.inner)
}
