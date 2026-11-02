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
#[serde(default, deny_unknown_fields)]
pub struct TextFormat {
    pub font: String,
    pub size: f64,
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
    pub leading: f64,
    pub align: TextAlign,
    #[serde(rename = "leftMargin")]
    pub left_margin: f64,
    #[serde(rename = "rightMargin")]
    pub right_margin: f64,
    pub indent: f64,
    #[serde(rename = "blockIndent")]
    pub block_indent: f64,
    pub underline: bool,
    #[serde(rename = "letterSpacing")]
    pub letter_spacing: f64,
    pub kerning: Option<bool>,
    pub bullet: bool,
    #[serde(rename = "tabStops")]
    pub tab_stops: Vec<f64>,
    pub url: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "alignMode")]
    pub align_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
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
            align_mode: None,
            display: None,
        }
    }
}
impl TextFormat {
    pub fn effective_align_mode(&self) -> &str {
        if let Some(mode) = &self.align_mode {
            mode.as_str()
        } else {
            match self.align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            }
        }
    }
    pub fn validate(&self) -> Result<(), TextError> {
        if let Some(mode) = &self.align_mode
            && !matches!(
                mode.as_str(),
                "left" | "center" | "right" | "justify" | "start" | "end"
            )
        {
            return Err(error(
                "alignMode requires left, center, right, justify, start or end",
            ));
        }
        if let Some(display) = &self.display
            && !matches!(display.as_str(), "inline" | "block" | "none")
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
        if self.url.len() > 4096
            || self.target.len() > 256
            || self
                .url
                .chars()
                .chain(self.target.chars())
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
            format.align_mode = None;
        }
        if let Some(value) = &self.align_mode {
            match value.as_str() {
                "left" => {
                    format.align = TextAlign::Left;
                    format.align_mode = None;
                }
                "center" => {
                    format.align = TextAlign::Center;
                    format.align_mode = None;
                }
                "right" => {
                    format.align = TextAlign::Right;
                    format.align_mode = None;
                }
                "justify" | "start" | "end" => {
                    format.align = if value == "end" {
                        TextAlign::Right
                    } else {
                        TextAlign::Left
                    };
                    format.align_mode = Some(value.clone());
                }
                _ => {
                    format.align_mode = Some(value.clone());
                }
            }
        }
        if let Some(value) = &self.display {
            format.display = Some(value.clone());
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
            format.target = value.clone();
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
            format_runs: Vec::new(),
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
            } => Some(Self {
                text: normalize_text(text),
                box_size: Some(TextBoxSize {
                    width: *width,
                    height: *height,
                }),
                format: format.clone(),
                default_format: format.clone(),
                multiline: *multiline,
                word_wrap: *word_wrap,
                alpha: 255,
                revision: "1".into(),
                ..Self::default()
            }),
            _ => None,
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
                "target": f.target,
                "tab_stops": f.tab_stops,
                "display": f.display.clone().unwrap_or_else(|| "block".into()),
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
            "target": if same(&|f| f.target == f0.target) { serde_json::json!(f0.target) } else { serde_json::Value::Null },
            "tab_stops": if same(&|f| f.tab_stops == f0.tab_stops) { serde_json::json!(f0.tab_stops) } else { serde_json::Value::Null },
            "display": if same(&|f| f.display.as_deref().unwrap_or("block") == f0.display.as_deref().unwrap_or("block")) { serde_json::json!(f0.display.clone().unwrap_or_else(|| "block".into())) } else { serde_json::Value::Null },
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
    } else {
        field.text.as_str()
    };
    for ch in display_source.chars() {
        let newline = ch == '\n';
        let is_tab = split_tabs_for_wrap && ch == '\t';
        let nextspace = ch.is_whitespace();
        let hit_boundary = !run_boundaries.is_empty() && run_boundaries.contains(&offset);
        if !text.is_empty() && (newline || is_tab || hit_boundary || space != nextspace) {
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
        widths = measured(&strings, &field.format, &mut measure)?;
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
            let w = measured(&strings, &mat_runs[r_idx].format, &mut measure)?;
            for (i, width) in indices.into_iter().zip(w) {
                widths[i] = width;
            }
        }
    }

    let padding = if field.box_size.is_some() { 2. } else { 0. };
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
        let orig_x = padding + pfmt.left_margin + pfmt.block_indent + b_indent;
        let p_indent = if paragraph_starts.contains(&start) {
            pfmt.indent
        } else {
            0.
        };
        let avail = field.box_size.map(|b| {
            (b.width
                - 2. * padding
                - pfmt.left_margin
                - pfmt.right_margin
                - pfmt.block_indent
                - b_indent
                - p_indent)
                .max(0.)
        });
        (orig_x + p_indent, avail, pfmt)
    };
    let wrap = field.multiline && field.word_wrap && field.box_size.is_some();

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
        let pfmt = fmt_at(part.start).clone();
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
                cur_max_size = cur_max_size.max(pfmt.size);
                cur_leading = cur_leading.max(pfmt.leading);
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
            cur_max_size = cur_max_size.max(pfmt.size);
            cur_leading = cur_leading.max(pfmt.leading);
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

    let mut y_cursor = padding;
    let mut text_width = 0.0_f64;
    let line_count_usize = built_lines.len();
    let mut next_starts: Vec<Option<u32>> = built_lines
        .iter()
        .skip(1)
        .map(|b| Some(b.line.start))
        .collect();
    next_starts.push(None);

    for (idx, bl) in built_lines.iter_mut().enumerate() {
        let (base_x, avail_opt, pfmt) = line_metrics_at(bl.line.start);
        let align_mode = pfmt.effective_align_mode();
        let soft_wrapped = idx + 1 < line_count_usize
            && next_starts[idx].is_some_and(|ns| !paragraph_starts.contains(&ns));
        if align_mode == "justify"
            && soft_wrapped
            && let Some(avail) = avail_opt
        {
            // Trim trailing whitespace segments for justification slack calculation.
            let last_non_ws = bl.segs.iter().rposition(|s| !s.text.trim().is_empty());
            if let Some(last_idx) = last_non_ws {
                let content_w = bl.segs[last_idx].rel_x + bl.segs[last_idx].width;
                let ws_indices: Vec<usize> = (0..last_idx)
                    .filter(|&i| bl.segs[i].text.chars().all(char::is_whitespace))
                    .collect();
                let slack = (avail - content_w).max(0.0);
                if !ws_indices.is_empty() && slack > 0.0 {
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
                }
            }
        }
        bl.line.x =
            base_x + avail_opt.map_or(0., |avail| align_offset(align_mode, avail, bl.line.width));
        bl.line.y = y_cursor;
        y_cursor += bl.max_size * 1.2 + bl.leading;
        text_width = text_width.max(bl.line.width);
    }

    let base_advance = field.format.size * 1.2 + field.format.leading;
    let text_height = if built_lines.is_empty() {
        field.format.size * 1.2
    } else {
        let last = built_lines.last().expect("non-empty");
        (last.line.y - padding) + last.line.height
    };
    let visible_height = field
        .box_size
        .map_or(text_height, |size| (size.height - 2. * padding).max(0.));
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
            .map_or(f64::from(scroll - 1) * base_advance, |b| b.line.y - padding)
    };

    let mut runs = Vec::new();
    let mut decorations = Vec::new();
    let mut bullets = Vec::new();
    let mut lines = Vec::with_capacity(built_lines.len());
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
        for seg in bl.segs {
            let sx = bl.line.x + seg.rel_x;
            let sy = bl.line.y + (bl.max_size - seg.fmt.size).max(0.0);
            if seg.fmt.underline && !seg.text.is_empty() {
                decorations.push(TextDecoration {
                    x: sx,
                    y: sy + seg.fmt.size * 1.05,
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
    };
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: base_advance,
        decorations,
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
    if !field.format_runs.is_empty() {
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
}
