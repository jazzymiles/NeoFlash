//! Bounded AS2 styles and uniform HTML, adapted to NeoFlash's owned text model.
//! CSS/TextFormat mappings follow Ruffle MIT v0.7.1 (89a7049965f3090e706265670df309a5128233ac).
use crate::{TextAlign, TextError, TextFieldState, TextFormat, TextFormatPatch};
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
    },
    EscapeHtml {
        text: String,
    },
    SerializeHtml {
        text: String,
        format: TextFormat,
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
                patch.align = Some(match text(value)?.to_ascii_lowercase().as_str() {
                    "left" => TextAlign::Left,
                    "center" => TextAlign::Center,
                    "right" => TextAlign::Right,
                    _ => return Err(error("CSS textAlign supports left/center/right")),
                })
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
#[derive(Serialize)]
struct HtmlResult {
    text: String,
    format: TextFormat,
}
fn html(
    source: &str,
    base: TextFormat,
    styles: &StyleSheet,
    condense: bool,
) -> Result<HtmlResult, TextError> {
    if source.len() > MAX_SOURCE {
        return Err(error("htmlText exceeds 1 MiB"));
    }
    styles.validate()?;
    base.validate()?;
    let source = source
        .replace("<br>", "<br/>")
        .replace("<BR>", "<BR/>")
        .replace("&nbsp;", "&#160;");
    let tree = crate::parse_xml(&source)
        .map_err(|e| error(format!("htmlText requires balanced supported tags: {e:?}")))?;
    let mut result = String::new();
    let mut uniform: Option<TextFormat> = None;
    fn visit(
        index: usize,
        tree: &crate::XmlParseTree,
        format: &TextFormat,
        styles: &StyleSheet,
        condense: bool,
        out: &mut String,
        uniform: &mut Option<TextFormat>,
    ) -> Result<(), TextError> {
        let node = &tree.nodes[index];
        if node.kind == crate::XmlNodeKind::Text {
            let mut value = node.value.clone();
            if condense {
                let mut clean = String::new();
                let mut space = out.ends_with(char::is_whitespace);
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
            if !value.trim().is_empty() {
                if uniform.as_ref().is_some_and(|existing| existing != format) {
                    return Err(error(
                        "htmlText contains mixed formatting; this batch supports one uniform TextFormat across visible text",
                    ));
                }
                *uniform = Some(format.clone());
            }
            out.push_str(&value);
            return Ok(());
        }
        if node.kind == crate::XmlNodeKind::Comment {
            return Ok(());
        }
        if node.kind != crate::XmlNodeKind::Element {
            return Err(error("HTML processing instructions are unsupported"));
        }
        let tag = node.name.as_ref().unwrap().local.to_ascii_lowercase();
        if tag == "br" {
            out.push('\n');
            return Ok(());
        }
        if !matches!(
            tag.as_str(),
            "p" | "span" | "b" | "strong" | "i" | "em" | "u" | "font" | "textformat" | "a"
        ) {
            return Err(error(format!(
                "htmlText tag <{tag}> is unsupported; content was not discarded"
            )));
        }
        let mut next = format.clone();
        if matches!(tag.as_str(), "b" | "strong") {
            next.bold = true;
        }
        if matches!(tag.as_str(), "i" | "em") {
            next.italic = true;
        }
        if tag == "u" {
            next.underline = true;
        }
        if let Some(style) = styles.styles.get(&tag) {
            transform(style)?.apply(&mut next);
        }
        for attr in &node.attributes {
            let name = attr.name.local.to_ascii_lowercase();
            let value = &attr.value;
            match name.as_str() {
                "class" => {
                    for class in value.split_whitespace() {
                        if let Some(style) = styles.styles.get(&format!(".{class}")) {
                            transform(style)?.apply(&mut next);
                        }
                    }
                }
                "align" if tag == "p" => {
                    next.align = match value.as_str() {
                        "left" => TextAlign::Left,
                        "center" => TextAlign::Center,
                        "right" => TextAlign::Right,
                        _ => return Err(error("HTML paragraph alignment is unsupported")),
                    };
                }
                "href" if tag == "a" => next.url = value.clone(),
                "target" if tag == "a" => next.target = value.clone(),
                "letterspacing" if tag == "font" => next.letter_spacing = number(&json!(value))?,
                "kerning" if tag == "font" => {
                    next.kerning = Some(match value.as_str() {
                        "1" | "true" => true,
                        "0" | "false" => false,
                        _ => return Err(error("font kerning requires0/1")),
                    })
                }
                "face" if tag == "font" => next.font = value.clone(),
                "size" if tag == "font" => next.size = number(&json!(value))?,
                "color" if tag == "font" => next.color = color(&json!(value))?,
                "leftmargin" if tag == "textformat" => next.left_margin = number(&json!(value))?,
                "rightmargin" if tag == "textformat" => next.right_margin = number(&json!(value))?,
                "indent" if tag == "textformat" => next.indent = number(&json!(value))?,
                "blockindent" if tag == "textformat" => next.block_indent = number(&json!(value))?,
                "leading" if tag == "textformat" => next.leading = number(&json!(value))?,
                _ => {
                    return Err(error(format!(
                        "htmlText attribute '{name}' on <{tag}> is unsupported"
                    )));
                }
            }
        }
        next.validate()?;
        if tag == "p" && !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for child in &node.children {
            visit(*child, tree, &next, styles, condense, out, uniform)?;
        }
        if tag == "p" && !out.ends_with('\n') {
            out.push('\n');
        }
        Ok(())
    }
    for root in &tree.roots {
        visit(
            *root,
            &tree,
            &base,
            styles,
            condense,
            &mut result,
            &mut uniform,
        )?;
    }
    Ok(HtmlResult {
        text: result,
        format: uniform.unwrap_or(base),
    })
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
            if field.format.url.is_empty()
                || field
                    .box_size
                    .is_some_and(|b| x < 0. || y < 0. || x > b.width || y > b.height)
            {
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
                json!({"url":field.format.url,"target":field.format.target})
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
        } => Ok(
            serde_json::to_value(html(&source, format, &styles, condense_white)?)
                .map_err(|e| error(e.to_string()))?,
        ),
        Request::SerializeHtml { text, format } => {
            format.validate()?;
            if text.len() > MAX_SOURCE {
                return Err(error("HTML source exceeds 1 MiB"));
            }
            let escape = |value: &str| {
                value
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
            };
            let mut body = escape(&text).replace('\n', "<br/>");
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
                    escape(&format.target)
                );
            }
            let body = format!(
                "<font face=\"{}\" size=\"{}\" color=\"#{:06X}\">{body}</font>",
                escape(&format.font),
                format.size,
                format.color
            );
            Ok(
                json!({"result":format!("<textformat leftmargin=\"{}\" rightmargin=\"{}\" indent=\"{}\" blockindent=\"{}\" leading=\"{}\">{body}</textformat>",format.left_margin,format.right_margin,format.indent,format.block_indent,format.leading)}),
            )
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
                    let level =
                        value
                            .as_i64()
                            .filter(|v| (0..=64).contains(v))
                            .ok_or_else(|| {
                                error("TextRenderer.maxLevel requires an integer in 0..=64")
                            })? as i32;
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
}
