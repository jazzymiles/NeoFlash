//! Interactive Visual UI & System Layout Studio (`Window ▸ Developer ▸ UI Layout Studio…`).
//!
//! Enables visual authoring, inspection, and live system-level tuning of NeoFlash panels,
//! containers, grids, and NF2 design tokens without leaving the editor:
//! - **Sizing in `%` or `px` (`pt`)**: every container (`Row`, `Column`, `Grid`, `Section`, `Panel`)
//!   and leaf control supports switching width and height between `Percent(0.0..=100.0)`,
//!   `Px(pt)`, `Auto`, and `Fill(fr)` with `min`/`max` constraints.
//! - **Paddings, Gaps & Typography**: 4-sided padding/margin, `[gap_x, gap_y]` item spacing
//!   (bound to NF2 `S0`..`S6` tokens or custom `px`), and typography (`System(TextStyle)` or
//!   custom family, size, weight, line height, tracking, alignment, and semantic color role).
//! - **Edit Systems as Systems**: live inspector for the NF2 spacing scale (`S1`..`S6`),
//!   typography scale (`Title`, `Body`, `Small`, `Caption`, `Mono`, `Heading`), density metrics
//!   (`Comfortable`, `Compact`, `Classic Flash`), shared layout systems (`TwoColumnFormSystem`,
//!   `SectionHeaderSystem`, `PanelHeaderSystem`, `ToolRailSystem`), and Dark/Light palette overrides.
//! - **Structural Container Operations**: wrap selected nodes into a `Row`, `Column`, or
//!   `Grid` (with 1-click 2-col, 3-col, 4-col, and mixed `[120px, 50%, 50%]` presets), unwrap
//!   containers, nest grids inside grid cells to arbitrary depth, create/duplicate/delete widgets,
//!   and reorder with multi-step Undo/Redo.
//! - **Live WYSIWYG Canvas & Build Export**: renders real NF2 components (`neoflash_ui::components`)
//!   with click-to-select outlines and exports both JSON overrides
//!   (`target/ui-studio/ui-layout-overrides.json`) and deterministic Rust layout code
//!   (`target/ui-studio/exported_layout.rs`).

#![allow(dead_code)]

use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline::{
    self as tl, GridKind, LayerColumns, LayerKind, LayerRow, LayersHeaderColumns, ModeCell,
};
use neoflash_ui::icons::{self, Icon, i16};
use neoflash_ui::tokens::{DARK, LIGHT, Palette, TextStyle, radius, space};
use neoflash_ui::typography::rich;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Default path for persisted JSON studio overrides.
pub const DEFAULT_STUDIO_JSON_PATH: &str = "target/ui-studio/ui-layout-overrides.json";
/// Default path for generated Rust layout & system token code.
pub const DEFAULT_STUDIO_RUST_EXPORT_PATH: &str = "target/ui-studio/exported_layout.rs";
/// Default path for exported SVG layout artboards.
pub const DEFAULT_STUDIO_SVG_EXPORT_PATH: &str = "target/ui-studio/exported-layout.svg";

/// Unique identifier for a node inside a [`UiStudioDocument`].
pub type UiNodeId = u64;

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
// 2. Paddings, Gaps, Typography & Visual Styling Tokens
// ─────────────────────────────────────────────────────────────────────────────

/// NF2 spacing scale token (`S0` = 0 pt, `S1`..`S6` = 4, 8, 12, 16, 24, 32 pt by default).
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
}

impl SpacingToken {
    pub const ALL: [Self; 7] = [
        Self::S0,
        Self::S1,
        Self::S2,
        Self::S3,
        Self::S4,
        Self::S5,
        Self::S6,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::S0 => "S0 (0)",
            Self::S1 => "S1",
            Self::S2 => "S2",
            Self::S3 => "S3",
            Self::S4 => "S4",
            Self::S5 => "S5",
            Self::S6 => "S6",
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

    pub fn format_short(self, systems: &UiStudioSystems) -> String {
        match self {
            Self::Token(tok) => format!(
                "{} ({:.0}px)",
                tok.short_name(),
                systems.spacing.resolve(tok)
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
            Self::Sm => radius::SM,
            Self::Md => radius::MD,
            Self::Lg => radius::LG,
            Self::Xl => radius::XL,
            Self::Full => radius::FULL,
            Self::CustomPx(v) => v.max(0.0),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None (0)",
            Self::Sm => "SM (4px)",
            Self::Md => "MD (6px)",
            Self::Lg => "LG (8px)",
            Self::Xl => "XL (12px)",
            Self::Full => "Full (999px)",
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

/// System-wide NF2 spacing scale (`S0`..`S6` in `pt`).
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
            s1: space::S1,
            s2: space::S2,
            s3: space::S3,
            s4: space::S4,
            s5: space::S5,
            s6: space::S6,
        }
    }
}

impl SpacingScaleSystem {
    pub const fn resolve(&self, token: SpacingToken) -> f32 {
        match token {
            SpacingToken::S0 => self.s0,
            SpacingToken::S1 => self.s1,
            SpacingToken::S2 => self.s2,
            SpacingToken::S3 => self.s3,
            SpacingToken::S4 => self.s4,
            SpacingToken::S5 => self.s5,
            SpacingToken::S6 => self.s6,
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

/// Active density profile in the studio (`Comfortable`, `Compact`, `Classic Flash`).
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
        match self {
            Self::Comfortable => "Comfortable",
            Self::Compact => "Compact",
            Self::ClassicFlash => "Classic Flash",
        }
    }
}

/// Geometry metrics for one density profile.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
    pub const fn comfortable() -> Self {
        Self {
            control_height: 32.0,
            row_height: 28.0,
            layer_row_height: 28.0,
            section_header_height: 32.0,
            panel_header_height: 40.0,
            icon_grid_pt: 20.0,
            inspector_width: 320.0,
            tools_rail_width: 56.0,
            tools_two_col_width: 80.0,
        }
    }

    pub const fn compact() -> Self {
        Self {
            control_height: 28.0,
            row_height: 24.0,
            layer_row_height: 22.0,
            section_header_height: 28.0,
            panel_header_height: 32.0,
            icon_grid_pt: 16.0,
            inspector_width: 288.0,
            tools_rail_width: 48.0,
            tools_two_col_width: 68.0,
        }
    }

    pub const fn classic_flash() -> Self {
        Self {
            control_height: 24.0,
            row_height: 20.0,
            layer_row_height: 16.0,
            section_header_height: 24.0,
            panel_header_height: 28.0,
            icon_grid_pt: 12.0,
            inspector_width: 256.0,
            tools_rail_width: 40.0,
            tools_two_col_width: 56.0,
        }
    }
}

/// System-wide density profiles and active selection.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DensitySystem {
    pub active_profile: StudioDensityProfile,
    pub comfortable: DensityProfileMetrics,
    pub compact: DensityProfileMetrics,
    pub classic_flash: DensityProfileMetrics,
}

impl Default for DensitySystem {
    fn default() -> Self {
        Self {
            active_profile: StudioDensityProfile::Comfortable,
            comfortable: DensityProfileMetrics::comfortable(),
            compact: DensityProfileMetrics::compact(),
            classic_flash: DensityProfileMetrics::classic_flash(),
        }
    }
}

impl DensitySystem {
    pub const fn active(&self) -> &DensityProfileMetrics {
        match self.active_profile {
            StudioDensityProfile::Comfortable => &self.comfortable,
            StudioDensityProfile::Compact => &self.compact,
            StudioDensityProfile::ClassicFlash => &self.classic_flash,
        }
    }

    pub fn active_mut(&mut self) -> &mut DensityProfileMetrics {
        match self.active_profile {
            StudioDensityProfile::Comfortable => &mut self.comfortable,
            StudioDensityProfile::Compact => &mut self.compact,
            StudioDensityProfile::ClassicFlash => &mut self.classic_flash,
        }
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
            label_width_px: 76.0,
            stack_below_width: 196.0,
        }
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
            height: 32.0,
            padding_x: SpacingValue::Token(SpacingToken::S2),
            body_gap: SpacingValue::Token(SpacingToken::S2),
            text_style: StudioTextStyle::Overline,
            collapsible: true,
        }
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
            height: 40.0,
            tab_height: 36.0,
            padding_x: SpacingValue::Token(SpacingToken::S3),
            body_padding: SpacingValue::Token(SpacingToken::S4),
            text_style: StudioTextStyle::SmallStrong,
            show_menu_button: true,
        }
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
            button_side: 32.0,
            gap: 2.0,
            padding: 7.0,
        }
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
            layer_row_height_px: 24.0,
            ruler_height_px: 22.0,
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
            layer_list_width: UiSizeUnit::Px(220.0),
            layer_indent_px: 14.0,
            layer_icon_gap_px: 6.0,
        }
    }
}

impl TimelineGeometrySystem {
    /// Resolve the left layer-tree column width in pixels for a given total timeline width.
    pub fn resolve_layer_list_width(&self, total_timeline_width: f32) -> f32 {
        let raw = match self.layer_list_width {
            UiSizeUnit::Px(px) => px,
            UiSizeUnit::Percent(pct) => total_timeline_width * (pct / 100.0),
            UiSizeUnit::Auto => 220.0,
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
            top_menubar_height_px: 26.0,
            doc_tabs_height_px: 28.0,
            edit_bar_height_px: 24.0,
            shell_left_tools_width: UiSizeUnit::Px(46.0),
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
        let menu_h = self.top_menubar_height_px.clamp(18.0, 44.0);
        let tabs_h = self.doc_tabs_height_px.clamp(20.0, 44.0);
        let edit_h = self.edit_bar_height_px.clamp(18.0, 40.0);

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
            UiSizeUnit::Px(px) => px,
            UiSizeUnit::Percent(pct) => total_w * (pct / 100.0),
            UiSizeUnit::Auto => 46.0,
            UiSizeUnit::Fill(_) => 46.0,
        };
        let left_w = raw_left_w.clamp(32.0, (total_w * 0.25).max(36.0));

        let raw_right_w = match self.shell_right_inspector_width {
            UiSizeUnit::Px(px) => px,
            UiSizeUnit::Percent(pct) => total_w * (pct / 100.0),
            UiSizeUnit::Auto => 240.0,
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
            stage_ruler_thickness_px: 18.0,
            guide_stroke_width_px: 1.0,
            selection_handle_size_px: 6.0,
            stage_aspect_w: 550.0,
            stage_aspect_h: 400.0,
            show_rulers: true,
            show_guides: true,
        }
    }
}

/// Dark & Light semantic palette token overrides.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaletteTokenOverrides {
    pub dark: BTreeMap<StudioColorRole, [u8; 4]>,
    pub light: BTreeMap<StudioColorRole, [u8; 4]>,
}

impl PaletteTokenOverrides {
    pub fn resolve_color(&self, dark_mode: bool, role: StudioColorRole) -> Color32 {
        let map = if dark_mode { &self.dark } else { &self.light };
        if let Some(&[r, g, b, a]) = map.get(&role) {
            return Color32::from_rgba_unmultiplied(r, g, b, a);
        }
        let base = if dark_mode { &DARK } else { &LIGHT };
        role.read_from_palette(base)
    }

    pub fn set_color_override(&mut self, dark_mode: bool, role: StudioColorRole, color: Color32) {
        let map = if dark_mode {
            &mut self.dark
        } else {
            &mut self.light
        };
        map.insert(role, color.to_srgba_unmultiplied());
    }

    pub fn reset_color_override(&mut self, dark_mode: bool, role: StudioColorRole) {
        let map = if dark_mode {
            &mut self.dark
        } else {
            &mut self.light
        };
        map.remove(&role);
    }

    pub fn has_override(&self, dark_mode: bool, role: StudioColorRole) -> bool {
        let map = if dark_mode { &self.dark } else { &self.light };
        map.contains_key(&role)
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

impl UiStudioSystems {
    /// Reset every system scale and shared layout system back to NF2 reference defaults.
    pub fn reset_all_to_defaults(&mut self) {
        *self = Self::default();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Node Tree & Structural Operations (Wrap / Unwrap / Grids / Reorder)
// ─────────────────────────────────────────────────────────────────────────────

/// Button style variant inside the studio widget tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioButtonKind {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

impl StudioButtonKind {
    pub const ALL: [Self; 4] = [Self::Primary, Self::Secondary, Self::Ghost, Self::Danger];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
            Self::Ghost => "Ghost",
            Self::Danger => "Danger",
        }
    }

    pub const fn to_nf2(self) -> c::ButtonType {
        match self {
            Self::Primary => c::ButtonType::Primary,
            Self::Secondary => c::ButtonType::Secondary,
            Self::Ghost => c::ButtonType::Ghost,
            Self::Danger => c::ButtonType::Danger,
        }
    }
}

/// Container and leaf widget kinds supported by the UI Layout Studio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiNodeKind {
    // ── Containers ──
    /// Root or nested panel surface with optional header tabs.
    Panel {
        tabs: Vec<String>,
        active_tab: usize,
    },
    /// Collapsible or static section group (`NF2/Section Header` + body).
    Section { collapsible: bool, expanded: bool },
    /// Horizontal flex/track container (`Row`).
    Row,
    /// Vertical stack container (`Column`).
    Column,
    /// Multi-column grid container (`2`, `3`, `4+` columns in `%`, `px`, `Auto`, or `Fill`).
    /// Any cell (child) inside a `Grid` can itself be another `Grid`, `Row`, or `Column`!
    Grid { columns: Vec<UiSizeUnit> },

    // ── Leaf Widgets ──
    /// Numeric field (`NF2/Field` / `NF2/Stepper Field`).
    Field {
        value: f64,
        prefix: String,
        unit: String,
        decimals: usize,
        stepper: bool,
    },
    /// Text input (`NF2/TextField`).
    TextField {
        text: String,
        prefix: String,
        hint: String,
        mono: bool,
    },
    /// Dropdown select (`NF2/Select`).
    Select {
        selected: usize,
        options: Vec<String>,
    },
    /// Action button (`NF2/Button`).
    Button {
        button_kind: StudioButtonKind,
        icon_name: Option<String>,
    },
    /// Square icon button (`NF2/Icon Button`).
    IconButton {
        icon_name: String,
        selected: bool,
        md_size: bool,
    },
    /// Boolean checkbox (`NF2/Checkbox`).
    Checkbox { checked: bool },
    /// Immediate toggle switch (`NF2/Switch`).
    Switch { on: bool },
    /// Color well / swatch chip (`NF2/Color Well` / `NF2/Swatch`).
    ColorSwatch { rgba: [u8; 4], is_stroke: bool },
    /// Numeric slider (`NF2/Slider`).
    Slider {
        value: f64,
        min: f64,
        max: f64,
        show_field: bool,
    },
    /// Text label (`NF2/*` typography).
    Label,
    /// Horizontal or vertical divider line (`NF2/Divider`).
    Separator { vertical: bool },
    /// Multi-column table row (`NF2/Table Row`).
    TableRow {
        cells: Vec<String>,
        column_fractions: Vec<f32>,
        is_header: bool,
        selected: bool,
    },
}

impl UiNodeKind {
    pub const fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Panel { .. }
                | Self::Section { .. }
                | Self::Row
                | Self::Column
                | Self::Grid { .. }
        )
    }

    pub const fn type_badge(&self) -> &'static str {
        match self {
            Self::Panel { .. } => "Panel",
            Self::Section { .. } => "Section",
            Self::Row => "Row",
            Self::Column => "Column",
            Self::Grid { .. } => "Grid",
            Self::Field { .. } => "Field",
            Self::TextField { .. } => "TextField",
            Self::Select { .. } => "Select",
            Self::Button { .. } => "Button",
            Self::IconButton { .. } => "IconButton",
            Self::Checkbox { .. } => "Checkbox",
            Self::Switch { .. } => "Switch",
            Self::ColorSwatch { .. } => "ColorSwatch",
            Self::Slider { .. } => "Slider",
            Self::Label => "Label",
            Self::Separator { .. } => "Separator",
            Self::TableRow { .. } => "TableRow",
        }
    }

    pub fn default_text_style(&self) -> StudioTextStyle {
        match self {
            Self::Panel { .. } => StudioTextStyle::SmallStrong,
            Self::Section { .. } => StudioTextStyle::Overline,
            Self::Field { .. } | Self::ColorSwatch { .. } => StudioTextStyle::Mono,
            Self::TableRow {
                is_header: true, ..
            } => StudioTextStyle::Overline,
            Self::TableRow {
                is_header: false, ..
            } => StudioTextStyle::Small,
            _ => StudioTextStyle::Body,
        }
    }
}

/// One node in the UI Layout Studio hierarchy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiStudioNode {
    pub id: UiNodeId,
    pub label: String,
    pub kind: UiNodeKind,
    pub box_constraints: UiBoxConstraints,
    pub style: NodeVisualStyle,
    pub visible: bool,
    pub enabled: bool,
    pub children: Vec<UiStudioNode>,
}

impl UiStudioNode {
    pub fn new(id: UiNodeId, label: impl Into<String>, kind: UiNodeKind) -> Self {
        let default_text = kind.default_text_style();
        let mut style = NodeVisualStyle {
            typography: TypographyBinding::System(default_text),
            ..Default::default()
        };
        let box_constraints = match &kind {
            UiNodeKind::Panel { .. } => {
                UiBoxConstraints::new(UiSizeUnit::Px(320.0), UiSizeUnit::Auto)
            }
            UiNodeKind::Section { .. } => {
                style.system_binding = Some(SharedSystemKind::SectionHeader);
                style.gap = GapPair::both_token(SpacingToken::S2);
                UiBoxConstraints::percent_width(100.0)
            }
            UiNodeKind::Row | UiNodeKind::Column | UiNodeKind::Grid { .. } => {
                style.gap = GapPair::both_token(SpacingToken::S2);
                UiBoxConstraints::percent_width(100.0)
            }
            UiNodeKind::IconButton { .. } => {
                UiBoxConstraints::new(UiSizeUnit::Px(32.0), UiSizeUnit::Auto)
            }
            UiNodeKind::ColorSwatch { .. } => {
                UiBoxConstraints::new(UiSizeUnit::Px(74.0), UiSizeUnit::Auto)
            }
            UiNodeKind::Button { .. } => UiBoxConstraints::new(UiSizeUnit::Auto, UiSizeUnit::Auto),
            UiNodeKind::Separator { vertical: false } => {
                UiBoxConstraints::new(UiSizeUnit::Percent(100.0), UiSizeUnit::Px(1.0))
            }
            UiNodeKind::Separator { vertical: true } => {
                UiBoxConstraints::new(UiSizeUnit::Px(1.0), UiSizeUnit::Px(20.0))
            }
            _ => UiBoxConstraints::percent_width(100.0),
        };
        Self {
            id,
            label: label.into(),
            kind,
            box_constraints,
            style,
            visible: true,
            enabled: true,
            children: Vec::new(),
        }
    }

    pub fn with_width(mut self, width: UiSizeUnit) -> Self {
        self.box_constraints.width = width;
        self
    }

    pub fn with_height(mut self, height: UiSizeUnit) -> Self {
        self.box_constraints.height = height;
        self
    }

    pub fn with_padding(mut self, padding: Padding) -> Self {
        self.style.padding = padding;
        self
    }

    pub fn with_gap(mut self, gap: GapPair) -> Self {
        self.style.gap = gap;
        self
    }

    pub fn with_system_binding(mut self, sys: SharedSystemKind) -> Self {
        self.style.system_binding = Some(sys);
        self
    }

    pub fn with_children(mut self, children: Vec<UiStudioNode>) -> Self {
        self.children = children;
        self
    }
}

/// 1-click container presets for wrapping selected elements or inserting new containers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ContainerWrapPreset {
    /// Horizontal `Row`.
    Row,
    /// Vertical `Column`.
    Column,
    /// 2-column equal percentage grid (`[50%, 50%]`).
    Grid2Equal,
    /// 3-column equal percentage grid (`[33.33%, 33.33%, 33.34%]`).
    Grid3Equal,
    /// 4-column equal percentage grid (`[25%, 25%, 25%, 25%]`).
    Grid4Equal,
    /// Label column in `px` + 2 equal `%` columns (`[120px, 50%, 50%]`).
    GridMixedLabel2Col,
    /// Custom column tracks.
    GridCustom(Vec<UiSizeUnit>),
    /// Collapsible `Section`.
    Section,
}

impl ContainerWrapPreset {
    pub fn to_node_kind(&self) -> (String, UiNodeKind, Option<SharedSystemKind>) {
        match self {
            Self::Row => ("Row".to_owned(), UiNodeKind::Row, None),
            Self::Column => ("Column".to_owned(), UiNodeKind::Column, None),
            Self::Grid2Equal => (
                "Grid (2 Cols)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                },
                Some(SharedSystemKind::TwoColumnForm),
            ),
            Self::Grid3Equal => (
                "Grid (3 Cols)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![
                        UiSizeUnit::Percent(33.3333),
                        UiSizeUnit::Percent(33.3333),
                        UiSizeUnit::Percent(33.3334),
                    ],
                },
                None,
            ),
            Self::Grid4Equal => (
                "Grid (4 Cols)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![
                        UiSizeUnit::Percent(25.0),
                        UiSizeUnit::Percent(25.0),
                        UiSizeUnit::Percent(25.0),
                        UiSizeUnit::Percent(25.0),
                    ],
                },
                None,
            ),
            Self::GridMixedLabel2Col => (
                "Grid (120px + 50% + 50%)".to_owned(),
                UiNodeKind::Grid {
                    columns: vec![
                        UiSizeUnit::Px(120.0),
                        UiSizeUnit::Percent(50.0),
                        UiSizeUnit::Percent(50.0),
                    ],
                },
                None,
            ),
            Self::GridCustom(cols) => (
                format!("Grid ({} Cols)", cols.len()),
                UiNodeKind::Grid {
                    columns: cols.clone(),
                },
                None,
            ),
            Self::Section => (
                "Section".to_owned(),
                UiNodeKind::Section {
                    collapsible: true,
                    expanded: true,
                },
                Some(SharedSystemKind::SectionHeader),
            ),
        }
    }
}

/// Authored UI Layout Studio document (panel tree + next ID generator).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiStudioDocument {
    pub name: String,
    pub root: UiStudioNode,
    pub next_id: UiNodeId,
}

impl Default for UiStudioDocument {
    fn default() -> Self {
        StudioPanelPreset::PropertiesMovieClip.build_document()
    }
}

impl UiStudioDocument {
    pub fn alloc_id(&mut self) -> UiNodeId {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn all_node_ids(&self) -> Vec<UiNodeId> {
        fn visit(node: &UiStudioNode, out: &mut Vec<UiNodeId>) {
            out.push(node.id);
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, &mut out);
        out
    }

    pub fn find_node(&self, id: UiNodeId) -> Option<&UiStudioNode> {
        fn visit(node: &UiStudioNode, id: UiNodeId) -> Option<&UiStudioNode> {
            if node.id == id {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = visit(child, id) {
                    return Some(found);
                }
            }
            None
        }
        visit(&self.root, id)
    }

    pub fn find_node_mut(&mut self, id: UiNodeId) -> Option<&mut UiStudioNode> {
        fn visit(node: &mut UiStudioNode, id: UiNodeId) -> Option<&mut UiStudioNode> {
            if node.id == id {
                return Some(node);
            }
            for child in &mut node.children {
                if let Some(found) = visit(child, id) {
                    return Some(found);
                }
            }
            None
        }
        visit(&mut self.root, id)
    }

    pub fn find_parent_id(&self, target_id: UiNodeId) -> Option<UiNodeId> {
        fn visit(node: &UiStudioNode, target_id: UiNodeId) -> Option<UiNodeId> {
            for child in &node.children {
                if child.id == target_id {
                    return Some(node.id);
                }
                if let Some(p) = visit(child, target_id) {
                    return Some(p);
                }
            }
            None
        }
        visit(&self.root, target_id)
    }

    /// Returns `true` if `candidate_id` is inside `ancestor_id`'s subtree (including `ancestor_id` itself).
    pub fn is_descendant(&self, ancestor_id: UiNodeId, candidate_id: UiNodeId) -> bool {
        let Some(ancestor) = self.find_node(ancestor_id) else {
            return false;
        };
        fn visit(node: &UiStudioNode, candidate_id: UiNodeId) -> bool {
            if node.id == candidate_id {
                return true;
            }
            node.children.iter().any(|c| visit(c, candidate_id))
        }
        visit(ancestor, candidate_id)
    }

    /// Wrap 1 or more selected nodes into a new container (`Row`, `Column`, `Grid(2/3/4)`, or `Section`).
    ///
    /// Selected nodes that belong to the first selected node's parent are extracted in their original
    /// sibling order and placed inside the newly created container at the position of the first selected node.
    pub fn wrap_selected_nodes(
        &mut self,
        selected_ids: &[UiNodeId],
        preset: ContainerWrapPreset,
    ) -> Option<UiNodeId> {
        let first_id = *selected_ids
            .iter()
            .find(|&&id| id != self.root.id && self.find_parent_id(id).is_some())?;
        let parent_id = self.find_parent_id(first_id)?;
        let wrapper_id = self.alloc_id();
        let (label, kind, sys_binding) = preset.to_node_kind();
        let mut wrapper = UiStudioNode::new(wrapper_id, label, kind);
        if let Some(sys) = sys_binding {
            wrapper.style.system_binding = Some(sys);
        }

        let parent = self.find_node_mut(parent_id)?;
        let first_pos = parent.children.iter().position(|c| c.id == first_id)?;

        let mut extracted = Vec::new();
        let mut idx = 0;
        while idx < parent.children.len() {
            if selected_ids.contains(&parent.children[idx].id) {
                extracted.push(parent.children.remove(idx));
            } else {
                idx += 1;
            }
        }
        if extracted.is_empty() {
            return None;
        }
        wrapper.children = extracted;
        let insert_at = first_pos.min(parent.children.len());
        parent.children.insert(insert_at, wrapper);
        Some(wrapper_id)
    }

    /// Unwrap (`dissolve`) a container (`Row`, `Column`, `Grid`, `Section`), splicing all of its
    /// children directly into its parent container at the exact position of `container_id`.
    pub fn unwrap_selected_container(&mut self, container_id: UiNodeId) -> bool {
        if container_id == self.root.id {
            return false;
        }
        let Some(parent_id) = self.find_parent_id(container_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(pos) = parent.children.iter().position(|c| c.id == container_id) else {
            return false;
        };
        if !parent.children[pos].kind.is_container() {
            return false;
        }
        let removed = parent.children.remove(pos);
        for (offset, child) in removed.children.into_iter().enumerate() {
            parent.children.insert(pos + offset, child);
        }
        true
    }

    /// Extract a single child node out of its parent container (`Row`/`Column`/`Grid`/`Section`)
    /// into the grandparent container right after the parent container.
    pub fn extract_node_to_parent(&mut self, node_id: UiNodeId) -> bool {
        if node_id == self.root.id {
            return false;
        }
        let Some(parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        if parent_id == self.root.id {
            return false;
        }
        let Some(grandparent_id) = self.find_parent_id(parent_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(child_pos) = parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        let extracted = parent.children.remove(child_pos);

        let Some(grandparent) = self.find_node_mut(grandparent_id) else {
            return false;
        };
        let insert_pos = grandparent
            .children
            .iter()
            .position(|c| c.id == parent_id)
            .map_or(grandparent.children.len(), |p| p + 1);
        grandparent.children.insert(insert_pos, extracted);
        true
    }

    /// Insert a new node inside `parent_id` (or inside `parent_id`'s parent if `parent_id` is a leaf).
    pub fn insert_node(
        &mut self,
        target_id: UiNodeId,
        label: impl Into<String>,
        kind: UiNodeKind,
    ) -> Option<UiNodeId> {
        let new_id = self.alloc_id();
        let node = UiStudioNode::new(new_id, label, kind);
        let is_container = self
            .find_node(target_id)
            .is_some_and(|n| n.kind.is_container());
        if is_container {
            let parent = self.find_node_mut(target_id)?;
            parent.children.push(node);
            Some(new_id)
        } else if let Some(parent_id) = self.find_parent_id(target_id) {
            let parent = self.find_node_mut(parent_id)?;
            let pos = parent
                .children
                .iter()
                .position(|c| c.id == target_id)
                .map_or(parent.children.len(), |p| p + 1);
            parent.children.insert(pos, node);
            Some(new_id)
        } else {
            self.root.children.push(node);
            Some(new_id)
        }
    }

    /// Insert a nested `Grid` (e.g. 2, 3, or 4 columns) inside `target_container_id` (such as a cell
    /// of an outer `Row` or `Grid`), optionally pre-populating sample `Field` cells.
    pub fn insert_nested_grid(
        &mut self,
        target_container_id: UiNodeId,
        columns: Vec<UiSizeUnit>,
        populate_sample_cells: bool,
    ) -> Option<UiNodeId> {
        let col_count = columns.len().max(1);
        let grid_id = self.insert_node(
            target_container_id,
            format!("Nested Grid ({col_count} Cols)"),
            UiNodeKind::Grid { columns },
        )?;
        if populate_sample_cells {
            for col_idx in 0..col_count {
                let cell_id = self.alloc_id();
                let cell = UiStudioNode::new(
                    cell_id,
                    format!("Cell {}", col_idx + 1),
                    UiNodeKind::Field {
                        value: (col_idx + 1) as f64 * 10.0,
                        prefix: format!("C{}", col_idx + 1),
                        unit: "px".to_owned(),
                        decimals: 0,
                        stepper: false,
                    },
                );
                if let Some(grid_node) = self.find_node_mut(grid_id) {
                    grid_node.children.push(cell);
                }
            }
        }
        Some(grid_id)
    }

    /// Deep-clone `node_id` (regenerating fresh IDs for the entire cloned subtree) and insert right after `node_id`.
    pub fn duplicate_node(&mut self, node_id: UiNodeId) -> Option<UiNodeId> {
        if node_id == self.root.id {
            return None;
        }
        let parent_id = self.find_parent_id(node_id)?;
        let original = self.find_node(node_id)?.clone();

        fn clone_with_fresh_ids(
            doc: &mut UiStudioDocument,
            mut node: UiStudioNode,
            is_top: bool,
        ) -> UiStudioNode {
            node.id = doc.alloc_id();
            if is_top {
                node.label = format!("{} Copy", node.label);
            }
            node.children = node
                .children
                .into_iter()
                .map(|c| clone_with_fresh_ids(doc, c, false))
                .collect();
            node
        }

        let cloned = clone_with_fresh_ids(self, original, true);
        let new_id = cloned.id;
        let parent = self.find_node_mut(parent_id)?;
        let pos = parent
            .children
            .iter()
            .position(|c| c.id == node_id)
            .map_or(parent.children.len(), |p| p + 1);
        parent.children.insert(pos, cloned);
        Some(new_id)
    }

    /// Delete all specified nodes (never deletes the root `Panel`). Returns count of deleted top-level targets.
    pub fn delete_nodes(&mut self, ids: &[UiNodeId]) -> usize {
        fn remove_rec(node: &mut UiStudioNode, ids: &[UiNodeId]) -> usize {
            let mut removed = 0;
            let mut i = 0;
            while i < node.children.len() {
                if ids.contains(&node.children[i].id) {
                    node.children.remove(i);
                    removed += 1;
                } else {
                    removed += remove_rec(&mut node.children[i], ids);
                    i += 1;
                }
            }
            removed
        }
        remove_rec(&mut self.root, ids)
    }

    /// Move `node_id` one slot earlier (`↑`) inside its parent container.
    pub fn move_node_up(&mut self, node_id: UiNodeId) -> bool {
        let Some(parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(pos) = parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        if pos == 0 {
            return false;
        }
        parent.children.swap(pos - 1, pos);
        true
    }

    /// Move `node_id` one slot later (`↓`) inside its parent container.
    pub fn move_node_down(&mut self, node_id: UiNodeId) -> bool {
        let Some(parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        let Some(parent) = self.find_node_mut(parent_id) else {
            return false;
        };
        let Some(pos) = parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        if pos + 1 >= parent.children.len() {
            return false;
        }
        parent.children.swap(pos, pos + 1);
        true
    }

    /// Move `node_id` into `new_parent_id` at `insert_index` (or at the end if `None`),
    /// rejecting invalid moves where `new_parent_id` is not a container or is inside `node_id`.
    pub fn reparent_node(
        &mut self,
        node_id: UiNodeId,
        new_parent_id: UiNodeId,
        insert_index: Option<usize>,
    ) -> bool {
        if node_id == self.root.id || self.is_descendant(node_id, new_parent_id) {
            return false;
        }
        if !self
            .find_node(new_parent_id)
            .is_some_and(|n| n.kind.is_container())
        {
            return false;
        }
        let Some(old_parent_id) = self.find_parent_id(node_id) else {
            return false;
        };
        let Some(old_parent) = self.find_node_mut(old_parent_id) else {
            return false;
        };
        let Some(old_pos) = old_parent.children.iter().position(|c| c.id == node_id) else {
            return false;
        };
        let extracted = old_parent.children.remove(old_pos);

        let Some(new_parent) = self.find_node_mut(new_parent_id) else {
            return false;
        };
        let idx = insert_index
            .unwrap_or(new_parent.children.len())
            .min(new_parent.children.len());
        new_parent.children.insert(idx, extracted);
        true
    }

    /// Reset a node's local style overrides back to NF2 system tokens.
    pub fn reset_node_to_system_defaults(&mut self, node_id: UiNodeId) -> bool {
        let Some(node) = self.find_node_mut(node_id) else {
            return false;
        };
        let default_text = node.kind.default_text_style();
        node.style.reset_to_system_tokens(default_text);
        true
    }
}

/// Snapshot entry in [`UiStudioHistory`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiStudioSnapshot {
    pub label: String,
    pub document: UiStudioDocument,
    pub systems: UiStudioSystems,
}

/// Multi-step Undo/Redo history stack for the UI Layout Studio.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiStudioHistory {
    pub undo_stack: Vec<UiStudioSnapshot>,
    pub redo_stack: Vec<UiStudioSnapshot>,
    pub max_entries: usize,
}

impl UiStudioHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_entries: max_entries.max(8),
        }
    }

    pub fn push_snapshot(
        &mut self,
        label: impl Into<String>,
        document: &UiStudioDocument,
        systems: &UiStudioSystems,
    ) {
        let cap = if self.max_entries == 0 {
            64
        } else {
            self.max_entries
        };
        if self.undo_stack.len() >= cap {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(UiStudioSnapshot {
            label: label.into(),
            document: document.clone(),
            systems: systems.clone(),
        });
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(
        &mut self,
        current_doc: &mut UiStudioDocument,
        current_systems: &mut UiStudioSystems,
    ) -> Option<String> {
        let prev = self.undo_stack.pop()?;
        let label = prev.label.clone();
        self.redo_stack.push(UiStudioSnapshot {
            label: label.clone(),
            document: current_doc.clone(),
            systems: current_systems.clone(),
        });
        *current_doc = prev.document;
        *current_systems = prev.systems;
        Some(label)
    }

    pub fn redo(
        &mut self,
        current_doc: &mut UiStudioDocument,
        current_systems: &mut UiStudioSystems,
    ) -> Option<String> {
        let next = self.redo_stack.pop()?;
        let label = next.label.clone();
        self.undo_stack.push(UiStudioSnapshot {
            label: label.clone(),
            document: current_doc.clone(),
            systems: current_systems.clone(),
        });
        *current_doc = next.document;
        *current_systems = next.systems;
        Some(label)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Pure Deterministic Box & Grid Layout Engine (`compute_studio_layout`)
// ─────────────────────────────────────────────────────────────────────────────

/// Resolved layout box and styling metrics for a single [`UiStudioNode`].
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedNodeBox {
    pub id: UiNodeId,
    pub rect: Rect,
    pub content_rect: Rect,
    pub resolved_width: f32,
    pub resolved_height: f32,
    pub padding: [f32; 4],
    pub gap: Vec2,
    pub typography: TypographyOverride,
}

/// Complete layout solution for a [`UiStudioDocument`] at a given canvas origin and viewport size.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStudioLayout {
    pub boxes: BTreeMap<UiNodeId, ComputedNodeBox>,
    pub order: Vec<UiNodeId>,
    pub root_rect: Rect,
}

impl Default for ComputedStudioLayout {
    fn default() -> Self {
        Self {
            boxes: BTreeMap::new(),
            order: Vec::new(),
            root_rect: Rect::NOTHING,
        }
    }
}

impl ComputedStudioLayout {
    pub fn node_box(&self, id: UiNodeId) -> Option<&ComputedNodeBox> {
        self.boxes.get(&id)
    }

    /// Hit-test the deepest visible node containing `pos`.
    pub fn hit_test(&self, pos: Pos2) -> Option<UiNodeId> {
        for &id in self.order.iter().rev() {
            if let Some(b) = self.boxes.get(&id)
                && b.rect.contains(pos)
            {
                return Some(id);
            }
        }
        None
    }
}

/// Intrinsic default height of a leaf widget given the active [`UiStudioSystems`].
pub fn intrinsic_leaf_height(
    kind: &UiNodeKind,
    systems: &UiStudioSystems,
    typo: &TypographyOverride,
) -> f32 {
    let density = systems.density.active();
    match kind {
        UiNodeKind::Field { .. }
        | UiNodeKind::TextField { .. }
        | UiNodeKind::Select { .. }
        | UiNodeKind::Button { .. }
        | UiNodeKind::ColorSwatch { .. } => density.control_height,
        UiNodeKind::IconButton { md_size: true, .. } => (density.control_height + 8.0).max(28.0),
        UiNodeKind::IconButton { md_size: false, .. } => {
            if systems.tool_rail.button_side > 0.0 {
                density.control_height
            } else {
                32.0
            }
        }
        UiNodeKind::Checkbox { .. } | UiNodeKind::Switch { .. } | UiNodeKind::Slider { .. } => {
            density.row_height.max(20.0)
        }
        UiNodeKind::Label => typo.line_height_pt.max(typo.size_pt + 4.0).max(16.0),
        UiNodeKind::Separator { vertical: false } => 1.0,
        UiNodeKind::Separator { vertical: true } => 20.0,
        UiNodeKind::TableRow { .. } => density.row_height,
        UiNodeKind::Panel { .. }
        | UiNodeKind::Section { .. }
        | UiNodeKind::Row
        | UiNodeKind::Column
        | UiNodeKind::Grid { .. } => density.control_height,
    }
}

/// Intrinsic default width of a leaf widget when `UiSizeUnit::Auto` is used.
pub fn intrinsic_leaf_width(
    node: &UiStudioNode,
    systems: &UiStudioSystems,
    typo: &TypographyOverride,
) -> f32 {
    let density = systems.density.active();
    match &node.kind {
        UiNodeKind::IconButton { md_size: true, .. } => (density.control_height + 8.0).max(28.0),
        UiNodeKind::IconButton { md_size: false, .. } => density.control_height,
        UiNodeKind::ColorSwatch { .. } => 74.0,
        UiNodeKind::Button { .. } => {
            let char_w = (typo.size_pt * 0.58).max(5.5);
            (node.label.chars().count() as f32 * char_w + 28.0).max(64.0)
        }
        UiNodeKind::Checkbox { .. } | UiNodeKind::Switch { .. } => {
            let char_w = (typo.size_pt * 0.58).max(5.5);
            (node.label.chars().count() as f32 * char_w + 36.0).max(64.0)
        }
        UiNodeKind::Label => {
            let char_w = (typo.size_pt * 0.58).max(5.5);
            (node.label.chars().count() as f32 * char_w + 8.0).max(32.0)
        }
        UiNodeKind::Separator { vertical: true } => 1.0,
        _ => 120.0,
    }
}

/// Resolve effective `[top, right, bottom, left]` padding and `[gap_x, gap_y]` for `node`,
/// honoring `node.style.system_binding` unless locally overridden.
pub fn resolve_node_spacing(node: &UiStudioNode, systems: &UiStudioSystems) -> ([f32; 4], Vec2) {
    let mut pad = node.style.padding.resolve(systems);
    let mut gap = node.style.gap.resolve(systems);

    match node.style.system_binding {
        Some(SharedSystemKind::TwoColumnForm) => {
            if !node.style.gap.x.is_override() {
                gap.x = systems.two_column_form.gap.resolve(systems);
            }
            if !node.style.gap.y.is_override() {
                gap.y = systems.two_column_form.gap.resolve(systems);
            }
        }
        Some(SharedSystemKind::SectionHeader) => {
            if !node.style.gap.y.is_override() {
                gap.y = systems.section_header.body_gap.resolve(systems);
            }
        }
        Some(SharedSystemKind::PanelHeader) => {
            if pad == [0.0, 0.0, 0.0, 0.0] {
                let p = systems.panel_header.body_padding.resolve(systems);
                pad = [p, p, p, p];
            }
        }
        Some(SharedSystemKind::ToolRail) => {
            if pad == [0.0, 0.0, 0.0, 0.0] {
                let p = systems.tool_rail.padding;
                pad = [p, p, p, p];
            }
            if !node.style.gap.x.is_override() {
                gap.x = systems.tool_rail.gap;
            }
            if !node.style.gap.y.is_override() {
                gap.y = systems.tool_rail.gap;
            }
        }
        None => {
            if matches!(node.kind, UiNodeKind::Panel { .. }) && pad == [0.0, 0.0, 0.0, 0.0] {
                let p = systems.panel_header.body_padding.resolve(systems);
                pad = [p, p, p, p];
            }
        }
    }

    (pad, gap)
}

/// Compute the deterministic layout (`ComputedStudioLayout`) for `doc` and `systems`.
pub fn compute_studio_layout(
    doc: &UiStudioDocument,
    systems: &UiStudioSystems,
    origin: Pos2,
    available_size: Vec2,
) -> ComputedStudioLayout {
    let mut out = ComputedStudioLayout::default();
    let root_rect = layout_node_recursive(
        &doc.root,
        systems,
        origin,
        available_size.x.max(64.0),
        available_size.y.max(64.0),
        None,
        &mut out,
    );
    out.root_rect = root_rect;
    out
}

fn layout_node_recursive(
    node: &UiStudioNode,
    systems: &UiStudioSystems,
    origin: Pos2,
    parent_content_width: f32,
    parent_content_height: f32,
    forced_width: Option<f32>,
    out: &mut ComputedStudioLayout,
) -> Rect {
    let mut typo = node.style.typography.resolve(systems);
    if !node.style.typography.is_custom() {
        match node.style.system_binding {
            Some(SharedSystemKind::SectionHeader) => {
                typo = systems
                    .typography
                    .resolve_style(systems.section_header.text_style);
            }
            Some(SharedSystemKind::PanelHeader) => {
                typo = systems
                    .typography
                    .resolve_style(systems.panel_header.text_style);
            }
            _ => {}
        }
    }

    let (pad, gap) = resolve_node_spacing(node, systems);
    let pad_h = pad[1] + pad[3];
    let pad_v = pad[0] + pad[2];

    let intrinsic_w = intrinsic_leaf_width(node, systems, &typo);
    let mut resolved_width = forced_width.unwrap_or_else(|| {
        node.box_constraints
            .resolve_width(parent_content_width, intrinsic_w)
    });
    if let Some(min_w) = node.box_constraints.min_width {
        resolved_width = resolved_width.max(min_w);
    }
    if let Some(max_w) = node.box_constraints.max_width {
        resolved_width = resolved_width.min(max_w);
    }

    let inner_width = (resolved_width - pad_h).max(0.0);
    let header_h = match &node.kind {
        UiNodeKind::Panel { tabs, .. } => {
            if tabs.is_empty() {
                0.0
            } else {
                systems.panel_header.height.max(24.0)
            }
        }
        UiNodeKind::Section { .. } => systems.section_header.height.max(20.0),
        _ => 0.0,
    };

    let content_origin = egui::pos2(origin.x + pad[3], origin.y + header_h + pad[0]);
    let node_order_idx = out.order.len();
    out.order.push(node.id);

    let children_height = match &node.kind {
        UiNodeKind::Panel { .. } | UiNodeKind::Column => layout_column_children(
            &node.children,
            systems,
            content_origin,
            inner_width,
            gap.y,
            out,
        ),
        UiNodeKind::Section { expanded, .. } => {
            if *expanded {
                layout_column_children(
                    &node.children,
                    systems,
                    content_origin,
                    inner_width,
                    gap.y,
                    out,
                )
            } else {
                0.0
            }
        }
        UiNodeKind::Row => layout_row_children(
            &node.children,
            systems,
            content_origin,
            inner_width,
            gap.x,
            out,
        ),
        UiNodeKind::Grid { columns } => {
            let effective_cols = if node.style.system_binding == Some(SharedSystemKind::ToolRail) {
                vec![
                    UiSizeUnit::Px(systems.tool_rail.button_side);
                    systems.tool_rail.columns.max(1)
                ]
            } else if columns.is_empty() {
                vec![UiSizeUnit::Percent(100.0)]
            } else {
                columns.clone()
            };
            layout_grid_children(
                &node.children,
                &effective_cols,
                systems,
                content_origin,
                inner_width,
                gap,
                out,
            )
        }
        _ => intrinsic_leaf_height(&node.kind, systems, &typo),
    };

    let auto_total_h = header_h + pad_v + children_height;
    let resolved_height = node
        .box_constraints
        .resolve_height(parent_content_height, auto_total_h);

    let rect = Rect::from_min_size(origin, egui::vec2(resolved_width, resolved_height));
    let content_rect = Rect::from_min_size(
        content_origin,
        egui::vec2(inner_width, (resolved_height - header_h - pad_v).max(0.0)),
    );

    let _ = node_order_idx;
    out.boxes.insert(
        node.id,
        ComputedNodeBox {
            id: node.id,
            rect,
            content_rect,
            resolved_width,
            resolved_height,
            padding: pad,
            gap,
            typography: typo,
        },
    );

    rect
}

fn layout_column_children(
    children: &[UiStudioNode],
    systems: &UiStudioSystems,
    origin: Pos2,
    inner_width: f32,
    gap_y: f32,
    out: &mut ComputedStudioLayout,
) -> f32 {
    let mut y = origin.y;
    let mut visible_count = 0usize;
    for child in children {
        if !child.visible {
            continue;
        }
        if visible_count > 0 {
            y += gap_y;
        }
        let child_rect = layout_node_recursive(
            child,
            systems,
            egui::pos2(origin.x, y),
            inner_width,
            0.0,
            None,
            out,
        );
        y = child_rect.bottom();
        visible_count += 1;
    }
    (y - origin.y).max(0.0)
}

fn layout_row_children(
    children: &[UiStudioNode],
    systems: &UiStudioSystems,
    origin: Pos2,
    inner_width: f32,
    gap_x: f32,
    out: &mut ComputedStudioLayout,
) -> f32 {
    let visible: Vec<&UiStudioNode> = children.iter().filter(|c| c.visible).collect();
    if visible.is_empty() {
        return 0.0;
    }
    let tracks: Vec<UiSizeUnit> = visible.iter().map(|c| c.box_constraints.width).collect();
    let intrinsics: Vec<f32> = visible
        .iter()
        .map(|c| {
            let t = c.style.typography.resolve(systems);
            intrinsic_leaf_width(c, systems, &t)
        })
        .collect();
    let widths = resolve_track_sizes(&tracks, inner_width, gap_x, &intrinsics);

    let mut x = origin.x;
    let mut max_h = 0.0_f32;
    for (i, child) in visible.into_iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(64.0);
        let r = layout_node_recursive(
            child,
            systems,
            egui::pos2(x, origin.y),
            inner_width,
            0.0,
            Some(w),
            out,
        );
        max_h = max_h.max(r.height());
        x += w + gap_x;
    }
    max_h
}

fn layout_grid_children(
    children: &[UiStudioNode],
    columns: &[UiSizeUnit],
    systems: &UiStudioSystems,
    origin: Pos2,
    inner_width: f32,
    gap: Vec2,
    out: &mut ComputedStudioLayout,
) -> f32 {
    let visible: Vec<&UiStudioNode> = children.iter().filter(|c| c.visible).collect();
    if visible.is_empty() || columns.is_empty() {
        return 0.0;
    }
    let col_count = columns.len().max(1);
    let intrinsics = vec![64.0_f32; col_count];
    let col_widths = resolve_track_sizes(columns, inner_width, gap.x, &intrinsics);

    let mut y = origin.y;
    for (row_idx, row_slice) in visible.chunks(col_count).enumerate() {
        if row_idx > 0 {
            y += gap.y;
        }
        let mut x = origin.x;
        let mut row_h = 0.0_f32;
        for (col_idx, child) in row_slice.iter().copied().enumerate() {
            let cell_w = col_widths.get(col_idx).copied().unwrap_or(64.0);
            // Inside a grid cell: if the child is 100% width, it fills `cell_w`; if it has a custom
            // `%` or `px` width, it resolves relative to `cell_w`!
            let child_w = child.box_constraints.resolve_width(
                cell_w,
                intrinsic_leaf_width(child, systems, &child.style.typography.resolve(systems)),
            );
            let r = layout_node_recursive(
                child,
                systems,
                egui::pos2(x, y),
                cell_w,
                0.0,
                Some(child_w),
                out,
            );
            row_h = row_h.max(r.height());
            x += cell_w + gap.x;
        }
        y += row_h;
    }
    (y - origin.y).max(0.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Preloaded Real NeoFlash Panel Presets + JSON & Rust Codegen Export
// ─────────────────────────────────────────────────────────────────────────────

/// Built-in templates modeling real NeoFlash surfaces, workspaces, timelines, and panels for immediate visual editing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioPanelPreset {
    #[default]
    PropertiesMovieClip,
    FullWorkspaceShell,
    TimelineStudio,
    LibraryAndAssetsStudio,
    ColorAndGradientStudio,
    AlignAndTransform,
    PropertiesText,
    ColorPanel,
    ToolsPanel,
    FiltersPanel,
    TimelineHeader,
}

impl StudioPanelPreset {
    pub const ALL: [Self; 11] = [
        Self::PropertiesMovieClip,
        Self::LibraryAndAssetsStudio,
        Self::ColorAndGradientStudio,
        Self::AlignAndTransform,
        Self::ToolsPanel,
        Self::TimelineStudio,
        Self::FullWorkspaceShell,
        Self::PropertiesText,
        Self::ColorPanel,
        Self::FiltersPanel,
        Self::TimelineHeader,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FullWorkspaceShell => "Full Editor Workspace — Live Shell",
            Self::TimelineStudio => "Timeline & Tween Geometry Studio",
            Self::LibraryAndAssetsStudio => "Library Split: Preview + Tree",
            Self::ColorAndGradientStudio => "Color Mixer, Gradient Bar & Swatches",
            Self::AlignAndTransform => "Align & Transform",
            Self::PropertiesMovieClip => "Properties — Movie Clip",
            Self::PropertiesText => "Properties — Text",
            Self::ColorPanel => "Color",
            Self::ToolsPanel => "Tools",
            Self::FiltersPanel => "Filters",
            Self::TimelineHeader => "Timeline Header",
        }
    }

    pub const fn is_full_shell(self) -> bool {
        matches!(self, Self::FullWorkspaceShell)
    }

    pub const fn is_timeline_surface(self) -> bool {
        matches!(self, Self::TimelineStudio)
    }

    pub fn build_document(self) -> UiStudioDocument {
        let mut next_id = 1_u64;
        let mut alloc = || {
            let id = next_id;
            next_id += 1;
            id
        };

        let root = match self {
            Self::FullWorkspaceShell => {
                let root_id = alloc(); // 1
                let top_bars_id = alloc(); // 2
                let left_tools_id = alloc(); // 3
                let center_stage_id = alloc(); // 4
                let right_dock_id = alloc(); // 5
                let bottom_timeline_id = alloc(); // 6
                let tl_layers_col_id = alloc(); // 7
                let tl_ruler_playhead_id = alloc(); // 8
                let tl_classic_tween_id = alloc(); // 9
                let tl_shape_tween_id = alloc(); // 10
                let tl_motion_tween_id = alloc(); // 11
                let tl_broken_tween_id = alloc(); // 12
                let tl_keyframes_badges_id = alloc(); // 13
                let tl_audio_waveform_id = alloc(); // 14

                UiStudioNode::new(
                    root_id,
                    "Full Editor Workspace — Live Shell",
                    UiNodeKind::Panel {
                        tabs: vec![
                            "Animator Workspace".to_owned(),
                            "Classic Flash".to_owned(),
                            "Developer".to_owned(),
                        ],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Percent(100.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        top_bars_id,
                        "Top Menu Bar + Document Tabs + Edit Breadcrumb Bar",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(78.0)),
                    UiStudioNode::new(
                        left_tools_id,
                        "Left Tools Dock Rail (1-Col / 2-Col)",
                        UiNodeKind::Column,
                    )
                    .with_width(UiSizeUnit::Px(46.0))
                    .with_system_binding(SharedSystemKind::ToolRail),
                    UiStudioNode::new(
                        center_stage_id,
                        "Center Stage Viewport (Rulers, Cyan Guides & Vector Instance)",
                        UiNodeKind::Section {
                            collapsible: false,
                            expanded: true,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(70.0)),
                    UiStudioNode::new(
                        right_dock_id,
                        "Right Multi-Panel Inspector Dock (Properties + Library + Color)",
                        UiNodeKind::Column,
                    )
                    .with_width(UiSizeUnit::Percent(25.0))
                    .with_system_binding(SharedSystemKind::TwoColumnForm),
                    UiStudioNode::new(
                        bottom_timeline_id,
                        "Bottom Timeline Dock (Layers + Frame Grid + Tweens)",
                        UiNodeKind::Section {
                            collapsible: false,
                            expanded: true,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Percent(34.0))
                    .with_children(vec![
                        UiStudioNode::new(
                            tl_layers_col_id,
                            "Layer Tree Column (Folder / Mask / Masked / Guide / Audio)",
                            UiNodeKind::Column,
                        )
                        .with_width(UiSizeUnit::Px(220.0)),
                        UiStudioNode::new(
                            tl_ruler_playhead_id,
                            "Frame Ruler, Red Playhead & Onion Skin Brackets",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(22.0)),
                        UiStudioNode::new(
                            tl_classic_tween_id,
                            "Classic Tween Span (Purple + Arrow ──────►)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_shape_tween_id,
                            "Shape Tween Span (Green + Arrow ──────►)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_motion_tween_id,
                            "Motion Tween Span (Blue + Property Diamonds ◆)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_broken_tween_id,
                            "Broken Tween Span (Dashed Line - - - - -)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_keyframes_badges_id,
                            "Keyframe Dots (●/○), Span End ([ ]), Label Flag & Action (a)",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                        UiStudioNode::new(
                            tl_audio_waveform_id,
                            "Soundtrack Audio Waveform Span",
                            UiNodeKind::Row,
                        )
                        .with_width(UiSizeUnit::Percent(100.0))
                        .with_height(UiSizeUnit::Px(24.0)),
                    ]),
                ])
            }
            Self::TimelineStudio => {
                let root_id = alloc(); // 1
                let bar_row = alloc(); // 2
                let layers_col_id = alloc(); // 3
                let ruler_playhead_id = alloc(); // 4
                let classic_tween_id = alloc(); // 5
                let shape_tween_id = alloc(); // 6
                let motion_tween_id = alloc(); // 7
                let broken_tween_id = alloc(); // 8
                let keyframes_badges_id = alloc(); // 9
                let audio_waveform_id = alloc(); // 10

                let new_layer_btn = alloc();
                let folder_btn = alloc();
                let trash_btn = alloc();
                let play_btn = alloc();
                let loop_btn = alloc();
                let onion_sw = alloc();
                let frame_fld = alloc();
                let fps_fld = alloc();

                UiStudioNode::new(
                    root_id,
                    "Timeline & Tween Geometry Studio",
                    UiNodeKind::Panel {
                        tabs: vec!["Timeline".to_owned(), "Motion Editor".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Percent(100.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(bar_row, "Timeline Header Toolbar", UiNodeKind::Row)
                        .with_children(vec![
                            UiStudioNode::new(
                                new_layer_btn,
                                "New Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "layerplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                folder_btn,
                                "New Folder",
                                UiNodeKind::IconButton {
                                    icon_name: "folderplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                trash_btn,
                                "Delete Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "trash".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                play_btn,
                                "Play",
                                UiNodeKind::IconButton {
                                    icon_name: "play".to_owned(),
                                    selected: true,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(
                                loop_btn,
                                "Loop",
                                UiNodeKind::IconButton {
                                    icon_name: "loop".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(28.0)),
                            UiStudioNode::new(onion_sw, "Onion", UiNodeKind::Switch { on: true })
                                .with_width(UiSizeUnit::Px(82.0)),
                            UiStudioNode::new(
                                frame_fld,
                                "Current Frame",
                                UiNodeKind::Field {
                                    value: 14.0,
                                    prefix: "F".to_owned(),
                                    unit: "".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(72.0)),
                            UiStudioNode::new(
                                fps_fld,
                                "Frame Rate",
                                UiNodeKind::Field {
                                    value: 24.0,
                                    prefix: "FPS".to_owned(),
                                    unit: "fps".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(88.0)),
                        ]),
                    UiStudioNode::new(
                        layers_col_id,
                        "Layer Tree Column (6 Layers: Folder, Mask, Masked, Guide, Normal, Audio)",
                        UiNodeKind::Column,
                    )
                    .with_width(UiSizeUnit::Px(220.0)),
                    UiStudioNode::new(
                        ruler_playhead_id,
                        "Frame Ruler (1..35), Red Playhead & Onion Skin Brackets",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(22.0)),
                    UiStudioNode::new(
                        classic_tween_id,
                        "Classic Tween Span (Purple + Arrow ──────►)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        shape_tween_id,
                        "Shape Tween Span (Green + Arrow ──────►)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        motion_tween_id,
                        "Motion Tween Span (Blue + Property Diamonds ◆)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        broken_tween_id,
                        "Broken Tween Span (Dashed Line - - - - -)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        keyframes_badges_id,
                        "Keyframe Dots (●/○), Span End ([ ]), Label Flag & Action (a)",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                    UiStudioNode::new(
                        audio_waveform_id,
                        "Soundtrack Audio Waveform Span",
                        UiNodeKind::Row,
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                    .with_height(UiSizeUnit::Px(24.0)),
                ])
            }
            Self::LibraryAndAssetsStudio => {
                let root_id = alloc();
                let preview_sec = alloc();
                let search_fld = alloc();
                let hdr_row = alloc();
                let sym1 = alloc();
                let sym2 = alloc();
                let sym3 = alloc();
                let sym4 = alloc();
                let footer_row = alloc();
                let new_sym_btn = alloc();
                let folder_btn = alloc();
                let props_btn = alloc();
                let del_btn = alloc();

                UiStudioNode::new(
                    root_id,
                    "Library Split: Preview + Tree",
                    UiNodeKind::Panel {
                        tabs: vec!["Library".to_owned(), "Assets".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        preview_sec,
                        "SYMBOL PREVIEW STAGE (Hero_MC · Movie Clip)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![UiStudioNode::new(
                        search_fld,
                        "Filter Symbols",
                        UiNodeKind::TextField {
                            text: "".to_owned(),
                            prefix: "Find".to_owned(),
                            hint: "Search symbols, bitmaps, sounds…".to_owned(),
                            mono: false,
                        },
                    )]),
                    UiStudioNode::new(
                        hdr_row,
                        "Library Columns Header",
                        UiNodeKind::TableRow {
                            cells: vec!["NAME".to_owned(), "KIND".to_owned(), "USE".to_owned()],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: true,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        sym1,
                        "Hero_MC Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "Hero_MC".to_owned(),
                                "Movie Clip".to_owned(),
                                "4".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: true,
                        },
                    ),
                    UiStudioNode::new(
                        sym2,
                        "CTA_Button Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "CTA_Button".to_owned(),
                                "Button".to_owned(),
                                "2".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        sym3,
                        "Cloud_Graphic Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "Cloud_Graphic".to_owned(),
                                "Graphic".to_owned(),
                                "9".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        sym4,
                        "Theme_Loop.mp3 Row",
                        UiNodeKind::TableRow {
                            cells: vec![
                                "Theme_Loop.mp3".to_owned(),
                                "Sound".to_owned(),
                                "1".to_owned(),
                            ],
                            column_fractions: vec![0.0, 0.56, 0.84],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        footer_row,
                        "Library Footer Actions (4-Col Grid)",
                        UiNodeKind::Grid {
                            columns: vec![
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                            ],
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            new_sym_btn,
                            "New Symbol",
                            UiNodeKind::IconButton {
                                icon_name: "plus".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                        UiStudioNode::new(
                            folder_btn,
                            "New Folder",
                            UiNodeKind::IconButton {
                                icon_name: "folderplus".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                        UiStudioNode::new(
                            props_btn,
                            "Symbol Properties",
                            UiNodeKind::IconButton {
                                icon_name: "settings".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                        UiStudioNode::new(
                            del_btn,
                            "Delete Symbol",
                            UiNodeKind::IconButton {
                                icon_name: "trash".to_owned(),
                                selected: false,
                                md_size: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                    ]),
                ])
            }
            Self::ColorAndGradientStudio => {
                let root_id = alloc();
                let top_row = alloc();
                let stroke_well = alloc();
                let fill_well = alloc();
                let mode_sel = alloc();
                let grad_sec = alloc();
                let stop_grid = alloc();
                let stop1_pos = alloc();
                let stop2_pos = alloc();
                let rgba_grid = alloc();
                let r_fld = alloc();
                let g_fld = alloc();
                let b_fld = alloc();
                let a_fld = alloc();
                let swatches_sec = alloc();
                let sw_grid = alloc();

                let mut sw_children = Vec::new();
                for (lbl, rgba) in [
                    ("Crimson", [235, 68, 90, 255]),
                    ("Amber", [255, 168, 52, 255]),
                    ("Emerald", [52, 208, 132, 255]),
                    ("Cobalt", [76, 132, 255, 255]),
                ] {
                    sw_children.push(
                        UiStudioNode::new(
                            alloc(),
                            lbl,
                            UiNodeKind::ColorSwatch {
                                rgba,
                                is_stroke: false,
                            },
                        )
                        .with_width(UiSizeUnit::Percent(100.0)),
                    );
                }

                UiStudioNode::new(
                    root_id,
                    "Color Mixer, Gradient Bar & Swatches",
                    UiNodeKind::Panel {
                        tabs: vec![
                            "Color Mixer".to_owned(),
                            "Gradient".to_owned(),
                            "Swatches".to_owned(),
                        ],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(top_row, "Stroke / Fill Wells & Paint Mode", UiNodeKind::Row)
                        .with_children(vec![
                            UiStudioNode::new(
                                stroke_well,
                                "Stroke Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [108, 147, 255, 255],
                                    is_stroke: true,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                fill_well,
                                "Fill Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [255, 114, 92, 255],
                                    is_stroke: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                mode_sel,
                                "Gradient Mode",
                                UiNodeKind::Select {
                                    selected: 1,
                                    options: vec![
                                        "Solid".to_owned(),
                                        "Linear Gradient".to_owned(),
                                        "Radial Gradient".to_owned(),
                                        "Bitmap Fill".to_owned(),
                                    ],
                                },
                            )
                            .with_width(UiSizeUnit::Fill(1.0)),
                        ]),
                    UiStudioNode::new(
                        grad_sec,
                        "GRADIENT RAMP & COLOR STOPS",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            stop_grid,
                            "Gradient Stop Positions (2-Col Grid)",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                stop1_pos,
                                "Stop 1 Pos",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "S1".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                stop2_pos,
                                "Stop 2 Pos",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "S2".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                        ]),
                    ]),
                    UiStudioNode::new(
                        rgba_grid,
                        "RGBA Channels (4-Col Grid)",
                        UiNodeKind::Grid {
                            columns: vec![
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                            ],
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            r_fld,
                            "Red",
                            UiNodeKind::Field {
                                value: 255.0,
                                prefix: "R".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            g_fld,
                            "Green",
                            UiNodeKind::Field {
                                value: 114.0,
                                prefix: "G".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            b_fld,
                            "Blue",
                            UiNodeKind::Field {
                                value: 92.0,
                                prefix: "B".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            a_fld,
                            "Alpha",
                            UiNodeKind::Field {
                                value: 100.0,
                                prefix: "A".to_owned(),
                                unit: "%".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                    ]),
                    UiStudioNode::new(
                        swatches_sec,
                        "SWATCH PALETTE (4-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            sw_grid,
                            "Swatches 4-Col Row",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                ],
                            },
                        )
                        .with_children(sw_children),
                    ]),
                ])
            }
            Self::AlignAndTransform => {
                let root_id = alloc();
                let stage_sw = alloc();
                let align_sec = alloc();
                let align_grid = alloc();
                let dist_sec = alloc();
                let dist_grid = alloc();
                let xform_sec = alloc();
                let xform_grid = alloc();
                let sx_id = alloc();
                let sy_id = alloc();
                let rot_id = alloc();
                let skew_id = alloc();

                let align_btns = [
                    ("Align Left", "align-left"),
                    ("Align Center H", "align-center-h"),
                    ("Align Right", "align-right"),
                    ("Align Top", "align-top"),
                    ("Align Center V", "align-center-v"),
                    ("Align Bottom", "align-bottom"),
                ]
                .into_iter()
                .map(|(lbl, ico)| {
                    UiStudioNode::new(
                        alloc(),
                        lbl,
                        UiNodeKind::IconButton {
                            icon_name: ico.to_owned(),
                            selected: false,
                            md_size: false,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                })
                .collect();

                let dist_btns = [
                    ("Distribute Left", "distribute-h"),
                    ("Distribute Center H", "space-h"),
                    ("Distribute Top", "distribute-v"),
                    ("Distribute Center V", "space-v"),
                ]
                .into_iter()
                .map(|(lbl, ico)| {
                    UiStudioNode::new(
                        alloc(),
                        lbl,
                        UiNodeKind::IconButton {
                            icon_name: ico.to_owned(),
                            selected: false,
                            md_size: false,
                        },
                    )
                    .with_width(UiSizeUnit::Percent(100.0))
                })
                .collect();

                UiStudioNode::new(
                    root_id,
                    "Align & Transform",
                    UiNodeKind::Panel {
                        tabs: vec!["Align".to_owned(), "Transform".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(268.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(stage_sw, "Align to Stage", UiNodeKind::Switch { on: true }),
                    UiStudioNode::new(
                        align_sec,
                        "ALIGN OBJECTS (3-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            align_grid,
                            "Align 3-Column Grid",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(33.3333),
                                    UiSizeUnit::Percent(33.3333),
                                    UiSizeUnit::Percent(33.3334),
                                ],
                            },
                        )
                        .with_children(align_btns),
                    ]),
                    UiStudioNode::new(
                        dist_sec,
                        "DISTRIBUTE & SPACE (4-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            dist_grid,
                            "Distribute 4-Column Grid",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                ],
                            },
                        )
                        .with_children(dist_btns),
                    ]),
                    UiStudioNode::new(
                        xform_sec,
                        "TRANSFORM MATRIX (2-COL GRID)",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            xform_grid,
                            "Transform 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                sx_id,
                                "Scale X",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "SX".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                sy_id,
                                "Scale Y",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "SY".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                rot_id,
                                "Rotate",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "Rot".to_owned(),
                                    unit: "°".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                skew_id,
                                "Skew X",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "SkX".to_owned(),
                                    unit: "°".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                        ]),
                    ]),
                ])
            }
            Self::PropertiesMovieClip => {
                let root_id = alloc();
                let instance_id = alloc();
                let pos_sec_id = alloc();
                let pos_grid_id = alloc();
                let x_id = alloc();
                let y_id = alloc();
                let w_id = alloc();
                let h_id = alloc();
                let rot_id = alloc();
                let skew_id = alloc();
                let disp_sec_id = alloc();
                let blend_id = alloc();
                let alpha_id = alloc();
                let vis_row_id = alloc();
                let vis_chk_id = alloc();
                let cache_chk_id = alloc();

                UiStudioNode::new(
                    root_id,
                    "Properties — Movie Clip",
                    UiNodeKind::Panel {
                        tabs: vec!["Properties".to_owned(), "Filters".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        instance_id,
                        "Instance Name",
                        UiNodeKind::TextField {
                            text: "hero_mc".to_owned(),
                            prefix: "Name".to_owned(),
                            hint: "<Instance Name>".to_owned(),
                            mono: true,
                        },
                    ),
                    UiStudioNode::new(
                        pos_sec_id,
                        "POSITION AND SIZE",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            pos_grid_id,
                            "Transform 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                x_id,
                                "Position X",
                                UiNodeKind::Field {
                                    value: 140.0,
                                    prefix: "X".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                y_id,
                                "Position Y",
                                UiNodeKind::Field {
                                    value: 96.0,
                                    prefix: "Y".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                w_id,
                                "Width",
                                UiNodeKind::Field {
                                    value: 240.0,
                                    prefix: "W".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                h_id,
                                "Height",
                                UiNodeKind::Field {
                                    value: 180.0,
                                    prefix: "H".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                rot_id,
                                "Rotation",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "R".to_owned(),
                                    unit: "°".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                skew_id,
                                "Scale X",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "SX".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                        ]),
                    ]),
                    UiStudioNode::new(
                        disp_sec_id,
                        "DISPLAY",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            blend_id,
                            "Blend Mode",
                            UiNodeKind::Select {
                                selected: 0,
                                options: vec![
                                    "Normal".to_owned(),
                                    "Multiply".to_owned(),
                                    "Screen".to_owned(),
                                    "Overlay".to_owned(),
                                    "Add".to_owned(),
                                ],
                            },
                        ),
                        UiStudioNode::new(
                            alpha_id,
                            "Alpha (%)",
                            UiNodeKind::Slider {
                                value: 100.0,
                                min: 0.0,
                                max: 100.0,
                                show_field: true,
                            },
                        ),
                        UiStudioNode::new(vis_row_id, "Display Flags Row", UiNodeKind::Row)
                            .with_children(vec![
                                UiStudioNode::new(
                                    vis_chk_id,
                                    "Visible",
                                    UiNodeKind::Checkbox { checked: true },
                                )
                                .with_width(UiSizeUnit::Percent(50.0)),
                                UiStudioNode::new(
                                    cache_chk_id,
                                    "Cache as bitmap",
                                    UiNodeKind::Checkbox { checked: false },
                                )
                                .with_width(UiSizeUnit::Percent(50.0)),
                            ]),
                    ]),
                ])
            }
            Self::PropertiesText => {
                let root_id = alloc();
                let char_sec = alloc();
                let family_sel = alloc();
                let style_row = alloc();
                let weight_sel = alloc();
                let size_fld = alloc();
                let metrics_grid = alloc();
                let line_fld = alloc();
                let track_fld = alloc();
                let align_grid = alloc();
                let al_l = alloc();
                let al_c = alloc();
                let al_r = alloc();
                let al_j = alloc();

                UiStudioNode::new(
                    root_id,
                    "Properties — Text",
                    UiNodeKind::Panel {
                        tabs: vec!["Properties".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        char_sec,
                        "CHARACTER",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            family_sel,
                            "Font Family",
                            UiNodeKind::Select {
                                selected: 0,
                                options: vec![
                                    "Inter".to_owned(),
                                    "Roboto Mono".to_owned(),
                                    "Helvetica Neue".to_owned(),
                                ],
                            },
                        ),
                        UiStudioNode::new(style_row, "Weight & Size Row", UiNodeKind::Row)
                            .with_children(vec![
                                UiStudioNode::new(
                                    weight_sel,
                                    "Font Weight",
                                    UiNodeKind::Select {
                                        selected: 1,
                                        options: vec![
                                            "Regular".to_owned(),
                                            "Medium".to_owned(),
                                            "Semi Bold".to_owned(),
                                        ],
                                    },
                                )
                                .with_width(UiSizeUnit::Percent(60.0)),
                                UiStudioNode::new(
                                    size_fld,
                                    "Font Size",
                                    UiNodeKind::Field {
                                        value: 16.0,
                                        prefix: "Sz".to_owned(),
                                        unit: "pt".to_owned(),
                                        decimals: 0,
                                        stepper: true,
                                    },
                                )
                                .with_width(UiSizeUnit::Percent(40.0)),
                            ]),
                        UiStudioNode::new(
                            metrics_grid,
                            "Spacing 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                line_fld,
                                "Line Spacing",
                                UiNodeKind::Field {
                                    value: 20.0,
                                    prefix: "LH".to_owned(),
                                    unit: "pt".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                track_fld,
                                "Letter Spacing",
                                UiNodeKind::Field {
                                    value: 0.0,
                                    prefix: "AV".to_owned(),
                                    unit: "em".to_owned(),
                                    decimals: 2,
                                    stepper: false,
                                },
                            ),
                        ]),
                        UiStudioNode::new(
                            align_grid,
                            "Paragraph Align (4-Col Grid)",
                            UiNodeKind::Grid {
                                columns: vec![
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                    UiSizeUnit::Percent(25.0),
                                ],
                            },
                        )
                        .with_children(vec![
                            UiStudioNode::new(
                                al_l,
                                "Align Left",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-left".to_owned(),
                                    selected: true,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                            UiStudioNode::new(
                                al_c,
                                "Align Center",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-center".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                            UiStudioNode::new(
                                al_r,
                                "Align Right",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-right".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                            UiStudioNode::new(
                                al_j,
                                "Justify",
                                UiNodeKind::IconButton {
                                    icon_name: "talign-justify".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Percent(100.0)),
                        ]),
                    ]),
                ])
            }
            Self::ColorPanel => {
                let root_id = alloc();
                let top_row = alloc();
                let stroke_well = alloc();
                let fill_well = alloc();
                let mode_sel = alloc();
                let rgba_grid = alloc();
                let r_fld = alloc();
                let g_fld = alloc();
                let b_fld = alloc();
                let a_fld = alloc();
                let alpha_slider = alloc();

                UiStudioNode::new(
                    root_id,
                    "Color",
                    UiNodeKind::Panel {
                        tabs: vec!["Color".to_owned(), "Swatches".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(288.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(top_row, "Wells & Mode Row", UiNodeKind::Row).with_children(
                        vec![
                            UiStudioNode::new(
                                stroke_well,
                                "Stroke Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [108, 147, 255, 255],
                                    is_stroke: true,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                fill_well,
                                "Fill Well",
                                UiNodeKind::ColorSwatch {
                                    rgba: [255, 154, 92, 255],
                                    is_stroke: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(74.0)),
                            UiStudioNode::new(
                                mode_sel,
                                "Paint Type",
                                UiNodeKind::Select {
                                    selected: 0,
                                    options: vec![
                                        "Solid".to_owned(),
                                        "Linear".to_owned(),
                                        "Radial".to_owned(),
                                    ],
                                },
                            )
                            .with_width(UiSizeUnit::Fill(1.0)),
                        ],
                    ),
                    UiStudioNode::new(
                        rgba_grid,
                        "RGBA Channels (4-Col Grid)",
                        UiNodeKind::Grid {
                            columns: vec![
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                                UiSizeUnit::Percent(25.0),
                            ],
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            r_fld,
                            "Red",
                            UiNodeKind::Field {
                                value: 108.0,
                                prefix: "R".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            g_fld,
                            "Green",
                            UiNodeKind::Field {
                                value: 147.0,
                                prefix: "G".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            b_fld,
                            "Blue",
                            UiNodeKind::Field {
                                value: 255.0,
                                prefix: "B".to_owned(),
                                unit: "".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                        UiStudioNode::new(
                            a_fld,
                            "Alpha",
                            UiNodeKind::Field {
                                value: 100.0,
                                prefix: "A".to_owned(),
                                unit: "%".to_owned(),
                                decimals: 0,
                                stepper: false,
                            },
                        ),
                    ]),
                    UiStudioNode::new(
                        alpha_slider,
                        "Opacity Slider",
                        UiNodeKind::Slider {
                            value: 100.0,
                            min: 0.0,
                            max: 100.0,
                            show_field: true,
                        },
                    ),
                ])
            }
            Self::ToolsPanel => {
                let root_id = alloc();
                let grid_id = alloc();
                let tools = [
                    ("Select (V)", "select", true),
                    ("Subselect (A)", "subselect", false),
                    ("Transform (Q)", "transform", false),
                    ("Pen (P)", "pen", false),
                    ("Text (T)", "text", false),
                    ("Line (N)", "line", false),
                    ("Rectangle (R)", "rect", false),
                    ("Oval (O)", "oval", false),
                    ("Pencil (Y)", "pencil", false),
                    ("Brush (B)", "brush", false),
                    ("Paint Bucket (K)", "bucket", false),
                    ("Eyedropper (I)", "eyedropper", false),
                ];
                let tool_nodes = tools
                    .into_iter()
                    .map(|(lbl, ico, sel)| {
                        UiStudioNode::new(
                            alloc(),
                            lbl,
                            UiNodeKind::IconButton {
                                icon_name: ico.to_owned(),
                                selected: sel,
                                md_size: false,
                            },
                        )
                    })
                    .collect();

                UiStudioNode::new(
                    root_id,
                    "Tools",
                    UiNodeKind::Panel {
                        tabs: Vec::new(),
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(80.0))
                .with_padding(Padding::all_px(7.0))
                .with_children(vec![
                    UiStudioNode::new(
                        grid_id,
                        "Tools 2-Column Rail",
                        UiNodeKind::Grid {
                            columns: vec![UiSizeUnit::Px(32.0), UiSizeUnit::Px(32.0)],
                        },
                    )
                    .with_system_binding(SharedSystemKind::ToolRail)
                    .with_gap(GapPair::both_px(2.0))
                    .with_children(tool_nodes),
                ])
            }
            Self::FiltersPanel => {
                let root_id = alloc();
                let hdr_row = alloc();
                let row1 = alloc();
                let row2 = alloc();
                let params_sec = alloc();
                let params_grid = alloc();
                let bx = alloc();
                let by = alloc();
                let str_fld = alloc();
                let qual_sel = alloc();

                UiStudioNode::new(
                    root_id,
                    "Filters",
                    UiNodeKind::Panel {
                        tabs: vec!["Properties".to_owned(), "Filters".to_owned()],
                        active_tab: 1,
                    },
                )
                .with_width(UiSizeUnit::Px(320.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(
                        hdr_row,
                        "Filter Table Header",
                        UiNodeKind::TableRow {
                            cells: vec!["FILTER".to_owned(), "QUALITY".to_owned()],
                            column_fractions: vec![0.0, 0.65],
                            is_header: true,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        row1,
                        "Drop Shadow Row",
                        UiNodeKind::TableRow {
                            cells: vec!["Drop Shadow".to_owned(), "High".to_owned()],
                            column_fractions: vec![0.0, 0.65],
                            is_header: false,
                            selected: true,
                        },
                    ),
                    UiStudioNode::new(
                        row2,
                        "Blur Filter Row",
                        UiNodeKind::TableRow {
                            cells: vec!["Blur".to_owned(), "Medium".to_owned()],
                            column_fractions: vec![0.0, 0.65],
                            is_header: false,
                            selected: false,
                        },
                    ),
                    UiStudioNode::new(
                        params_sec,
                        "DROP SHADOW PARAMETERS",
                        UiNodeKind::Section {
                            collapsible: true,
                            expanded: true,
                        },
                    )
                    .with_children(vec![
                        UiStudioNode::new(
                            params_grid,
                            "Filter Params 2-Col Grid",
                            UiNodeKind::Grid {
                                columns: vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
                            },
                        )
                        .with_system_binding(SharedSystemKind::TwoColumnForm)
                        .with_children(vec![
                            UiStudioNode::new(
                                bx,
                                "Blur X",
                                UiNodeKind::Field {
                                    value: 8.0,
                                    prefix: "BX".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                by,
                                "Blur Y",
                                UiNodeKind::Field {
                                    value: 8.0,
                                    prefix: "BY".to_owned(),
                                    unit: "px".to_owned(),
                                    decimals: 1,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                str_fld,
                                "Strength",
                                UiNodeKind::Field {
                                    value: 100.0,
                                    prefix: "Str".to_owned(),
                                    unit: "%".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            ),
                            UiStudioNode::new(
                                qual_sel,
                                "Quality",
                                UiNodeKind::Select {
                                    selected: 2,
                                    options: vec![
                                        "Low".to_owned(),
                                        "Medium".to_owned(),
                                        "High".to_owned(),
                                    ],
                                },
                            ),
                        ]),
                    ]),
                ])
            }
            Self::TimelineHeader => {
                let root_id = alloc();
                let bar_row = alloc();
                let new_layer_btn = alloc();
                let folder_btn = alloc();
                let trash_btn = alloc();
                let sep1 = alloc();
                let play_btn = alloc();
                let loop_btn = alloc();
                let onion_sw = alloc();
                let frame_fld = alloc();
                let fps_fld = alloc();

                UiStudioNode::new(
                    root_id,
                    "Timeline Header",
                    UiNodeKind::Panel {
                        tabs: vec!["Timeline".to_owned(), "Motion Editor".to_owned()],
                        active_tab: 0,
                    },
                )
                .with_width(UiSizeUnit::Px(560.0))
                .with_system_binding(SharedSystemKind::PanelHeader)
                .with_children(vec![
                    UiStudioNode::new(bar_row, "Timeline Controls Row", UiNodeKind::Row)
                        .with_children(vec![
                            UiStudioNode::new(
                                new_layer_btn,
                                "New Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "layerplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                folder_btn,
                                "New Folder",
                                UiNodeKind::IconButton {
                                    icon_name: "folderplus".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                trash_btn,
                                "Delete Layer",
                                UiNodeKind::IconButton {
                                    icon_name: "trash".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                sep1,
                                "Divider",
                                UiNodeKind::Separator { vertical: true },
                            )
                            .with_width(UiSizeUnit::Px(8.0)),
                            UiStudioNode::new(
                                play_btn,
                                "Play",
                                UiNodeKind::IconButton {
                                    icon_name: "play".to_owned(),
                                    selected: true,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(
                                loop_btn,
                                "Loop Playback",
                                UiNodeKind::IconButton {
                                    icon_name: "loop".to_owned(),
                                    selected: false,
                                    md_size: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(32.0)),
                            UiStudioNode::new(onion_sw, "Onion", UiNodeKind::Switch { on: true })
                                .with_width(UiSizeUnit::Px(86.0)),
                            UiStudioNode::new(
                                frame_fld,
                                "Current Frame",
                                UiNodeKind::Field {
                                    value: 1.0,
                                    prefix: "F".to_owned(),
                                    unit: "".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Px(76.0)),
                            UiStudioNode::new(
                                fps_fld,
                                "Frame Rate",
                                UiNodeKind::Field {
                                    value: 24.0,
                                    prefix: "FPS".to_owned(),
                                    unit: "fps".to_owned(),
                                    decimals: 0,
                                    stepper: false,
                                },
                            )
                            .with_width(UiSizeUnit::Fill(1.0)),
                        ]),
                ])
            }
        };

        UiStudioDocument {
            name: self.label().to_owned(),
            root,
            next_id,
        }
    }
}

/// Generate deterministic, human-readable Rust NF2 layout code for `doc`.
pub fn export_rust_layout_code(doc: &UiStudioDocument) -> String {
    export_rust_layout_with_systems(doc, &UiStudioSystems::default())
}

/// Generate deterministic Rust code for both the panel widget hierarchy (`doc`) and the edited
/// NF2 system tokens (`systems`), ready to paste or apply into the NeoFlash build.
pub fn export_rust_layout_with_systems(
    doc: &UiStudioDocument,
    systems: &UiStudioSystems,
) -> String {
    let mut out = String::new();
    let fn_ident = doc
        .name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let fn_name = if fn_ident.is_empty() {
        "studio_panel".to_owned()
    } else {
        format!("render_{fn_ident}")
    };

    writeln!(
        out,
        "// Generated by NeoFlash UI Layout Studio — panel: {:?}",
        doc.name
    )
    .unwrap();
    writeln!(out, "use eframe::egui::{{self, Ui}};").unwrap();
    writeln!(out, "use neoflash_ui::components as c;").unwrap();
    writeln!(out, "use neoflash_ui::tokens::{{TextStyle, space}};").unwrap();
    writeln!(out, "use neoflash_ui::typography::rich;\n").unwrap();

    writeln!(out, "pub mod studio_system_tokens {{").unwrap();
    writeln!(
        out,
        "    pub const SPACE_S1: f32 = {:.1};",
        systems.spacing.s1
    )
    .unwrap();
    writeln!(
        out,
        "    pub const SPACE_S2: f32 = {:.1};",
        systems.spacing.s2
    )
    .unwrap();
    writeln!(
        out,
        "    pub const SPACE_S3: f32 = {:.1};",
        systems.spacing.s3
    )
    .unwrap();
    writeln!(
        out,
        "    pub const SPACE_S4: f32 = {:.1};",
        systems.spacing.s4
    )
    .unwrap();
    writeln!(
        out,
        "    pub const SPACE_S5: f32 = {:.1};",
        systems.spacing.s5
    )
    .unwrap();
    writeln!(
        out,
        "    pub const SPACE_S6: f32 = {:.1};",
        systems.spacing.s6
    )
    .unwrap();
    writeln!(
        out,
        "    pub const CONTROL_HEIGHT_COMFORTABLE: f32 = {:.1};",
        systems.density.comfortable.control_height
    )
    .unwrap();
    writeln!(
        out,
        "    pub const CONTROL_HEIGHT_COMPACT: f32 = {:.1};",
        systems.density.compact.control_height
    )
    .unwrap();
    writeln!(
        out,
        "    pub const CONTROL_HEIGHT_CLASSIC: f32 = {:.1};",
        systems.density.classic_flash.control_height
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWO_COL_GAP_PX: f32 = {:.1};",
        systems.two_column_form.gap.resolve(systems)
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWO_COL_LABEL_WIDTH_PX: f32 = {:.1};",
        systems.two_column_form.label_width_px
    )
    .unwrap();
    writeln!(
        out,
        "    // Timeline & Tween Arrow Geometry (NeoFlash UI Studio)"
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWEEN_ARROW_HEAD_LENGTH_PX: f32 = {:.2};",
        systems.timeline_geometry.tween_arrow_head_length_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWEEN_ARROW_HEAD_WING_PX: f32 = {:.2};",
        systems.timeline_geometry.tween_arrow_head_wing_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWEEN_ARROW_START_INSET_PX: f32 = {:.2};",
        systems.timeline_geometry.tween_arrow_start_inset_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWEEN_ARROW_END_INSET_PX: f32 = {:.2};",
        systems.timeline_geometry.tween_arrow_end_inset_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWEEN_ARROW_STROKE_WIDTH_PX: f32 = {:.2};",
        systems.timeline_geometry.tween_arrow_stroke_width_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TWEEN_ARROW_Y_PCT: f32 = {:.1};",
        systems.timeline_geometry.tween_arrow_y_pct
    )
    .unwrap();
    writeln!(
        out,
        "    pub const BROKEN_TWEEN_DASH_PX: f32 = {:.1};",
        systems.timeline_geometry.broken_tween_dash_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const BROKEN_TWEEN_GAP_PX: f32 = {:.1};",
        systems.timeline_geometry.broken_tween_gap_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TIMELINE_FRAME_CELL_WIDTH_PX: f32 = {:.1};",
        systems.timeline_geometry.frame_cell_width_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TIMELINE_LAYER_ROW_HEIGHT_PX: f32 = {:.1};",
        systems.timeline_geometry.layer_row_height_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const TIMELINE_RULER_HEIGHT_PX: f32 = {:.1};",
        systems.timeline_geometry.ruler_height_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const KEYFRAME_DOT_RADIUS_PX: f32 = {:.2};",
        systems.timeline_geometry.keyframe_dot_radius_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const MOTION_DIAMOND_RADIUS_PX: f32 = {:.2};",
        systems.timeline_geometry.motion_diamond_radius_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const PLAYHEAD_CAP_WIDTH_PX: f32 = {:.1};",
        systems.timeline_geometry.playhead_cap_width_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const PLAYHEAD_CAP_HEIGHT_PX: f32 = {:.1};",
        systems.timeline_geometry.playhead_cap_height_px
    )
    .unwrap();
    writeln!(
        out,
        "    pub const STAGE_RULER_THICKNESS_PX: f32 = {:.1};",
        systems.stage_overlay.stage_ruler_thickness_px
    )
    .unwrap();
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "pub fn {fn_name}(ui: &mut Ui) {{").unwrap();
    emit_node_rust(&doc.root, systems, 1, &mut out);
    writeln!(out, "}}").unwrap();
    out
}

fn emit_node_rust(node: &UiStudioNode, systems: &UiStudioSystems, indent: usize, out: &mut String) {
    if !node.visible {
        return;
    }
    let pad_str = "    ".repeat(indent);
    let (pad, gap) = resolve_node_spacing(node, systems);
    let w_comment = node.box_constraints.width.format_compact();
    let h_comment = node.box_constraints.height.format_compact();
    writeln!(
        out,
        "{pad_str}// Node #{}: {:?} ({}) [w={w_comment}, h={h_comment}, pad=({:.0},{:.0},{:.0},{:.0}), gap=({:.0},{:.0})]",
        node.id,
        node.label,
        node.kind.type_badge(),
        pad[0],
        pad[1],
        pad[2],
        pad[3],
        gap.x,
        gap.y
    )
    .unwrap();

    match &node.kind {
        UiNodeKind::Panel { tabs, active_tab } => {
            if let UiSizeUnit::Px(px) = node.box_constraints.width {
                writeln!(out, "{pad_str}ui.set_width({px:.1});").unwrap();
            }
            if !tabs.is_empty() {
                let tabs_lit = tabs
                    .iter()
                    .map(|t| format!("{t:?}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(
                    out,
                    "{pad_str}let _header = c::FloatingPanelHeader {{ tabs: &[{tabs_lit}], active: {active_tab}, collapsed: false }}.show(ui);"
                )
                .unwrap();
            }
            writeln!(
                out,
                "{pad_str}ui.spacing_mut().item_spacing = egui::vec2({:.1}, {:.1});",
                gap.x, gap.y
            )
            .unwrap();
            for child in &node.children {
                emit_node_rust(child, systems, indent, out);
            }
        }
        UiNodeKind::Section { expanded, .. } => {
            writeln!(
                out,
                "{pad_str}c::SectionHeader::new({:?}).default_open({expanded}).show(ui, |ui| {{",
                node.label
            )
            .unwrap();
            writeln!(
                out,
                "{pad_str}    ui.spacing_mut().item_spacing = egui::vec2({:.1}, {:.1});",
                gap.x, gap.y
            )
            .unwrap();
            for child in &node.children {
                emit_node_rust(child, systems, indent + 1, out);
            }
            writeln!(out, "{pad_str}}});").unwrap();
        }
        UiNodeKind::Row => {
            writeln!(out, "{pad_str}ui.horizontal(|ui| {{").unwrap();
            writeln!(
                out,
                "{pad_str}    ui.spacing_mut().item_spacing.x = {:.1};",
                gap.x
            )
            .unwrap();
            for child in &node.children {
                emit_node_rust(child, systems, indent + 1, out);
            }
            writeln!(out, "{pad_str}}});").unwrap();
        }
        UiNodeKind::Column => {
            writeln!(out, "{pad_str}ui.vertical(|ui| {{").unwrap();
            writeln!(
                out,
                "{pad_str}    ui.spacing_mut().item_spacing.y = {:.1};",
                gap.y
            )
            .unwrap();
            for child in &node.children {
                emit_node_rust(child, systems, indent + 1, out);
            }
            writeln!(out, "{pad_str}}});").unwrap();
        }
        UiNodeKind::Grid { columns } => {
            let col_specs = columns
                .iter()
                .map(|c| c.format_compact())
                .collect::<Vec<_>>()
                .join(", ");
            let col_count = columns.len().max(1);
            writeln!(out, "{pad_str}// Grid ({col_count} columns: [{col_specs}])").unwrap();
            for row_slice in node.children.chunks(col_count) {
                writeln!(out, "{pad_str}ui.horizontal(|ui| {{").unwrap();
                writeln!(
                    out,
                    "{pad_str}    ui.spacing_mut().item_spacing.x = {:.1};",
                    gap.x
                )
                .unwrap();
                for child in row_slice {
                    emit_node_rust(child, systems, indent + 1, out);
                }
                writeln!(out, "{pad_str}}});").unwrap();
            }
        }
        UiNodeKind::Field {
            value,
            prefix,
            unit,
            decimals,
            stepper,
        } => {
            let mut chain = format!(
                "c::Field::new(&mut v_{}, {:?}).prefix({:?}).unit({:?}).decimals({})",
                node.id, node.label, prefix, unit, decimals
            );
            if *stepper {
                chain.push_str(".stepper()");
            }
            if let UiSizeUnit::Px(px) = node.box_constraints.width {
                let _ = write!(chain, ".width({px:.1})");
            }
            writeln!(out, "{pad_str}let mut v_{} = {:.2}_f64;", node.id, value).unwrap();
            writeln!(out, "{pad_str}ui.add({chain});").unwrap();
        }
        UiNodeKind::TextField {
            text, prefix, hint, ..
        } => {
            writeln!(
                out,
                "{pad_str}let mut t_{} = {:?}.to_owned();",
                node.id, text
            )
            .unwrap();
            writeln!(
                out,
                "{pad_str}ui.add(c::TextField::new(&mut t_{}, {:?}).prefix({:?}));",
                node.id,
                if hint.is_empty() { &node.label } else { hint },
                prefix
            )
            .unwrap();
        }
        UiNodeKind::Select { selected, options } => {
            let current = options
                .get(*selected)
                .map_or(node.label.as_str(), String::as_str);
            writeln!(
                out,
                "{pad_str}c::Select::new({:?}, {:?}).show(ui, |_ui| {{}});",
                format!("studio-sel-{}", node.id),
                current
            )
            .unwrap();
        }
        UiNodeKind::Button { button_kind, .. } => {
            let ctor = match button_kind {
                StudioButtonKind::Primary => "primary",
                StudioButtonKind::Secondary => "new",
                StudioButtonKind::Ghost => "ghost",
                StudioButtonKind::Danger => "danger",
            };
            writeln!(
                out,
                "{pad_str}let _ = ui.add(c::Button::{ctor}({:?}));",
                node.label
            )
            .unwrap();
        }
        UiNodeKind::IconButton {
            icon_name,
            selected,
            ..
        } => {
            writeln!(
                out,
                "{pad_str}// IconButton {:?} (icon={:?}, selected={})",
                node.label, icon_name, selected
            )
            .unwrap();
        }
        UiNodeKind::Checkbox { checked } => {
            writeln!(out, "{pad_str}let mut chk_{} = {checked};", node.id).unwrap();
            writeln!(
                out,
                "{pad_str}ui.add(c::Checkbox::new(&mut chk_{}, {:?}));",
                node.id, node.label
            )
            .unwrap();
        }
        UiNodeKind::Switch { on } => {
            writeln!(out, "{pad_str}let mut sw_{} = {on};", node.id).unwrap();
            writeln!(
                out,
                "{pad_str}ui.add(c::Switch::new(&mut sw_{}, {:?}));",
                node.id, node.label
            )
            .unwrap();
        }
        UiNodeKind::ColorSwatch { rgba, is_stroke } => {
            let target = if *is_stroke {
                "c::ColorWellTarget::Stroke"
            } else {
                "c::ColorWellTarget::Fill"
            };
            writeln!(
                out,
                "{pad_str}ui.add(c::ColorWell {{ target: {target}, color: Some(egui::Color32::from_rgba_unmultiplied({}, {}, {}, {})), active: false }});",
                rgba[0], rgba[1], rgba[2], rgba[3]
            )
            .unwrap();
        }
        UiNodeKind::Slider {
            value, min, max, ..
        } => {
            writeln!(out, "{pad_str}let mut sl_{} = {:.2}_f64;", node.id, value).unwrap();
            writeln!(
                out,
                "{pad_str}ui.add(c::Slider::new(&mut sl_{}, {:.1}..={:.1}, {:?}));",
                node.id, min, max, node.label
            )
            .unwrap();
        }
        UiNodeKind::Label => {
            writeln!(
                out,
                "{pad_str}ui.label(rich({:?}, TextStyle::Body));",
                node.label
            )
            .unwrap();
        }
        UiNodeKind::Separator { vertical } => {
            writeln!(out, "{pad_str}c::divider(ui, {vertical});").unwrap();
        }
        UiNodeKind::TableRow {
            cells,
            column_fractions,
            is_header,
            selected,
        } => {
            let cells_lit = cells
                .iter()
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            let cols_lit = column_fractions
                .iter()
                .map(|f| format!("{f:.2}"))
                .collect::<Vec<_>>()
                .join(", ");
            let kind_lit = if *is_header {
                "c::TableRowKind::Header"
            } else if *selected {
                "c::TableRowKind::Selected"
            } else {
                "c::TableRowKind::Row"
            };
            writeln!(
                out,
                "{pad_str}ui.add(c::TableRow {{ cells: &[{cells_lit}], kind: {kind_lit}, columns: &[{cols_lit}] }});"
            )
            .unwrap();
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6b. SVG Vector Export & SVG-to-Panel Importer (Round-Trip + Raw Figma SVG)
// ─────────────────────────────────────────────────────────────────────────────

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn color32_to_hex(c: Color32) -> String {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn parse_svg_hex_color(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() == 6 {
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        Some([r, g, b, 255])
    } else if h.len() == 3 {
        let r = u8::from_str_radix(&h[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&h[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&h[2..3], 16).ok()? * 17;
        Some([r, g, b, 255])
    } else {
        None
    }
}

/// Export a single [`UiStudioDocument`] artboard to a standards-compliant `.svg` vector document.
pub fn export_artboard_to_svg(
    doc: &UiStudioDocument,
    systems: &UiStudioSystems,
    dark_mode: bool,
) -> String {
    let default_w = match doc.root.box_constraints.width {
        UiSizeUnit::Px(px) => px.max(240.0),
        _ => 340.0,
    };
    let avail = egui::vec2(default_w, 640.0);
    let computed = compute_studio_layout(doc, systems, Pos2::ZERO, avail);
    let svg_w = computed.root_rect.width().max(260.0);
    let svg_h = computed.root_rect.height().max(240.0);

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{svg_w:.0}" height="{svg_h:.0}" viewBox="0 0 {svg_w:.0} {svg_h:.0}" data-nf-studio="1" data-nf-doc-name="{0}" data-nf-panel="{0}">"#,
        xml_escape(&doc.name)
    )
    .unwrap();
    emit_node_svg(
        &doc.root,
        systems,
        &computed,
        dark_mode,
        Pos2::ZERO,
        1,
        &mut svg,
    );
    writeln!(svg, "</svg>").unwrap();
    svg
}

/// Export all visible artboards on the Studio Canvas into one composite multi-panel `.svg` board.
pub fn export_all_artboards_to_svg(state: &UiLayoutStudioState, dark_mode: bool) -> String {
    let mut max_x = 640.0_f32;
    let mut max_y = 480.0_f32;
    for ab in &state.artboards {
        if ab.visible {
            max_x = max_x.max(ab.canvas_pos[0] + ab.canvas_size[0] + 32.0);
            max_y = max_y.max(ab.canvas_pos[1] + ab.canvas_size[1] + 32.0);
        }
    }
    let bg_app = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgApp);

    let mut svg = String::new();
    writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{max_x:.0}" height="{max_y:.0}" viewBox="0 0 {max_x:.0} {max_y:.0}" data-nf-studio-board="1">"#
    )
    .unwrap();
    writeln!(
        svg,
        r#"  <rect x="0" y="0" width="{max_x:.0}" height="{max_y:.0}" fill="{}" />"#,
        color32_to_hex(bg_app)
    )
    .unwrap();

    for ab in &state.artboards {
        if !ab.visible {
            continue;
        }
        let origin = egui::pos2(ab.canvas_pos[0], ab.canvas_pos[1]);
        let avail = egui::vec2(ab.canvas_size[0].max(80.0), ab.canvas_size[1].max(80.0));
        let computed = compute_studio_layout(&ab.document, &state.systems, origin, avail);
        writeln!(
            svg,
            r#"  <g id="artboard-{}" data-nf-artboard="{}" transform="translate(0, 0)">"#,
            ab.id,
            xml_escape(&ab.document.name)
        )
        .unwrap();
        emit_node_svg(
            &ab.document.root,
            &state.systems,
            &computed,
            dark_mode,
            Pos2::ZERO,
            2,
            &mut svg,
        );
        if ab.preset.is_timeline_surface() || ab.preset.is_full_shell() {
            let tl_rect = Rect::from_min_size(origin, avail);
            emit_timeline_geometry_svg(&mut svg, tl_rect, &state.systems, dark_mode, 2);
        }
        writeln!(svg, "  </g>").unwrap();
    }
    writeln!(svg, "</svg>").unwrap();
    svg
}

fn emit_timeline_geometry_svg(
    svg: &mut String,
    rect: Rect,
    systems: &UiStudioSystems,
    dark_mode: bool,
    indent: usize,
) {
    let pad = "  ".repeat(indent);
    let tg = &systems.timeline_geometry;
    let layer_col_w = tg.resolve_layer_list_width(rect.width());
    let ruler_h = tg.ruler_height_px.clamp(14.0, 40.0);
    let row_h = tg.layer_row_height_px.clamp(14.0, 48.0);
    let cell_w = tg.frame_cell_width_px.clamp(6.0, 32.0);
    let grid_left = rect.left() + layer_col_w;
    let classic_span = Rect::from_min_max(
        egui::pos2(grid_left, rect.top() + ruler_h + row_h),
        egui::pos2(
            (grid_left + 20.0 * cell_w).min(rect.right()),
            rect.top() + ruler_h + row_h * 2.0,
        ),
    );
    let (shaft_start, _tip, tri) = tg.compute_tween_arrow_points(classic_span);
    let arrow_col = if dark_mode { "#E2E7F2" } else { "#1F2430" };
    let span_fill = if dark_mode { "#2B224C" } else { "#DDD2FC" };
    writeln!(
        svg,
        r#"{pad}<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{span_fill}" data-nf-timeline-span="classic" />"#,
        classic_span.left(),
        classic_span.top(),
        classic_span.width(),
        classic_span.height()
    )
    .unwrap();
    writeln!(
        svg,
        r#"{pad}<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{arrow_col}" stroke-width="{:.2}" data-nf-tween-arrow="shaft" />"#,
        shaft_start.x,
        shaft_start.y,
        tri[1].x,
        shaft_start.y,
        tg.tween_arrow_stroke_width_px
    )
    .unwrap();
    writeln!(
        svg,
        r#"{pad}<polygon points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" fill="{arrow_col}" data-nf-tween-arrow="head" />"#,
        tri[0].x, tri[0].y, tri[1].x, tri[1].y, tri[2].x, tri[2].y
    )
    .unwrap();
}

fn emit_node_svg(
    node: &UiStudioNode,
    systems: &UiStudioSystems,
    computed: &ComputedStudioLayout,
    dark_mode: bool,
    offset: Pos2,
    indent: usize,
    svg: &mut String,
) {
    if !node.visible {
        return;
    }
    let Some(b) = computed.node_box(node.id) else {
        return;
    };
    let r = b.rect.translate(offset.to_vec2());
    let pad = "  ".repeat(indent);
    let base_pal = if dark_mode { &DARK } else { &LIGHT };

    let bg_panel = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::BgPanel),
    );
    let bg_raised = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::BgRaised),
    );
    let bg_field = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::BgField),
    );
    let border_def = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::BorderDefault),
    );
    let border_sub = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::BorderSubtle),
    );
    let text_pri = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::TextPrimary),
    );
    let text_sec = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::TextSecondary),
    );
    let accent = color32_to_hex(
        systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::AccentDefault),
    );
    let _ = base_pal;

    let kind_tag = node.kind.type_badge();
    let w_attr = node.box_constraints.width.format_compact();
    let h_attr = node.box_constraints.height.format_compact();
    let mut extra_attrs = String::new();
    match &node.kind {
        UiNodeKind::Panel { tabs, active_tab } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-tabs="{}" data-nf-active-tab="{active_tab}""#,
                xml_escape(&tabs.join("|"))
            );
        }
        UiNodeKind::Grid { columns } => {
            let tracks = columns
                .iter()
                .map(|c| c.format_compact())
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(extra_attrs, r#" data-nf-cols="{}""#, xml_escape(&tracks));
        }
        UiNodeKind::Field {
            value,
            prefix,
            unit,
            decimals,
            stepper,
        } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-val="{value}" data-nf-prefix="{}" data-nf-unit="{}" data-nf-dec="{decimals}" data-nf-step="{stepper}""#,
                xml_escape(prefix),
                xml_escape(unit)
            );
        }
        UiNodeKind::TextField {
            text, prefix, hint, ..
        } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-text="{}" data-nf-prefix="{}" data-nf-hint="{}""#,
                xml_escape(text),
                xml_escape(prefix),
                xml_escape(hint)
            );
        }
        UiNodeKind::Select { selected, options } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-sel="{selected}" data-nf-opts="{}""#,
                xml_escape(&options.join("|"))
            );
        }
        UiNodeKind::IconButton {
            icon_name,
            selected,
            ..
        } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-icon="{}" data-nf-sel="{selected}""#,
                xml_escape(icon_name)
            );
        }
        UiNodeKind::Checkbox { checked } => {
            let _ = write!(extra_attrs, r#" data-nf-checked="{checked}""#);
        }
        UiNodeKind::Switch { on } => {
            let _ = write!(extra_attrs, r#" data-nf-on="{on}""#);
        }
        UiNodeKind::ColorSwatch { rgba, is_stroke } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-rgba="{},{},{},{}" data-nf-stroke="{is_stroke}""#,
                rgba[0], rgba[1], rgba[2], rgba[3]
            );
        }
        UiNodeKind::Slider {
            value, min, max, ..
        } => {
            let _ = write!(
                extra_attrs,
                r#" data-nf-val="{value}" data-nf-min="{min}" data-nf-max="{max}""#
            );
        }
        UiNodeKind::TableRow {
            cells,
            column_fractions,
            is_header,
            selected,
        } => {
            let cols = column_fractions
                .iter()
                .map(|f| format!("{f:.2}"))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(
                extra_attrs,
                r#" data-nf-cells="{}" data-nf-fracs="{cols}" data-nf-hdr="{is_header}" data-nf-sel="{selected}""#,
                xml_escape(&cells.join("|"))
            );
        }
        _ => {}
    }

    writeln!(
        svg,
        r#"{pad}<g id="node-{}" data-nf-kind="{kind_tag}" data-nf-label="{}" data-nf-w="{w_attr}" data-nf-h="{h_attr}"{extra_attrs}>"#,
        node.id,
        xml_escape(&node.label)
    )
    .unwrap();

    match &node.kind {
        UiNodeKind::Panel { tabs, .. } => {
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="6" fill="{bg_panel}" stroke="{border_def}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
            if !tabs.is_empty() {
                let hdr_h = systems.panel_header.height.max(24.0);
                writeln!(
                    svg,
                    r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{hdr_h:.1}" rx="6" fill="{bg_raised}" />"#,
                    r.left(),
                    r.top(),
                    r.width()
                )
                .unwrap();
                writeln!(
                    svg,
                    r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="12" font-weight="600">{}</text>"#,
                    r.left() + 12.0,
                    r.top() + hdr_h * 0.62,
                    xml_escape(&tabs.join("  |  "))
                )
                .unwrap();
            }
            for child in &node.children {
                emit_node_svg(child, systems, computed, dark_mode, offset, indent + 1, svg);
            }
        }
        UiNodeKind::Section { expanded, .. } => {
            let hdr_h = systems.section_header.height.max(20.0);
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_sec}" font-family="Inter, sans-serif" font-size="10" font-weight="600">{}</text>"#,
                r.left() + 6.0,
                r.top() + hdr_h * 0.65,
                xml_escape(&node.label.to_uppercase())
            )
            .unwrap();
            writeln!(
                svg,
                r#"{pad}  <line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{border_sub}" stroke-width="1" />"#,
                r.left(),
                r.top() + hdr_h,
                r.right(),
                r.top() + hdr_h
            )
            .unwrap();
            if *expanded {
                for child in &node.children {
                    emit_node_svg(child, systems, computed, dark_mode, offset, indent + 1, svg);
                }
            }
        }
        UiNodeKind::Row | UiNodeKind::Column | UiNodeKind::Grid { .. } => {
            for child in &node.children {
                emit_node_svg(child, systems, computed, dark_mode, offset, indent + 1, svg);
            }
        }
        UiNodeKind::Field {
            value,
            prefix,
            unit,
            decimals,
            ..
        } => {
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{bg_field}" stroke="{border_def}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
            let formatted = format!("{prefix} {value:.decimals$} {unit}", decimals = *decimals);
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Roboto Mono, monospace" font-size="11">{}</text>"#,
                r.left() + 8.0,
                r.center().y + 4.0,
                xml_escape(formatted.trim())
            )
            .unwrap();
        }
        UiNodeKind::TextField { text, prefix, .. } => {
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{bg_field}" stroke="{border_def}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
            let display = if prefix.is_empty() {
                text.clone()
            } else {
                format!("{prefix}: {text}")
            };
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="11">{}</text>"#,
                r.left() + 8.0,
                r.center().y + 4.0,
                xml_escape(&display)
            )
            .unwrap();
        }
        UiNodeKind::Select { selected, options } => {
            let cur = options
                .get(*selected)
                .map_or(node.label.as_str(), String::as_str);
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{bg_field}" stroke="{border_def}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="11">{}</text>"#,
                r.left() + 8.0,
                r.center().y + 4.0,
                xml_escape(cur)
            )
            .unwrap();
        }
        UiNodeKind::Button { button_kind, .. } => {
            let fill = if *button_kind == StudioButtonKind::Primary {
                &accent
            } else {
                &bg_raised
            };
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{fill}" stroke="{border_def}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="11" text-anchor="middle">{}</text>"#,
                r.center().x,
                r.center().y + 4.0,
                xml_escape(&node.label)
            )
            .unwrap();
        }
        UiNodeKind::IconButton { selected, .. } => {
            let fill = if *selected { &accent } else { &bg_raised };
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{fill}" stroke="{border_sub}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
        }
        UiNodeKind::Checkbox { checked } | UiNodeKind::Switch { on: checked } => {
            let box_fill = if *checked { &accent } else { &bg_field };
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="16" height="16" rx="3" fill="{box_fill}" stroke="{border_def}" stroke-width="1" />"#,
                r.left() + 2.0,
                r.center().y - 8.0
            )
            .unwrap();
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="11">{}</text>"#,
                r.left() + 24.0,
                r.center().y + 4.0,
                xml_escape(&node.label)
            )
            .unwrap();
        }
        UiNodeKind::ColorSwatch { rgba, .. } => {
            let sw_hex = color32_to_hex(Color32::from_rgba_unmultiplied(
                rgba[0], rgba[1], rgba[2], rgba[3],
            ));
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{sw_hex}" stroke="{border_def}" stroke-width="1" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
        }
        UiNodeKind::Slider {
            value, min, max, ..
        } => {
            let cy = r.center().y;
            let span = (max - min).abs().max(1.0);
            let ratio = ((value - min) / span).clamp(0.0, 1.0) as f32;
            let tx = r.left() + 8.0 + (r.width() - 16.0).max(12.0) * ratio;
            writeln!(
                svg,
                r#"{pad}  <line x1="{:.1}" y1="{cy:.1}" x2="{:.1}" y2="{cy:.1}" stroke="{border_def}" stroke-width="3" />"#,
                r.left() + 8.0,
                r.right() - 8.0
            )
            .unwrap();
            writeln!(
                svg,
                r#"{pad}  <circle cx="{tx:.1}" cy="{cy:.1}" r="6" fill="{accent}" />"#
            )
            .unwrap();
        }
        UiNodeKind::Label => {
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="{:.0}">{}</text>"#,
                r.left() + 2.0,
                r.center().y + 4.0,
                b.typography.size_pt,
                xml_escape(&node.label)
            )
            .unwrap();
        }
        UiNodeKind::Separator { vertical } => {
            if *vertical {
                writeln!(
                    svg,
                    r#"{pad}  <line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{border_sub}" stroke-width="1" />"#,
                    r.center().x,
                    r.top(),
                    r.center().x,
                    r.bottom()
                )
                .unwrap();
            } else {
                writeln!(
                    svg,
                    r#"{pad}  <line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{border_sub}" stroke-width="1" />"#,
                    r.left(),
                    r.center().y,
                    r.right(),
                    r.center().y
                )
                .unwrap();
            }
        }
        UiNodeKind::TableRow {
            cells, selected, ..
        } => {
            let row_fill = if *selected { &bg_raised } else { &bg_panel };
            writeln!(
                svg,
                r#"{pad}  <rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{row_fill}" />"#,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            )
            .unwrap();
            writeln!(
                svg,
                r#"{pad}  <text x="{:.1}" y="{:.1}" fill="{text_pri}" font-family="Inter, sans-serif" font-size="11">{}</text>"#,
                r.left() + 8.0,
                r.center().y + 4.0,
                xml_escape(&cells.join("   ·   "))
            )
            .unwrap();
        }
    }

    writeln!(svg, "{pad}</g>").unwrap();
}

fn parse_xml_attr(tag: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{attr_name}=\"");
    let mut search_from = 0usize;
    while let Some(rel) = tag[search_from..].find(&pattern) {
        let idx = search_from + rel;
        let ok_prefix = idx == 0
            || tag.as_bytes()[idx - 1].is_ascii_whitespace()
            || tag.as_bytes()[idx - 1] == b'<';
        if ok_prefix {
            let val_start = idx + pattern.len();
            let val_end = tag[val_start..].find('"')? + val_start;
            return Some(xml_unescape(&tag[val_start..val_end]));
        }
        search_from = idx + pattern.len();
    }
    None
}

fn parse_compact_size_unit(s: &str) -> UiSizeUnit {
    let t = s.trim();
    if let Some(pct) = t.strip_suffix('%')
        && let Ok(v) = pct.trim().parse::<f32>()
    {
        return UiSizeUnit::Percent(v);
    }
    if let Some(px) = t.strip_suffix("px")
        && let Ok(v) = px.trim().parse::<f32>()
    {
        return UiSizeUnit::Px(v);
    }
    if let Some(fr) = t.strip_suffix("fr")
        && let Ok(v) = fr.trim().parse::<f32>()
    {
        return UiSizeUnit::Fill(v);
    }
    if let Ok(v) = t.parse::<f32>() {
        return UiSizeUnit::Px(v);
    }
    UiSizeUnit::Auto
}

/// Sample Figma-style SVG panel markup so the user can test `Import SVG → Panel` with 1 click.
pub fn sample_svg_panel_snippet() -> String {
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="360" viewBox="0 0 320 360">
  <rect x="0" y="0" width="320" height="360" rx="6" fill="#1E222A" stroke="#343B48" />
  <text x="12" y="22" font-size="12" fill="#E6EAF2">SVG Imported Panel — Camera &amp; Blur</text>
  <line x1="0" y1="32" x2="320" y2="32" stroke="#2D3340" />
  <text x="12" y="54" font-size="10" fill="#9AA3B6">CAMERA TRANSFORM</text>
  <rect x="12" y="64" width="144" height="28" rx="4" fill="#14171D" stroke="#343B48" />
  <text x="20" y="82" font-size="11" fill="#E6EAF2">X 120 px</text>
  <rect x="164" y="64" width="144" height="28" rx="4" fill="#14171D" stroke="#343B48" />
  <text x="172" y="82" font-size="11" fill="#E6EAF2">Y 85 px</text>
  <rect x="12" y="100" width="144" height="28" rx="4" fill="#14171D" stroke="#343B48" />
  <text x="20" y="118" font-size="11" fill="#E6EAF2">Zoom 100 %</text>
  <rect x="164" y="100" width="144" height="28" rx="4" fill="#14171D" stroke="#343B48" />
  <text x="172" y="118" font-size="11" fill="#E6EAF2">Tilt 0 °</text>
  <text x="12" y="152" font-size="10" fill="#9AA3B6">DEPTH OF FIELD</text>
  <rect x="12" y="164" width="16" height="16" rx="3" fill="#4C84FF" />
  <text x="36" y="176" font-size="11" fill="#E6EAF2">Enable Depth Blur</text>
  <line x1="12" y1="204" x2="308" y2="204" stroke="#343B48" stroke-width="3" />
  <circle cx="180" cy="204" r="6" fill="#4C84FF" />
  <rect x="12" y="232" width="296" height="30" rx="4" fill="#4C84FF" />
  <text x="160" y="251" font-size="11" fill="#FFFFFF">Apply Camera Keyframe</text>
</svg>"##
        .to_owned()
}

/// Reconstruct a live, editable [`UiStudioDocument`] panel from any SVG XML string!
/// Supports both:
/// 1. Exact round-trip SVGs exported by NeoFlash UI Studio (`data-nf-kind` groups), and
/// 2. Raw Figma / Illustrator / browser SVGs (`<rect>`, `<text>`, `<line>`, `<circle>`) by classifying
///    shapes into NF2 controls (`Field`, `Checkbox`, `Slider`, `Button`, `Section`, `Separator`) and
///    grouping horizontal rows into responsive `Grid` / `Row` containers.
pub fn import_svg_to_document(svg_xml: &str) -> Result<UiStudioDocument, String> {
    let trimmed = svg_xml.trim();
    if !trimmed.contains("<svg") {
        return Err("Input does not contain an <svg> root element.".to_owned());
    }

    // 1. Check for NeoFlash Studio round-trip metadata (`data-nf-kind`)
    if trimmed.contains("data-nf-kind=\"") {
        return import_roundtrip_nf_svg(trimmed);
    }

    // 2. General / Raw Figma or Illustrator SVG -> Panel reconstruction
    import_raw_svg_to_panel(trimmed)
}

fn import_roundtrip_nf_svg(svg_xml: &str) -> Result<UiStudioDocument, String> {
    let mut doc_name = "Imported SVG Panel".to_owned();
    let mut next_id = 1_u64;
    let mut stack: Vec<UiStudioNode> = Vec::new();
    let mut root_node: Option<UiStudioNode> = None;

    let mut pos = 0usize;
    while let Some(lt) = svg_xml[pos..].find('<') {
        let start = pos + lt;
        let Some(gt_rel) = svg_xml[start..].find('>') else {
            break;
        };
        let end = start + gt_rel;
        let tag = &svg_xml[start..=end];
        pos = end + 1;

        if tag.starts_with("<svg")
            && let Some(n) = parse_xml_attr(tag, "data-nf-doc-name")
        {
            doc_name = n;
        } else if tag.starts_with("<g ") && tag.contains("data-nf-kind=\"") {
            let id = parse_xml_attr(tag, "id")
                .and_then(|s| s.strip_prefix("node-").and_then(|n| n.parse::<u64>().ok()))
                .unwrap_or_else(|| {
                    let i = next_id;
                    next_id += 1;
                    i
                });
            next_id = next_id.max(id + 1);
            let kind_str = parse_xml_attr(tag, "data-nf-kind").unwrap_or_default();
            let label = parse_xml_attr(tag, "data-nf-label").unwrap_or_else(|| kind_str.clone());
            let w_unit = parse_xml_attr(tag, "data-nf-w")
                .map(|s| parse_compact_size_unit(&s))
                .unwrap_or(UiSizeUnit::Percent(100.0));
            let h_unit = parse_xml_attr(tag, "data-nf-h")
                .map(|s| parse_compact_size_unit(&s))
                .unwrap_or(UiSizeUnit::Auto);

            let kind = match kind_str.as_str() {
                "Panel" => {
                    let tabs = parse_xml_attr(tag, "data-nf-tabs")
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split('|').map(str::to_owned).collect())
                        .unwrap_or_else(|| vec![label.clone()]);
                    let active_tab = parse_xml_attr(tag, "data-nf-active-tab")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    UiNodeKind::Panel { tabs, active_tab }
                }
                "Section" => UiNodeKind::Section {
                    collapsible: true,
                    expanded: true,
                },
                "Row" => UiNodeKind::Row,
                "Column" => UiNodeKind::Column,
                "Grid" => {
                    let columns = parse_xml_attr(tag, "data-nf-cols")
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(parse_compact_size_unit).collect())
                        .unwrap_or_else(|| {
                            vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)]
                        });
                    UiNodeKind::Grid { columns }
                }
                "Field" => UiNodeKind::Field {
                    value: parse_xml_attr(tag, "data-nf-val")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0),
                    prefix: parse_xml_attr(tag, "data-nf-prefix").unwrap_or_default(),
                    unit: parse_xml_attr(tag, "data-nf-unit").unwrap_or_default(),
                    decimals: parse_xml_attr(tag, "data-nf-dec")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1),
                    stepper: parse_xml_attr(tag, "data-nf-step").is_some_and(|s| s == "true"),
                },
                "TextField" => UiNodeKind::TextField {
                    text: parse_xml_attr(tag, "data-nf-text").unwrap_or_default(),
                    prefix: parse_xml_attr(tag, "data-nf-prefix").unwrap_or_default(),
                    hint: parse_xml_attr(tag, "data-nf-hint").unwrap_or_default(),
                    mono: true,
                },
                "Select" => UiNodeKind::Select {
                    selected: parse_xml_attr(tag, "data-nf-sel")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                    options: parse_xml_attr(tag, "data-nf-opts")
                        .map(|s| s.split('|').map(str::to_owned).collect())
                        .unwrap_or_else(|| vec![label.clone()]),
                },
                "Button" => UiNodeKind::Button {
                    button_kind: StudioButtonKind::Primary,
                    icon_name: None,
                },
                "IconButton" => UiNodeKind::IconButton {
                    icon_name: parse_xml_attr(tag, "data-nf-icon")
                        .unwrap_or_else(|| "select".to_owned()),
                    selected: parse_xml_attr(tag, "data-nf-sel").is_some_and(|s| s == "true"),
                    md_size: false,
                },
                "Checkbox" => UiNodeKind::Checkbox {
                    checked: parse_xml_attr(tag, "data-nf-checked").is_none_or(|s| s == "true"),
                },
                "Switch" => UiNodeKind::Switch {
                    on: parse_xml_attr(tag, "data-nf-on").is_none_or(|s| s == "true"),
                },
                "ColorSwatch" => {
                    let rgba = parse_xml_attr(tag, "data-nf-rgba")
                        .and_then(|s| {
                            let p: Vec<u8> = s.split(',').filter_map(|x| x.parse().ok()).collect();
                            (p.len() == 4).then(|| [p[0], p[1], p[2], p[3]])
                        })
                        .unwrap_or([108, 147, 255, 255]);
                    let is_stroke =
                        parse_xml_attr(tag, "data-nf-stroke").is_some_and(|s| s == "true");
                    UiNodeKind::ColorSwatch { rgba, is_stroke }
                }
                "Slider" => UiNodeKind::Slider {
                    value: parse_xml_attr(tag, "data-nf-val")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(50.0),
                    min: parse_xml_attr(tag, "data-nf-min")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0),
                    max: parse_xml_attr(tag, "data-nf-max")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(100.0),
                    show_field: true,
                },
                "Separator" => UiNodeKind::Separator { vertical: false },
                "TableRow" => UiNodeKind::TableRow {
                    cells: parse_xml_attr(tag, "data-nf-cells")
                        .map(|s| s.split('|').map(str::to_owned).collect())
                        .unwrap_or_else(|| vec![label.clone()]),
                    column_fractions: parse_xml_attr(tag, "data-nf-fracs")
                        .map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect())
                        .unwrap_or_else(|| vec![0.0, 0.6]),
                    is_header: parse_xml_attr(tag, "data-nf-hdr").is_some_and(|s| s == "true"),
                    selected: parse_xml_attr(tag, "data-nf-sel").is_some_and(|s| s == "true"),
                },
                _ => UiNodeKind::Label,
            };

            let mut node = UiStudioNode::new(id, label, kind);
            node.box_constraints.width = w_unit;
            node.box_constraints.height = h_unit;
            stack.push(node);
        } else if tag == "</g>" && !stack.is_empty() {
            let finished = stack.pop().unwrap();
            if let Some(parent) = stack.last_mut() {
                parent.children.push(finished);
            } else if root_node.is_none() {
                root_node = Some(finished);
            }
        }
    }

    while let Some(finished) = stack.pop() {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(finished);
        } else if root_node.is_none() {
            root_node = Some(finished);
        }
    }

    let root = root_node.ok_or_else(|| "No NF2 root node found in SVG.".to_owned())?;
    Ok(UiStudioDocument {
        name: doc_name,
        root,
        next_id: next_id.max(2),
    })
}

#[derive(Clone, Debug)]
struct RawSvgRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill: Option<String>,
}

#[derive(Clone, Debug)]
struct RawSvgText {
    x: f32,
    y: f32,
    font_size: f32,
    text: String,
    used: bool,
}

#[derive(Clone, Debug)]
struct RawSvgLine {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

#[derive(Clone, Debug)]
struct RawSvgCircle {
    cx: f32,
    cy: f32,
    r: f32,
}

fn import_raw_svg_to_panel(svg_xml: &str) -> Result<UiStudioDocument, String> {
    let mut svg_w = 320.0_f32;
    let mut svg_h = 400.0_f32;
    let mut rects = Vec::new();
    let mut texts = Vec::new();
    let mut lines = Vec::new();
    let mut circles = Vec::new();

    let mut pos = 0usize;
    while let Some(lt) = svg_xml[pos..].find('<') {
        let start = pos + lt;
        let Some(gt_rel) = svg_xml[start..].find('>') else {
            break;
        };
        let end = start + gt_rel;
        let tag = &svg_xml[start..=end];
        pos = end + 1;

        if tag.starts_with("<svg") {
            if let Some(w) = parse_xml_attr(tag, "width")
                .and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
            {
                svg_w = w.max(120.0);
            }
            if let Some(h) = parse_xml_attr(tag, "height")
                .and_then(|s| s.trim_end_matches("px").parse::<f32>().ok())
            {
                svg_h = h.max(120.0);
            }
        } else if tag.starts_with("<rect") {
            let x = parse_xml_attr(tag, "x")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let y = parse_xml_attr(tag, "y")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let w = parse_xml_attr(tag, "width")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let h = parse_xml_attr(tag, "height")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let fill = parse_xml_attr(tag, "fill");
            if w > 4.0 && h > 4.0 {
                rects.push(RawSvgRect { x, y, w, h, fill });
            }
        } else if tag.starts_with("<line") {
            let x1 = parse_xml_attr(tag, "x1")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let y1 = parse_xml_attr(tag, "y1")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let x2 = parse_xml_attr(tag, "x2")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let y2 = parse_xml_attr(tag, "y2")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            lines.push(RawSvgLine { x1, y1, x2, y2 });
        } else if tag.starts_with("<circle") {
            let cx = parse_xml_attr(tag, "cx")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let cy = parse_xml_attr(tag, "cy")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let r = parse_xml_attr(tag, "r")
                .and_then(|s| s.parse().ok())
                .unwrap_or(4.0);
            circles.push(RawSvgCircle { cx, cy, r });
        } else if tag.starts_with("<text")
            && !tag.ends_with("/>")
            && let Some(close_rel) = svg_xml[pos..].find("</text>")
        {
            let inner = xml_unescape(svg_xml[pos..pos + close_rel].trim());
            pos += close_rel + "</text>".len();
            if !inner.is_empty() {
                let x = parse_xml_attr(tag, "x")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let y = parse_xml_attr(tag, "y")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let font_size = parse_xml_attr(tag, "font-size")
                    .and_then(|s| s.trim_end_matches("px").parse().ok())
                    .unwrap_or(11.0);
                texts.push(RawSvgText {
                    x,
                    y,
                    font_size,
                    text: inner,
                    used: false,
                });
            }
        }
    }

    let mut next_id = 1_u64;
    let mut alloc = || {
        let id = next_id;
        next_id += 1;
        id
    };

    // Determine panel title from top-most text if present
    let mut panel_title = "SVG Imported Panel".to_owned();
    for t in &mut texts {
        if t.y <= 30.0 {
            panel_title = t.text.clone();
            t.used = true;
            break;
        }
    }

    // Collect positioned items `(y_center, x_left, width_px, UiStudioNode)`
    let mut positioned_items: Vec<(f32, f32, f32, UiStudioNode)> = Vec::new();

    for r in &rects {
        // Skip outer background frame rect covering >=85% of the SVG
        if r.w >= svg_w * 0.85 && r.h >= svg_h * 0.80 {
            continue;
        }
        // Skip full-width header background strip at y == 0
        if r.y <= 2.0 && r.w >= svg_w * 0.85 && r.h <= 40.0 {
            continue;
        }

        // Small checkbox square (10..=22 px) with adjacent text on the right
        if (10.0..=22.0).contains(&r.w) && (10.0..=22.0).contains(&r.h) {
            let mut chk_label = "Option".to_owned();
            for t in &mut texts {
                if !t.used && (t.y - r.y - r.h * 0.7).abs() <= 14.0 && t.x >= r.x + r.w - 2.0 {
                    chk_label = t.text.clone();
                    t.used = true;
                    break;
                }
            }
            let node =
                UiStudioNode::new(alloc(), chk_label, UiNodeKind::Checkbox { checked: true })
                    .with_width(UiSizeUnit::Percent(100.0));
            positioned_items.push((r.y + r.h * 0.5, r.x, r.w, node));
            continue;
        }

        // Square IconButton (22..=36 px) without inside text
        if (22.0..=36.0).contains(&r.w) && (22.0..=36.0).contains(&r.h) && (r.w - r.h).abs() <= 4.0
        {
            if let Some(rgba) = r.fill.as_deref().and_then(parse_svg_hex_color)
                && (rgba[0] > 80 || rgba[1] > 80 || rgba[2] > 100)
            {
                let node = UiStudioNode::new(
                    alloc(),
                    "Swatch",
                    UiNodeKind::ColorSwatch {
                        rgba,
                        is_stroke: false,
                    },
                )
                .with_width(UiSizeUnit::Px(r.w));
                positioned_items.push((r.y + r.h * 0.5, r.x, r.w, node));
                continue;
            }
            let node = UiStudioNode::new(
                alloc(),
                "Icon Action",
                UiNodeKind::IconButton {
                    icon_name: "select".to_owned(),
                    selected: false,
                    md_size: false,
                },
            )
            .with_width(UiSizeUnit::Px(r.w));
            positioned_items.push((r.y + r.h * 0.5, r.x, r.w, node));
            continue;
        }

        // Find text inside this rect
        let mut inside_text: Option<String> = None;
        for t in &mut texts {
            if !t.used
                && t.x >= r.x - 4.0
                && t.x <= r.x + r.w + 4.0
                && t.y >= r.y
                && t.y <= r.y + r.h + 6.0
            {
                inside_text = Some(t.text.clone());
                t.used = true;
                break;
            }
        }

        let txt = inside_text.unwrap_or_else(|| "Control".to_owned());
        let tokens: Vec<&str> = txt.split_whitespace().collect();
        let node =
            if tokens.len() >= 2
                && let Ok(num) = tokens[1]
                    .trim_end_matches(['%', 'p', 'x', '°'])
                    .parse::<f64>()
            {
                let prefix = tokens[0].trim_end_matches(':').to_owned();
                let unit = if tokens.len() >= 3 {
                    tokens[2].to_owned()
                } else {
                    String::new()
                };
                UiStudioNode::new(
                    alloc(),
                    format!("Field {prefix}"),
                    UiNodeKind::Field {
                        value: num,
                        prefix,
                        unit,
                        decimals: 1,
                        stepper: false,
                    },
                )
                .with_width(UiSizeUnit::Px(r.w))
            } else if r.fill.as_deref().is_some_and(|f| {
                f.eq_ignore_ascii_case("#4C84FF") || f.eq_ignore_ascii_case("#3B82F6")
            }) {
                UiStudioNode::new(
                    alloc(),
                    txt,
                    UiNodeKind::Button {
                        button_kind: StudioButtonKind::Primary,
                        icon_name: None,
                    },
                )
                .with_width(UiSizeUnit::Percent(100.0))
            } else {
                UiStudioNode::new(
                    alloc(),
                    txt.clone(),
                    UiNodeKind::TextField {
                        text: txt,
                        prefix: "".to_owned(),
                        hint: "".to_owned(),
                        mono: false,
                    },
                )
                .with_width(UiSizeUnit::Px(r.w))
            };
        positioned_items.push((r.y + r.h * 0.5, r.x, r.w, node));
    }

    // Horizontal lines: either a Slider (if a circle sits on the line) or a Separator
    for ln in &lines {
        if (ln.y1 - ln.y2).abs() <= 2.0 && (ln.x2 - ln.x1).abs() >= 48.0 {
            let ly = ln.y1;
            if ly <= 36.0 {
                continue;
            }
            let min_x = ln.x1.min(ln.x2);
            let max_x = ln.x1.max(ln.x2);
            if let Some(thumb) = circles
                .iter()
                .find(|c| (c.cy - ly).abs() <= 8.0 && c.cx >= min_x - 4.0 && c.cx <= max_x + 4.0)
            {
                let pct =
                    (((thumb.cx - min_x) / (max_x - min_x).max(1.0)) * 100.0).clamp(0.0, 100.0);
                let node = UiStudioNode::new(
                    alloc(),
                    "Slider",
                    UiNodeKind::Slider {
                        value: f64::from(pct.round()),
                        min: 0.0,
                        max: 100.0,
                        show_field: true,
                    },
                )
                .with_width(UiSizeUnit::Percent(100.0));
                positioned_items.push((ly, min_x, max_x - min_x, node));
            } else if (max_x - min_x) >= svg_w * 0.5 {
                let node = UiStudioNode::new(
                    alloc(),
                    "Divider",
                    UiNodeKind::Separator { vertical: false },
                )
                .with_width(UiSizeUnit::Percent(100.0));
                positioned_items.push((ly, min_x, max_x - min_x, node));
            }
        }
    }

    // Remaining unused texts -> Section headers (if uppercase) or Labels
    for t in &texts {
        if t.used {
            continue;
        }
        let is_sec = t.text.len() >= 3
            && t.text
                .chars()
                .all(|c| !c.is_alphabetic() || c.is_uppercase());
        let mut node = if is_sec {
            UiStudioNode::new(
                alloc(),
                t.text.clone(),
                UiNodeKind::Section {
                    collapsible: true,
                    expanded: true,
                },
            )
            .with_system_binding(SharedSystemKind::SectionHeader)
        } else {
            let mut lbl = UiStudioNode::new(alloc(), t.text.clone(), UiNodeKind::Label);
            lbl.style.typography = TypographyBinding::Custom(TypographyOverride {
                size_pt: t.font_size.clamp(9.0, 28.0),
                ..Default::default()
            });
            lbl
        };
        node.box_constraints.width = UiSizeUnit::Percent(100.0);
        positioned_items.push((t.y, t.x, svg_w * 0.8, node));
    }

    // Sort top-to-bottom by Y, then left-to-right by X, and group horizontal bands into Grids & Sections!
    positioned_items.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));

    let mut rows: Vec<Vec<(f32, f32, f32, UiStudioNode)>> = Vec::new();
    for item in positioned_items {
        if let Some(last_row) = rows.last_mut()
            && (item.0 - last_row[0].0).abs() <= 14.0
        {
            last_row.push(item);
        } else {
            rows.push(vec![item]);
        }
    }

    let mut panel_children: Vec<UiStudioNode> = Vec::new();
    let mut active_section: Option<UiStudioNode> = None;

    for mut row in rows {
        row.sort_by(|a, b| a.1.total_cmp(&b.1));
        if row.len() == 1 && matches!(row[0].3.kind, UiNodeKind::Section { .. }) {
            if let Some(sec) = active_section.take() {
                panel_children.push(sec);
            }
            active_section = Some(row.pop().unwrap().3);
            continue;
        }
        let built_row = if row.len() == 1 {
            let mut single = row.pop().unwrap().3;
            if matches!(single.box_constraints.width, UiSizeUnit::Px(w) if w >= svg_w * 0.75) {
                single.box_constraints.width = UiSizeUnit::Percent(100.0);
            }
            single
        } else {
            let n = row.len();
            let eq_pct = 100.0 / (n as f32);
            let cols = vec![UiSizeUnit::Percent(eq_pct); n];
            let children = row
                .into_iter()
                .map(|(_, _, _, mut child)| {
                    child.box_constraints.width = UiSizeUnit::Percent(100.0);
                    child
                })
                .collect();
            UiStudioNode::new(
                alloc(),
                format!("SVG {n}-Col Grid"),
                UiNodeKind::Grid { columns: cols },
            )
            .with_system_binding(SharedSystemKind::TwoColumnForm)
            .with_children(children)
        };
        if let Some(sec) = active_section.as_mut() {
            sec.children.push(built_row);
        } else {
            panel_children.push(built_row);
        }
    }
    if let Some(sec) = active_section.take() {
        panel_children.push(sec);
    }

    let root = UiStudioNode::new(
        alloc(),
        panel_title.clone(),
        UiNodeKind::Panel {
            tabs: vec![panel_title.clone()],
            active_tab: 0,
        },
    )
    .with_width(UiSizeUnit::Px(svg_w))
    .with_system_binding(SharedSystemKind::PanelHeader)
    .with_children(panel_children);

    Ok(UiStudioDocument {
        name: panel_title,
        root,
        next_id,
    })
}

/// Active tab in the right-hand Studio Inspector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioInspectorTab {
    /// Inspect & edit the selected node's `%`/`px` box sizing, paddings, gaps, typography, and grid tracks.
    #[default]
    NodeInspector,
    /// Edit NF2 Systems as Systems (`Spacing`, `Typography`, `Density`, `Shared Layouts`, `Palette`).
    SystemsTab,
    /// View & export generated Rust NF2 layout code and JSON overrides.
    CodeExport,
}

/// Sub-section inside [`StudioInspectorTab::SystemsTab`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemsSubTab {
    #[default]
    TimelineGeometry,
    WorkspaceAndStage,
    SpacingAndDensity,
    TypographyScale,
    SharedLayouts,
    PaletteTokens,
}

/// A free-positioned, live-resizable panel artboard on the Multi-Panel Studio Canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudioArtboard {
    pub id: u32,
    pub preset: StudioPanelPreset,
    pub document: UiStudioDocument,
    /// Position on the free artboard canvas (in canvas coordinates, px).
    pub canvas_pos: [f32; 2],
    /// Artboard width & height on the canvas (user can drag-resize width/height live to test % vs px!).
    pub canvas_size: [f32; 2],
    pub visible: bool,
}

impl StudioArtboard {
    pub fn default_size_for(preset: StudioPanelPreset) -> [f32; 2] {
        match preset {
            StudioPanelPreset::FullWorkspaceShell => [1180.0, 680.0],
            StudioPanelPreset::TimelineStudio => [1310.0, 390.0],
            StudioPanelPreset::LibraryAndAssetsStudio => [290.0, 480.0],
            StudioPanelPreset::ColorAndGradientStudio => [280.0, 480.0],
            StudioPanelPreset::AlignAndTransform => [260.0, 480.0],
            StudioPanelPreset::PropertiesMovieClip => [300.0, 480.0],
            StudioPanelPreset::PropertiesText => [300.0, 480.0],
            StudioPanelPreset::ColorPanel => [280.0, 440.0],
            StudioPanelPreset::ToolsPanel => [84.0, 480.0],
            StudioPanelPreset::FiltersPanel => [290.0, 420.0],
            StudioPanelPreset::TimelineHeader => [620.0, 96.0],
        }
    }

    /// Default startup multi-panel board laying out 6 core NeoFlash panels + timeline without clipping.
    pub fn default_board() -> Vec<Self> {
        let specs = [
            (
                1,
                StudioPanelPreset::PropertiesMovieClip,
                [24.0, 28.0],
                [300.0, 480.0],
            ),
            (
                2,
                StudioPanelPreset::LibraryAndAssetsStudio,
                [348.0, 28.0],
                [290.0, 480.0],
            ),
            (
                3,
                StudioPanelPreset::ColorAndGradientStudio,
                [662.0, 28.0],
                [280.0, 480.0],
            ),
            (
                4,
                StudioPanelPreset::AlignAndTransform,
                [966.0, 28.0],
                [260.0, 480.0],
            ),
            (
                5,
                StudioPanelPreset::ToolsPanel,
                [1250.0, 28.0],
                [84.0, 480.0],
            ),
            (
                6,
                StudioPanelPreset::TimelineStudio,
                [24.0, 540.0],
                [1310.0, 390.0],
            ),
        ];
        specs
            .into_iter()
            .map(|(id, preset, canvas_pos, canvas_size)| Self {
                id,
                preset,
                document: preset.build_document(),
                canvas_pos,
                canvas_size,
                visible: true,
            })
            .collect()
    }
}

/// Persisted JSON bundle combining the studio document, multi-panel artboards, and system token scales.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudioPersistedBundle {
    pub document: UiStudioDocument,
    pub systems: UiStudioSystems,
    #[serde(default)]
    pub artboards: Vec<StudioArtboard>,
}

/// Complete interactive state of the UI Layout Studio (`Window ▸ Developer ▸ UI Layout Studio…`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiLayoutStudioState {
    pub open: bool,
    pub active_preset: StudioPanelPreset,
    pub document: UiStudioDocument,
    pub systems: UiStudioSystems,
    #[serde(default = "StudioArtboard::default_board")]
    pub artboards: Vec<StudioArtboard>,
    #[serde(default)]
    pub active_artboard_idx: usize,
    #[serde(default)]
    pub canvas_pan: [f32; 2],
    #[serde(default = "default_canvas_zoom")]
    pub canvas_zoom: f32,
    #[serde(default)]
    pub solo_active_artboard: bool,
    #[serde(default)]
    pub svg_import_modal_open: bool,
    #[serde(default = "sample_svg_panel_snippet")]
    pub svg_buffer: String,
    #[serde(default)]
    pub preset_to_add: StudioPanelPreset,
    pub selected_nodes: Vec<UiNodeId>,
    pub hovered_node: Option<UiNodeId>,
    pub inspector_tab: StudioInspectorTab,
    pub systems_subtab: SystemsSubTab,
    pub show_wireframe_outlines: bool,
    pub show_dimension_badges: bool,
    pub preview_canvas_width: f32,
    pub preview_dark_mode: bool,
    pub status_message: String,
    #[serde(skip)]
    pub history: UiStudioHistory,
    #[serde(skip)]
    pub drag_source_node: Option<UiNodeId>,
}

fn default_canvas_zoom() -> f32 {
    1.0
}

impl Default for UiLayoutStudioState {
    fn default() -> Self {
        let artboards = StudioArtboard::default_board();
        let preset = artboards[0].preset;
        let document = artboards[0].document.clone();
        let root_id = document.root.id;
        Self {
            open: false,
            active_preset: preset,
            document,
            systems: UiStudioSystems::default(),
            artboards,
            active_artboard_idx: 0,
            canvas_pan: [0.0, 0.0],
            canvas_zoom: 1.0,
            solo_active_artboard: false,
            svg_import_modal_open: false,
            svg_buffer: sample_svg_panel_snippet(),
            preset_to_add: StudioPanelPreset::PropertiesText,
            selected_nodes: vec![root_id],
            hovered_node: None,
            inspector_tab: StudioInspectorTab::NodeInspector,
            systems_subtab: SystemsSubTab::TimelineGeometry,
            show_wireframe_outlines: true,
            show_dimension_badges: true,
            preview_canvas_width: 820.0,
            preview_dark_mode: true,
            status_message: "Multi-Panel Artboard Canvas ready — drag headers to move, drag bottom-right grips to resize (% vs px), or Import/Export SVG."
                .to_owned(),
            history: UiStudioHistory::new(64),
            drag_source_node: None,
        }
    }
}

impl UiLayoutStudioState {
    /// Synchronize `self.document` and `self.active_preset` into the currently active artboard.
    pub fn sync_active_to_artboard(&mut self) {
        if self.artboards.is_empty() {
            self.artboards = StudioArtboard::default_board();
            self.active_artboard_idx = 0;
        }
        let idx = self.active_artboard_idx.min(self.artboards.len() - 1);
        self.active_artboard_idx = idx;
        self.artboards[idx].document = self.document.clone();
        self.artboards[idx].preset = self.active_preset;
    }

    /// Select an artboard on the multi-panel canvas by index and make its document active.
    pub fn select_artboard(&mut self, idx: usize) {
        if idx >= self.artboards.len() {
            return;
        }
        self.sync_active_to_artboard();
        self.active_artboard_idx = idx;
        self.active_preset = self.artboards[idx].preset;
        self.document = self.artboards[idx].document.clone();
        self.selected_nodes = vec![self.document.root.id];
    }

    /// Add a new panel artboard to the multi-panel canvas and select it.
    pub fn add_artboard(&mut self, preset: StudioPanelPreset) -> usize {
        self.sync_active_to_artboard();
        let next_id = self
            .artboards
            .iter()
            .map(|a| a.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let size = StudioArtboard::default_size_for(preset);
        let count = self.artboards.len();
        let pos = [
            24.0 + ((count % 4) as f32) * 320.0,
            28.0 + ((count / 4) as f32) * 510.0,
        ];
        let doc = preset.build_document();
        self.artboards.push(StudioArtboard {
            id: next_id,
            preset,
            document: doc.clone(),
            canvas_pos: pos,
            canvas_size: size,
            visible: true,
        });
        let new_idx = self.artboards.len() - 1;
        self.active_artboard_idx = new_idx;
        self.active_preset = preset;
        self.document = doc;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Added '{}' artboard to canvas.", preset.label());
        new_idx
    }

    /// Duplicate an existing artboard on the canvas.
    pub fn duplicate_artboard(&mut self, idx: usize) -> usize {
        self.sync_active_to_artboard();
        let Some(src) = self.artboards.get(idx).cloned() else {
            return self.active_artboard_idx;
        };
        let next_id = self
            .artboards
            .iter()
            .map(|a| a.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let mut dup = src;
        dup.id = next_id;
        dup.canvas_pos[0] += 32.0;
        dup.canvas_pos[1] += 32.0;
        dup.document.name = format!("{} Copy", dup.document.name);
        self.artboards.push(dup.clone());
        let new_idx = self.artboards.len() - 1;
        self.active_artboard_idx = new_idx;
        self.active_preset = dup.preset;
        self.document = dup.document;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = "Duplicated artboard on canvas.".to_owned();
        new_idx
    }

    /// Remove an artboard from the canvas (keeping at least one artboard).
    pub fn remove_artboard(&mut self, idx: usize) {
        if self.artboards.len() <= 1 || idx >= self.artboards.len() {
            return;
        }
        self.sync_active_to_artboard();
        let removed_name = self.artboards[idx].document.name.clone();
        self.artboards.remove(idx);
        let next_idx = self.active_artboard_idx.min(self.artboards.len() - 1);
        self.active_artboard_idx = next_idx;
        self.active_preset = self.artboards[next_idx].preset;
        self.document = self.artboards[next_idx].document.clone();
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Closed artboard '{removed_name}'.");
    }

    /// Resize an artboard on the free canvas (clamped to sensible minimums so it never clips).
    pub fn resize_artboard(&mut self, idx: usize, new_width: f32, new_height: f32) {
        self.sync_active_to_artboard();
        if let Some(ab) = self.artboards.get_mut(idx) {
            ab.canvas_size[0] = new_width.clamp(72.0, 2400.0);
            ab.canvas_size[1] = new_height.clamp(80.0, 1800.0);
            ab.document.root.box_constraints.width = UiSizeUnit::Percent(100.0);
            if idx == self.active_artboard_idx {
                self.document.root.box_constraints.width = UiSizeUnit::Percent(100.0);
            }
        }
    }

    /// Reset to the 6 core NeoFlash panels laid out cleanly on the multi-panel board.
    pub fn open_all_core_artboards(&mut self) {
        self.commit_history("Open All Core Panels");
        self.artboards = StudioArtboard::default_board();
        self.active_artboard_idx = 0;
        self.active_preset = self.artboards[0].preset;
        self.document = self.artboards[0].document.clone();
        self.solo_active_artboard = false;
        self.canvas_pan = [0.0, 0.0];
        self.selected_nodes = vec![self.document.root.id];
        self.status_message =
            "Opened all 6 core NeoFlash panels + Timeline on the artboard canvas.".to_owned();
    }

    /// Auto-arrange all visible artboards into a non-overlapping multi-row grid on the canvas.
    pub fn auto_arrange_artboards(&mut self) {
        self.sync_active_to_artboard();
        let gap_x = 24.0_f32;
        let gap_y = 32.0_f32;
        let max_row_w = 1360.0_f32;
        let mut cursor_x = 24.0_f32;
        let mut cursor_y = 28.0_f32;
        let mut row_max_h = 0.0_f32;

        for ab in &mut self.artboards {
            if !ab.visible {
                continue;
            }
            if cursor_x > 24.0 && cursor_x + ab.canvas_size[0] > max_row_w {
                cursor_x = 24.0;
                cursor_y += row_max_h + gap_y;
                row_max_h = 0.0;
            }
            ab.canvas_pos = [cursor_x, cursor_y];
            cursor_x += ab.canvas_size[0] + gap_x;
            row_max_h = row_max_h.max(ab.canvas_size[1]);
        }
        self.canvas_pan = [0.0, 0.0];
        self.status_message = "Auto-arranged all visible artboards on canvas.".to_owned();
    }

    /// Load one of the built-in NeoFlash panel presets (`Properties — Movie Clip`, `Color`, `Tools`, etc.).
    pub fn load_preset(&mut self, preset: StudioPanelPreset) {
        self.sync_active_to_artboard();
        self.commit_history(format!("Load preset {}", preset.label()));
        let doc = preset.build_document();
        if let Some(existing_idx) = self.artboards.iter().position(|a| a.preset == preset) {
            self.active_artboard_idx = existing_idx;
            self.artboards[existing_idx].visible = true;
            self.artboards[existing_idx].document = doc.clone();
        } else if !self.artboards.is_empty() {
            let idx = self.active_artboard_idx.min(self.artboards.len() - 1);
            self.active_artboard_idx = idx;
            self.artboards[idx].preset = preset;
            self.artboards[idx].document = doc.clone();
            self.artboards[idx].canvas_size = StudioArtboard::default_size_for(preset);
        }
        self.active_preset = preset;
        self.document = doc;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Loaded preset '{}'.", preset.label());
    }

    /// Record an undoable snapshot before mutating `document` or `systems`.
    pub fn commit_history(&mut self, action_label: impl Into<String>) {
        self.sync_active_to_artboard();
        self.history
            .push_snapshot(action_label, &self.document, &self.systems);
    }

    pub fn undo(&mut self) -> bool {
        if let Some(lbl) = self.history.undo(&mut self.document, &mut self.systems) {
            self.sync_active_to_artboard();
            self.status_message = format!("Undo: {lbl}");
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(lbl) = self.history.redo(&mut self.document, &mut self.systems) {
            self.sync_active_to_artboard();
            self.status_message = format!("Redo: {lbl}");
            true
        } else {
            false
        }
    }

    pub fn primary_selected_id(&self) -> UiNodeId {
        self.selected_nodes
            .first()
            .copied()
            .filter(|&id| self.document.find_node(id).is_some())
            .unwrap_or(self.document.root.id)
    }

    pub fn select_single(&mut self, id: UiNodeId) {
        self.selected_nodes.clear();
        self.selected_nodes.push(id);
    }

    pub fn toggle_multi_select(&mut self, id: UiNodeId) {
        if let Some(pos) = self.selected_nodes.iter().position(|&x| x == id) {
            if self.selected_nodes.len() > 1 {
                self.selected_nodes.remove(pos);
            }
        } else {
            self.selected_nodes.push(id);
        }
    }

    /// Export the active artboard to SVG XML.
    pub fn export_active_svg(&self) -> String {
        export_artboard_to_svg(&self.document, &self.systems, self.preview_dark_mode)
    }

    /// Export all visible artboards on the multi-panel board into a combined SVG document.
    pub fn export_board_svg(&self) -> String {
        export_all_artboards_to_svg(self, self.preview_dark_mode)
    }

    /// Write the active or multi-panel SVG export to disk (`target/ui-studio/exported_panel.svg`).
    pub fn save_svg_to_file(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let svg = if self.solo_active_artboard || self.artboards.len() <= 1 {
            self.export_active_svg()
        } else {
            self.export_board_svg()
        };
        fs::write(path, svg).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }

    /// Parse an SVG string (either NeoFlash round-trip SVG or raw Figma/Illustrator SVG) and add it as a live editable artboard on the canvas.
    pub fn import_svg_as_artboard(&mut self, svg_text: &str) -> Result<usize, String> {
        self.sync_active_to_artboard();
        let doc = import_svg_to_document(svg_text)?;
        self.commit_history(format!("Import SVG '{}'", doc.name));
        let next_id = self
            .artboards
            .iter()
            .map(|a| a.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let w = match doc.root.box_constraints.width {
            UiSizeUnit::Px(px) => px.clamp(160.0, 1400.0),
            _ => 320.0,
        };
        let h = match doc.root.box_constraints.height {
            UiSizeUnit::Px(px) => px.clamp(160.0, 1200.0),
            _ => 460.0,
        };
        let count = self.artboards.len();
        let pos = [
            48.0 + ((count % 4) as f32) * 310.0,
            48.0 + ((count / 4) as f32) * 460.0,
        ];
        let preset = StudioPanelPreset::PropertiesMovieClip;
        self.artboards.push(StudioArtboard {
            id: next_id,
            preset,
            document: doc.clone(),
            canvas_pos: pos,
            canvas_size: [w, h],
            visible: true,
        });
        let new_idx = self.artboards.len() - 1;
        self.active_artboard_idx = new_idx;
        self.active_preset = preset;
        self.document = doc;
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!(
            "Imported SVG as editable panel '{}' (#{} nodes).",
            self.document.name,
            self.document.all_node_ids().len()
        );
        Ok(new_idx)
    }

    /// Save studio document and system token overrides as pretty JSON (`target/ui-studio/ui-layout-overrides.json`).
    pub fn save_studio_json(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let mut artboards = self.artboards.clone();
        if let Some(ab) = artboards.get_mut(self.active_artboard_idx) {
            ab.document = self.document.clone();
            ab.preset = self.active_preset;
        }
        let bundle = StudioPersistedBundle {
            document: self.document.clone(),
            systems: self.systems.clone(),
            artboards,
        };
        let json = serde_json::to_string_pretty(&bundle)
            .map_err(|e| format!("Failed to serialize studio JSON: {e}"))?;
        fs::write(path, json).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }

    /// Load studio document and system token overrides from JSON.
    pub fn load_studio_json(&mut self, path: &Path) -> Result<(), String> {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let bundle: StudioPersistedBundle =
            serde_json::from_str(&raw).map_err(|e| format!("Failed to parse studio JSON: {e}"))?;
        self.commit_history("Load JSON overrides");
        self.document = bundle.document;
        self.systems = bundle.systems;
        if !bundle.artboards.is_empty() {
            self.artboards = bundle.artboards;
            self.active_artboard_idx = self.active_artboard_idx.min(self.artboards.len() - 1);
            self.artboards[self.active_artboard_idx].document = self.document.clone();
        } else {
            self.sync_active_to_artboard();
        }
        self.selected_nodes = vec![self.document.root.id];
        self.status_message = format!("Loaded studio overrides from {}", path.display());
        Ok(())
    }

    /// Export generated Rust layout & system token code to disk (`target/ui-studio/exported_layout.rs`).
    pub fn export_rust_to_file(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let code = export_rust_layout_with_systems(&self.document, &self.systems);
        fs::write(path, code).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Interactive UI Entrypoints, Hierarchy Tree, Palette & Live WYSIWYG Canvas
// ─────────────────────────────────────────────────────────────────────────────

fn studio_temp_id() -> egui::Id {
    egui::Id::new("nf2-ui-layout-studio-state")
}

/// Check whether the context-managed UI Layout Studio window is currently open.
pub fn is_studio_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| {
        d.get_temp::<UiLayoutStudioState>(studio_temp_id())
            .is_some_and(|s| s.open)
    })
}

/// Open or close the context-managed UI Layout Studio window.
pub fn set_studio_open(ctx: &egui::Context, open: bool) {
    let mut state = ctx
        .data(|d| d.get_temp::<UiLayoutStudioState>(studio_temp_id()))
        .unwrap_or_default();
    state.open = open;
    ctx.data_mut(|d| d.insert_temp(studio_temp_id(), state));
}

/// Show the context-managed UI Layout Studio floating window if open.
pub fn show_studio_window(ctx: &egui::Context) {
    let Some(mut state) = ctx.data(|d| d.get_temp::<UiLayoutStudioState>(studio_temp_id())) else {
        return;
    };
    if !state.open {
        return;
    }
    ui_layout_studio_window_ui(ctx, &mut state);
    ctx.data_mut(|d| d.insert_temp(studio_temp_id(), state));
}

/// Floating window UI for the Interactive Visual UI & System Layout Studio.
pub fn ui_layout_studio_window_ui(ctx: &egui::Context, state: &mut UiLayoutStudioState) {
    if !state.open {
        return;
    }
    let nf = neoflash_ui::palette(ctx);
    let pos_id = egui::Id::new("nf2-ui-studio-win-pos");
    let default_pos = egui::pos2(
        (ctx.content_rect().left() + 72.0).max(16.0),
        (ctx.content_rect().top() + 56.0).max(16.0),
    );
    let mut pos = ctx
        .data(|d| d.get_temp::<Pos2>(pos_id))
        .unwrap_or(default_pos);
    let mut close_requested = false;
    let mut drag_delta = Vec2::ZERO;
    let win_width = (ctx.content_rect().width() - 96.0).clamp(880.0, 1360.0);

    egui::Area::new(egui::Id::new("nf2-ui-layout-studio-window"))
        .order(egui::Order::Foreground)
        .current_pos(pos)
        .movable(false)
        .constrain(true)
        .show(ctx, |ui| {
            c::Surface::Dialog.frame(ctx).show(ui, |ui| {
                ui.set_width(win_width);
                let top = ui.cursor().min;
                let header_rect = Rect::from_min_size(
                    top,
                    egui::vec2(win_width, neoflash_ui::metrics::dialog_header()),
                );
                let drag = ui.interact(
                    header_rect,
                    ui.id().with("studio-title-drag"),
                    Sense::drag(),
                );
                close_requested = c::DialogHeader {
                    title: "UI Layout & Design System Studio",
                }
                .show(ui);
                if drag.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                drag_delta = drag.drag_delta();
                ui.painter().hline(
                    header_rect.x_range(),
                    header_rect.bottom() - 0.5,
                    Stroke::new(1.0_f32, nf.border_subtle),
                );

                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S3 as i8))
                    .show(ui, |ui| {
                        ui.set_width(win_width - space::S3 * 2.0);
                        ui_layout_studio_panel_ui(ui, state);
                    });
            });
        });

    if drag_delta != Vec2::ZERO {
        pos += drag_delta;
        ctx.data_mut(|d| d.insert_temp(pos_id, pos));
    }
    if close_requested {
        state.open = false;
    }
}

/// Embeddable panel/body UI for the Interactive Visual UI & System Layout Studio.
pub fn ui_layout_studio_panel_ui(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
    state.sync_active_to_artboard();

    // ── Top Toolbar: Preset Selector, Multi-Artboard Controls, SVG Import/Export, Wrap/Unwrap ──
    render_studio_top_toolbar(ui, state);
    c::divider(ui, false);

    // ── Main 3-Column Studio Workspace: [Tree + Palette | Multi-Panel Artboard Canvas | Inspector/Systems] ──
    let total_w = ui.available_width().max(760.0);
    let left_w = (total_w * 0.16).clamp(185.0, 235.0);
    let right_w = (total_w * 0.25).clamp(265.0, 345.0);
    let center_w = (total_w - left_w - right_w - space::S3 * 2.0).max(300.0);
    let body_h = (ui.available_height() - 48.0).clamp(420.0, 1400.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = space::S3;

        // 1. Left Column: Hierarchy Tree + Structural Actions + Widget Creation Palette
        ui.allocate_ui_with_layout(
            egui::vec2(left_w, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_hierarchy_and_palette_column(ui, state, left_w, body_h);
            },
        );

        // 2. Center Column: Multi-Panel Free Artboard Canvas (unclipped pan/zoom + live resize grips)
        ui.allocate_ui_with_layout(
            egui::vec2(center_w, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_live_canvas_column(ui, state, center_w, body_h);
            },
        );

        // 3. Right Column: Node Inspector (% / px / Padding / Gap / Typography) & Systems Tab
        ui.allocate_ui_with_layout(
            egui::vec2(right_w, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_inspector_and_systems_column(ui, state, right_w, body_h);
            },
        );
    });

    state.sync_active_to_artboard();

    if state.svg_import_modal_open {
        render_svg_import_modal(ui.ctx(), state);
    }

    c::divider(ui, false);
    // ── Footer Status Bar ──
    ui.horizontal(|ui| {
        ui.add(c::Badge::new("NF2 Studio", c::BadgeType::Accent));
        ui.label(
            rich(
                format!(
                    "Artboards: {}   ·   Active: {}",
                    state.artboards.iter().filter(|a| a.visible).count(),
                    state.document.name
                ),
                TextStyle::MonoSmall,
            )
            .color(nf.text_tertiary),
        );
        ui.label(rich(&state.status_message, TextStyle::Small).color(nf.text_secondary));
    });
}

fn render_studio_top_toolbar(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;

        // Prominent 1-Click Mode Switcher: [Multi-Panel Board] | [Full Workspace Shell] | [Timeline Studio]
        let is_shell = state.solo_active_artboard
            && state.active_preset == StudioPanelPreset::FullWorkspaceShell;
        let is_timeline =
            state.solo_active_artboard && state.active_preset == StudioPanelPreset::TimelineStudio;
        let is_multi_board = !state.solo_active_artboard;

        let board_btn = if is_multi_board {
            c::Button::primary("Multi-Panel Board (6)").icon(i16::GRID)
        } else {
            c::Button::new("Multi-Panel Board (6)").icon(i16::GRID)
        };
        if ui.add(board_btn).clicked() {
            state.open_all_core_artboards();
        }

        let shell_btn = if is_shell {
            c::Button::primary("Full Workspace Shell").icon(i16::DESKTOP)
        } else {
            c::Button::new("Full Workspace Shell").icon(i16::DESKTOP)
        };
        if ui.add(shell_btn).clicked() {
            state.solo_active_artboard = true;
            state.load_preset(StudioPanelPreset::FullWorkspaceShell);
            state.systems_subtab = SystemsSubTab::WorkspaceAndStage;
        }

        let tl_btn = if is_timeline {
            c::Button::primary("Timeline & Tween Studio").icon(i16::LAYERS)
        } else {
            c::Button::new("Timeline & Tween Studio").icon(i16::LAYERS)
        };
        if ui.add(tl_btn).clicked() {
            state.load_preset(StudioPanelPreset::TimelineStudio);
            state.systems_subtab = SystemsSubTab::TimelineGeometry;
        }

        c::divider(ui, true);

        // Full Preset selector (all 11 workspace, timeline, library, color, align & panel presets)
        let mut preset = state.active_preset;
        let preset_options: [(StudioPanelPreset, &str); 11] = [
            (
                StudioPanelPreset::PropertiesMovieClip,
                "Panel: Properties — Movie Clip",
            ),
            (
                StudioPanelPreset::LibraryAndAssetsStudio,
                "Panel: Library Split (Preview + Tree)",
            ),
            (
                StudioPanelPreset::ColorAndGradientStudio,
                "Panel: Color Mixer, Gradient & Swatches",
            ),
            (
                StudioPanelPreset::AlignAndTransform,
                "Panel: Align & Transform",
            ),
            (StudioPanelPreset::ToolsPanel, "Panel: Tools Rail"),
            (
                StudioPanelPreset::TimelineStudio,
                "Surface: Timeline & Tween Geometry Studio",
            ),
            (
                StudioPanelPreset::FullWorkspaceShell,
                "Surface: Full Editor Workspace — Live Shell",
            ),
            (
                StudioPanelPreset::PropertiesText,
                "Panel: Properties — Text",
            ),
            (StudioPanelPreset::ColorPanel, "Panel: Color (Compact)"),
            (StudioPanelPreset::FiltersPanel, "Panel: Filters"),
            (
                StudioPanelPreset::TimelineHeader,
                "Panel: Timeline Header Bar",
            ),
        ];
        if c::Select::options(
            ui,
            "studio-preset-select",
            &mut preset,
            &preset_options,
            234.0,
        ) {
            state.load_preset(preset);
        }

        if ui
            .add(c::Button::new("+ Add to Canvas").icon(i16::PLUS))
            .clicked()
        {
            state.solo_active_artboard = false;
            state.add_artboard(preset);
        }
        if ui.add(c::Button::new("Auto-Arrange")).clicked() {
            state.auto_arrange_artboards();
        }

        // Undo / Redo
        if ui
            .add_enabled(
                state.history.can_undo(),
                c::IconButton::new(i16::UNDO, "Undo studio edit").shortcut("⌘Z"),
            )
            .clicked()
        {
            state.undo();
        }
        if ui
            .add_enabled(
                state.history.can_redo(),
                c::IconButton::new(i16::REDO, "Redo studio edit").shortcut("⇧⌘Z"),
            )
            .clicked()
        {
            state.redo();
        }

        c::divider(ui, true);

        // Quick 1-click Wrap buttons
        if ui.add(c::Button::new("Wrap Row")).clicked() {
            let sel = state.selected_nodes.clone();
            state.commit_history("Wrap in Row");
            if let Some(new_id) = state
                .document
                .wrap_selected_nodes(&sel, ContainerWrapPreset::Row)
            {
                state.select_single(new_id);
                state.sync_active_to_artboard();
                state.status_message = "Wrapped selection in Row.".to_owned();
            }
        }
        if ui.add(c::Button::new("Wrap Col")).clicked() {
            let sel = state.selected_nodes.clone();
            state.commit_history("Wrap in Column");
            if let Some(new_id) = state
                .document
                .wrap_selected_nodes(&sel, ContainerWrapPreset::Column)
            {
                state.select_single(new_id);
                state.sync_active_to_artboard();
                state.status_message = "Wrapped selection in Column.".to_owned();
            }
        }
        for (btn_label, wrap_preset, desc) in [
            (
                "Wrap 2-Grid",
                ContainerWrapPreset::Grid2Equal,
                "2-column Grid (50%/50%)",
            ),
            (
                "Wrap 3-Grid",
                ContainerWrapPreset::Grid3Equal,
                "3-column Grid (33.3% × 3)",
            ),
            (
                "Wrap 4-Grid",
                ContainerWrapPreset::Grid4Equal,
                "4-column Grid (25% × 4)",
            ),
        ] {
            if ui.add(c::Button::new(btn_label)).clicked() {
                let sel = state.selected_nodes.clone();
                state.commit_history(format!("Wrap in {desc}"));
                if let Some(new_id) = state.document.wrap_selected_nodes(&sel, wrap_preset) {
                    state.select_single(new_id);
                    state.sync_active_to_artboard();
                    state.status_message = format!("Wrapped selection in {desc}.");
                }
            }
        }

        if ui.add(c::Button::new("Unwrap")).clicked() {
            let target = state.primary_selected_id();
            state.commit_history("Unwrap container");
            if state.document.unwrap_selected_container(target) {
                state.sync_active_to_artboard();
                state.status_message = "Unwrapped container children into parent.".to_owned();
            } else if state.document.extract_node_to_parent(target) {
                state.sync_active_to_artboard();
                state.status_message = "Extracted node out of parent container.".to_owned();
            }
        }

        c::divider(ui, true);

        // SVG Export & Import + Save JSON + Export Rust Code
        if ui
            .add(c::Button::new("Export SVG").icon(i16::SAVE))
            .clicked()
        {
            let svg = state.export_active_svg();
            ui.ctx().copy_text(svg.clone());
            state.svg_buffer = svg;
            let svg_path = Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH);
            match state.save_svg_to_file(svg_path) {
                Ok(saved) => {
                    state.inspector_tab = StudioInspectorTab::CodeExport;
                    state.status_message = format!(
                        "Exported SVG to {} (and copied to clipboard).",
                        saved.display()
                    );
                }
                Err(err) => {
                    state.status_message = err;
                }
            }
        }
        if ui
            .add(c::Button::new("Import SVG → Panel…").icon(i16::UPLOAD))
            .clicked()
        {
            state.svg_import_modal_open = true;
        }
        if ui
            .add(c::Button::new("Save JSON").icon(i16::SAVE))
            .clicked()
        {
            let path = Path::new(DEFAULT_STUDIO_JSON_PATH);
            match state.save_studio_json(path) {
                Ok(saved) => {
                    state.status_message = format!("Saved overrides to {}", saved.display());
                }
                Err(err) => {
                    state.status_message = err;
                }
            }
        }
        if ui
            .add(c::Button::primary("Export Build Code").icon(i16::CODE))
            .clicked()
        {
            let json_path = Path::new(DEFAULT_STUDIO_JSON_PATH);
            let _ = state.save_studio_json(json_path);
            let rust_path = Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH);
            match state.export_rust_to_file(rust_path) {
                Ok(saved) => {
                    state.inspector_tab = StudioInspectorTab::CodeExport;
                    state.status_message =
                        format!("Exported Rust build code to {}", saved.display());
                }
                Err(err) => {
                    state.status_message = err;
                }
            }
        }
    });
}

fn render_svg_import_modal(ctx: &egui::Context, state: &mut UiLayoutStudioState) {
    let nf = neoflash_ui::palette(ctx);
    let screen = ctx.content_rect();
    let modal_w = 640.0_f32.min((screen.width() - 48.0).max(360.0));
    let modal_pos = egui::pos2(
        screen.center().x - modal_w * 0.5,
        (screen.top() + 92.0).max(24.0),
    );
    let mut close_modal = false;

    egui::Area::new(egui::Id::new("nf2-studio-svg-import-modal"))
        .order(egui::Order::Foreground)
        .current_pos(modal_pos)
        .constrain(true)
        .show(ctx, |ui| {
            c::Surface::Dialog.frame(ctx).show(ui, |ui| {
                ui.set_width(modal_w);
                if (c::DialogHeader {
                    title: "Import SVG → Live Editable NF2 Panel",
                })
                .show(ui)
                {
                    close_modal = true;
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(space::S3 as i8))
                    .show(ui, |ui| {
                        ui.set_width(modal_w - space::S3 * 2.0);
                        ui.spacing_mut().item_spacing.y = space::S2;
                        ui.label(
                            rich(
                                "Paste any SVG below (exported from NeoFlash UI Studio, Figma, or Illustrator) or load from target/ui-studio/exported_panel.svg. The importer reconstructs Panel, Sections, Grids/Rows, Fields, Selects, Swatches, Sliders, Buttons, and Labels as an interactive artboard on the canvas.",
                                TextStyle::Small,
                            )
                            .color(nf.text_secondary),
                        );

                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add(c::Button::new("Load Sample Figma Panel SVG"))
                                .clicked()
                            {
                                state.svg_buffer = sample_svg_panel_snippet();
                            }
                            if ui
                                .add(c::Button::new("Load Current Active Artboard SVG"))
                                .clicked()
                            {
                                state.svg_buffer = state.export_active_svg();
                            }
                            if ui
                                .add(c::Button::new("Read target/ui-studio/exported_panel.svg"))
                                .clicked()
                            {
                                match fs::read_to_string(DEFAULT_STUDIO_SVG_EXPORT_PATH) {
                                    Ok(raw) => {
                                        state.svg_buffer = raw;
                                        state.status_message = format!(
                                            "Loaded SVG text from {DEFAULT_STUDIO_SVG_EXPORT_PATH}"
                                        );
                                    }
                                    Err(err) => {
                                        state.status_message = format!(
                                            "Could not read {DEFAULT_STUDIO_SVG_EXPORT_PATH}: {err}"
                                        );
                                    }
                                }
                            }
                        });

                        ui.add(
                            c::TextArea::new(&mut state.svg_buffer, "SVG XML Input").rows(14),
                        );

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    c::Button::primary("Convert SVG → Add Panel Artboard")
                                        .icon(i16::CHECK),
                                )
                                .clicked()
                            {
                                let buf = state.svg_buffer.clone();
                                match state.import_svg_as_artboard(&buf) {
                                    Ok(_) => {
                                        state.solo_active_artboard = false;
                                        close_modal = true;
                                    }
                                    Err(err) => {
                                        state.status_message = format!("SVG import error: {err}");
                                    }
                                }
                            }
                            if ui.add(c::Button::new("Cancel")).clicked() {
                                close_modal = true;
                            }
                        });
                    });
            });
        });

    if close_modal {
        state.svg_import_modal_open = false;
    }
}

fn icon_for_node_kind(kind: &UiNodeKind) -> Icon {
    match kind {
        UiNodeKind::Panel { .. } => i16::DESKTOP,
        UiNodeKind::Section { .. } => i16::FOLDEROPEN,
        UiNodeKind::Row => i16::SPACE_H,
        UiNodeKind::Column => i16::SPACE_V,
        UiNodeKind::Grid { .. } => i16::GRID,
        UiNodeKind::Field { .. } | UiNodeKind::TextField { .. } => i16::TEXT,
        UiNodeKind::Select { .. } => i16::CHEVDOWN,
        UiNodeKind::Button { .. } | UiNodeKind::IconButton { .. } => i16::SELECT,
        UiNodeKind::Checkbox { .. } | UiNodeKind::Switch { .. } => i16::CHECK,
        UiNodeKind::ColorSwatch { .. } => i16::SWATCHES,
        UiNodeKind::Slider { .. } => i16::MIXER,
        UiNodeKind::Label => i16::FONT,
        UiNodeKind::Separator { .. } => i16::LINE,
        UiNodeKind::TableRow { .. } => i16::LAYERS,
    }
}

fn resolve_icon_by_name(name: &str) -> Icon {
    let trimmed = name.trim().trim_start_matches("16/");
    if let Some(ico) = Icon::by_path(&format!("16/{trimmed}")) {
        return ico;
    }
    match trimmed {
        "select" => i16::SELECT,
        "subselect" => i16::SUBSELECT,
        "transform" => i16::TRANSFORM,
        "lasso" => i16::LASSO,
        "pen" => i16::PEN,
        "text" => i16::TEXT,
        "line" => i16::LINE,
        "rect" => i16::RECT,
        "oval" => i16::OVAL,
        "pencil" | "ink" | "inkbottle" => i16::PENCIL,
        "brush" => i16::BRUSH,
        "bucket" => i16::BUCKET,
        "eyedropper" => i16::EYEDROPPER,
        "eraser" => i16::ERASER,
        "hand" => i16::HAND,
        "zoom" | "search" => i16::SEARCH,
        "align-left" => i16::ALIGN_LEFT,
        "align-right" => i16::ALIGN_RIGHT,
        "align-top" => i16::ALIGN_TOP,
        "align-bottom" => i16::ALIGN_BOTTOM,
        "space-h" | "align-center-h" | "distribute-h" | "match-width" => i16::SPACE_H,
        "space-v" | "align-center-v" | "distribute-v" | "match-height" => i16::SPACE_V,
        "flip-h" => i16::FLIP_H,
        "flip-v" => i16::FLIP_V,
        "rotate-cw" => i16::ROTATE_CW,
        "rotate-ccw" => i16::ROTATE_CCW,
        "eye" => i16::EYE,
        "lock" => i16::LOCK,
        "folder" | "folderopen" => i16::FOLDEROPEN,
        "folderplus" => i16::FOLDERPLUS,
        "layer" | "layers" => i16::LAYERS,
        "layerplus" | "plus" => i16::PLUS,
        "trash" | "delete" => i16::TRASH,
        "play" => i16::PLAY,
        "loop" => i16::LOOP,
        "onion" => i16::ONION,
        "settings" => i16::SETTINGS,
        "code" => i16::CODE,
        "save" => i16::SAVE,
        "copy" => i16::COPY,
        _ => i16::SELECT,
    }
}

fn render_hierarchy_and_palette_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.set_width(width);

    // Hierarchy Header + Reorder / Duplicate / Delete Toolbar
    ui.horizontal(|ui| {
        ui.label(rich("LAYOUT HIERARCHY", TextStyle::Overline).color(nf.text_secondary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let sel_id = state.primary_selected_id();
            let is_root = sel_id == state.document.root.id;
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::TRASH, "Delete selected").side(24.0),
                )
                .clicked()
            {
                let targets = state.selected_nodes.clone();
                state.commit_history("Delete node(s)");
                let count = state.document.delete_nodes(&targets);
                state.select_single(state.document.root.id);
                state.status_message = format!("Deleted {count} node(s).");
            }
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::COPY, "Duplicate selected").side(24.0),
                )
                .clicked()
            {
                state.commit_history("Duplicate node");
                if let Some(dup_id) = state.document.duplicate_node(sel_id) {
                    state.select_single(dup_id);
                    state.status_message = "Duplicated selected node.".to_owned();
                }
            }
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::CHEVDOWN, "Move node down").side(24.0),
                )
                .clicked()
            {
                state.commit_history("Move node down");
                state.document.move_node_down(sel_id);
            }
            if ui
                .add_enabled(
                    !is_root,
                    c::IconButton::new(i16::CHEVUP, "Move node up").side(24.0),
                )
                .clicked()
            {
                state.commit_history("Move node up");
                state.document.move_node_up(sel_id);
            }
        });
    });

    let tree_h = (height * 0.54).max(180.0);
    egui::Frame::new()
        .fill(nf.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .stroke(Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::same(space::S1 as i8))
        .show(ui, |ui| {
            ui.set_width(width - space::S2);
            egui::ScrollArea::vertical()
                .id_salt("studio-hierarchy-scroll")
                .max_height(tree_h)
                .themed()
                .show(ui, |ui| {
                    let mut rows = Vec::new();
                    collect_tree_rows(&state.document.root, 0, &mut rows);
                    let mut clicked_row: Option<(UiNodeId, bool)> = None;
                    let mut drop_target: Option<(UiNodeId, UiNodeId)> = None;

                    for (id, depth, label, meta, kind_icon, is_container) in rows {
                        let is_selected = state.selected_nodes.contains(&id);
                        let item_kind = if is_container {
                            c::TreeItemType::FolderOpen
                        } else {
                            c::TreeItemType::Symbol(kind_icon)
                        };
                        let tree_item = c::TreeItem::new(&label, item_kind)
                            .icon(kind_icon)
                            .depth(depth)
                            .selected(is_selected)
                            .meta(&meta)
                            .show(ui);

                        if tree_item.row.clicked() {
                            let multi = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                            clicked_row = Some((id, multi));
                        }
                        if tree_item.row.drag_started() && id != state.document.root.id {
                            state.drag_source_node = Some(id);
                        }
                        if let Some(drag_id) = state.drag_source_node
                            && drag_id != id
                            && is_container
                            && tree_item.row.hovered()
                            && ui.input(|i| i.pointer.any_released())
                        {
                            drop_target = Some((drag_id, id));
                        }
                    }

                    if ui.input(|i| i.pointer.any_released()) {
                        state.drag_source_node = None;
                    }
                    if let Some((id, multi)) = clicked_row {
                        if multi {
                            state.toggle_multi_select(id);
                        } else {
                            state.select_single(id);
                        }
                    }
                    if let Some((drag_id, target_container)) = drop_target {
                        state.commit_history("Reparent node in hierarchy");
                        if state
                            .document
                            .reparent_node(drag_id, target_container, None)
                        {
                            state.select_single(drag_id);
                            state.status_message = "Moved node into container.".to_owned();
                        }
                    }
                });
        });

    ui.add_space(space::S1);
    ui.label(rich("ADD CONTAINER / NESTED GRID", TextStyle::Overline).color(nf.text_secondary));
    let target_id = state.primary_selected_id();

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
        if ui.add(c::Button::new("+ Row")).clicked() {
            state.commit_history("Insert Row");
            if let Some(id) = state
                .document
                .insert_node(target_id, "Row", UiNodeKind::Row)
            {
                state.select_single(id);
            }
        }
        if ui.add(c::Button::new("+ Column")).clicked() {
            state.commit_history("Insert Column");
            if let Some(id) = state
                .document
                .insert_node(target_id, "Column", UiNodeKind::Column)
            {
                state.select_single(id);
            }
        }
        if ui.add(c::Button::new("+ Section")).clicked() {
            state.commit_history("Insert Section");
            if let Some(id) = state.document.insert_node(
                target_id,
                "NEW SECTION",
                UiNodeKind::Section {
                    collapsible: true,
                    expanded: true,
                },
            ) {
                state.select_single(id);
            }
        }
        for (lbl, cols) in [
            (
                "+ 2-Col Grid",
                vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
            ),
            (
                "+ 3-Col Grid",
                vec![
                    UiSizeUnit::Percent(33.3333),
                    UiSizeUnit::Percent(33.3333),
                    UiSizeUnit::Percent(33.3334),
                ],
            ),
            (
                "+ 4-Col Grid",
                vec![
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                ],
            ),
            (
                "+ 120px+50%+50%",
                vec![
                    UiSizeUnit::Px(120.0),
                    UiSizeUnit::Percent(50.0),
                    UiSizeUnit::Percent(50.0),
                ],
            ),
        ] {
            if ui.add(c::Button::new(lbl)).clicked() {
                state.commit_history(format!("Insert {lbl}"));
                if let Some(id) = state.document.insert_nested_grid(target_id, cols, true) {
                    state.select_single(id);
                    state.status_message = format!("Inserted nested grid ({lbl}).");
                }
            }
        }
    });

    ui.add_space(space::S1);
    ui.label(rich("ADD NF2 WIDGET", TextStyle::Overline).color(nf.text_secondary));
    egui::ScrollArea::vertical()
        .id_salt("studio-palette-scroll")
        .max_height((height * 0.28).max(96.0))
        .themed()
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S1, space::S1);
                for (btn_lbl, node_lbl, kind) in [
                    (
                        "+ Field",
                        "Numeric Field",
                        UiNodeKind::Field {
                            value: 100.0,
                            prefix: "W".to_owned(),
                            unit: "px".to_owned(),
                            decimals: 0,
                            stepper: false,
                        },
                    ),
                    (
                        "+ TextField",
                        "Text Input",
                        UiNodeKind::TextField {
                            text: "Sample".to_owned(),
                            prefix: "ID".to_owned(),
                            hint: "Enter text…".to_owned(),
                            mono: false,
                        },
                    ),
                    (
                        "+ Select",
                        "Dropdown",
                        UiNodeKind::Select {
                            selected: 0,
                            options: vec!["Option A".to_owned(), "Option B".to_owned()],
                        },
                    ),
                    (
                        "+ Button",
                        "Action",
                        UiNodeKind::Button {
                            button_kind: StudioButtonKind::Secondary,
                            icon_name: None,
                        },
                    ),
                    (
                        "+ IconBtn",
                        "Icon Action",
                        UiNodeKind::IconButton {
                            icon_name: "settings".to_owned(),
                            selected: false,
                            md_size: false,
                        },
                    ),
                    (
                        "+ Checkbox",
                        "Enable option",
                        UiNodeKind::Checkbox { checked: true },
                    ),
                    ("+ Switch", "Live toggle", UiNodeKind::Switch { on: true }),
                    (
                        "+ Swatch",
                        "Fill Color",
                        UiNodeKind::ColorSwatch {
                            rgba: [108, 147, 255, 255],
                            is_stroke: false,
                        },
                    ),
                    (
                        "+ Slider",
                        "Amount",
                        UiNodeKind::Slider {
                            value: 50.0,
                            min: 0.0,
                            max: 100.0,
                            show_field: true,
                        },
                    ),
                    ("+ Label", "Section caption", UiNodeKind::Label),
                    (
                        "+ Divider",
                        "Divider",
                        UiNodeKind::Separator { vertical: false },
                    ),
                    (
                        "+ TableRow",
                        "Table Row",
                        UiNodeKind::TableRow {
                            cells: vec!["Property".to_owned(), "Value".to_owned()],
                            column_fractions: vec![0.0, 0.5],
                            is_header: false,
                            selected: false,
                        },
                    ),
                ] {
                    if ui.add(c::Button::new(btn_lbl)).clicked() {
                        state.commit_history(format!("Insert {node_lbl}"));
                        if let Some(new_id) = state.document.insert_node(target_id, node_lbl, kind)
                        {
                            state.select_single(new_id);
                        }
                    }
                }
            });
        });
}

fn collect_tree_rows(
    node: &UiStudioNode,
    depth: usize,
    out: &mut Vec<(UiNodeId, usize, String, String, Icon, bool)>,
) {
    let w_str = node.box_constraints.width.format_compact();
    let h_str = node.box_constraints.height.format_compact();
    let meta = match &node.kind {
        UiNodeKind::Grid { columns } => format!("{}c · {w_str}", columns.len()),
        _ => format!("{w_str} × {h_str}"),
    };
    out.push((
        node.id,
        depth,
        node.label.clone(),
        meta,
        icon_for_node_kind(&node.kind),
        node.kind.is_container(),
    ));
    for child in &node.children {
        collect_tree_rows(child, depth + 1, out);
    }
}

fn render_live_canvas_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.set_width(width);
    state.sync_active_to_artboard();

    // Canvas Header Controls: Outlines, %/px Badges, Solo Active, Zoom, Auto-Arrange, Open All Core Panels
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        ui.label(rich("MULTI-PANEL ARTBOARD CANVAS", TextStyle::Overline).color(nf.text_secondary));
        ui.add(c::Checkbox::new(
            &mut state.show_wireframe_outlines,
            "Outlines",
        ));
        ui.add(c::Checkbox::new(
            &mut state.show_dimension_badges,
            "% / px Badges",
        ));
        ui.add(c::Checkbox::new(
            &mut state.solo_active_artboard,
            "Solo Active Panel",
        ));

        c::divider(ui, true);

        // Zoom controls (- / 100% / +)
        if ui.add(c::Button::new("−")).clicked() {
            state.canvas_zoom = (state.canvas_zoom - 0.1).clamp(0.35, 2.5);
        }
        let zoom_lbl = format!("{:.0}%", state.canvas_zoom * 100.0);
        if ui.add(c::Button::new(&zoom_lbl)).clicked() {
            state.canvas_zoom = 1.0;
            state.canvas_pan = [0.0, 0.0];
        }
        if ui.add(c::Button::new("+")).clicked() {
            state.canvas_zoom = (state.canvas_zoom + 0.1).clamp(0.35, 2.5);
        }

        if ui.add(c::Button::new("Open All 6 Panels")).clicked() {
            state.open_all_core_artboards();
        }
        if ui.add(c::Button::new("Auto-Arrange")).clicked() {
            state.auto_arrange_artboards();
        }

        let mut density = state.systems.density.active_profile;
        if c::Select::options(
            ui,
            "studio-canvas-density",
            &mut density,
            &[
                (StudioDensityProfile::Comfortable, "Comfortable (32pt)"),
                (StudioDensityProfile::Compact, "Compact (28pt)"),
                (StudioDensityProfile::ClassicFlash, "Classic Flash (24pt)"),
            ],
            144.0,
        ) {
            state.systems.density.active_profile = density;
        }
    });

    let canvas_h = (height - 40.0).max(240.0);
    let dark_mode = ui.ctx().style().visuals.dark_mode;
    let stage_bg = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgApp);
    let border_col = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderSubtle);

    egui::Frame::new()
        .fill(stage_bg)
        .corner_radius(egui::CornerRadius::same(radius::LG as u8))
        .stroke(Stroke::new(1.0_f32, border_col))
        .inner_margin(egui::Margin::same(space::S2 as i8))
        .show(ui, |ui| {
            ui.set_min_size(egui::vec2(
                width - space::S2 * 2.0,
                canvas_h - space::S2 * 2.0,
            ));
            egui::ScrollArea::both()
                .id_salt("studio-live-canvas-scroll")
                .max_height(canvas_h - space::S2 * 2.0)
                .auto_shrink([false, false])
                .themed()
                .show(ui, |ui| {
                    let zoom = state.canvas_zoom.clamp(0.35, 2.5);
                    let indices: Vec<usize> = if state.solo_active_artboard {
                        vec![state.active_artboard_idx.min(state.artboards.len().saturating_sub(1))]
                    } else {
                        state
                            .artboards
                            .iter()
                            .enumerate()
                            .filter(|(_, a)| a.visible)
                            .map(|(i, _)| i)
                            .collect()
                    };

                    // Compute required 2D board dimensions so no panel is ever clipped
                    let mut max_x = (width - space::S3 * 2.0).max(1380.0);
                    let mut max_y = (canvas_h - space::S3 * 2.0).max(960.0);
                    for &idx in &indices {
                        if let Some(ab) = state.artboards.get(idx) {
                            let px = if state.solo_active_artboard {
                                24.0
                            } else {
                                ab.canvas_pos[0] + state.canvas_pan[0]
                            };
                            let py = if state.solo_active_artboard {
                                28.0
                            } else {
                                ab.canvas_pos[1] + state.canvas_pan[1]
                            };
                            max_x = max_x.max(px + ab.canvas_size[0] * zoom + 48.0);
                            max_y = max_y.max(py + ab.canvas_size[1] * zoom + 64.0);
                        }
                    }

                    let board_origin = ui.cursor().min;
                    let (board_rect, board_resp) = ui.allocate_exact_size(
                        egui::vec2(max_x, max_y),
                        Sense::click_and_drag(),
                    );

                    // Subtle pasteboard dot-grid on the multi-panel artboard canvas
                    let grid_step = (24.0 * zoom).clamp(14.0, 48.0);
                    let dot_col = border_col.gamma_multiply(0.45);
                    let mut gx = board_rect.left() + 12.0;
                    while gx < board_rect.right() - 4.0 {
                        let mut gy = board_rect.top() + 12.0;
                        while gy < board_rect.bottom() - 4.0 {
                            ui.painter()
                                .circle_filled(egui::pos2(gx, gy), 1.0, dot_col);
                            gy += grid_step;
                        }
                        gx += grid_step;
                    }

                    // Middle-mouse or Space+drag pans the entire board
                    if board_resp.dragged_by(egui::PointerButton::Middle)
                        || (board_resp.dragged() && ui.input(|i| i.key_down(egui::Key::Space)))
                    {
                        let d = board_resp.drag_delta();
                        state.canvas_pan[0] = (state.canvas_pan[0] + d.x).max(-600.0);
                        state.canvas_pan[1] = (state.canvas_pan[1] + d.y).max(-600.0);
                    }

                    let mut pending_select_artboard: Option<usize> = None;
                    let mut pending_dup_artboard: Option<usize> = None;
                    let mut pending_remove_artboard: Option<usize> = None;
                    let mut pending_svg_export_artboard: Option<usize> = None;

                    for &idx in &indices {
                        let Some(ab_snapshot) = state.artboards.get(idx).cloned() else {
                            continue;
                        };
                        let is_active = idx == state.active_artboard_idx;
                        let pos_x = if state.solo_active_artboard {
                            24.0
                        } else {
                            (ab_snapshot.canvas_pos[0] + state.canvas_pan[0]).max(8.0)
                        };
                        let pos_y = if state.solo_active_artboard {
                            32.0
                        } else {
                            (ab_snapshot.canvas_pos[1] + state.canvas_pan[1]).max(28.0)
                        };

                        let ab_w = (ab_snapshot.canvas_size[0] * zoom).max(76.0);
                        let ab_h = (ab_snapshot.canvas_size[1] * zoom).max(80.0);
                        let ab_rect = Rect::from_min_size(
                            board_origin + egui::vec2(pos_x, pos_y),
                            egui::vec2(ab_w, ab_h),
                        );

                        // ── 1. Artboard Header Bar (draggable title + [W×H px] + [SVG] [Dup] [✕]) ──
                        let hdr_h = 22.0_f32;
                        let hdr_rect = Rect::from_min_size(
                            egui::pos2(ab_rect.left(), ab_rect.top() - hdr_h - 2.0),
                            egui::vec2(ab_rect.width().max(150.0), hdr_h),
                        );
                        let hdr_bg = if is_active {
                            nf.accent_default.gamma_multiply(0.24)
                        } else {
                            nf.bg_raised
                        };
                        let hdr_border = if is_active {
                            nf.accent_default
                        } else {
                            nf.border_subtle
                        };
                        ui.painter().rect_filled(hdr_rect, 4.0, hdr_bg);
                        ui.painter().rect_stroke(
                            hdr_rect,
                            4.0,
                            Stroke::new(1.0_f32, hdr_border),
                            StrokeKind::Inside,
                        );

                        // Active indicator dot + title + size badge
                        let dot_c = if is_active {
                            nf.accent_default
                        } else {
                            nf.text_tertiary
                        };
                        ui.painter().circle_filled(
                            egui::pos2(hdr_rect.left() + 8.0, hdr_rect.center().y),
                            3.5,
                            dot_c,
                        );
                        let title_txt = format!(
                            "{}  ({:.0}×{:.0}px)",
                            ab_snapshot.document.name,
                            ab_snapshot.canvas_size[0],
                            ab_snapshot.canvas_size[1]
                        );
                        ui.painter().text(
                            egui::pos2(hdr_rect.left() + 16.0, hdr_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            title_txt,
                            neoflash_ui::typography::font(TextStyle::MonoSmall),
                            if is_active {
                                nf.text_primary
                            } else {
                                nf.text_secondary
                            },
                        );

                        // Header action pills on the right: [SVG] [Dup] [✕]
                        let close_r = Rect::from_min_size(
                            egui::pos2(hdr_rect.right() - 20.0, hdr_rect.top() + 2.0),
                            egui::vec2(18.0, hdr_h - 4.0),
                        );
                        let dup_r = Rect::from_min_size(
                            egui::pos2(close_r.left() - 30.0, hdr_rect.top() + 2.0),
                            egui::vec2(28.0, hdr_h - 4.0),
                        );
                        let svg_r = Rect::from_min_size(
                            egui::pos2(dup_r.left() - 30.0, hdr_rect.top() + 2.0),
                            egui::vec2(28.0, hdr_h - 4.0),
                        );
                        if hdr_rect.width() >= 190.0 {
                            for (pill_r, pill_txt) in [
                                (svg_r, "SVG"),
                                (dup_r, "Dup"),
                                (close_r, "×"),
                            ] {
                                ui.painter()
                                    .rect_filled(pill_r, 3.0, nf.bg_panel.gamma_multiply(0.85));
                                ui.painter().text(
                                    pill_r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    pill_txt,
                                    egui::FontId::monospace(8.5),
                                    nf.text_secondary,
                                );
                            }
                        }

                        let hdr_drag_rect = Rect::from_min_max(
                            hdr_rect.min,
                            egui::pos2(
                                if hdr_rect.width() >= 190.0 {
                                    svg_r.left() - 2.0
                                } else {
                                    hdr_rect.right()
                                },
                                hdr_rect.bottom(),
                            ),
                        );
                        let hdr_resp = ui.interact(
                            hdr_drag_rect,
                            ui.id().with(("studio-ab-hdr", ab_snapshot.id)),
                            Sense::click_and_drag(),
                        );
                        if hdr_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if hdr_resp.clicked() || hdr_resp.drag_started() {
                            pending_select_artboard = Some(idx);
                        }
                        if hdr_resp.dragged() && !state.solo_active_artboard {
                            let d = hdr_resp.drag_delta();
                            if let Some(ab_mut) = state.artboards.get_mut(idx) {
                                ab_mut.canvas_pos[0] = (ab_mut.canvas_pos[0] + d.x).max(8.0);
                                ab_mut.canvas_pos[1] = (ab_mut.canvas_pos[1] + d.y).max(28.0);
                            }
                        }

                        if hdr_rect.width() >= 190.0 {
                            if ui
                                .interact(
                                    svg_r,
                                    ui.id().with(("studio-ab-svg", ab_snapshot.id)),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                pending_svg_export_artboard = Some(idx);
                            }
                            if ui
                                .interact(
                                    dup_r,
                                    ui.id().with(("studio-ab-dup", ab_snapshot.id)),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                pending_dup_artboard = Some(idx);
                            }
                            if ui
                                .interact(
                                    close_r,
                                    ui.id().with(("studio-ab-close", ab_snapshot.id)),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                pending_remove_artboard = Some(idx);
                            }
                        }

                        // ── 2. Artboard Surface Shadow & Live Unclipped Layout Rendering ──
                        ui.painter().rect_filled(
                            ab_rect.translate(egui::vec2(2.0, 3.0)),
                            6.0,
                            Color32::from_black_alpha(if dark_mode { 72 } else { 28 }),
                        );

                        let doc_for_layout = if is_active {
                            &state.document
                        } else {
                            &ab_snapshot.document
                        };
                        let mut computed = compute_studio_layout(
                            doc_for_layout,
                            &state.systems,
                            ab_rect.min,
                            ab_rect.size(),
                        );

                        if ab_snapshot.preset.is_full_shell() {
                            paint_full_workspace_shell_canvas(
                                ui,
                                state,
                                ab_rect,
                                &mut computed,
                                dark_mode,
                            );
                        } else if ab_snapshot.preset.is_timeline_surface() {
                            paint_timeline_studio_canvas(
                                ui,
                                state,
                                ab_rect,
                                &mut computed,
                                dark_mode,
                            );
                        } else if is_active {
                            paint_node_wysiwyg(
                                ui,
                                &mut state.document.root,
                                &state.systems,
                                &computed,
                                dark_mode,
                            );
                        } else if let Some(ab_mut) = state.artboards.get_mut(idx) {
                            paint_node_wysiwyg(
                                ui,
                                &mut ab_mut.document.root,
                                &state.systems,
                                &computed,
                                dark_mode,
                            );
                        }

                        // Active artboard highlight frame + pointer hit-testing across all artboards
                        if is_active {
                            ui.painter().rect_stroke(
                                ab_rect,
                                6.0,
                                Stroke::new(1.5_f32, nf.accent_default.gamma_multiply(0.85)),
                                StrokeKind::Outside,
                            );
                            paint_canvas_overlays(ui, state, &computed, dark_mode);
                        }

                        // Pointer click inside `ab_rect` selects this artboard and hit-tests its nodes
                        if let Some(pos) = ui.input(|i| i.pointer.hover_pos())
                            && ab_rect.contains(pos)
                            && is_active
                        {
                            state.hovered_node = computed.hit_test(pos);
                        }
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                            && ab_rect.contains(pos)
                            && (board_resp.clicked() || ui.input(|i| i.pointer.primary_clicked()))
                        {
                            if !is_active {
                                pending_select_artboard = Some(idx);
                            }
                            if let Some(hit_id) = computed.hit_test(pos) {
                                let multi =
                                    ui.input(|i| i.modifiers.shift || i.modifiers.command);
                                if is_active {
                                    if multi {
                                        state.toggle_multi_select(hit_id);
                                    } else {
                                        state.select_single(hit_id);
                                    }
                                    if let Some(node) = state.document.find_node(hit_id) {
                                        state.status_message = format!(
                                            "Selected '{}' (#{hit_id}) on '{}' — edit % / px or Systems in right inspector.",
                                            node.label, state.document.name
                                        );
                                    }
                                }
                            }
                        }

                        // ── 3. Interactive Bottom-Right & Right-Edge Resize Grips (live % vs px test!) ──
                        let grip_rect = Rect::from_center_size(
                            ab_rect.right_bottom(),
                            egui::vec2(14.0, 14.0),
                        );
                        ui.painter().rect_filled(
                            grip_rect,
                            3.0,
                            if is_active {
                                nf.accent_default
                            } else {
                                nf.border_strong
                            },
                        );
                        let grip_resp = ui.interact(
                            grip_rect,
                            ui.id().with(("studio-ab-resize", ab_snapshot.id)),
                            Sense::drag(),
                        );
                        if grip_resp.hovered() || grip_resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                        if grip_resp.dragged() {
                            let d = grip_resp.drag_delta() / zoom;
                            if let Some(ab_mut) = state.artboards.get_mut(idx) {
                                ab_mut.canvas_size[0] =
                                    (ab_mut.canvas_size[0] + d.x).clamp(76.0, 2400.0);
                                ab_mut.canvas_size[1] =
                                    (ab_mut.canvas_size[1] + d.y).clamp(80.0, 1800.0);
                                ab_mut.document.root.box_constraints.width =
                                    UiSizeUnit::Percent(100.0);
                                if is_active {
                                    state.document.root.box_constraints.width =
                                        UiSizeUnit::Percent(100.0);
                                }
                                state.status_message = format!(
                                    "Resized '{}' artboard to {:.0}×{:.0}px (testing live % vs px layout).",
                                    ab_mut.document.name,
                                    ab_mut.canvas_size[0],
                                    ab_mut.canvas_size[1]
                                );
                            }
                        }
                    }

                    if let Some(idx) = pending_select_artboard {
                        state.select_artboard(idx);
                    }
                    if let Some(idx) = pending_svg_export_artboard {
                        state.select_artboard(idx);
                        let svg = state.export_active_svg();
                        ui.ctx().copy_text(svg.clone());
                        state.svg_buffer = svg;
                        let _ = state.save_svg_to_file(Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH));
                        state.inspector_tab = StudioInspectorTab::CodeExport;
                        state.status_message = format!(
                            "Exported '{}' SVG to {DEFAULT_STUDIO_SVG_EXPORT_PATH} & clipboard.",
                            state.document.name
                        );
                    }
                    if let Some(idx) = pending_dup_artboard {
                        state.duplicate_artboard(idx);
                    }
                    if let Some(idx) = pending_remove_artboard {
                        state.remove_artboard(idx);
                    }
                });
        });
}

#[derive(Clone, Copy, Debug)]
struct TimelineHitIds {
    layers_col: UiNodeId,
    ruler_playhead: UiNodeId,
    classic_tween: UiNodeId,
    shape_tween: UiNodeId,
    motion_tween: UiNodeId,
    broken_tween: UiNodeId,
    keyframes_badges: UiNodeId,
    audio_waveform: UiNodeId,
}

fn register_hit_box(
    computed: &mut ComputedStudioLayout,
    systems: &UiStudioSystems,
    id: UiNodeId,
    rect: Rect,
) {
    let typo = TypographyBinding::System(StudioTextStyle::Small).resolve(systems);
    computed.boxes.insert(
        id,
        ComputedNodeBox {
            id,
            rect,
            content_rect: rect,
            resolved_width: rect.width(),
            resolved_height: rect.height(),
            padding: [0.0; 4],
            gap: Vec2::ZERO,
            typography: typo,
        },
    );
    if let Some(pos) = computed.order.iter().position(|&x| x == id) {
        computed.order.remove(pos);
    }
    computed.order.push(id);
}

fn paint_full_workspace_shell_canvas(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    shell_rect: Rect,
    computed: &mut ComputedStudioLayout,
    dark_mode: bool,
) {
    computed.root_rect = shell_rect;
    computed.order.clear();
    register_hit_box(computed, &state.systems, 1, shell_rect);

    let nf = neoflash_ui::palette(ui.ctx());
    let bg_app = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgApp);
    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgRaised);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderSubtle);
    let border_def = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderDefault);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::AccentDefault);

    let rects = state
        .systems
        .workspace_shell
        .resolve_shell_rects(shell_rect);
    ui.painter().rect_filled(shell_rect, 6.0, bg_app);
    ui.painter().rect_stroke(
        shell_rect,
        6.0,
        Stroke::new(1.0_f32, border_def),
        StrokeKind::Inside,
    );

    // 1. Top Menu Bar + Document Tabs + Edit Breadcrumb Bar (Node 2)
    let top_bars_rect = rects.menubar.union(rects.doc_tabs).union(rects.edit_bar);
    register_hit_box(computed, &state.systems, 2, top_bars_rect);

    ui.painter().rect_filled(rects.menubar, 0.0, bg_raised);
    let menus = [
        "File", "Edit", "View", "Insert", "Modify", "Text", "Commands", "Control", "Window", "Help",
    ];
    let mut mx = rects.menubar.left() + 10.0;
    for m in menus {
        ui.painter().text(
            egui::pos2(mx, rects.menubar.center().y),
            egui::Align2::LEFT_CENTER,
            m,
            neoflash_ui::typography::font(TextStyle::Small),
            text_pri,
        );
        mx += (m.len() as f32) * 6.4 + 12.0;
    }
    ui.painter().text(
        egui::pos2(rects.menubar.right() - 10.0, rects.menubar.center().y),
        egui::Align2::RIGHT_CENTER,
        "Animator · HTML5 / WASM",
        neoflash_ui::typography::font(TextStyle::MonoSmall),
        text_sec,
    );

    // Document Tabs Bar
    ui.painter().rect_filled(rects.doc_tabs, 0.0, bg_panel);
    let tab1_rect = Rect::from_min_size(
        egui::pos2(rects.doc_tabs.left() + 6.0, rects.doc_tabs.top() + 3.0),
        egui::vec2(108.0, rects.doc_tabs.height() - 3.0),
    );
    let tab2_rect = Rect::from_min_size(
        egui::pos2(tab1_rect.right() + 4.0, rects.doc_tabs.top() + 3.0),
        egui::vec2(114.0, rects.doc_tabs.height() - 3.0),
    );
    ui.painter().rect_filled(tab1_rect, 4.0, bg_raised);
    ui.painter().hline(
        tab1_rect.x_range(),
        tab1_rect.top() + 1.0,
        Stroke::new(2.0_f32, accent),
    );
    ui.painter().text(
        tab1_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Scene 1 *",
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        text_pri,
    );
    ui.painter()
        .rect_filled(tab2_rect, 4.0, bg_panel.gamma_multiply(0.8));
    ui.painter().text(
        tab2_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Character_Rig",
        neoflash_ui::typography::font(TextStyle::Small),
        text_sec,
    );

    // Edit Breadcrumb Bar
    ui.painter().rect_filled(rects.edit_bar, 0.0, bg_raised);
    ui.painter().hline(
        rects.edit_bar.x_range(),
        rects.edit_bar.bottom() - 0.5,
        Stroke::new(1.0_f32, border_sub),
    );
    ui.painter().text(
        egui::pos2(rects.edit_bar.left() + 12.0, rects.edit_bar.center().y),
        egui::Align2::LEFT_CENTER,
        "Scene 1   ▸   Hero_MC (Movie Clip)",
        neoflash_ui::typography::font(TextStyle::Small),
        text_pri,
    );
    ui.painter().text(
        egui::pos2(rects.edit_bar.right() - 12.0, rects.edit_bar.center().y),
        egui::Align2::RIGHT_CENTER,
        "Zoom: 100%  |  Fit Stage",
        neoflash_ui::typography::font(TextStyle::MonoSmall),
        text_sec,
    );

    // 2. Left Tools Dock Rail (Node 3)
    register_hit_box(computed, &state.systems, 3, rects.left_tools);
    ui.painter().rect_filled(rects.left_tools, 4.0, bg_panel);
    ui.painter().rect_stroke(
        rects.left_tools,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );
    let tool_icons = [
        (i16::SELECT, true),
        (i16::SUBSELECT, false),
        (i16::TRANSFORM, false),
        (i16::PEN, false),
        (i16::TEXT, false),
        (i16::LINE, false),
        (i16::RECT, false),
        (i16::OVAL, false),
        (i16::PENCIL, false),
        (i16::BRUSH, false),
        (i16::BUCKET, false),
        (i16::EYEDROPPER, false),
    ];
    let cols = state.systems.tool_rail.columns.clamp(1, 2);
    let btn_side = state.systems.tool_rail.button_side.clamp(
        20.0,
        (rects.left_tools.width() / cols as f32 - 4.0).max(20.0),
    );
    let start_x = rects.left_tools.left()
        + ((rects.left_tools.width() - cols as f32 * (btn_side + 2.0)).max(4.0)) * 0.5;
    let mut ty = rects.left_tools.top() + 6.0;
    for chunk in tool_icons.chunks(cols) {
        for (c_idx, &(ico, active)) in chunk.iter().enumerate() {
            let bx = start_x + c_idx as f32 * (btn_side + 2.0);
            let b_rect = Rect::from_min_size(egui::pos2(bx, ty), egui::vec2(btn_side, btn_side));
            if b_rect.bottom() < rects.left_tools.bottom() - 40.0 {
                if active {
                    ui.painter()
                        .rect_filled(b_rect, 4.0, accent.gamma_multiply(0.28));
                    ui.painter().rect_stroke(
                        b_rect,
                        4.0,
                        Stroke::new(1.0_f32, accent),
                        StrokeKind::Inside,
                    );
                }
                icons::paint(
                    ui.painter(),
                    ui.ctx(),
                    Rect::from_center_size(b_rect.center(), egui::vec2(14.0, 14.0)),
                    ico,
                    if active { text_pri } else { text_sec },
                );
            }
        }
        ty += btn_side + 2.0;
    }
    // Stroke & Fill wells at bottom of Tools rail
    let well_y = (rects.left_tools.bottom() - 28.0).max(rects.left_tools.top() + 8.0);
    let sw_rect = Rect::from_center_size(
        egui::pos2(rects.left_tools.center().x - 6.0, well_y),
        egui::vec2(14.0, 14.0),
    );
    let fw_rect = Rect::from_center_size(
        egui::pos2(rects.left_tools.center().x + 6.0, well_y + 6.0),
        egui::vec2(14.0, 14.0),
    );
    ui.painter()
        .rect_filled(sw_rect, 2.0, Color32::from_rgb(108, 147, 255));
    ui.painter()
        .rect_filled(fw_rect, 2.0, Color32::from_rgb(255, 134, 82));

    // 3. Center Stage Viewport with Rulers, Cyan Guides & Selected Vector Instance (Node 4)
    register_hit_box(computed, &state.systems, 4, rects.center_stage);
    paint_stage_viewport_mock(ui, state, rects.center_stage, dark_mode);

    // 4. Right Multi-Panel Inspector Dock (Node 5)
    register_hit_box(computed, &state.systems, 5, rects.right_inspector);
    paint_right_inspector_dock_mock(ui, state, rects.right_inspector, dark_mode);

    // 5. Bottom Timeline Dock (Node 6 + Sub-Nodes 7..14)
    register_hit_box(computed, &state.systems, 6, rects.bottom_timeline);
    let hit_ids = TimelineHitIds {
        layers_col: 7,
        ruler_playhead: 8,
        classic_tween: 9,
        shape_tween: 10,
        motion_tween: 11,
        broken_tween: 12,
        keyframes_badges: 13,
        audio_waveform: 14,
    };
    let _ = nf;
    paint_interactive_timeline_surface(
        ui,
        state,
        rects.bottom_timeline,
        computed,
        hit_ids,
        dark_mode,
    );
}

fn paint_stage_viewport_mock(
    ui: &mut Ui,
    state: &UiLayoutStudioState,
    rect: Rect,
    dark_mode: bool,
) {
    let pasteboard = if dark_mode {
        Color32::from_rgb(24, 26, 31)
    } else {
        Color32::from_rgb(212, 216, 224)
    };
    let ruler_bg = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgRaised);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderSubtle);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::AccentDefault);

    ui.painter().rect_filled(rect, 4.0, pasteboard);
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );

    let r_thick = if state.systems.stage_overlay.show_rulers {
        state
            .systems
            .stage_overlay
            .stage_ruler_thickness_px
            .clamp(12.0, 32.0)
    } else {
        0.0
    };

    if r_thick > 0.0 {
        let top_ruler = Rect::from_min_size(rect.min, egui::vec2(rect.width(), r_thick));
        let left_ruler = Rect::from_min_size(rect.min, egui::vec2(r_thick, rect.height()));
        ui.painter().rect_filled(top_ruler, 0.0, ruler_bg);
        ui.painter().rect_filled(left_ruler, 0.0, ruler_bg);
        ui.painter().hline(
            top_ruler.x_range(),
            top_ruler.bottom(),
            Stroke::new(1.0_f32, border_sub),
        );
        ui.painter().vline(
            left_ruler.right(),
            left_ruler.y_range(),
            Stroke::new(1.0_f32, border_sub),
        );

        let mut tx = top_ruler.left() + r_thick + 20.0;
        let mut val = 0_i32;
        while tx < top_ruler.right() - 12.0 {
            ui.painter().vline(
                tx,
                (top_ruler.bottom() - 6.0)..=top_ruler.bottom(),
                Stroke::new(1.0_f32, border_sub),
            );
            ui.painter().text(
                egui::pos2(tx + 2.0, top_ruler.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{val}"),
                egui::FontId::monospace(8.5),
                text_sec,
            );
            tx += 50.0;
            val += 100;
        }
        let mut ty = left_ruler.top() + r_thick + 20.0;
        while ty < left_ruler.bottom() - 10.0 {
            ui.painter().hline(
                (left_ruler.right() - 6.0)..=left_ruler.right(),
                ty,
                Stroke::new(1.0_f32, border_sub),
            );
            ty += 50.0;
        }
    }

    let inner = Rect::from_min_max(rect.min + egui::vec2(r_thick, r_thick), rect.max);
    let stage_w = (inner.width() * 0.72).clamp(140.0, 460.0);
    let stage_h = (stage_w
        * (state.systems.stage_overlay.stage_aspect_h
            / state.systems.stage_overlay.stage_aspect_w.max(1.0)))
    .clamp(90.0, (inner.height() - 24.0).max(90.0));
    let stage_rect = Rect::from_center_size(inner.center(), egui::vec2(stage_w, stage_h));

    // Stage paper shadow + surface
    ui.painter().rect_filled(
        stage_rect.translate(egui::vec2(3.0, 4.0)),
        2.0,
        Color32::from_black_alpha(70),
    );
    ui.painter().rect_filled(
        stage_rect,
        2.0,
        if dark_mode {
            Color32::from_rgb(246, 248, 252)
        } else {
            Color32::WHITE
        },
    );

    // Cyan guides
    if state.systems.stage_overlay.show_guides {
        let cyan = Color32::from_rgb(0, 212, 255);
        let gw = state
            .systems
            .stage_overlay
            .guide_stroke_width_px
            .clamp(0.5, 3.0);
        let gx = stage_rect.left() + stage_rect.width() * 0.38;
        let gy = stage_rect.top() + stage_rect.height() * 0.54;
        ui.painter()
            .vline(gx, inner.y_range(), Stroke::new(gw, cyan));
        ui.painter()
            .hline(inner.x_range(), gy, Stroke::new(gw, cyan));
    }

    // Motion guide path & vector character instance with 8 selection handles
    let hero_rect = Rect::from_center_size(
        stage_rect.center(),
        egui::vec2(stage_w * 0.34, stage_h * 0.42),
    );
    ui.painter().circle_filled(
        hero_rect.center(),
        hero_rect.height().min(hero_rect.width()) * 0.38,
        Color32::from_rgb(255, 126, 76),
    );
    ui.painter().rect_stroke(
        hero_rect,
        0.0,
        Stroke::new(1.25_f32, accent),
        StrokeKind::Outside,
    );
    let hs = state
        .systems
        .stage_overlay
        .selection_handle_size_px
        .clamp(3.0, 12.0);
    for pt in [
        hero_rect.left_top(),
        egui::pos2(hero_rect.center().x, hero_rect.top()),
        hero_rect.right_top(),
        egui::pos2(hero_rect.right(), hero_rect.center().y),
        hero_rect.right_bottom(),
        egui::pos2(hero_rect.center().x, hero_rect.bottom()),
        hero_rect.left_bottom(),
        egui::pos2(hero_rect.left(), hero_rect.center().y),
    ] {
        let h_rect = Rect::from_center_size(pt, egui::vec2(hs, hs));
        ui.painter().rect_filled(h_rect, 1.0, Color32::WHITE);
        ui.painter().rect_stroke(
            h_rect,
            1.0,
            Stroke::new(1.0_f32, accent),
            StrokeKind::Outside,
        );
    }
    // Transformation pivot circle
    ui.painter()
        .circle_filled(hero_rect.center(), 3.5, Color32::WHITE);
    ui.painter().circle_stroke(
        hero_rect.center(),
        3.5,
        Stroke::new(1.2_f32, Color32::BLACK),
    );
}

fn paint_right_inspector_dock_mock(
    ui: &mut Ui,
    state: &UiLayoutStudioState,
    rect: Rect,
    dark_mode: bool,
) {
    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgRaised);
    let bg_field = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgField);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderSubtle);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::AccentDefault);

    ui.painter().rect_filled(rect, 4.0, bg_panel);
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );

    let split_y = rect.top() + rect.height() * 0.54;
    let top_pnl = Rect::from_min_max(rect.min, egui::pos2(rect.right(), split_y - 2.0));
    let bot_pnl = Rect::from_min_max(egui::pos2(rect.left(), split_y + 2.0), rect.max);

    // Top Panel: Properties / Filters
    let hdr_h = state.systems.panel_header.height.clamp(22.0, 36.0);
    let top_hdr = Rect::from_min_size(top_pnl.min, egui::vec2(top_pnl.width(), hdr_h));
    ui.painter().rect_filled(top_hdr, 4.0, bg_raised);
    ui.painter().text(
        egui::pos2(top_hdr.left() + 10.0, top_hdr.center().y),
        egui::Align2::LEFT_CENTER,
        "Properties   |   Filters",
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        text_pri,
    );

    let ctrl_h = state
        .systems
        .density
        .active()
        .control_height
        .clamp(20.0, 32.0);
    let gap = state
        .systems
        .two_column_form
        .gap
        .resolve(&state.systems)
        .clamp(2.0, 16.0);
    let pad = 8.0_f32;
    let mut y = top_hdr.bottom() + 6.0;

    // Instance name field mock
    let inst_r = Rect::from_min_size(
        egui::pos2(top_pnl.left() + pad, y),
        egui::vec2((top_pnl.width() - pad * 2.0).max(40.0), ctrl_h),
    );
    if inst_r.bottom() < top_pnl.bottom() {
        ui.painter().rect_filled(inst_r, 4.0, bg_field);
        ui.painter().text(
            egui::pos2(inst_r.left() + 8.0, inst_r.center().y),
            egui::Align2::LEFT_CENTER,
            "Name   hero_mc",
            neoflash_ui::typography::font(TextStyle::MonoSmall),
            text_pri,
        );
        y += ctrl_h + 6.0;
    }

    // 2-Column X/Y and W/H grid mock
    let col_w = ((top_pnl.width() - pad * 2.0 - gap) * 0.5).max(32.0);
    for pair in [
        ("X  140.0 px", "Y   96.0 px"),
        ("W  240.0 px", "H  180.0 px"),
    ] {
        let r1 = Rect::from_min_size(
            egui::pos2(top_pnl.left() + pad, y),
            egui::vec2(col_w, ctrl_h),
        );
        let r2 = Rect::from_min_size(egui::pos2(r1.right() + gap, y), egui::vec2(col_w, ctrl_h));
        if r1.bottom() < top_pnl.bottom() - 4.0 {
            ui.painter().rect_filled(r1, 4.0, bg_field);
            ui.painter().rect_filled(r2, 4.0, bg_field);
            ui.painter().text(
                egui::pos2(r1.left() + 6.0, r1.center().y),
                egui::Align2::LEFT_CENTER,
                pair.0,
                neoflash_ui::typography::font(TextStyle::MonoSmall),
                text_pri,
            );
            ui.painter().text(
                egui::pos2(r2.left() + 6.0, r2.center().y),
                egui::Align2::LEFT_CENTER,
                pair.1,
                neoflash_ui::typography::font(TextStyle::MonoSmall),
                text_pri,
            );
            y += ctrl_h + gap;
        }
    }

    // Bottom Panel: Library / Color & Swatches
    ui.painter()
        .hline(rect.x_range(), split_y, Stroke::new(1.0_f32, border_sub));
    let bot_hdr = Rect::from_min_size(bot_pnl.min, egui::vec2(bot_pnl.width(), hdr_h));
    ui.painter().rect_filled(bot_hdr, 0.0, bg_raised);
    ui.painter().text(
        egui::pos2(bot_hdr.left() + 10.0, bot_hdr.center().y),
        egui::Align2::LEFT_CENTER,
        "Library   |   Color & Swatches",
        neoflash_ui::typography::font(TextStyle::SmallStrong),
        text_pri,
    );

    let mut ly = bot_hdr.bottom() + 4.0;
    let row_h = state.systems.density.active().row_height.clamp(18.0, 28.0);
    for (idx, (sym_name, sym_kind)) in [
        ("Hero_MC", "Movie Clip"),
        ("Spotlight_Mask", "Graphic"),
        ("CTA_Button", "Button"),
        ("Soundtrack.mp3", "Audio"),
    ]
    .into_iter()
    .enumerate()
    {
        let r = Rect::from_min_size(
            egui::pos2(bot_pnl.left() + 4.0, ly),
            egui::vec2(bot_pnl.width() - 8.0, row_h),
        );
        if r.bottom() <= bot_pnl.bottom() - 2.0 {
            if idx == 0 {
                ui.painter()
                    .rect_filled(r, 3.0, accent.gamma_multiply(0.22));
            }
            ui.painter().text(
                egui::pos2(r.left() + 8.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                sym_name,
                neoflash_ui::typography::font(TextStyle::Small),
                text_pri,
            );
            ui.painter().text(
                egui::pos2(r.right() - 8.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                sym_kind,
                neoflash_ui::typography::font(TextStyle::Caption),
                text_sec,
            );
            ly += row_h + 1.0;
        }
    }
}

fn paint_timeline_studio_canvas(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    tl_rect: Rect,
    computed: &mut ComputedStudioLayout,
    dark_mode: bool,
) {
    computed.root_rect = tl_rect;
    computed.order.clear();
    register_hit_box(computed, &state.systems, 1, tl_rect);

    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgRaised);
    let border_def = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderDefault);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderSubtle);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextSecondary);

    ui.painter().rect_filled(tl_rect, 6.0, bg_panel);
    ui.painter().rect_stroke(
        tl_rect,
        6.0,
        Stroke::new(1.0_f32, border_def),
        StrokeKind::Inside,
    );

    // Top Panel Header + Toolbar (Node 2) — real NF2 FloatingPanelHeader + Transport & Layer Action Icons
    let hdr_h = state.systems.panel_header.height.clamp(24.0, 44.0);
    let bar_h = 34.0_f32;
    let top_bar_rect = Rect::from_min_size(tl_rect.min, egui::vec2(tl_rect.width(), hdr_h + bar_h));
    register_hit_box(computed, &state.systems, 2, top_bar_rect);

    let hdr_only_rect = Rect::from_min_size(tl_rect.min, egui::vec2(tl_rect.width(), hdr_h));
    {
        let mut hdr_ui = ui.new_child(egui::UiBuilder::new().max_rect(hdr_only_rect));
        let _ = c::FloatingPanelHeader {
            tabs: &["Timeline", "Motion Editor"],
            active: 0,
            collapsed: false,
        }
        .show(&mut hdr_ui);
    }

    let action_bar_rect = Rect::from_min_size(
        egui::pos2(tl_rect.left(), tl_rect.top() + hdr_h),
        egui::vec2(tl_rect.width(), bar_h),
    );
    ui.painter().rect_filled(action_bar_rect, 0.0, bg_raised);
    ui.painter().hline(
        action_bar_rect.x_range(),
        action_bar_rect.bottom() - 0.5,
        Stroke::new(1.0_f32, border_sub),
    );
    let mut act_x = action_bar_rect.left() + 8.0;
    let act_cy = action_bar_rect.center().y;
    for &(ico, active) in &[
        (i16::LAYERPLUS, false),
        (i16::FOLDERPLUS, false),
        (i16::TRASH, false),
        (i16::CHEVLEFT, false),
        (i16::PLAY, true),
        (i16::CHEVRIGHT, false),
        (i16::LOOP, false),
        (i16::ONION, true),
        (i16::FRAME, false),
        (i16::LAYERS, false),
        (i16::PLUS, false),
    ] {
        let b_rect =
            Rect::from_center_size(egui::pos2(act_x + 12.0, act_cy), egui::vec2(24.0, 24.0));
        if b_rect.right() < action_bar_rect.right() - 260.0 {
            if active {
                ui.painter().rect_filled(
                    b_rect,
                    4.0,
                    neoflash_ui::palette(ui.ctx())
                        .accent_default
                        .gamma_multiply(0.24),
                );
            }
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(b_rect.center(), egui::vec2(16.0, 16.0)),
                ico,
                if active { text_pri } else { text_sec },
            );
        }
        act_x += 26.0;
    }
    ui.painter().text(
        egui::pos2(action_bar_rect.right() - 12.0, act_cy),
        egui::Align2::RIGHT_CENTER,
        format!(
            "Frame: {}   |   24.0 fps   |   Cell: {:.1}×{:.1}px   |   Arrow Head: {:.1}px",
            state.systems.timeline_geometry.playhead_frame,
            state.systems.timeline_geometry.frame_cell_width_px,
            state.systems.timeline_geometry.layer_row_height_px,
            state.systems.timeline_geometry.tween_arrow_head_length_px
        ),
        neoflash_ui::typography::font(TextStyle::MonoSmall),
        text_sec,
    );

    // Interactive Timeline Surface in the middle
    let magnifier_h = 112.0_f32;
    let surface_top = top_bar_rect.bottom();
    let surface_h = (tl_rect.height() - hdr_h - bar_h - magnifier_h - 8.0).max(190.0);
    let surface_rect = Rect::from_min_size(
        egui::pos2(tl_rect.left() + 4.0, surface_top),
        egui::vec2(tl_rect.width() - 8.0, surface_h),
    );

    let hit_ids = TimelineHitIds {
        layers_col: 3,
        ruler_playhead: 4,
        classic_tween: 5,
        shape_tween: 6,
        motion_tween: 7,
        broken_tween: 8,
        keyframes_badges: 9,
        audio_waveform: 10,
    };
    paint_interactive_timeline_surface(ui, state, surface_rect, computed, hit_ids, dark_mode);

    // Bottom 2.5x Zoomed Tween Arrow & Keyframe Geometry Callout Strip
    let mag_rect = Rect::from_min_max(
        egui::pos2(tl_rect.left() + 8.0, surface_rect.bottom() + 6.0),
        egui::pos2(tl_rect.right() - 8.0, tl_rect.bottom() - 6.0),
    );
    if mag_rect.height() >= 64.0 {
        ui.painter().rect_filled(mag_rect, 4.0, bg_raised);
        ui.painter().rect_stroke(
            mag_rect,
            4.0,
            Stroke::new(1.0_f32, border_sub),
            StrokeKind::Inside,
        );
        let tg = &state.systems.timeline_geometry;
        ui.painter().text(
            egui::pos2(mag_rect.left() + 10.0, mag_rect.top() + 12.0),
            egui::Align2::LEFT_CENTER,
            format!(
                "ZOOMED TWEEN ARROW & KEYFRAME GEOMETRY CALLOUT (2.5×)   ·   Head Length: {:.1}px   ·   Wing: {:.1}px   ·   Start Inset: {:.1}px   ·   End Inset: {:.1}px   ·   Stroke: {:.2}px   ·   Y-Pos: {:.0}%",
                tg.tween_arrow_head_length_px,
                tg.tween_arrow_head_wing_px,
                tg.tween_arrow_start_inset_px,
                tg.tween_arrow_end_inset_px,
                tg.tween_arrow_stroke_width_px,
                tg.tween_arrow_y_pct
            ),
            neoflash_ui::typography::font(TextStyle::MonoSmall),
            text_sec,
        );

        let sample_span = Rect::from_min_max(
            egui::pos2(mag_rect.left() + 24.0, mag_rect.top() + 28.0),
            egui::pos2(mag_rect.right() - 24.0, mag_rect.bottom() - 10.0),
        );
        let purple_bg =
            Color32::from_rgba_unmultiplied(136, 94, 242, if dark_mode { 68 } else { 52 });
        let purple_fg = Color32::from_rgb(176, 138, 255);
        ui.painter().rect_filled(sample_span, 4.0, purple_bg);
        ui.painter().rect_stroke(
            sample_span,
            4.0,
            Stroke::new(1.0_f32, purple_fg.gamma_multiply(0.5)),
            StrokeKind::Inside,
        );

        let zoom = 2.4_f32;
        let y_line = sample_span.top()
            + sample_span.height() * (tg.tween_arrow_y_pct.clamp(10.0, 90.0) / 100.0);
        let kf_y = sample_span.top()
            + sample_span.height() * (tg.keyframe_dot_y_pct.clamp(15.0, 85.0) / 100.0);
        let dot_x = sample_span.left() + (tg.frame_cell_width_px * 0.5 * zoom).clamp(10.0, 44.0);
        ui.painter().circle_filled(
            egui::pos2(dot_x, kf_y),
            (tg.keyframe_dot_radius_px * zoom).clamp(3.0, 18.0),
            text_pri,
        );

        let shaft_start_x = (sample_span.left() + tg.tween_arrow_start_inset_px * zoom)
            .min(sample_span.right() - 16.0);
        let tip_x =
            (sample_span.right() - tg.tween_arrow_end_inset_px * zoom).max(shaft_start_x + 12.0);
        let head_len = (tg.tween_arrow_head_length_px * zoom).clamp(4.0, tip_x - shaft_start_x);
        let wing = (tg.tween_arrow_head_wing_px * zoom).clamp(2.0, 32.0);
        let base_x = tip_x - head_len;

        ui.painter().line_segment(
            [
                egui::pos2(shaft_start_x, y_line),
                egui::pos2(base_x, y_line),
            ],
            Stroke::new(
                (tg.tween_arrow_stroke_width_px * zoom).clamp(1.0, 10.0),
                purple_fg,
            ),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(tip_x, y_line),
                egui::pos2(base_x, y_line - wing),
                egui::pos2(base_x, y_line + wing),
            ],
            purple_fg,
            Stroke::NONE,
        ));
    }
}

fn paint_interactive_timeline_surface(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    rect: Rect,
    computed: &mut ComputedStudioLayout,
    hit_ids: TimelineHitIds,
    dark_mode: bool,
) {
    let bg_panel = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgPanel);
    let bg_raised = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgRaised);
    let border_sub = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderSubtle);
    let text_pri = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextPrimary);
    let text_sec = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextSecondary);
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::AccentDefault);

    ui.painter().rect_filled(rect, 4.0, bg_panel);
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, border_sub),
        StrokeKind::Inside,
    );

    let tg = state.systems.timeline_geometry.clone();
    let layer_col_w = tg.resolve_layer_list_width(rect.width());
    let ruler_h = tg.ruler_height_px.clamp(14.0, 40.0);
    let row_h = tg.layer_row_height_px.clamp(14.0, 48.0);
    let cell_w = tg.frame_cell_width_px.clamp(6.0, 32.0);

    let layers_col_rect = Rect::from_min_size(rect.min, egui::vec2(layer_col_w, rect.height()));
    let grid_area_rect =
        Rect::from_min_max(egui::pos2(layers_col_rect.right(), rect.top()), rect.max);
    let ruler_rect = Rect::from_min_size(
        grid_area_rect.min,
        egui::vec2(grid_area_rect.width(), ruler_h),
    );

    register_hit_box(
        computed,
        &state.systems,
        hit_ids.layers_col,
        layers_col_rect,
    );
    register_hit_box(computed, &state.systems, hit_ids.ruler_playhead, ruler_rect);

    // 1. Left Layer Tree Column Header + 6 Realistic Layers using real `tl::layers_header_with` & `tl::LayerRow`
    let layer_hdr_rect = Rect::from_min_size(rect.min, egui::vec2(layer_col_w, ruler_h));
    {
        let mut hdr_ui = ui.new_child(egui::UiBuilder::new().max_rect(layer_hdr_rect));
        let _ = tl::layers_header_with(
            &mut hdr_ui,
            layer_col_w,
            LayersHeaderColumns {
                mode: layer_col_w >= 210.0,
                solo: true,
            },
        );
    }

    let layers = [
        (
            "Character",
            LayerKind::FolderOpen,
            0,
            false,
            false,
            false,
            ModeCell::Empty,
            Color32::from_rgb(255, 146, 56),
        ),
        (
            "Spotlight",
            LayerKind::Mask,
            1,
            false,
            true,
            false,
            ModeCell::Mode("Normal"),
            Color32::from_rgb(176, 118, 255),
        ),
        (
            "Hero_MC",
            LayerKind::Masked,
            2,
            true,
            false,
            false,
            ModeCell::Mode("Multiply"),
            Color32::from_rgb(82, 162, 255),
        ),
        (
            "Motion Path",
            LayerKind::MotionGuide,
            1,
            false,
            false,
            true,
            ModeCell::Empty,
            Color32::from_rgb(0, 212, 224),
        ),
        (
            "Background",
            LayerKind::Layer,
            0,
            false,
            false,
            false,
            ModeCell::Mode("Normal"),
            Color32::from_rgb(72, 214, 134),
        ),
        (
            "Soundtrack",
            LayerKind::Sound,
            0,
            false,
            false,
            false,
            ModeCell::Empty,
            Color32::from_rgb(255, 158, 68),
        ),
    ];

    for (idx, &(name, kind, depth, selected, locked, outline, mode_cell, swatch_col)) in
        layers.iter().enumerate()
    {
        let ry = rect.top() + ruler_h + idx as f32 * row_h;
        if ry + 4.0 > rect.bottom() {
            break;
        }
        let r_rect = Rect::from_min_size(
            egui::pos2(rect.left(), ry),
            egui::vec2(layer_col_w, row_h.min(rect.bottom() - ry)),
        );
        let row_resp = ui.interact(
            r_rect,
            ui.id()
                .with(("studio-tl-layer-row", hit_ids.layers_col, idx)),
            Sense::hover(),
        );
        let _ = LayerRow {
            label: name,
            kind,
            selected,
            indent: depth,
            editing: selected,
            visible: true,
            locked,
            outline,
            color: swatch_col,
        }
        .paint_at_with(
            ui,
            r_rect,
            row_resp,
            LayerColumns {
                mode: if layer_col_w >= 210.0 {
                    mode_cell
                } else {
                    ModeCell::Hidden
                },
                solo: Some(selected),
            },
        );
    }

    let _ = (bg_raised, accent);
    ui.painter().vline(
        layers_col_rect.right(),
        rect.y_range(),
        Stroke::new(1.0_f32, border_sub),
    );

    // 2. Frame Ruler (1, 5, 10, 15, 20, 25...) + Onion Skin Range Brackets + Interactive Playhead
    let tl_geo = tl::Geometry {
        origin_x: grid_area_rect.left(),
        frame_width: cell_w,
        row_height: row_h,
    };
    tl::paint_ruler(
        &ui.painter().with_clip_rect(ruler_rect),
        ui.ctx(),
        ruler_rect,
        &tl_geo,
        24.0,
        ruler_h >= 26.0,
    );

    let max_frames = ((ruler_rect.width() / cell_w).floor() as usize).clamp(16, 80);

    // Interactive Playhead scrubbing when clicking/dragging on the ruler!
    if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        && ruler_rect.contains(pos)
        && ui.input(|i| i.pointer.primary_down())
    {
        let rel_x = (pos.x - ruler_rect.left()).max(0.0);
        let f = ((rel_x / cell_w).floor() as usize + 1).clamp(1, max_frames);
        state.systems.timeline_geometry.playhead_frame = f;
    }

    // Onion skin range overlay in ruler
    let onion_l = ruler_rect.left() + (tg.onion_start_frame.saturating_sub(1)) as f32 * cell_w;
    let onion_r = ruler_rect.left() + tg.onion_end_frame as f32 * cell_w;
    if onion_r > onion_l && onion_l < ruler_rect.right() {
        let o_rect = Rect::from_min_max(
            egui::pos2(onion_l.max(ruler_rect.left()), ruler_rect.top() + 2.0),
            egui::pos2(onion_r.min(ruler_rect.right()), ruler_rect.bottom() - 2.0),
        );
        let onion_col = Color32::from_rgba_unmultiplied(64, 196, 255, 48);
        let onion_stroke = Color32::from_rgb(64, 196, 255);
        ui.painter().rect_filled(o_rect, 2.0, onion_col);
        let bw = tg.onion_bracket_width_px.clamp(2.0, 14.0);
        let hr = tg.onion_handle_radius_px.clamp(1.5, 8.0);
        // Left bracket `[`
        ui.painter().vline(
            o_rect.left(),
            o_rect.y_range(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            o_rect.left()..=(o_rect.left() + bw),
            o_rect.top(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            o_rect.left()..=(o_rect.left() + bw),
            o_rect.bottom(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().circle_filled(
            egui::pos2(o_rect.left(), o_rect.center().y),
            hr,
            onion_stroke,
        );
        // Right bracket `]`
        ui.painter().vline(
            o_rect.right(),
            o_rect.y_range(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            (o_rect.right() - bw)..=o_rect.right(),
            o_rect.top(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().hline(
            (o_rect.right() - bw)..=o_rect.right(),
            o_rect.bottom(),
            Stroke::new(1.5_f32, onion_stroke),
        );
        ui.painter().circle_filled(
            egui::pos2(o_rect.right(), o_rect.center().y),
            hr,
            onion_stroke,
        );
    }

    // 3. Multi-Row Frame Grid (`tl::paint_grid`) + Tween Spans + Arrows + Keyframe Dots + Diamonds + Audio Waveform
    for r in 0..6_usize {
        let ry = rect.top() + ruler_h + r as f32 * row_h;
        if ry + 4.0 > rect.bottom() {
            break;
        }
        let row_rect = Rect::from_min_size(
            egui::pos2(grid_area_rect.left(), ry),
            egui::vec2(grid_area_rect.width(), row_h.min(rect.bottom() - ry)),
        );
        tl::paint_grid(
            &ui.painter().with_clip_rect(row_rect),
            ui.ctx(),
            row_rect,
            &tl_geo,
            if r == 0 {
                GridKind::Folder
            } else {
                GridKind::Layer
            },
        );
    }

    let span_rect_for = |row_idx: usize, start_f: usize, end_f: usize| -> Rect {
        let ry = (rect.top() + ruler_h + row_idx as f32 * row_h).min(rect.bottom() - 2.0);
        let rh = row_h.min((rect.bottom() - ry).max(2.0));
        let sx =
            (grid_area_rect.left() + (start_f - 1) as f32 * cell_w).min(grid_area_rect.right());
        let ex = (grid_area_rect.left() + end_f as f32 * cell_w).min(grid_area_rect.right());
        Rect::from_min_max(
            egui::pos2(sx, ry + 1.0),
            egui::pos2(ex.max(sx + 4.0), ry + rh - 1.0),
        )
    };

    let paint_keyframe_dot = |painter: &egui::Painter, cell_rect: Rect, hollow: bool| {
        let cx = cell_rect.left() + cell_w * 0.5;
        let cy = cell_rect.top()
            + cell_rect.height() * (tg.keyframe_dot_y_pct.clamp(15.0, 85.0) / 100.0);
        let rad = tg.keyframe_dot_radius_px.clamp(1.5, 8.0);
        if hollow {
            painter.circle_stroke(egui::pos2(cx, cy), rad, Stroke::new(1.2_f32, text_pri));
        } else {
            painter.circle_filled(egui::pos2(cx, cy), rad, text_pri);
        }
    };

    let paint_span_end_marker = |painter: &egui::Painter, span_r: Rect| {
        let ex = span_r.right() - cell_w * 0.5;
        let ey = span_r.center().y;
        let m_rect = Rect::from_center_size(
            egui::pos2(ex, ey),
            egui::vec2(
                tg.span_end_rect_w_px.clamp(2.0, 12.0),
                tg.span_end_rect_h_px.clamp(3.0, 16.0),
            ),
        );
        painter.rect_filled(m_rect, 1.0, Color32::WHITE);
        painter.rect_stroke(
            m_rect,
            1.0,
            Stroke::new(1.0_f32, text_sec),
            StrokeKind::Inside,
        );
    };

    // Row 0 (`Folder: Character`) & Row 3 (`Guide: Motion Path`): Ordinary Keyframe Span + Label Flag + Action `a` + `[ ]`
    let kf_span = span_rect_for(3, 1, 22);
    register_hit_box(computed, &state.systems, hit_ids.keyframes_badges, kf_span);
    ui.painter().rect_filled(
        kf_span,
        2.0,
        if dark_mode {
            Color32::from_rgba_unmultiplied(180, 188, 206, 28)
        } else {
            Color32::from_rgba_unmultiplied(120, 130, 150, 32)
        },
    );
    paint_keyframe_dot(ui.painter(), kf_span, false);
    paint_span_end_marker(ui.painter(), kf_span);

    // Red Frame Label Flag + `"intro"` text & Action `a` marker on Row 0 / Row 3
    let flag_sz = tg.label_flag_size_px.clamp(3.0, 12.0);
    let flag_x = kf_span.left() + 2.0;
    let flag_y = kf_span.top() + 2.0;
    let red_flag = Color32::from_rgb(238, 72, 82);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(flag_x, flag_y),
            egui::pos2(flag_x + flag_sz, flag_y + flag_sz * 0.5),
            egui::pos2(flag_x, flag_y + flag_sz),
        ],
        red_flag,
        Stroke::NONE,
    ));
    ui.painter().text(
        egui::pos2(flag_x + flag_sz + 3.0, flag_y + flag_sz * 0.5),
        egui::Align2::LEFT_CENTER,
        "motion_guide",
        egui::FontId::monospace(8.5),
        red_flag,
    );
    // Action marker `a` + Blank keyframe `○` at frame 23
    ui.painter().text(
        egui::pos2(kf_span.right() - cell_w * 1.5, kf_span.top() + 6.0),
        egui::Align2::CENTER_CENTER,
        "a",
        egui::FontId::monospace(tg.action_marker_size_px.clamp(5.0, 14.0)),
        Color32::from_rgb(255, 208, 72),
    );
    let blank_kf_rect = span_rect_for(3, 23, 25);
    paint_keyframe_dot(ui.painter(), blank_kf_rect, true);

    // Row 1 (`Mask: Spotlight`) & Row 2 (`Masked: Hero_MC`): Classic Tween Span (Purple + Parametric Arrow `──────►`)
    let classic_span = span_rect_for(1, 1, 20);
    register_hit_box(
        computed,
        &state.systems,
        hit_ids.classic_tween,
        classic_span,
    );
    let purple_fill =
        Color32::from_rgba_unmultiplied(138, 92, 246, if dark_mode { 76 } else { 58 });
    let purple_arrow = Color32::from_rgb(184, 146, 255);
    ui.painter().rect_filled(classic_span, 2.0, purple_fill);
    paint_keyframe_dot(ui.painter(), classic_span, false);
    let (c_start, _c_tip, c_tri) = tg.compute_tween_arrow_points(classic_span);
    ui.painter().line_segment(
        [c_start, egui::pos2(c_tri[1].x, c_start.y)],
        Stroke::new(tg.tween_arrow_stroke_width_px.clamp(0.5, 5.0), purple_arrow),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        c_tri.to_vec(),
        purple_arrow,
        Stroke::NONE,
    ));
    // End keyframe dot of classic tween
    let classic_end_cell = span_rect_for(1, 20, 20);
    paint_keyframe_dot(ui.painter(), classic_end_cell, false);

    // Row 2 (`Masked: Hero_MC`): Motion Tween Span (Blue + Property Diamonds `◆`) + Broken Tween Span (`- - - -`)
    let motion_span = span_rect_for(2, 1, 18);
    register_hit_box(computed, &state.systems, hit_ids.motion_tween, motion_span);
    let blue_fill = Color32::from_rgba_unmultiplied(56, 148, 255, if dark_mode { 76 } else { 56 });
    let blue_diamond = Color32::from_rgb(112, 192, 255);
    ui.painter().rect_filled(motion_span, 2.0, blue_fill);
    paint_keyframe_dot(ui.painter(), motion_span, false);
    let dr = tg.motion_diamond_radius_px.clamp(1.5, 9.0);
    let dy = motion_span.top()
        + motion_span.height() * (tg.keyframe_dot_y_pct.clamp(15.0, 85.0) / 100.0);
    for df in [5_usize, 10, 14, 18] {
        let dx = grid_area_rect.left() + (df as f32 - 0.5) * cell_w;
        if dx < motion_span.right() {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(dx, dy - dr),
                    egui::pos2(dx + dr, dy),
                    egui::pos2(dx, dy + dr),
                    egui::pos2(dx - dr, dy),
                ],
                blue_diamond,
                Stroke::NONE,
            ));
        }
    }

    // Broken Tween Span on Row 2 (frames 19..30)
    let broken_span = span_rect_for(2, 19, 30);
    register_hit_box(computed, &state.systems, hit_ids.broken_tween, broken_span);
    ui.painter().rect_filled(
        broken_span,
        2.0,
        Color32::from_rgba_unmultiplied(138, 92, 246, 36),
    );
    paint_keyframe_dot(ui.painter(), broken_span, false);
    let dash_w = tg.broken_tween_dash_px.clamp(1.0, 16.0);
    let gap_w = tg.broken_tween_gap_px.clamp(1.0, 16.0);
    let by =
        broken_span.top() + broken_span.height() * (tg.tween_arrow_y_pct.clamp(10.0, 90.0) / 100.0);
    let mut bx = broken_span.left() + tg.tween_arrow_start_inset_px.max(4.0);
    while bx + dash_w < broken_span.right() - 4.0 {
        ui.painter().line_segment(
            [egui::pos2(bx, by), egui::pos2(bx + dash_w, by)],
            Stroke::new(tg.tween_arrow_stroke_width_px.clamp(0.5, 5.0), purple_arrow),
        );
        bx += dash_w + gap_w;
    }

    // Row 4 (`Normal: Background`): Shape Tween Span (Green + Parametric Arrow `──────►`)
    let shape_span = span_rect_for(4, 1, 24);
    register_hit_box(computed, &state.systems, hit_ids.shape_tween, shape_span);
    let green_fill = Color32::from_rgba_unmultiplied(46, 204, 118, if dark_mode { 74 } else { 54 });
    let green_arrow = Color32::from_rgb(92, 236, 156);
    ui.painter().rect_filled(shape_span, 2.0, green_fill);
    paint_keyframe_dot(ui.painter(), shape_span, false);
    let (s_start, _s_tip, s_tri) = tg.compute_tween_arrow_points(shape_span);
    ui.painter().line_segment(
        [s_start, egui::pos2(s_tri[1].x, s_start.y)],
        Stroke::new(tg.tween_arrow_stroke_width_px.clamp(0.5, 5.0), green_arrow),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        s_tri.to_vec(),
        green_arrow,
        Stroke::NONE,
    ));
    let shape_end_cell = span_rect_for(4, 24, 24);
    paint_keyframe_dot(ui.painter(), shape_end_cell, false);

    // Row 5 (`Audio: Soundtrack`): Sound Waveform Span
    let audio_span = span_rect_for(5, 1, 30);
    register_hit_box(computed, &state.systems, hit_ids.audio_waveform, audio_span);
    ui.painter().rect_filled(
        audio_span,
        2.0,
        Color32::from_rgba_unmultiplied(255, 154, 62, 34),
    );
    paint_keyframe_dot(ui.painter(), audio_span, false);
    let wave_col = Color32::from_rgb(255, 164, 76);
    let mid_y = audio_span.center().y;
    let max_amp = (audio_span.height() * 0.40).max(2.0);
    let mut wx = audio_span.left() + 8.0;
    let mut step = 0_usize;
    while wx < audio_span.right() - 6.0 {
        let amp = (((step as f32 * 0.65).sin().abs() * 0.7
            + ((step as f32 * 1.3).cos().abs() * 0.3))
            * max_amp)
            .max(1.5);
        ui.painter().vline(
            wx,
            (mid_y - amp)..=(mid_y + amp),
            Stroke::new(1.15_f32, wave_col),
        );
        wx += 3.0;
        step += 1;
    }

    // 4. Red Playhead Cap + Vertical Line across all layers
    let ph_frame = tg.playhead_frame.clamp(1, max_frames);
    let ph_x = ruler_rect.left() + (ph_frame as f32 - 0.5) * cell_w;
    if ph_x >= ruler_rect.left() && ph_x <= ruler_rect.right() {
        let playhead_red = Color32::from_rgb(242, 62, 72);
        let cap_w = tg.playhead_cap_width_px.clamp(6.0, 24.0);
        let cap_h = tg.playhead_cap_height_px.clamp(6.0, ruler_h);
        let cap_rect = Rect::from_center_size(
            egui::pos2(ph_x, ruler_rect.bottom() - cap_h * 0.5),
            egui::vec2(cap_w, cap_h),
        );
        ui.painter().rect_filled(cap_rect, 2.0, playhead_red);
        ui.painter().vline(
            ph_x,
            cap_rect.bottom()..=rect.bottom(),
            Stroke::new(tg.playhead_line_width_px.clamp(0.5, 4.0), playhead_red),
        );
    }
}

fn paint_node_wysiwyg(
    ui: &mut Ui,
    node: &mut UiStudioNode,
    systems: &UiStudioSystems,
    computed: &ComputedStudioLayout,
    dark_mode: bool,
) {
    if !node.visible {
        return;
    }
    let Some(box_info) = computed.node_box(node.id) else {
        return;
    };
    let rect = box_info.rect;
    let nf = neoflash_ui::palette(ui.ctx());

    // Optional custom background or panel surface
    let bg_fill = node
        .style
        .bg_token
        .map(|tok| systems.palette_overrides.resolve_color(dark_mode, tok))
        .or_else(|| match node.kind {
            UiNodeKind::Panel { .. } => Some(
                systems
                    .palette_overrides
                    .resolve_color(dark_mode, StudioColorRole::BgPanel),
            ),
            _ => None,
        });

    let rad = match node.style.corner_radius {
        RadiusToken::None if matches!(node.kind, UiNodeKind::Panel { .. }) => radius::LG,
        other => other.resolve_px(),
    };

    if let Some(fill) = bg_fill {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(rad as u8), fill);
    }
    if let Some(border_tok) = node.style.border_token {
        let bcol = systems
            .palette_overrides
            .resolve_color(dark_mode, border_tok);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(rad as u8),
            Stroke::new(node.style.border_width.max(1.0), bcol),
            StrokeKind::Inside,
        );
    } else if matches!(node.kind, UiNodeKind::Panel { .. }) {
        let bcol = systems
            .palette_overrides
            .resolve_color(dark_mode, StudioColorRole::BorderDefault);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(rad as u8),
            Stroke::new(1.0_f32, bcol),
            StrokeKind::Inside,
        );
    }

    let text_col = systems
        .palette_overrides
        .resolve_color(dark_mode, box_info.typography.color_token);

    match &mut node.kind {
        UiNodeKind::Panel { tabs, active_tab } => {
            if !tabs.is_empty() {
                let hdr_h = systems.panel_header.height.max(24.0);
                let hdr_rect = Rect::from_min_size(rect.min, egui::vec2(rect.width(), hdr_h));
                let tab_refs: Vec<&str> = tabs.iter().map(String::as_str).collect();
                let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(hdr_rect));
                let resp = c::FloatingPanelHeader {
                    tabs: &tab_refs,
                    active: (*active_tab).min(tab_refs.len().saturating_sub(1)),
                    collapsed: false,
                }
                .show(&mut child_ui);
                if let Some(clicked_tab) = resp.tab {
                    *active_tab = clicked_tab;
                }
            }
            for child in &mut node.children {
                paint_node_wysiwyg(ui, child, systems, computed, dark_mode);
            }
        }
        UiNodeKind::Section { expanded, .. } => {
            let hdr_h = systems.section_header.height.max(20.0);
            let hdr_rect = Rect::from_min_size(rect.min, egui::vec2(rect.width(), hdr_h));
            let chev = if *expanded {
                i16::CHEVDOWN
            } else {
                i16::CHEVRIGHT
            };
            icons::paint(
                ui.painter(),
                ui.ctx(),
                Rect::from_center_size(
                    egui::pos2(hdr_rect.left() + 8.0, hdr_rect.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                chev,
                nf.text_secondary,
            );
            let display_lbl = if box_info.typography.uppercase {
                node.label.to_uppercase()
            } else {
                node.label.clone()
            };
            ui.painter().text(
                egui::pos2(hdr_rect.left() + 22.0, hdr_rect.center().y),
                egui::Align2::LEFT_CENTER,
                display_lbl,
                egui::FontId::proportional(box_info.typography.size_pt),
                text_col,
            );
            if *expanded {
                for child in &mut node.children {
                    paint_node_wysiwyg(ui, child, systems, computed, dark_mode);
                }
            }
        }
        UiNodeKind::Row | UiNodeKind::Column | UiNodeKind::Grid { .. } => {
            for child in &mut node.children {
                paint_node_wysiwyg(ui, child, systems, computed, dark_mode);
            }
        }
        UiNodeKind::Field {
            value,
            prefix,
            unit,
            decimals,
            stepper,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let mut fld = c::Field::new(value, &node.label)
                .prefix(prefix)
                .unit(unit)
                .decimals(*decimals)
                .width(rect.width());
            if *stepper {
                fld = fld.stepper();
            }
            child_ui.add(fld);
        }
        UiNodeKind::TextField {
            text, prefix, hint, ..
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.add(
                c::TextField::new(text, if hint.is_empty() { &node.label } else { hint })
                    .prefix(prefix)
                    .width(rect.width()),
            );
        }
        UiNodeKind::Select { selected, options } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let current = options
                .get(*selected)
                .map_or(node.label.as_str(), String::as_str);
            c::Select::new(("studio-canvas-sel", node.id), current)
                .width(rect.width())
                .show(&mut child_ui, |ui| {
                    for (idx, opt) in options.iter().enumerate() {
                        if ui
                            .add(c::MenuItem::new(opt).checked(idx == *selected))
                            .clicked()
                        {
                            *selected = idx;
                        }
                    }
                });
        }
        UiNodeKind::Button {
            button_kind,
            icon_name,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let mut btn = c::Button::new(&node.label)
                .kind(button_kind.to_nf2())
                .width(rect.width());
            if let Some(ico) = icon_name {
                btn = btn.icon(resolve_icon_by_name(ico));
            }
            let _ = child_ui.add(btn);
        }
        UiNodeKind::IconButton {
            icon_name,
            selected,
            ..
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let ico = resolve_icon_by_name(icon_name);
            let side = rect.width().min(rect.height()).max(20.0);
            if child_ui
                .add(
                    c::IconButton::new(ico, &node.label)
                        .selected(*selected)
                        .side(side),
                )
                .clicked()
            {
                *selected = !*selected;
            }
        }
        UiNodeKind::Checkbox { checked } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.add(c::Checkbox::new(checked, &node.label));
        }
        UiNodeKind::Switch { on } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child_ui.add(c::Switch::new(on, &node.label));
        }
        UiNodeKind::ColorSwatch { rgba, is_stroke } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let col = Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
            child_ui.add(c::ColorWell {
                target: if *is_stroke {
                    c::ColorWellTarget::Stroke
                } else {
                    c::ColorWellTarget::Fill
                },
                color: Some(col),
                active: false,
            });
        }
        UiNodeKind::Slider {
            value,
            min,
            max,
            show_field,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let val_mode = if *show_field && rect.width() > 140.0 {
                c::SliderValue::Field
            } else {
                c::SliderValue::Inline
            };
            let track_w = if val_mode == c::SliderValue::Field {
                (rect.width() - 86.0).max(48.0)
            } else {
                (rect.width() - 48.0).max(48.0)
            };
            child_ui.add(
                c::Slider::new(value, *min..=*max, &node.label)
                    .width(track_w)
                    .value(val_mode),
            );
        }
        UiNodeKind::Label => {
            let align = match box_info.typography.align {
                StudioTextAlign::Left => (rect.left_center(), egui::Align2::LEFT_CENTER),
                StudioTextAlign::Center => (rect.center(), egui::Align2::CENTER_CENTER),
                StudioTextAlign::Right => (rect.right_center(), egui::Align2::RIGHT_CENTER),
            };
            let txt = if box_info.typography.uppercase {
                node.label.to_uppercase()
            } else {
                node.label.clone()
            };
            let family = match box_info.typography.font_family {
                StudioFontFamily::Inter => egui::FontFamily::Proportional,
                StudioFontFamily::RobotoMono => egui::FontFamily::Monospace,
            };
            ui.painter().text(
                align.0,
                align.1,
                txt,
                egui::FontId::new(box_info.typography.size_pt, family),
                text_col,
            );
        }
        UiNodeKind::Separator { vertical } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            c::divider(&mut child_ui, *vertical);
        }
        UiNodeKind::TableRow {
            cells,
            column_fractions,
            is_header,
            selected,
        } => {
            let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let cell_refs: Vec<&str> = cells.iter().map(String::as_str).collect();
            let kind = if *is_header {
                c::TableRowKind::Header
            } else if *selected {
                c::TableRowKind::Selected
            } else {
                c::TableRowKind::Row
            };
            child_ui.add(c::TableRow {
                cells: &cell_refs,
                kind,
                columns: column_fractions,
            });
        }
    }
}

fn paint_canvas_overlays(
    ui: &mut Ui,
    state: &UiLayoutStudioState,
    computed: &ComputedStudioLayout,
    dark_mode: bool,
) {
    let accent = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::AccentDefault);
    let muted_border = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BorderStrong);
    let badge_bg = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::BgRaised);
    let badge_fg = state
        .systems
        .palette_overrides
        .resolve_color(dark_mode, StudioColorRole::TextPrimary);

    if state.show_wireframe_outlines {
        for &id in &computed.order {
            if let Some(node) = state.document.find_node(id)
                && node.kind.is_container()
                && let Some(b) = computed.node_box(id)
            {
                ui.painter().rect_stroke(
                    b.rect,
                    2.0,
                    Stroke::new(1.0_f32, muted_border.gamma_multiply(0.45)),
                    StrokeKind::Inside,
                );
            }
        }
    }

    if let Some(hov_id) = state.hovered_node
        && !state.selected_nodes.contains(&hov_id)
        && let Some(b) = computed.node_box(hov_id)
    {
        ui.painter().rect_stroke(
            b.rect,
            3.0,
            Stroke::new(1.5_f32, accent.gamma_multiply(0.65)),
            StrokeKind::Outside,
        );
    }

    for &sel_id in &state.selected_nodes {
        let Some(b) = computed.node_box(sel_id) else {
            continue;
        };
        ui.painter().rect_stroke(
            b.rect,
            3.0,
            Stroke::new(2.0_f32, accent),
            StrokeKind::Outside,
        );

        if state.show_dimension_badges
            && let Some(node) = state.document.find_node(sel_id)
        {
            let badge_txt = format!(
                "{} · {} ({:.0}×{:.0}px)",
                node.kind.type_badge(),
                node.box_constraints.width.format_compact(),
                b.resolved_width,
                b.resolved_height
            );
            let font = neoflash_ui::typography::font(TextStyle::MonoSmall);
            let galley = ui.painter().layout_no_wrap(badge_txt, font, badge_fg);
            let pill_size = galley.size() + egui::vec2(10.0, 4.0);
            let pill_min = egui::pos2(
                b.rect.left(),
                (b.rect.top() - pill_size.y - 2.0).max(computed.root_rect.top()),
            );
            let pill_rect = Rect::from_min_size(pill_min, pill_size);
            ui.painter().rect_filled(pill_rect, 3.0, badge_bg);
            ui.painter().rect_stroke(
                pill_rect,
                3.0,
                Stroke::new(1.0_f32, accent),
                StrokeKind::Inside,
            );
            ui.painter()
                .galley(pill_rect.min + egui::vec2(5.0, 2.0), galley, badge_fg);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Right-Hand Node Inspector (% / px / Paddings / Gaps / Typography) & Systems
// ─────────────────────────────────────────────────────────────────────────────

fn render_inspector_and_systems_column(
    ui: &mut Ui,
    state: &mut UiLayoutStudioState,
    width: f32,
    height: f32,
) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.set_width(width);

    // Top Segmented Tab Switcher: [Node Inspector | Systems & Tokens | Code Export]
    let mut tab_idx = match state.inspector_tab {
        StudioInspectorTab::NodeInspector => 0usize,
        StudioInspectorTab::SystemsTab => 1usize,
        StudioInspectorTab::CodeExport => 2usize,
    };
    if c::Segmented::show(
        ui,
        &mut tab_idx,
        &[
            (0usize, "Node (% / px)"),
            (1usize, "Systems"),
            (2usize, "Code Export"),
        ],
        true,
    )
    .is_some()
    {
        state.inspector_tab = match tab_idx {
            0 => StudioInspectorTab::NodeInspector,
            1 => StudioInspectorTab::SystemsTab,
            _ => StudioInspectorTab::CodeExport,
        };
    }

    let panel_h = (height - 42.0).max(240.0);
    egui::Frame::new()
        .fill(nf.bg_panel)
        .corner_radius(egui::CornerRadius::same(radius::MD as u8))
        .stroke(Stroke::new(1.0_f32, nf.border_subtle))
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            let inner_w = (width - space::S3 * 2.0).max(200.0);
            ui.set_width(inner_w);
            egui::ScrollArea::vertical()
                .id_salt(("studio-right-inspector-scroll", tab_idx))
                .max_height(panel_h - space::S3 * 2.0)
                .themed()
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.spacing_mut().item_spacing.y = space::S2;
                    match state.inspector_tab {
                        StudioInspectorTab::NodeInspector => {
                            render_node_inspector_tab(ui, state, inner_w);
                        }
                        StudioInspectorTab::SystemsTab => {
                            render_systems_inspector_tab(ui, state, inner_w);
                        }
                        StudioInspectorTab::CodeExport => {
                            render_code_export_tab(ui, state, inner_w);
                        }
                    }
                });
        });
}

fn render_size_unit_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    axis_label: &str,
    unit: &mut UiSizeUnit,
    reference_px: f32,
) -> bool {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        ui.label(rich(axis_label, TextStyle::SmallStrong).color(nf.text_secondary));
        let mut mode_idx = match unit {
            UiSizeUnit::Percent(_) => 0usize,
            UiSizeUnit::Px(_) => 1usize,
            UiSizeUnit::Auto => 2usize,
            UiSizeUnit::Fill(_) => 3usize,
        };
        if c::Select::options(
            ui,
            (id_salt, "unit-mode"),
            &mut mode_idx,
            &[
                (0usize, "% (Percent)"),
                (1usize, "px (Points)"),
                (2usize, "Auto"),
                (3usize, "Fill (fr)"),
            ],
            106.0,
        ) {
            *unit = match mode_idx {
                0 => unit.to_percent(reference_px),
                1 => unit.to_px(reference_px),
                2 => UiSizeUnit::Auto,
                _ => UiSizeUnit::Fill(1.0),
            };
            changed = true;
        }

        match unit {
            UiSizeUnit::Percent(pct) => {
                let mut v = f64::from(*pct);
                if ui
                    .add(
                        c::Field::new(&mut v, axis_label)
                            .prefix("%")
                            .unit("%")
                            .range(0.0..=100.0)
                            .decimals(1)
                            .width(104.0),
                    )
                    .changed()
                {
                    *pct = (v as f32).clamp(0.0, 100.0);
                    changed = true;
                }
            }
            UiSizeUnit::Px(px) => {
                let mut v = f64::from(*px);
                if ui
                    .add(
                        c::Field::new(&mut v, axis_label)
                            .prefix("px")
                            .unit("px")
                            .range(0.0..=2400.0)
                            .decimals(0)
                            .width(104.0),
                    )
                    .changed()
                {
                    *px = (v as f32).max(0.0);
                    changed = true;
                }
            }
            UiSizeUnit::Fill(fr) => {
                let mut v = f64::from(*fr);
                if ui
                    .add(
                        c::Field::new(&mut v, axis_label)
                            .prefix("fr")
                            .unit("fr")
                            .range(0.1..=12.0)
                            .decimals(1)
                            .width(104.0),
                    )
                    .changed()
                {
                    *fr = (v as f32).max(0.1);
                    changed = true;
                }
            }
            UiSizeUnit::Auto => {
                ui.add(c::Badge::new("Hug Content", c::BadgeType::Neutral));
            }
        }
    });
    changed
}

fn render_spacing_value_editor(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    label: &str,
    val: &mut SpacingValue,
    systems: &UiStudioSystems,
) -> bool {
    let nf = neoflash_ui::palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        ui.label(rich(label, TextStyle::Small).color(nf.text_secondary));
        let mut choice: i32 = match *val {
            SpacingValue::Token(SpacingToken::S0) => 0,
            SpacingValue::Token(SpacingToken::S1) => 1,
            SpacingValue::Token(SpacingToken::S2) => 2,
            SpacingValue::Token(SpacingToken::S3) => 3,
            SpacingValue::Token(SpacingToken::S4) => 4,
            SpacingValue::Token(SpacingToken::S5) => 5,
            SpacingValue::Token(SpacingToken::S6) => 6,
            SpacingValue::Px(_) => 7,
        };
        if c::Select::options(
            ui,
            (id_salt, "sp-tok"),
            &mut choice,
            &[
                (0, "S0 (0)"),
                (1, "S1"),
                (2, "S2"),
                (3, "S3"),
                (4, "S4"),
                (5, "S5"),
                (6, "S6"),
                (7, "Custom px"),
            ],
            96.0,
        ) {
            *val = match choice {
                0 => SpacingValue::Token(SpacingToken::S0),
                1 => SpacingValue::Token(SpacingToken::S1),
                2 => SpacingValue::Token(SpacingToken::S2),
                3 => SpacingValue::Token(SpacingToken::S3),
                4 => SpacingValue::Token(SpacingToken::S4),
                5 => SpacingValue::Token(SpacingToken::S5),
                6 => SpacingValue::Token(SpacingToken::S6),
                _ => SpacingValue::Px(val.resolve(systems)),
            };
            changed = true;
        }
        let mut px_v = f64::from(val.resolve(systems));
        if ui
            .add(
                c::Field::new(&mut px_v, label)
                    .unit("px")
                    .range(0.0..=128.0)
                    .decimals(0)
                    .width(78.0),
            )
            .changed()
        {
            *val = SpacingValue::Px(px_v as f32);
            changed = true;
        }
    });
    changed
}

fn render_node_inspector_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, _width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let sel_id = state.primary_selected_id();
    let Some(mut node_clone) = state.document.find_node(sel_id).cloned() else {
        return;
    };
    let mut modified = false;

    // 1. Node Header & Identity
    let mut reset_clicked = false;
    ui.horizontal(|ui| {
        ui.add(c::Badge::new(
            node_clone.kind.type_badge(),
            c::BadgeType::Accent,
        ));
        ui.label(rich(format!("Node #{sel_id}"), TextStyle::MonoSmall).color(nf.text_tertiary));
        if ui.add(c::Button::ghost("Reset to System")).clicked() {
            state.commit_history("Reset node to system tokens");
            state.document.reset_node_to_system_defaults(sel_id);
            state.status_message = format!("Reset node #{sel_id} to NF2 system tokens.");
            reset_clicked = true;
        }
    });
    if reset_clicked {
        return;
    }

    if ui
        .add(c::TextField::new(&mut node_clone.label, "Label").prefix("Label"))
        .changed()
    {
        modified = true;
    }

    ui.horizontal(|ui| {
        if ui
            .add(c::Checkbox::new(&mut node_clone.visible, "Visible"))
            .changed()
        {
            modified = true;
        }
        if ui
            .add(c::Checkbox::new(&mut node_clone.enabled, "Enabled"))
            .changed()
        {
            modified = true;
        }
    });

    // 1b. Contextual Live Geometry Inspector when editing Full Workspace Shell or Timeline Studio
    if state.active_preset.is_timeline_surface() {
        render_timeline_geometry_controls(ui, state);
    } else if state.active_preset.is_full_shell() {
        // Show Timeline controls first when a timeline sub-node (6..=14) is selected, otherwise Shell + Timeline
        if sel_id >= 6 {
            render_timeline_geometry_controls(ui, state);
            render_workspace_and_stage_controls(ui, state);
        } else {
            render_workspace_and_stage_controls(ui, state);
            render_timeline_geometry_controls(ui, state);
        }
    }

    // 2. Live Box Sizing (% or px / Auto / Fill)
    c::SectionHeader::new("BOX SIZING (% OR PX)")
        .id_salt(("studio-sec-box", sel_id))
        .show(ui, |ui| {
            let ref_w = state.systems.density.active().inspector_width;
            if render_size_unit_editor(
                ui,
                ("node-w", sel_id),
                "Width ",
                &mut node_clone.box_constraints.width,
                ref_w,
            ) {
                modified = true;
            }
            if render_size_unit_editor(
                ui,
                ("node-h", sel_id),
                "Height",
                &mut node_clone.box_constraints.height,
                state.systems.density.active().control_height,
            ) {
                modified = true;
            }

            // Quick 1-click % / px presets
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                for (lbl, unit) in [
                    ("25%", UiSizeUnit::Percent(25.0)),
                    ("33.3%", UiSizeUnit::Percent(33.3333)),
                    ("50%", UiSizeUnit::Percent(50.0)),
                    ("100%", UiSizeUnit::Percent(100.0)),
                    ("120px", UiSizeUnit::Px(120.0)),
                    ("320px", UiSizeUnit::Px(320.0)),
                    ("Auto", UiSizeUnit::Auto),
                ] {
                    if ui.add(c::Button::new(lbl)).clicked() {
                        node_clone.box_constraints.width = unit;
                        modified = true;
                    }
                }
            });

            // Min / Max width bounds
            ui.horizontal(|ui| {
                let mut min_w = f64::from(node_clone.box_constraints.min_width.unwrap_or(0.0));
                if ui
                    .add(
                        c::Field::new(&mut min_w, "Min W")
                            .prefix("MinW")
                            .unit("px")
                            .range(0.0..=1200.0)
                            .width(110.0),
                    )
                    .changed()
                {
                    node_clone.box_constraints.min_width = if min_w <= 0.0 {
                        None
                    } else {
                        Some(min_w as f32)
                    };
                    modified = true;
                }
                let mut max_w = f64::from(node_clone.box_constraints.max_width.unwrap_or(0.0));
                if ui
                    .add(
                        c::Field::new(&mut max_w, "Max W")
                            .prefix("MaxW")
                            .unit("px")
                            .range(0.0..=2400.0)
                            .width(110.0),
                    )
                    .changed()
                {
                    node_clone.box_constraints.max_width = if max_w <= 0.0 {
                        None
                    } else {
                        Some(max_w as f32)
                    };
                    modified = true;
                }
            });
        });

    // 3. Grid Column Tracks Editor (if node is a Grid)
    if let UiNodeKind::Grid { columns } = &mut node_clone.kind {
        c::SectionHeader::new("GRID COLUMNS (2, 3, 4+ TRACKS)")
            .id_salt(("studio-sec-grid", sel_id))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = space::S1;
                    if ui.add(c::Button::new("2 Cols (50/50%)")).clicked() {
                        *columns = vec![UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)];
                        modified = true;
                    }
                    if ui.add(c::Button::new("3 Cols (33.3%)")).clicked() {
                        *columns = vec![
                            UiSizeUnit::Percent(33.3333),
                            UiSizeUnit::Percent(33.3333),
                            UiSizeUnit::Percent(33.3334),
                        ];
                        modified = true;
                    }
                    if ui.add(c::Button::new("4 Cols (25%)")).clicked() {
                        *columns = vec![
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                            UiSizeUnit::Percent(25.0),
                        ];
                        modified = true;
                    }
                    if ui.add(c::Button::new("120px + 50% + 50%")).clicked() {
                        *columns = vec![
                            UiSizeUnit::Px(120.0),
                            UiSizeUnit::Percent(50.0),
                            UiSizeUnit::Percent(50.0),
                        ];
                        modified = true;
                    }
                });

                for (col_idx, col_unit) in columns.iter_mut().enumerate() {
                    if render_size_unit_editor(
                        ui,
                        ("grid-col", sel_id, col_idx),
                        &format!("Col {}", col_idx + 1),
                        col_unit,
                        280.0,
                    ) {
                        modified = true;
                    }
                }

                ui.horizontal(|ui| {
                    if ui.add(c::Button::new("+ Add Col")).clicked() {
                        columns.push(UiSizeUnit::Percent(25.0));
                        modified = true;
                    }
                    if columns.len() > 1 && ui.add(c::Button::new("- Remove Col")).clicked() {
                        columns.pop();
                        modified = true;
                    }
                });
            });
    }

    // 4. Paddings, Gaps & System Binding
    c::SectionHeader::new("PADDING, GAP & SYSTEM BINDING")
        .id_salt(("studio-sec-pad", sel_id))
        .show(ui, |ui| {
            let mut sys_choice: usize = match node_clone.style.system_binding {
                None => 0,
                Some(SharedSystemKind::TwoColumnForm) => 1,
                Some(SharedSystemKind::SectionHeader) => 2,
                Some(SharedSystemKind::PanelHeader) => 3,
                Some(SharedSystemKind::ToolRail) => 4,
            };
            if c::Select::options(
                ui,
                ("node-sys-bind", sel_id),
                &mut sys_choice,
                &[
                    (0, "System Binding: None (Local)"),
                    (1, "System: Two-Column Form"),
                    (2, "System: Section Header"),
                    (3, "System: Panel Header"),
                    (4, "System: Tool Rail"),
                ],
                220.0,
            ) {
                node_clone.style.system_binding = match sys_choice {
                    1 => Some(SharedSystemKind::TwoColumnForm),
                    2 => Some(SharedSystemKind::SectionHeader),
                    3 => Some(SharedSystemKind::PanelHeader),
                    4 => Some(SharedSystemKind::ToolRail),
                    _ => None,
                };
                modified = true;
            }

            if render_spacing_value_editor(
                ui,
                ("gap-x", sel_id),
                "Gap X ",
                &mut node_clone.style.gap.x,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("gap-y", sel_id),
                "Gap Y ",
                &mut node_clone.style.gap.y,
                &state.systems,
            ) {
                modified = true;
            }

            ui.horizontal_wrapped(|ui| {
                ui.label(rich("Uniform Pad:", TextStyle::Small).color(nf.text_secondary));
                for tok in [
                    SpacingToken::S0,
                    SpacingToken::S1,
                    SpacingToken::S2,
                    SpacingToken::S3,
                    SpacingToken::S4,
                ] {
                    if ui.add(c::Button::new(tok.short_name())).clicked() {
                        node_clone.style.padding = Padding::all_token(tok);
                        modified = true;
                    }
                }
            });

            if render_spacing_value_editor(
                ui,
                ("pad-t", sel_id),
                "Pad Top   ",
                &mut node_clone.style.padding.top,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("pad-r", sel_id),
                "Pad Right ",
                &mut node_clone.style.padding.right,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("pad-b", sel_id),
                "Pad Bottom",
                &mut node_clone.style.padding.bottom,
                &state.systems,
            ) {
                modified = true;
            }
            if render_spacing_value_editor(
                ui,
                ("pad-l", sel_id),
                "Pad Left  ",
                &mut node_clone.style.padding.left,
                &state.systems,
            ) {
                modified = true;
            }
        });

    // 5. Typography & Surface Styling
    c::SectionHeader::new("TYPOGRAPHY & SURFACE TOKENS")
        .id_salt(("studio-sec-typo", sel_id))
        .show(ui, |ui| {
            let mut is_custom = node_clone.style.typography.is_custom();
            if ui
                .add(c::Switch::new(&mut is_custom, "Custom Typography Override"))
                .changed()
            {
                if is_custom {
                    let _ = node_clone
                        .style
                        .typography
                        .promote_to_custom(&state.systems);
                } else {
                    node_clone
                        .style
                        .typography
                        .reset_to_system(node_clone.kind.default_text_style());
                }
                modified = true;
            }

            match &mut node_clone.style.typography {
                TypographyBinding::System(style_tok) => {
                    let opts: Vec<(StudioTextStyle, &str)> = StudioTextStyle::ALL
                        .into_iter()
                        .map(|s| (s, s.label()))
                        .collect();
                    if c::Select::options(ui, ("node-sys-typo", sel_id), style_tok, &opts, 210.0) {
                        modified = true;
                    }
                }
                TypographyBinding::Custom(ov) => {
                    if c::Select::options(
                        ui,
                        ("node-cust-fam", sel_id),
                        &mut ov.font_family,
                        &[
                            (StudioFontFamily::Inter, "Inter (UI)"),
                            (StudioFontFamily::RobotoMono, "Roboto Mono"),
                        ],
                        180.0,
                    ) {
                        modified = true;
                    }
                    ui.horizontal(|ui| {
                        let mut sz = f64::from(ov.size_pt);
                        if ui
                            .add(
                                c::Field::new(&mut sz, "Font Size")
                                    .prefix("Sz")
                                    .unit("pt")
                                    .range(8.0..=48.0)
                                    .decimals(1)
                                    .width(96.0),
                            )
                            .changed()
                        {
                            ov.size_pt = sz as f32;
                            modified = true;
                        }
                        let mut lh = f64::from(ov.line_height_pt);
                        if ui
                            .add(
                                c::Field::new(&mut lh, "Line Height")
                                    .prefix("LH")
                                    .unit("pt")
                                    .range(8.0..=64.0)
                                    .decimals(1)
                                    .width(96.0),
                            )
                            .changed()
                        {
                            ov.line_height_pt = lh as f32;
                            modified = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if c::Select::options(
                            ui,
                            ("node-cust-wt", sel_id),
                            &mut ov.weight,
                            &[
                                (400, "Regular (400)"),
                                (500, "Medium (500)"),
                                (600, "SemiBold (600)"),
                            ],
                            132.0,
                        ) {
                            modified = true;
                        }
                        if c::Select::options(
                            ui,
                            ("node-cust-align", sel_id),
                            &mut ov.align,
                            &[
                                (StudioTextAlign::Left, "Left"),
                                (StudioTextAlign::Center, "Center"),
                                (StudioTextAlign::Right, "Right"),
                            ],
                            92.0,
                        ) {
                            modified = true;
                        }
                    });
                    let role_opts: Vec<(StudioColorRole, &str)> = StudioColorRole::ALL
                        .into_iter()
                        .map(|r| (r, r.label()))
                        .collect();
                    if c::Select::options(
                        ui,
                        ("node-cust-col", sel_id),
                        &mut ov.color_token,
                        &role_opts,
                        210.0,
                    ) {
                        modified = true;
                    }
                }
            }
        });

    if modified {
        state.commit_history(format!("Edit node #{sel_id}"));
        if let Some(target) = state.document.find_node_mut(sel_id) {
            *target = node_clone;
        }
    }
}

fn render_timeline_geometry_controls(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    c::SectionHeader::new("TWEEN ARROWS (СТРЕЛКИ ТВИНОВ)")
        .id_salt("sys-sec-tween-arrows")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            let mut head_len = f64::from(tg.tween_arrow_head_length_px);
            if ui
                .add(
                    c::Field::new(&mut head_len, "Arrow Head Length (Длина наконечника)")
                        .prefix("Head L")
                        .unit("px")
                        .range(2.0..=28.0)
                        .decimals(1)
                        .stepper()
                        .width(176.0),
                )
                .changed()
            {
                tg.tween_arrow_head_length_px = head_len as f32;
            }

            let mut wing = f64::from(tg.tween_arrow_head_wing_px);
            if ui
                .add(
                    c::Field::new(&mut wing, "Arrow Wing (Размах крыльев)")
                        .prefix("Wing")
                        .unit("px")
                        .range(1.0..=16.0)
                        .decimals(1)
                        .stepper()
                        .width(176.0),
                )
                .changed()
            {
                tg.tween_arrow_head_wing_px = wing as f32;
            }

            ui.horizontal(|ui| {
                let mut start_in = f64::from(tg.tween_arrow_start_inset_px);
                if ui
                    .add(
                        c::Field::new(&mut start_in, "Start Inset (Отступ начала)")
                            .prefix("In L")
                            .unit("px")
                            .range(0.0..=40.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_start_inset_px = start_in as f32;
                }
                let mut end_in = f64::from(tg.tween_arrow_end_inset_px);
                if ui
                    .add(
                        c::Field::new(&mut end_in, "End Inset (Отступ конца)")
                            .prefix("In R")
                            .unit("px")
                            .range(0.0..=40.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_end_inset_px = end_in as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut stroke_w = f64::from(tg.tween_arrow_stroke_width_px);
                if ui
                    .add(
                        c::Field::new(&mut stroke_w, "Arrow Stroke (Толщина линии)")
                            .prefix("Stroke")
                            .unit("px")
                            .range(0.5..=5.0)
                            .decimals(2)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_stroke_width_px = stroke_w as f32;
                }
                let mut y_pct = f64::from(tg.tween_arrow_y_pct);
                if ui
                    .add(
                        c::Field::new(&mut y_pct, "Arrow Y Position (%)")
                            .prefix("Y Pos")
                            .unit("%")
                            .range(10.0..=90.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.tween_arrow_y_pct = y_pct as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut dash = f64::from(tg.broken_tween_dash_px);
                if ui
                    .add(
                        c::Field::new(&mut dash, "Broken Dash")
                            .prefix("Dash")
                            .unit("px")
                            .range(1.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.broken_tween_dash_px = dash as f32;
                }
                let mut gap = f64::from(tg.broken_tween_gap_px);
                if ui
                    .add(
                        c::Field::new(&mut gap, "Broken Gap")
                            .prefix("Gap")
                            .unit("px")
                            .range(1.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.broken_tween_gap_px = gap as f32;
                }
            });
        });

    c::SectionHeader::new("TIMELINE CELLS, KEYFRAMES & BADGES")
        .id_salt("sys-sec-tl-cells")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            ui.horizontal(|ui| {
                let mut cw = f64::from(tg.frame_cell_width_px);
                if ui
                    .add(
                        c::Field::new(&mut cw, "Frame Cell Width")
                            .prefix("Cell W")
                            .unit("px")
                            .range(6.0..=32.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.frame_cell_width_px = cw as f32;
                }
                let mut rh = f64::from(tg.layer_row_height_px);
                if ui
                    .add(
                        c::Field::new(&mut rh, "Layer Row Height")
                            .prefix("Row H")
                            .unit("px")
                            .range(14.0..=48.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.layer_row_height_px = rh as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut dot_r = f64::from(tg.keyframe_dot_radius_px);
                if ui
                    .add(
                        c::Field::new(&mut dot_r, "Keyframe Dot Radius (●/○)")
                            .prefix("Dot R")
                            .unit("px")
                            .range(1.5..=8.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.keyframe_dot_radius_px = dot_r as f32;
                }
                let mut dia_r = f64::from(tg.motion_diamond_radius_px);
                if ui
                    .add(
                        c::Field::new(&mut dia_r, "Motion Diamond Radius (◆)")
                            .prefix("Dia R")
                            .unit("px")
                            .range(1.5..=9.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.motion_diamond_radius_px = dia_r as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut end_w = f64::from(tg.span_end_rect_w_px);
                if ui
                    .add(
                        c::Field::new(&mut end_w, "Span End Rect W ([ ])")
                            .prefix("End W")
                            .unit("px")
                            .range(2.0..=12.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.span_end_rect_w_px = end_w as f32;
                }
                let mut end_h = f64::from(tg.span_end_rect_h_px);
                if ui
                    .add(
                        c::Field::new(&mut end_h, "Span End Rect H ([ ])")
                            .prefix("End H")
                            .unit("px")
                            .range(3.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.span_end_rect_h_px = end_h as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut act_sz = f64::from(tg.action_marker_size_px);
                if ui
                    .add(
                        c::Field::new(&mut act_sz, "Action Marker 'a' Size")
                            .prefix("Act a")
                            .unit("px")
                            .range(5.0..=14.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.action_marker_size_px = act_sz as f32;
                }
                let mut flag_sz = f64::from(tg.label_flag_size_px);
                if ui
                    .add(
                        c::Field::new(&mut flag_sz, "Label Flag Size")
                            .prefix("Flag")
                            .unit("px")
                            .range(3.0..=12.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.label_flag_size_px = flag_sz as f32;
                }
            });
        });

    c::SectionHeader::new("PLAYHEAD, RULER & ONION SKIN")
        .id_salt("sys-sec-playhead")
        .show(ui, |ui| {
            let tg = &mut state.systems.timeline_geometry;
            ui.horizontal(|ui| {
                let mut ph_f = tg.playhead_frame as f64;
                if ui
                    .add(
                        c::Field::new(&mut ph_f, "Playhead Frame")
                            .prefix("Frame")
                            .range(1.0..=60.0)
                            .decimals(0)
                            .stepper()
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_frame = (ph_f.round() as usize).clamp(1, 60);
                }
                let mut rul_h = f64::from(tg.ruler_height_px);
                if ui
                    .add(
                        c::Field::new(&mut rul_h, "Ruler Height")
                            .prefix("Ruler H")
                            .unit("px")
                            .range(14.0..=40.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.ruler_height_px = rul_h as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut cap_w = f64::from(tg.playhead_cap_width_px);
                if ui
                    .add(
                        c::Field::new(&mut cap_w, "Playhead Cap W")
                            .prefix("Cap W")
                            .unit("px")
                            .range(6.0..=24.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_cap_width_px = cap_w as f32;
                }
                let mut cap_h = f64::from(tg.playhead_cap_height_px);
                if ui
                    .add(
                        c::Field::new(&mut cap_h, "Playhead Cap H")
                            .prefix("Cap H")
                            .unit("px")
                            .range(6.0..=24.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_cap_height_px = cap_h as f32;
                }
            });

            ui.horizontal(|ui| {
                let mut ph_lw = f64::from(tg.playhead_line_width_px);
                if ui
                    .add(
                        c::Field::new(&mut ph_lw, "Playhead Line Width")
                            .prefix("Line W")
                            .unit("px")
                            .range(0.5..=4.0)
                            .decimals(2)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.playhead_line_width_px = ph_lw as f32;
                }
                let mut ob_w = f64::from(tg.onion_bracket_width_px);
                if ui
                    .add(
                        c::Field::new(&mut ob_w, "Onion Bracket Arm W")
                            .prefix("Onion W")
                            .unit("px")
                            .range(2.0..=14.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.onion_bracket_width_px = ob_w as f32;
                }
            });
        });

    c::SectionHeader::new("LAYER TREE COLUMN (% OR PX)")
        .id_salt("sys-sec-layer-col")
        .show(ui, |ui| {
            let _ = render_size_unit_editor(
                ui,
                "tl-layer-col-w",
                "Layer Col W",
                &mut state.systems.timeline_geometry.layer_list_width,
                760.0,
            );
            let tg = &mut state.systems.timeline_geometry;
            ui.horizontal(|ui| {
                let mut ind = f64::from(tg.layer_indent_px);
                if ui
                    .add(
                        c::Field::new(&mut ind, "Folder Indent")
                            .prefix("Indent")
                            .unit("px")
                            .range(6.0..=32.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.layer_indent_px = ind as f32;
                }
                let mut igap = f64::from(tg.layer_icon_gap_px);
                if ui
                    .add(
                        c::Field::new(&mut igap, "Icon Gap")
                            .prefix("IcoGap")
                            .unit("px")
                            .range(2.0..=16.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    tg.layer_icon_gap_px = igap as f32;
                }
            });
        });
}

fn render_workspace_and_stage_controls(ui: &mut Ui, state: &mut UiLayoutStudioState) {
    c::SectionHeader::new("WORKSPACE DOCK PROPORTIONS (% OR PX)")
        .id_salt("sys-sec-shell-docks")
        .show(ui, |ui| {
            let _ = render_size_unit_editor(
                ui,
                "shell-left-tools-w",
                "Left Tools ",
                &mut state.systems.workspace_shell.shell_left_tools_width,
                820.0,
            );
            let _ = render_size_unit_editor(
                ui,
                "shell-right-insp-w",
                "Right Dock ",
                &mut state.systems.workspace_shell.shell_right_inspector_width,
                820.0,
            );
            let _ = render_size_unit_editor(
                ui,
                "shell-bot-tl-h",
                "Timeline H ",
                &mut state.systems.workspace_shell.shell_bottom_timeline_height,
                560.0,
            );

            let ws = &mut state.systems.workspace_shell;
            ui.horizontal(|ui| {
                let mut mh = f64::from(ws.top_menubar_height_px);
                if ui
                    .add(
                        c::Field::new(&mut mh, "Menu Bar H")
                            .prefix("Menu H")
                            .unit("px")
                            .range(18.0..=44.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    ws.top_menubar_height_px = mh as f32;
                }
                let mut th = f64::from(ws.doc_tabs_height_px);
                if ui
                    .add(
                        c::Field::new(&mut th, "Doc Tabs H")
                            .prefix("Tabs H")
                            .unit("px")
                            .range(20.0..=44.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    ws.doc_tabs_height_px = th as f32;
                }
            });
        });

    c::SectionHeader::new("STAGE RULERS, GUIDES & HANDLES")
        .id_salt("sys-sec-stage-overlay")
        .show(ui, |ui| {
            let stg = &mut state.systems.stage_overlay;
            ui.horizontal(|ui| {
                ui.add(c::Checkbox::new(&mut stg.show_rulers, "Show Rulers"));
                ui.add(c::Checkbox::new(&mut stg.show_guides, "Show Cyan Guides"));
            });
            ui.horizontal(|ui| {
                let mut rt = f64::from(stg.stage_ruler_thickness_px);
                if ui
                    .add(
                        c::Field::new(&mut rt, "Stage Ruler Thickness")
                            .prefix("Ruler")
                            .unit("px")
                            .range(12.0..=32.0)
                            .decimals(0)
                            .width(116.0),
                    )
                    .changed()
                {
                    stg.stage_ruler_thickness_px = rt as f32;
                }
                let mut hs = f64::from(stg.selection_handle_size_px);
                if ui
                    .add(
                        c::Field::new(&mut hs, "Selection Handle Size")
                            .prefix("Handle")
                            .unit("px")
                            .range(3.0..=12.0)
                            .decimals(1)
                            .width(116.0),
                    )
                    .changed()
                {
                    stg.selection_handle_size_px = hs as f32;
                }
            });
        });
}

fn render_systems_inspector_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, _width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S1;
        for (sub, lbl) in [
            (SystemsSubTab::TimelineGeometry, "Timeline & Arrows"),
            (SystemsSubTab::WorkspaceAndStage, "Shell & Stage"),
            (SystemsSubTab::SpacingAndDensity, "Spacing & Density"),
            (SystemsSubTab::TypographyScale, "Typography"),
            (SystemsSubTab::SharedLayouts, "Shared Systems"),
            (SystemsSubTab::PaletteTokens, "Palette"),
        ] {
            let active = state.systems_subtab == sub;
            let btn = if active {
                c::Button::primary(lbl)
            } else {
                c::Button::new(lbl)
            };
            if ui.add(btn).clicked() {
                state.systems_subtab = sub;
            }
        }
    });

    let mut reset_systems_clicked = false;
    ui.horizontal(|ui| {
        ui.label(
            rich(
                "Editing a system updates all bound widgets across the canvas.",
                TextStyle::Caption,
            )
            .color(nf.text_secondary),
        );
        if ui.add(c::Button::ghost("Reset All Systems")).clicked() {
            state.commit_history("Reset all NF2 systems to defaults");
            state.systems.reset_all_to_defaults();
            state.status_message = "Reset all NF2 system scales to defaults.".to_owned();
            reset_systems_clicked = true;
        }
    });
    if reset_systems_clicked {
        return;
    }

    match state.systems_subtab {
        SystemsSubTab::TimelineGeometry => {
            render_timeline_geometry_controls(ui, state);
        }
        SystemsSubTab::WorkspaceAndStage => {
            render_workspace_and_stage_controls(ui, state);
        }
        SystemsSubTab::SpacingAndDensity => {
            c::SectionHeader::new("SPACING SCALE (S1..S6 IN PT)")
                .id_salt("sys-sec-spacing")
                .show(ui, |ui| {
                    for tok in [
                        SpacingToken::S1,
                        SpacingToken::S2,
                        SpacingToken::S3,
                        SpacingToken::S4,
                        SpacingToken::S5,
                        SpacingToken::S6,
                    ] {
                        let mut v = f64::from(state.systems.spacing.resolve(tok));
                        ui.horizontal(|ui| {
                            ui.label(
                                rich(tok.short_name(), TextStyle::MonoSmall)
                                    .color(nf.text_secondary),
                            );
                            if ui
                                .add(
                                    c::Field::new(&mut v, tok.short_name())
                                        .prefix(tok.short_name())
                                        .unit("pt")
                                        .range(0.0..=96.0)
                                        .decimals(0)
                                        .stepper()
                                        .width(128.0),
                                )
                                .changed()
                            {
                                state.systems.spacing.set(tok, v as f32);
                            }
                        });
                    }
                });

            c::SectionHeader::new("DENSITY PROFILE METRICS")
                .id_salt("sys-sec-density")
                .show(ui, |ui| {
                    let mut prof = state.systems.density.active_profile;
                    if c::Select::options(
                        ui,
                        "sys-density-prof",
                        &mut prof,
                        &[
                            (StudioDensityProfile::Comfortable, "Profile: Comfortable"),
                            (StudioDensityProfile::Compact, "Profile: Compact"),
                            (StudioDensityProfile::ClassicFlash, "Profile: Classic Flash"),
                        ],
                        210.0,
                    ) {
                        state.systems.density.active_profile = prof;
                    }

                    let metrics = state.systems.density.active_mut();
                    let mut ctrl_h = f64::from(metrics.control_height);
                    if ui
                        .add(
                            c::Field::new(&mut ctrl_h, "Control Height")
                                .prefix("Ctrl H")
                                .unit("pt")
                                .range(18.0..=48.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.control_height = ctrl_h as f32;
                    }

                    let mut row_h = f64::from(metrics.row_height);
                    if ui
                        .add(
                            c::Field::new(&mut row_h, "Row Height")
                                .prefix("Row H")
                                .unit("pt")
                                .range(16.0..=44.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.row_height = row_h as f32;
                    }

                    let mut layer_h = f64::from(metrics.layer_row_height);
                    if ui
                        .add(
                            c::Field::new(&mut layer_h, "Layer Row Height")
                                .prefix("Layer H")
                                .unit("pt")
                                .range(12.0..=44.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.layer_row_height = layer_h as f32;
                    }

                    let mut icon_g = f64::from(metrics.icon_grid_pt);
                    if ui
                        .add(
                            c::Field::new(&mut icon_g, "Icon Grid")
                                .prefix("Icon")
                                .unit("pt")
                                .range(12.0..=24.0)
                                .decimals(0)
                                .width(160.0),
                        )
                        .changed()
                    {
                        metrics.icon_grid_pt = icon_g as f32;
                    }
                });
        }
        SystemsSubTab::TypographyScale => {
            c::SectionHeader::new("NF2 TYPOGRAPHY SCALE")
                .id_salt("sys-sec-typo-scale")
                .show(ui, |ui| {
                    for style in [
                        StudioTextStyle::Title,
                        StudioTextStyle::Body,
                        StudioTextStyle::BodyStrong,
                        StudioTextStyle::Small,
                        StudioTextStyle::Caption,
                        StudioTextStyle::Overline,
                        StudioTextStyle::Mono,
                        StudioTextStyle::Heading,
                    ] {
                        let entry = state.systems.typography.entry_mut(style);
                        ui.label(
                            rich(style.label(), TextStyle::SmallStrong).color(nf.text_primary),
                        );
                        ui.horizontal(|ui| {
                            let mut sz = f64::from(entry.size_pt);
                            if ui
                                .add(
                                    c::Field::new(&mut sz, "Size")
                                        .prefix("Sz")
                                        .unit("pt")
                                        .range(8.0..=36.0)
                                        .decimals(1)
                                        .width(92.0),
                                )
                                .changed()
                            {
                                entry.size_pt = sz as f32;
                            }
                            let mut lh = f64::from(entry.line_height_pt);
                            if ui
                                .add(
                                    c::Field::new(&mut lh, "Line Height")
                                        .prefix("LH")
                                        .unit("pt")
                                        .range(8.0..=48.0)
                                        .decimals(1)
                                        .width(92.0),
                                )
                                .changed()
                            {
                                entry.line_height_pt = lh as f32;
                            }
                        });
                    }
                });
        }
        SystemsSubTab::SharedLayouts => {
            c::SectionHeader::new("TWO-COLUMN FORM SYSTEM")
                .id_salt("sys-sec-2col")
                .show(ui, |ui| {
                    let mut min_cell = f64::from(state.systems.two_column_form.min_cell_width);
                    if ui
                        .add(
                            c::Field::new(&mut min_cell, "Min Cell W")
                                .prefix("MinW")
                                .unit("px")
                                .range(48.0..=240.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.min_cell_width = min_cell as f32;
                    }
                    let mut lbl_w = f64::from(state.systems.two_column_form.label_width_px);
                    if ui
                        .add(
                            c::Field::new(&mut lbl_w, "Label Width")
                                .prefix("LblW")
                                .unit("px")
                                .range(40.0..=180.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.label_width_px = lbl_w as f32;
                    }
                    let mut gap_px =
                        f64::from(state.systems.two_column_form.gap.resolve(&state.systems));
                    if ui
                        .add(
                            c::Field::new(&mut gap_px, "Form Gap")
                                .prefix("Gap")
                                .unit("px")
                                .range(0.0..=48.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.two_column_form.gap = SpacingValue::Px(gap_px as f32);
                    }
                });

            c::SectionHeader::new("SECTION & PANEL HEADER SYSTEMS")
                .id_salt("sys-sec-headers")
                .show(ui, |ui| {
                    let mut sec_h = f64::from(state.systems.section_header.height);
                    if ui
                        .add(
                            c::Field::new(&mut sec_h, "Section Header H")
                                .prefix("SecH")
                                .unit("pt")
                                .range(20.0..=56.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.section_header.height = sec_h as f32;
                    }
                    let mut pnl_h = f64::from(state.systems.panel_header.height);
                    if ui
                        .add(
                            c::Field::new(&mut pnl_h, "Panel Header H")
                                .prefix("PnlH")
                                .unit("pt")
                                .range(24.0..=64.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.panel_header.height = pnl_h as f32;
                    }
                });

            c::SectionHeader::new("TOOL RAIL SYSTEM")
                .id_salt("sys-sec-toolrail")
                .show(ui, |ui| {
                    let mut cols = state.systems.tool_rail.columns as f64;
                    if ui
                        .add(
                            c::Field::new(&mut cols, "Tool Columns")
                                .prefix("Cols")
                                .range(1.0..=4.0)
                                .stepper()
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.tool_rail.columns = (cols.round() as usize).clamp(1, 4);
                    }
                    let mut side = f64::from(state.systems.tool_rail.button_side);
                    if ui
                        .add(
                            c::Field::new(&mut side, "Tool Button Side")
                                .prefix("Side")
                                .unit("pt")
                                .range(20.0..=48.0)
                                .width(150.0),
                        )
                        .changed()
                    {
                        state.systems.tool_rail.button_side = side as f32;
                    }
                });
        }
        SystemsSubTab::PaletteTokens => {
            let dark_mode = ui.ctx().style().visuals.dark_mode;
            c::SectionHeader::new(if dark_mode {
                "DARK PALETTE TOKEN OVERRIDES"
            } else {
                "LIGHT PALETTE TOKEN OVERRIDES"
            })
            .id_salt("sys-sec-palette")
            .show(ui, |ui| {
                for role in [
                    StudioColorRole::BgApp,
                    StudioColorRole::BgPanel,
                    StudioColorRole::BgRaised,
                    StudioColorRole::BgField,
                    StudioColorRole::TextPrimary,
                    StudioColorRole::TextSecondary,
                    StudioColorRole::AccentDefault,
                    StudioColorRole::BorderDefault,
                    StudioColorRole::BorderSubtle,
                ] {
                    let mut col = state
                        .systems
                        .palette_overrides
                        .resolve_color(dark_mode, role);
                    ui.horizontal(|ui| {
                        ui.label(rich(role.label(), TextStyle::MonoSmall).color(nf.text_secondary));
                        let resp = c::ColorField::new(&mut col, role.label())
                            .width(124.0)
                            .show(ui);
                        if resp.response.changed() {
                            state
                                .systems
                                .palette_overrides
                                .set_color_override(dark_mode, role, col);
                        }
                        if state
                            .systems
                            .palette_overrides
                            .has_override(dark_mode, role)
                            && ui.add(c::Button::ghost("Reset")).clicked()
                        {
                            state
                                .systems
                                .palette_overrides
                                .reset_color_override(dark_mode, role);
                        }
                    });
                }
            });
        }
    }
}

fn render_code_export_tab(ui: &mut Ui, state: &mut UiLayoutStudioState, width: f32) {
    let nf = neoflash_ui::palette(ui.ctx());
    let code = export_rust_layout_with_systems(&state.document, &state.systems);
    let svg_code = state.export_active_svg();

    ui.horizontal_wrapped(|ui| {
        if ui
            .add(c::Button::new("Copy Rust Code").icon(i16::COPY))
            .clicked()
        {
            ui.ctx().copy_text(code.clone());
            state.status_message = "Copied generated Rust NF2 code to clipboard.".to_owned();
        }
        if ui
            .add(c::Button::new("Copy SVG XML").icon(i16::COPY))
            .clicked()
        {
            ui.ctx().copy_text(svg_code.clone());
            state.svg_buffer = svg_code.clone();
            state.status_message = "Copied active artboard SVG XML to clipboard.".to_owned();
        }
        if ui
            .add(c::Button::new("Import SVG → Panel…").icon(i16::UPLOAD))
            .clicked()
        {
            state.svg_import_modal_open = true;
        }
        if ui
            .add(c::Button::primary("Write All to target/ui-studio/").icon(i16::SAVE))
            .clicked()
        {
            let _ = state.save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH));
            let _ = state.save_svg_to_file(Path::new(DEFAULT_STUDIO_SVG_EXPORT_PATH));
            if let Ok(path) = state.export_rust_to_file(Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH))
            {
                state.status_message = format!(
                    "Wrote {}, {}, and {DEFAULT_STUDIO_SVG_EXPORT_PATH}",
                    DEFAULT_STUDIO_JSON_PATH,
                    path.display()
                );
            }
        }
    });

    ui.label(
        rich(
            format!(
                "JSON: {DEFAULT_STUDIO_JSON_PATH}\nRust: {DEFAULT_STUDIO_RUST_EXPORT_PATH}\nSVG:  {DEFAULT_STUDIO_SVG_EXPORT_PATH}"
            ),
            TextStyle::MonoSmall,
        )
        .color(nf.text_tertiary),
    );

    let mut preview_code = code;
    let _ = width;
    ui.label(rich("GENERATED RUST NF2 CODE", TextStyle::Overline).color(nf.text_secondary));
    ui.add(c::TextArea::new(&mut preview_code, "Generated Rust Layout Code").rows(12));

    let mut preview_svg = svg_code;
    ui.label(
        rich(
            "GENERATED SVG ARTBOARD XML (ROUND-TRIP & FIGMA READY)",
            TextStyle::Overline,
        )
        .color(nf.text_secondary),
    );
    ui.add(c::TextArea::new(&mut preview_svg, "Generated SVG XML").rows(10));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8b. Standalone Native Application (`NeoFlash UI Studio.app`)
// ─────────────────────────────────────────────────────────────────────────────

/// Standalone native `eframe::App` for `NeoFlash UI Studio.app`.
pub struct StandaloneUiStudioApp {
    pub state: UiLayoutStudioState,
    pub dark_mode: bool,
}

impl StandaloneUiStudioApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut defs = egui::FontDefinitions::default();
        neoflash_ui::typography::add_fonts(&mut defs);
        cc.egui_ctx.set_fonts(defs);
        crate::theme::initialize_editor_style(&cc.egui_ctx);
        let dark_mode = std::env::var("NEOFLASH_SCREENSHOT_THEME")
            .ok()
            .is_none_or(|t| !t.eq_ignore_ascii_case("light"));
        neoflash_ui::theme::apply_visuals(
            &cc.egui_ctx,
            if dark_mode {
                neoflash_ui::Theme::Dark
            } else {
                neoflash_ui::Theme::Light
            },
        );
        let mut state = UiLayoutStudioState {
            open: true,
            ..Default::default()
        };
        let json_path = Path::new(DEFAULT_STUDIO_JSON_PATH);
        if json_path.exists() {
            let _ = state.load_studio_json(json_path);
        }
        Self { state, dark_mode }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let mut do_undo = false;
        let mut do_redo = false;
        let mut do_dup = false;
        let mut do_del = false;
        let mut do_save = false;
        let mut do_export = false;
        ctx.input_mut(|i| {
            if i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) {
                do_redo = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                do_undo = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::D) {
                do_dup = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                do_save = true;
            } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::E) {
                do_export = true;
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
            {
                do_del = true;
            }
        });
        if do_undo {
            self.state.undo();
        } else if do_redo {
            self.state.redo();
        } else if do_dup {
            let sel = self.state.primary_selected_id();
            if sel != self.state.document.root.id {
                self.state.commit_history("Duplicate node");
                if let Some(dup_id) = self.state.document.duplicate_node(sel) {
                    self.state.select_single(dup_id);
                    self.state.status_message = "Duplicated selected node (⌘D).".to_owned();
                }
            }
        } else if do_del {
            let targets: Vec<UiNodeId> = self
                .state
                .selected_nodes
                .iter()
                .copied()
                .filter(|&id| id != self.state.document.root.id)
                .collect();
            if !targets.is_empty() {
                self.state.commit_history("Delete node(s)");
                let count = self.state.document.delete_nodes(&targets);
                self.state.select_single(self.state.document.root.id);
                self.state.status_message = format!("Deleted {count} node(s).");
            }
        } else if do_save {
            if let Ok(saved) = self
                .state
                .save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH))
            {
                self.state.status_message = format!("Saved overrides to {} (⌘S)", saved.display());
            }
        } else if do_export {
            let _ = self
                .state
                .save_studio_json(Path::new(DEFAULT_STUDIO_JSON_PATH));
            if let Ok(saved) = self
                .state
                .export_rust_to_file(Path::new(DEFAULT_STUDIO_RUST_EXPORT_PATH))
            {
                self.state.inspector_tab = StudioInspectorTab::CodeExport;
                self.state.status_message =
                    format!("Exported Rust build code to {} (⌘E)", saved.display());
            }
        }
    }
}

impl eframe::App for StandaloneUiStudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);
        let nf = neoflash_ui::palette(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(nf.bg_panel)
                    .inner_margin(egui::Margin::same(space::S3 as i8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        rich(
                            "NEOFLASH UI LAYOUT & DESIGN SYSTEM STUDIO",
                            TextStyle::Title,
                        )
                        .color(nf.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let theme_lbl = if self.dark_mode {
                            "Theme: Dark"
                        } else {
                            "Theme: Light"
                        };
                        if ui.add(c::Button::new(theme_lbl)).clicked() {
                            self.dark_mode = !self.dark_mode;
                            neoflash_ui::theme::apply_visuals(
                                ctx,
                                if self.dark_mode {
                                    neoflash_ui::Theme::Dark
                                } else {
                                    neoflash_ui::Theme::Light
                                },
                            );
                        }
                    });
                });
                ui.add_space(space::S1);
                ui_layout_studio_panel_ui(ui, &mut self.state);
            });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Unit & Headless UI Verification Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_px_sizing_and_track_resolution_are_exact() {
        let box_pct = UiBoxConstraints {
            width: UiSizeUnit::Percent(50.0),
            height: UiSizeUnit::Px(32.0),
            min_width: Some(100.0),
            max_width: Some(220.0),
            min_height: None,
            max_height: None,
        };
        assert_eq!(box_pct.resolve_width(300.0, 60.0), 150.0);
        // Clamped to min_width = 100.0 when 50% of 160 is 80
        assert_eq!(box_pct.resolve_width(160.0, 60.0), 100.0);
        // Clamped to max_width = 220.0 when 50% of 600 is 300
        assert_eq!(box_pct.resolve_width(600.0, 60.0), 220.0);
        assert_eq!(box_pct.resolve_height(500.0, 20.0), 32.0);

        // Unit conversion % <-> px
        assert_eq!(UiSizeUnit::Percent(25.0).to_px(320.0), UiSizeUnit::Px(80.0));
        assert_eq!(
            UiSizeUnit::Px(80.0).to_percent(320.0),
            UiSizeUnit::Percent(25.0)
        );

        // 2-col, 3-col, 4-col, and mixed [120px, 50%, 50%] track resolution
        let two_col = resolve_track_sizes(
            &[UiSizeUnit::Percent(50.0), UiSizeUnit::Percent(50.0)],
            308.0,
            8.0,
            &[64.0, 64.0],
        );
        assert_eq!(two_col, vec![150.0, 150.0]);

        let four_col = resolve_track_sizes(
            &[
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
                UiSizeUnit::Percent(25.0),
            ],
            424.0,
            8.0,
            &[40.0; 4],
        );
        assert_eq!(four_col, vec![100.0, 100.0, 100.0, 100.0]);

        let mixed = resolve_track_sizes(
            &[
                UiSizeUnit::Px(120.0),
                UiSizeUnit::Percent(50.0),
                UiSizeUnit::Percent(50.0),
            ],
            336.0,
            8.0,
            &[64.0; 3],
        );
        // 336 - 2*8 gap = 320 usable; 120px fixed leaves 200px, split 50%/50% = 100px each
        assert_eq!(mixed, vec![120.0, 100.0, 100.0]);
    }

    #[test]
    fn system_tokens_propagate_to_bound_nodes_and_preserve_local_overrides() {
        let mut state = UiLayoutStudioState::default();
        state.load_preset(StudioPanelPreset::PropertiesMovieClip);
        let origin = egui::pos2(0.0, 0.0);
        let avail = egui::vec2(400.0, 600.0);

        // Locate the 2-column Transform Grid inside Properties — Movie Clip
        let grid_id = state.document.root.children[1].children[0].id;
        let first_field_id = state.document.root.children[1].children[0].children[0].id;

        let layout_before = compute_studio_layout(&state.document, &state.systems, origin, avail);
        let gap_before = layout_before.node_box(grid_id).unwrap().gap.x;
        let field_h_before = layout_before
            .node_box(first_field_id)
            .unwrap()
            .resolved_height;
        assert_eq!(gap_before, 8.0);
        assert_eq!(field_h_before, 32.0);

        // 1. Edit TwoColumnFormSystem gap -> immediately updates bound Grid gap
        state.systems.two_column_form.gap = SpacingValue::Px(16.0);
        // 2. Switch density profile to Classic Flash -> immediately updates leaf control heights to 24pt
        state.systems.density.active_profile = StudioDensityProfile::ClassicFlash;
        // 3. Edit Typography Mono system scale -> immediately updates Field typography
        state
            .systems
            .typography
            .entry_mut(StudioTextStyle::Mono)
            .size_pt = 13.5;

        let layout_after = compute_studio_layout(&state.document, &state.systems, origin, avail);
        assert_eq!(layout_after.node_box(grid_id).unwrap().gap.x, 16.0);
        assert_eq!(
            layout_after
                .node_box(first_field_id)
                .unwrap()
                .resolved_height,
            24.0
        );
        assert_eq!(
            layout_after
                .node_box(first_field_id)
                .unwrap()
                .typography
                .size_pt,
            13.5
        );

        // Local override on `first_field_id` wins over system scale until reset
        {
            let node = state.document.find_node_mut(first_field_id).unwrap();
            node.style
                .typography
                .promote_to_custom(&state.systems)
                .size_pt = 19.0;
        }
        let layout_override = compute_studio_layout(&state.document, &state.systems, origin, avail);
        assert_eq!(
            layout_override
                .node_box(first_field_id)
                .unwrap()
                .typography
                .size_pt,
            19.0
        );

        // 1-click reset restores system binding
        assert!(state.document.reset_node_to_system_defaults(first_field_id));
        let layout_reset = compute_studio_layout(&state.document, &state.systems, origin, avail);
        assert_eq!(
            layout_reset
                .node_box(first_field_id)
                .unwrap()
                .typography
                .size_pt,
            13.5
        );
    }

    #[test]
    fn wrap_unwrap_nested_grid_in_grid_and_reordering_work_with_undo_redo() {
        let mut state = UiLayoutStudioState::default();
        state.load_preset(StudioPanelPreset::PropertiesMovieClip);
        let pos_sec_id = state.document.root.children[1].id;
        let pos_grid_id = state.document.root.children[1].children[0].id;
        let x_id = state.document.root.children[1].children[0].children[0].id;
        let y_id = state.document.root.children[1].children[0].children[1].id;

        // Wrap X and Y fields inside a nested 3-column Grid inside the outer 2-column Grid's first cell!
        state.commit_history("Wrap X and Y in 3-col Grid");
        let nested_grid_id = state
            .document
            .wrap_selected_nodes(&[x_id, y_id], ContainerWrapPreset::Grid3Equal)
            .expect("wrap succeeds");

        // Verify nested grid is now child 0 of `pos_grid_id` and holds `[x_id, y_id]`
        let outer_grid = state.document.find_node(pos_grid_id).unwrap();
        assert_eq!(outer_grid.children[0].id, nested_grid_id);
        assert_eq!(outer_grid.children[0].children.len(), 2);

        // Insert a 4-column sub-grid inside `nested_grid_id` (3 levels of nested grids!)
        let deep_grid_id = state
            .document
            .insert_nested_grid(
                nested_grid_id,
                vec![
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                    UiSizeUnit::Percent(25.0),
                ],
                true,
            )
            .expect("deep nested grid");

        let computed = compute_studio_layout(
            &state.document,
            &state.systems,
            egui::pos2(0.0, 0.0),
            egui::vec2(360.0, 600.0),
        );
        let outer_box = computed.node_box(pos_grid_id).unwrap();
        let nested_box = computed.node_box(nested_grid_id).unwrap();
        let deep_box = computed.node_box(deep_grid_id).unwrap();
        assert!(nested_box.resolved_width < outer_box.resolved_width);
        assert!(deep_box.resolved_width < nested_box.resolved_width);
        assert!(deep_box.resolved_width > 0.0);

        // Reorder X and Y inside `nested_grid_id`
        assert!(state.document.move_node_down(x_id));
        assert_eq!(
            state.document.find_node(nested_grid_id).unwrap().children[0].id,
            y_id
        );
        assert!(state.document.move_node_up(x_id));
        assert_eq!(
            state.document.find_node(nested_grid_id).unwrap().children[0].id,
            x_id
        );

        // Cycle prevention on reparenting: cannot reparent `pos_sec_id` into its own descendant `nested_grid_id`
        assert!(
            !state
                .document
                .reparent_node(pos_sec_id, nested_grid_id, None)
        );

        // Unwrap `nested_grid_id` -> splices children back into `pos_grid_id`
        assert!(state.document.unwrap_selected_container(nested_grid_id));
        assert!(state.document.find_node(nested_grid_id).is_none());
        assert_eq!(
            state.document.find_node(pos_grid_id).unwrap().children[0].id,
            x_id
        );

        // Undo & Redo restore the exact states
        assert!(state.undo());
        assert!(state.redo());
    }

    #[test]
    fn json_round_trip_and_rust_codegen_export_are_deterministic() {
        let mut state = UiLayoutStudioState::default();
        state.load_preset(StudioPanelPreset::PropertiesMovieClip);
        state.systems.spacing.s3 = 14.0;
        state.systems.two_column_form.label_width_px = 84.0;
        state.systems.timeline_geometry.tween_arrow_head_length_px = 12.5;

        let tmp_dir =
            std::env::temp_dir().join(format!("nf2-ui-studio-test-{}", std::process::id()));
        let json_path = tmp_dir.join("ui-layout-overrides.json");
        let rust_path = tmp_dir.join("exported_layout.rs");

        state.save_studio_json(&json_path).expect("save JSON");
        state.export_rust_to_file(&rust_path).expect("export Rust");

        let mut loaded = UiLayoutStudioState::default();
        loaded.load_studio_json(&json_path).expect("load JSON");
        assert_eq!(loaded.document, state.document);
        assert_eq!(loaded.systems.spacing.s3, 14.0);
        assert_eq!(loaded.systems.two_column_form.label_width_px, 84.0);
        assert_eq!(
            loaded.systems.timeline_geometry.tween_arrow_head_length_px,
            12.5
        );

        let code1 = export_rust_layout_with_systems(&state.document, &state.systems);
        let code2 = export_rust_layout_with_systems(&loaded.document, &loaded.systems);
        assert_eq!(code1, code2);
        assert!(code1.contains("pub fn render_properties_movie_clip(ui: &mut Ui)"));
        assert!(code1.contains("c::SectionHeader::new(\"POSITION AND SIZE\")"));
        assert!(code1.contains("pub const TWEEN_ARROW_HEAD_LENGTH_PX: f32 = 12.50;"));

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn timeline_geometry_and_workspace_shell_systems_resolve_arrows_and_docks() {
        let mut tg = TimelineGeometrySystem::default();
        let span = Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(200.0, 24.0));
        let (start, tip, tri) = tg.compute_tween_arrow_points(span);
        assert_eq!(start.x, 110.0); // 100 + 10.0 start_inset
        assert_eq!(tip.x, 296.0); // 300 - 4.0 end_inset
        assert_eq!(tri[0].x, 296.0);
        assert_eq!(tri[1].x, 289.0); // 296 - 7.0 head_length

        // Change arrow head length to 18px and wing to 6px
        tg.tween_arrow_head_length_px = 18.0;
        tg.tween_arrow_head_wing_px = 6.0;
        let (_start2, tip2, tri2) = tg.compute_tween_arrow_points(span);
        assert_eq!(tri2[1].x, tip2.x - 18.0);
        assert!((tri2[2].y - tri2[1].y - 12.0).abs() < 1e-4);

        // Layer column % vs px resolution
        assert_eq!(tg.resolve_layer_list_width(800.0), 220.0);
        tg.layer_list_width = UiSizeUnit::Percent(25.0);
        assert_eq!(tg.resolve_layer_list_width(800.0), 200.0);

        // WorkspaceShellSystem dock proportions in % and px
        let mut ws = WorkspaceShellSystem::default();
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let r1 = ws.resolve_shell_rects(viewport);
        assert_eq!(r1.left_tools.width(), 46.0);
        assert_eq!(r1.right_inspector.width(), 250.0); // 25% of 1000

        ws.shell_right_inspector_width = UiSizeUnit::Px(310.0);
        let r2 = ws.resolve_shell_rects(viewport);
        assert_eq!(r2.right_inspector.width(), 310.0);
    }

    #[test]
    fn studio_window_and_all_presets_render_in_both_themes() {
        let ctx = egui::Context::default();
        ctx.set_fonts(neoflash_ui::typography::font_definitions());

        for theme in [neoflash_ui::Theme::Dark, neoflash_ui::Theme::Light] {
            neoflash_ui::theme::install(&ctx, theme);
            let mut state = UiLayoutStudioState {
                open: true,
                ..Default::default()
            };
            for preset in StudioPanelPreset::ALL {
                state.load_preset(preset);
                for tab in [
                    StudioInspectorTab::NodeInspector,
                    StudioInspectorTab::SystemsTab,
                    StudioInspectorTab::CodeExport,
                ] {
                    state.inspector_tab = tab;
                    let input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(1920.0, 1080.0),
                        )),
                        ..Default::default()
                    };
                    let out = ctx.run(input, |ctx| {
                        ui_layout_studio_window_ui(ctx, &mut state);
                    });
                    assert!(
                        out.shapes.len() > 20,
                        "Preset {:?} tab {:?} painted too few shapes",
                        preset,
                        tab
                    );
                }
            }
        }
    }

    #[test]
    fn multi_panel_artboard_canvas_lays_out_and_resizes_without_clipping() {
        let mut state = UiLayoutStudioState::default();
        // Default board has 6 simultaneous core panels + timeline laid out on the canvas
        assert_eq!(state.artboards.len(), 6);
        assert_eq!(
            state.artboards[0].preset,
            StudioPanelPreset::PropertiesMovieClip
        );
        assert_eq!(
            state.artboards[1].preset,
            StudioPanelPreset::LibraryAndAssetsStudio
        );
        assert_eq!(
            state.artboards[2].preset,
            StudioPanelPreset::ColorAndGradientStudio
        );
        assert_eq!(
            state.artboards[3].preset,
            StudioPanelPreset::AlignAndTransform
        );
        assert_eq!(state.artboards[4].preset, StudioPanelPreset::ToolsPanel);
        assert_eq!(state.artboards[5].preset, StudioPanelPreset::TimelineStudio);

        // Verify top-row artboards do not overlap horizontally and timeline is below them
        for i in 0..4 {
            let right_edge = state.artboards[i].canvas_pos[0] + state.artboards[i].canvas_size[0];
            assert!(
                right_edge <= state.artboards[i + 1].canvas_pos[0],
                "Artboard {i} overlaps artboard {}",
                i + 1
            );
        }
        assert!(
            state.artboards[5].canvas_pos[1]
                >= state.artboards[0].canvas_pos[1] + state.artboards[0].canvas_size[1]
        );

        // Resize artboard 0 from 300px wide to 480px wide and verify % children scale proportionally
        let grid_id = state.artboards[0].document.root.children[1].children[0].id;
        let layout_300 = compute_studio_layout(
            &state.artboards[0].document,
            &state.systems,
            Pos2::ZERO,
            egui::vec2(
                state.artboards[0].canvas_size[0],
                state.artboards[0].canvas_size[1],
            ),
        );
        state.resize_artboard(0, 480.0, 520.0);
        assert_eq!(state.artboards[0].canvas_size, [480.0, 520.0]);
        let layout_480 = compute_studio_layout(
            &state.artboards[0].document,
            &state.systems,
            Pos2::ZERO,
            egui::vec2(
                state.artboards[0].canvas_size[0],
                state.artboards[0].canvas_size[1],
            ),
        );
        assert!(
            layout_480.node_box(grid_id).unwrap().resolved_width
                > layout_300.node_box(grid_id).unwrap().resolved_width
        );

        // Add, duplicate, auto-arrange, and remove artboards
        let new_idx = state.add_artboard(StudioPanelPreset::FiltersPanel);
        assert_eq!(state.artboards.len(), 7);
        assert_eq!(state.active_artboard_idx, new_idx);
        let dup_idx = state.duplicate_artboard(new_idx);
        assert_eq!(state.artboards.len(), 8);
        assert_eq!(state.active_artboard_idx, dup_idx);
        state.auto_arrange_artboards();
        state.remove_artboard(dup_idx);
        assert_eq!(state.artboards.len(), 7);
    }

    #[test]
    fn svg_export_and_import_roundtrip_and_raw_figma_svg_to_panel_conversion() {
        let mut state = UiLayoutStudioState::default();
        state.select_artboard(0); // Properties — Movie Clip

        // 1. Export active artboard to SVG and verify SVG structure & NF2 metadata
        let svg = state.export_active_svg();
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("data-nf-panel=\"Properties — Movie Clip\""));
        assert!(svg.to_ascii_lowercase().contains("data-nf-kind=\"field\""));
        assert!(svg.contains("POSITION AND SIZE"));

        // 2. Round-trip import of NeoFlash SVG reconstructs the editable panel
        let roundtrip_doc = import_svg_to_document(&svg).expect("round-trip SVG import succeeds");
        assert_eq!(roundtrip_doc.name, "Properties — Movie Clip");
        assert!(roundtrip_doc.all_node_ids().len() >= 10);

        // 3. Raw Figma / Illustrator SVG (without `data-nf-kind` attributes) converts into a structured NF2 panel!
        let raw_figma_svg = sample_svg_panel_snippet();
        assert!(!raw_figma_svg.contains("data-nf-kind"));
        let before_count = state.artboards.len();
        let imported_idx = state
            .import_svg_as_artboard(&raw_figma_svg)
            .expect("raw Figma SVG converts to NF2 panel artboard");
        assert_eq!(state.artboards.len(), before_count + 1);
        assert_eq!(state.active_artboard_idx, imported_idx);

        let imported_doc = &state.artboards[imported_idx].document;
        assert!(imported_doc.all_node_ids().len() >= 6);
        // Check that sections, rows/grids, fields, selects, or buttons were reconstructed
        let mut has_section = false;
        let mut has_field = false;
        let mut has_button_or_swatch = false;
        for id in imported_doc.all_node_ids() {
            if let Some(n) = imported_doc.find_node(id) {
                match n.kind {
                    UiNodeKind::Section { .. } => has_section = true,
                    UiNodeKind::Field { .. } | UiNodeKind::TextField { .. } => has_field = true,
                    UiNodeKind::Button { .. } | UiNodeKind::ColorSwatch { .. } => {
                        has_button_or_swatch = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(
            has_section,
            "Expected reconstructed Section header from raw SVG"
        );
        assert!(
            has_field,
            "Expected reconstructed numeric/text Field from raw SVG"
        );
        assert!(
            has_button_or_swatch,
            "Expected reconstructed Button or ColorSwatch from raw SVG"
        );

        // 4. Combined multi-artboard board SVG export works
        let board_svg = state.export_board_svg();
        assert!(board_svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(board_svg.contains("Properties — Movie Clip"));
        assert!(board_svg.contains("Align &amp; Transform"));
    }
}
