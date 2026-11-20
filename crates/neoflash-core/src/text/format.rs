use serde::{Deserialize, Serialize};

use super::*;

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
    pub fn is_plain_point_text_compatible(&self) -> bool {
        !self.bold
            && !self.italic
            && !self.underline
            && self.letter_spacing.abs() < 1e-9
            && self.leading.abs() < 1e-9
            && self.align == TextAlign::Left
            && self.align_mode().is_none()
            && self.display().is_none()
            && self.left_margin.abs() < 1e-9
            && self.right_margin.abs() < 1e-9
            && self.indent.abs() < 1e-9
            && self.block_indent.abs() < 1e-9
            && !self.bullet
            && self.kerning.is_none()
            && self.tab_stops.is_empty()
            && self.url.is_empty()
            && self.target_str().is_empty()
            && self.field_options().is_none()
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
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn diff_from(before: &TextFormat, after: &TextFormat) -> Self {
        let mut patch = Self::default();
        if before.font != after.font {
            patch.font = Some(after.font.clone());
        }
        if (before.size - after.size).abs() > 1e-9 {
            patch.size = Some(after.size);
        }
        if before.color != after.color {
            patch.color = Some(after.color);
        }
        if before.bold != after.bold {
            patch.bold = Some(after.bold);
        }
        if before.italic != after.italic {
            patch.italic = Some(after.italic);
        }
        if (before.leading - after.leading).abs() > 1e-9 {
            patch.leading = Some(after.leading);
        }
        if before.effective_align_mode() != after.effective_align_mode() {
            patch.align_mode = Some(after.effective_align_mode().to_owned());
        } else if before.align != after.align {
            patch.align = Some(after.align);
        }
        if before.display() != after.display() {
            patch.display = Some(after.display().unwrap_or("block").to_owned());
        }
        if (before.left_margin - after.left_margin).abs() > 1e-9 {
            patch.left_margin = Some(after.left_margin);
        }
        if (before.right_margin - after.right_margin).abs() > 1e-9 {
            patch.right_margin = Some(after.right_margin);
        }
        if (before.indent - after.indent).abs() > 1e-9 {
            patch.indent = Some(after.indent);
        }
        if (before.block_indent - after.block_indent).abs() > 1e-9 {
            patch.block_indent = Some(after.block_indent);
        }
        if before.underline != after.underline {
            patch.underline = Some(after.underline);
        }
        if (before.letter_spacing - after.letter_spacing).abs() > 1e-9 {
            patch.letter_spacing = Some(after.letter_spacing);
        }
        if before.kerning != after.kerning {
            patch.kerning = Some(after.kerning.unwrap_or(false));
        }
        if before.bullet != after.bullet {
            patch.bullet = Some(after.bullet);
        }
        if before.tab_stops != after.tab_stops {
            patch.tab_stops = Some(after.tab_stops.clone());
        }
        if before.url != after.url {
            patch.url = Some(after.url.clone());
        }
        if before.target_str() != after.target_str() {
            patch.target = Some(after.target_str().to_owned());
        }
        patch
    }

    pub fn character_only(&self) -> Self {
        Self {
            font: self.font.clone(),
            size: self.size,
            color: self.color,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            letter_spacing: self.letter_spacing,
            kerning: self.kerning,
            url: self.url.clone(),
            target: self.target.clone(),
            ..Self::default()
        }
    }

    pub fn paragraph_only(&self) -> Self {
        Self {
            leading: self.leading,
            align: self.align,
            align_mode: self.align_mode.clone(),
            display: self.display.clone(),
            left_margin: self.left_margin,
            right_margin: self.right_margin,
            indent: self.indent,
            block_indent: self.block_indent,
            bullet: self.bullet,
            tab_stops: self.tab_stops.clone(),
            ..Self::default()
        }
    }

    pub fn apply(&self, format: &mut TextFormat) {
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
