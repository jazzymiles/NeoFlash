#![allow(unused_imports)]
use super::*;
use crate::theme::{ScrollTheme as _, ThemeMode};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline::{
    self as tl, GridKind, LayerColumns, LayerKind, LayerRow, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::icons::{self, Icon, i16};
use neoflash_ui::metrics::Density;
use neoflash_ui::tokens::{Palette, TextStyle, radius, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// 1. Live Sizing in `%` or `px` (`pt`), `Auto`, and `Fill`
// ─────────────────────────────────────────────────────────────────────────────

/// Sizing unit for width, height, and grid column tracks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "snake_case")]
pub enum UiSizeUnit {
    /// Fixed size in logical points / pixels (`px` / `pt`).
    Px(f32),
    /// Percentage (`0.0..=100.0`) of the parent container's content dimension.
    Percent(f32),
    /// Hug intrinsic content size.
    Auto,
    /// Proportional flex / fractional share (`fr`) of remaining available space.
    Fill(f32),
}

impl Default for UiSizeUnit {
    fn default() -> Self {
        Self::Percent(100.0)
    }
}

impl UiSizeUnit {
    /// Resolve this unit into logical points given `parent_size` and `content_size`.
    pub fn resolve(self, parent_size: f32, content_size: f32) -> f32 {
        let parent = parent_size.max(0.0);
        let content = content_size.max(0.0);
        match self {
            Self::Px(px) => px.max(0.0),
            Self::Percent(pct) => (parent * (pct.clamp(0.0, 100.0) / 100.0)).max(0.0),
            Self::Auto => content,
            Self::Fill(weight) => {
                if weight <= 0.0 {
                    0.0
                } else {
                    (parent * weight.min(1.0)).max(content)
                }
            }
        }
    }

    /// Convert the current unit to `UiSizeUnit::Percent` relative to `reference_px`.
    pub fn to_percent(self, reference_px: f32) -> Self {
        let denom = reference_px.max(1.0);
        match self {
            Self::Percent(p) => Self::Percent(p.clamp(0.0, 100.0)),
            Self::Px(px) => Self::Percent(((px / denom) * 100.0).clamp(0.0, 100.0)),
            Self::Auto => Self::Percent(100.0),
            Self::Fill(_) => Self::Percent(100.0),
        }
    }

    /// Convert the current unit to `UiSizeUnit::Px` relative to `reference_px`.
    pub fn to_px(self, reference_px: f32) -> Self {
        let base = reference_px.max(1.0);
        match self {
            Self::Px(px) => Self::Px(px.max(0.0)),
            Self::Percent(pct) => Self::Px((base * (pct.clamp(0.0, 100.0) / 100.0)).round()),
            Self::Auto => Self::Px(base.round()),
            Self::Fill(_) => Self::Px(base.round()),
        }
    }

    /// Numeric scalar for inspector editing.
    pub fn scalar_value(self) -> f64 {
        match self {
            Self::Px(v) | Self::Percent(v) | Self::Fill(v) => f64::from(v),
            Self::Auto => 0.0,
        }
    }

    /// Update scalar while keeping the same unit kind (`Auto` becomes `Px`).
    pub fn with_scalar(self, v: f32) -> Self {
        match self {
            Self::Px(_) | Self::Auto => Self::Px(v.max(0.0)),
            Self::Percent(_) => Self::Percent(v.clamp(0.0, 100.0)),
            Self::Fill(_) => Self::Fill(v.max(0.1)),
        }
    }

    /// Short unit suffix for UI badges and fields.
    pub const fn unit_suffix(self) -> &'static str {
        match self {
            Self::Px(_) => "px",
            Self::Percent(_) => "%",
            Self::Auto => "auto",
            Self::Fill(_) => "fr",
        }
    }

    /// Human-readable compact spec (`"100%"`, `"120px"`, `"auto"`, `"1fr"`).
    pub fn format_compact(self) -> String {
        match self {
            Self::Px(px) => format!("{:.0}px", px),
            Self::Percent(pct) => {
                if (pct - pct.round()).abs() < 0.05 {
                    format!("{:.0}%", pct)
                } else {
                    format!("{:.1}%", pct)
                }
            }
            Self::Auto => "auto".to_owned(),
            Self::Fill(w) => format!("{:.1}fr", w),
        }
    }

    /// Rust codegen literal for this `UiSizeUnit`.
    pub fn to_rust_expr(self) -> String {
        match self {
            Self::Px(px) => format!("UiSizeUnit::Px({:.1})", px),
            Self::Percent(pct) => format!("UiSizeUnit::Percent({:.2})", pct),
            Self::Auto => "UiSizeUnit::Auto".to_owned(),
            Self::Fill(w) => format!("UiSizeUnit::Fill({:.2})", w),
        }
    }
}

/// Box sizing constraints (`width`, `height`, and optional `min`/`max` bounds in `px`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiBoxConstraints {
    pub width: UiSizeUnit,
    pub height: UiSizeUnit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<f32>,
}

impl Default for UiBoxConstraints {
    fn default() -> Self {
        Self {
            width: UiSizeUnit::Percent(100.0),
            height: UiSizeUnit::Auto,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
        }
    }
}

impl UiBoxConstraints {
    pub fn new(width: UiSizeUnit, height: UiSizeUnit) -> Self {
        Self {
            width,
            height,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
        }
    }

    pub fn fixed_px(width: f32, height: f32) -> Self {
        Self::new(UiSizeUnit::Px(width), UiSizeUnit::Px(height))
    }

    pub fn percent_width(pct: f32) -> Self {
        Self::new(UiSizeUnit::Percent(pct), UiSizeUnit::Auto)
    }

    pub fn resolve_width(&self, parent_width: f32, content_width: f32) -> f32 {
        let mut w = self.width.resolve(parent_width, content_width);
        if let Some(min_w) = self.min_width {
            w = w.max(min_w.max(0.0));
        }
        if let Some(max_w) = self.max_width {
            w = w.min(max_w.max(0.0));
        }
        w.max(0.0)
    }

    pub fn resolve_height(&self, parent_height: f32, content_height: f32) -> f32 {
        let mut h = self.height.resolve(parent_height, content_height);
        if let Some(min_h) = self.min_height {
            h = h.max(min_h.max(0.0));
        }
        if let Some(max_h) = self.max_height {
            h = h.min(max_h.max(0.0));
        }
        h.max(0.0)
    }
}

/// Resolve a slice of track definitions (`Row` children or `Grid` columns) across `available_px`
/// with `gap_px` between tracks.
///
/// Supports pure `%` tracks (`[50%, 50%]`, `[33.3%, 33.3%, 33.4%]`, `[25%, 25%, 25%, 25%]`),
/// pure `px` tracks, `Fill(fr)` tracks, and mixed tracks (`[120px, 50%, 50%]`).
pub fn resolve_track_sizes(
    tracks: &[UiSizeUnit],
    available_px: f32,
    gap_px: f32,
    intrinsic_sizes: &[f32],
) -> Vec<f32> {
    if tracks.is_empty() {
        return Vec::new();
    }
    let total_gaps = gap_px.max(0.0) * (tracks.len().saturating_sub(1) as f32);
    let usable = (available_px - total_gaps).max(0.0);

    let mut resolved = vec![0.0_f32; tracks.len()];
    let mut fixed_sum = 0.0_f32;
    let mut pct_sum = 0.0_f32;
    let mut fill_weight_sum = 0.0_f32;

    for (i, track) in tracks.iter().copied().enumerate() {
        let intrinsic = intrinsic_sizes.get(i).copied().unwrap_or(64.0).max(0.0);
        match track {
            UiSizeUnit::Px(px) => {
                let v = px.max(0.0);
                resolved[i] = v;
                fixed_sum += v;
            }
            UiSizeUnit::Auto => {
                resolved[i] = intrinsic;
                fixed_sum += intrinsic;
            }
            UiSizeUnit::Percent(pct) => {
                pct_sum += pct.clamp(0.0, 100.0);
            }
            UiSizeUnit::Fill(w) => {
                fill_weight_sum += w.max(0.0);
            }
        }
    }

    // When fixed tracks are mixed with percentage tracks (e.g. `[120px, 50%, 50%]`) and
    // resolving percentages against full `usable` would overflow, resolve percentages against
    // the remaining space after fixed tracks so `[120px, 50%, 50%]` fits `usable` cleanly.
    let pct_base = if fixed_sum > 0.0 && fixed_sum + usable * (pct_sum / 100.0) > usable + 0.01 {
        (usable - fixed_sum).max(0.0)
    } else {
        usable
    };

    let mut pct_allocated = 0.0_f32;
    for (i, track) in tracks.iter().copied().enumerate() {
        if let UiSizeUnit::Percent(pct) = track {
            let v = (pct_base * (pct.clamp(0.0, 100.0) / 100.0)).max(0.0);
            resolved[i] = v;
            pct_allocated += v;
        }
    }

    if fill_weight_sum > 0.0 {
        let remaining = (usable - fixed_sum - pct_allocated).max(0.0);
        for (i, track) in tracks.iter().copied().enumerate() {
            if let UiSizeUnit::Fill(w) = track {
                resolved[i] = remaining * (w.max(0.0) / fill_weight_sum);
            }
        }
    }

    resolved
}

// ─────────────────────────────────────────────────────────────────────────────
// 1b. Density bridge: every density-driven size comes from `neoflash_ui::metrics`
// ─────────────────────────────────────────────────────────────────────────────

/// `DENSITY_TABLE` row for `name` as `[Normal, Compact, Classic Flash]`.
pub fn density_values(name: &str) -> [f32; 3] {
    neoflash_ui::metrics::DENSITY_TABLE
        .iter()
        .find(|(metric, _)| *metric == name)
        .map(|(_, values)| *values)
        .unwrap_or_else(|| panic!("unknown density metric `{name}`"))
}

/// Column of `DENSITY_TABLE` for a density.
pub const fn density_index(density: Density) -> usize {
    match density {
        Density::Comfortable => 0,
        Density::Compact => 1,
        Density::ClassicFlash => 2,
    }
}

/// The three values of a density metric as shown in the inspector, e.g. `6 · 4 · 4`.
pub fn format_density_values(values: [f32; 3]) -> String {
    format!("{:.0} · {:.0} · {:.0}", values[0], values[1], values[2])
}

/// Whether a stored system size still equals its Normal reference and so follows the density system.
pub fn follows_density(stored: f32, metric: &str) -> bool {
    stored == density_values(metric)[0]
}

/// Effective size of a stored system value: the metric of the current density while the value equals
/// its Normal reference; any other value is a Studio-local override used in every density.
pub fn follow_density(stored: f32, metric: &str) -> f32 {
    let values = density_values(metric);
    if stored == values[0] {
        values[density_index(neoflash_ui::metrics::density())]
    } else {
        stored
    }
}

/// Scope that lays out and paints a preview in `density`; the previous density of the UI thread is
/// restored on drop, so the host editor keeps its own profile.
pub struct PreviewDensity {
    previous: Density,
}

impl PreviewDensity {
    pub fn enter(density: Density) -> Self {
        let previous = neoflash_ui::metrics::density();
        neoflash_ui::metrics::set_density(density);
        Self { previous }
    }

    /// Make egui spacing (interact size, item spacing, paddings) of a preview `Ui` follow the scope.
    pub fn style(ui: &mut Ui) {
        neoflash_ui::theme::spacing(ui.style_mut());
    }
}

impl Drop for PreviewDensity {
    fn drop(&mut self) {
        neoflash_ui::metrics::set_density(self.previous);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Paddings, Gaps, Typography & Visual Styling Tokens
// ─────────────────────────────────────────────────────────────────────────────

/// Step of the density spacing system: `space::s1()…s6()` and `metrics::row_gap()`. `S0` is zero.
/// Every step has one value per density (`DENSITY_TABLE`); JSON stores the step name.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "UPPERCASE")]
pub enum SpacingToken {
    S0,
    S1,
    #[default]
    S2,
    S3,
    S4,
    S5,
    S6,
    /// `metrics::row_gap()`: vertical gap between form rows.
    #[serde(rename = "ROW_GAP")]
    RowGap,
}

impl SpacingToken {
    pub const ALL: [Self; 8] = [
        Self::S0,
        Self::S1,
        Self::S2,
        Self::S3,
        Self::S4,
        Self::S5,
        Self::S6,
        Self::RowGap,
    ];

    /// Name of the step in the inspector.
    pub const fn label(self) -> &'static str {
        match self {
            Self::S0 => "none",
            Self::S1 => "space 1",
            Self::S2 => "space 2",
            Self::S3 => "space 3",
            Self::S4 => "space 4",
            Self::S5 => "space 5",
            Self::S6 => "space 6",
            Self::RowGap => "row gap",
        }
    }

    pub const fn short_name(self) -> &'static str {
        match self {
            Self::S0 => "S0",
            Self::S1 => "S1",
            Self::S2 => "S2",
            Self::S3 => "S3",
            Self::S4 => "S4",
            Self::S5 => "S5",
            Self::S6 => "S6",
            Self::RowGap => "row gap",
        }
    }

    /// `DENSITY_TABLE` name of the step; `S0` has none.
    pub const fn metric_name(self) -> Option<&'static str> {
        match self {
            Self::S0 => None,
            Self::S1 => Some("space_1"),
            Self::S2 => Some("space_2"),
            Self::S3 => Some("space_3"),
            Self::S4 => Some("space_4"),
            Self::S5 => Some("space_5"),
            Self::S6 => Some("space_6"),
            Self::RowGap => Some("row_gap"),
        }
    }

    /// Normal / Compact / Classic Flash values of the step.
    pub fn density_values(self) -> [f32; 3] {
        self.metric_name().map_or([0.0; 3], density_values)
    }

    /// Rust expression of the step in generated code.
    pub const fn rust_expr(self) -> &'static str {
        match self {
            Self::S0 => "0.0",
            Self::S1 => "space::s1()",
            Self::S2 => "space::s2()",
            Self::S3 => "space::s3()",
            Self::S4 => "space::s4()",
            Self::S5 => "space::s5()",
            Self::S6 => "space::s6()",
            Self::RowGap => "neoflash_ui::metrics::row_gap()",
        }
    }
}

/// Spacing value: either bound to a system [`SpacingToken`] or a custom local `px` override.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum SpacingValue {
    Token(SpacingToken),
    Px(f32),
}

impl Default for SpacingValue {
    fn default() -> Self {
        Self::Token(SpacingToken::S0)
    }
}

impl SpacingValue {
    pub fn resolve(self, systems: &UiStudioSystems) -> f32 {
        match self {
            Self::Token(tok) => systems.spacing.resolve(tok),
            Self::Px(px) => px.max(0.0),
        }
    }

    pub const fn is_override(self) -> bool {
        matches!(self, Self::Px(_))
    }

    /// Rust expression for generated code: a call into the density system for a step, a literal for
    /// custom pixels or a step whose scale was overridden in the Studio.
    pub fn rust_expr(self, systems: &UiStudioSystems) -> String {
        match self {
            Self::Token(tok) if !systems.spacing.is_override(tok) => tok.rust_expr().to_owned(),
            Self::Token(tok) => format!("studio_system_tokens::SPACE_{}", tok.short_name()),
            Self::Px(px) => format!("{:.1}", px.max(0.0)),
        }
    }

    pub fn format_short(self, _systems: &UiStudioSystems) -> String {
        match self {
            Self::Token(tok) => format!(
                "{} ({})",
                tok.label(),
                format_density_values(tok.density_values())
            ),
            Self::Px(px) => format!("{:.0}px*", px),
        }
    }
}

/// 4-sided box padding or margin (`top`, `right`, `bottom`, `left`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Padding {
    pub top: SpacingValue,
    pub right: SpacingValue,
    pub bottom: SpacingValue,
    pub left: SpacingValue,
}

impl Padding {
    pub const fn zero() -> Self {
        Self {
            top: SpacingValue::Token(SpacingToken::S0),
            right: SpacingValue::Token(SpacingToken::S0),
            bottom: SpacingValue::Token(SpacingToken::S0),
            left: SpacingValue::Token(SpacingToken::S0),
        }
    }

    pub const fn all_token(token: SpacingToken) -> Self {
        Self {
            top: SpacingValue::Token(token),
            right: SpacingValue::Token(token),
            bottom: SpacingValue::Token(token),
            left: SpacingValue::Token(token),
        }
    }

    pub const fn all_px(px: f32) -> Self {
        Self {
            top: SpacingValue::Px(px),
            right: SpacingValue::Px(px),
            bottom: SpacingValue::Px(px),
            left: SpacingValue::Px(px),
        }
    }

    pub const fn symmetric_px(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: SpacingValue::Px(vertical),
            right: SpacingValue::Px(horizontal),
            bottom: SpacingValue::Px(vertical),
            left: SpacingValue::Px(horizontal),
        }
    }

    /// Resolve `[top, right, bottom, left]` in logical points against `systems`.
    pub fn resolve(&self, systems: &UiStudioSystems) -> [f32; 4] {
        [
            self.top.resolve(systems),
            self.right.resolve(systems),
            self.bottom.resolve(systems),
            self.left.resolve(systems),
        ]
    }

    pub fn horizontal_sum(&self, systems: &UiStudioSystems) -> f32 {
        self.left.resolve(systems) + self.right.resolve(systems)
    }

    pub fn vertical_sum(&self, systems: &UiStudioSystems) -> f32 {
        self.top.resolve(systems) + self.bottom.resolve(systems)
    }

    pub fn reset_to_token(&mut self, token: SpacingToken) {
        *self = Self::all_token(token);
    }
}

/// 2D item gap (`[gap_x, gap_y]`) for containers and grids.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GapPair {
    pub x: SpacingValue,
    pub y: SpacingValue,
}

impl Default for GapPair {
    fn default() -> Self {
        Self {
            x: SpacingValue::Token(SpacingToken::S2),
            y: SpacingValue::Token(SpacingToken::S2),
        }
    }
}

impl GapPair {
    pub const fn both_token(token: SpacingToken) -> Self {
        Self {
            x: SpacingValue::Token(token),
            y: SpacingValue::Token(token),
        }
    }

    pub const fn both_px(px: f32) -> Self {
        Self {
            x: SpacingValue::Px(px),
            y: SpacingValue::Px(px),
        }
    }

    pub fn resolve(&self, systems: &UiStudioSystems) -> Vec2 {
        egui::vec2(self.x.resolve(systems), self.y.resolve(systems))
    }
}

/// NF2 Typography roles supported by the studio and system scale inspector.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum StudioTextStyle {
    Title,
    #[default]
    Body,
    BodyStrong,
    Small,
    SmallStrong,
    Caption,
    Overline,
    Mono,
    MonoSmall,
    Heading,
}

impl StudioTextStyle {
    pub const ALL: [Self; 10] = [
        Self::Title,
        Self::Body,
        Self::BodyStrong,
        Self::Small,
        Self::SmallStrong,
        Self::Caption,
        Self::Overline,
        Self::Mono,
        Self::MonoSmall,
        Self::Heading,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Body => "Body",
            Self::BodyStrong => "Body Strong",
            Self::Small => "Small",
            Self::SmallStrong => "Small Strong",
            Self::Caption => "Caption",
            Self::Overline => "Overline",
            Self::Mono => "Mono",
            Self::MonoSmall => "Mono Small",
            Self::Heading => "Heading (Dialog Title)",
        }
    }

    pub const fn to_nf2(self) -> TextStyle {
        match self {
            Self::Title => TextStyle::Title,
            Self::Body => TextStyle::Body,
            Self::BodyStrong => TextStyle::BodyStrong,
            Self::Small => TextStyle::Small,
            Self::SmallStrong => TextStyle::SmallStrong,
            Self::Caption => TextStyle::Caption,
            Self::Overline => TextStyle::Overline,
            Self::Mono => TextStyle::Mono,
            Self::MonoSmall => TextStyle::MonoSmall,
            Self::Heading => TextStyle::DialogTitle,
        }
    }
}

/// Font family choice for typography tokens and custom overrides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioFontFamily {
    #[default]
    Inter,
    RobotoMono,
}

impl StudioFontFamily {
    pub const ALL: [Self; 2] = [Self::Inter, Self::RobotoMono];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Inter => "Inter (UI)",
            Self::RobotoMono => "Roboto Mono",
        }
    }
}

/// Horizontal text alignment inside labels and headers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioTextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl StudioTextAlign {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }
}

/// Semantic color token roles from the NF2 palette.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum StudioColorRole {
    #[default]
    TextPrimary,
    TextSecondary,
    TextTertiary,
    TextDisabled,
    TextOnAccent,
    AccentDefault,
    AccentHover,
    AccentSubtle,
    BgApp,
    BgPanel,
    BgRaised,
    BgField,
    BgHover,
    BgSelected,
    BorderSubtle,
    BorderDefault,
    BorderStrong,
    BorderFocus,
    DangerDefault,
    WarningDefault,
    SuccessDefault,
}

impl StudioColorRole {
    pub const ALL: [Self; 21] = [
        Self::TextPrimary,
        Self::TextSecondary,
        Self::TextTertiary,
        Self::TextDisabled,
        Self::TextOnAccent,
        Self::AccentDefault,
        Self::AccentHover,
        Self::AccentSubtle,
        Self::BgApp,
        Self::BgPanel,
        Self::BgRaised,
        Self::BgField,
        Self::BgHover,
        Self::BgSelected,
        Self::BorderSubtle,
        Self::BorderDefault,
        Self::BorderStrong,
        Self::BorderFocus,
        Self::DangerDefault,
        Self::WarningDefault,
        Self::SuccessDefault,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::TextPrimary => "text/primary",
            Self::TextSecondary => "text/secondary",
            Self::TextTertiary => "text/tertiary",
            Self::TextDisabled => "text/disabled",
            Self::TextOnAccent => "text/on-accent",
            Self::AccentDefault => "accent/default",
            Self::AccentHover => "accent/hover",
            Self::AccentSubtle => "accent/subtle",
            Self::BgApp => "bg/app",
            Self::BgPanel => "bg/panel",
            Self::BgRaised => "bg/raised",
            Self::BgField => "bg/field",
            Self::BgHover => "bg/hover",
            Self::BgSelected => "bg/selected",
            Self::BorderSubtle => "border/subtle",
            Self::BorderDefault => "border/default",
            Self::BorderStrong => "border/strong",
            Self::BorderFocus => "border/focus",
            Self::DangerDefault => "danger/default",
            Self::WarningDefault => "warning/default",
            Self::SuccessDefault => "success/default",
        }
    }

    pub const fn read_from_palette(self, p: &Palette) -> Color32 {
        match self {
            Self::TextPrimary => p.text_primary,
            Self::TextSecondary => p.text_secondary,
            Self::TextTertiary => p.text_tertiary,
            Self::TextDisabled => p.text_disabled,
            Self::TextOnAccent => p.text_on_accent,
            Self::AccentDefault => p.accent_default,
            Self::AccentHover => p.accent_hover,
            Self::AccentSubtle => p.accent_subtle,
            Self::BgApp => p.bg_app,
            Self::BgPanel => p.bg_panel,
            Self::BgRaised => p.bg_raised,
            Self::BgField => p.bg_field,
            Self::BgHover => p.bg_hover,
            Self::BgSelected => p.bg_selected,
            Self::BorderSubtle => p.border_subtle,
            Self::BorderDefault => p.border_default,
            Self::BorderStrong => p.border_strong,
            Self::BorderFocus => p.border_focus,
            Self::DangerDefault => p.danger_default,
            Self::WarningDefault => p.warning_default,
            Self::SuccessDefault => p.success_default,
        }
    }
}

/// Custom or resolved typography specification for a widget or container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypographyOverride {
    pub font_family: StudioFontFamily,
    pub size_pt: f32,
    pub weight: u16,
    pub line_height_pt: f32,
    pub letter_spacing: f32,
    pub align: StudioTextAlign,
    pub color_token: StudioColorRole,
    pub uppercase: bool,
}

impl Default for TypographyOverride {
    fn default() -> Self {
        Self {
            font_family: StudioFontFamily::Inter,
            size_pt: 11.0,
            weight: 400,
            line_height_pt: 14.0,
            letter_spacing: 0.0,
            align: StudioTextAlign::Left,
            color_token: StudioColorRole::TextPrimary,
            uppercase: false,
        }
    }
}

/// Typography binding on a node: either inherits a system [`StudioTextStyle`] or uses a local [`TypographyOverride`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "spec", rename_all = "snake_case")]
pub enum TypographyBinding {
    System(StudioTextStyle),
    Custom(TypographyOverride),
}

impl Default for TypographyBinding {
    fn default() -> Self {
        Self::System(StudioTextStyle::Body)
    }
}

impl TypographyBinding {
    pub fn resolve(&self, systems: &UiStudioSystems) -> TypographyOverride {
        match self {
            Self::System(style) => systems.typography.resolve_style(*style),
            Self::Custom(custom) => custom.clone(),
        }
    }

    pub const fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }

    pub fn promote_to_custom(&mut self, systems: &UiStudioSystems) -> &mut TypographyOverride {
        if !matches!(self, Self::Custom(_)) {
            let resolved = self.resolve(systems);
            *self = Self::Custom(resolved);
        }
        match self {
            Self::Custom(ov) => ov,
            Self::System(_) => unreachable!(),
        }
    }

    pub fn reset_to_system(&mut self, style: StudioTextStyle) {
        *self = Self::System(style);
    }
}

/// Corner radius token or custom pixel radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum RadiusToken {
    #[default]
    None,
    Sm,
    Md,
    Lg,
    Xl,
    Full,
    CustomPx(f32),
}

impl RadiusToken {
    pub fn resolve_px(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Sm => radius::sm(),
            Self::Md => radius::md(),
            Self::Lg => radius::lg(),
            Self::Xl => radius::xl(),
            Self::Full => radius::FULL,
            Self::CustomPx(v) => v.max(0.0),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Sm => "Marks (sm)",
            Self::Md => "Controls (md)",
            Self::Lg => "Panels (lg)",
            Self::Xl => "Dialogs (xl)",
            Self::Full => "Full",
            Self::CustomPx(_) => "Custom px",
        }
    }
}

/// Shared layout system binding so editing a system updates every bound container across the studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedSystemKind {
    TwoColumnForm,
    SectionHeader,
    PanelHeader,
    ToolRail,
}

impl SharedSystemKind {
    pub const ALL: [Self; 4] = [
        Self::TwoColumnForm,
        Self::SectionHeader,
        Self::PanelHeader,
        Self::ToolRail,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::TwoColumnForm => "Two-Column Form System",
            Self::SectionHeader => "Section Header System",
            Self::PanelHeader => "Panel Header System",
            Self::ToolRail => "Tool Rail System",
        }
    }
}

/// Complete visual & spacing style attached to a [`UiStudioNode`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeVisualStyle {
    pub padding: Padding,
    pub margin: Padding,
    pub gap: GapPair,
    pub typography: TypographyBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_token: Option<StudioColorRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_token: Option<StudioColorRole>,
    pub border_width: f32,
    pub corner_radius: RadiusToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_binding: Option<SharedSystemKind>,
}

impl Default for NodeVisualStyle {
    fn default() -> Self {
        Self {
            padding: Padding::zero(),
            margin: Padding::zero(),
            gap: GapPair::default(),
            typography: TypographyBinding::System(StudioTextStyle::Body),
            bg_token: None,
            border_token: None,
            border_width: 0.0,
            corner_radius: RadiusToken::None,
            system_binding: None,
        }
    }
}

impl NodeVisualStyle {
    /// Reset all local overrides on this node back to NF2 system tokens.
    pub fn reset_to_system_tokens(&mut self, default_text: StudioTextStyle) {
        self.padding = Padding::zero();
        self.margin = Padding::zero();
        self.gap = GapPair::both_token(SpacingToken::S2);
        self.typography = TypographyBinding::System(default_text);
        self.bg_token = None;
        self.border_token = None;
        self.border_width = 0.0;
        self.corner_radius = RadiusToken::None;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Edit Systems as Systems (`UiStudioSystems` & Token Scales)
// ─────────────────────────────────────────────────────────────────────────────

/// Spacing scale of the Studio. A stored step equal to its Normal reference follows the density
/// system (`space::s1()…s6()`); any other stored value is a Studio-local override.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpacingScaleSystem {
    pub s0: f32,
    pub s1: f32,
    pub s2: f32,
    pub s3: f32,
    pub s4: f32,
    pub s5: f32,
    pub s6: f32,
}

impl Default for SpacingScaleSystem {
    fn default() -> Self {
        Self {
            s0: 0.0,
            s1: density_values("space_1")[0],
            s2: density_values("space_2")[0],
            s3: density_values("space_3")[0],
            s4: density_values("space_4")[0],
            s5: density_values("space_5")[0],
            s6: density_values("space_6")[0],
        }
    }
}

impl SpacingScaleSystem {
    /// NF2 scale stored by sessions saved before the density system.
    const LEGACY: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 24.0, 32.0];

    fn stored(&self, token: SpacingToken) -> Option<f32> {
        match token {
            SpacingToken::S0 => Some(self.s0),
            SpacingToken::S1 => Some(self.s1),
            SpacingToken::S2 => Some(self.s2),
            SpacingToken::S3 => Some(self.s3),
            SpacingToken::S4 => Some(self.s4),
            SpacingToken::S5 => Some(self.s5),
            SpacingToken::S6 => Some(self.s6),
            SpacingToken::RowGap => None,
        }
    }

    /// Value of `token` in the current density (`neoflash_ui::metrics::density()`).
    pub fn resolve(&self, token: SpacingToken) -> f32 {
        match (token.metric_name(), self.stored(token)) {
            (Some(metric), Some(stored)) => follow_density(stored, metric),
            (Some(metric), None) => {
                density_values(metric)[density_index(neoflash_ui::metrics::density())]
            }
            (None, stored) => stored.unwrap_or(0.0),
        }
    }

    /// The step was changed in the Studio and no longer follows the density system.
    pub fn is_override(&self, token: SpacingToken) -> bool {
        match (token.metric_name(), self.stored(token)) {
            (Some(metric), Some(stored)) => !follows_density(stored, metric),
            _ => false,
        }
    }

    pub fn set(&mut self, token: SpacingToken, value_pt: f32) {
        let v = value_pt.max(0.0);
        match token {
            SpacingToken::S0 => self.s0 = v,
            SpacingToken::S1 => self.s1 = v,
            SpacingToken::S2 => self.s2 = v,
            SpacingToken::S3 => self.s3 = v,
            SpacingToken::S4 => self.s4 = v,
            SpacingToken::S5 => self.s5 = v,
            SpacingToken::S6 => self.s6 = v,
            SpacingToken::RowGap => {}
        }
    }

    /// Put a step back on the density system.
    pub fn reset(&mut self, token: SpacingToken) {
        if let Some(metric) = token.metric_name() {
            self.set(token, density_values(metric)[0]);
        }
    }

    fn migrate_legacy(&mut self) {
        if [self.s1, self.s2, self.s3, self.s4, self.s5, self.s6] == Self::LEGACY {
            *self = Self::default();
        }
    }
}

/// System-wide typography scale entry for one [`StudioTextStyle`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypographyScaleEntry {
    pub style: StudioTextStyle,
    pub font_family: StudioFontFamily,
    pub size_pt: f32,
    pub line_height_pt: f32,
    pub weight: u16,
    pub letter_spacing: f32,
    pub uppercase: bool,
    pub default_color: StudioColorRole,
}

impl TypographyScaleEntry {
    pub fn from_nf2(style: StudioTextStyle) -> Self {
        let spec = style.to_nf2().spec();
        let font_family = match spec.family {
            neoflash_ui::tokens::Family::Inter => StudioFontFamily::Inter,
            neoflash_ui::tokens::Family::Mono => StudioFontFamily::RobotoMono,
        };
        let default_color = match style {
            StudioTextStyle::Overline | StudioTextStyle::Caption => StudioColorRole::TextSecondary,
            _ => StudioColorRole::TextPrimary,
        };
        Self {
            style,
            font_family,
            size_pt: spec.size,
            line_height_pt: spec.line_height,
            weight: spec.weight,
            letter_spacing: spec.tracking_em,
            uppercase: spec.uppercase,
            default_color,
        }
    }

    pub fn to_override(&self) -> TypographyOverride {
        TypographyOverride {
            font_family: self.font_family,
            size_pt: self.size_pt,
            weight: self.weight,
            line_height_pt: self.line_height_pt,
            letter_spacing: self.letter_spacing,
            align: StudioTextAlign::Left,
            color_token: self.default_color,
            uppercase: self.uppercase,
        }
    }
}

/// System-wide typography scale (`Title`, `Body`, `Small`, `Caption`, `Mono`, `Heading`, etc.).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypographyScaleSystem {
    pub entries: Vec<TypographyScaleEntry>,
}

impl Default for TypographyScaleSystem {
    fn default() -> Self {
        Self {
            entries: StudioTextStyle::ALL
                .into_iter()
                .map(TypographyScaleEntry::from_nf2)
                .collect(),
        }
    }
}

impl TypographyScaleSystem {
    pub fn entry(&self, style: StudioTextStyle) -> TypographyScaleEntry {
        self.entries
            .iter()
            .find(|e| e.style == style)
            .cloned()
            .unwrap_or_else(|| TypographyScaleEntry::from_nf2(style))
    }

    pub fn entry_mut(&mut self, style: StudioTextStyle) -> &mut TypographyScaleEntry {
        if !self.entries.iter().any(|e| e.style == style) {
            self.entries.push(TypographyScaleEntry::from_nf2(style));
        }
        self.entries
            .iter_mut()
            .find(|e| e.style == style)
            .expect("entry inserted")
    }

    pub fn resolve_style(&self, style: StudioTextStyle) -> TypographyOverride {
        self.entry(style).to_override()
    }
}

/// Density the canvas previews are laid out with (`Normal`, `Compact`, `Classic Flash`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioDensityProfile {
    #[default]
    Comfortable,
    Compact,
    ClassicFlash,
}

impl StudioDensityProfile {
    pub const ALL: [Self; 3] = [Self::Comfortable, Self::Compact, Self::ClassicFlash];

    pub const fn label(self) -> &'static str {
        self.ui().label()
    }

    /// The editor density this profile stands for.
    pub const fn ui(self) -> Density {
        match self {
            Self::Comfortable => Density::Comfortable,
            Self::Compact => Density::Compact,
            Self::ClassicFlash => Density::ClassicFlash,
        }
    }

    pub const fn from_ui(density: Density) -> Self {
        match density {
            Density::Comfortable => Self::Comfortable,
            Density::Compact => Self::Compact,
            Density::ClassicFlash => Self::ClassicFlash,
        }
    }
}

/// Geometry of one density, read from `neoflash_ui::metrics::DENSITY_TABLE` (never stored).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DensityProfileMetrics {
    pub control_height: f32,
    pub row_height: f32,
    pub layer_row_height: f32,
    pub section_header_height: f32,
    pub panel_header_height: f32,
    pub icon_grid_pt: f32,
    pub inspector_width: f32,
    pub tools_rail_width: f32,
    pub tools_two_col_width: f32,
}

impl DensityProfileMetrics {
    pub fn for_density(density: Density) -> Self {
        let i = density_index(density);
        let pick = |name: &str| density_values(name)[i];
        Self {
            control_height: pick("control"),
            row_height: pick("row"),
            layer_row_height: density.pick(
                neoflash_ui::metrics::LAYER_ROW,
                neoflash_ui::metrics::LAYER_ROW_SHORT,
                neoflash_ui::metrics::LAYER_ROW_COMPACT,
            ),
            section_header_height: pick("section_header"),
            panel_header_height: pick("panel_header"),
            icon_grid_pt: pick("icon_tool"),
            inspector_width: pick("inspector"),
            tools_rail_width: pick("tools_rail"),
            tools_two_col_width: pick("tools_two_columns"),
        }
    }

    /// Metrics of the density set on this UI thread (inside a [`PreviewDensity`] scope: the preview's).
    pub fn current() -> Self {
        Self::for_density(neoflash_ui::metrics::density())
    }

    pub fn comfortable() -> Self {
        Self::for_density(Density::Comfortable)
    }

    pub fn compact() -> Self {
        Self::for_density(Density::Compact)
    }

    pub fn classic_flash() -> Self {
        Self::for_density(Density::ClassicFlash)
    }
}

/// Preview density of the Studio. The sizes themselves live in `neoflash_ui::metrics`, so the Studio
/// cannot drift from the editor; sessions saved with per-profile numbers load and ignore them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DensitySystem {
    pub active_profile: StudioDensityProfile,
}

impl DensitySystem {
    pub fn active(&self) -> DensityProfileMetrics {
        DensityProfileMetrics::for_density(self.active_profile.ui())
    }
}

/// Shared 2-column inspector form system (`X`/`Y`, `W`/`H`, label + control rows).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TwoColumnFormSystem {
    pub min_cell_width: f32,
    pub gap: SpacingValue,
    pub label_ratio: f32,
    pub label_width_px: f32,
    pub stack_below_width: f32,
}

impl Default for TwoColumnFormSystem {
    fn default() -> Self {
        Self {
            min_cell_width: 88.0,
            gap: SpacingValue::Token(SpacingToken::S2),
            label_ratio: 0.30,
            label_width_px: neoflash_ui::metrics::LABEL_WIDTH,
            stack_below_width: 196.0,
        }
    }
}

impl TwoColumnFormSystem {
    /// Label column in the current density (`metrics::label_width()` unless overridden).
    pub fn resolved_label_width(&self) -> f32 {
        follow_density(self.label_width_px, "label_width")
    }
}

/// Shared section header layout system (`NF2/Section Header`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionHeaderSystem {
    pub height: f32,
    pub padding_x: SpacingValue,
    pub body_gap: SpacingValue,
    pub text_style: StudioTextStyle,
    pub collapsible: bool,
}

impl Default for SectionHeaderSystem {
    fn default() -> Self {
        Self {
            height: neoflash_ui::metrics::SECTION_HEADER,
            padding_x: SpacingValue::Token(SpacingToken::S2),
            body_gap: SpacingValue::Token(SpacingToken::RowGap),
            text_style: StudioTextStyle::Overline,
            collapsible: true,
        }
    }
}

impl SectionHeaderSystem {
    /// Header height in the current density (`metrics::section_header()` unless overridden).
    pub fn resolved_height(&self) -> f32 {
        follow_density(self.height, "section_header")
    }
}

/// Shared panel header & tab layout system (`NF2/Panel Header`, `NF2/Floating Panel Header`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelHeaderSystem {
    pub height: f32,
    pub tab_height: f32,
    pub padding_x: SpacingValue,
    pub body_padding: SpacingValue,
    pub text_style: StudioTextStyle,
    pub show_menu_button: bool,
}

impl Default for PanelHeaderSystem {
    fn default() -> Self {
        Self {
            height: neoflash_ui::metrics::PANEL_HEADER,
            tab_height: neoflash_ui::metrics::PANEL_TAB,
            padding_x: SpacingValue::Token(SpacingToken::S3),
            body_padding: SpacingValue::Token(SpacingToken::S4),
            text_style: StudioTextStyle::SmallStrong,
            show_menu_button: true,
        }
    }
}

impl PanelHeaderSystem {
    /// Header height in the current density (`metrics::panel_header()` unless overridden).
    pub fn resolved_height(&self) -> f32 {
        follow_density(self.height, "panel_header")
    }

    pub fn resolved_tab_height(&self) -> f32 {
        follow_density(self.tab_height, "panel_tab")
    }
}

/// Shared tools rail layout system (`NF2/Tools Panel`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolRailSystem {
    pub columns: usize,
    pub button_side: f32,
    pub gap: f32,
    pub padding: f32,
}

impl Default for ToolRailSystem {
    fn default() -> Self {
        Self {
            columns: 2,
            button_side: neoflash_ui::metrics::TOOL_BUTTON,
            gap: 2.0,
            padding: 7.0,
        }
    }
}

impl ToolRailSystem {
    /// Tool button side in the current density (`metrics::tool_button()` unless overridden).
    pub fn resolved_button_side(&self) -> f32 {
        follow_density(self.button_side, "tool_button")
    }
}

/// Parametric geometry system for the Timeline (Tween arrows, keyframe dots, diamonds,
/// span-end markers, frame cells, ruler ticks, playhead, onion-skin brackets, and layer column).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineGeometrySystem {
    // ── Tween Arrows (Стрелки твинов) ──
    /// Length of the tween arrowhead triangle along the horizontal shaft (`px`).
    pub tween_arrow_head_length_px: f32,
    /// Half-height (wing spread) of the tween arrowhead triangle (`px`).
    pub tween_arrow_head_wing_px: f32,
    /// Horizontal inset from the left edge of the tween span to the start of the arrow shaft (`px`).
    pub tween_arrow_start_inset_px: f32,
    /// Horizontal inset from the right edge of the tween span to the tip of the arrowhead (`px`).
    pub tween_arrow_end_inset_px: f32,
    /// Stroke thickness of the tween arrow shaft (`px`).
    pub tween_arrow_stroke_width_px: f32,
    /// Vertical position of the tween arrow inside the layer row (`%`, `10.0..=90.0`).
    pub tween_arrow_y_pct: f32,
    /// Dash length for broken tween spans (`px`).
    pub broken_tween_dash_px: f32,
    /// Gap length between dashes for broken tween spans (`px`).
    pub broken_tween_gap_px: f32,

    // ── Timeline Cells, Keyframes & Badges (Ячейки, ключи и маркеры) ──
    /// Width of a single frame cell in the timeline grid (`px`).
    pub frame_cell_width_px: f32,
    /// Height of a single layer row in the timeline (`px`).
    pub layer_row_height_px: f32,
    /// Height of the frame number ruler row (`px`).
    pub ruler_height_px: f32,
    /// Radius of keyframe (`●`) and blank keyframe (`○`) dots (`px`).
    pub keyframe_dot_radius_px: f32,
    /// Vertical position of keyframe dots inside the frame cell (`%`).
    pub keyframe_dot_y_pct: f32,
    /// Radius of motion-tween property keyframe diamonds (`◆`) (`px`).
    pub motion_diamond_radius_px: f32,
    /// Width of the hollow end-of-span marker rectangle (`[ ]`) (`px`).
    pub span_end_rect_w_px: f32,
    /// Height of the hollow end-of-span marker rectangle (`[ ]`) (`px`).
    pub span_end_rect_h_px: f32,
    /// Font/badge size of the frame action script marker (`a`) (`px`).
    pub action_marker_size_px: f32,
    /// Size of the red frame-label flag triangle/banner (`px`).
    pub label_flag_size_px: f32,
    /// Height of major 5-frame ruler ticks (`px`).
    pub ruler_major_tick_h_px: f32,
    /// Height of minor 1-frame ruler ticks (`px`).
    pub ruler_minor_tick_h_px: f32,

    // ── Playhead & Onion Skin (Плейхед и Onion Skin) ──
    /// Active playhead frame index (`1`-based) in the interactive timeline preview.
    pub playhead_frame: usize,
    /// Width of the red playhead cap in the ruler (`px`).
    pub playhead_cap_width_px: f32,
    /// Height of the red playhead cap in the ruler (`px`).
    pub playhead_cap_height_px: f32,
    /// Stroke width of the vertical playhead line across all layers (`px`).
    pub playhead_line_width_px: f32,
    /// Onion skin start frame (`1`-based).
    pub onion_start_frame: usize,
    /// Onion skin end frame (`1`-based).
    pub onion_end_frame: usize,
    /// Horizontal arm width of onion-skin range brackets (`px`).
    pub onion_bracket_width_px: f32,
    /// Radius of onion-skin range drag handles (`px`).
    pub onion_handle_radius_px: f32,

    // ── Layer Tree Column (Колонка слоёв в % или px) ──
    /// Width of the left layer-list column (`Px` or `Percent` of the timeline width).
    pub layer_list_width: UiSizeUnit,
    /// Indentation per folder nesting depth in the layer tree (`px`).
    pub layer_indent_px: f32,
    /// Horizontal spacing between visibility/lock/outline icons in a layer row (`px`).
    pub layer_icon_gap_px: f32,
}

impl Default for TimelineGeometrySystem {
    fn default() -> Self {
        Self {
            tween_arrow_head_length_px: 7.0,
            tween_arrow_head_wing_px: 3.5,
            tween_arrow_start_inset_px: 10.0,
            tween_arrow_end_inset_px: 4.0,
            tween_arrow_stroke_width_px: 1.25,
            tween_arrow_y_pct: 62.0,
            broken_tween_dash_px: 4.0,
            broken_tween_gap_px: 3.0,
            frame_cell_width_px: 10.0,
            layer_row_height_px: neoflash_ui::metrics::LAYER_ROW,
            ruler_height_px: neoflash_ui::metrics::TIMELINE_RULER,
            keyframe_dot_radius_px: 3.0,
            keyframe_dot_y_pct: 62.0,
            motion_diamond_radius_px: 3.5,
            span_end_rect_w_px: 4.0,
            span_end_rect_h_px: 7.0,
            action_marker_size_px: 8.0,
            label_flag_size_px: 6.0,
            ruler_major_tick_h_px: 8.0,
            ruler_minor_tick_h_px: 4.0,
            playhead_frame: 14,
            playhead_cap_width_px: 11.0,
            playhead_cap_height_px: 13.0,
            playhead_line_width_px: 1.5,
            onion_start_frame: 10,
            onion_end_frame: 19,
            onion_bracket_width_px: 5.0,
            onion_handle_radius_px: 3.5,
            layer_list_width: UiSizeUnit::Px(neoflash_ui::metrics::LAYER_COLUMN),
            layer_indent_px: neoflash_ui::metrics::LAYER_INDENT,
            layer_icon_gap_px: 6.0,
        }
    }
}

impl TimelineGeometrySystem {
    /// Layer row height in the current density (the density's default row unless overridden).
    pub fn resolved_layer_row_height(&self) -> f32 {
        if self.layer_row_height_px == neoflash_ui::metrics::LAYER_ROW {
            neoflash_ui::metrics::default_layer_row()
        } else {
            self.layer_row_height_px
        }
    }

    /// Frame ruler height in the current density (`metrics::timeline_ruler()` unless overridden).
    pub fn resolved_ruler_height(&self) -> f32 {
        follow_density(self.ruler_height_px, "timeline_ruler")
    }

    /// Resolve the left layer-tree column width in pixels for a given total timeline width.
    pub fn resolve_layer_list_width(&self, total_timeline_width: f32) -> f32 {
        let raw = match self.layer_list_width {
            UiSizeUnit::Px(px) => follow_density(px, "layer_column"),
            UiSizeUnit::Percent(pct) => total_timeline_width * (pct / 100.0),
            UiSizeUnit::Auto => neoflash_ui::metrics::layer_column(),
            UiSizeUnit::Fill(_) => total_timeline_width * 0.25,
        };
        raw.clamp(120.0, (total_timeline_width - 140.0).max(120.0))
    }

    /// Compute the exact shaft endpoints `(shaft_start, arrow_tip)` and arrowhead triangle
    /// `[arrow_tip, upper_wing, lower_wing]` for a tween span rectangle.
    pub fn compute_tween_arrow_points(&self, span_rect: Rect) -> (Pos2, Pos2, [Pos2; 3]) {
        let y = span_rect.top()
            + span_rect.height() * (self.tween_arrow_y_pct.clamp(10.0, 90.0) / 100.0);
        let start_x = (span_rect.left() + self.tween_arrow_start_inset_px.max(0.0))
            .min(span_rect.right() - 2.0);
        let tip_x = (span_rect.right() - self.tween_arrow_end_inset_px.max(0.0)).max(start_x + 2.0);
        let head_len = self
            .tween_arrow_head_length_px
            .clamp(2.0, (tip_x - start_x).max(2.0));
        let wing = self.tween_arrow_head_wing_px.clamp(1.0, 20.0);
        let base_x = tip_x - head_len;
        let shaft_start = egui::pos2(start_x, y);
        let arrow_tip = egui::pos2(tip_x, y);
        let triangle = [
            arrow_tip,
            egui::pos2(base_x, y - wing),
            egui::pos2(base_x, y + wing),
        ];
        (shaft_start, arrow_tip, triangle)
    }
}

/// Resolved dock rectangles for the Full Editor Workspace Shell preview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkspaceShellRects {
    pub menubar: Rect,
    pub doc_tabs: Rect,
    pub edit_bar: Rect,
    pub left_tools: Rect,
    pub center_stage: Rect,
    pub right_inspector: Rect,
    pub bottom_timeline: Rect,
}

/// Parametric layout system for the Full Editor Workspace Shell (`Tools`, `Stage`, `Inspector`, `Timeline`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceShellSystem {
    pub top_menubar_height_px: f32,
    pub doc_tabs_height_px: f32,
    pub edit_bar_height_px: f32,
    pub shell_left_tools_width: UiSizeUnit,
    pub shell_right_inspector_width: UiSizeUnit,
    pub shell_bottom_timeline_height: UiSizeUnit,
    pub dock_splitter_gap_px: f32,
}

impl Default for WorkspaceShellSystem {
    fn default() -> Self {
        Self {
            top_menubar_height_px: neoflash_ui::metrics::TOP_BAR,
            doc_tabs_height_px: neoflash_ui::metrics::DOCK_TAB,
            edit_bar_height_px: neoflash_ui::metrics::BREADCRUMB,
            shell_left_tools_width: UiSizeUnit::Px(neoflash_ui::metrics::TOOLS_RAIL),
            shell_right_inspector_width: UiSizeUnit::Percent(25.0),
            shell_bottom_timeline_height: UiSizeUnit::Percent(34.0),
            dock_splitter_gap_px: 3.0,
        }
    }
}

impl WorkspaceShellSystem {
    /// Resolve all dock and bar rectangles inside `viewport` using live `%` or `px` settings.
    pub fn resolve_shell_rects(&self, viewport: Rect) -> WorkspaceShellRects {
        let gap = self.dock_splitter_gap_px.clamp(0.0, 16.0);
        let menu_h = follow_density(self.top_menubar_height_px, "top_bar").clamp(18.0, 48.0);
        let tabs_h = follow_density(self.doc_tabs_height_px, "dock_tab").clamp(20.0, 44.0);
        let edit_h = follow_density(self.edit_bar_height_px, "breadcrumb").clamp(18.0, 40.0);

        let menubar = Rect::from_min_size(viewport.min, egui::vec2(viewport.width(), menu_h));
        let doc_tabs = Rect::from_min_size(
            egui::pos2(viewport.left(), menubar.bottom()),
            egui::vec2(viewport.width(), tabs_h),
        );
        let edit_bar = Rect::from_min_size(
            egui::pos2(viewport.left(), doc_tabs.bottom()),
            egui::vec2(viewport.width(), edit_h),
        );

        let body_top = edit_bar.bottom() + gap;
        let body_h = (viewport.bottom() - body_top).max(160.0);

        let raw_timeline_h = match self.shell_bottom_timeline_height {
            UiSizeUnit::Px(px) => px,
            UiSizeUnit::Percent(pct) => body_h * (pct / 100.0),
            UiSizeUnit::Auto => 210.0,
            UiSizeUnit::Fill(_) => body_h * 0.34,
        };
        let timeline_h = raw_timeline_h.clamp(96.0, (body_h - 110.0).max(96.0));
        let upper_h = (body_h - timeline_h - gap).max(96.0);

        let total_w = viewport.width().max(320.0);
        let raw_left_w = match self.shell_left_tools_width {
            UiSizeUnit::Px(px) => follow_density(px, "tools_rail"),
            UiSizeUnit::Percent(pct) => total_w * (pct / 100.0),
            UiSizeUnit::Auto | UiSizeUnit::Fill(_) => neoflash_ui::metrics::tools_rail(),
        };
        let left_w = raw_left_w.clamp(32.0, (total_w * 0.25).max(36.0));

        let raw_right_w = match self.shell_right_inspector_width {
            UiSizeUnit::Px(px) => px,
            UiSizeUnit::Percent(pct) => total_w * (pct / 100.0),
            UiSizeUnit::Auto => neoflash_ui::metrics::inspector(),
            UiSizeUnit::Fill(_) => total_w * 0.25,
        };
        let right_w = raw_right_w.clamp(140.0, (total_w - left_w - 140.0).max(140.0));
        let center_w = (total_w - left_w - right_w - gap * 2.0).max(100.0);

        let left_tools = Rect::from_min_size(
            egui::pos2(viewport.left(), body_top),
            egui::vec2(left_w, upper_h),
        );
        let center_stage = Rect::from_min_size(
            egui::pos2(left_tools.right() + gap, body_top),
            egui::vec2(center_w, upper_h),
        );
        let right_inspector = Rect::from_min_size(
            egui::pos2(center_stage.right() + gap, body_top),
            egui::vec2(right_w, upper_h),
        );
        let bottom_timeline = Rect::from_min_size(
            egui::pos2(viewport.left(), left_tools.bottom() + gap),
            egui::vec2(total_w, timeline_h),
        );

        WorkspaceShellRects {
            menubar,
            doc_tabs,
            edit_bar,
            left_tools,
            center_stage,
            right_inspector,
            bottom_timeline,
        }
    }
}

/// Parametric geometry system for the Center Stage Viewport (rulers, guides, transform handles).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageOverlaySystem {
    pub stage_ruler_thickness_px: f32,
    pub guide_stroke_width_px: f32,
    pub selection_handle_size_px: f32,
    pub stage_aspect_w: f32,
    pub stage_aspect_h: f32,
    pub show_rulers: bool,
    pub show_guides: bool,
}

impl Default for StageOverlaySystem {
    fn default() -> Self {
        Self {
            stage_ruler_thickness_px: neoflash_ui::metrics::STAGE_RULER,
            guide_stroke_width_px: 1.0,
            selection_handle_size_px: 6.0,
            stage_aspect_w: 550.0,
            stage_aspect_h: 400.0,
            show_rulers: true,
            show_guides: true,
        }
    }
}

/// Per-theme semantic palette token overrides.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaletteTokenOverrides {
    #[serde(default)]
    pub dark: BTreeMap<StudioColorRole, [u8; 4]>,
    #[serde(default)]
    pub light: BTreeMap<StudioColorRole, [u8; 4]>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mid_neutral_gray: BTreeMap<StudioColorRole, [u8; 4]>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dark_graphite: BTreeMap<StudioColorRole, [u8; 4]>,
}

impl PaletteTokenOverrides {
    fn for_theme(&self, theme: ThemeMode) -> &BTreeMap<StudioColorRole, [u8; 4]> {
        match theme {
            ThemeMode::Dark => &self.dark,
            ThemeMode::Light => &self.light,
            ThemeMode::MidNeutralGray => &self.mid_neutral_gray,
            ThemeMode::DarkGraphite => &self.dark_graphite,
        }
    }

    fn for_theme_mut(&mut self, theme: ThemeMode) -> &mut BTreeMap<StudioColorRole, [u8; 4]> {
        match theme {
            ThemeMode::Dark => &mut self.dark,
            ThemeMode::Light => &mut self.light,
            ThemeMode::MidNeutralGray => &mut self.mid_neutral_gray,
            ThemeMode::DarkGraphite => &mut self.dark_graphite,
        }
    }

    pub fn resolve_color(&self, theme: ThemeMode, role: StudioColorRole) -> Color32 {
        let map = self.for_theme(theme);
        if let Some(&[r, g, b, a]) = map.get(&role) {
            return Color32::from_rgba_unmultiplied(r, g, b, a);
        }
        role.read_from_palette(theme.nf2().palette())
    }

    pub fn set_color_override(&mut self, theme: ThemeMode, role: StudioColorRole, color: Color32) {
        self.for_theme_mut(theme)
            .insert(role, color.to_srgba_unmultiplied());
    }

    pub fn reset_color_override(&mut self, theme: ThemeMode, role: StudioColorRole) {
        self.for_theme_mut(theme).remove(&role);
    }

    pub fn has_override(&self, theme: ThemeMode, role: StudioColorRole) -> bool {
        self.for_theme(theme).contains_key(&role)
    }
}

/// Top-level collection of all editable NF2 systems and token scales.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiStudioSystems {
    pub spacing: SpacingScaleSystem,
    pub typography: TypographyScaleSystem,
    pub density: DensitySystem,
    pub two_column_form: TwoColumnFormSystem,
    pub section_header: SectionHeaderSystem,
    pub panel_header: PanelHeaderSystem,
    pub tool_rail: ToolRailSystem,
    #[serde(default)]
    pub timeline_geometry: TimelineGeometrySystem,
    #[serde(default)]
    pub workspace_shell: WorkspaceShellSystem,
    #[serde(default)]
    pub stage_overlay: StageOverlaySystem,
    pub palette_overrides: PaletteTokenOverrides,
}

impl StageOverlaySystem {
    /// Stage ruler thickness in the current density (`metrics::stage_ruler()` unless overridden).
    pub fn resolved_ruler_thickness(&self) -> f32 {
        follow_density(self.stage_ruler_thickness_px, "stage_ruler")
    }
}

impl UiStudioSystems {
    /// Reset every system scale and shared layout system back to the reference defaults.
    pub fn reset_all_to_defaults(&mut self) {
        *self = Self::default();
    }

    /// Sessions saved before the density system stored the NF2 reference sizes as plain numbers.
    /// A value still equal to its old reference moves to the current one, so it follows density.
    pub fn migrate_legacy_defaults(&mut self) {
        fn moved(value: &mut f32, legacy: f32, reference: f32) {
            if *value == legacy {
                *value = reference;
            }
        }
        fn moved_px(value: &mut UiSizeUnit, legacy: f32, reference: f32) {
            if *value == UiSizeUnit::Px(legacy) {
                *value = UiSizeUnit::Px(reference);
            }
        }
        use neoflash_ui::metrics as m;
        self.spacing.migrate_legacy();
        moved(
            &mut self.two_column_form.label_width_px,
            76.0,
            m::LABEL_WIDTH,
        );
        moved(&mut self.section_header.height, 32.0, m::SECTION_HEADER);
        moved(&mut self.panel_header.height, 40.0, m::PANEL_HEADER);
        moved(&mut self.panel_header.tab_height, 36.0, m::PANEL_TAB);
        moved(&mut self.tool_rail.button_side, 32.0, m::TOOL_BUTTON);
        let tg = &mut self.timeline_geometry;
        moved(&mut tg.layer_row_height_px, 24.0, m::LAYER_ROW);
        moved(&mut tg.ruler_height_px, 22.0, m::TIMELINE_RULER);
        moved_px(&mut tg.layer_list_width, 220.0, m::LAYER_COLUMN);
        let ws = &mut self.workspace_shell;
        moved(&mut ws.top_menubar_height_px, 26.0, m::TOP_BAR);
        moved(&mut ws.doc_tabs_height_px, 28.0, m::DOCK_TAB);
        moved(&mut ws.edit_bar_height_px, 24.0, m::BREADCRUMB);
        moved_px(&mut ws.shell_left_tools_width, 46.0, m::TOOLS_RAIL);
    }
}
