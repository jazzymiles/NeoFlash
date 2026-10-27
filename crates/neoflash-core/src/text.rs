//! Uniform plain text and a bounded shared line-layout algorithm.
//! Hosts measure batches in their font engine; Rust owns wrapping, UTF-16 ranges,
//! alignment and line positions. This is not Flash's original text shaper.
use crate::ElementKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
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
        }
    }
}
impl TextFormat {
    pub fn validate(&self) -> Result<(), TextError> {
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
}
impl TextFormatPatch {
    fn apply(&self, format: &mut TextFormat) {
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
            format.align = value
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextFieldState {
    pub text: String,
    #[serde(rename = "box")]
    pub box_size: Option<TextBoxSize>,
    pub format: TextFormat,
    pub default_format: TextFormat,
    pub multiline: bool,
    pub word_wrap: bool,
    pub alpha: u8,
    pub revision: String,
}
impl Default for TextFieldState {
    fn default() -> Self {
        Self {
            text: String::new(),
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
            }),
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<(), TextError> {
        if self.text.len() > MAX_TEXT_BYTES {
            return Err(error("text exceeds 1048576 UTF-8 bytes"));
        }
        self.format.validate()?;
        self.default_format.validate()?;
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
            > 1
        {
            return Err(error(
                "a text update must contain at most one replacement or append operation",
            ));
        }
        let mut next = self.clone();
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
        }
        if let Some(text) = &patch.append {
            if next.text.len().saturating_add(text.len()) > MAX_TEXT_BYTES {
                return Err(error("append exceeds 1048576 UTF-8 bytes"));
            }
            if !text.is_empty() && !next.text.is_empty() && next.format != next.default_format {
                return Err(error(
                    "append would create mixed formatting; apply a uniform format or replace the whole text first",
                ));
            }
            if next.text.is_empty() {
                next.format = next.default_format.clone()
            }
            next.text.push_str(&normalize_text(text));
        }
        if let Some(replacement) = &patch.replace {
            let start = utf16_byte(&next.text, replacement.begin)?;
            let end = utf16_byte(&next.text, replacement.end)?;
            if start > end {
                return Err(error("replacement range must be ordered"));
            }
            if next.text.len() - (end - start) + replacement.text.len() > MAX_TEXT_BYTES {
                return Err(error("replacement exceeds 1048576 UTF-8 bytes"));
            }
            let retained = start > 0 || end < next.text.len();
            if retained && !replacement.text.is_empty() && next.format != next.default_format {
                return Err(error(
                    "replacement would create mixed formatting; only a uniform whole-field format is supported",
                ));
            }
            if !retained {
                next.format = next.default_format.clone()
            }
            next.text
                .replace_range(start..end, &normalize_text(&replacement.text));
        }
        if let Some(format) = &patch.format {
            format.apply(&mut next.format)
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
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatch {
    pub text: Option<String>,
    pub append: Option<String>,
    pub replace: Option<TextReplacement>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub multiline: Option<bool>,
    pub word_wrap: Option<bool>,
    pub format: Option<TextFormatPatch>,
    pub default_format: Option<TextFormatPatch>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextReplacement {
    pub begin: u32,
    pub end: u32,
    pub text: String,
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
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub text_width: f64,
    pub text_height: f64,
    pub line_height: f64,
}
struct Part {
    text: String,
    start: u32,
    end: u32,
    newline: bool,
}
fn parts(field: &TextFieldState) -> Result<Vec<Part>, TextError> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut start = 0;
    let mut offset = 0;
    let mut space = false;
    for ch in field.text.chars() {
        let newline = ch == '\n';
        let nextspace = ch.is_whitespace();
        if !text.is_empty() && (newline || space != nextspace) {
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
    let result = measure(strings, format)?;
    if result.len() != strings.len() || result.iter().any(|w| !w.is_finite() || *w < 0. || *w > 1e9)
    {
        return Err(error(
            "font measurement must return one finite width in 0..=1e9 per string",
        ));
    }
    Ok(result)
}
/// At most three host calls; summed segment widths choose wraps, then complete
/// lines are measured once for coherent alignment/metrics. Very long segments
/// break on Unicode scalar boundaries using measured advances, not graphemes.
pub fn layout_text<M>(field: &TextFieldState, mut measure: M) -> Result<TextLayout, TextError>
where
    M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
{
    field.validate()?;
    let parts = parts(field)?;
    let strings: Vec<_> = parts.iter().map(|p| p.text.clone()).collect();
    let widths = measured(&strings, &field.format, &mut measure)?;
    let padding = if field.box_size.is_some() { 2. } else { 0. };
    let available = field.box_size.map(|b| (b.width - 2. * padding).max(0.));
    let wrap = field.multiline && field.word_wrap && available.is_some();
    let mut glyphs = BTreeSet::new();
    if wrap {
        for (part, width) in parts.iter().zip(&widths) {
            if *width > available.unwrap() {
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
        if wrap && *width > available.unwrap() {
            let mut offset = part.start;
            for ch in part.text.chars() {
                let width = glyph_widths[&ch];
                if !current.text.is_empty() && current.width + width > available.unwrap() {
                    flush(&mut lines, &mut current)?
                }
                current.text.push(ch);
                current.width += width;
                offset += ch.len_utf16() as u32;
                current.end = offset;
            }
        } else {
            if wrap && !current.text.is_empty() && current.width + width > available.unwrap() {
                flush(&mut lines, &mut current)?
            }
            current.text.push_str(&part.text);
            current.width += width;
            current.end = part.end;
        }
    }
    flush(&mut lines, &mut current)?;
    let strings: Vec<_> = lines.iter().map(|line| line.text.clone()).collect();
    let widths = measured(&strings, &field.format, &mut measure)?;
    let advance = field.format.size * 1.2 + field.format.leading;
    let mut text_width = 0_f64;
    for (index, (line, width)) in lines.iter_mut().zip(widths).enumerate() {
        line.width = width;
        line.x = padding
            + available.map_or(0., |available| match field.format.align {
                TextAlign::Left => 0.,
                TextAlign::Center => (available - width) / 2.,
                TextAlign::Right => available - width,
            });
        line.y = padding + index as f64 * advance;
        text_width = text_width.max(width);
    }
    let text_height = field.format.size * 1.2 + (lines.len().saturating_sub(1)) as f64 * advance;
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: advance,
    })
}
