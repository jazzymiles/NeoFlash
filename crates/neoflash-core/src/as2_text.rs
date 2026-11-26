//! Bounded AS2 styles and uniform HTML, adapted to NeoFlash's owned text model.
//! CSS/TextFormat mappings follow Ruffle MIT v0.7.1 (89a7049965f3090e706265670df309a5128233ac).
use crate::{TextAlign, TextError, TextFieldState, TextFormat, TextFormatPatch, TextFormatRun};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
const MAX_SOURCE: usize = 1_048_576;
pub type Style = BTreeMap<String, Value>;
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StyleSheet {
    pub styles: BTreeMap<String, Style>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextFieldRenderProps {
    pub embed_fonts: bool,
    pub anti_alias_type: String,
    pub grid_fit_type: String,
    pub sharpness: f64,
    pub thickness: f64,
}
impl Default for TextFieldRenderProps {
    fn default() -> Self {
        Self {
            embed_fonts: false,
            anti_alias_type: "normal".into(),
            grid_fit_type: "pixel".into(),
            sharpness: 0.0,
            thickness: 0.0,
        }
    }
}
impl TextFieldRenderProps {
    fn validate(&self) -> Result<(), TextError> {
        if !matches!(self.anti_alias_type.as_str(), "normal" | "advanced") {
            return Err(error(
                "TextField.antiAliasType requires 'normal' or 'advanced'",
            ));
        }
        if !matches!(self.grid_fit_type.as_str(), "none" | "pixel" | "subpixel") {
            return Err(error(
                "TextField.gridFitType requires 'none', 'pixel' or 'subpixel'",
            ));
        }
        if !self.sharpness.is_finite()
            || !(-400.0..=400.0).contains(&self.sharpness)
            || !self.thickness.is_finite()
            || !(-200.0..=200.0).contains(&self.thickness)
        {
            return Err(error("TextField sharpness/thickness is out of bounds"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AntialiasEntry {
    pub font_size: f64,
    pub inside_cutoff: f64,
    pub outside_cutoff: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextRendererState {
    pub display_mode: String,
    pub max_level: i32,
    #[serde(default)]
    pub tables: BTreeMap<String, Vec<AntialiasEntry>>,
}
impl Default for TextRendererState {
    fn default() -> Self {
        Self {
            display_mode: "default".into(),
            max_level: 4,
            tables: BTreeMap::new(),
        }
    }
}
impl TextRendererState {
    fn validate(&self) -> Result<(), TextError> {
        if !matches!(self.display_mode.as_str(), "default" | "lcd" | "crt") {
            return Err(error(
                "TextRenderer.displayMode requires 'default', 'lcd' or 'crt'",
            ));
        }
        if !(0..=64).contains(&self.max_level) {
            return Err(error("TextRenderer.maxLevel requires an integer in 0..=64"));
        }
        if self.tables.len() > 256 {
            return Err(error("TextRenderer antialiasing tables exceed 256 entries"));
        }
        Ok(())
    }
}
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    HitLink {
        field: TextFieldState,
        layout: crate::TextLayout,
        x: f64,
        y: f64,
    },
    Wheel {
        field: TextFieldState,
        layout: crate::TextLayout,
        delta: f64,
        mode: u8,
    },
    ParseCss {
        state: StyleSheet,
        source: String,
    },
    SetStyle {
        state: StyleSheet,
        selector: String,
        style: Option<Style>,
    },
    GetStyle {
        state: StyleSheet,
        selector: String,
    },
    Names {
        state: StyleSheet,
    },
    Clear,
    Transform {
        style: Style,
    },
    Html {
        source: String,
        format: TextFormat,
        styles: StyleSheet,
        #[serde(default)]
        condense_white: bool,
        #[serde(default)]
        multi_run: bool,
    },
    EscapeHtml {
        text: String,
    },
    SerializeHtml {
        text: String,
        format: TextFormat,
        #[serde(default)]
        format_runs: Vec<TextFormatRun>,
    },
    ExtentField {
        text: String,
        format: TextFormat,
        width: Option<f64>,
    },
    ExtentResult {
        layout: crate::TextLayout,
        format: TextFormat,
    },
    FieldRenderSet {
        #[serde(default)]
        state: TextFieldRenderProps,
        name: String,
        value: Value,
    },
    FontList {
        candidates: Vec<String>,
    },
    RendererSet {
        #[serde(default)]
        state: TextRendererState,
        name: String,
        value: Value,
    },
    RendererTable {
        #[serde(default)]
        state: TextRendererState,
        font_name: String,
        font_style: String,
        color_type: String,
        table: Vec<AntialiasEntry>,
    },
    RendererResolve {
        #[serde(default)]
        state: TextRendererState,
        font_name: String,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        italic: bool,
        #[serde(default)]
        rgb: u32,
        font_size: f64,
    },
}
fn error(message: impl Into<String>) -> TextError {
    TextError(message.into())
}
fn selector(value: &str) -> Result<(), TextError> {
    let value = value.strip_prefix('.').unwrap_or(value);
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(error(
            "StyleSheet supports simple tag and .class selectors up to 128 bytes",
        ));
    }
    Ok(())
}
fn validate_style(style: &Style) -> Result<(), TextError> {
    if style.len() > 128 {
        return Err(error("StyleSheet declaration exceeds 128 properties"));
    }
    for (name, value) in style {
        if name.len() > 128
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || !matches!(
                value,
                Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
            )
        {
            return Err(error(
                "StyleSheet properties require bounded names and primitive data values",
            ));
        }
        if value.as_str().is_some_and(|v| v.len() > 4096) {
            return Err(error("StyleSheet property exceeds 4096 bytes"));
        }
    }
    Ok(())
}
impl StyleSheet {
    fn validate(&self) -> Result<(), TextError> {
        if self.styles.len() > 1024 {
            return Err(error("StyleSheet exceeds 1024 selectors"));
        }
        for (name, style) in &self.styles {
            selector(name)?;
            validate_style(style)?;
        }
        Ok(())
    }
}
fn camel(name: &str) -> Result<String, TextError> {
    let mut result = String::new();
    let mut upper = false;
    for c in name.trim().chars() {
        if c == '-' {
            upper = true;
        } else if c.is_ascii_alphanumeric() {
            result.push(if upper { c.to_ascii_uppercase() } else { c });
            upper = false;
        } else {
            return Err(error("Invalid CSS property name"));
        }
    }
    if result.is_empty() {
        return Err(error("Empty CSS property name"));
    }
    Ok(result)
}
fn css(source: &str, previous: &StyleSheet) -> Result<StyleSheet, TextError> {
    if source.len() > MAX_SOURCE {
        return Err(error("CSS source exceeds 1 MiB"));
    }
    let mut clean = String::new();
    let mut rest = source;
    while let Some(begin) = rest.find("/*") {
        clean.push_str(&rest[..begin]);
        let end = rest[begin + 2..]
            .find("*/")
            .ok_or_else(|| error("Unclosed CSS comment"))?
            + begin
            + 2;
        rest = &rest[end + 2..];
    }
    clean.push_str(rest);
    let mut rest = clean.trim();
    let mut next = previous.clone();
    while !rest.is_empty() {
        let open = rest
            .find('{')
            .ok_or_else(|| error("CSS rule needs an opening brace"))?;
        let close = rest[open + 1..]
            .find('}')
            .ok_or_else(|| error("CSS rule needs a closing brace"))?
            + open
            + 1;
        let body = &rest[open + 1..close];
        if body.contains('{') {
            return Err(error("Nested CSS rules are unsupported"));
        }
        let mut style = Style::new();
        for declaration in body.split(';').filter(|part| !part.trim().is_empty()) {
            let (name, value) = declaration
                .split_once(':')
                .ok_or_else(|| error("CSS declaration needs a colon"))?;
            let value = value.trim();
            if value.contains(['{', '}', ';']) {
                return Err(error("Unsupported CSS declaration syntax"));
            }
            style.insert(
                camel(name)?,
                Value::String(value.trim_matches(['\'', '"']).into()),
            );
        }
        validate_style(&style)?;
        for name in rest[..open].split(',').map(str::trim) {
            selector(name)?;
            next.styles.insert(name.into(), style.clone());
        }
        rest = rest[close + 1..].trim();
    }
    next.validate()?;
    Ok(next)
}
fn text(value: &Value) -> Result<String, TextError> {
    match value {
        Value::String(v) => Ok(v.clone()),
        Value::Number(v) => Ok(v.to_string()),
        Value::Bool(v) => Ok(v.to_string()),
        _ => Err(error("Style value requires a string, number or Boolean")),
    }
}
fn number(value: &Value) -> Result<f64, TextError> {
    let value = text(value)?;
    let value = value.trim().strip_suffix("px").unwrap_or(value.trim());
    let number = value
        .parse::<f64>()
        .map_err(|_| error("CSS length requires a finite number or px length"))?;
    if !number.is_finite() {
        return Err(error("CSS length must be finite"));
    }
    Ok(number)
}
fn color(value: &Value) -> Result<u32, TextError> {
    let value = text(value)?;
    let digits = value
        .strip_prefix('#')
        .ok_or_else(|| error("CSS colors require #RRGGBB"))?;
    if digits.len() != 6 {
        return Err(error("CSS colors require #RRGGBB"));
    }
    u32::from_str_radix(digits, 16).map_err(|_| error("CSS colors require #RRGGBB"))
}
pub fn transform(style: &Style) -> Result<TextFormatPatch, TextError> {
    validate_style(style)?;
    let mut patch = TextFormatPatch::default();
    for (name, value) in style {
        if value.is_null() {
            continue;
        }
        match name.as_str() {
            "fontFamily" => patch.font = Some(text(value)?),
            "fontSize" => patch.size = Some(number(value)?),
            "color" => patch.color = Some(color(value)?),
            "fontWeight" => {
                patch.bold = Some(match text(value)?.as_str() {
                    "bold" => true,
                    "normal" => false,
                    _ => return Err(error("CSS fontWeight supports normal/bold")),
                })
            }
            "fontStyle" => {
                patch.italic = Some(match text(value)?.as_str() {
                    "italic" => true,
                    "normal" => false,
                    _ => return Err(error("CSS fontStyle supports normal/italic")),
                })
            }
            "textDecoration" => {
                patch.underline = Some(match text(value)?.as_str() {
                    "underline" => true,
                    "none" => false,
                    _ => return Err(error("CSS textDecoration supports none/underline")),
                })
            }
            "textAlign" => {
                let mode = text(value)?.to_ascii_lowercase();
                patch.align = Some(match mode.as_str() {
                    "left" | "start" | "justify" => TextAlign::Left,
                    "center" => TextAlign::Center,
                    "right" | "end" => TextAlign::Right,
                    _ => return Err(error("CSS textAlign supports left/center/right/justify")),
                });
                patch.align_mode = Some(mode);
            }
            "display" => {
                let mode = text(value)?.to_ascii_lowercase();
                if !matches!(mode.as_str(), "inline" | "block" | "none") {
                    return Err(error("CSS display supports inline/block/none"));
                }
                patch.display = Some(mode);
            }
            "marginLeft" => patch.left_margin = Some(number(value)?.max(0.)),
            "marginRight" => patch.right_margin = Some(number(value)?.max(0.)),
            "textIndent" => patch.indent = Some(number(value)?),
            "leading" => patch.leading = Some(number(value)?),
            "letterSpacing" => patch.letter_spacing = Some(number(value)?),
            "kerning" => {
                patch.kerning = Some(match text(value)?.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(error("CSS kerning requires true/false")),
                })
            }
            _ => {
                return Err(error(format!(
                    "StyleSheet property '{name}' is stored but cannot yet be rendered by the uniform text profile"
                )));
            }
        }
    }
    let mut format = TextFormat::default();
    patch.apply(&mut format);
    format.validate()?;
    Ok(patch)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlImageSpec {
    pub id: Option<String>,
    pub src: String,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub hspace: f64,
    pub vspace: f64,
    pub align: String,
    pub char_index: u32,
}

#[derive(Serialize)]
struct HtmlResult {
    text: String,
    format: TextFormat,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    format_runs: Vec<TextFormatRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<HtmlImageSpec>,
}

fn decode_flash_html_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_owned();
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            let max_end = (i + 16).min(bytes.len());
            let mut semi = None;
            let mut j = i + 1;
            while j < max_end {
                let b = bytes[j];
                if b == b';' {
                    if j > i + 1 {
                        semi = Some(j);
                    }
                    break;
                }
                if !(b == b'#' || b.is_ascii_alphanumeric()) {
                    break;
                }
                j += 1;
            }
            if let Some(end) = semi {
                let entity = &input[i + 1..end];
                let decoded = if let Some(hex) = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|c| *c != '\0')
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|c| *c != '\0')
                } else if entity.eq_ignore_ascii_case("amp") {
                    Some('&')
                } else if entity.eq_ignore_ascii_case("lt") {
                    Some('<')
                } else if entity.eq_ignore_ascii_case("gt") {
                    Some('>')
                } else if entity.eq_ignore_ascii_case("quot") {
                    Some('"')
                } else if entity.eq_ignore_ascii_case("apos") {
                    Some('\'')
                } else if entity.eq_ignore_ascii_case("nbsp") {
                    Some('\u{00A0}')
                } else {
                    None
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    i = end + 1;
                    continue;
                }
            }
            out.push('&');
            i += 1;
            continue;
        }
        let ch = input[i..].chars().next().expect("valid utf-8");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[derive(Debug)]
enum FlashHtmlToken {
    Text(String),
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    EndTag(String),
}

fn parse_flash_tag_attributes(raw: &str) -> Vec<(String, String)> {
    let bytes = raw.as_bytes();
    let mut attrs = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() {
            let b = bytes[i];
            if b.is_ascii_whitespace() || matches!(b, b'=' | b'/' | b'>' | b'"' | b'\'') {
                break;
            }
            let ch = raw[i..].chars().next().expect("valid utf-8");
            i += ch.len_utf8();
        }
        if i == name_start {
            let ch = raw[i..].chars().next().expect("valid utf-8");
            i += ch.len_utf8();
            continue;
        }
        let name = raw[name_start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    let ch = raw[i..].chars().next().expect("valid utf-8");
                    i += ch.len_utf8();
                }
                let slice = &raw[val_start..i];
                if i < bytes.len() && bytes[i] == quote {
                    i += 1;
                }
                decode_flash_html_entities(slice)
            } else {
                let val_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    if bytes[i] == b'/' && i + 1 == bytes.len() {
                        break;
                    }
                    let ch = raw[i..].chars().next().expect("valid utf-8");
                    i += ch.len_utf8();
                }
                decode_flash_html_entities(&raw[val_start..i])
            }
        } else {
            String::new()
        };
        attrs.push((name, value));
    }
    attrs
}

fn tokenize_flash_html(source: &str) -> Vec<FlashHtmlToken> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut literal_buf = String::new();
    let flush_literal = |buf: &mut String, out: &mut Vec<FlashHtmlToken>| {
        if !buf.is_empty() {
            let text = std::mem::take(buf);
            if let Some(FlashHtmlToken::Text(prev)) = out.last_mut() {
                prev.push_str(&text);
            } else {
                out.push(FlashHtmlToken::Text(text));
            }
        }
    };
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            literal_buf.push_str(&decode_flash_html_entities(&source[start..i]));
            continue;
        }
        // Comment <!-- ... -->
        if source[i..].starts_with("<!--") {
            flush_literal(&mut literal_buf, &mut tokens);
            if let Some(rel) = source[i + 4..].find("-->") {
                i += 4 + rel + 3;
            } else {
                i = bytes.len();
            }
            continue;
        }
        // CDATA <![CDATA[ ... ]]>
        if bytes.len() >= i + 9 && source[i..i + 9].eq_ignore_ascii_case("<![cdata[") {
            if let Some(rel) = source[i + 9..].find("]]>") {
                literal_buf.push_str(&source[i + 9..i + 9 + rel]);
                i += 9 + rel + 3;
            } else {
                literal_buf.push_str(&source[i + 9..]);
                i = bytes.len();
            }
            continue;
        }
        // Processing instruction <?...?> or declaration <!DOCTYPE ...>
        if i + 1 < bytes.len() && (bytes[i + 1] == b'?' || bytes[i + 1] == b'!') {
            if let Some(rel) = source[i + 2..].find('>') {
                flush_literal(&mut literal_buf, &mut tokens);
                i += 2 + rel + 1;
                continue;
            }
            literal_buf.push('<');
            i += 1;
            continue;
        }
        // Closing tag </name ...>
        if i + 2 < bytes.len() && bytes[i + 1] == b'/' && bytes[i + 2].is_ascii_alphabetic() {
            let name_start = i + 2;
            let mut j = name_start;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'_' | b'-' | b':'))
            {
                j += 1;
            }
            let name = source[name_start..j].to_ascii_lowercase();
            let mut close_pos = None;
            let mut hit_lt = false;
            let mut k = j;
            while k < bytes.len() {
                if bytes[k] == b'>' {
                    close_pos = Some(k);
                    break;
                }
                if bytes[k] == b'<' {
                    hit_lt = true;
                    break;
                }
                k += 1;
            }
            if !hit_lt && let Some(end) = close_pos {
                flush_literal(&mut literal_buf, &mut tokens);
                tokens.push(FlashHtmlToken::EndTag(name));
                i = end + 1;
                continue;
            }
            literal_buf.push('<');
            i += 1;
            continue;
        }
        // Opening tag <name ...>
        if i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            let name_start = i + 1;
            let mut j = name_start;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'_' | b'-' | b':'))
            {
                j += 1;
            }
            if j < bytes.len()
                && !(bytes[j].is_ascii_whitespace() || bytes[j] == b'/' || bytes[j] == b'>')
            {
                literal_buf.push('<');
                i += 1;
                continue;
            }
            let name = source[name_start..j].to_ascii_lowercase();
            let mut quote: Option<u8> = None;
            let mut close_pos = None;
            let mut hit_unquoted_lt = false;
            let mut k = j;
            while k < bytes.len() {
                let b = bytes[k];
                if let Some(q) = quote {
                    if b == q {
                        quote = None;
                    }
                } else if b == b'"' || b == b'\'' {
                    quote = Some(b);
                } else if b == b'>' {
                    close_pos = Some(k);
                    break;
                } else if b == b'<' {
                    hit_unquoted_lt = true;
                    break;
                }
                k += 1;
            }
            if !hit_unquoted_lt && let Some(end) = close_pos {
                let attr_slice = &source[j..end];
                let trimmed = attr_slice.trim_end();
                let self_closing = trimmed.ends_with('/');
                let clean_attrs = if self_closing {
                    &trimmed[..trimmed.len() - 1]
                } else {
                    attr_slice
                };
                flush_literal(&mut literal_buf, &mut tokens);
                tokens.push(FlashHtmlToken::StartTag {
                    name,
                    attrs: parse_flash_tag_attributes(clean_attrs),
                    self_closing,
                });
                i = end + 1;
                continue;
            }
        }
        // Bare '<' (e.g. "HP < 10", "<3", unclosed "<foo") renders literally.
        literal_buf.push('<');
        i += 1;
    }
    flush_literal(&mut literal_buf, &mut tokens);
    tokens
}

fn parse_html_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_suffix("px")
        .or_else(|| trimmed.strip_suffix("PX"))
        .or_else(|| trimmed.strip_suffix("pt"))
        .or_else(|| trimmed.strip_suffix("PT"))
        .unwrap_or(trimmed)
        .trim();
    let v = stripped.parse::<f64>().ok()?;
    v.is_finite().then_some(v)
}

fn parse_html_color(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    let hex = if let Some(h) = trimmed.strip_prefix('#') {
        h
    } else if let Some(h) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        h
    } else if trimmed.len() == 6 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        trimmed
    } else {
        return None;
    };
    if hex.len() == 3 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut expanded = String::with_capacity(6);
        for ch in hex.chars() {
            expanded.push(ch);
            expanded.push(ch);
        }
        return u32::from_str_radix(&expanded, 16).ok();
    }
    if hex.len() <= 6 && !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return u32::from_str_radix(hex, 16).ok().filter(|v| *v <= 0xffffff);
    }
    None
}

fn canonical_html_tag(tag: &str) -> &str {
    match tag {
        "strong" => "b",
        "em" => "i",
        other => other,
    }
}

fn append_html_slice(
    out: &mut String,
    utf16_cursor: &mut u32,
    runs: &mut Vec<TextFormatRun>,
    slice: &str,
    format: &TextFormat,
) {
    if slice.is_empty() {
        return;
    }
    let units = slice.encode_utf16().count() as u32;
    let start = *utf16_cursor;
    let end = start + units;
    *utf16_cursor = end;
    out.push_str(slice);
    if let Some(last) = runs.last_mut()
        && last.end == start
        && last.format == *format
    {
        last.end = end;
    } else {
        runs.push(TextFormatRun {
            start,
            end,
            format: format.clone(),
        });
    }
}

fn html(
    source: &str,
    base: TextFormat,
    styles: &StyleSheet,
    condense: bool,
    multi_run: bool,
) -> Result<HtmlResult, TextError> {
    if source.len() > MAX_SOURCE {
        return Err(error("htmlText exceeds 1 MiB"));
    }
    styles.validate()?;
    base.validate()?;

    struct OpenFrame {
        tag: String,
        format: TextFormat,
        is_block: bool,
        suppress: bool,
    }

    let tokens = tokenize_flash_html(source);
    let mut result = String::new();
    let mut utf16_cursor = 0u32;
    let mut uniform: Option<TextFormat> = None;
    let mut runs: Vec<TextFormatRun> = Vec::new();
    let mut images: Vec<HtmlImageSpec> = Vec::new();
    let mut stack: Vec<OpenFrame> = Vec::new();

    let close_frame = |frame: OpenFrame,
                       out: &mut String,
                       cursor: &mut u32,
                       out_runs: &mut Vec<TextFormatRun>| {
        if frame.is_block && !frame.suppress && !out.ends_with('\n') {
            append_html_slice(out, cursor, out_runs, "\n", &frame.format);
        }
    };

    for token in tokens {
        match token {
            FlashHtmlToken::Text(mut value) => {
                let suppress = stack.last().is_some_and(|f| f.suppress);
                if suppress {
                    continue;
                }
                let active_format = stack.last().map_or(&base, |f| &f.format);
                if condense {
                    let mut clean = String::new();
                    let mut space = result.ends_with(char::is_whitespace);
                    for c in value.chars() {
                        if c.is_whitespace() {
                            if !space {
                                clean.push(' ');
                            }
                            space = true;
                        } else {
                            clean.push(c);
                            space = false;
                        }
                    }
                    value = clean;
                }
                if !value.trim().is_empty() && uniform.is_none() {
                    uniform = Some(active_format.clone());
                }
                append_html_slice(
                    &mut result,
                    &mut utf16_cursor,
                    &mut runs,
                    &value,
                    active_format,
                );
            }
            FlashHtmlToken::StartTag {
                name: tag,
                attrs,
                self_closing,
            } => {
                let parent_suppress = stack.last().is_some_and(|f| f.suppress);
                if matches!(tag.as_str(), "br" | "sbr") {
                    if !parent_suppress {
                        let active_format = stack.last().map_or(&base, |f| &f.format);
                        append_html_slice(
                            &mut result,
                            &mut utf16_cursor,
                            &mut runs,
                            "\n",
                            active_format,
                        );
                    }
                    continue;
                }
                if tag == "img" {
                    if multi_run && !parent_suppress {
                        let mut id = None;
                        let mut src = String::new();
                        let mut width = None;
                        let mut height = None;
                        let mut hspace = 8.0;
                        let mut vspace = 8.0;
                        let mut align = String::from("left");
                        for (attr_name, value) in &attrs {
                            match attr_name.as_str() {
                                "id" => id = Some(value.clone()),
                                "src" => src = value.clone(),
                                "width" => {
                                    if let Some(v) = parse_html_number(value) {
                                        width = Some(v.clamp(0.0, 100_000.0));
                                    }
                                }
                                "height" => {
                                    if let Some(v) = parse_html_number(value) {
                                        height = Some(v.clamp(0.0, 100_000.0));
                                    }
                                }
                                "hspace" => {
                                    if let Some(v) = parse_html_number(value) {
                                        hspace = v.clamp(0.0, 100_000.0);
                                    }
                                }
                                "vspace" => {
                                    if let Some(v) = parse_html_number(value) {
                                        vspace = v.clamp(0.0, 100_000.0);
                                    }
                                }
                                "align" => {
                                    let m = value.trim().to_ascii_lowercase();
                                    if matches!(m.as_str(), "left" | "right") {
                                        align = m;
                                    }
                                }
                                _ => {}
                            }
                        }
                        images.push(HtmlImageSpec {
                            id,
                            src,
                            width,
                            height,
                            hspace,
                            vspace,
                            align,
                            char_index: utf16_cursor,
                        });
                    }
                    continue;
                }
                if matches!(
                    tag.as_str(),
                    "hr" | "meta" | "link" | "input" | "col" | "area" | "base" | "param" | "wbr"
                ) {
                    continue;
                }
                // Auto-close open <p> on new <p>, or open <li> on sibling <li>.
                if tag == "p"
                    && let Some(pos) = stack.iter().rposition(|f| f.tag == "p")
                {
                    while stack.len() > pos {
                        let f = stack.pop().expect("non-empty");
                        close_frame(f, &mut result, &mut utf16_cursor, &mut runs);
                    }
                } else if tag == "li" {
                    let list_boundary = stack
                        .iter()
                        .rposition(|f| matches!(f.tag.as_str(), "ul" | "ol"))
                        .map_or(0, |idx| idx + 1);
                    if let Some(rel) = stack[list_boundary..].iter().rposition(|f| f.tag == "li") {
                        let pos = list_boundary + rel;
                        while stack.len() > pos {
                            let f = stack.pop().expect("non-empty");
                            close_frame(f, &mut result, &mut utf16_cursor, &mut runs);
                        }
                    }
                }

                let parent_suppress = stack.last().is_some_and(|f| f.suppress);
                let current_format = stack.last().map_or(&base, |f| &f.format).clone();
                let known = matches!(
                    tag.as_str(),
                    "p" | "span"
                        | "b"
                        | "strong"
                        | "i"
                        | "em"
                        | "u"
                        | "font"
                        | "textformat"
                        | "a"
                        | "li"
                        | "ul"
                        | "ol"
                );

                let mut next = current_format.clone();
                if known {
                    if matches!(tag.as_str(), "b" | "strong") {
                        next.bold = true;
                    }
                    if matches!(tag.as_str(), "i" | "em") {
                        next.italic = true;
                    }
                    if tag == "u" {
                        next.underline = true;
                    }
                    if tag == "li" && multi_run {
                        next.bullet = true;
                    }
                    if let Some(style) = styles.styles.get(&tag)
                        && let Ok(patch) = transform(style)
                    {
                        patch.apply(&mut next);
                    }
                    for (name, value) in &attrs {
                        match name.as_str() {
                            "class" => {
                                for class in value.split_whitespace() {
                                    if let Some(style) = styles.styles.get(&format!(".{class}"))
                                        && let Ok(patch) = transform(style)
                                    {
                                        patch.apply(&mut next);
                                    }
                                }
                            }
                            "align" if tag == "p" => {
                                let mode = value.trim().to_ascii_lowercase();
                                match mode.as_str() {
                                    "left" => {
                                        next.align = TextAlign::Left;
                                        if multi_run {
                                            next.set_align_mode(None);
                                        }
                                    }
                                    "center" => {
                                        next.align = TextAlign::Center;
                                        if multi_run {
                                            next.set_align_mode(None);
                                        }
                                    }
                                    "right" => {
                                        next.align = TextAlign::Right;
                                        if multi_run {
                                            next.set_align_mode(None);
                                        }
                                    }
                                    "justify" => {
                                        next.align = TextAlign::Left;
                                        if multi_run {
                                            next.set_align_mode(Some(mode));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "href" if tag == "a" => {
                                let clean: String = value
                                    .chars()
                                    .filter(|c| !c.is_control())
                                    .take(4096)
                                    .collect();
                                next.url = clean;
                            }
                            "target" if tag == "a" => {
                                let clean: String = value
                                    .chars()
                                    .filter(|c| !c.is_control())
                                    .take(256)
                                    .collect();
                                next.set_target_str(clean);
                            }
                            "letterspacing" if tag == "font" => {
                                if let Some(v) = parse_html_number(value) {
                                    next.letter_spacing = v.clamp(-1000.0, 1000.0);
                                }
                            }
                            "kerning" if tag == "font" => match value.trim() {
                                "1" | "true" => next.kerning = Some(true),
                                "0" | "false" => next.kerning = Some(false),
                                _ => {}
                            },
                            "face" if tag == "font" => {
                                let clean: String =
                                    value.chars().filter(|c| !c.is_control()).collect();
                                let trimmed = clean.trim();
                                if !trimmed.is_empty() && trimmed.len() <= 1024 {
                                    next.font = trimmed.to_owned();
                                }
                            }
                            "size" if tag == "font" => {
                                let trimmed = value.trim();
                                if (trimmed.starts_with('+') || trimmed.starts_with('-'))
                                    && let Some(delta) = parse_html_number(trimmed)
                                {
                                    next.size = (next.size + delta).clamp(1.0, 10000.0);
                                } else if let Some(sz) = parse_html_number(trimmed)
                                    && sz > 0.0
                                {
                                    next.size = sz.clamp(1.0, 10000.0);
                                }
                            }
                            "color" if tag == "font" => {
                                if let Some(c) = parse_html_color(value) {
                                    next.color = c;
                                }
                            }
                            "leftmargin" if tag == "textformat" => {
                                if let Some(v) = parse_html_number(value) {
                                    next.left_margin = v.clamp(0.0, 10000.0);
                                }
                            }
                            "rightmargin" if tag == "textformat" => {
                                if let Some(v) = parse_html_number(value) {
                                    next.right_margin = v.clamp(0.0, 10000.0);
                                }
                            }
                            "indent" if tag == "textformat" => {
                                if let Some(v) = parse_html_number(value) {
                                    next.indent = v.clamp(-10000.0, 10000.0);
                                }
                            }
                            "blockindent" if tag == "textformat" => {
                                if let Some(v) = parse_html_number(value) {
                                    next.block_indent = v.clamp(0.0, 10000.0);
                                }
                            }
                            "leading" if tag == "textformat" => {
                                if let Some(v) = parse_html_number(value) {
                                    next.leading = v.clamp(-10000.0, 10000.0);
                                }
                            }
                            "tabstops" if tag == "textformat" && multi_run => {
                                let mut stops = Vec::new();
                                for part in value
                                    .trim_matches(['[', ']'])
                                    .split([',', ' '])
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                {
                                    if let Some(v) = parse_html_number(part)
                                        && (0.0..=100_000.0).contains(&v)
                                    {
                                        stops.push(v);
                                    }
                                }
                                stops.sort_by(|a, b| a.total_cmp(b));
                                stops.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                                stops.truncate(64);
                                next.tab_stops = stops;
                            }
                            _ => {}
                        }
                    }
                    if next.validate().is_err() {
                        next = current_format;
                    }
                }

                let suppress = parent_suppress || (multi_run && next.display() == Some("none"));
                let is_block = known
                    && (matches!(tag.as_str(), "p" | "li")
                        || (multi_run && next.display() == Some("block") && tag == "span"));
                if is_block && !suppress && !result.is_empty() && !result.ends_with('\n') {
                    append_html_slice(&mut result, &mut utf16_cursor, &mut runs, "\n", &next);
                }
                let frame = OpenFrame {
                    tag: canonical_html_tag(&tag).to_owned(),
                    format: next,
                    is_block,
                    suppress,
                };
                if self_closing {
                    close_frame(frame, &mut result, &mut utf16_cursor, &mut runs);
                } else {
                    stack.push(frame);
                }
            }
            FlashHtmlToken::EndTag(raw_tag) => {
                let target = canonical_html_tag(&raw_tag);
                if let Some(pos) = stack.iter().rposition(|f| f.tag == target) {
                    while stack.len() > pos {
                        let f = stack.pop().expect("non-empty");
                        close_frame(f, &mut result, &mut utf16_cursor, &mut runs);
                    }
                }
            }
        }
    }

    while let Some(frame) = stack.pop() {
        close_frame(frame, &mut result, &mut utf16_cursor, &mut runs);
    }

    let representative = if multi_run {
        runs.first()
            .map(|r| r.format.clone())
            .or(uniform)
            .unwrap_or(base)
    } else {
        uniform.unwrap_or(base)
    };
    let out_runs = if !multi_run || runs.len() <= 1 {
        Vec::new()
    } else {
        runs
    };
    Ok(HtmlResult {
        text: result,
        format: representative,
        format_runs: out_runs,
        images,
    })
}

pub(crate) fn parse_authored_html(
    source: &str,
    base: TextFormat,
    condense: bool,
) -> Result<(String, TextFormat, Vec<TextFormatRun>), TextError> {
    let parsed = html(source, base.clone(), &StyleSheet::default(), condense, true)?;
    let mut runs = parsed.format_runs;
    if runs.is_empty() && !parsed.text.is_empty() && parsed.format != base {
        runs.push(TextFormatRun {
            start: 0,
            end: parsed.text.encode_utf16().count() as u32,
            format: parsed.format.clone(),
        });
    }
    Ok((parsed.text, parsed.format, runs))
}

fn serialize_single_run_html(text: &str, format: &TextFormat) -> String {
    let escape = |value: &str| {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let mut body = escape(text).replace('\n', "<br/>");
    if format.bold {
        body = format!("<b>{body}</b>");
    }
    if format.italic {
        body = format!("<i>{body}</i>");
    }
    if format.underline {
        body = format!("<u>{body}</u>");
    }
    if !format.url.is_empty() {
        body = format!(
            "<a href=\"{}\" target=\"{}\">{body}</a>",
            escape(&format.url),
            escape(format.target_str())
        );
    }
    let body = format!(
        "<font face=\"{}\" size=\"{}\" color=\"#{:06X}\">{body}</font>",
        escape(&format.font),
        format.size,
        format.color
    );
    format!(
        "<textformat leftmargin=\"{}\" rightmargin=\"{}\" indent=\"{}\" blockindent=\"{}\" leading=\"{}\">{body}</textformat>",
        format.left_margin, format.right_margin, format.indent, format.block_indent, format.leading
    )
}

fn utf16_slice(text: &str, start_u16: u32, end_u16: u32) -> &str {
    let mut cur = 0u32;
    let mut s_byte = text.len();
    let mut e_byte = text.len();
    let mut got_s = false;
    for (b, ch) in text.char_indices() {
        if !got_s && cur >= start_u16 {
            s_byte = b;
            got_s = true;
        }
        if cur >= end_u16 {
            e_byte = b;
            return &text[s_byte..e_byte];
        }
        cur += ch.len_utf16() as u32;
    }
    if !got_s { "" } else { &text[s_byte..e_byte] }
}

pub fn execute(request: Request) -> Result<Value, TextError> {
    match request {
        Request::HitLink {
            field,
            layout,
            x,
            y,
        } => {
            field.validate()?;
            if !x.is_finite() || !y.is_finite() {
                return Err(error("Text link hit requires finite local coordinates"));
            }
            if field
                .box_size
                .is_some_and(|b| x < 0. || y < 0. || x > b.width || y > b.height)
            {
                return Ok(Value::Null);
            }
            if !field.format_runs.is_empty() {
                for run in &layout.runs {
                    if let Some(rfmt) = &run.format
                        && !rfmt.url.is_empty()
                        && !run.text.trim().is_empty()
                    {
                        let approx_w = run.text.chars().count() as f64 * rfmt.size * 0.65;
                        let run_h = rfmt.size * 1.25;
                        if x >= run.x && x <= run.x + approx_w && y >= run.y && y <= run.y + run_h {
                            return Ok(json!({"url": rfmt.url, "target": rfmt.target_str()}));
                        }
                    }
                }
                return Ok(Value::Null);
            }
            if field.format.url.is_empty() {
                return Ok(Value::Null);
            }
            let hit = layout.lines.iter().any(|line| {
                !line.text.trim().is_empty()
                    && x >= line.x
                    && x <= line.x + line.width
                    && y >= line.y
                    && y <= line.y + line.height
            });
            Ok(if hit {
                json!({"url":field.format.url,"target":field.format.target_str()})
            } else {
                Value::Null
            })
        }
        Request::Wheel {
            field,
            layout,
            delta,
            mode,
        } => {
            field.validate()?;
            if !delta.is_finite() || delta.abs() > 1e9 || mode > 2 {
                return Err(error(
                    "TextField wheel requires finite delta and DOM mode0..2",
                ));
            }
            let current = layout.viewport.scroll;
            if !field.as2_viewport.mouse_wheel_enabled || delta == 0. {
                return Ok(json!({"scroll":current,"handled":false}));
            }
            let units = match mode {
                0 => delta / 40.,
                1 => delta,
                _ => delta * f64::from(layout.viewport.bottom_scroll.saturating_sub(current) + 1),
            };
            let step = units.abs().ceil().min(1000.) as i64 * if units < 0. { -1 } else { 1 };
            let scroll = (i64::from(current) + step)
                .clamp(1, i64::from(layout.viewport.maxscroll.max(1)))
                as u32;
            Ok(json!({"scroll":scroll,"handled":layout.viewport.maxscroll>1}))
        }
        Request::ParseCss { state, source } => {
            state.validate()?;
            match css(&source, &state) {
                Ok(next) => Ok(json!({"state":next,"result":true})),
                Err(reason) => {
                    Ok(json!({"state":state,"result":false,"diagnostic":reason.to_string()}))
                }
            }
        }
        Request::SetStyle {
            mut state,
            selector: name,
            style,
        } => {
            state.validate()?;
            selector(&name)?;
            if let Some(style) = style {
                validate_style(&style)?;
                state.styles.insert(name, style);
            } else {
                state.styles.remove(&name);
            }
            state.validate()?;
            Ok(json!({"state":state}))
        }
        Request::GetStyle { state, selector } => {
            state.validate()?;
            Ok(json!({"result":state.styles.get(&selector)}))
        }
        Request::Names { state } => {
            state.validate()?;
            Ok(json!({"result":state.styles.keys().collect::<Vec<_>>()}))
        }
        Request::Clear => Ok(json!({"state":StyleSheet::default()})),
        Request::Transform { style } => Ok(json!({"result":transform(&style)?})),
        Request::Html {
            source,
            format,
            styles,
            condense_white,
            multi_run,
        } => Ok(
            serde_json::to_value(html(&source, format, &styles, condense_white, multi_run)?)
                .map_err(|e| error(e.to_string()))?,
        ),
        Request::SerializeHtml {
            text,
            format,
            format_runs,
        } => {
            format.validate()?;
            if text.len() > MAX_SOURCE {
                return Err(error("HTML source exceeds 1 MiB"));
            }
            if format_runs.is_empty() {
                return Ok(json!({"result": serialize_single_run_html(&text, &format)}));
            }
            let mut out = String::new();
            for run in &format_runs {
                run.format.validate()?;
                let sub = utf16_slice(&text, run.start, run.end);
                if !sub.is_empty() {
                    out.push_str(&serialize_single_run_html(sub, &run.format));
                }
            }
            Ok(json!({"result": out}))
        }
        Request::EscapeHtml { text } => {
            if text.len() > MAX_SOURCE {
                return Err(error("HTML source exceeds 1 MiB"));
            }
            Ok(
                json!({"result":text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\n',"<br/>")}),
            )
        }
        Request::ExtentResult { layout, format } => {
            format.validate()?;
            Ok(
                json!({"width":layout.text_width,"height":layout.text_height,"textFieldWidth":layout.text_width+4.,"textFieldHeight":layout.text_height+4.,"ascent":format.size,"descent":format.size*0.2}),
            )
        }
        Request::ExtentField {
            text,
            format,
            width,
        } => {
            let mut field = TextFieldState {
                text: crate::text::normalize_text(&text),
                format: format.clone(),
                default_format: format,
                multiline: true,
                word_wrap: width.is_some(),
                box_size: width.map(|width| crate::TextBoxSize {
                    width,
                    height: 1_000_000.,
                }),
                ..TextFieldState::default()
            };
            field.selection_begin = 0;
            field.selection_end = 0;
            field.validate()?;
            Ok(json!({"field":field}))
        }
        Request::FieldRenderSet {
            mut state,
            name,
            value,
        } => {
            state.validate()?;
            match name.as_str() {
                "embedFonts" => {
                    state.embed_fonts = value
                        .as_bool()
                        .ok_or_else(|| error("TextField.embedFonts requires a Boolean"))?;
                }
                "antiAliasType" => {
                    let mode = value.as_str().ok_or_else(|| {
                        error("TextField.antiAliasType requires 'normal' or 'advanced'")
                    })?;
                    state.anti_alias_type = mode.into();
                }
                "gridFitType" => {
                    let mode = value.as_str().ok_or_else(|| {
                        error("TextField.gridFitType requires 'none', 'pixel' or 'subpixel'")
                    })?;
                    state.grid_fit_type = mode.into();
                }
                "sharpness" => {
                    let number = value
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| error("TextField.sharpness requires a finite Number"))?;
                    state.sharpness = number.clamp(-400.0, 400.0);
                }
                "thickness" => {
                    let number = value
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| error("TextField.thickness requires a finite Number"))?;
                    state.thickness = number.clamp(-200.0, 200.0);
                }
                _ => return Err(error("Unknown AS2 TextField render property")),
            }
            state.validate()?;
            Ok(json!({"state":state}))
        }
        Request::FontList { candidates } => {
            if candidates.len() > 4096 {
                return Err(error("TextField.getFontList exceeds 4096 font candidates"));
            }
            let mut deduped = BTreeMap::<String, String>::new();
            for candidate in candidates {
                let clean = candidate.trim().trim_matches(['\'', '"']);
                if clean.is_empty() || clean.len() > 256 || clean.chars().any(char::is_control) {
                    continue;
                }
                let key = clean.to_ascii_lowercase();
                deduped.entry(key).or_insert_with(|| clean.to_string());
            }
            let mut fonts: Vec<String> = deduped.into_values().collect();
            fonts.sort_by(|a, b| {
                a.to_ascii_lowercase()
                    .cmp(&b.to_ascii_lowercase())
                    .then_with(|| a.cmp(b))
            });
            Ok(json!({"result":fonts}))
        }
        Request::RendererSet {
            mut state,
            name,
            value,
        } => {
            state.validate()?;
            match name.as_str() {
                "displayMode" => {
                    let mode = value.as_str().ok_or_else(|| {
                        error("TextRenderer.displayMode requires 'default', 'lcd' or 'crt'")
                    })?;
                    state.display_mode = mode.into();
                }
                "maxLevel" => {
                    let level = value
                        .as_i64()
                        .map(|v| v.clamp(0, 64) as i32)
                        .or_else(|| {
                            value
                                .as_f64()
                                .filter(|v| v.is_finite())
                                .map(|v| (v.trunc() as i64).clamp(0, 64) as i32)
                        })
                        .ok_or_else(|| error("TextRenderer.maxLevel requires a finite integer"))?;
                    state.max_level = level;
                }
                _ => return Err(error("Unknown TextRenderer property")),
            }
            state.validate()?;
            Ok(json!({"state":state}))
        }
        Request::RendererTable {
            mut state,
            font_name,
            font_style,
            color_type,
            table,
        } => {
            state.validate()?;
            let clean_font = font_name.trim();
            if clean_font.is_empty()
                || clean_font.len() > 256
                || clean_font.chars().any(char::is_control)
            {
                return Err(error(
                    "TextRenderer.setAdvancedAntialiasingTable requires a nonempty fontName up to 256 bytes",
                ));
            }
            if !matches!(
                font_style.as_str(),
                "none" | "bold" | "italic" | "boldItalic"
            ) {
                return Err(error(
                    "TextRenderer fontStyle requires 'none', 'bold', 'italic' or 'boldItalic'",
                ));
            }
            if !matches!(color_type.as_str(), "dark" | "light") {
                return Err(error("TextRenderer colorType requires 'dark' or 'light'"));
            }
            if table.len() > 256 {
                return Err(error("TextRenderer antialiasing table exceeds 256 entries"));
            }
            for entry in &table {
                if ![entry.font_size, entry.inside_cutoff, entry.outside_cutoff]
                    .into_iter()
                    .all(f64::is_finite)
                    || entry.font_size <= 0.0
                    || entry.font_size > 4096.0
                    || entry.inside_cutoff.abs() > 4096.0
                    || entry.outside_cutoff.abs() > 4096.0
                {
                    return Err(error(
                        "TextRenderer table entries require positive fontSize and finite cutoffs",
                    ));
                }
            }
            let key = format!("{clean_font}\0{font_style}\0{color_type}");
            if !state.tables.contains_key(&key) && state.tables.len() >= 256 {
                return Err(error("TextRenderer antialiasing tables exceed 256 entries"));
            }
            state.tables.insert(key, table);
            Ok(json!({"state":state}))
        }
        Request::RendererResolve {
            state,
            font_name,
            bold,
            italic,
            rgb,
            font_size,
        } => {
            state.validate()?;
            let clean_font = font_name.trim().trim_matches(['"', '\'']);
            let style_key = match (bold, italic) {
                (true, true) => "boldItalic",
                (true, false) => "bold",
                (false, true) => "italic",
                (false, false) => "none",
            };
            let r = f64::from((rgb >> 16) & 255);
            let g = f64::from((rgb >> 8) & 255);
            let b = f64::from(rgb & 255);
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let color_type = if lum >= 128.0 { "light" } else { "dark" };
            let sz = if font_size.is_finite() && font_size > 0.0 {
                font_size
            } else {
                12.0
            };

            let exact_key = format!("{clean_font}\0{style_key}\0{color_type}");
            let fallback_style_key = format!("{clean_font}\0none\0{color_type}");
            let matched = state
                .tables
                .get(&exact_key)
                .or_else(|| state.tables.get(&fallback_style_key))
                .or_else(|| {
                    let lower_font = clean_font.to_ascii_lowercase();
                    state
                        .tables
                        .iter()
                        .find(|(k, _)| {
                            let mut parts = k.split('\0');
                            let f = parts.next().unwrap_or("");
                            let s = parts.next().unwrap_or("");
                            let c = parts.next().unwrap_or("");
                            f.to_ascii_lowercase() == lower_font
                                && c == color_type
                                && (s == style_key || s == "none")
                        })
                        .map(|(_, v)| v)
                });

            let mut has_csm = false;
            let mut inside = 0.5f64;
            let mut outside = -0.5f64;
            if let Some(entries) = matched
                && !entries.is_empty()
            {
                has_csm = true;
                let mut sorted = entries.clone();
                sorted.sort_by(|a, b| a.font_size.total_cmp(&b.font_size));
                if sz <= sorted[0].font_size || sorted.len() == 1 {
                    inside = sorted[0].inside_cutoff;
                    outside = sorted[0].outside_cutoff;
                } else if sz >= sorted.last().unwrap().font_size {
                    let last = sorted.last().unwrap();
                    inside = last.inside_cutoff;
                    outside = last.outside_cutoff;
                } else {
                    for pair in sorted.windows(2) {
                        if sz >= pair[0].font_size && sz <= pair[1].font_size {
                            let span = (pair[1].font_size - pair[0].font_size).max(1e-6);
                            let t = ((sz - pair[0].font_size) / span).clamp(0.0, 1.0);
                            inside = pair[0].inside_cutoff
                                + (pair[1].inside_cutoff - pair[0].inside_cutoff) * t;
                            outside = pair[0].outside_cutoff
                                + (pair[1].outside_cutoff - pair[0].outside_cutoff) * t;
                            break;
                        }
                    }
                }
            }
            let stroke_delta = if has_csm {
                ((inside + outside) * 0.5).clamp(-1.5, 1.5)
            } else {
                0.0
            };
            let contrast_scale = if has_csm {
                (1.0 + (inside - outside - 1.0) * 0.25).clamp(0.5, 1.6)
            } else {
                1.0
            };
            let aa_levels = 1u32 << (state.max_level.clamp(0, 8) as u32);
            Ok(json!({
                "displayMode": state.display_mode,
                "maxLevel": state.max_level,
                "aaLevels": aa_levels,
                "hasCsm": has_csm,
                "insideCutoff": inside,
                "outsideCutoff": outside,
                "strokeDelta": stroke_delta,
                "contrastScale": contrast_scale,
                "colorType": color_type,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_render_props_font_list_and_text_renderer_validate_and_clamp() {
        let state = TextFieldRenderProps::default();
        let next = execute(Request::FieldRenderSet {
            state: state.clone(),
            name: "sharpness".into(),
            value: json!(950.0),
        })
        .unwrap();
        assert_eq!(next["state"]["sharpness"], 400.0);
        let next = execute(Request::FieldRenderSet {
            state,
            name: "thickness".into(),
            value: json!(-500.0),
        })
        .unwrap();
        assert_eq!(next["state"]["thickness"], -200.0);
        assert!(
            execute(Request::FieldRenderSet {
                state: TextFieldRenderProps::default(),
                name: "antiAliasType".into(),
                value: json!("bogus"),
            })
            .is_err()
        );

        let fonts = execute(Request::FontList {
            candidates: vec![
                " Verdana ".into(),
                "sans-serif".into(),
                "verdana".into(),
                "\"Arial\"".into(),
                "".into(),
            ],
        })
        .unwrap();
        assert_eq!(fonts["result"], json!(["Arial", "sans-serif", "Verdana"]));

        let renderer = execute(Request::RendererSet {
            state: TextRendererState::default(),
            name: "displayMode".into(),
            value: json!("lcd"),
        })
        .unwrap();
        assert_eq!(renderer["state"]["displayMode"], "lcd");
        assert_eq!(renderer["state"]["maxLevel"], 4);

        let renderer_state: TextRendererState =
            serde_json::from_value(renderer["state"].clone()).unwrap();
        let table = execute(Request::RendererTable {
            state: renderer_state,
            font_name: "Arial".into(),
            font_style: "bold".into(),
            color_type: "dark".into(),
            table: vec![AntialiasEntry {
                font_size: 12.0,
                inside_cutoff: 0.65,
                outside_cutoff: -0.35,
            }],
        })
        .unwrap();
        assert_eq!(
            table["state"]["tables"]["Arial\0bold\0dark"][0]["fontSize"],
            12.0
        );
    }

    #[test]
    fn permissive_flash_html_handles_malformed_entities_tags_and_base_inheritance() {
        let mut base = TextFormat {
            font: "Courier New".into(),
            size: 13.0,
            color: 0xBBF7D0,
            bold: false,
            italic: true,
            underline: false,
            leading: 3.0,
            align: TextAlign::Center,
            ..TextFormat::default()
        };
        base.set_align_mode(None);

        // Bare '&', bare '<', '<3', unclosed '<foo', unknown tags, mismatched '<b><i>text</b></i>', unclosed '<font>'
        let parsed = html(
            "Rats & Brood | HP < 10 | <3 | <custom attr='1'>inner</custom> | <b><i>overlap</b></i> | <font color='#FACC15'>Gold &amp; &#65;&#x42;",
            base.clone(),
            &StyleSheet::default(),
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            parsed.text,
            "Rats & Brood | HP < 10 | <3 | inner | overlap | Gold & AB"
        );
        assert!(parsed.format_runs.len() >= 3);
        // Unwrapped prefix inherits base format
        assert_eq!(parsed.format_runs[0].format.color, 0xBBF7D0);
        assert_eq!(parsed.format_runs[0].format.font, "Courier New");
        assert_eq!(parsed.format_runs[0].format.size, 13.0);
        assert!(parsed.format_runs[0].format.italic);
        assert_eq!(parsed.format_runs[0].format.align, TextAlign::Center);
        assert_eq!(parsed.format_runs[0].format.leading, 3.0);
        // Gold run overrides color while inheriting base font, size, italic, align, leading
        let gold_run = parsed.format_runs.last().unwrap();
        assert_eq!(gold_run.format.color, 0xFACC15);
        assert_eq!(gold_run.format.font, "Courier New");
        assert_eq!(gold_run.format.size, 13.0);
        assert!(gold_run.format.italic);

        // Unclosed '<foo' at end of string renders literally without throwing
        let unclosed = html(
            "Alert <foo",
            base.clone(),
            &StyleSheet::default(),
            false,
            true,
        )
        .unwrap();
        assert_eq!(unclosed.text, "Alert <foo");
        assert_eq!(unclosed.format.color, 0xBBF7D0);

        // Single-run mode (multi_run == false) coalesces mixed formatting cleanly without throwing
        let coalesced = html(
            "Plain <b>Bold</b> <font color='#FF0000'>Red</font>",
            base.clone(),
            &StyleSheet::default(),
            false,
            false,
        )
        .unwrap();
        assert_eq!(coalesced.text, "Plain Bold Red");
        assert!(coalesced.format_runs.is_empty());
        assert_eq!(coalesced.format.color, 0xBBF7D0);
    }
}
