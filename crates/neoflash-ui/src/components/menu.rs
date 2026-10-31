//! `NF2/Menu Item`, `NF2/Menu Section`, `NF2/Menu Separator`, `NF2/Menu Bar Item` and popup helpers.
//!
//! Menus are `bg/raised` popovers (radius 8, 4 px inner margin, popover shadow). Rows are 28 px:
//! 24 px check column, label (`NF2/Body`), shortcut right-aligned (`NF2/Small`, `text/tertiary`), ▸ for submenus.
use super::{State, a11y, galley, text_at};
use crate::icons::{self, Icon, i16};
use crate::metrics::{MENU_SECTION, MENU_SEPARATOR, ROW};
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
            galley(ui, self.shortcut, TextStyle::Small, p.text_tertiary)
                .size()
                .x
                + space::S5
        };
        let min_w = 24.0 + label_w + short_w + space::S3 + if self.submenu { 16.0 } else { 0.0 };
        let width = ui.available_width().max(min_w);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW), Sense::click());
        let s = State::of(ui, &response);
        let open = self.submenu && response.hovered();
        if s.enabled && (s.hovered || open) {
            ui.painter().rect_filled(rect, cr(radius::MD), p.bg_hover);
        }
        let fg = if !s.enabled {
            p.text_disabled
        } else if self.danger {
            p.danger_default
        } else {
            p.text_primary
        };
        let col = Rect::from_min_size(rect.min, egui::vec2(24.0, ROW));
        if self.checked {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                col.translate(egui::vec2(4.0, 0.0)),
                i16::CHECK,
                if s.enabled {
                    p.accent_default
                } else {
                    p.text_disabled
                },
            );
        } else if let Some(icon) = self.icon {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                col.translate(egui::vec2(4.0, 0.0)),
                icon,
                if s.enabled {
                    p.text_secondary
                } else {
                    p.text_disabled
                },
            );
        }
        text_at(
            ui,
            rect.left() + 28.0,
            rect,
            galley(ui, self.label, TextStyle::Body, fg),
            fg,
        );
        let mut right = rect.right() - space::S2;
        if self.submenu {
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(right - 8.0, rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                i16::CHEVRIGHT,
                p.text_secondary,
            );
            right -= 16.0;
        }
        if !self.shortcut.is_empty() {
            let c = if s.enabled {
                p.text_tertiary
            } else {
                p.text_disabled
            };
            let g = galley(ui, self.shortcut, TextStyle::Small, c);
            text_at(ui, right - g.size().x, rect, g, c);
        }
        let kind = if self.checked {
            egui::WidgetType::Checkbox
        } else {
            egui::WidgetType::Button
        };
        a11y(&response, kind, self.label, self.checked.then_some(true));
        if response.clicked() && !self.submenu {
            ui.close();
        }
        response
    }
}

/// `NF2/Menu Section` — overline caption inside menus and popovers ("RECENT", "SCENES · 3").
pub fn menu_section(ui: &mut Ui, label: &str) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), MENU_SECTION),
        Sense::hover(),
    );
    text_at(
        ui,
        rect.left() + space::S3,
        rect,
        galley(ui, label, TextStyle::Overline, p.text_tertiary),
        p.text_tertiary,
    );
}

/// `NF2/Menu Separator` — 1 px `border/subtle` with 4 px vertical margins.
pub fn menu_separator(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), MENU_SEPARATOR),
        Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range().shrink(space::S1),
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
        .gap(space::S1)
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
            ui.set_min_width(220.0);
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
        .corner_radius(cr(radius::LG))
        .inner_margin(egui::Margin::same(space::S1 as i8))
        .shadow(crate::metrics::SHADOW_POPOVER.egui())
}

/// Style for menu contents: no extra item spacing, 28 px rows, NF2 text.
pub fn menu_style(style: &mut egui::Style) {
    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
    style.spacing.button_padding = egui::vec2(space::S2, space::S1);
    style.spacing.interact_size.y = ROW;
    style.spacing.menu_margin = egui::Margin::same(space::S1 as i8);
    style.visuals.menu_corner_radius = cr(radius::LG);
}

/// `NF2/Menu Bar Item` — ghost text button in the top bar; `State=Open` while its menu is shown.
/// Must be called inside [`menu_bar`] so hover switches between open menus.
pub struct MenuBarItem;

impl MenuBarItem {
    pub fn show<R>(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let p = palette(ui.ctx());
        let text = crate::typography::rich(label, TextStyle::Body).color(p.text_secondary);
        ui.menu_button(text, |ui| {
            ui.set_min_width(240.0);
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
        ui.spacing_mut().button_padding = egui::vec2(space::S3 - 2.0, space::S1);
        ui.spacing_mut().item_spacing.x = 0.0;
        egui::MenuBar::new()
            .config(egui::containers::menu::MenuConfig::new().style(menu_style))
            .ui(ui, |ui| {
                // The bar itself keeps NF2 Menu Bar Item spacing; `menu_style` only styles dropdowns.
                // Figma top bar: ~30 pt between menu titles = 2 × 12 padding + 6 spacing.
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.spacing_mut().button_padding = egui::vec2(space::S3, space::S1);
                ui.spacing_mut().interact_size.y = ROW;
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
            ui.set_min_width(220.0);
            content(ui)
        })
        .map(|r| r.inner)
}
