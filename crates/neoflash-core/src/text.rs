//! Uniform plain text and a bounded shared line-layout algorithm.
//! Hosts measure batches in their font engine; Rust owns wrapping, UTF-16 ranges,
//! alignment and line positions. This is not Flash's original text shaper.
use crate::ElementKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
pub const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_PARTS: usize = 16_384;
const MAX_GLYPHS: usize = 65_536;
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("TextField: {0}")]
pub struct TextError(pub String);
fn error(message: &str) -> TextError {
    TextError(message.into())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "TextFormatWire", into = "TextFormatWire")]
pub struct TextFormat {
    pub font: String,
    pub size: f64,
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
    pub leading: f64,
    pub align: TextAlign,
    pub left_margin: f64,
    pub right_margin: f64,
    pub indent: f64,
    pub block_indent: f64,
    pub underline: bool,
    pub letter_spacing: f64,
    pub kerning: Option<bool>,
    pub bullet: bool,
    pub tab_stops: Vec<f64>,
    pub url: String,
    pub target: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TextFormatWire {
    font: String,
    size: f64,
    color: u32,
    bold: bool,
    italic: bool,
    leading: f64,
    align: TextAlign,
    #[serde(rename = "leftMargin")]
    left_margin: f64,
    #[serde(rename = "rightMargin")]
    right_margin: f64,
    indent: f64,
    #[serde(rename = "blockIndent")]
    block_indent: f64,
    underline: bool,
    #[serde(rename = "letterSpacing")]
    letter_spacing: f64,
    kerning: Option<bool>,
    bullet: bool,
    #[serde(rename = "tabStops")]
    tab_stops: Vec<f64>,
    url: String,
    target: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "alignMode")]
    align_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "fieldOptions"
    )]
    field_options: Option<AuthoredTextFieldOptions>,
}
impl Default for TextFormatWire {
    fn default() -> Self {
        Self::from(TextFormat::default())
    }
}
impl From<TextFormatWire> for TextFormat {
    fn from(wire: TextFormatWire) -> Self {
        let mut fmt = Self {
            font: wire.font,
            size: wire.size,
            color: wire.color,
            bold: wire.bold,
            italic: wire.italic,
            leading: wire.leading,
            align: wire.align,
            left_margin: wire.left_margin,
            right_margin: wire.right_margin,
            indent: wire.indent,
            block_indent: wire.block_indent,
            underline: wire.underline,
            letter_spacing: wire.letter_spacing,
            kerning: wire.kerning,
            bullet: wire.bullet,
            tab_stops: wire.tab_stops,
            url: wire.url,
            target: wire.target,
        };
        fmt.set_align_mode(wire.align_mode);
        fmt.set_display(wire.display);
        fmt.set_field_options(wire.field_options);
        fmt
    }
}
impl From<TextFormat> for TextFormatWire {
    fn from(fmt: TextFormat) -> Self {
        let target = fmt.target_str().to_owned();
        let align_mode = fmt.align_mode().map(str::to_owned);
        let display = fmt.display().map(str::to_owned);
        let field_options = fmt.field_options().filter(|o| !o.is_default());
        Self {
            font: fmt.font,
            size: fmt.size,
            color: fmt.color,
            bold: fmt.bold,
            italic: fmt.italic,
            leading: fmt.leading,
            align: fmt.align,
            left_margin: fmt.left_margin,
            right_margin: fmt.right_margin,
            indent: fmt.indent,
            block_indent: fmt.block_indent,
            underline: fmt.underline,
            letter_spacing: fmt.letter_spacing,
            kerning: fmt.kerning,
            bullet: fmt.bullet,
            tab_stops: fmt.tab_stops,
            url: fmt.url,
            target,
            align_mode,
            display,
            field_options,
        }
    }
}
impl Default for TextFormat {
    fn default() -> Self {
        Self {
            font: "Times New Roman".into(),
            size: 12.,
            color: 0,
            bold: false,
            italic: false,
            leading: 0.,
            align: TextAlign::Left,
            left_margin: 0.,
            right_margin: 0.,
            indent: 0.,
            block_indent: 0.,
            underline: false,
            letter_spacing: 0.,
            kerning: None,
            bullet: false,
            tab_stops: Vec::new(),
            url: String::new(),
            target: String::new(),
        }
    }
}
impl TextFormat {
    pub fn target_str(&self) -> &str {
        self.target
            .split_once("\0\x1f")
            .map_or(self.target.as_str(), |(base, _)| base)
    }
    pub fn align_mode(&self) -> Option<&str> {
        let (_, meta) = self.target.split_once("\0\x1f")?;
        let align_part = meta.split('\x1f').next()?;
        (!align_part.is_empty()).then_some(align_part)
    }
    pub fn display(&self) -> Option<&str> {
        let (_, meta) = self.target.split_once("\0\x1f")?;
        let display_part = meta.split('\x1f').nth(1)?;
        (!display_part.is_empty()).then_some(display_part)
    }
    pub fn field_options(&self) -> Option<AuthoredTextFieldOptions> {
        let (_, meta) = self.target.split_once("\0\x1f")?;
        let opts_part = meta.split('\x1f').nth(2)?;
        if opts_part.is_empty() {
            return None;
        }
        serde_json::from_str::<AuthoredTextFieldOptions>(opts_part)
            .ok()
            .filter(|o| !o.is_default())
    }
    fn write_target_meta(
        &mut self,
        base: String,
        align_mode: Option<String>,
        display: Option<String>,
        field_options: Option<AuthoredTextFieldOptions>,
    ) {
        let a = align_mode.as_deref().unwrap_or("");
        let d = display.as_deref().unwrap_or("");
        let o = field_options
            .filter(|opts| !opts.is_default())
            .and_then(|opts| serde_json::to_string(&opts).ok())
            .unwrap_or_default();
        if a.is_empty() && d.is_empty() && o.is_empty() {
            self.target = base;
        } else if o.is_empty() {
            self.target = format!("{base}\0\x1f{a}\x1f{d}");
        } else {
            self.target = format!("{base}\0\x1f{a}\x1f{d}\x1f{o}");
        }
    }
    pub fn set_target_str(&mut self, base: String) {
        let a = self.align_mode().map(str::to_owned);
        let d = self.display().map(str::to_owned);
        let o = self.field_options();
        self.write_target_meta(base, a, d, o);
    }
    pub fn set_align_mode(&mut self, mode: Option<String>) {
        let base = self.target_str().to_owned();
        let d = self.display().map(str::to_owned);
        let o = self.field_options();
        self.write_target_meta(base, mode, d, o);
    }
    pub fn set_display(&mut self, display: Option<String>) {
        let base = self.target_str().to_owned();
        let a = self.align_mode().map(str::to_owned);
        let o = self.field_options();
        self.write_target_meta(base, a, display, o);
    }
    pub fn set_field_options(&mut self, options: Option<AuthoredTextFieldOptions>) {
        let base = self.target_str().to_owned();
        let a = self.align_mode().map(str::to_owned);
        let d = self.display().map(str::to_owned);
        self.write_target_meta(base, a, d, options);
    }
    pub fn effective_align_mode(&self) -> &str {
        if let Some(mode) = self.align_mode() {
            mode
        } else {
            match self.align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            }
        }
    }
    pub fn validate(&self) -> Result<(), TextError> {
        if let Some(mode) = self.align_mode()
            && !matches!(
                mode,
                "left" | "center" | "right" | "justify" | "justify_all" | "start" | "end"
            )
        {
            return Err(error(
                "alignMode requires left, center, right, justify, justify_all, start or end",
            ));
        }
        if let Some(display) = self.display()
            && !matches!(display, "inline" | "block" | "none")
        {
            return Err(error("TextFormat.display requires inline, block or none"));
        }
        if self.font.is_empty() || self.font.len() > 1024 || self.font.chars().any(char::is_control)
        {
            return Err(error(
                "font must be a nonempty family name of at most 1024 UTF-8 bytes",
            ));
        }
        if !self.size.is_finite()
            || self.size <= 0.
            || self.size > 10000.
            || !self.leading.is_finite()
            || self.leading.abs() > 10000.
            || self.size * 1.2 + self.leading <= 0.
        {
            return Err(error(
                "size must be in (0,10000], leading in [-10000,10000], and line advance positive",
            ));
        }
        if ![self.left_margin, self.right_margin, self.block_indent]
            .iter()
            .all(|value| value.is_finite() && (0. ..=10000.).contains(value))
            || !self.indent.is_finite()
            || self.indent.abs() > 10000.
        {
            return Err(error(
                "paragraph margins/blockIndent require 0..=10000; indent requires -10000..=10000",
            ));
        }
        if !self.letter_spacing.is_finite() || self.letter_spacing.abs() > 1000. {
            return Err(error("letterSpacing requires -1000..=1000"));
        }
        if self.tab_stops.len() > 64
            || self
                .tab_stops
                .iter()
                .any(|v| !v.is_finite() || *v < 0. || *v > 100000.)
            || self.tab_stops.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(error(
                "tabStops require at most64 increasing nonnegative lengths <=100000",
            ));
        }
        let base_target = self.target_str();
        if self.url.len() > 4096
            || base_target.len() > 256
            || self
                .url
                .chars()
                .chain(base_target.chars())
                .any(char::is_control)
        {
            return Err(error(
                "TextFormat url/target require bounded strings without controls",
            ));
        }
        if self.color > 0xffffff {
            return Err(error("color must be a 24-bit RGB integer"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextFormatPatch {
    pub font: Option<String>,
    pub size: Option<f64>,
    pub color: Option<u32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub leading: Option<f64>,
    pub align: Option<TextAlign>,
    #[serde(rename = "leftMargin")]
    pub left_margin: Option<f64>,
    #[serde(rename = "rightMargin")]
    pub right_margin: Option<f64>,
    pub indent: Option<f64>,
    #[serde(rename = "blockIndent")]
    pub block_indent: Option<f64>,
    pub underline: Option<bool>,
    #[serde(rename = "letterSpacing")]
    pub letter_spacing: Option<f64>,
    pub kerning: Option<bool>,
    pub bullet: Option<bool>,
    #[serde(rename = "tabStops")]
    pub tab_stops: Option<Vec<f64>>,
    pub url: Option<String>,
    pub target: Option<String>,
    #[serde(default, rename = "alignMode")]
    pub align_mode: Option<String>,
    #[serde(default)]
    pub display: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextFormatRun {
    pub start: u32,
    pub end: u32,
    pub format: TextFormat,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextFormatRangePatch {
    pub begin: u32,
    pub end: u32,
    pub format: TextFormatPatch,
}
impl TextFormatPatch {
    pub(crate) fn apply(&self, format: &mut TextFormat) {
        if let Some(value) = &self.font {
            format.font = value.clone()
        }
        if let Some(value) = self.size {
            format.size = value
        }
        if let Some(value) = self.color {
            format.color = value
        }
        if let Some(value) = self.bold {
            format.bold = value
        }
        if let Some(value) = self.italic {
            format.italic = value
        }
        if let Some(value) = self.leading {
            format.leading = value
        }
        if let Some(value) = self.align {
            format.align = value;
            format.set_align_mode(None);
        }
        if let Some(value) = &self.align_mode {
            match value.as_str() {
                "left" => {
                    format.align = TextAlign::Left;
                    format.set_align_mode(None);
                }
                "center" => {
                    format.align = TextAlign::Center;
                    format.set_align_mode(None);
                }
                "right" => {
                    format.align = TextAlign::Right;
                    format.set_align_mode(None);
                }
                "justify" | "justify_all" | "start" | "end" => {
                    format.align = if value == "end" {
                        TextAlign::Right
                    } else {
                        TextAlign::Left
                    };
                    format.set_align_mode(Some(value.clone()));
                }
                _ => {
                    format.set_align_mode(Some(value.clone()));
                }
            }
        }
        if let Some(value) = &self.display {
            format.set_display(Some(value.clone()));
        }
        if let Some(value) = self.left_margin {
            format.left_margin = value
        }
        if let Some(value) = self.right_margin {
            format.right_margin = value
        }
        if let Some(value) = self.indent {
            format.indent = value
        }
        if let Some(value) = self.block_indent {
            format.block_indent = value
        }
        if let Some(value) = self.letter_spacing {
            format.letter_spacing = value;
        }
        if let Some(value) = self.kerning {
            format.kerning = Some(value);
        }
        if let Some(value) = self.bullet {
            format.bullet = value;
        }
        if let Some(value) = &self.tab_stops {
            format.tab_stops = value.clone();
        }
        if let Some(value) = &self.url {
            format.url = value.clone();
        }
        if let Some(value) = &self.target {
            format.set_target_str(value.clone());
        }
        if let Some(value) = self.underline {
            format.underline = value
        }
    }
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
            format_runs: Vec::new(),
        }
    }
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
            format_runs: state.format_runs.clone(),
        }
    }
}

pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
impl TextFieldState {
    pub fn from_kind(kind: &ElementKind) -> Option<Self> {
        match kind {
            ElementKind::Text {
                text,
                font_family,
                font_size,
                color,
            } => {
                let format = TextFormat {
                    font: font_family.clone(),
                    size: *font_size,
                    color: u32::from(color[0]) << 16
                        | u32::from(color[1]) << 8
                        | u32::from(color[2]),
                    ..TextFormat::default()
                };
                Some(Self {
                    text: normalize_text(text),
                    box_size: None,
                    default_format: format.clone(),
                    format,
                    multiline: true,
                    word_wrap: false,
                    alpha: color[3],
                    revision: "1".into(),
                    ..Self::default()
                })
            }
            ElementKind::TextBox {
                text,
                width,
                height,
                format,
                word_wrap,
                multiline,
            } => {
                let mut clean_format = format.clone();
                let opts = clean_format.field_options();
                clean_format.set_field_options(None);
                let mut state = Self {
                    text: normalize_text(text),
                    box_size: Some(TextBoxSize {
                        width: *width,
                        height: *height,
                    }),
                    format: clean_format.clone(),
                    default_format: clean_format,
                    multiline: *multiline,
                    word_wrap: *word_wrap,
                    alpha: 255,
                    revision: "1".into(),
                    ..Self::default()
                };
                if let Some(opts) = opts {
                    opts.apply_to_state(&mut state);
                }
                Some(state)
            }
            _ => None,
        }
    }

    pub fn write_to_kind(&self, kind: &mut ElementKind) {
        if let ElementKind::TextBox {
            text,
            width,
            height,
            format,
            word_wrap,
            multiline,
        } = kind
        {
            let prev_opts = format.field_options();
            let prev_var = prev_opts.as_ref().and_then(|o| o.variable.clone());
            let prev_html = prev_opts.as_ref().is_some_and(|o| o.html);
            *text = self.text.clone();
            if let Some(size) = self.box_size {
                *width = size.width;
                *height = size.height;
            }
            *word_wrap = self.word_wrap;
            *multiline = self.multiline;
            let mut next_fmt = self.format.clone();
            let opts = AuthoredTextFieldOptions::from_state(self, prev_var, prev_html);
            next_fmt.set_field_options((!opts.is_default()).then_some(opts));
            *format = next_fmt;
        }
    }
    pub(crate) fn storage_bytes(&self) -> usize {
        self.text.len() + self.restrict.as_ref().map_or(0, String::len)
    }
    pub fn validate(&self) -> Result<(), TextError> {
        if !matches!(
            self.as2_viewport.auto_size.as_str(),
            "none" | "left" | "center" | "right"
        ) {
            return Err(error("autoSize requires none, left, center or right"));
        }
        if !matches!(self.anti_alias_type.as_str(), "normal" | "advanced") {
            return Err(error("antiAliasType requires normal or advanced"));
        }
        if !matches!(self.grid_fit_type.as_str(), "none" | "pixel" | "subpixel") {
            return Err(error("gridFitType requires none, pixel or subpixel"));
        }
        if !self.sharpness.is_finite() || !(-400.0..=400.0).contains(&self.sharpness) {
            return Err(error("sharpness requires a finite number in -400..=400"));
        }
        if !self.thickness.is_finite() || !(-200.0..=200.0).contains(&self.thickness) {
            return Err(error("thickness requires a finite number in -200..=200"));
        }
        if self.as2_viewport.background_color > 0xffffff
            || self.as2_viewport.border_color > 0xffffff
            || self.as2_viewport.scroll == 0
            || self.as2_viewport.scroll > 1_000_000
            || self.as2_viewport.hscroll > 1_000_000
        {
            return Err(error(
                "AS2 viewport colors require RGB; scroll requires 1..=1000000; hscroll requires 0..=1000000",
            ));
        }
        if self.text.len() > MAX_TEXT_BYTES {
            return Err(error("text exceeds 1048576 UTF-8 bytes"));
        }
        if self.max_chars > MAX_TEXT_BYTES as u32 {
            return Err(error("maxChars must be in 0..=1048576 UTF-16 units"));
        }
        if let Some(pattern) = &self.restrict {
            Restriction::compile(pattern)?;
        }
        if self.selection_begin > self.selection_end
            || (self.caret != self.selection_begin && self.caret != self.selection_end)
        {
            return Err(error("invalid selection endpoints/caret"));
        }
        utf16_byte(&self.text, self.selection_begin)?;
        utf16_byte(&self.text, self.selection_end)?;
        if self.field_type == TextFieldType::Input && self.format.bullet {
            return Err(error("bullet currently requires a non-input TextField"));
        }
        self.format.validate()?;
        self.default_format.validate()?;
        if !self.format_runs.is_empty() {
            let total_utf16 = self.text.encode_utf16().count() as u32;
            let mut cursor = 0u32;
            for run in &self.format_runs {
                if run.start != cursor || run.end <= run.start || run.end > total_utf16 {
                    return Err(error("format_runs must partition the UTF-16 text range"));
                }
                utf16_byte(&self.text, run.start)?;
                utf16_byte(&self.text, run.end)?;
                run.format.validate()?;
                cursor = run.end;
            }
            if cursor != total_utf16 {
                return Err(error("format_runs must cover the entire UTF-16 text range"));
            }
        }
        if let Some(size) = self.box_size {
            size.validate()?
        }
        if self.revision.parse::<u64>().is_err() {
            return Err(error("invalid text revision"));
        }
        Ok(())
    }
    pub fn apply(&mut self, patch: &TextPatch) -> Result<(), TextError> {
        if usize::from(patch.text.is_some())
            + usize::from(patch.append.is_some())
            + usize::from(patch.replace.is_some())
            + usize::from(patch.replace_selected.is_some())
            > 1
        {
            return Err(error(
                "a text update must contain at most one replacement or append operation",
            ));
        }
        let selected_replacement = patch.replace_selected.as_ref().map(|text| TextReplacement {
            begin: self.selection_begin,
            end: self.selection_end,
            text: text.clone(),
        });
        let mut next = self.clone();
        if let Some(viewport) = &patch.as2_viewport {
            next.as2_viewport = viewport.clone();
        }
        if let Some(value) = patch.field_type {
            next.field_type = value;
        }
        if let Some(value) = patch.selectable {
            next.selectable = value;
        }
        if let Some(value) = patch.display_as_password {
            next.display_as_password = value;
        }
        if let Some(value) = patch.max_chars {
            next.max_chars = value;
        }
        if let Some(value) = &patch.restrict {
            next.restrict = value.value.clone();
        }
        if let Some(value) = &patch.anti_alias_type {
            next.anti_alias_type = value.clone();
        }
        if let Some(value) = &patch.grid_fit_type {
            next.grid_fit_type = value.clone();
        }
        if let Some(value) = patch.sharpness {
            if !value.is_finite() {
                return Err(error("sharpness requires a finite number"));
            }
            next.sharpness = value.clamp(-400.0, 400.0);
        }
        if let Some(value) = patch.thickness {
            if !value.is_finite() {
                return Err(error("thickness requires a finite number"));
            }
            next.thickness = value.clamp(-200.0, 200.0);
        }
        if let Some(value) = patch.embed_fonts {
            next.embed_fonts = value;
        }
        if let Some(value) = patch.condense_white {
            next.condense_white = value;
        }
        if let Some(value) = patch.always_show_selection {
            next.always_show_selection = value;
        }

        if let Some(format) = &patch.default_format {
            format.apply(&mut next.default_format)
        }
        if let Some(value) = patch.width {
            next.box_size
                .get_or_insert(TextBoxSize {
                    width: 100.,
                    height: 100.,
                })
                .width = value
        }
        if let Some(value) = patch.height {
            next.box_size
                .get_or_insert(TextBoxSize {
                    width: 100.,
                    height: 100.,
                })
                .height = value
        }
        if let Some(value) = patch.multiline {
            next.multiline = value
        }
        if let Some(value) = patch.word_wrap {
            next.word_wrap = value
        }
        if let Some(text) = &patch.text {
            if text.len() > MAX_TEXT_BYTES {
                return Err(error("text exceeds 1048576 UTF-8 bytes"));
            }
            next.text = normalize_text(text);
            next.format = next.default_format.clone();
            next.format_runs.clear();
        }
        if let Some(text) = &patch.append {
            if next.text.len().saturating_add(text.len()) > MAX_TEXT_BYTES {
                return Err(error("append exceeds 1048576 UTF-8 bytes"));
            }
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                let old_units = next.text.encode_utf16().count() as u32;
                let added_units = normalized.encode_utf16().count() as u32;
                if old_units == 0 {
                    next.format = next.default_format.clone();
                    next.format_runs.clear();
                } else if !next.format_runs.is_empty() || next.format != next.default_format {
                    let mut runs = next.materialized_runs(old_units);
                    runs.push(TextFormatRun {
                        start: old_units,
                        end: old_units + added_units,
                        format: next.default_format.clone(),
                    });
                    next.format_runs = runs;
                }
                next.text.push_str(&normalized);
                next.normalize_runs();
            }
        }
        if let Some(replacement) = patch.replace.as_ref().or(selected_replacement.as_ref()) {
            let start = utf16_byte(&next.text, replacement.begin)?;
            let end = utf16_byte(&next.text, replacement.end)?;
            if start > end {
                return Err(error("replacement range must be ordered"));
            }
            if next.text.len() - (end - start) + replacement.text.len() > MAX_TEXT_BYTES {
                return Err(error("replacement exceeds 1048576 UTF-8 bytes"));
            }
            let normalized = normalize_text(&replacement.text);
            let old_units = next.text.encode_utf16().count() as u32;
            let insert_units = normalized.encode_utf16().count() as u32;
            let retained = start > 0 || end < next.text.len();
            if !retained {
                if selected_replacement.is_some() || old_units == 0 {
                    next.format = next.default_format.clone();
                }
                next.format_runs.clear();
            } else {
                let insert_fmt = if selected_replacement.is_some() {
                    next.default_format.clone()
                } else {
                    next.format_at_utf16(replacement.begin, old_units)
                };
                if !next.format_runs.is_empty() || insert_fmt != next.format {
                    let old_runs = next.materialized_runs(old_units);
                    let removed = replacement.end - replacement.begin;
                    let mut runs = Vec::new();
                    for r in &old_runs {
                        if r.end <= replacement.begin {
                            runs.push(r.clone());
                        } else if r.start < replacement.begin {
                            runs.push(TextFormatRun {
                                start: r.start,
                                end: replacement.begin,
                                format: r.format.clone(),
                            });
                        }
                    }
                    if insert_units > 0 {
                        runs.push(TextFormatRun {
                            start: replacement.begin,
                            end: replacement.begin + insert_units,
                            format: insert_fmt,
                        });
                    }
                    for r in &old_runs {
                        if r.start >= replacement.end {
                            runs.push(TextFormatRun {
                                start: r.start - removed + insert_units,
                                end: r.end - removed + insert_units,
                                format: r.format.clone(),
                            });
                        } else if r.end > replacement.end {
                            runs.push(TextFormatRun {
                                start: replacement.begin + insert_units,
                                end: r.end - removed + insert_units,
                                format: r.format.clone(),
                            });
                        }
                    }
                    next.format_runs = runs;
                }
            }
            next.text.replace_range(start..end, &normalized);
            next.normalize_runs();
        }
        if let Some(format) = &patch.format {
            format.apply(&mut next.format);
            for run in &mut next.format_runs {
                format.apply(&mut run.format);
            }
            next.normalize_runs();
        }
        if let Some(runs) = &patch.format_runs {
            next.format_runs = runs.clone();
            next.normalize_runs();
        }
        if let Some(range_patch) = &patch.format_range {
            utf16_byte(&next.text, range_patch.begin)?;
            utf16_byte(&next.text, range_patch.end)?;
            if range_patch.begin > range_patch.end {
                return Err(error("format_range endpoints must be ordered"));
            }
            let total_units = next.text.encode_utf16().count() as u32;
            if range_patch.begin < range_patch.end && total_units > 0 {
                let old_runs = next.materialized_runs(total_units);
                let mut runs = Vec::new();
                for r in old_runs {
                    if r.end <= range_patch.begin || r.start >= range_patch.end {
                        runs.push(r);
                    } else {
                        if r.start < range_patch.begin {
                            runs.push(TextFormatRun {
                                start: r.start,
                                end: range_patch.begin,
                                format: r.format.clone(),
                            });
                        }
                        let mid_start = r.start.max(range_patch.begin);
                        let mid_end = r.end.min(range_patch.end);
                        let mut mid_fmt = r.format.clone();
                        range_patch.format.apply(&mut mid_fmt);
                        runs.push(TextFormatRun {
                            start: mid_start,
                            end: mid_end,
                            format: mid_fmt,
                        });
                        if r.end > range_patch.end {
                            runs.push(TextFormatRun {
                                start: range_patch.end,
                                end: r.end,
                                format: r.format,
                            });
                        }
                    }
                }
                next.format_runs = runs;
                next.normalize_runs();
            }
        }
        if patch.text.is_some() {
            next.selection_begin = 0;
            next.selection_end = 0;
            next.caret = 0;
        }
        if let Some(replacement) = &selected_replacement {
            let caret =
                replacement.begin + normalize_text(&replacement.text).encode_utf16().count() as u32;
            next.selection_begin = caret;
            next.selection_end = caret;
            next.caret = caret;
        }
        next.selection_begin = clamp_utf16(&next.text, i64::from(next.selection_begin));
        next.selection_end = clamp_utf16(&next.text, i64::from(next.selection_end));
        next.caret = clamp_utf16(&next.text, i64::from(next.caret));
        if let Some(selection) = patch.selection {
            let begin = clamp_utf16(&next.text, selection.begin);
            let end = clamp_utf16(&next.text, selection.end);
            next.selection_begin = begin.min(end);
            next.selection_end = begin.max(end);
            next.caret = end;
        }
        next.validate()?;
        if next != *self {
            next.revision = self
                .revision
                .parse::<u64>()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| error("text revision exhausted"))?
                .to_string();
            *self = next;
        }
        Ok(())
    }
    /// Commit a user replacement against the exact revision observed by the
    /// DOM composition transaction. Programmatic mutations use apply instead.
    pub fn apply_input(&mut self, edit: &TextInputEdit) -> Result<(), TextError> {
        if self.field_type != TextFieldType::Input {
            return Err(error("user editing requires an input TextField"));
        }
        if edit.expected_revision != self.revision {
            return Err(error(
                "stale text input revision; refresh the editor before committing",
            ));
        }
        if edit.text.len() > MAX_TEXT_BYTES {
            return Err(error("input insertion exceeds 1048576 UTF-8 bytes"));
        }
        utf16_byte(&self.text, edit.begin)?;
        utf16_byte(&self.text, edit.end)?;
        if edit.begin > edit.end {
            return Err(error("input replacement range must be ordered"));
        }
        let restriction = self
            .restrict
            .as_deref()
            .map(Restriction::compile)
            .transpose()?;
        let retained = self.text.encode_utf16().count() - (edit.end - edit.begin) as usize;
        let capacity = if self.max_chars == 0 {
            MAX_TEXT_BYTES
        } else {
            (self.max_chars as usize).saturating_sub(retained)
        };
        let mut inserted = String::new();
        let mut units = 0;
        for ch in normalize_text(&edit.text).chars() {
            if (!self.multiline && ch == '\n')
                || restriction.as_ref().is_some_and(|rule| !rule.allows(ch))
            {
                continue;
            }
            if units + ch.len_utf16() > capacity {
                break;
            }
            inserted.push(ch);
            units += ch.len_utf16();
        }
        // Rejected typing must not erase an existing selection.
        if !edit.text.is_empty() && inserted.is_empty() {
            return Ok(());
        }
        let caret = i64::from(edit.begin) + units as i64;
        self.apply(&TextPatch {
            replace: Some(TextReplacement {
                begin: edit.begin,
                end: edit.end,
                text: inserted,
            }),
            selection: Some(TextSelection {
                begin: caret,
                end: caret,
            }),
            ..TextPatch::default()
        })
    }

    pub fn materialized_runs(&self, total_units: u32) -> Vec<TextFormatRun> {
        if total_units == 0 {
            return Vec::new();
        }
        if self.format_runs.is_empty() {
            return vec![TextFormatRun {
                start: 0,
                end: total_units,
                format: self.format.clone(),
            }];
        }
        let mut sorted = self.format_runs.clone();
        sorted.sort_by_key(|r| (r.start, r.end));
        let mut out: Vec<TextFormatRun> = Vec::new();
        let mut cursor = 0u32;
        for run in sorted {
            let start = run.start.min(total_units).max(cursor);
            let end = run.end.min(total_units);
            if start > cursor {
                Self::push_merged_run(
                    &mut out,
                    TextFormatRun {
                        start: cursor,
                        end: start,
                        format: self.format.clone(),
                    },
                );
            }
            if end > start {
                Self::push_merged_run(
                    &mut out,
                    TextFormatRun {
                        start,
                        end,
                        format: run.format,
                    },
                );
                cursor = end;
            }
        }
        if cursor < total_units {
            Self::push_merged_run(
                &mut out,
                TextFormatRun {
                    start: cursor,
                    end: total_units,
                    format: self.format.clone(),
                },
            );
        }
        out
    }

    fn push_merged_run(out: &mut Vec<TextFormatRun>, run: TextFormatRun) {
        if run.start >= run.end {
            return;
        }
        if let Some(last) = out.last_mut()
            && last.end == run.start
            && last.format == run.format
        {
            last.end = run.end;
            return;
        }
        out.push(run);
    }

    pub fn format_at_utf16(&self, pos: u32, total_units: u32) -> TextFormat {
        if total_units == 0 || self.format_runs.is_empty() {
            return self.format.clone();
        }
        for r in &self.format_runs {
            if r.start <= pos && pos < r.end {
                return r.format.clone();
            }
        }
        if pos > 0 {
            for r in &self.format_runs {
                if r.end == pos {
                    return r.format.clone();
                }
            }
        }
        self.format.clone()
    }

    pub fn normalize_runs(&mut self) {
        let total_units = self.text.encode_utf16().count() as u32;
        if total_units == 0 {
            self.format_runs.clear();
            return;
        }
        if self.format_runs.is_empty() {
            return;
        }
        let runs = self.materialized_runs(total_units);
        if runs.len() <= 1 {
            if let Some(only) = runs.into_iter().next() {
                self.format = only.format;
            }
            self.format_runs.clear();
        } else {
            self.format = runs[0].format.clone();
            self.format_runs = runs;
        }
    }

    pub fn merged_format_json(&self, begin: i64, end: i64) -> Result<serde_json::Value, TextError> {
        let total = self.text.encode_utf16().count() as i64;
        let format_to_json = |f: &TextFormat| {
            serde_json::json!({
                "font": f.font,
                "size": f.size,
                "color": f.color,
                "bold": f.bold,
                "italic": f.italic,
                "underline": f.underline,
                "align": f.effective_align_mode(),
                "leading": f.leading,
                "letter_spacing": f.letter_spacing,
                "left_margin": f.left_margin,
                "right_margin": f.right_margin,
                "indent": f.indent,
                "block_indent": f.block_indent,
                "bullet": f.bullet,
                "kerning": f.kerning,
                "url": f.url,
                "target": f.target_str(),
                "tab_stops": f.tab_stops,
                "display": f.display().unwrap_or("block"),
            })
        };
        if total == 0 {
            if begin == -1 && end == -1 {
                return Ok(format_to_json(&self.format));
            }
            return Err(error("TextField.getTextFormat range is out of bounds"));
        }
        let (r_begin, r_end) = if begin == -1 && end == -1 {
            (0u32, total as u32)
        } else if begin >= 0 && end == -1 {
            if begin >= total {
                return Err(error("TextField.getTextFormat beginIndex is out of bounds"));
            }
            (begin as u32, (begin + 1) as u32)
        } else {
            if begin < 0 || end < begin || begin >= total || end > total {
                return Err(error("TextField.getTextFormat range is out of bounds"));
            }
            if begin == end {
                let fmt = self.format_at_utf16(begin as u32, total as u32);
                return Ok(format_to_json(&fmt));
            }
            (begin as u32, end as u32)
        };
        let runs: Vec<TextFormatRun> = self
            .materialized_runs(total as u32)
            .into_iter()
            .filter(|r| r.end > r_begin && r.start < r_end)
            .collect();
        let Some(first) = runs.first() else {
            return Ok(format_to_json(&self.format));
        };
        if runs.len() == 1 {
            return Ok(format_to_json(&first.format));
        }
        let f0 = &first.format;
        let same = |pred: &dyn Fn(&TextFormat) -> bool| runs.iter().all(|r| pred(&r.format));
        Ok(serde_json::json!({
            "font": if same(&|f| f.font == f0.font) { serde_json::json!(f0.font) } else { serde_json::Value::Null },
            "size": if same(&|f| (f.size - f0.size).abs() < f64::EPSILON) { serde_json::json!(f0.size) } else { serde_json::Value::Null },
            "color": if same(&|f| f.color == f0.color) { serde_json::json!(f0.color) } else { serde_json::Value::Null },
            "bold": if same(&|f| f.bold == f0.bold) { serde_json::json!(f0.bold) } else { serde_json::Value::Null },
            "italic": if same(&|f| f.italic == f0.italic) { serde_json::json!(f0.italic) } else { serde_json::Value::Null },
            "underline": if same(&|f| f.underline == f0.underline) { serde_json::json!(f0.underline) } else { serde_json::Value::Null },
            "align": if same(&|f| f.effective_align_mode() == f0.effective_align_mode()) { serde_json::json!(f0.effective_align_mode()) } else { serde_json::Value::Null },
            "leading": if same(&|f| (f.leading - f0.leading).abs() < f64::EPSILON) { serde_json::json!(f0.leading) } else { serde_json::Value::Null },
            "letter_spacing": if same(&|f| (f.letter_spacing - f0.letter_spacing).abs() < f64::EPSILON) { serde_json::json!(f0.letter_spacing) } else { serde_json::Value::Null },
            "left_margin": if same(&|f| (f.left_margin - f0.left_margin).abs() < f64::EPSILON) { serde_json::json!(f0.left_margin) } else { serde_json::Value::Null },
            "right_margin": if same(&|f| (f.right_margin - f0.right_margin).abs() < f64::EPSILON) { serde_json::json!(f0.right_margin) } else { serde_json::Value::Null },
            "indent": if same(&|f| (f.indent - f0.indent).abs() < f64::EPSILON) { serde_json::json!(f0.indent) } else { serde_json::Value::Null },
            "block_indent": if same(&|f| (f.block_indent - f0.block_indent).abs() < f64::EPSILON) { serde_json::json!(f0.block_indent) } else { serde_json::Value::Null },
            "bullet": if same(&|f| f.bullet == f0.bullet) { serde_json::json!(f0.bullet) } else { serde_json::Value::Null },
            "kerning": if same(&|f| f.kerning == f0.kerning) { serde_json::json!(f0.kerning) } else { serde_json::Value::Null },
            "url": if same(&|f| f.url == f0.url) { serde_json::json!(f0.url) } else { serde_json::Value::Null },
            "target": if same(&|f| f.target_str() == f0.target_str()) { serde_json::json!(f0.target_str()) } else { serde_json::Value::Null },
            "tab_stops": if same(&|f| f.tab_stops == f0.tab_stops) { serde_json::json!(f0.tab_stops) } else { serde_json::Value::Null },
            "display": if same(&|f| f.display().unwrap_or("block") == f0.display().unwrap_or("block")) { serde_json::json!(f0.display().unwrap_or("block")) } else { serde_json::Value::Null },
        }))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatch {
    pub as2_viewport: Option<As2TextViewport>,
    pub text: Option<String>,
    pub append: Option<String>,
    pub replace: Option<TextReplacement>,
    pub replace_selected: Option<String>,
    pub selection: Option<TextSelection>,
    pub field_type: Option<TextFieldType>,
    pub selectable: Option<bool>,
    pub display_as_password: Option<bool>,
    pub max_chars: Option<u32>,
    pub restrict: Option<TextRestrictPatch>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub multiline: Option<bool>,
    pub word_wrap: Option<bool>,
    pub format: Option<TextFormatPatch>,
    pub default_format: Option<TextFormatPatch>,
    #[serde(default)]
    pub format_range: Option<TextFormatRangePatch>,
    #[serde(default)]
    pub format_runs: Option<Vec<TextFormatRun>>,
    pub anti_alias_type: Option<String>,
    pub grid_fit_type: Option<String>,
    pub sharpness: Option<f64>,
    pub thickness: Option<f64>,
    pub embed_fonts: Option<bool>,
    pub condense_white: Option<bool>,
    pub always_show_selection: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextReplacement {
    pub begin: u32,
    pub end: u32,
    pub text: String,
}
fn clamp_utf16(text: &str, index: i64) -> u32 {
    let wanted = index.max(0) as u64;
    let mut units = 0u32;
    for ch in text.chars() {
        let next = units + ch.len_utf16() as u32;
        if u64::from(next) > wanted {
            return units;
        }
        units = next;
    }
    units
}
/// Compiled change points keep membership O(log ranges), including long pasted
/// text. The common restrict grammar is exact Unicode ranges/toggles/escapes;
/// automatic legacy case substitution and malformed ranges are not guessed.
struct Restriction {
    points: BTreeMap<u32, bool>,
}
impl Restriction {
    fn compile(pattern: &str) -> Result<Self, TextError> {
        if pattern.len() > 4096 {
            return Err(error("restrict exceeds 4096 UTF-8 bytes"));
        }
        #[derive(Clone, Copy)]
        enum Token {
            Char(char),
            Toggle,
            Dash,
        }
        let mut tokens = vec![];
        let mut chars = pattern.chars();
        while let Some(ch) = chars.next() {
            tokens.push(match ch {
                '\\' => match chars.next() {
                    Some(value @ ('\\' | '^' | '-')) => Token::Char(value),
                    _ => {
                        return Err(error(
                            "restrict accepts only escaped backslash, caret or hyphen",
                        ));
                    }
                },
                '^' => Token::Toggle,
                '-' => Token::Dash,
                value => Token::Char(value),
            });
        }
        let mut result = Self {
            points: BTreeMap::from([
                (0, matches!(tokens.first(), Some(Token::Toggle))),
                (0x110000, false),
            ]),
        };
        let mut include = true;
        let mut i = 0;
        let mut rules = 0;
        while i < tokens.len() {
            let start = match tokens[i] {
                Token::Toggle => {
                    include = !include;
                    i += 1;
                    continue;
                }
                Token::Dash => '-',
                Token::Char(ch) => ch,
            };
            let mut end = start;
            if let (Some(Token::Dash), Some(Token::Char(last))) =
                (tokens.get(i + 1), tokens.get(i + 2))
            {
                end = *last;
                i += 2;
            }
            if start > end {
                return Err(error("restrict range endpoints must be ordered"));
            }
            rules += 1;
            if rules > 512 {
                return Err(error("restrict exceeds 512 ranges/characters"));
            }
            let start = start as u32;
            let end = end as u32 + 1;
            let after = *result
                .points
                .range(..=end)
                .next_back()
                .expect("initial point")
                .1;
            let keys: Vec<_> = result
                .points
                .range(start..end)
                .map(|(key, _)| *key)
                .collect();
            for key in keys {
                result.points.remove(&key);
            }
            result.points.insert(start, include);
            result.points.insert(end, after);
            i += 1;
        }
        Ok(result)
    }
    fn allows(&self, ch: char) -> bool {
        *self
            .points
            .range(..=ch as u32)
            .next_back()
            .expect("initial point")
            .1
    }
}
fn utf16_byte(text: &str, index: u32) -> Result<usize, TextError> {
    let mut units = 0u32;
    for (byte, ch) in text.char_indices() {
        if units == index {
            return Ok(byte);
        }
        units += ch.len_utf16() as u32;
        if units > index {
            return Err(error("UTF-16 range splits a surrogate pair"));
        }
    }
    if units == index {
        Ok(text.len())
    } else {
        Err(error("UTF-16 range is outside the text"))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLine {
    pub text: String,
    pub start: u32,
    pub end: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextDecoration {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextViewportMetrics {
    pub offset_x: f64,
    pub offset_y: f64,
    pub scroll: u32,
    pub hscroll: u32,
    pub maxscroll: u32,
    pub maxhscroll: u32,
    pub bottom_scroll: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSurface {
    pub width: f64,
    pub height: f64,
    pub background: Option<u32>,
    pub border: Option<u32>,
    #[serde(default = "default_one_f64", skip_serializing_if = "is_one_f64")]
    pub border_width: f64,
    #[serde(default = "default_one_u32", skip_serializing_if = "is_one_u32")]
    pub column_count: u32,
    #[serde(
        default = "default_gutter_f64",
        skip_serializing_if = "is_default_gutter"
    )]
    pub column_gutter: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub text_width: f64,
    pub text_height: f64,
    pub line_height: f64,
    pub decorations: Vec<TextDecoration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<TextDecoration>,
    pub runs: Vec<TextRun>,
    pub bullets: Vec<[f64; 3]>,
    pub viewport: TextViewportMetrics,
    pub surface: TextSurface,
}
struct Part {
    text: String,
    start: u32,
    end: u32,
    newline: bool,
}
fn apply_typographic_case(text: &str, mode: &str) -> String {
    match mode {
        "uppercase" | "lowercase_to_small_caps" => text
            .chars()
            .map(|ch| {
                let up: String = ch.to_uppercase().collect();
                if up.encode_utf16().count() == ch.len_utf16() {
                    up
                } else {
                    ch.to_string()
                }
            })
            .collect(),
        "lowercase" => text
            .chars()
            .map(|ch| {
                let lo: String = ch.to_lowercase().collect();
                if lo.encode_utf16().count() == ch.len_utf16() {
                    lo
                } else {
                    ch.to_string()
                }
            })
            .collect(),
        _ => text.to_owned(),
    }
}
fn parts(field: &TextFieldState) -> Result<Vec<Part>, TextError> {
    let mut run_boundaries = BTreeSet::new();
    for r in &field.format_runs {
        run_boundaries.insert(r.start);
        run_boundaries.insert(r.end);
    }
    let split_tabs_for_wrap = field.multiline
        && field.word_wrap
        && field.box_size.is_some()
        && (!field.format.tab_stops.is_empty()
            || field
                .format_runs
                .iter()
                .any(|r| !r.format.tab_stops.is_empty()));
    let break_all = matches!(field.line_break.as_str(), "all" | "any")
        || matches!(field.orientation.as_str(), "vertical_ltr" | "vertical_rtl");
    let mut result = Vec::new();
    let mut text = String::new();
    let mut start = 0;
    let mut offset = 0;
    let mut space = false;
    let displayed;
    let display_source = if field.display_as_password {
        displayed = field
            .text
            .chars()
            .map(|ch| {
                if ch == '\n' {
                    "\n".into()
                } else {
                    "*".repeat(ch.len_utf16())
                }
            })
            .collect::<String>();
        displayed.as_str()
    } else if !is_default_str(&field.typographic_case) {
        displayed = apply_typographic_case(&field.text, &field.typographic_case);
        displayed.as_str()
    } else {
        field.text.as_str()
    };
    for ch in display_source.chars() {
        let newline = ch == '\n';
        let is_tab = split_tabs_for_wrap && ch == '\t';
        let nextspace = ch.is_whitespace();
        let hit_boundary = !run_boundaries.is_empty() && run_boundaries.contains(&offset);
        if !text.is_empty()
            && (newline || is_tab || hit_boundary || space != nextspace || break_all)
        {
            result.push(Part {
                text: std::mem::take(&mut text),
                start,
                end: offset,
                newline: false,
            });
            start = offset;
        }
        if newline {
            result.push(Part {
                text: if field.multiline {
                    String::new()
                } else {
                    " ".into()
                },
                start: offset,
                end: offset + 1,
                newline: field.multiline,
            });
            start = offset + 1;
        } else if is_tab {
            result.push(Part {
                text: "\t".into(),
                start: offset,
                end: offset + 1,
                newline: false,
            });
            start = offset + 1;
        } else {
            text.push(ch);
            space = nextspace;
        }
        offset += ch.len_utf16() as u32;
        if result.len() > MAX_PARTS {
            return Err(error("layout exceeds 16384 word/space/line segments"));
        }
    }
    if !text.is_empty() {
        result.push(Part {
            text,
            start,
            end: offset,
            newline: false,
        })
    }
    if result.len() > MAX_PARTS {
        return Err(error("layout exceeds 16384 word/space/line segments"));
    }
    Ok(result)
}
fn measured<M>(
    strings: &[String],
    format: &TextFormat,
    measure: &mut M,
) -> Result<Vec<f64>, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    let mut result = measure(strings, format)?;
    if result.len() != strings.len() || result.iter().any(|w| !w.is_finite() || *w < 0. || *w > 1e9)
    {
        return Err(error(
            "font measurement must return one finite width in 0..=1e9 per string",
        ));
    }
    for (width, text) in result.iter_mut().zip(strings) {
        *width = (*width + text.chars().count().saturating_sub(1) as f64 * format.letter_spacing)
            .max(0.);
    }
    Ok(result)
}

fn next_tab_stop(x: f64, format: &TextFormat) -> f64 {
    format
        .tab_stops
        .iter()
        .copied()
        .find(|stop| *stop > x)
        .unwrap_or_else(|| {
            let origin = format.tab_stops.last().copied().unwrap_or(0.);
            let step = format.size * 4.;
            origin + ((x - origin) / step).floor().max(0.) * step + step
        })
}

fn align_offset(align_mode: &str, avail: f64, width: f64) -> f64 {
    match align_mode {
        "center" => (avail - width) / 2.,
        "right" | "end" => avail - width,
        _ => 0.,
    }
}

fn line_advance_for(max_size: f64, leading: f64, leading_mode: &str) -> f64 {
    if leading_mode == "percent" {
        let pct = if leading <= 0.0 { 120.0 } else { leading };
        (max_size * (pct / 100.0)).max(1.0)
    } else {
        (max_size * 1.2 + leading).max(1.0)
    }
}

fn effective_run_format(base: &TextFormat, field: &TextFieldState) -> TextFormat {
    let mut fmt = base.clone();
    if field.superscript || field.subscript {
        fmt.size = (fmt.size * 0.65).max(1.0);
    } else if matches!(
        field.typographic_case.as_str(),
        "caps_to_small_caps" | "lowercase_to_small_caps"
    ) {
        fmt.size = (fmt.size * 0.8).max(1.0);
    }
    fmt
}

impl TextFieldState {
    pub fn has_extended_layout_options(&self) -> bool {
        self.column_count > 1
            || !is_default_pad(&self.padding_left)
            || !is_default_pad(&self.padding_right)
            || !is_default_pad(&self.padding_top)
            || !is_default_pad(&self.padding_bottom)
            || !is_one_f64(&self.border_width)
            || !is_top_str(&self.vertical_align)
            || !is_auto_str(&self.first_line_offset_mode)
            || !is_horiz_str(&self.orientation)
            || !is_ltr_str(&self.direction)
            || !is_pt_str(&self.leading_mode)
            || self.highlight
            || self.strikethrough
            || self.superscript
            || self.subscript
            || !is_default_str(&self.typographic_case)
            || !is_auto_str(&self.line_break)
            || !is_zero_f64(&self.baseline_shift)
            || !is_zero_f64(&self.space_before)
            || !is_zero_f64(&self.space_after)
            || !is_word_spacing_str(&self.text_justify)
            || self.format.effective_align_mode() == "justify_all"
    }
}

fn layout_multirun_text<M>(field: &TextFieldState, mut measure: M) -> Result<TextLayout, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    let total_units = field.text.encode_utf16().count() as u32;
    let parts = parts(field)?;
    let mat_runs = field.materialized_runs(total_units);
    let run_index_at = |pos: u32| -> usize {
        mat_runs
            .iter()
            .position(|r| r.start <= pos && pos < r.end)
            .unwrap_or_else(|| mat_runs.len().saturating_sub(1))
    };
    let fmt_at = |pos: u32| -> &TextFormat {
        if mat_runs.is_empty() {
            &field.format
        } else {
            &mat_runs[run_index_at(pos)].format
        }
    };

    // Measure parts grouped by materialized run format to minimize host calls.
    let mut widths = vec![0.0_f64; parts.len()];
    if mat_runs.is_empty() {
        let strings: Vec<_> = parts.iter().map(|p| p.text.clone()).collect();
        let eff_fmt = effective_run_format(&field.format, field);
        widths = measured(&strings, &eff_fmt, &mut measure)?;
    } else {
        let mut grouped_indices: Vec<Vec<usize>> = vec![Vec::new(); mat_runs.len()];
        for (idx, part) in parts.iter().enumerate() {
            if !part.newline {
                grouped_indices[run_index_at(part.start)].push(idx);
            }
        }
        for (r_idx, indices) in grouped_indices.into_iter().enumerate() {
            if indices.is_empty() {
                continue;
            }
            let strings: Vec<String> = indices.iter().map(|&i| parts[i].text.clone()).collect();
            let eff_fmt = effective_run_format(&mat_runs[r_idx].format, field);
            let w = measured(&strings, &eff_fmt, &mut measure)?;
            for (i, width) in indices.into_iter().zip(w) {
                widths[i] = width;
            }
        }
    }

    let (pad_l, pad_r, pad_t, pad_b) = if field.box_size.is_some() {
        (
            field.padding_left.max(0.0),
            field.padding_right.max(0.0),
            field.padding_top.max(0.0),
            field.padding_bottom.max(0.0),
        )
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    let cols = if field.box_size.is_some() {
        field.column_count.clamp(1, 50)
    } else {
        1
    };
    let gutter = if cols > 1 {
        field.column_gutter.max(0.0)
    } else {
        0.0
    };
    let is_vertical = matches!(field.orientation.as_str(), "vertical_ltr" | "vertical_rtl");
    let col_width_opt = field.box_size.map(|b| {
        if is_vertical {
            (b.height - pad_t - pad_b).max(0.0)
        } else {
            let inner_w = (b.width - pad_l - pad_r).max(0.0);
            let total_gutter = gutter * f64::from(cols - 1);
            ((inner_w - total_gutter) / f64::from(cols)).max(0.0)
        }
    });

    let mut paragraph_starts = BTreeSet::from([0u32]);
    let mut offset = 0u32;
    for ch in field.text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            paragraph_starts.insert(offset);
        }
    }
    let line_metrics_at = |start: u32| -> (f64, Option<f64>, &TextFormat) {
        let pfmt = fmt_at(start);
        let b_indent = if pfmt.bullet { pfmt.size } else { 0. };
        let rel_orig_x = pfmt.left_margin + pfmt.block_indent + b_indent;
        let p_indent = if paragraph_starts.contains(&start) {
            pfmt.indent
        } else {
            0.
        };
        let avail = col_width_opt.map(|cw| {
            (cw - pfmt.left_margin - pfmt.right_margin - pfmt.block_indent - b_indent - p_indent)
                .max(0.)
        });
        (rel_orig_x + p_indent, avail, pfmt)
    };
    let wrap = field.multiline
        && field.word_wrap
        && field.box_size.is_some()
        && field.line_break != "no_break";

    #[derive(Clone)]
    struct Seg {
        text: String,
        fmt: TextFormat,
        rel_x: f64,
        width: f64,
    }
    struct BuiltLine {
        line: TextLine,
        segs: Vec<Seg>,
        max_size: f64,
        leading: f64,
    }
    let mut built_lines: Vec<BuiltLine> = Vec::new();
    let mut cur_line = TextLine {
        text: String::new(),
        start: 0,
        end: 0,
        x: 0.,
        y: 0.,
        width: 0.,
        height: fmt_at(0).size * 1.2,
    };
    let mut cur_segs: Vec<Seg> = Vec::new();
    let mut cur_max_size = fmt_at(0).size;
    let mut cur_leading = fmt_at(0).leading;

    let flush = |built: &mut Vec<BuiltLine>,
                 cur_l: &mut TextLine,
                 cur_s: &mut Vec<Seg>,
                 max_sz: &mut f64,
                 lead: &mut f64|
     -> Result<(), TextError> {
        if built.len() >= MAX_PARTS {
            return Err(error("layout exceeds 16384 lines"));
        }
        cur_l.height = *max_sz * 1.2;
        let next_fmt = fmt_at(cur_l.end);
        let next_line = TextLine {
            text: String::new(),
            start: cur_l.end,
            end: cur_l.end,
            x: 0.,
            y: 0.,
            width: 0.,
            height: next_fmt.size * 1.2,
        };
        let done_line = std::mem::replace(cur_l, next_line);
        let done_segs = std::mem::take(cur_s);
        let done_max = *max_sz;
        let done_lead = *lead;
        *max_sz = next_fmt.size;
        *lead = next_fmt.leading;
        built.push(BuiltLine {
            line: done_line,
            segs: done_segs,
            max_size: done_max,
            leading: done_lead,
        });
        Ok(())
    };

    for (part, &part_w) in parts.iter().zip(&widths) {
        if part.newline {
            cur_line.end = part.end;
            flush(
                &mut built_lines,
                &mut cur_line,
                &mut cur_segs,
                &mut cur_max_size,
                &mut cur_leading,
            )?;
            continue;
        }
        let base_pfmt = fmt_at(part.start);
        let pfmt = effective_run_format(base_pfmt, field);
        if part.text == "\t" && !pfmt.tab_stops.is_empty() {
            let next_x = next_tab_stop(cur_line.width, &pfmt);
            cur_line.text.push('\t');
            cur_line.width = next_x;
            cur_line.end = part.end;
            continue;
        }
        let (_, avail_opt, _) = line_metrics_at(cur_line.start);
        let avail = avail_opt.unwrap_or(f64::MAX);
        if wrap && !cur_line.text.is_empty() && cur_line.width + part_w > avail {
            flush(
                &mut built_lines,
                &mut cur_line,
                &mut cur_segs,
                &mut cur_max_size,
                &mut cur_leading,
            )?;
        }
        if wrap && part_w > line_metrics_at(cur_line.start).1.unwrap_or(f64::MAX) {
            let chars: Vec<String> = part.text.chars().map(|c| c.to_string()).collect();
            let cw = measured(&chars, &pfmt, &mut measure)?;
            let mut char_off = part.start;
            let mut chunk = String::new();
            let mut chunk_x = cur_line.width;
            let mut chunk_w = 0.0;
            for (ch, w) in part.text.chars().zip(cw) {
                let cur_avail = line_metrics_at(cur_line.start).1.unwrap_or(f64::MAX);
                if !cur_line.text.is_empty() && cur_line.width + w > cur_avail {
                    if !chunk.is_empty() {
                        cur_segs.push(Seg {
                            text: std::mem::take(&mut chunk),
                            fmt: pfmt.clone(),
                            rel_x: chunk_x,
                            width: chunk_w,
                        });
                    }
                    flush(
                        &mut built_lines,
                        &mut cur_line,
                        &mut cur_segs,
                        &mut cur_max_size,
                        &mut cur_leading,
                    )?;
                    chunk_x = 0.0;
                    chunk_w = 0.0;
                }
                let gap = if cur_line.text.is_empty() {
                    0.0
                } else {
                    pfmt.letter_spacing
                };
                if chunk.is_empty() {
                    chunk_x = cur_line.width + gap;
                } else {
                    chunk_w += pfmt.letter_spacing;
                }
                cur_line.width += gap + w;
                cur_line.text.push(ch);
                chunk.push(ch);
                chunk_w += w;
                char_off += ch.len_utf16() as u32;
                cur_line.end = char_off;
                cur_max_size = cur_max_size.max(base_pfmt.size);
                cur_leading = cur_leading.max(base_pfmt.leading);
            }
            if !chunk.is_empty() {
                cur_segs.push(Seg {
                    text: chunk,
                    fmt: pfmt,
                    rel_x: chunk_x,
                    width: chunk_w,
                });
            }
        } else {
            let gap = if cur_line.text.is_empty() {
                0.0
            } else {
                pfmt.letter_spacing
            };
            let rel_x = cur_line.width + gap;
            cur_line.width = rel_x + part_w;
            cur_line.text.push_str(&part.text);
            cur_line.end = part.end;
            cur_max_size = cur_max_size.max(base_pfmt.size);
            cur_leading = cur_leading.max(base_pfmt.leading);
            cur_segs.push(Seg {
                text: part.text.clone(),
                fmt: pfmt,
                rel_x,
                width: part_w,
            });
        }
    }
    flush(
        &mut built_lines,
        &mut cur_line,
        &mut cur_segs,
        &mut cur_max_size,
        &mut cur_leading,
    )?;

    let first_line_top_extra = match field.first_line_offset_mode.as_str() {
        "line_height" => built_lines
            .first()
            .map_or(0.0, |b| (b.max_size * 0.2).max(0.0)),
        "pt" => field.first_line_offset.max(0.0),
        _ => 0.0,
    };
    let col_top_y = pad_t + first_line_top_extra;
    let col_avail_h_opt = field
        .box_size
        .map(|b| (b.height - pad_t - pad_b - first_line_top_extra).max(0.0));

    let line_count_usize = built_lines.len();
    let mut next_starts: Vec<Option<u32>> = built_lines
        .iter()
        .skip(1)
        .map(|b| Some(b.line.start))
        .collect();
    next_starts.push(None);

    // Compute natural height so multi-column containers either overflow at col_avail_h
    // or balance lines across columns when total content height fits inside one column.
    let natural_total_h: f64 = built_lines
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let adv = if i + 1 < line_count_usize {
                line_advance_for(b.max_size, b.leading, &field.leading_mode)
            } else {
                b.line.height
            };
            let para_gap = if i > 0 && paragraph_starts.contains(&b.line.start) {
                field.space_before.max(field.space_after).max(0.0)
            } else {
                0.0
            };
            adv + para_gap
        })
        .sum();
    let balanced_lines_per_col = if cols > 1
        && line_count_usize > 1
        && col_avail_h_opt.is_some_and(|ch| natural_total_h <= ch)
    {
        Some(line_count_usize.div_ceil(cols as usize).max(1))
    } else {
        None
    };

    let mut line_col_indices = vec![0u32; line_count_usize];
    let mut col_ranges: Vec<(usize, usize)> = Vec::new();
    let mut cur_col = 0u32;
    let mut cur_col_start_idx = 0usize;
    let mut y_in_col = 0.0_f64;
    let mut lines_in_col = 0usize;
    let mut text_width = 0.0_f64;
    let mut max_col_bottom = col_top_y;

    for (idx, bl) in built_lines.iter_mut().enumerate() {
        let (rel_base_x, avail_opt, pfmt) = line_metrics_at(bl.line.start);
        let raw_align = pfmt.effective_align_mode();
        let align_mode = if field.direction == "rtl" && pfmt.align_mode() == Some("start") {
            "right"
        } else if field.direction == "rtl" && pfmt.align_mode() == Some("end") {
            "left"
        } else {
            raw_align
        };
        let soft_wrapped = idx + 1 < line_count_usize
            && next_starts[idx].is_some_and(|ns| !paragraph_starts.contains(&ns));
        let should_justify =
            (align_mode == "justify" && soft_wrapped) || align_mode == "justify_all";
        if should_justify && let Some(avail) = avail_opt {
            let last_non_ws = bl.segs.iter().rposition(|s| !s.text.trim().is_empty());
            if let Some(last_idx) = last_non_ws {
                let content_w = bl.segs[last_idx].rel_x + bl.segs[last_idx].width;
                let slack = (avail - content_w).max(0.0);
                if slack > 0.0 {
                    let ws_indices: Vec<usize> = (0..last_idx)
                        .filter(|&i| bl.segs[i].text.chars().all(char::is_whitespace))
                        .collect();
                    if field.text_justify != "letter_spacing" && !ws_indices.is_empty() {
                        let extra_per_gap = slack / ws_indices.len() as f64;
                        let mut shift = 0.0;
                        for (s_i, seg) in bl.segs.iter_mut().enumerate() {
                            seg.rel_x += shift;
                            if s_i < last_idx && seg.text.chars().all(char::is_whitespace) {
                                seg.width += extra_per_gap;
                                shift += extra_per_gap;
                            }
                        }
                        bl.line.width = avail;
                    } else if last_idx > 0 {
                        let extra_per_seg = slack / last_idx as f64;
                        for (s_i, seg) in bl.segs.iter_mut().enumerate().take(last_idx + 1) {
                            seg.rel_x += s_i as f64 * extra_per_seg;
                        }
                        bl.line.width = avail;
                    }
                }
            }
        }

        let is_para_start = paragraph_starts.contains(&bl.line.start);
        let para_gap = if lines_in_col > 0 && is_para_start {
            field.space_before.max(field.space_after).max(0.0)
        } else {
            0.0
        };
        let need_next_col = if cur_col + 1 < cols && lines_in_col > 0 && !is_vertical {
            if let Some(target_lines) = balanced_lines_per_col {
                lines_in_col >= target_lines
            } else if let Some(col_h) = col_avail_h_opt {
                y_in_col + para_gap + bl.line.height > col_h
            } else {
                false
            }
        } else {
            false
        };
        if need_next_col {
            col_ranges.push((cur_col_start_idx, idx));
            cur_col += 1;
            cur_col_start_idx = idx;
            y_in_col = 0.0;
            lines_in_col = 0;
        } else {
            y_in_col += para_gap;
        }

        line_col_indices[idx] = cur_col;
        let col_w = col_width_opt.unwrap_or(0.0);
        let col_x_origin = if cols <= 1 {
            pad_l
        } else if field.direction == "rtl" {
            pad_l + f64::from(cols - 1 - cur_col) * (col_w + gutter)
        } else {
            pad_l + f64::from(cur_col) * (col_w + gutter)
        };
        let align_shift = avail_opt.map_or(0., |avail| {
            let mode = if align_mode == "justify_all" {
                "left"
            } else {
                align_mode
            };
            align_offset(mode, avail, bl.line.width)
        });
        bl.line.x = col_x_origin + rel_base_x + align_shift;
        bl.line.y = col_top_y + y_in_col;
        let advance = line_advance_for(bl.max_size, bl.leading, &field.leading_mode);
        y_in_col += advance;
        lines_in_col += 1;
        max_col_bottom = max_col_bottom.max(bl.line.y + bl.line.height);
        text_width = text_width.max(bl.line.width);
    }
    if cur_col_start_idx < line_count_usize {
        col_ranges.push((cur_col_start_idx, line_count_usize));
    }

    // Apply per-column vertical alignment (Top / Center / Bottom / Justify) when box_size is set.
    if let Some(col_avail_h) = col_avail_h_opt
        && !is_top_str(&field.vertical_align)
        && !is_vertical
    {
        for (start_idx, end_idx) in &col_ranges {
            if start_idx >= end_idx {
                continue;
            }
            let last_line = &built_lines[end_idx - 1].line;
            let content_h = (last_line.y - col_top_y) + last_line.height;
            let slack_v = (col_avail_h - content_h).max(0.0);
            if slack_v <= 0.0 {
                continue;
            }
            match field.vertical_align.as_str() {
                "center" => {
                    let shift = slack_v * 0.5;
                    for bl in &mut built_lines[*start_idx..*end_idx] {
                        bl.line.y += shift;
                    }
                }
                "bottom" => {
                    for bl in &mut built_lines[*start_idx..*end_idx] {
                        bl.line.y += slack_v;
                    }
                }
                "justify" if end_idx - start_idx > 1 => {
                    let count_in_col = end_idx - start_idx;
                    let step = slack_v / (count_in_col - 1) as f64;
                    for (local_i, bl) in built_lines[*start_idx..*end_idx].iter_mut().enumerate() {
                        bl.line.y += local_i as f64 * step;
                    }
                }
                _ => {}
            }
            let new_bottom = built_lines[end_idx - 1].line.y + built_lines[end_idx - 1].line.height;
            max_col_bottom = max_col_bottom.max(new_bottom);
        }
    }

    let base_advance =
        line_advance_for(field.format.size, field.format.leading, &field.leading_mode);
    let text_height = if built_lines.is_empty() {
        field.format.size * 1.2
    } else {
        (max_col_bottom - pad_t).max(field.format.size * 1.2)
    };
    let visible_height = field
        .box_size
        .map_or(text_height, |size| (size.height - pad_t - pad_b).max(0.));
    let visible_lines = (visible_height / base_advance.max(1.)).floor().max(1.) as u32;
    let line_count = built_lines.len() as u32;
    let maxscroll = if field.box_size.is_some() {
        line_count
            .saturating_sub(visible_lines)
            .saturating_add(1)
            .max(1)
    } else {
        1
    };
    let (_, first_avail, _) = line_metrics_at(0);
    let maxhscroll = if field.box_size.is_some() && !field.word_wrap {
        (text_width - first_avail.unwrap_or(text_width))
            .max(0.)
            .ceil()
            .min(1_000_000.) as u32
    } else {
        0
    };
    let scroll = field.as2_viewport.scroll.min(maxscroll);
    let hscroll = field.as2_viewport.hscroll.min(maxhscroll);
    let bottom_scroll = (scroll + (visible_height / base_advance.max(1.)).ceil().max(1.) as u32
        - 1)
    .min(line_count);
    let scroll_y = if scroll <= 1 {
        0.0
    } else {
        built_lines
            .get((scroll - 1) as usize)
            .map_or(f64::from(scroll - 1) * base_advance, |b| b.line.y - pad_t)
    };

    let mut runs = Vec::new();
    let mut decorations = Vec::new();
    let mut highlights = Vec::new();
    let mut bullets = Vec::new();
    let mut lines = Vec::with_capacity(built_lines.len());
    let sub_sup_shift = |max_size: f64| -> f64 {
        if field.superscript {
            -max_size * 0.35
        } else if field.subscript {
            max_size * 0.20
        } else {
            0.0
        }
    };
    let baseline_shift = -field.baseline_shift;

    if is_vertical {
        let box_w = field
            .box_size
            .map_or(text_width.max(field.format.size * 2.0), |b| b.width);
        let mut x_cursor = if field.orientation == "vertical_rtl" {
            (box_w - pad_r - field.format.size * 1.2).max(pad_l)
        } else {
            pad_l
        };
        for mut bl in built_lines {
            let col_step = bl.max_size * 1.2 + bl.leading.max(0.0);
            bl.line.x = x_cursor;
            bl.line.y = col_top_y;
            let mut char_y = col_top_y;
            for seg in bl.segs {
                for ch in seg.text.chars() {
                    let s = ch.to_string();
                    if field.highlight && !ch.is_whitespace() {
                        highlights.push(TextDecoration {
                            x: x_cursor,
                            y: char_y,
                            width: seg.fmt.size,
                            height: seg.fmt.size * 1.1,
                            color: Some(field.highlight_color),
                        });
                    }
                    runs.push(TextRun {
                        text: s,
                        x: x_cursor,
                        y: char_y + baseline_shift,
                        format: Some(seg.fmt.clone()),
                    });
                    char_y += (seg.fmt.size * 1.05 + seg.fmt.letter_spacing).max(2.0);
                }
            }
            bl.line.width = bl.max_size;
            bl.line.height = (char_y - col_top_y).max(bl.max_size * 1.2);
            lines.push(bl.line);
            if field.orientation == "vertical_rtl" {
                x_cursor -= col_step;
            } else {
                x_cursor += col_step;
            }
        }
    } else {
        for mut bl in built_lines {
            bl.line.x -= f64::from(hscroll);
            bl.line.y -= scroll_y;
            let pfmt = fmt_at(bl.line.start);
            if pfmt.bullet && paragraph_starts.contains(&bl.line.start) {
                bullets.push([
                    bl.line.x - pfmt.size * 0.6,
                    bl.line.y + pfmt.size * 0.6,
                    (pfmt.size * 0.1).max(1.),
                ]);
            }
            let v_shift = sub_sup_shift(bl.max_size) + baseline_shift;
            for seg in bl.segs {
                let sx = bl.line.x + seg.rel_x;
                let sy = bl.line.y + (bl.max_size - seg.fmt.size).max(0.0) + v_shift;
                if field.highlight && !seg.text.is_empty() {
                    highlights.push(TextDecoration {
                        x: sx,
                        y: bl.line.y,
                        width: seg.width,
                        height: bl.line.height,
                        color: Some(field.highlight_color),
                    });
                }
                if seg.fmt.underline && !seg.text.is_empty() {
                    decorations.push(TextDecoration {
                        x: sx,
                        y: sy + seg.fmt.size * 1.05,
                        width: seg.width,
                        height: (seg.fmt.size / 16.).max(1.),
                        color: Some(seg.fmt.color),
                    });
                }
                if field.strikethrough && !seg.text.is_empty() {
                    decorations.push(TextDecoration {
                        x: sx,
                        y: sy + seg.fmt.size * 0.55,
                        width: seg.width,
                        height: (seg.fmt.size / 16.).max(1.),
                        color: Some(seg.fmt.color),
                    });
                }
                runs.push(TextRun {
                    text: seg.text,
                    x: sx,
                    y: sy,
                    format: Some(seg.fmt),
                });
            }
            lines.push(bl.line);
        }
    }

    let viewport = TextViewportMetrics {
        offset_x: f64::from(hscroll),
        offset_y: scroll_y,
        scroll,
        hscroll,
        maxscroll,
        maxhscroll,
        bottom_scroll,
    };
    let surface = TextSurface {
        width: field.box_size.map_or(text_width, |size| size.width),
        height: field.box_size.map_or(text_height, |size| size.height),
        background: field
            .as2_viewport
            .background
            .then_some(field.as2_viewport.background_color),
        border: field
            .as2_viewport
            .border
            .then_some(field.as2_viewport.border_color),
        border_width: field.border_width,
        column_count: cols,
        column_gutter: field.column_gutter,
    };
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: base_advance,
        decorations,
        highlights,
        runs,
        bullets,
        viewport,
        surface,
    })
}

/// At most three host calls for uniform text; summed segment widths choose wraps,
/// then complete lines are measured once for coherent alignment/metrics. Multi-run
/// text delegates to `layout_multirun_text` so every run renders with its own style.
pub fn layout_text<M>(field: &TextFieldState, mut measure: M) -> Result<TextLayout, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    field.validate()?;
    if field.html
        && field.format_runs.is_empty()
        && field.text.contains('<')
        && let Ok((parsed_text, parsed_format, parsed_runs)) = crate::as2_text::parse_authored_html(
            &field.text,
            field.format.clone(),
            field.condense_white,
        )
    {
        let mut effective = field.clone();
        effective.html = false;
        effective.text = parsed_text;
        effective.format = parsed_format;
        effective.format_runs = parsed_runs;
        let max_len = effective.text.encode_utf16().count() as u32;
        effective.selection_begin = effective.selection_begin.min(max_len);
        effective.selection_end = effective.selection_end.min(max_len);
        effective.caret = effective.caret.min(max_len);
        return layout_text(&effective, measure);
    }
    if !field.format_runs.is_empty() || field.has_extended_layout_options() {
        return layout_multirun_text(field, measure);
    }
    let parts = parts(field)?;
    let strings: Vec<_> = parts.iter().map(|p| p.text.clone()).collect();
    let widths = measured(&strings, &field.format, &mut measure)?;
    let padding = if field.box_size.is_some() { 2. } else { 0. };
    let bullet_indent = if field.format.bullet {
        field.format.size
    } else {
        0.
    };
    let origin_x = padding + field.format.left_margin + field.format.block_indent + bullet_indent;
    let available = field.box_size.map(|b| {
        (b.width
            - 2. * padding
            - field.format.left_margin
            - field.format.right_margin
            - field.format.block_indent
            - bullet_indent)
            .max(0.)
    });
    let mut paragraph_starts = BTreeSet::from([0u32]);
    let mut offset = 0;
    for ch in field.text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            paragraph_starts.insert(offset);
        }
    }
    let line_indent = |start: u32| {
        if paragraph_starts.contains(&start) {
            field.format.indent
        } else {
            0.
        }
    };
    let available_at = |start: u32| (available.unwrap_or(f64::MAX) - line_indent(start)).max(0.);
    let wrap = field.multiline && field.word_wrap && available.is_some();
    let mut glyphs = BTreeSet::new();
    if wrap {
        for (part, width) in parts.iter().zip(&widths) {
            if *width > (available.unwrap() - field.format.indent.max(0.)).max(0.) {
                glyphs.extend(part.text.chars());
                if glyphs.len() > MAX_GLYPHS {
                    return Err(error("layout exceeds 65536 fallback glyphs"));
                }
            }
        }
    }
    let glyph_strings: Vec<_> = glyphs.iter().map(char::to_string).collect();
    let glyph_widths = if glyph_strings.is_empty() {
        vec![]
    } else {
        measured(&glyph_strings, &field.format, &mut measure)?
    };
    let glyph_widths: HashMap<_, _> = glyphs.into_iter().zip(glyph_widths).collect();
    let mut lines = Vec::new();
    let mut current = TextLine {
        text: String::new(),
        start: 0,
        end: 0,
        x: 0.,
        y: 0.,
        width: 0.,
        height: field.format.size * 1.2,
    };
    let flush = |lines: &mut Vec<TextLine>, current: &mut TextLine| -> Result<(), TextError> {
        if lines.len() >= MAX_PARTS {
            return Err(error("layout exceeds 16384 lines"));
        }
        let next = TextLine {
            text: String::new(),
            start: current.end,
            end: current.end,
            x: 0.,
            y: 0.,
            width: 0.,
            height: current.height,
        };
        lines.push(std::mem::replace(current, next));
        Ok(())
    };
    for (part, width) in parts.iter().zip(&widths) {
        if part.newline {
            current.end = part.end;
            flush(&mut lines, &mut current)?;
            continue;
        }
        if wrap && part.text == "\t" && !field.format.tab_stops.is_empty() {
            let next_x = next_tab_stop(current.width, &field.format);
            if !current.text.is_empty() && next_x > available_at(current.start) {
                flush(&mut lines, &mut current)?;
                current.text.push('\t');
                current.width = next_tab_stop(0.0, &field.format);
            } else {
                current.text.push('\t');
                current.width = next_x;
            }
            current.end = part.end;
            continue;
        }
        if wrap && *width > available_at(current.start) {
            let mut offset = part.start;
            for ch in part.text.chars() {
                let width = glyph_widths[&ch];
                if !current.text.is_empty() && current.width + width > available_at(current.start) {
                    flush(&mut lines, &mut current)?
                }
                if !current.text.is_empty() {
                    current.width += field.format.letter_spacing;
                }
                current.text.push(ch);
                current.width += width;
                offset += ch.len_utf16() as u32;
                current.end = offset;
            }
        } else {
            if wrap
                && !current.text.is_empty()
                && current.width + width > available_at(current.start)
            {
                flush(&mut lines, &mut current)?
            }
            if !current.text.is_empty() {
                current.width += field.format.letter_spacing;
            }
            current.text.push_str(&part.text);
            current.width += width;
            current.end = part.end;
        }
    }
    flush(&mut lines, &mut current)?;
    let strings: Vec<_> = lines.iter().map(|line| line.text.clone()).collect();
    let mut widths = measured(&strings, &field.format, &mut measure)?;
    let mut tab_runs: Vec<Vec<(String, f64)>> = vec![];
    if !field.format.tab_stops.is_empty() {
        let segments: Vec<String> = lines
            .iter()
            .flat_map(|line| line.text.split('\t').map(str::to_owned))
            .collect();
        if segments.len() > MAX_PARTS {
            return Err(error("custom tab layout exceeds16384 segments"));
        }
        let sizes = measured(&segments, &field.format, &mut measure)?;
        let mut index = 0;
        for (line, width) in lines.iter().zip(&mut widths) {
            let mut x = 0.;
            let mut runs = vec![];
            let mut parts = line.text.split('\t').peekable();
            while let Some(part) = parts.next() {
                runs.push((part.into(), x));
                x += sizes[index];
                index += 1;
                if parts.peek().is_some() {
                    x = next_tab_stop(x, &field.format);
                }
            }
            *width = x;
            tab_runs.push(runs);
        }
    }
    let align_mode = field.format.effective_align_mode();
    if align_mode == "justify" && available.is_some() && tab_runs.is_empty() && lines.len() > 1 {
        let line_total = lines.len();
        let next_line_starts: Vec<Option<u32>> = lines
            .iter()
            .skip(1)
            .map(|l| Some(l.start))
            .chain(std::iter::once(None))
            .collect();
        let mut all_words: Vec<String> = Vec::new();
        let mut line_word_counts: Vec<usize> = Vec::with_capacity(line_total);
        for (idx, line) in lines.iter().enumerate() {
            let soft_wrapped = idx + 1 < line_total
                && next_line_starts[idx].is_some_and(|ns| !paragraph_starts.contains(&ns));
            if soft_wrapped {
                let words: Vec<String> = line.text.split_whitespace().map(str::to_owned).collect();
                line_word_counts.push(words.len());
                all_words.extend(words);
            } else {
                line_word_counts.push(0);
            }
        }
        if !all_words.is_empty() && all_words.len() <= MAX_PARTS {
            let word_widths = measured(&all_words, &field.format, &mut measure)?;
            let mut cursor = 0usize;
            for (idx, (line, width)) in lines.iter().zip(&mut widths).enumerate() {
                let count = line_word_counts[idx];
                if count >= 2 {
                    let words = &all_words[cursor..cursor + count];
                    let ww = &word_widths[cursor..cursor + count];
                    let words_sum: f64 = ww.iter().sum();
                    let target_w = available_at(line.start);
                    if target_w > words_sum {
                        let gap = (target_w - words_sum) / (count - 1) as f64;
                        let mut x = 0.0;
                        let mut runs = Vec::with_capacity(count);
                        for (w_str, &w_len) in words.iter().zip(ww) {
                            runs.push((w_str.clone(), x));
                            x += w_len + gap;
                        }
                        *width = target_w;
                        tab_runs.push(runs);
                        cursor += count;
                        continue;
                    }
                }
                cursor += count;
                tab_runs.push(vec![(line.text.clone(), 0.0)]);
            }
        }
    }
    let advance = field.format.size * 1.2 + field.format.leading;
    let mut text_width = 0_f64;
    for (index, (line, width)) in lines.iter_mut().zip(widths).enumerate() {
        line.width = width;
        line.x = origin_x
            + line_indent(line.start)
            + available.map_or(0., |_| {
                align_offset(align_mode, available_at(line.start), width)
            });
        line.y = padding + index as f64 * advance;
        text_width = text_width.max(width);
    }
    let text_height = field.format.size * 1.2 + (lines.len().saturating_sub(1)) as f64 * advance;
    let visible_height = field
        .box_size
        .map_or(text_height, |size| (size.height - 2. * padding).max(0.));
    let visible_lines = (visible_height / advance).floor().max(1.) as u32;
    let line_count = lines.len() as u32;
    let maxscroll = if field.box_size.is_some() {
        line_count
            .saturating_sub(visible_lines)
            .saturating_add(1)
            .max(1)
    } else {
        1
    };
    let maxhscroll = if field.box_size.is_some() && !field.word_wrap {
        (text_width - available.unwrap_or(text_width))
            .max(0.)
            .ceil()
            .min(1_000_000.) as u32
    } else {
        0
    };
    let scroll = field.as2_viewport.scroll.min(maxscroll);
    let hscroll = field.as2_viewport.hscroll.min(maxhscroll);
    let bottom_scroll =
        (scroll + (visible_height / advance).ceil().max(1.) as u32 - 1).min(line_count);
    for line in &mut lines {
        line.x -= f64::from(hscroll);
        line.y -= f64::from(scroll - 1) * advance;
    }
    let viewport = TextViewportMetrics {
        offset_x: f64::from(hscroll),
        offset_y: f64::from(scroll - 1) * advance,
        scroll,
        hscroll,
        maxscroll,
        maxhscroll,
        bottom_scroll,
    };
    let surface = TextSurface {
        width: field.box_size.map_or(text_width, |size| size.width),
        height: field.box_size.map_or(text_height, |size| size.height),
        background: field
            .as2_viewport
            .background
            .then_some(field.as2_viewport.background_color),
        border: field
            .as2_viewport
            .border
            .then_some(field.as2_viewport.border_color),
        border_width: field.border_width,
        column_count: 1,
        column_gutter: field.column_gutter,
    };
    let runs = tab_runs
        .into_iter()
        .zip(&lines)
        .flat_map(|(runs, line)| {
            runs.into_iter().map(move |(text, x)| TextRun {
                text,
                x: line.x + x,
                y: line.y,
                format: None,
            })
        })
        .collect();
    let bullets = if field.format.bullet {
        lines
            .iter()
            .filter(|line| paragraph_starts.contains(&line.start))
            .map(|line| {
                [
                    line.x - field.format.size * 0.6,
                    line.y + field.format.size * 0.6,
                    (field.format.size * 0.1).max(1.),
                ]
            })
            .collect()
    } else {
        vec![]
    };
    let decorations = if field.format.underline {
        lines
            .iter()
            .filter(|line| !line.text.is_empty())
            .map(|line| TextDecoration {
                x: line.x,
                y: line.y + field.format.size * 1.05,
                width: line.width,
                height: (field.format.size / 16.).max(1.),
                color: None,
            })
            .collect()
    } else {
        vec![]
    };
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: advance,
        decorations,
        highlights: vec![],
        runs,
        bullets,
        viewport,
        surface,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextQuery {
    FirstCharInParagraph { char_index: i64 },
    ParagraphLength { char_index: i64 },
    LineIndexAtPoint { x: f64, y: f64 },
    CharBoundaries { char_index: i64 },
    CharIndexAtPoint { x: f64, y: f64 },
    GetFormat { begin: i64, end: i64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextCharBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn paragraph_range(text: &str, char_index: i64) -> Option<(u32, u32)> {
    if char_index < 0 {
        return None;
    }
    let wanted = char_index as u64;
    let mut para_start = 0u32;
    let mut offset = 0u32;
    for ch in text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            if wanted < u64::from(offset) {
                return Some((para_start, offset - para_start));
            }
            para_start = offset;
        }
    }
    if wanted < u64::from(offset) {
        Some((para_start, offset - para_start))
    } else {
        None
    }
}

struct LineCharSpan {
    start: u32,
    end: u32,
    glyph_x: f64,
    glyph_width: f64,
    cell_left: f64,
    cell_right: f64,
}

fn line_char_spans<M>(
    field: &TextFieldState,
    line: &TextLine,
    measure: &mut M,
) -> Result<Vec<LineCharSpan>, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    if line.text.is_empty() {
        return Ok(Vec::new());
    }
    let source_slice = {
        let start_byte = utf16_byte(&field.text, line.start)?;
        let end_byte = utf16_byte(&field.text, line.end)?;
        &field.text[start_byte..end_byte]
    };
    let source_chars: Vec<char> = source_slice
        .chars()
        .filter(|ch| !(field.multiline && *ch == '\n'))
        .collect();
    if field.format.tab_stops.is_empty() {
        let mut prefixes = Vec::new();
        let mut current = String::new();
        for ch in &source_chars {
            if field.display_as_password {
                for _ in 0..ch.len_utf16() {
                    current.push('*');
                }
            } else {
                current.push(*ch);
            }
            prefixes.push(current.clone());
        }
        let widths = measured(&prefixes, &field.format, measure)?;
        let mut spans = Vec::with_capacity(source_chars.len());
        let mut offset = line.start;
        for (idx, ch) in source_chars.iter().enumerate() {
            let len = ch.len_utf16() as u32;
            let prev = if idx == 0 { 0.0 } else { widths[idx - 1] };
            let next = widths[idx];
            let gap = if idx == 0 {
                0.0
            } else {
                field.format.letter_spacing
            };
            let glyph_left = (prev + gap).clamp(0.0, next);
            spans.push(LineCharSpan {
                start: offset,
                end: offset + len,
                glyph_x: line.x + glyph_left,
                glyph_width: (next - glyph_left).max(0.0),
                cell_left: line.x + prev.min(next),
                cell_right: line.x + next.max(prev),
            });
            offset += len;
        }
        return Ok(spans);
    }
    // Custom tabStops: measure cumulative prefixes within each non-tab run.
    let mut prefixes = Vec::new();
    let mut run_prefix_counts = Vec::new();
    let mut run_chars: Vec<Vec<char>> = vec![Vec::new()];
    for ch in &source_chars {
        if *ch == '\t' {
            run_chars.push(Vec::new());
        } else {
            run_chars.last_mut().expect("active run").push(*ch);
        }
    }
    for chars in &run_chars {
        let mut current = String::new();
        for ch in chars {
            if field.display_as_password {
                for _ in 0..ch.len_utf16() {
                    current.push('*');
                }
            } else {
                current.push(*ch);
            }
            prefixes.push(current.clone());
        }
        run_prefix_counts.push(chars.len());
    }
    let widths = if prefixes.is_empty() {
        Vec::new()
    } else {
        measured(&prefixes, &field.format, measure)?
    };
    let mut spans = Vec::with_capacity(source_chars.len());
    let mut offset = line.start;
    let mut width_cursor = 0usize;
    let mut x = 0.0_f64;
    for (run_idx, chars) in run_chars.iter().enumerate() {
        let count = run_prefix_counts[run_idx];
        let slice = &widths[width_cursor..width_cursor + count];
        width_cursor += count;
        for (idx, ch) in chars.iter().enumerate() {
            let len = ch.len_utf16() as u32;
            let prev = if idx == 0 { 0.0 } else { slice[idx - 1] };
            let next = slice[idx];
            let gap = if idx == 0 {
                0.0
            } else {
                field.format.letter_spacing
            };
            let glyph_left = (prev + gap).clamp(0.0, next);
            spans.push(LineCharSpan {
                start: offset,
                end: offset + len,
                glyph_x: line.x + x + glyph_left,
                glyph_width: (next - glyph_left).max(0.0),
                cell_left: line.x + x + prev.min(next),
                cell_right: line.x + x + next.max(prev),
            });
            offset += len;
        }
        if let Some(&last_w) = slice.last() {
            x += last_w;
        }
        if run_idx + 1 < run_chars.len() {
            let next_stop = field
                .format
                .tab_stops
                .iter()
                .copied()
                .find(|stop| *stop > x)
                .unwrap_or_else(|| {
                    let origin = field.format.tab_stops.last().copied().unwrap_or(0.);
                    let step = field.format.size * 4.;
                    origin + ((x - origin) / step).floor().max(0.) * step + step
                });
            spans.push(LineCharSpan {
                start: offset,
                end: offset + 1,
                glyph_x: line.x + x,
                glyph_width: (next_stop - x).max(0.0),
                cell_left: line.x + x,
                cell_right: line.x + next_stop.max(x),
            });
            offset += 1;
            x = next_stop;
        }
    }
    Ok(spans)
}

/// Lay out a text field once and return each character's glyph bounding box in field coordinates.
pub fn layout_char_boxes<M>(
    field: &TextFieldState,
    mut measure: M,
) -> Result<Vec<(char, TextCharBox)>, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    let layout = layout_text(field, &mut measure)?;
    let mut out = Vec::new();
    for line in &layout.lines {
        let spans = line_char_spans(field, line, &mut measure)?;
        for span in spans {
            let start_byte = utf16_byte(&field.text, span.start)?;
            let end_byte = utf16_byte(&field.text, span.end)?;
            if let Some(ch) = field.text[start_byte..end_byte].chars().next() {
                out.push((
                    ch,
                    TextCharBox {
                        x: span.glyph_x,
                        y: line.y,
                        width: span.glyph_width,
                        height: line.height,
                    },
                ));
            }
        }
    }
    Ok(out)
}

pub fn query_text<M>(
    field: &TextFieldState,
    query: &TextQuery,
    mut measure: M,
) -> Result<serde_json::Value, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    field.validate()?;
    match *query {
        TextQuery::FirstCharInParagraph { char_index } => Ok(serde_json::json!(
            paragraph_range(&field.text, char_index).map_or(-1, |(start, _)| start as i32)
        )),
        TextQuery::ParagraphLength { char_index } => Ok(serde_json::json!(
            paragraph_range(&field.text, char_index).map_or(-1, |(_, len)| len as i32)
        )),
        TextQuery::LineIndexAtPoint { x, y } => {
            if !x.is_finite() || !y.is_finite() || field.text.is_empty() {
                return Ok(serde_json::json!(-1));
            }
            if let Some(box_size) = field.box_size
                && (x < 0.0 || y < 0.0 || x > box_size.width || y > box_size.height)
            {
                return Ok(serde_json::json!(-1));
            }
            let layout = layout_text(field, &mut measure)?;
            let row_advance = layout.line_height.max(0.0);
            for (idx, line) in layout.lines.iter().enumerate() {
                let bottom = line.y + row_advance.max(line.height);
                let left = field.box_size.map_or(line.x, |_| 2.0_f64.min(line.x));
                let right = field.box_size.map_or(line.x + line.width, |b| {
                    (b.width - 2.0).max(line.x + line.width)
                });
                if y >= line.y && y < bottom && x >= left && x <= right {
                    return Ok(serde_json::json!(idx as i32));
                }
            }
            Ok(serde_json::json!(-1))
        }
        TextQuery::CharBoundaries { char_index } => {
            let total_utf16 = field.text.encode_utf16().count() as i64;
            if char_index < 0 || char_index >= total_utf16 {
                return Ok(serde_json::Value::Null);
            }
            let target = char_index as u32;
            let layout = layout_text(field, &mut measure)?;
            let Some(line) = layout
                .lines
                .iter()
                .find(|line| target >= line.start && target < line.end)
            else {
                return Ok(serde_json::Value::Null);
            };
            let spans = line_char_spans(field, line, &mut measure)?;
            if let Some(span) = spans
                .iter()
                .find(|span| target >= span.start && target < span.end)
            {
                return Ok(serde_json::json!(TextCharBox {
                    x: span.glyph_x,
                    y: line.y,
                    width: span.glyph_width,
                    height: line.height,
                }));
            }
            // Trailing newline character on a multiline line.
            Ok(serde_json::json!(TextCharBox {
                x: line.x + line.width,
                y: line.y,
                width: 0.0,
                height: line.height,
            }))
        }
        TextQuery::CharIndexAtPoint { x, y } => {
            if !x.is_finite() || !y.is_finite() || field.text.is_empty() {
                return Ok(serde_json::json!(-1));
            }
            if let Some(box_size) = field.box_size
                && (x < 0.0 || y < 0.0 || x > box_size.width || y > box_size.height)
            {
                return Ok(serde_json::json!(-1));
            }
            let layout = layout_text(field, &mut measure)?;
            let row_advance = layout.line_height.max(0.0);
            for line in &layout.lines {
                let bottom = line.y + row_advance.max(line.height);
                if y >= line.y && y < bottom {
                    let spans = line_char_spans(field, line, &mut measure)?;
                    for (idx, span) in spans.iter().enumerate() {
                        let hit = if idx + 1 == spans.len() {
                            x >= span.cell_left && x <= span.cell_right
                        } else {
                            x >= span.cell_left && x < span.cell_right
                        };
                        if hit {
                            return Ok(serde_json::json!(span.start as i32));
                        }
                    }
                    return Ok(serde_json::json!(-1));
                }
            }
            Ok(serde_json::json!(-1))
        }
        TextQuery::GetFormat { begin, end } => field.merged_format_json(begin, end),
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontPolicy {
    Device,
    #[default]
    Embed,
    Subset,
    Outlines,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortableFontMetrics {
    pub family: String,
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub default_advance: u16,
    pub advances: std::collections::BTreeMap<u32, u16>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub glyphs: std::collections::BTreeMap<u32, Vec<crate::PathCommand>>,
}

impl PortableFontMetrics {
    pub fn builtin(family: &str) -> Self {
        let lower = family.trim().to_ascii_lowercase();
        let is_mono = lower.contains("mono") || lower.contains("courier") || lower.contains("code");
        let is_serif = !is_mono
            && (lower.contains("serif") && !lower.contains("sans")
                || lower.contains("times")
                || lower.contains("georgia"));
        let mut advances = std::collections::BTreeMap::new();
        for cp in 32u32..=126 {
            let ch = char::from_u32(cp).unwrap_or(' ');
            let adv = if is_mono {
                600
            } else {
                match ch {
                    ' ' => 278,
                    '!' | '\'' | '|' | 'i' | 'l' | 'I' | '.' | ',' | ':' | ';' => {
                        if is_serif {
                            278
                        } else {
                            250
                        }
                    }
                    'j' | 'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '`' => {
                        if is_serif {
                            333
                        } else {
                            310
                        }
                    }
                    's' | 'c' | 'z' | 'v' | 'x' | 'y' | 'k' | 'J' => 500,
                    'a' | 'b' | 'd' | 'e' | 'g' | 'h' | 'n' | 'o' | 'p' | 'q' | 'u' | '0'..='9' => {
                        556
                    }
                    'm' | 'w' => 833,
                    'M' | 'W' => 889,
                    'A'..='Z' => {
                        if is_serif {
                            667
                        } else {
                            680
                        }
                    }
                    '@' | '%' => 889,
                    _ => 556,
                }
            };
            advances.insert(cp, adv);
        }
        Self {
            family: family.to_string(),
            units_per_em: 1000,
            ascent: 800,
            descent: -200,
            line_gap: 200,
            default_advance: if is_mono { 600 } else { 580 },
            advances,
            glyphs: std::collections::BTreeMap::new(),
        }
    }

    pub fn from_bytes(family: &str, bytes: &[u8]) -> Result<Self, TextError> {
        if bytes.starts_with(b"NFPM") && bytes.len() >= 16 {
            let units_per_em = u16::from_be_bytes([bytes[4], bytes[5]]).max(1);
            let ascent = i16::from_be_bytes([bytes[6], bytes[7]]);
            let descent = i16::from_be_bytes([bytes[8], bytes[9]]);
            let line_gap = i16::from_be_bytes([bytes[10], bytes[11]]);
            let count = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
            let mut advances = std::collections::BTreeMap::new();
            let mut pos = 16usize;
            for _ in 0..count {
                if pos + 6 > bytes.len() {
                    break;
                }
                let cp = u32::from_be_bytes([
                    bytes[pos],
                    bytes[pos + 1],
                    bytes[pos + 2],
                    bytes[pos + 3],
                ]);
                let adv = u16::from_be_bytes([bytes[pos + 4], bytes[pos + 5]]);
                advances.insert(cp, adv);
                pos += 6;
            }
            return Ok(Self {
                family: family.to_string(),
                units_per_em,
                ascent,
                descent,
                line_gap,
                default_advance: (units_per_em * 3) / 5,
                advances,
                glyphs: std::collections::BTreeMap::new(),
            });
        }
        if bytes.len() >= 12 {
            let tag = &bytes[0..4];
            if (tag == [0, 1, 0, 0] || tag == b"OTTO" || tag == b"true" || tag == b"wOFF")
                && let Some(parsed) = parse_sfnt_metrics(family, bytes)
            {
                return Ok(parsed);
            }
        }
        Ok(Self::builtin(family))
    }

    pub fn advance_for_char(&self, ch: char) -> u16 {
        let cp = ch as u32;
        if let Some(&adv) = self.advances.get(&cp) {
            return adv;
        }
        if (0x2E80..=0x9FFF).contains(&cp) {
            return self.units_per_em;
        }
        self.default_advance
    }

    pub fn measure_string(&self, text: &str, format: &TextFormat) -> f64 {
        let upm = f64::from(self.units_per_em.max(1));
        let scale = format.size.max(1.0) / upm;
        let bold_factor = if format.bold { 1.06 } else { 1.0 };
        let mut total = 0.0;
        let mut count = 0usize;
        for ch in text.chars() {
            if ch == '\r' || ch == '\n' {
                continue;
            }
            total += f64::from(self.advance_for_char(ch)) * scale * bold_factor;
            count += 1;
        }
        if count > 1 && format.letter_spacing.is_finite() {
            total += (count.saturating_sub(1) as f64) * format.letter_spacing;
        }
        total.max(0.0)
    }

    pub fn measure_strings(&self, strings: &[String], format: &TextFormat) -> Vec<f64> {
        strings
            .iter()
            .map(|s| self.measure_string(s, format))
            .collect()
    }

    pub fn subset(&self, codepoints: &std::collections::BTreeSet<u32>) -> Self {
        let mut advances = std::collections::BTreeMap::new();
        let mut glyphs = std::collections::BTreeMap::new();
        for &cp in codepoints.iter().chain([0u32, 32u32].iter()) {
            let ch = char::from_u32(cp).unwrap_or(' ');
            let adv = self
                .advances
                .get(&cp)
                .copied()
                .unwrap_or_else(|| self.advance_for_char(ch));
            advances.insert(cp, adv);
            if let Some(g) = self.glyphs.get(&cp) {
                glyphs.insert(cp, g.clone());
            }
        }
        Self {
            family: self.family.clone(),
            units_per_em: self.units_per_em,
            ascent: self.ascent,
            descent: self.descent,
            line_gap: self.line_gap,
            default_advance: self.default_advance,
            advances,
            glyphs,
        }
    }

    pub fn encode_subset_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.advances.len() * 6);
        out.extend_from_slice(b"NFPM");
        out.extend_from_slice(&self.units_per_em.to_be_bytes());
        out.extend_from_slice(&self.ascent.to_be_bytes());
        out.extend_from_slice(&self.descent.to_be_bytes());
        out.extend_from_slice(&self.line_gap.to_be_bytes());
        out.extend_from_slice(&(self.advances.len() as u32).to_be_bytes());
        for (&cp, &adv) in &self.advances {
            out.extend_from_slice(&cp.to_be_bytes());
            out.extend_from_slice(&adv.to_be_bytes());
        }
        out
    }

    pub fn glyph_outline(&self, ch: char, x: f64, y: f64, size: f64) -> Vec<crate::PathCommand> {
        if ch.is_whitespace() {
            return Vec::new();
        }
        let cp = ch as u32;
        if let Some(contours) = self.glyphs.get(&cp)
            && !contours.is_empty()
        {
            let upm = f64::from(self.units_per_em.max(1));
            let scale = size.max(1.0) / upm;
            let baseline = y + f64::from(self.ascent.max(1)) * scale;
            return contours
                .iter()
                .map(|cmd| match *cmd {
                    crate::PathCommand::MoveTo { x: gx, y: gy } => crate::PathCommand::MoveTo {
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::LineTo { x: gx, y: gy } => crate::PathCommand::LineTo {
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::QuadraticTo {
                        cx,
                        cy,
                        x: gx,
                        y: gy,
                    } => crate::PathCommand::QuadraticTo {
                        cx: x + cx * scale,
                        cy: baseline - cy * scale,
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::CubicTo {
                        cx1,
                        cy1,
                        cx2,
                        cy2,
                        x: gx,
                        y: gy,
                    } => crate::PathCommand::CubicTo {
                        cx1: x + cx1 * scale,
                        cy1: baseline - cy1 * scale,
                        cx2: x + cx2 * scale,
                        cy2: baseline - cy2 * scale,
                        x: x + gx * scale,
                        y: baseline - gy * scale,
                    },
                    crate::PathCommand::Close => crate::PathCommand::Close,
                })
                .collect();
        }
        synthesize_glyph_outline(ch, x, y, size, self.advance_for_char(ch), self.units_per_em)
    }
}

fn parse_sfnt_metrics(family: &str, bytes: &[u8]) -> Option<PortableFontMetrics> {
    if bytes.len() < 12 {
        return None;
    }
    let is_woff = &bytes[0..4] == b"wOFF";
    let num_tables = if is_woff {
        if bytes.len() < 44 {
            return None;
        }
        u16::from_be_bytes([bytes[12], bytes[13]]) as usize
    } else {
        u16::from_be_bytes([bytes[4], bytes[5]]) as usize
    };
    let mut tables = std::collections::HashMap::<[u8; 4], (usize, usize)>::new();
    for i in 0..num_tables.min(128) {
        if is_woff {
            let entry = 44 + i * 20;
            if entry + 20 > bytes.len() {
                break;
            }
            let tag = [
                bytes[entry],
                bytes[entry + 1],
                bytes[entry + 2],
                bytes[entry + 3],
            ];
            let offset = u32::from_be_bytes([
                bytes[entry + 4],
                bytes[entry + 5],
                bytes[entry + 6],
                bytes[entry + 7],
            ]) as usize;
            let comp_len = u32::from_be_bytes([
                bytes[entry + 8],
                bytes[entry + 9],
                bytes[entry + 10],
                bytes[entry + 11],
            ]) as usize;
            let orig_len = u32::from_be_bytes([
                bytes[entry + 12],
                bytes[entry + 13],
                bytes[entry + 14],
                bytes[entry + 15],
            ]) as usize;
            if comp_len == orig_len && offset.saturating_add(comp_len) <= bytes.len() {
                tables.insert(tag, (offset, comp_len));
            }
        } else {
            let entry = 12 + i * 16;
            if entry + 16 > bytes.len() {
                break;
            }
            let tag = [
                bytes[entry],
                bytes[entry + 1],
                bytes[entry + 2],
                bytes[entry + 3],
            ];
            let offset = u32::from_be_bytes([
                bytes[entry + 8],
                bytes[entry + 9],
                bytes[entry + 10],
                bytes[entry + 11],
            ]) as usize;
            let length = u32::from_be_bytes([
                bytes[entry + 12],
                bytes[entry + 13],
                bytes[entry + 14],
                bytes[entry + 15],
            ]) as usize;
            if offset.saturating_add(length) <= bytes.len() {
                tables.insert(tag, (offset, length));
            }
        }
    }
    let &(head_off, head_len) = tables.get(b"head")?;
    if head_len < 54 {
        return None;
    }
    let head = &bytes[head_off..head_off + head_len];
    let units_per_em = u16::from_be_bytes([head[18], head[19]]).max(16);
    let index_to_loc_format = i16::from_be_bytes([head[50], head[51]]);

    let mut ascent = ((units_per_em as i32 * 4) / 5) as i16;
    let mut descent = -((units_per_em as i32) / 5) as i16;
    let mut line_gap = ((units_per_em as i32) / 5) as i16;
    let mut num_h_metrics = 1usize;
    if let Some(&(hhea_off, hhea_len)) = tables.get(b"hhea")
        && hhea_len >= 36
    {
        let hhea = &bytes[hhea_off..hhea_off + hhea_len];
        ascent = i16::from_be_bytes([hhea[4], hhea[5]]);
        descent = i16::from_be_bytes([hhea[6], hhea[7]]);
        line_gap = i16::from_be_bytes([hhea[8], hhea[9]]);
        num_h_metrics = u16::from_be_bytes([hhea[34], hhea[35]]).max(1) as usize;
    }
    let mut num_glyphs = num_h_metrics;
    if let Some(&(maxp_off, maxp_len)) = tables.get(b"maxp")
        && maxp_len >= 6
    {
        let maxp = &bytes[maxp_off..maxp_off + maxp_len];
        num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]).max(1) as usize;
    }

    let mut glyph_advances = Vec::with_capacity(num_glyphs);
    if let Some(&(hmtx_off, hmtx_len)) = tables.get(b"hmtx") {
        let hmtx = &bytes[hmtx_off..hmtx_off + hmtx_len];
        let mut last_adv = (units_per_em * 3) / 5;
        for g in 0..num_glyphs.min(65535) {
            if g < num_h_metrics && (g + 1) * 4 <= hmtx.len() {
                last_adv = u16::from_be_bytes([hmtx[g * 4], hmtx[g * 4 + 1]]);
            }
            glyph_advances.push(last_adv);
        }
    }

    let mut cmap_map = std::collections::BTreeMap::<u32, u16>::new();
    if let Some(&(cmap_off, cmap_len)) = tables.get(b"cmap")
        && cmap_len >= 4
    {
        let cmap = &bytes[cmap_off..cmap_off + cmap_len];
        let subtables = u16::from_be_bytes([cmap[2], cmap[3]]) as usize;
        for i in 0..subtables.min(32) {
            let rec = 4 + i * 8;
            if rec + 8 > cmap.len() {
                break;
            }
            let sub_off =
                u32::from_be_bytes([cmap[rec + 4], cmap[rec + 5], cmap[rec + 6], cmap[rec + 7]])
                    as usize;
            if sub_off + 4 > cmap.len() {
                continue;
            }
            let fmt = u16::from_be_bytes([cmap[sub_off], cmap[sub_off + 1]]);
            if fmt == 4 && sub_off + 14 <= cmap.len() {
                let seg_count_x2 =
                    u16::from_be_bytes([cmap[sub_off + 6], cmap[sub_off + 7]]) as usize;
                let seg_count = seg_count_x2 / 2;
                let end_codes = sub_off + 14;
                let start_codes = end_codes + seg_count_x2 + 2;
                let id_deltas = start_codes + seg_count_x2;
                let id_range_offsets = id_deltas + seg_count_x2;
                if id_range_offsets + seg_count_x2 <= cmap.len() {
                    for s in 0..seg_count {
                        let end_c = u16::from_be_bytes([
                            cmap[end_codes + s * 2],
                            cmap[end_codes + s * 2 + 1],
                        ]);
                        let start_c = u16::from_be_bytes([
                            cmap[start_codes + s * 2],
                            cmap[start_codes + s * 2 + 1],
                        ]);
                        if start_c == 0xFFFF {
                            break;
                        }
                        let delta = i16::from_be_bytes([
                            cmap[id_deltas + s * 2],
                            cmap[id_deltas + s * 2 + 1],
                        ]) as i32;
                        let ro_pos = id_range_offsets + s * 2;
                        let ro = u16::from_be_bytes([cmap[ro_pos], cmap[ro_pos + 1]]) as usize;
                        for c in start_c..=end_c {
                            let gid = if ro == 0 {
                                ((c as i32 + delta) & 0xFFFF) as u16
                            } else {
                                let glyph_idx_off = ro_pos + ro + ((c - start_c) as usize) * 2;
                                if glyph_idx_off + 2 <= cmap.len() {
                                    let raw = u16::from_be_bytes([
                                        cmap[glyph_idx_off],
                                        cmap[glyph_idx_off + 1],
                                    ]);
                                    if raw == 0 {
                                        0
                                    } else {
                                        ((raw as i32 + delta) & 0xFFFF) as u16
                                    }
                                } else {
                                    0
                                }
                            };
                            if gid > 0 {
                                cmap_map.insert(u32::from(c), gid);
                            }
                        }
                    }
                }
            } else if fmt == 12 && sub_off + 16 <= cmap.len() {
                let n_groups = u32::from_be_bytes([
                    cmap[sub_off + 12],
                    cmap[sub_off + 13],
                    cmap[sub_off + 14],
                    cmap[sub_off + 15],
                ]) as usize;
                for g in 0..n_groups.min(4096) {
                    let grp = sub_off + 16 + g * 12;
                    if grp + 12 > cmap.len() {
                        break;
                    }
                    let start_c = u32::from_be_bytes([
                        cmap[grp],
                        cmap[grp + 1],
                        cmap[grp + 2],
                        cmap[grp + 3],
                    ]);
                    let end_c = u32::from_be_bytes([
                        cmap[grp + 4],
                        cmap[grp + 5],
                        cmap[grp + 6],
                        cmap[grp + 7],
                    ]);
                    let start_gid = u32::from_be_bytes([
                        cmap[grp + 8],
                        cmap[grp + 9],
                        cmap[grp + 10],
                        cmap[grp + 11],
                    ]);
                    if end_c.saturating_sub(start_c) <= 1024 {
                        for c in start_c..=end_c {
                            let gid = (start_gid + (c - start_c)) as u16;
                            if gid > 0 {
                                cmap_map.insert(c, gid);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut base = PortableFontMetrics::builtin(family);
    base.units_per_em = units_per_em;
    base.ascent = ascent;
    base.descent = descent;
    base.line_gap = line_gap;
    base.default_advance = glyph_advances
        .first()
        .copied()
        .unwrap_or((units_per_em * 3) / 5);
    for (&cp, &gid) in &cmap_map {
        if let Some(&adv) = glyph_advances.get(gid as usize) {
            base.advances.insert(cp, adv);
        }
    }

    if let (Some(&(loca_off, loca_len)), Some(&(glyf_off, glyf_len))) =
        (tables.get(b"loca"), tables.get(b"glyf"))
    {
        let loca = &bytes[loca_off..loca_off + loca_len];
        let glyf = &bytes[glyf_off..glyf_off + glyf_len];
        for (&cp, &gid) in &cmap_map {
            let g = gid as usize;
            let (g_start, g_end) = if index_to_loc_format == 0 {
                if (g + 2) * 2 > loca.len() {
                    continue;
                }
                (
                    (u16::from_be_bytes([loca[g * 2], loca[g * 2 + 1]]) as usize) * 2,
                    (u16::from_be_bytes([loca[(g + 1) * 2], loca[(g + 1) * 2 + 1]]) as usize) * 2,
                )
            } else {
                if (g + 2) * 4 > loca.len() {
                    continue;
                }
                (
                    u32::from_be_bytes([
                        loca[g * 4],
                        loca[g * 4 + 1],
                        loca[g * 4 + 2],
                        loca[g * 4 + 3],
                    ]) as usize,
                    u32::from_be_bytes([
                        loca[(g + 1) * 4],
                        loca[(g + 1) * 4 + 1],
                        loca[(g + 1) * 4 + 2],
                        loca[(g + 1) * 4 + 3],
                    ]) as usize,
                )
            };
            if g_end > g_start
                && g_end <= glyf.len()
                && let Some(cmds) = parse_simple_glyf_outline(&glyf[g_start..g_end])
            {
                base.glyphs.insert(cp, cmds);
            }
        }
    }

    Some(base)
}

fn parse_simple_glyf_outline(slice: &[u8]) -> Option<Vec<crate::PathCommand>> {
    if slice.len() < 12 {
        return None;
    }
    let num_contours = i16::from_be_bytes([slice[0], slice[1]]);
    if num_contours <= 0 || num_contours > 64 {
        return None;
    }
    let c_count = num_contours as usize;
    let end_pts_end = 10 + c_count * 2;
    if end_pts_end + 2 > slice.len() {
        return None;
    }
    let mut end_pts = Vec::with_capacity(c_count);
    for i in 0..c_count {
        let p = 10 + i * 2;
        end_pts.push(u16::from_be_bytes([slice[p], slice[p + 1]]) as usize);
    }
    let num_points = (*end_pts.last()?) + 1;
    if num_points > 2048 {
        return None;
    }
    let insn_len = u16::from_be_bytes([slice[end_pts_end], slice[end_pts_end + 1]]) as usize;
    let mut pos = end_pts_end + 2 + insn_len;
    if pos > slice.len() {
        return None;
    }
    let mut flags = Vec::with_capacity(num_points);
    while flags.len() < num_points && pos < slice.len() {
        let f = slice[pos];
        pos += 1;
        flags.push(f);
        if (f & 0x08) != 0 && pos < slice.len() {
            let repeat = slice[pos] as usize;
            pos += 1;
            for _ in 0..repeat.min(num_points - flags.len()) {
                flags.push(f);
            }
        }
    }
    if flags.len() != num_points {
        return None;
    }
    let mut xs = Vec::with_capacity(num_points);
    let mut cur_x = 0i32;
    for &f in &flags {
        if (f & 0x02) != 0 {
            if pos >= slice.len() {
                return None;
            }
            let dx = slice[pos] as i32;
            pos += 1;
            cur_x += if (f & 0x10) != 0 { dx } else { -dx };
        } else if (f & 0x10) == 0 {
            if pos + 2 > slice.len() {
                return None;
            }
            cur_x += i16::from_be_bytes([slice[pos], slice[pos + 1]]) as i32;
            pos += 2;
        }
        xs.push(cur_x as f64);
    }
    let mut ys = Vec::with_capacity(num_points);
    let mut cur_y = 0i32;
    for &f in &flags {
        if (f & 0x04) != 0 {
            if pos >= slice.len() {
                return None;
            }
            let dy = slice[pos] as i32;
            pos += 1;
            cur_y += if (f & 0x20) != 0 { dy } else { -dy };
        } else if (f & 0x20) == 0 {
            if pos + 2 > slice.len() {
                return None;
            }
            cur_y += i16::from_be_bytes([slice[pos], slice[pos + 1]]) as i32;
            pos += 2;
        }
        ys.push(cur_y as f64);
    }
    let mut cmds = Vec::new();
    let mut start = 0usize;
    for &end in &end_pts {
        if end < start || end >= num_points {
            break;
        }
        let len = end - start + 1;
        if len >= 2 {
            let x0 = xs[start];
            let y0 = ys[start];
            cmds.push(crate::PathCommand::MoveTo { x: x0, y: y0 });
            let mut i = 1usize;
            while i < len {
                let idx = start + i;
                let on_curve = (flags[idx] & 0x01) != 0;
                if on_curve {
                    cmds.push(crate::PathCommand::LineTo {
                        x: xs[idx],
                        y: ys[idx],
                    });
                    i += 1;
                } else {
                    let next_idx = start + ((i + 1) % len);
                    let next_on = (flags[next_idx] & 0x01) != 0;
                    if next_on {
                        cmds.push(crate::PathCommand::QuadraticTo {
                            cx: xs[idx],
                            cy: ys[idx],
                            x: xs[next_idx],
                            y: ys[next_idx],
                        });
                        i += 2;
                    } else {
                        let mx = (xs[idx] + xs[next_idx]) * 0.5;
                        let my = (ys[idx] + ys[next_idx]) * 0.5;
                        cmds.push(crate::PathCommand::QuadraticTo {
                            cx: xs[idx],
                            cy: ys[idx],
                            x: mx,
                            y: my,
                        });
                        i += 1;
                    }
                }
            }
            cmds.push(crate::PathCommand::Close);
        }
        start = end + 1;
    }
    (!cmds.is_empty()).then_some(cmds)
}

fn synthesize_glyph_outline(
    ch: char,
    x: f64,
    y: f64,
    size: f64,
    advance: u16,
    units_per_em: u16,
) -> Vec<crate::PathCommand> {
    if ch.is_whitespace() {
        return Vec::new();
    }
    let upm = f64::from(units_per_em.max(1));
    let w = (f64::from(advance) / upm * size).max(size * 0.25);
    let h = size.max(1.0);
    let left = x + w * 0.1;
    let right = x + w * 0.9;
    let top = y + h * 0.15;
    let bottom = y + h * 0.88;
    let mid_x = (left + right) * 0.5;
    let mid_y = (top + bottom) * 0.5;
    let stroke = (h * 0.12).max(0.8);
    let mut cmds = Vec::new();
    let add_rect = |cmds: &mut Vec<crate::PathCommand>, rx: f64, ry: f64, rw: f64, rh: f64| {
        if rw <= 0.0 || rh <= 0.0 {
            return;
        }
        cmds.push(crate::PathCommand::MoveTo { x: rx, y: ry });
        cmds.push(crate::PathCommand::LineTo { x: rx + rw, y: ry });
        cmds.push(crate::PathCommand::LineTo {
            x: rx + rw,
            y: ry + rh,
        });
        cmds.push(crate::PathCommand::LineTo { x: rx, y: ry + rh });
        cmds.push(crate::PathCommand::Close);
    };
    match ch {
        '.' | ',' => add_rect(
            &mut cmds,
            mid_x - stroke * 0.6,
            bottom - stroke * 1.2,
            stroke * 1.2,
            stroke * 1.2,
        ),
        '-' | '_' => {
            let bar_y = if ch == '_' {
                bottom - stroke
            } else {
                mid_y - stroke * 0.5
            };
            add_rect(&mut cmds, left, bar_y, (right - left).max(stroke), stroke);
        }
        'I' | 'i' | 'l' | '|' | '!' => {
            add_rect(
                &mut cmds,
                mid_x - stroke * 0.5,
                top,
                stroke,
                (bottom - top).max(stroke),
            );
        }
        'L' => {
            add_rect(&mut cmds, left, top, stroke, (bottom - top).max(stroke));
            add_rect(
                &mut cmds,
                left,
                bottom - stroke,
                (right - left).max(stroke),
                stroke,
            );
        }
        'T' => {
            add_rect(&mut cmds, left, top, (right - left).max(stroke), stroke);
            add_rect(
                &mut cmds,
                mid_x - stroke * 0.5,
                top,
                stroke,
                (bottom - top).max(stroke),
            );
        }
        'H' => {
            add_rect(&mut cmds, left, top, stroke, (bottom - top).max(stroke));
            add_rect(
                &mut cmds,
                right - stroke,
                top,
                stroke,
                (bottom - top).max(stroke),
            );
            add_rect(
                &mut cmds,
                left,
                mid_y - stroke * 0.5,
                (right - left).max(stroke),
                stroke,
            );
        }
        'E' | 'F' => {
            add_rect(&mut cmds, left, top, stroke, (bottom - top).max(stroke));
            add_rect(&mut cmds, left, top, (right - left).max(stroke), stroke);
            add_rect(
                &mut cmds,
                left,
                mid_y - stroke * 0.5,
                (right - left) * 0.8,
                stroke,
            );
            if ch == 'E' {
                add_rect(
                    &mut cmds,
                    left,
                    bottom - stroke,
                    (right - left).max(stroke),
                    stroke,
                );
            }
        }
        'O' | '0' | 'o' => {
            let t = if ch == 'o' {
                mid_y - (bottom - top) * 0.25
            } else {
                top
            };
            cmds.push(crate::PathCommand::MoveTo { x: mid_x, y: t });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: right,
                cy: t,
                x: right,
                y: (t + bottom) * 0.5,
            });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: right,
                cy: bottom,
                x: mid_x,
                y: bottom,
            });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: left,
                cy: bottom,
                x: left,
                y: (t + bottom) * 0.5,
            });
            cmds.push(crate::PathCommand::QuadraticTo {
                cx: left,
                cy: t,
                x: mid_x,
                y: t,
            });
            cmds.push(crate::PathCommand::Close);
        }
        _ => {
            let is_lower = ch.is_lowercase();
            let g_top = if is_lower {
                top + (bottom - top) * 0.22
            } else {
                top
            };
            add_rect(&mut cmds, left, g_top, (right - left).max(stroke), stroke);
            add_rect(&mut cmds, left, g_top, stroke, (bottom - g_top).max(stroke));
            add_rect(
                &mut cmds,
                right - stroke,
                g_top,
                stroke,
                (bottom - g_top).max(stroke),
            );
            if (ch as u32).is_multiple_of(2) {
                add_rect(
                    &mut cmds,
                    left,
                    bottom - stroke,
                    (right - left).max(stroke),
                    stroke,
                );
            } else {
                add_rect(
                    &mut cmds,
                    left,
                    (g_top + bottom) * 0.5,
                    (right - left).max(stroke),
                    stroke,
                );
            }
        }
    }
    cmds
}

impl TextFormat {
    pub fn portable_measure_strings(
        strings: &[String],
        format: &TextFormat,
        font_bytes: Option<&[u8]>,
    ) -> Vec<f64> {
        let metrics = font_bytes
            .and_then(|bytes| PortableFontMetrics::from_bytes(&format.font, bytes).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(&format.font));
        metrics.measure_strings(strings, format)
    }

    pub fn portable_font_metrics_json(
        family: &str,
        font_bytes: Option<&[u8]>,
    ) -> Result<String, TextError> {
        let metrics = match font_bytes {
            Some(bytes) => PortableFontMetrics::from_bytes(family, bytes)?,
            None => PortableFontMetrics::builtin(family),
        };
        serde_json::to_string(&metrics).map_err(|e| TextError(e.to_string()))
    }

    pub fn subset_font_bytes(
        bytes: &[u8],
        codepoints: &std::collections::BTreeSet<u32>,
    ) -> Result<Vec<u8>, TextError> {
        let metrics = PortableFontMetrics::from_bytes("SubsetFont", bytes)?;
        Ok(metrics.subset(codepoints).encode_subset_binary())
    }

    pub fn subset_font_codepoints(bytes: &[u8]) -> Vec<u32> {
        PortableFontMetrics::from_bytes("SubsetFont", bytes)
            .map(|m| m.advances.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn glyph_outline_commands(
        ch: char,
        x: f64,
        y: f64,
        font_family: &str,
        font_size: f64,
        font_bytes: Option<&[u8]>,
    ) -> Vec<crate::PathCommand> {
        let metrics = font_bytes
            .and_then(|b| PortableFontMetrics::from_bytes(font_family, b).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(font_family));
        metrics.glyph_outline(ch, x, y, font_size)
    }
}

impl TextFieldState {
    pub fn layout_portable(&self, font_bytes: Option<&[u8]>) -> Result<TextLayout, TextError> {
        layout_text(self, |strings, fmt| {
            Ok(TextFormat::portable_measure_strings(
                strings, fmt, font_bytes,
            ))
        })
    }

    pub fn to_outline_commands(&self, font_bytes: Option<&[u8]>) -> Vec<crate::PathCommand> {
        let Ok(boxes) = layout_char_boxes(self, |strings, fmt| {
            Ok(TextFormat::portable_measure_strings(
                strings, fmt, font_bytes,
            ))
        }) else {
            return Vec::new();
        };
        let metrics = font_bytes
            .and_then(|b| PortableFontMetrics::from_bytes(&self.format.font, b).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(&self.format.font));
        let mut out = Vec::new();
        for (ch, char_box) in boxes {
            out.extend(metrics.glyph_outline(ch, char_box.x, char_box.y, self.format.size));
        }
        out
    }

    pub fn static_text_to_outline_commands(
        text: &str,
        font_family: &str,
        font_size: f64,
        font_bytes: Option<&[u8]>,
    ) -> Vec<crate::PathCommand> {
        let metrics = font_bytes
            .and_then(|b| PortableFontMetrics::from_bytes(font_family, b).ok())
            .unwrap_or_else(|| PortableFontMetrics::builtin(font_family));
        let upm = f64::from(metrics.units_per_em.max(1));
        let scale = font_size.max(1.0) / upm;
        let line_step = font_size.max(1.0) * 1.2;
        let mut out = Vec::new();
        for (line_idx, line) in text.split('\n').enumerate() {
            let mut cursor_x = 0.0;
            let line_y = (line_idx as f64) * line_step;
            for ch in line.chars() {
                if ch == '\r' {
                    continue;
                }
                out.extend(metrics.glyph_outline(ch, cursor_x, line_y, font_size));
                cursor_x += f64::from(metrics.advance_for_char(ch)) * scale;
            }
        }
        out
    }

    pub fn apply_auto_size(
        &mut self,
        mode: &str,
        x: &mut f64,
        layout: &TextLayout,
    ) -> Result<bool, TextError> {
        let normalized = match mode.trim().to_ascii_lowercase().as_str() {
            "left" | "true" => "left",
            "center" => "center",
            "right" => "right",
            _ => "none",
        };
        self.as2_viewport.auto_size = normalized.to_string();
        if normalized == "none" {
            return Ok(false);
        }
        let old = self.box_size.unwrap_or(TextBoxSize {
            width: 100.0,
            height: 20.0,
        });
        let new_width = if self.word_wrap {
            old.width
        } else {
            (layout.text_width
                + self.format.left_margin
                + self.format.right_margin
                + self.format.block_indent
                + self.format.indent.max(0.0)
                + if self.format.bullet {
                    self.format.size
                } else {
                    0.0
                }
                + 4.0)
                .max(4.0)
        };
        let new_height = (layout.text_height + 4.0).max(4.0);
        if (new_width - old.width).abs() < 1e-9 && (new_height - old.height).abs() < 1e-9 {
            return Ok(false);
        }
        let delta = (old.width - new_width)
            * match normalized {
                "center" => 0.5,
                "right" => 1.0,
                _ => 0.0,
            };
        *x += delta;
        self.box_size = Some(TextBoxSize {
            width: new_width,
            height: new_height,
        });
        self.revision = self
            .revision
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_add(1)
            .to_string();
        Ok(true)
    }

    pub fn parse_html(
        source: &str,
        base_format: &TextFormat,
        css_styles: Option<
            &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
        >,
        condense_white: bool,
    ) -> Result<Self, TextError> {
        let mut field = Self {
            format: base_format.clone(),
            default_format: base_format.clone(),
            ..Self::default()
        };
        field.set_html_text(source, css_styles, condense_white)?;
        Ok(field)
    }

    pub fn set_html_text(
        &mut self,
        source: &str,
        css_styles: Option<
            &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
        >,
        condense_white: bool,
    ) -> Result<(), TextError> {
        if source.len() > MAX_TEXT_BYTES {
            return Err(TextError("HTML text exceeds maximum byte budget".into()));
        }
        let mut stack: Vec<(String, TextFormat)> =
            vec![("root".into(), self.default_format.clone())];
        let mut plain = String::new();
        let mut char_formats: Vec<TextFormat> = Vec::new();
        let mut chars = source.chars().peekable();
        let mut last_was_space = false;

        let apply_css_props =
            |fmt: &mut TextFormat, props: &std::collections::BTreeMap<String, String>| {
                for (k, v) in props {
                    let key = k.trim().to_ascii_lowercase();
                    let val = v.trim();
                    match key.as_str() {
                        "font-family" | "fontfamily" => {
                            let family = val
                                .split(',')
                                .next()
                                .unwrap_or(val)
                                .trim()
                                .trim_matches(['"', '\'']);
                            if !family.is_empty() {
                                fmt.font = family.to_string();
                            }
                        }
                        "font-size" | "fontsize" => {
                            let num = val.trim_end_matches("px").trim_end_matches("pt").trim();
                            if let Ok(sz) = num.parse::<f64>()
                                && sz.is_finite()
                            {
                                fmt.size = sz.clamp(1.0, 127.0);
                            }
                        }
                        "color" => {
                            if let Some(hex) = val.strip_prefix('#')
                                && let Ok(c) = u32::from_str_radix(hex, 16)
                            {
                                fmt.color = c & 0xFFFFFF;
                            }
                        }
                        "font-weight" | "fontweight" => {
                            fmt.bold = val.eq_ignore_ascii_case("bold")
                                || val == "700"
                                || val == "800"
                                || val == "900";
                        }
                        "font-style" | "fontstyle" => {
                            fmt.italic = val.eq_ignore_ascii_case("italic")
                                || val.eq_ignore_ascii_case("oblique");
                        }
                        "text-decoration" | "textdecoration" => {
                            fmt.underline = val.to_ascii_lowercase().contains("underline");
                        }
                        "text-align" | "textalign" => {
                            let lower = val.to_ascii_lowercase();
                            if matches!(lower.as_str(), "left" | "center" | "right" | "justify") {
                                fmt.set_align_mode(Some(lower.clone()));
                                fmt.align = match lower.as_str() {
                                    "center" => TextAlign::Center,
                                    "right" => TextAlign::Right,
                                    _ => TextAlign::Left,
                                };
                            }
                        }
                        "letter-spacing" | "letterspacing" => {
                            let num = val.trim_end_matches("px").trim();
                            if let Ok(ls) = num.parse::<f64>()
                                && ls.is_finite()
                            {
                                fmt.letter_spacing = ls.clamp(-60.0, 60.0);
                            }
                        }
                        _ => {}
                    }
                }
            };

        while let Some(ch) = chars.next() {
            if ch == '<' {
                let mut tag_buf = String::new();
                for tc in chars.by_ref() {
                    if tc == '>' {
                        break;
                    }
                    tag_buf.push(tc);
                }
                let trimmed = tag_buf.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(closing) = trimmed.strip_prefix('/') {
                    let close_name = closing.trim().to_ascii_lowercase();
                    if matches!(close_name.as_str(), "p" | "li" | "div")
                        && !plain.is_empty()
                        && !plain.ends_with('\n')
                    {
                        plain.push('\n');
                        let cur_fmt = stack
                            .last()
                            .map(|(_, f)| f.clone())
                            .unwrap_or_else(|| self.default_format.clone());
                        for _ in 0..'\n'.len_utf16() {
                            char_formats.push(cur_fmt.clone());
                        }
                        last_was_space = false;
                    }
                    if stack.len() > 1 {
                        if let Some(pos) = stack.iter().rposition(|(t, _)| t == &close_name) {
                            stack.truncate(pos);
                        } else {
                            stack.pop();
                        }
                    }
                    continue;
                }
                let self_closing = trimmed.ends_with('/');
                let content = trimmed.trim_end_matches('/').trim();
                let mut parts = content.splitn(2, char::is_whitespace);
                let tag_name = parts.next().unwrap_or("").to_ascii_lowercase();
                let attr_str = parts.next().unwrap_or("");
                let attrs = parse_html_attributes(attr_str);

                if tag_name == "br" {
                    plain.push('\n');
                    let cur_fmt = stack
                        .last()
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| self.default_format.clone());
                    char_formats.push(cur_fmt);
                    last_was_space = false;
                    continue;
                }
                if matches!(tag_name.as_str(), "p" | "li" | "div")
                    && !plain.is_empty()
                    && !plain.ends_with('\n')
                {
                    plain.push('\n');
                    let cur_fmt = stack
                        .last()
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| self.default_format.clone());
                    char_formats.push(cur_fmt);
                    last_was_space = false;
                }

                let mut next_fmt = stack
                    .last()
                    .map(|(_, f)| f.clone())
                    .unwrap_or_else(|| self.default_format.clone());
                if let Some(styles) = css_styles {
                    if let Some(tag_style) = styles.get(&tag_name) {
                        apply_css_props(&mut next_fmt, tag_style);
                    }
                    if let Some(cls) = attrs.get("class") {
                        for item in cls.split_whitespace() {
                            let dot_cls = format!(".{item}");
                            if let Some(cls_style) =
                                styles.get(&dot_cls).or_else(|| styles.get(item))
                            {
                                apply_css_props(&mut next_fmt, cls_style);
                            }
                        }
                    }
                }
                if let Some(inline_style) = attrs.get("style") {
                    let mut inline_map = std::collections::BTreeMap::new();
                    for decl in inline_style.split(';') {
                        if let Some((k, v)) = decl.split_once(':') {
                            inline_map.insert(k.trim().to_string(), v.trim().to_string());
                        }
                    }
                    apply_css_props(&mut next_fmt, &inline_map);
                }

                match tag_name.as_str() {
                    "b" | "strong" => next_fmt.bold = true,
                    "i" | "em" => next_fmt.italic = true,
                    "u" => next_fmt.underline = true,
                    "li" => next_fmt.bullet = true,
                    "p" | "div" => {
                        if let Some(align) = attrs.get("align") {
                            let lower = align.trim().to_ascii_lowercase();
                            if matches!(lower.as_str(), "left" | "center" | "right" | "justify") {
                                next_fmt.set_align_mode(Some(lower.clone()));
                                next_fmt.align = match lower.as_str() {
                                    "center" => TextAlign::Center,
                                    "right" => TextAlign::Right,
                                    _ => TextAlign::Left,
                                };
                            }
                        }
                    }
                    "font" => {
                        if let Some(face) = attrs.get("face")
                            && !face.trim().is_empty()
                        {
                            next_fmt.font = face.trim().to_string();
                        }
                        if let Some(size_str) = attrs.get("size") {
                            let s = size_str.trim();
                            if let Some(rel) = s.strip_prefix('+') {
                                if let Ok(d) = rel.parse::<f64>() {
                                    next_fmt.size = (next_fmt.size + d).clamp(1.0, 127.0);
                                }
                            } else if let Some(rel) = s.strip_prefix('-') {
                                if let Ok(d) = rel.parse::<f64>() {
                                    next_fmt.size = (next_fmt.size - d).clamp(1.0, 127.0);
                                }
                            } else if let Ok(abs) = s.parse::<f64>() {
                                next_fmt.size = abs.clamp(1.0, 127.0);
                            }
                        }
                        if let Some(col_str) = attrs.get("color")
                            && let Some(hex) = col_str.trim().strip_prefix('#')
                            && let Ok(c) = u32::from_str_radix(hex, 16)
                        {
                            next_fmt.color = c & 0xFFFFFF;
                        }
                        if let Some(ls_str) = attrs.get("letterspacing")
                            && let Ok(ls) = ls_str.trim().parse::<f64>()
                            && ls.is_finite()
                        {
                            next_fmt.letter_spacing = ls.clamp(-60.0, 60.0);
                        }
                    }
                    "a" => {
                        if let Some(href) = attrs.get("href") {
                            next_fmt.url = href.clone();
                        }
                        if let Some(target) = attrs.get("target") {
                            next_fmt.set_target_str(target.clone());
                        }
                    }
                    _ => {}
                }
                if !self_closing {
                    stack.push((tag_name, next_fmt));
                }
            } else if ch == '&' {
                let mut ent = String::new();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc == ';' {
                        break;
                    }
                    ent.push(nc);
                    if ent.len() > 12 {
                        break;
                    }
                }
                let decoded = match ent.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" | "#39" => '\'',
                    "nbsp" => '\u{00A0}',
                    _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                        u32::from_str_radix(&ent[2..], 16)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or('?')
                    }
                    _ if ent.starts_with('#') => ent[1..]
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or('?'),
                    _ => '?',
                };
                let cur_fmt = stack
                    .last()
                    .map(|(_, f)| f.clone())
                    .unwrap_or_else(|| self.default_format.clone());
                plain.push(decoded);
                for _ in 0..decoded.len_utf16() {
                    char_formats.push(cur_fmt.clone());
                }
                last_was_space = false;
            } else {
                if condense_white && ch.is_whitespace() {
                    if last_was_space {
                        continue;
                    }
                    last_was_space = true;
                    let cur_fmt = stack
                        .last()
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| self.default_format.clone());
                    plain.push(' ');
                    char_formats.push(cur_fmt);
                    continue;
                }
                last_was_space = false;
                let cur_fmt = stack
                    .last()
                    .map(|(_, f)| f.clone())
                    .unwrap_or_else(|| self.default_format.clone());
                plain.push(ch);
                for _ in 0..ch.len_utf16() {
                    char_formats.push(cur_fmt.clone());
                }
            }
        }

        if plain.ends_with('\n') && !source.contains("<br") {
            plain.pop();
            char_formats.pop();
        }

        self.text = plain;
        self.format_runs.clear();
        if !char_formats.is_empty() {
            self.format = char_formats[0].clone();
            let mut run_start = 0u32;
            let mut run_fmt = char_formats[0].clone();
            for (idx, fmt) in char_formats.iter().enumerate().skip(1) {
                if *fmt != run_fmt {
                    self.format_runs.push(TextFormatRun {
                        start: run_start,
                        end: idx as u32,
                        format: run_fmt,
                    });
                    run_start = idx as u32;
                    run_fmt = fmt.clone();
                }
            }
            self.format_runs.push(TextFormatRun {
                start: run_start,
                end: char_formats.len() as u32,
                format: run_fmt,
            });
            if self.format_runs.len() == 1 {
                self.format_runs.clear();
            }
        }
        self.selection_begin = clamp_utf16(&self.text, i64::from(self.selection_begin));
        self.selection_end = clamp_utf16(&self.text, i64::from(self.selection_end));
        self.caret = clamp_utf16(&self.text, i64::from(self.caret));
        self.revision = self
            .revision
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_add(1)
            .to_string();
        Ok(())
    }

    pub fn to_html_text(&self) -> String {
        let escape = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        let wrap_span = |text: &str, fmt: &TextFormat| {
            if text.is_empty() {
                return String::new();
            }
            let mut inner = escape(text);
            if fmt.underline {
                inner = format!("<u>{inner}</u>");
            }
            if fmt.italic {
                inner = format!("<i>{inner}</i>");
            }
            if fmt.bold {
                inner = format!("<b>{inner}</b>");
            }
            if !fmt.url.is_empty() {
                let tgt = fmt.target_str();
                let target_attr = if tgt.is_empty() {
                    String::new()
                } else {
                    format!(" target=\"{}\"", escape(tgt))
                };
                inner = format!("<a href=\"{}\"{target_attr}>{inner}</a>", escape(&fmt.url));
            }
            format!(
                "<font face=\"{}\" size=\"{}\" color=\"#{:06x}\">{inner}</font>",
                escape(&fmt.font),
                fmt.size.round() as i32,
                fmt.color & 0xFFFFFF
            )
        };
        let align = self.format.effective_align_mode();
        if self.format_runs.is_empty() {
            let body = self
                .text
                .split('\n')
                .map(|line| format!("<p align=\"{align}\">{}</p>", wrap_span(line, &self.format)))
                .collect::<Vec<_>>()
                .join("");
            return body;
        }
        let mut out = format!("<p align=\"{align}\">");
        for run in &self.format_runs {
            let Ok(sb) = utf16_byte(&self.text, run.start) else {
                continue;
            };
            let Ok(eb) = utf16_byte(&self.text, run.end) else {
                continue;
            };
            let slice = &self.text[sb..eb];
            let mut parts = slice.split('\n').peekable();
            while let Some(part) = parts.next() {
                out.push_str(&wrap_span(part, &run.format));
                if parts.peek().is_some() {
                    let next_align = run.format.effective_align_mode();
                    out.push_str(&format!("</p><p align=\"{next_align}\">"));
                }
            }
        }
        out.push_str("</p>");
        out
    }
}

fn parse_html_attributes(input: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let mut chars = input.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        if chars.peek() != Some(&'=') {
            if !key.is_empty() {
                map.insert(key.to_ascii_lowercase(), String::new());
            }
            continue;
        }
        chars.next(); // consume '='
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        let mut val = String::new();
        if let Some(&quote @ ('"' | '\'')) = chars.peek() {
            chars.next();
            for c in chars.by_ref() {
                if c == quote {
                    break;
                }
                val.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                val.push(c);
                chars.next();
            }
        }
        if !key.is_empty() {
            map.insert(key.to_ascii_lowercase(), val);
        }
    }
    map
}

#[cfg(test)]
mod query_tests {
    use super::*;

    fn fixed_measure(strings: &[String], _format: &TextFormat) -> Result<Vec<f64>, TextError> {
        Ok(strings
            .iter()
            .map(|s| s.chars().count() as f64 * 10.0)
            .collect())
    }

    #[test]
    fn multirun_formats_and_justify_align_modes_work() {
        let mut field = TextFieldState {
            text: "Alpha Beta Gamma".into(),
            multiline: true,
            word_wrap: true,
            box_size: Some(TextBoxSize {
                width: 124.0,
                height: 80.0,
            }),
            ..TextFieldState::default()
        };
        field
            .apply(&TextPatch {
                format: Some(TextFormatPatch {
                    align_mode: Some("justify".into()),
                    ..TextFormatPatch::default()
                }),
                ..TextPatch::default()
            })
            .unwrap();
        let justified = layout_text(&field, fixed_measure).unwrap();
        assert_eq!(justified.lines.len(), 2);
        assert_eq!(justified.lines[0].width, 120.0);
        assert!(justified.runs.len() >= 3);

        field
            .apply(&TextPatch {
                format_range: Some(TextFormatRangePatch {
                    begin: 0,
                    end: 5,
                    format: TextFormatPatch {
                        bold: Some(true),
                        color: Some(0xff0000),
                        display: Some("inline".into()),
                        ..TextFormatPatch::default()
                    },
                }),
                ..TextPatch::default()
            })
            .unwrap();
        assert_eq!(field.format_runs.len(), 2);
        let first_fmt = query_text(
            &field,
            &TextQuery::GetFormat { begin: 0, end: 5 },
            fixed_measure,
        )
        .unwrap();
        assert_eq!(first_fmt["bold"], serde_json::json!(true));
        assert_eq!(first_fmt["color"], serde_json::json!(0xff0000));
        assert_eq!(first_fmt["display"], serde_json::json!("inline"));
        let mixed_fmt = query_text(
            &field,
            &TextQuery::GetFormat { begin: -1, end: -1 },
            fixed_measure,
        )
        .unwrap();
        assert!(mixed_fmt["bold"].is_null());
        assert!(mixed_fmt["color"].is_null());
        assert_eq!(mixed_fmt["font"], serde_json::json!("Times New Roman"));
        let mr_layout = layout_text(&field, fixed_measure).unwrap();
        assert!(mr_layout.runs.iter().any(|r| {
            r.format
                .as_ref()
                .is_some_and(|f| f.bold && f.color == 0xff0000)
        }));
    }

    #[test]
    fn paragraph_and_char_queries_respect_utf16_and_scroll() {
        let mut field = TextFieldState {
            text: "Hello\nWorld!".into(),
            multiline: true,
            box_size: Some(TextBoxSize {
                width: 200.0,
                height: 100.0,
            }),
            ..TextFieldState::default()
        };
        assert_eq!(
            query_text(
                &field,
                &TextQuery::FirstCharInParagraph { char_index: 0 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(0)
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::ParagraphLength { char_index: 4 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(6)
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::FirstCharInParagraph { char_index: 6 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(6)
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::ParagraphLength { char_index: 8 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(6)
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::FirstCharInParagraph { char_index: 99 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(-1)
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::CharBoundaries { char_index: 1 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!({"x": 12.0, "y": 2.0, "width": 10.0, "height": 12.0 * 1.2})
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::CharBoundaries { char_index: -1 },
                fixed_measure
            )
            .unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::CharIndexAtPoint { x: 15.0, y: 8.0 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(1)
        );
        assert_eq!(
            query_text(
                &field,
                &TextQuery::LineIndexAtPoint { x: 15.0, y: 20.0 },
                fixed_measure
            )
            .unwrap(),
            serde_json::json!(1)
        );
        field
            .apply(&TextPatch {
                sharpness: Some(999.0),
                thickness: Some(-500.0),
                anti_alias_type: Some("advanced".into()),
                grid_fit_type: Some("subpixel".into()),
                ..TextPatch::default()
            })
            .unwrap();
        assert_eq!(field.sharpness, 400.0);
        assert_eq!(field.thickness, -200.0);
        assert_eq!(field.anti_alias_type, "advanced");
        assert_eq!(field.grid_fit_type, "subpixel");
    }

    #[test]
    fn portable_font_metrics_subset_and_outline_generation_work() {
        let fmt = TextFormat {
            font: "Arial".into(),
            size: 20.0,
            ..TextFormat::default()
        };
        let widths = TextFormat::portable_measure_strings(
            &["Hello".into(), "i".into(), "W".into()],
            &fmt,
            None,
        );
        assert_eq!(widths.len(), 3);
        assert!(
            widths[1] < widths[2],
            "proportional metrics make 'i' narrower than 'W'"
        );

        let mut used = std::collections::BTreeSet::new();
        used.insert('A' as u32);
        used.insert('B' as u32);
        let subset_bytes = TextFormat::subset_font_bytes(b"\x00\x01\x00\x00", &used).unwrap();
        let cps = TextFormat::subset_font_codepoints(&subset_bytes);
        assert!(cps.contains(&('A' as u32)));
        assert!(cps.contains(&('B' as u32)));
        assert!(!cps.contains(&('Z' as u32)));

        let cmds = TextFieldState::static_text_to_outline_commands("Hi!", "Arial", 24.0, None);
        assert!(!cmds.is_empty());
        assert!(
            cmds.iter()
                .any(|c| matches!(c, crate::PathCommand::MoveTo { .. }))
        );
        assert!(cmds.iter().any(|c| matches!(c, crate::PathCommand::Close)));
    }

    #[test]
    fn html_css_parsing_serialization_and_auto_size_work() {
        let mut css = std::collections::BTreeMap::new();
        let mut highlight = std::collections::BTreeMap::new();
        highlight.insert("color".into(), "#ff0000".into());
        highlight.insert("font-weight".into(), "bold".into());
        css.insert(".highlight".into(), highlight);

        let field = TextFieldState::parse_html(
            "<p align=\"center\">Hello <span class=\"highlight\"><i>World</i></span> <a href=\"https://example.com\">Link</a></p>",
            &TextFormat::default(),
            Some(&css),
            true,
        )
        .unwrap();
        assert_eq!(field.text, "Hello World Link");
        assert!(field.format_runs.len() >= 3);
        assert!(
            field
                .format_runs
                .iter()
                .any(|r| r.format.bold && r.format.italic && r.format.color == 0xff0000)
        );
        assert!(
            field
                .format_runs
                .iter()
                .any(|r| r.format.url == "https://example.com")
        );
        let serialized = field.to_html_text();
        assert!(serialized.contains("<b><i>World</i></b>"));
        assert!(serialized.contains("href=\"https://example.com\""));

        let mut auto_field = TextFieldState {
            text: "AutoSize Center Test".into(),
            box_size: Some(TextBoxSize {
                width: 80.0,
                height: 20.0,
            }),
            ..TextFieldState::default()
        };
        let layout = auto_field.layout_portable(None).unwrap();
        let mut x = 100.0;
        assert!(
            auto_field
                .apply_auto_size("center", &mut x, &layout)
                .unwrap()
        );
        assert!(auto_field.box_size.unwrap().width > 80.0);
        assert!(x < 100.0);
    }
}
