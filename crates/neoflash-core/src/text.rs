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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAutoSize {
    #[default]
    None,
    Left,
    Center,
    Right,
}
fn opaque() -> u8 {
    255
}
fn white() -> u32 {
    0xffffff
}
fn first_line() -> u32 {
    1
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
    #[serde(default)]
    pub auto_size: TextAutoSize,
    #[serde(default)]
    pub scroll_h: u32,
    #[serde(default = "first_line")]
    pub scroll_v: u32,
    #[serde(default)]
    pub background: bool,
    #[serde(default = "white")]
    pub background_color: u32,
    #[serde(default = "opaque")]
    pub background_alpha: u8,
    #[serde(default)]
    pub border: bool,
    #[serde(default)]
    pub border_color: u32,
    #[serde(default = "opaque")]
    pub border_alpha: u8,
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
            field_type: TextFieldType::Dynamic,
            selectable: true,
            display_as_password: false,
            max_chars: 0,
            restrict: None,
            selection_begin: 0,
            selection_end: 0,
            caret: 0,
            auto_size: TextAutoSize::None,
            scroll_h: 0,
            scroll_v: 1,
            background: false,
            background_color: 0xffffff,
            background_alpha: 255,
            border: false,
            border_color: 0,
            border_alpha: 255,
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
        if self.text.len() > MAX_TEXT_BYTES {
            return Err(error("text exceeds 1048576 UTF-8 bytes"));
        }
        if self.max_chars > MAX_TEXT_BYTES as u32 {
            return Err(error("maxChars must be in 0..=1048576 UTF-16 units"));
        }
        if self.scroll_v == 0 || self.background_color > 0xffffff || self.border_color > 0xffffff {
            return Err(error(
                "scrollV must be at least one and background/border colors must be RGB integers",
            ));
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
        if let Some(value) = patch.auto_size {
            next.auto_size = value;
        }
        if let Some(value) = patch.scroll_h {
            next.scroll_h = value.max(0) as u32;
        }
        if let Some(value) = patch.scroll_v {
            next.scroll_v = value.max(1) as u32;
        }
        if let Some(value) = patch.background {
            next.background = value;
        }
        if let Some(value) = patch.background_color {
            next.background_color = value;
        }
        if let Some(value) = patch.border {
            next.border = value;
        }
        if let Some(value) = patch.border_color {
            next.border_color = value;
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
        if let Some(replacement) = patch.replace.as_ref().or(selected_replacement.as_ref()) {
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
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatch {
    pub auto_size: Option<TextAutoSize>,
    pub scroll_h: Option<i32>,
    pub scroll_v: Option<i32>,
    pub background: Option<bool>,
    pub background_color: Option<u32>,
    pub border: Option<bool>,
    pub border_color: Option<u32>,
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
    pub ascent: f64,
    pub descent: f64,
    pub leading: f64,
    pub advance_scale: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextGlyph {
    pub text: String,
    pub width: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextParagraph {
    pub start: u32,
    pub end: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub text_width: f64,
    pub text_height: f64,
    pub line_height: f64,
    pub viewport: Option<TextBoxSize>,
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub scroll_h: u32,
    pub scroll_v: u32,
    pub max_scroll_h: u32,
    pub max_scroll_v: u32,
    pub bottom_scroll_v: u32,
    pub glyphs: Vec<TextGlyph>,
    pub paragraphs: Vec<TextParagraph>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextQuery {
    CharBounds { index: i32 },
    CharAtPoint { x: f64, y: f64 },
    LineAtPoint { x: f64, y: f64 },
    FirstCharInParagraph { index: i32 },
    ParagraphLength { index: i32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TextQueryResult {
    Rectangle(TextRectangle),
    Index(i32),
    Empty,
}

/// Query compact scalar advances; no host prefix-measurement loop or persistent
/// second text model. Widths are apportioned to the measured whole-line width.
pub fn query_text(
    field: &TextFieldState,
    layout: &TextLayout,
    query: &TextQuery,
) -> Result<TextQueryResult, TextError> {
    field.validate()?;
    if layout.lines.len() > MAX_PARTS
        || layout.glyphs.len() > MAX_GLYPHS
        || layout.paragraphs.len() > MAX_PARTS + 1
        || layout
            .lines
            .iter()
            .map(|line| line.text.len())
            .sum::<usize>()
            > MAX_TEXT_BYTES
    {
        return Err(error("text query exceeds the bounded layout budget"));
    }
    if let Some(viewport) = layout.viewport {
        viewport.validate()?;
    }
    let valid = |v: f64| v.is_finite() && v.abs() <= 1e12;
    if !valid(layout.scroll_x)
        || !valid(layout.scroll_y)
        || layout.lines.iter().any(|line| {
            ![line.x, line.y, line.width, line.height, line.advance_scale]
                .into_iter()
                .all(valid)
                || line.width < 0.
                || line.height < 0.
                || line.advance_scale < 0.
        })
    {
        return Err(error("invalid text query layout coordinates"));
    }
    let mut glyphs = HashMap::new();
    for glyph in &layout.glyphs {
        let mut chars = glyph.text.chars();
        let ch = chars
            .next()
            .ok_or_else(|| error("invalid text query glyph"))?;
        if chars.next().is_some() || !valid(glyph.width) || glyph.width < 0. {
            return Err(error("invalid text query glyph"));
        }
        glyphs.insert(ch, glyph.width);
    }
    let units = field.text.encode_utf16().count() as u32;
    if let TextQuery::FirstCharInParagraph { index } | TextQuery::ParagraphLength { index } = query
    {
        if *index < 0 || *index as u32 > units {
            return Err(error("paragraph character index is outside the text"));
        }
        let index = *index as u32;
        let paragraph = layout
            .paragraphs
            .iter()
            .rev()
            .find(|p| index >= p.start && (index < p.end || index == units && index == p.end))
            .ok_or_else(|| error("paragraph character index is outside the text"))?;
        if paragraph.start > paragraph.end || paragraph.end > units {
            return Err(error("invalid paragraph geometry"));
        }
        return Ok(TextQueryResult::Index(
            if matches!(query, TextQuery::FirstCharInParagraph { .. }) {
                paragraph.start
            } else {
                paragraph.end - paragraph.start
            } as i32,
        ));
    }
    let point = match query {
        TextQuery::CharAtPoint { x, y } | TextQuery::LineAtPoint { x, y } => Some((*x, *y)),
        _ => None,
    };
    if let Some((x, y)) = point {
        if !x.is_finite() || !y.is_finite() {
            return Err(error("text query point must be finite"));
        }
        if layout
            .viewport
            .is_some_and(|b| x < 0. || y < 0. || x >= b.width || y >= b.height)
        {
            return Ok(TextQueryResult::Index(-1));
        }
    }
    for (line_index, line) in layout.lines.iter().enumerate() {
        if let TextQuery::CharBounds { index } = query
            && (*index < 0 || (*index as u32) < line.start || (*index as u32) >= line.end)
        {
            continue;
        }
        let y = line.y - layout.scroll_y;
        if let Some((_, py)) = point
            && (py < y || py >= y + line.height)
        {
            continue;
        }
        let mut x = line.x - layout.scroll_x;
        if matches!(query, TextQuery::LineAtPoint { .. }) {
            if let Some((px, _)) = point
                && px >= x
                && px < x + line.width
            {
                return Ok(TextQueryResult::Index(line_index as i32));
            }
            continue;
        }
        let mut index = line.start;
        for ch in line.text.chars() {
            let width = glyphs
                .get(&ch)
                .ok_or_else(|| error("text query glyph measurement is missing"))?
                * line.advance_scale;
            let end = index + ch.len_utf16() as u32;
            let selected = match query {
                TextQuery::CharBounds { index: requested } => {
                    *requested >= 0 && (*requested as u32) >= index && (*requested as u32) < end
                }
                TextQuery::CharAtPoint { x: px, .. } => *px >= x && *px < x + width,
                _ => false,
            };
            if selected {
                return Ok(if point.is_some() {
                    TextQueryResult::Index(index as i32)
                } else {
                    TextQueryResult::Rectangle(TextRectangle {
                        x,
                        y,
                        width,
                        height: line.height,
                    })
                });
            }
            x += width;
            index = end;
        }
        // A hard paragraph terminator has a caret-sized box, not a painted glyph.
        if let TextQuery::CharBounds { index: requested } = query
            && *requested >= 0
            && (*requested as u32) >= index
            && (*requested as u32) < line.end
        {
            return Ok(TextQueryResult::Rectangle(TextRectangle {
                x,
                y,
                width: 0.,
                height: line.height,
            }));
        }
    }
    Ok(if point.is_some() {
        TextQueryResult::Index(-1)
    } else {
        TextQueryResult::Empty
    })
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
    let padding = if field.box_size.is_some() || field.auto_size != TextAutoSize::None {
        2.
    } else {
        0.
    };
    let available = field.box_size.map(|b| (b.width - 2. * padding).max(0.));
    let wrap = field.multiline && field.word_wrap && available.is_some();
    let mut glyphs = BTreeSet::new();
    for part in &parts {
        glyphs.extend(part.text.chars());
        if glyphs.len() > MAX_GLYPHS {
            return Err(error("layout exceeds 65536 distinct measured glyphs"));
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
        ascent: field.format.size,
        descent: field.format.size * 0.2,
        leading: field.format.leading,
        advance_scale: 1.,
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
            ascent: current.ascent,
            descent: current.descent,
            leading: current.leading,
            advance_scale: 1.,
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
    let text_width = widths.iter().copied().fold(0_f64, f64::max);
    let text_height = field.format.size * 1.2 + (lines.len().saturating_sub(1)) as f64 * advance;
    let viewport = if field.auto_size != TextAutoSize::None {
        let size = TextBoxSize {
            width: if field.word_wrap {
                field
                    .box_size
                    .map_or(text_width + 2. * padding, |size| size.width)
            } else {
                text_width + 2. * padding
            },
            height: text_height + 2. * padding,
        };
        size.validate()?;
        Some(size)
    } else {
        field.box_size
    };
    let available = viewport.map(|size| (size.width - 2. * padding).max(0.));
    let inner_height = viewport.map_or(text_height, |size| (size.height - 2. * padding).max(0.));
    let max_scroll_h = if field.word_wrap {
        0
    } else {
        available.map_or(0, |width| {
            (text_width - width).max(0.).ceil().min(f64::from(u32::MAX)) as u32
        })
    };
    let max_scroll_v = (((text_height - inner_height).max(0.) / advance).ceil() as usize)
        .saturating_add(1)
        .min(lines.len())
        .max(1) as u32;
    let scroll_h = field.scroll_h.min(max_scroll_h);
    let scroll_v = field.scroll_v.clamp(1, max_scroll_v);
    let bottom_scroll_v = u64::from(scroll_v)
        .saturating_add((inner_height / advance).ceil().max(1.) as u64)
        .saturating_sub(1)
        .min(lines.len() as u64) as u32;
    for (index, (line, width)) in lines.iter_mut().zip(widths).enumerate() {
        line.width = width;
        line.x = padding
            + available.map_or(0., |available| match field.format.align {
                TextAlign::Left => 0.,
                TextAlign::Center => (available - width) / 2.,
                TextAlign::Right => available - width,
            });
        line.y = padding + index as f64 * advance;
        let sum: f64 = line.text.chars().map(|ch| glyph_widths[&ch]).sum();
        line.advance_scale = if sum > 0. { width / sum } else { 0. };
        if !line.advance_scale.is_finite() {
            return Err(error("font advances exceed finite layout precision"));
        }
    }
    let mut paragraphs = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for ch in field.text.chars() {
        offset += ch.len_utf16() as u32;
        if ch == '\n' {
            paragraphs.push(TextParagraph { start, end: offset });
            start = offset;
        }
    }
    paragraphs.push(TextParagraph { start, end: offset });
    Ok(TextLayout {
        lines,
        text_width,
        text_height,
        line_height: advance,
        viewport,
        scroll_x: f64::from(scroll_h),
        scroll_y: f64::from(scroll_v - 1) * advance,
        scroll_h,
        scroll_v,
        max_scroll_h,
        max_scroll_v,
        bottom_scroll_v,
        glyphs: glyph_strings
            .into_iter()
            .map(|text| TextGlyph {
                width: glyph_widths[&text.chars().next().expect("glyph")],
                text,
            })
            .collect(),
        paragraphs,
    })
}
