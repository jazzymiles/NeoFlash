//! NF2 theme installation. Dark is the default; every theme swaps values only.
//!
//! egui exposes only Light and Dark style slots. NF2 has several dark-class palettes, so the exact
//! active [`Theme`] is also stored in the context. Plain egui widgets use the selected slot while
//! hand-painted components read the full palette with [`palette`] and never compare themes.
use crate::metrics::{self, SHADOW_DIALOG, SHADOW_POPOVER};
use crate::tokens::{DARK, DARK_GRAPHITE, LIGHT, MID_NEUTRAL_GRAY, Palette, radius, space};
use crate::typography;
use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    MidNeutralGray,
    DarkGraphite,
}

impl Theme {
    pub const ALL: [Self; 4] = [
        Self::Dark,
        Self::Light,
        Self::MidNeutralGray,
        Self::DarkGraphite,
    ];

    pub const fn palette(self) -> &'static Palette {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
            Self::MidNeutralGray => &MID_NEUTRAL_GRAY,
            Self::DarkGraphite => &DARK_GRAPHITE,
        }
    }

    pub const fn is_dark(self) -> bool {
        !matches!(self, Self::Light)
    }

    /// Parse a stable public theme slug. Hyphens are canonical; underscore aliases keep older
    /// automation and persisted tooling compatible.
    pub fn from_slug(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("dark") {
            Some(Self::Dark)
        } else if value.eq_ignore_ascii_case("light") {
            Some(Self::Light)
        } else if value.eq_ignore_ascii_case("mid-neutral-gray")
            || value.eq_ignore_ascii_case("mid_neutral_gray")
        {
            Some(Self::MidNeutralGray)
        } else if value.eq_ignore_ascii_case("dark-graphite")
            || value.eq_ignore_ascii_case("dark_graphite")
        {
            Some(Self::DarkGraphite)
        } else {
            None
        }
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::MidNeutralGray => "mid-neutral-gray",
            Self::DarkGraphite => "dark-graphite",
        }
    }

    pub fn current(ctx: &egui::Context) -> Self {
        let selected = ctx.data(|data| data.get_temp::<Self>(active_theme_id()));
        if let Some(selected) = selected
            && selected.egui() == ctx.theme()
        {
            return selected;
        }
        match ctx.theme() {
            egui::Theme::Dark => Self::Dark,
            egui::Theme::Light => Self::Light,
        }
    }

    const fn egui(self) -> egui::Theme {
        match self {
            Self::Light => egui::Theme::Light,
            Self::Dark | Self::MidNeutralGray | Self::DarkGraphite => egui::Theme::Dark,
        }
    }
}

fn active_theme_id() -> egui::Id {
    egui::Id::new("nf2-active-theme")
}

/// Current full NF2 palette for a context.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    Theme::current(ctx).palette()
}

pub(crate) fn cr(r: f32) -> CornerRadius {
    CornerRadius::same(r.min(255.0) as u8)
}

/// Install NF2 spacing, text styles and both egui theme-slot visuals, then switch to `theme`.
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
    // Keep both built-in slots valid. The dark slot receives the exact selected dark-class NF2
    // palette; switching back to canonical Dark replaces it again without disturbing other style.
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals = visuals(Theme::Light)
    });
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals = visuals(if theme.is_dark() { theme } else { Theme::Dark })
    });
    ctx.data_mut(|data| data.insert_temp(active_theme_id(), theme));
    ctx.set_theme(theme.egui());
    ctx.request_repaint();
}

/// NF3 spacing for egui's built-in widgets at the current density, plus the density-dependent corner
/// radii of the style's visuals. Call again (on every style slot) after `metrics::set_density`
/// changes the profile; colours are untouched.
pub fn spacing(style: &mut egui::Style) {
    density_radii(&mut style.visuals);
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(space::s2(), metrics::row_gap());
    s.button_padding = egui::vec2(space::s3() + 2.0, space::s1());
    s.interact_size = egui::vec2(metrics::control(), metrics::control());
    s.icon_width = metrics::ICON_UI;
    s.icon_width_inner = 8.0;
    s.icon_spacing = space::s2();
    s.indent = metrics::LAYER_INDENT;
    s.menu_margin = Margin::same(space::s1() as i8);
    s.window_margin = Margin::same(space::s3() as i8);
    s.combo_width = metrics::field_m();
    s.text_edit_width = metrics::field_m();
    s.slider_width = metrics::field_m();
    s.slider_rail_height = 2.0;
    s.tooltip_width = 320.0;
    // NF2/Scrollbar (Technical shelf): a thin 4 px thumb floating over content at rest, growing to an
    // 8 px thumb on a track while hovered or dragged. No reserved gutter, as in the Figma panels.
    s.scroll.floating = true;
    // Keep the whole bar inside the narrowest panel gutter (`space::s3()`: 8 / 6 / 5 pt) so it
    // never overlaps the right border of a field: the 4 pt thumb is centred in it, and the
    // hovered thumb grows only as far as the gutter allows.
    let gutter = space::s3();
    let floating = 4.0;
    let outer = ((gutter - floating) / 2.0).max(0.0);
    s.scroll.bar_width = (gutter - outer).clamp(floating, 6.0);
    s.scroll.floating_width = floating;
    s.scroll.floating_allocated_width = 0.0;
    s.scroll.bar_inner_margin = 0.0;
    s.scroll.bar_outer_margin = outer;
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

/// Corner radii of egui's own windows, menus and widgets for the current density.
fn density_radii(v: &mut egui::Visuals) {
    v.window_corner_radius = cr(radius::xl());
    v.menu_corner_radius = cr(radius::lg());
    let w = &mut v.widgets;
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = cr(radius::md());
    }
}

pub fn visuals(theme: Theme) -> egui::Visuals {
    let p = theme.palette();
    let mut v = if theme.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = None;
    v.weak_text_color = Some(p.text_tertiary);
    v.panel_fill = p.bg_panel;
    v.window_fill = p.bg_raised;
    v.window_stroke = Stroke::new(1.0_f32, p.border_subtle);
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
        s.expansion = 0.0;
    }
    density_radii(&mut v);
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

/// Reveal a newly focused row or chip without changing a timeline's horizontal position.
pub(crate) fn reveal_focus_vertically(ui: &egui::Ui, response: &egui::Response) {
    let clip = ui.clip_rect();
    if response.gained_focus()
        && response.rect.height() <= clip.height()
        && (response.rect.top() < clip.top() || response.rect.bottom() > clip.bottom())
    {
        let x = clip.center().x;
        ui.scroll_to_rect(
            egui::Rect::from_x_y_ranges(x..=x, response.rect.y_range()),
            None,
        );
    }
}

/// Paint the standard keyboard ring inside a control that touches neighbours or a scroll clip.
pub fn focus_ring_inside(ui: &egui::Ui, response: &egui::Response, radius: f32) {
    if !response.has_focus() || !keyboard_focus(ui, response) {
        return;
    }
    reveal_focus_vertically(ui, response);
    let visible = response.interact_rect.intersect(ui.clip_rect());
    if visible.is_positive() {
        let inset = 2.0 * (metrics::FOCUS_OFFSET + metrics::FOCUS_RING);
        let size = (visible.size() - egui::Vec2::splat(inset)).max(egui::Vec2::ZERO);
        focus_ring(
            ui,
            egui::Rect::from_center_size(visible.center(), size),
            radius,
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_nf2_theme_keeps_its_full_identity_and_palette() {
        let ctx = egui::Context::default();
        assert_eq!(MID_NEUTRAL_GRAY.bg_app, Color32::from_rgb(0x50, 0x50, 0x50));
        assert_ne!(MID_NEUTRAL_GRAY, DARK_GRAPHITE);
        for theme in Theme::ALL {
            install(&ctx, theme);
            assert_eq!(Theme::current(&ctx), theme);
            assert_eq!(palette(&ctx), theme.palette());
            assert_eq!(ctx.style().visuals.dark_mode, theme.is_dark());
            assert_eq!(ctx.style().visuals.panel_fill, theme.palette().bg_panel);
            let p = theme.palette();
            assert_ne!(p.syntax_identifier, p.text_primary);
            assert_ne!(p.syntax_identifier, p.text_secondary);
            assert_ne!(p.syntax_identifier, p.syntax_type);
            assert_ne!(p.syntax_identifier, p.syntax_function);
            assert_ne!(p.syntax_identifier, p.syntax_keyword);
        }
    }

    #[test]
    fn respacing_after_a_density_change_updates_sizes_and_radii_in_both_slots() {
        let ctx = egui::Context::default();
        install(&ctx, Theme::Light);
        for density in metrics::Density::ALL {
            metrics::set_density(density);
            ctx.all_styles_mut(spacing);
            for slot in [egui::Theme::Light, egui::Theme::Dark] {
                let style = ctx.style_of(slot);
                assert_eq!(style.spacing.interact_size.y, metrics::control());
                assert_eq!(
                    style.spacing.item_spacing,
                    egui::vec2(space::s2(), metrics::row_gap())
                );
                assert_eq!(style.spacing.combo_width, metrics::field_m());
                assert_eq!(style.spacing.indent, metrics::LAYER_INDENT);
                assert_eq!(style.visuals.window_corner_radius, cr(radius::xl()));
                assert_eq!(style.visuals.menu_corner_radius, cr(radius::lg()));
                assert_eq!(
                    style.visuals.widgets.inactive.corner_radius,
                    cr(radius::md())
                );
            }
            // A theme switch keeps the density radii.
            apply_visuals(&ctx, Theme::Dark);
            assert_eq!(ctx.style().visuals.window_corner_radius, cr(radius::xl()));
            assert_eq!(ctx.style().visuals.panel_fill, DARK.bg_panel);
            apply_visuals(&ctx, Theme::Light);
        }
        metrics::set_density(metrics::Density::Comfortable);
    }

    #[test]
    fn a_direct_egui_slot_change_cannot_report_the_wrong_lightness() {
        let ctx = egui::Context::default();
        install(&ctx, Theme::MidNeutralGray);
        ctx.set_theme(egui::Theme::Light);
        assert_eq!(Theme::current(&ctx), Theme::Light);
        assert_eq!(palette(&ctx), &LIGHT);
    }

    #[test]
    fn public_theme_slugs_cover_all_four_palettes_and_legacy_aliases() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_slug(theme.slug()), Some(theme));
        }
        assert_eq!(
            Theme::from_slug("MID_NEUTRAL_GRAY"),
            Some(Theme::MidNeutralGray)
        );
        assert_eq!(Theme::from_slug("dark_graphite"), Some(Theme::DarkGraphite));
        assert_eq!(Theme::from_slug("unknown"), None);
    }
}
