//! Uniform plain text and a bounded shared line-layout algorithm.
//! Hosts measure batches in their font engine; Rust owns wrapping, UTF-16 ranges,
//! alignment and line positions. This is not Flash's original text shaper.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod editing;
mod fonts_html;
mod format;
mod graphemes;
mod layout;
mod queries;
#[cfg(test)]
mod tests;

use self::fonts_html::parse_simple_glyf_outline;
pub use self::{editing::*, fonts_html::*, format::*, graphemes::*, layout::*, queries::*};

pub const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_PARTS: usize = 16_384;
const MAX_GLYPHS: usize = 65_536;
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("TextField: {0}")]
pub struct TextError(pub String);
fn error(message: &str) -> TextError {
    TextError(message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextBoxSize {
    pub width: f64,
    pub height: f64,
}
impl TextBoxSize {
    pub fn validate(&self) -> Result<(), TextError> {
        if ![self.width, self.height]
            .iter()
            .all(|v| v.is_finite() && *v >= 0. && *v <= 1e6)
        {
            return Err(error("box dimensions must be finite and in 0..=1000000"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFieldType {
    #[default]
    Dynamic,
    Input,
}
fn selectable_default() -> bool {
    true
}
fn anti_alias_default() -> String {
    "normal".into()
}
fn grid_fit_default() -> String {
    "pixel".into()
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSelection {
    pub begin: i64,
    pub end: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextRestrictPatch {
    pub value: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextInputEdit {
    pub expected_revision: String,
    pub begin: u32,
    pub end: u32,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct As2TextViewport {
    pub active: bool,
    pub background: bool,
    #[serde(rename = "backgroundColor")]
    pub background_color: u32,
    pub border: bool,
    #[serde(rename = "borderColor")]
    pub border_color: u32,
    pub scroll: u32,
    pub hscroll: u32,
    pub auto_size: String,
    pub mouse_wheel_enabled: bool,
}
impl Default for As2TextViewport {
    fn default() -> Self {
        Self {
            active: false,
            background: false,
            background_color: 0xffffff,
            border: false,
            border_color: 0,
            scroll: 1,
            hscroll: 0,
            auto_size: "none".into(),
            mouse_wheel_enabled: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextFieldState {
    pub text: String,
    #[serde(default)]
    pub as2_viewport: As2TextViewport,
    #[serde(rename = "box")]
    pub box_size: Option<TextBoxSize>,
    pub format: TextFormat,
    pub default_format: TextFormat,
    pub multiline: bool,
    pub word_wrap: bool,
    pub alpha: u8,
    pub revision: String,
    #[serde(default)]
    pub field_type: TextFieldType,
    #[serde(default = "selectable_default")]
    pub selectable: bool,
    #[serde(default)]
    pub display_as_password: bool,
    #[serde(default)]
    pub max_chars: u32,
    #[serde(default)]
    pub restrict: Option<String>,
    #[serde(default)]
    pub selection_begin: u32,
    #[serde(default)]
    pub selection_end: u32,
    #[serde(default)]
    pub caret: u32,
    #[serde(default = "anti_alias_default")]
    pub anti_alias_type: String,
    #[serde(default = "grid_fit_default")]
    pub grid_fit_type: String,
    #[serde(default)]
    pub sharpness: f64,
    #[serde(default)]
    pub thickness: f64,
    #[serde(default)]
    pub embed_fonts: bool,
    #[serde(default)]
    pub condense_white: bool,
    #[serde(default)]
    pub always_show_selection: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub html: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    #[serde(default = "default_one_u32", skip_serializing_if = "is_one_u32")]
    pub column_count: u32,
    #[serde(
        default = "default_gutter_f64",
        skip_serializing_if = "is_default_gutter"
    )]
    pub column_gutter: f64,
    #[serde(default = "default_top_str", skip_serializing_if = "is_top_str")]
    pub vertical_align: String,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_left: f64,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_right: f64,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_top: f64,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_bottom: f64,
    #[serde(default = "default_one_f64", skip_serializing_if = "is_one_f64")]
    pub border_width: f64,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub first_line_offset_mode: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub first_line_offset: f64,
    #[serde(default = "default_horiz_str", skip_serializing_if = "is_horiz_str")]
    pub orientation: String,
    #[serde(default = "default_ltr_str", skip_serializing_if = "is_ltr_str")]
    pub direction: String,
    #[serde(default = "default_pt_str", skip_serializing_if = "is_pt_str")]
    pub leading_mode: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub highlight: bool,
    #[serde(
        default = "default_highlight_yellow",
        skip_serializing_if = "is_highlight_yellow"
    )]
    pub highlight_color: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strikethrough: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superscript: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subscript: bool,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub glyph_rotation: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub typographic_case: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub digit_case: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub digit_width: String,
    #[serde(default = "default_common_str", skip_serializing_if = "is_common_str")]
    pub ligatures: String,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub line_break: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub baseline_shift: f64,
    #[serde(
        default = "default_locale_str",
        skip_serializing_if = "is_default_locale_str"
    )]
    pub locale: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub space_before: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub space_after: f64,
    #[serde(
        default = "default_word_spacing_str",
        skip_serializing_if = "is_word_spacing_str"
    )]
    pub text_justify: String,
    #[serde(
        default = "default_normal_interaction_str",
        skip_serializing_if = "is_normal_interaction_str"
    )]
    pub interaction_mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub format_runs: Vec<TextFormatRun>,
}
impl Default for TextFieldState {
    fn default() -> Self {
        Self {
            text: String::new(),
            as2_viewport: As2TextViewport::default(),
            box_size: Some(TextBoxSize {
                width: 100.,
                height: 100.,
            }),
            format: TextFormat::default(),
            default_format: TextFormat::default(),
            multiline: false,
            word_wrap: false,
            alpha: 255,
            revision: "1".into(),
            field_type: TextFieldType::Dynamic,
            selectable: true,
            display_as_password: false,
            max_chars: 0,
            restrict: None,
            selection_begin: 0,
            selection_end: 0,
            caret: 0,
            anti_alias_type: anti_alias_default(),
            grid_fit_type: grid_fit_default(),
            sharpness: 0.,
            thickness: 0.,
            embed_fonts: false,
            condense_white: false,
            always_show_selection: false,
            html: false,
            variable: None,
            column_count: 1,
            column_gutter: 20.0,
            vertical_align: "top".into(),
            padding_left: 2.0,
            padding_right: 2.0,
            padding_top: 2.0,
            padding_bottom: 2.0,
            border_width: 1.0,
            first_line_offset_mode: "auto".into(),
            first_line_offset: 0.0,
            orientation: "horizontal".into(),
            direction: "ltr".into(),
            leading_mode: "pt".into(),
            highlight: false,
            highlight_color: 0xffff00,
            strikethrough: false,
            superscript: false,
            subscript: false,
            glyph_rotation: "auto".into(),
            typographic_case: "default".into(),
            digit_case: "default".into(),
            digit_width: "default".into(),
            ligatures: "common".into(),
            line_break: "auto".into(),
            baseline_shift: 0.0,
            locale: "en_US".into(),
            space_before: 0.0,
            space_after: 0.0,
            text_justify: "word_spacing".into(),
            interaction_mode: "normal".into(),
            format_runs: Vec::new(),
        }
    }
}

fn default_normal_interaction_str() -> String {
    "normal".into()
}

fn is_normal_interaction_str(v: &str) -> bool {
    v.is_empty() || v == "normal"
}

fn default_true_bool() -> bool {
    true
}

fn is_true_bool(v: &bool) -> bool {
    *v
}

fn is_zero_u32_text(v: &u32) -> bool {
    *v == 0
}

fn default_one_u32() -> u32 {
    1
}

fn is_one_u32(v: &u32) -> bool {
    *v <= 1
}

fn default_gutter_f64() -> f64 {
    20.0
}

fn is_default_gutter(v: &f64) -> bool {
    (*v - 20.0).abs() < 1e-6
}

fn default_pad_f64() -> f64 {
    2.0
}

fn is_default_pad(v: &f64) -> bool {
    (*v - 2.0).abs() < 1e-6
}

fn default_one_f64() -> f64 {
    1.0
}

fn is_one_f64(v: &f64) -> bool {
    (*v - 1.0).abs() < 1e-6
}

fn is_zero_f64(v: &f64) -> bool {
    v.abs() < 1e-6
}

fn default_top_str() -> String {
    "top".into()
}

fn is_top_str(s: &str) -> bool {
    s.is_empty() || s == "top"
}

fn default_auto_str() -> String {
    "auto".into()
}

fn is_auto_str(s: &str) -> bool {
    s.is_empty() || s == "auto"
}

fn default_horiz_str() -> String {
    "horizontal".into()
}

fn is_horiz_str(s: &str) -> bool {
    s.is_empty() || s == "horizontal"
}

fn default_ltr_str() -> String {
    "ltr".into()
}

fn is_ltr_str(s: &str) -> bool {
    s.is_empty() || s == "ltr"
}

fn default_pt_str() -> String {
    "pt".into()
}

fn is_pt_str(s: &str) -> bool {
    s.is_empty() || s == "pt"
}

fn default_default_str() -> String {
    "default".into()
}

fn is_default_str(s: &str) -> bool {
    s.is_empty() || s == "default"
}

fn default_common_str() -> String {
    "common".into()
}

fn is_common_str(s: &str) -> bool {
    s.is_empty() || s == "common"
}

fn default_locale_str() -> String {
    "en_US".into()
}

fn is_default_locale_str(s: &str) -> bool {
    s.is_empty() || s == "en_US"
}

fn default_word_spacing_str() -> String {
    "word_spacing".into()
}

fn is_word_spacing_str(s: &str) -> bool {
    s.is_empty() || s == "word_spacing"
}

fn is_normal_anti_alias(s: &str) -> bool {
    s.is_empty() || s == "normal"
}

fn is_pixel_grid_fit(s: &str) -> bool {
    s.is_empty() || s == "pixel"
}

fn default_highlight_yellow() -> u32 {
    0xffff00
}

fn is_highlight_yellow(v: &u32) -> bool {
    *v == 0xffff00
}

fn default_none_auto_size() -> String {
    "none".into()
}

fn is_none_auto_size(s: &str) -> bool {
    s == "none" || s.is_empty()
}

/// Serializable authored rich-text and TextField behavior options attached to a `TextBox` or `Text` element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthoredTextFieldOptions {
    #[serde(default)]
    pub field_type: TextFieldType,
    #[serde(default = "default_true_bool", skip_serializing_if = "is_true_bool")]
    pub selectable: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub display_as_password: bool,
    #[serde(default, skip_serializing_if = "is_zero_u32_text")]
    pub max_chars: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restrict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    #[serde(
        default = "default_none_auto_size",
        skip_serializing_if = "is_none_auto_size"
    )]
    pub auto_size: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub border: bool,
    #[serde(default, skip_serializing_if = "is_zero_u32_text")]
    pub border_color: u32,
    #[serde(default = "default_one_f64", skip_serializing_if = "is_one_f64")]
    pub border_width: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub background: bool,
    #[serde(default = "default_bg_white", skip_serializing_if = "is_bg_white")]
    pub background_color: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub html: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub embed_fonts: bool,
    #[serde(default = "default_one_u32", skip_serializing_if = "is_one_u32")]
    pub column_count: u32,
    #[serde(
        default = "default_gutter_f64",
        skip_serializing_if = "is_default_gutter"
    )]
    pub column_gutter: f64,
    #[serde(default = "default_top_str", skip_serializing_if = "is_top_str")]
    pub vertical_align: String,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_left: f64,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_right: f64,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_top: f64,
    #[serde(default = "default_pad_f64", skip_serializing_if = "is_default_pad")]
    pub padding_bottom: f64,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub first_line_offset_mode: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub first_line_offset: f64,
    #[serde(default = "default_horiz_str", skip_serializing_if = "is_horiz_str")]
    pub orientation: String,
    #[serde(default = "default_ltr_str", skip_serializing_if = "is_ltr_str")]
    pub direction: String,
    #[serde(default = "default_pt_str", skip_serializing_if = "is_pt_str")]
    pub leading_mode: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub highlight: bool,
    #[serde(
        default = "default_highlight_yellow",
        skip_serializing_if = "is_highlight_yellow"
    )]
    pub highlight_color: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strikethrough: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superscript: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subscript: bool,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub glyph_rotation: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub typographic_case: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub digit_case: String,
    #[serde(
        default = "default_default_str",
        skip_serializing_if = "is_default_str"
    )]
    pub digit_width: String,
    #[serde(default = "default_common_str", skip_serializing_if = "is_common_str")]
    pub ligatures: String,
    #[serde(default = "default_auto_str", skip_serializing_if = "is_auto_str")]
    pub line_break: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub baseline_shift: f64,
    #[serde(
        default = "default_locale_str",
        skip_serializing_if = "is_default_locale_str"
    )]
    pub locale: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub space_before: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub space_after: f64,
    #[serde(
        default = "default_word_spacing_str",
        skip_serializing_if = "is_word_spacing_str"
    )]
    pub text_justify: String,
    #[serde(
        default = "anti_alias_default",
        skip_serializing_if = "is_normal_anti_alias"
    )]
    pub anti_alias_type: String,
    #[serde(
        default = "grid_fit_default",
        skip_serializing_if = "is_pixel_grid_fit"
    )]
    pub grid_fit_type: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub sharpness: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub thickness: f64,
    #[serde(
        default = "default_normal_interaction_str",
        skip_serializing_if = "is_normal_interaction_str"
    )]
    pub interaction_mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub format_runs: Vec<TextFormatRun>,
}

fn default_bg_white() -> u32 {
    0xffffff
}

fn is_bg_white(v: &u32) -> bool {
    *v == 0xffffff
}

impl Default for AuthoredTextFieldOptions {
    fn default() -> Self {
        Self {
            field_type: TextFieldType::Dynamic,
            selectable: true,
            display_as_password: false,
            max_chars: 0,
            restrict: None,
            variable: None,
            auto_size: "none".into(),
            border: false,
            border_color: 0,
            border_width: 1.0,
            background: false,
            background_color: 0xffffff,
            html: false,
            embed_fonts: false,
            column_count: 1,
            column_gutter: 20.0,
            vertical_align: "top".into(),
            padding_left: 2.0,
            padding_right: 2.0,
            padding_top: 2.0,
            padding_bottom: 2.0,
            first_line_offset_mode: "auto".into(),
            first_line_offset: 0.0,
            orientation: "horizontal".into(),
            direction: "ltr".into(),
            leading_mode: "pt".into(),
            highlight: false,
            highlight_color: 0xffff00,
            strikethrough: false,
            superscript: false,
            subscript: false,
            glyph_rotation: "auto".into(),
            typographic_case: "default".into(),
            digit_case: "default".into(),
            digit_width: "default".into(),
            ligatures: "common".into(),
            line_break: "auto".into(),
            baseline_shift: 0.0,
            locale: "en_US".into(),
            space_before: 0.0,
            space_after: 0.0,
            text_justify: "word_spacing".into(),
            anti_alias_type: anti_alias_default(),
            grid_fit_type: grid_fit_default(),
            sharpness: 0.0,
            thickness: 0.0,
            interaction_mode: "normal".into(),
            format_runs: Vec::new(),
        }
    }
}

impl AuthoredTextFieldOptions {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn apply_to_state(&self, state: &mut TextFieldState) {
        state.field_type = self.field_type;
        state.selectable = self.selectable;
        state.display_as_password = self.display_as_password;
        state.max_chars = self.max_chars;
        state.restrict = self.restrict.clone();
        state.as2_viewport.auto_size = if self.auto_size.is_empty() {
            "none".into()
        } else {
            self.auto_size.clone()
        };
        state.as2_viewport.border = self.border;
        state.as2_viewport.border_color = self.border_color & 0xffffff;
        state.border_width = self.border_width.clamp(0.5, 200.0);
        state.as2_viewport.background = self.background;
        state.as2_viewport.background_color = self.background_color & 0xffffff;
        state.as2_viewport.active = self.border
            || self.background
            || !matches!(state.as2_viewport.auto_size.as_str(), "" | "none");
        state.embed_fonts = self.embed_fonts;
        state.html = self.html;
        state.variable = self.variable.clone().filter(|s| !s.trim().is_empty());
        state.column_count = self.column_count.clamp(1, 50);
        state.column_gutter = self.column_gutter.clamp(0.0, 1000.0);
        state.vertical_align = if self.vertical_align.is_empty() {
            "top".into()
        } else {
            self.vertical_align.clone()
        };
        state.padding_left = self.padding_left.clamp(0.0, 1000.0);
        state.padding_right = self.padding_right.clamp(0.0, 1000.0);
        state.padding_top = self.padding_top.clamp(0.0, 1000.0);
        state.padding_bottom = self.padding_bottom.clamp(0.0, 1000.0);
        state.first_line_offset_mode = if self.first_line_offset_mode.is_empty() {
            "auto".into()
        } else {
            self.first_line_offset_mode.clone()
        };
        state.first_line_offset = self.first_line_offset.clamp(0.0, 1000.0);
        state.orientation = if self.orientation.is_empty() {
            "horizontal".into()
        } else {
            self.orientation.clone()
        };
        state.direction = if self.direction.is_empty() {
            "ltr".into()
        } else {
            self.direction.clone()
        };
        state.leading_mode = if self.leading_mode.is_empty() {
            "pt".into()
        } else {
            self.leading_mode.clone()
        };
        state.highlight = self.highlight;
        state.highlight_color = self.highlight_color & 0xffffff;
        state.strikethrough = self.strikethrough;
        state.superscript = self.superscript;
        state.subscript = self.subscript && !self.superscript;
        state.glyph_rotation = if self.glyph_rotation.is_empty() {
            "auto".into()
        } else {
            self.glyph_rotation.clone()
        };
        state.typographic_case = if self.typographic_case.is_empty() {
            "default".into()
        } else {
            self.typographic_case.clone()
        };
        state.digit_case = if self.digit_case.is_empty() {
            "default".into()
        } else {
            self.digit_case.clone()
        };
        state.digit_width = if self.digit_width.is_empty() {
            "default".into()
        } else {
            self.digit_width.clone()
        };
        state.ligatures = if self.ligatures.is_empty() {
            "common".into()
        } else {
            self.ligatures.clone()
        };
        state.line_break = if self.line_break.is_empty() {
            "auto".into()
        } else {
            self.line_break.clone()
        };
        state.baseline_shift = self.baseline_shift.clamp(-720.0, 720.0);
        state.locale = if self.locale.is_empty() {
            "en_US".into()
        } else {
            self.locale.clone()
        };
        state.space_before = self.space_before.clamp(0.0, 1000.0);
        state.space_after = self.space_after.clamp(0.0, 1000.0);
        state.text_justify = if self.text_justify.is_empty() {
            "word_spacing".into()
        } else {
            self.text_justify.clone()
        };
        if !self.anti_alias_type.is_empty() {
            state.anti_alias_type = self.anti_alias_type.clone();
        }
        if !self.grid_fit_type.is_empty() {
            state.grid_fit_type = self.grid_fit_type.clone();
        }
        state.sharpness = self.sharpness.clamp(-400.0, 400.0);
        state.thickness = self.thickness.clamp(-200.0, 200.0);
        state.interaction_mode = if self.interaction_mode.is_empty() {
            "normal".into()
        } else {
            self.interaction_mode.clone()
        };
        state.format_runs = self.format_runs.clone();
        state.normalize_runs();
    }

    pub fn from_state(state: &TextFieldState, variable: Option<String>, html: bool) -> Self {
        Self {
            field_type: state.field_type,
            selectable: state.selectable,
            display_as_password: state.display_as_password,
            max_chars: state.max_chars,
            restrict: state.restrict.clone(),
            variable: variable
                .or_else(|| state.variable.clone())
                .filter(|s| !s.trim().is_empty()),
            auto_size: state.as2_viewport.auto_size.clone(),
            border: state.as2_viewport.border,
            border_color: state.as2_viewport.border_color & 0xffffff,
            border_width: state.border_width,
            background: state.as2_viewport.background,
            background_color: state.as2_viewport.background_color & 0xffffff,
            html: html || state.html,
            embed_fonts: state.embed_fonts,
            column_count: state.column_count.max(1),
            column_gutter: state.column_gutter,
            vertical_align: state.vertical_align.clone(),
            padding_left: state.padding_left,
            padding_right: state.padding_right,
            padding_top: state.padding_top,
            padding_bottom: state.padding_bottom,
            first_line_offset_mode: state.first_line_offset_mode.clone(),
            first_line_offset: state.first_line_offset,
            orientation: state.orientation.clone(),
            direction: state.direction.clone(),
            leading_mode: state.leading_mode.clone(),
            highlight: state.highlight,
            highlight_color: state.highlight_color & 0xffffff,
            strikethrough: state.strikethrough,
            superscript: state.superscript,
            subscript: state.subscript,
            glyph_rotation: state.glyph_rotation.clone(),
            typographic_case: state.typographic_case.clone(),
            digit_case: state.digit_case.clone(),
            digit_width: state.digit_width.clone(),
            ligatures: state.ligatures.clone(),
            line_break: state.line_break.clone(),
            baseline_shift: state.baseline_shift,
            locale: state.locale.clone(),
            space_before: state.space_before,
            space_after: state.space_after,
            text_justify: state.text_justify.clone(),
            anti_alias_type: state.anti_alias_type.clone(),
            grid_fit_type: state.grid_fit_type.clone(),
            sharpness: state.sharpness,
            thickness: state.thickness,
            interaction_mode: state.interaction_mode.clone(),
            format_runs: state.format_runs.clone(),
        }
    }
}

pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

include!("fonts.rs");
