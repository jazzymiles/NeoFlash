//! NF2 tokens generated from `interface/design-system/tokens/tokens.json`.
//!
//! * [`Palette`] has one field per colour variable: `color/bg/panel` → `bg_panel`,
//!   `color/timeline/classic-tween` → `timeline_classic_tween`, `color/text/on-accent` → `text_on_accent`.
//! * [`DARK`] is the default theme, [`LIGHT`] swaps values only. Components never branch on theme.
//! * `space::S1..S6` = 4 · 8 · 12 · 16 · 24 · 32, `radius::{SM, MD, LG, XL, FULL}` = 4 · 6 · 8 · 12 · 999.
use eframe::egui::Color32;

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

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
