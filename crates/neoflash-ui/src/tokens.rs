//! NF2 tokens generated from `interface/design-system/tokens/tokens.json`.
//!
//! * [`Palette`] has one field per colour variable: `color/bg/panel` → `bg_panel`,
//!   `color/timeline/classic-tween` → `timeline_classic_tween`, `color/text/on-accent` → `text_on_accent`.
//! * [`DARK`] is the default theme; [`LIGHT`], [`MID_NEUTRAL_GRAY`] and [`DARK_GRAPHITE`] swap
//!   semantic values only. Components never branch on theme.
//! * `space::S1..S6` = 4 · 8 · 12 · 16 · 24 · 32, `radius::{SM, MD, LG, XL, FULL}` = 4 · 6 · 8 · 12 · 999.
use eframe::egui::Color32;

mod generated {
    use super::{Family, TextSpec};
    use eframe::egui::Color32;

    include!(concat!(env!("OUT_DIR"), "/tokens.rs"));
}

pub use generated::{TextStyle, radius, space};

/// Every NF2 colour token (`color/<group>/<name>`) plus `syntax_identifier` (`T-375`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_raised: Color32,
    pub bg_field: Color32,
    pub bg_hover: Color32,
    pub bg_selected: Color32,
    pub bg_canvas: Color32,
    pub bg_stage: Color32,
    pub border_subtle: Color32,
    pub border_default: Color32,
    pub border_strong: Color32,
    pub border_focus: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub text_disabled: Color32,
    pub text_on_accent: Color32,
    pub text_inverse: Color32,
    pub accent_default: Color32,
    pub accent_hover: Color32,
    pub accent_subtle: Color32,
    pub brand_orange: Color32,
    pub danger_default: Color32,
    pub danger_subtle: Color32,
    pub success_default: Color32,
    pub warning_default: Color32,
    pub warning_subtle: Color32,
    pub layer_green: Color32,
    pub layer_red: Color32,
    pub layer_blue: Color32,
    pub timeline_cell: Color32,
    pub timeline_cell_alt: Color32,
    pub timeline_line: Color32,
    pub timeline_row_selected: Color32,
    pub timeline_playhead: Color32,
    pub timeline_empty: Color32,
    pub timeline_empty_5th: Color32,
    pub timeline_grid: Color32,
    pub timeline_static: Color32,
    pub timeline_classic_tween: Color32,
    pub timeline_shape_tween: Color32,
    pub timeline_motion_tween: Color32,
    pub timeline_selected: Color32,
    pub timeline_marker: Color32,
    pub timeline_marker_inverse: Color32,
    pub timeline_label: Color32,
    pub timeline_comment: Color32,
    pub timeline_folder: Color32,
    pub timeline_onion: Color32,
    pub syntax_function: Color32,
    pub syntax_identifier: Color32,
    pub syntax_string: Color32,
    pub syntax_keyword: Color32,
    pub syntax_type: Color32,
    pub syntax_comment: Color32,
    pub syntax_number: Color32,
    pub syntax_tag: Color32,
    pub syntax_attr: Color32,
    pub syntax_error: Color32,
    pub syntax_warning: Color32,
    pub syntax_breakpoint: Color32,
    pub syntax_selection: Color32,
    pub syntax_current_line: Color32,
    pub overlay: Color32,
    pub tooltip_bg: Color32,
    pub guide: Color32,
    pub guide_selected: Color32,
    pub stage_grid: Color32,
}

const fn extend_palette(g: generated::Palette, syntax_identifier: Color32) -> Palette {
    let generated::Palette {
        bg_app,
        bg_panel,
        bg_raised,
        bg_field,
        bg_hover,
        bg_selected,
        bg_canvas,
        bg_stage,
        border_subtle,
        border_default,
        border_strong,
        border_focus,
        text_primary,
        text_secondary,
        text_tertiary,
        text_disabled,
        text_on_accent,
        text_inverse,
        accent_default,
        accent_hover,
        accent_subtle,
        brand_orange,
        danger_default,
        danger_subtle,
        success_default,
        warning_default,
        warning_subtle,
        layer_green,
        layer_red,
        layer_blue,
        timeline_cell,
        timeline_cell_alt,
        timeline_line,
        timeline_row_selected,
        timeline_playhead,
        timeline_empty,
        timeline_empty_5th,
        timeline_grid,
        timeline_static,
        timeline_classic_tween,
        timeline_shape_tween,
        timeline_motion_tween,
        timeline_selected,
        timeline_marker,
        timeline_marker_inverse,
        timeline_label,
        timeline_comment,
        timeline_folder,
        timeline_onion,
        syntax_function,
        syntax_string,
        syntax_keyword,
        syntax_type,
        syntax_comment,
        syntax_number,
        syntax_tag,
        syntax_attr,
        syntax_error,
        syntax_warning,
        syntax_breakpoint,
        syntax_selection,
        syntax_current_line,
        overlay,
        tooltip_bg,
        guide,
        guide_selected,
        stage_grid,
    } = g;
    Palette {
        bg_app,
        bg_panel,
        bg_raised,
        bg_field,
        bg_hover,
        bg_selected,
        bg_canvas,
        bg_stage,
        border_subtle,
        border_default,
        border_strong,
        border_focus,
        text_primary,
        text_secondary,
        text_tertiary,
        text_disabled,
        text_on_accent,
        text_inverse,
        accent_default,
        accent_hover,
        accent_subtle,
        brand_orange,
        danger_default,
        danger_subtle,
        success_default,
        warning_default,
        warning_subtle,
        layer_green,
        layer_red,
        layer_blue,
        timeline_cell,
        timeline_cell_alt,
        timeline_line,
        timeline_row_selected,
        timeline_playhead,
        timeline_empty,
        timeline_empty_5th,
        timeline_grid,
        timeline_static,
        timeline_classic_tween,
        timeline_shape_tween,
        timeline_motion_tween,
        timeline_selected,
        timeline_marker,
        timeline_marker_inverse,
        timeline_label,
        timeline_comment,
        timeline_folder,
        timeline_onion,
        syntax_function,
        syntax_identifier,
        syntax_string,
        syntax_keyword,
        syntax_type,
        syntax_comment,
        syntax_number,
        syntax_tag,
        syntax_attr,
        syntax_error,
        syntax_warning,
        syntax_breakpoint,
        syntax_selection,
        syntax_current_line,
        overlay,
        tooltip_bg,
        guide,
        guide_selected,
        stage_grid,
    }
}

pub const DARK: Palette = extend_palette(generated::DARK, Color32::from_rgb(0x9C, 0xDC, 0xFE));
pub const LIGHT: Palette = extend_palette(generated::LIGHT, Color32::from_rgb(0x1E, 0x3A, 0x8A));
pub const MID_NEUTRAL_GRAY: Palette = extend_palette(
    generated::MID_NEUTRAL_GRAY,
    Color32::from_rgb(0x24, 0x3F, 0x80),
);
pub const DARK_GRAPHITE: Palette = extend_palette(
    generated::DARK_GRAPHITE,
    Color32::from_rgb(0x7E, 0xC4, 0xEB),
);

/// Font family of a text style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Inter (UI).
    Inter,
    /// Roboto Mono (code, timecode, numbers in rulers, hex values).
    Mono,
}

/// Resolved values of one `NF2/*` text style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSpec {
    pub name: &'static str,
    pub family: Family,
    pub weight: u16,
    pub size: f32,
    pub line_height: f32,
    /// Letter spacing in em (Overline = 0.06).
    pub tracking_em: f32,
    pub uppercase: bool,
}

impl Palette {
    /// `color` with an alpha multiplier, for the few translucent tints the spec allows
    /// (hover washes over artwork, out-of-range bands in the Ease graph).
    pub fn tint(color: Color32, alpha: f32) -> Color32 {
        color.gamma_multiply(alpha)
    }
}
