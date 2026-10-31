//! Dark / Light theme installation. Dark is the NF2 default; Light swaps values only.
//!
//! [`install`] writes the palette into egui's `Visuals` for **both** egui themes and selects one, so
//! plain egui widgets that panels still use (scroll areas, text edits, combo popups) already look NF2.
//! Hand-painted components read colours with [`palette`] and never compare themes.
use crate::metrics::{self, SHADOW_DIALOG, SHADOW_POPOVER};
use crate::tokens::{DARK, LIGHT, Palette, radius, space};
use crate::typography;
use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub const fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
        }
    }
    pub fn current(ctx: &egui::Context) -> Self {
        match ctx.theme() {
            egui::Theme::Dark => Self::Dark,
            egui::Theme::Light => Self::Light,
        }
    }
    const fn egui(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }
}

/// Current palette for a context (follows `ctx.theme()`).
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    Theme::current(ctx).palette()
}

pub(crate) fn cr(r: f32) -> CornerRadius {
    CornerRadius::same(r.min(255.0) as u8)
}

/// Install NF2 spacing, text styles and both themes' visuals, then switch to `theme`.
/// Call once at start-up; use [`apply_visuals`] for later theme switches.
pub fn install(ctx: &egui::Context, theme: Theme) {
    ctx.all_styles_mut(|style| {
        spacing(style);
        typography::install_text_styles(style);
    });
    apply_visuals(ctx, theme);
}

/// Switch theme: replaces only colours, so customised spacing / typography, widget memory and fonts
/// are untouched.
pub fn apply_visuals(ctx: &egui::Context, theme: Theme) {
    for t in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(t.egui(), |style| style.visuals = visuals(t));
    }
    ctx.set_theme(theme.egui());
    ctx.request_repaint();
}

/// NF2 spacing for egui's built-in widgets (§3.2).
pub fn spacing(style: &mut egui::Style) {
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(space::S2, space::S2);
    s.button_padding = egui::vec2(space::S3, space::S1 + 2.0);
    s.interact_size = egui::vec2(metrics::CONTROL, metrics::CONTROL);
    s.icon_width = metrics::ICON_UI;
    s.icon_width_inner = 8.0;
    s.icon_spacing = space::S2;
    s.indent = metrics::LAYER_INDENT;
    s.menu_margin = Margin::same(space::S1 as i8);
    s.window_margin = Margin::same(space::S3 as i8);
    s.combo_width = 160.0;
    s.text_edit_width = 160.0;
    s.slider_width = 140.0;
    s.slider_rail_height = 4.0;
    s.tooltip_width = 320.0;
    // NF2/Scrollbar (Technical shelf): a thin 4 px thumb floating over content at rest, growing to an
    // 8 px thumb on a track while hovered or dragged. No reserved gutter, as in the Figma panels.
    s.scroll.floating = true;
    // Keep the whole bar inside a 12–16 pt panel gutter so it never overlaps fields.
    s.scroll.bar_width = 6.0;
    s.scroll.floating_width = 4.0;
    s.scroll.floating_allocated_width = 0.0;
    s.scroll.bar_inner_margin = 0.0;
    s.scroll.bar_outer_margin = 3.0;
    s.scroll.handle_min_length = 24.0;
    s.scroll.foreground_color = false;
    s.scroll.dormant_background_opacity = 0.0;
    s.scroll.active_background_opacity = 1.0;
    s.scroll.interact_background_opacity = 1.0;
    s.scroll.dormant_handle_opacity = 1.0;
    s.scroll.active_handle_opacity = 1.0;
    s.scroll.interact_handle_opacity = 1.0;
    style.interaction.tooltip_delay = 0.4;
}

pub fn visuals(theme: Theme) -> egui::Visuals {
    let p = theme.palette();
    let mut v = match theme {
        Theme::Dark => egui::Visuals::dark(),
        Theme::Light => egui::Visuals::light(),
    };
    v.override_text_color = None;
    v.weak_text_color = Some(p.text_tertiary);
    v.panel_fill = p.bg_panel;
    v.window_fill = p.bg_raised;
    v.window_stroke = Stroke::new(1.0_f32, p.border_subtle);
    v.window_corner_radius = cr(radius::XL);
    v.menu_corner_radius = cr(radius::LG);
    v.window_shadow = SHADOW_DIALOG.egui();
    v.popup_shadow = SHADOW_POPOVER.egui();
    v.extreme_bg_color = p.bg_field;
    v.text_edit_bg_color = Some(p.bg_field);
    v.code_bg_color = p.bg_field;
    v.faint_bg_color = p.bg_hover;
    v.selection.bg_fill = p.syntax_selection;
    v.selection.stroke = Stroke::new(1.0_f32, p.accent_default);
    v.hyperlink_color = p.accent_default;
    v.warn_fg_color = p.warning_default;
    v.error_fg_color = p.danger_default;
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.accent_default);
    v.disabled_alpha = 1.0;
    v.interact_cursor = None;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.resize_corner_size = 8.0;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg_panel;
    w.noninteractive.weak_bg_fill = p.bg_panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_secondary);
    w.noninteractive.corner_radius = cr(radius::MD);
    w.inactive.bg_fill = p.bg_field;
    w.inactive.weak_bg_fill = p.bg_raised;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border_default);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    w.hovered.bg_fill = p.bg_hover;
    w.hovered.weak_bg_fill = p.bg_hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    w.active.bg_fill = p.bg_selected;
    w.active.weak_bg_fill = p.bg_selected;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent_default);
    w.active.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    w.open.bg_fill = p.bg_hover;
    w.open.weak_bg_fill = p.bg_hover;
    w.open.bg_stroke = Stroke::new(1.0_f32, p.border_default);
    w.open.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = cr(radius::MD);
        s.expansion = 0.0;
    }
    v
}

/// Paint the NF2 keyboard focus ring (`NF2/Focus Ring`: 2 px, 2 px outside the control radius).
/// Call only when `response.has_focus()` came from the keyboard (egui shows focus only after Tab).
pub fn focus_ring(ui: &egui::Ui, rect: egui::Rect, control_radius: f32) {
    let p = palette(ui.ctx());
    ui.painter().rect_stroke(
        rect.expand(metrics::FOCUS_OFFSET),
        cr(control_radius + metrics::FOCUS_OFFSET),
        Stroke::new(metrics::FOCUS_RING, p.border_focus),
        egui::StrokeKind::Outside,
    );
}

/// `true` when focus should be drawn: the widget has focus and the user navigates by keyboard.
/// Keyboard mode starts with Tab / arrow navigation and ends with any pointer press.
pub fn keyboard_focus(ui: &egui::Ui, response: &egui::Response) -> bool {
    let id = egui::Id::new("nf2-keyboard-navigation");
    let (tab, pointer) = ui.input(|i| (i.key_pressed(egui::Key::Tab), i.pointer.any_pressed()));
    let mut keyboard = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if tab {
        keyboard = true;
    }
    if pointer {
        keyboard = false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, keyboard));
    keyboard && response.has_focus()
}

/// Translucent black used for the modal scrim if a caller needs a non-token shade.
pub fn black(alpha: f32) -> Color32 {
    Color32::from_black_alpha((alpha * 255.0) as u8)
}
