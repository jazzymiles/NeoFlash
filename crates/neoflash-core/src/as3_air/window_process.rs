use super::*;

impl AirRuntime {
    pub fn html_load_url(
        &mut self,
        session_id: u32,
        url: &str,
        is_post: bool,
        fallback_html: Option<&str>,
    ) -> Result<String, AirError> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(AirError::Argument("URL must not be empty.".into()));
        }
        let html_content = if trimmed.eq_ignore_ascii_case("about:blank") {
            String::new()
        } else if let Some(rest) = trimmed.strip_prefix("data:") {
            decode_data_url_to_string(rest)?
        } else if trimmed.starts_with("app:/")
            || trimmed.starts_with("app-storage:/")
            || trimmed.starts_with("file:")
            || trimmed.starts_with('/')
        {
            let vfs_path = normalize_air_path(trimmed)?;
            let bytes = self.fs.read_bytes(&vfs_path)?;
            String::from_utf8_lossy(&bytes).into_owned()
        } else if let Some(body) = fallback_html {
            body.to_owned()
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let safe_title = xml_escape_text(trimmed);
            format!(
                "<!DOCTYPE html><html><head><title>{safe_title}</title></head><body><p>{safe_title}</p></body></html>"
            )
        } else {
            return Err(AirError::Io(format!(
                "Unsupported or unreachable URL '{trimmed}'."
            )));
        };
        self.html
            .load_document(session_id, &html_content, trimmed, trimmed, is_post, true)
    }
}

pub(super) fn xml_escape_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub(super) fn decode_data_url_to_string(rest: &str) -> Result<String, AirError> {
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(AirError::Argument("Malformed data: URL.".into()));
    };
    if meta.to_ascii_lowercase().ends_with(";base64") {
        let decoded = BASE64
            .decode(payload.trim())
            .map_err(|e| AirError::Argument(format!("Invalid base64 in data: URL: {e}")))?;
        Ok(String::from_utf8_lossy(&decoded).into_owned())
    } else {
        Ok(percent_decode_str(payload))
    }
}

pub(super) fn percent_decode_str(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// AIR HTML Document & History Engine (`flash.html.*`, `flash.media.StageWebView`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AirHtmlHistoryEntry {
    pub url: String,
    pub original_url: String,
    pub title: String,
    pub is_post: bool,
    #[serde(skip)]
    html_source: String,
}

#[derive(Debug, Clone)]
pub(super) struct AirHtmlDomElement {
    tag: String,
    id: String,
    text: String,
    href: String,
}

#[derive(Debug, Clone)]
pub(super) struct AirHtmlSession {
    width: f64,
    height: f64,
    scroll_h: f64,
    scroll_v: f64,
    content_width: f64,
    content_height: f64,
    loaded: bool,
    location: String,
    title: String,
    html_source: String,
    plain_text: String,
    has_focusable_content: bool,
    links: Vec<String>,
    scripts: Vec<String>,
    elements: Vec<AirHtmlDomElement>,
    history: Vec<AirHtmlHistoryEntry>,
    history_position: usize,
}

#[derive(Debug, Default)]
pub struct AirHtmlEngine {
    sessions: BTreeMap<u32, AirHtmlSession>,
    next_id: u32,
}

pub(super) struct ParsedHtmlDoc {
    title: String,
    plain_text: String,
    has_focusable: bool,
    links: Vec<String>,
    scripts: Vec<String>,
    elements: Vec<AirHtmlDomElement>,
    content_width: f64,
    content_height: f64,
}

pub(super) fn unescape_html_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub(super) fn extract_attr_value(tag_attrs: &str, attr_name: &str) -> Option<String> {
    let lower = tag_attrs.to_ascii_lowercase();
    let needle = format!("{}=", attr_name.to_ascii_lowercase());
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(&needle) {
        let pos = search_from + rel;
        let ok_prefix = pos == 0
            || tag_attrs.as_bytes()[pos - 1].is_ascii_whitespace()
            || tag_attrs.as_bytes()[pos - 1] == b'"'
            || tag_attrs.as_bytes()[pos - 1] == b'\'';
        if !ok_prefix {
            search_from = pos + needle.len();
            continue;
        }
        let after = tag_attrs[pos + needle.len()..].trim_start();
        if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            return Some(unescape_html_entities(&quoted[..end]));
        } else if let Some(quoted) = after.strip_prefix('\'') {
            let end = quoted.find('\'').unwrap_or(quoted.len());
            return Some(unescape_html_entities(&quoted[..end]));
        } else {
            let end = after
                .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                .unwrap_or(after.len());
            return Some(unescape_html_entities(&after[..end]));
        }
    }
    None
}

pub(super) fn parse_css_px_dimension(style: &str, prop: &str) -> Option<f64> {
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':')
            && k.trim().eq_ignore_ascii_case(prop)
        {
            let num_str = v.trim().trim_end_matches("px").trim();
            if let Ok(val) = num_str.parse::<f64>()
                && val > 0.0
            {
                return Some(val);
            }
        }
    }
    None
}

pub(super) fn parse_html_document(html: &str, viewport_w: f64, viewport_h: f64) -> ParsedHtmlDoc {
    let mut title = String::new();
    let mut text_out = String::new();
    let mut has_focusable = false;
    let mut links = Vec::new();
    let mut scripts = Vec::new();
    let mut elements = Vec::new();
    let mut explicit_w: Option<f64> = None;
    let mut explicit_h: Option<f64> = None;

    let bytes = html.as_bytes();
    let mut i = 0usize;
    let mut in_title = false;
    let mut open_elem: Option<(String, String, String, usize)> = None;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                if let Some(end_rel) = html[i + 4..].find("-->") {
                    i += 4 + end_rel + 3;
                } else {
                    break;
                }
                continue;
            }
            let Some(gt_rel) = html[i + 1..].find('>') else {
                break;
            };
            let raw_tag = html[i + 1..i + 1 + gt_rel].trim();
            i += gt_rel + 2;

            let is_closing = raw_tag.starts_with('/');
            let clean_tag = raw_tag.trim_start_matches('/').trim_end_matches('/').trim();
            let (tag_name_raw, attrs) = match clean_tag.find(char::is_whitespace) {
                Some(sp) => (&clean_tag[..sp], &clean_tag[sp..]),
                None => (clean_tag, ""),
            };
            let tag_lc = tag_name_raw.to_ascii_lowercase();

            if !is_closing && (tag_lc == "script" || tag_lc == "style") {
                let close_pat = format!("</{tag_lc}");
                let lower_rest = html[i..].to_ascii_lowercase();
                if let Some(end_rel) = lower_rest.find(&close_pat) {
                    let body = &html[i..i + end_rel];
                    if tag_lc == "script" {
                        scripts.push(body.to_owned());
                    }
                    let after_close = i + end_rel + close_pat.len();
                    if let Some(close_gt) = html[after_close..].find('>') {
                        i = after_close + close_gt + 1;
                    } else {
                        i = bytes.len();
                    }
                } else {
                    i = bytes.len();
                }
                continue;
            }

            if is_closing {
                if tag_lc == "title" {
                    in_title = false;
                }
                if let Some((open_tag, open_id, open_href, text_start)) = open_elem.take() {
                    let captured = text_out[text_start..].trim().to_owned();
                    elements.push(AirHtmlDomElement {
                        tag: open_tag,
                        id: open_id,
                        text: captured,
                        href: open_href,
                    });
                }
                if matches!(
                    tag_lc.as_str(),
                    "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr"
                ) && !text_out.ends_with('\n')
                {
                    text_out.push('\n');
                }
                continue;
            }

            if tag_lc == "title" {
                in_title = true;
                continue;
            }
            if tag_lc == "br" {
                text_out.push('\n');
            }
            if let Some(style_val) = extract_attr_value(attrs, "style") {
                if let Some(w) = parse_css_px_dimension(&style_val, "width") {
                    explicit_w = Some(explicit_w.map_or(w, |cur| cur.max(w)));
                }
                if let Some(h) = parse_css_px_dimension(&style_val, "height") {
                    explicit_h = Some(explicit_h.map_or(h, |cur| cur.max(h)));
                }
            }
            if matches!(tag_lc.as_str(), "input" | "button" | "textarea" | "select")
                || extract_attr_value(attrs, "tabindex").is_some()
            {
                has_focusable = true;
            }
            let href_val = extract_attr_value(attrs, "href").unwrap_or_default();
            if tag_lc == "a" && !href_val.is_empty() {
                has_focusable = true;
                links.push(href_val.clone());
            }
            if let Some(id_val) = extract_attr_value(attrs, "id")
                && !id_val.is_empty()
            {
                if raw_tag.ends_with('/')
                    || matches!(tag_lc.as_str(), "input" | "img" | "br" | "hr")
                {
                    let val = extract_attr_value(attrs, "value").unwrap_or_default();
                    elements.push(AirHtmlDomElement {
                        tag: tag_lc,
                        id: id_val,
                        text: val,
                        href: href_val,
                    });
                } else {
                    open_elem = Some((tag_lc, id_val, href_val, text_out.len()));
                }
            }
        } else {
            let next_lt = html[i..].find('<').map_or(bytes.len(), |r| i + r);
            let raw_text = unescape_html_entities(&html[i..next_lt]);
            if in_title {
                title.push_str(raw_text.trim());
            } else {
                let collapsed: Vec<&str> = raw_text.split_whitespace().collect();
                if !collapsed.is_empty() {
                    if !text_out.is_empty() && !text_out.ends_with('\n') && !text_out.ends_with(' ')
                    {
                        text_out.push(' ');
                    }
                    text_out.push_str(&collapsed.join(" "));
                }
            }
            i = next_lt;
        }
    }

    let plain_text = text_out.trim().to_owned();
    let lines: Vec<&str> = if plain_text.is_empty() {
        Vec::new()
    } else {
        plain_text.lines().collect()
    };
    let max_line_chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let calc_w = (max_line_chars as f64 * 8.0 + 16.0).max(viewport_w.max(1.0));
    let calc_h = (lines.len() as f64 * 18.0 + 16.0).max(viewport_h.max(1.0));

    ParsedHtmlDoc {
        title,
        plain_text,
        has_focusable,
        links,
        scripts,
        elements,
        content_width: explicit_w.map_or(calc_w, |w| w.max(viewport_w.max(1.0))),
        content_height: explicit_h.map_or(calc_h, |h| h.max(viewport_h.max(1.0))),
    }
}

impl AirHtmlEngine {
    pub fn create_session(&mut self, width: f64, height: f64) -> u32 {
        self.next_id += 1;
        let id = self.next_id;
        let w = width.max(0.0);
        let h = height.max(0.0);
        self.sessions.insert(
            id,
            AirHtmlSession {
                width: w,
                height: h,
                scroll_h: 0.0,
                scroll_v: 0.0,
                content_width: w,
                content_height: h,
                loaded: false,
                location: String::new(),
                title: String::new(),
                html_source: String::new(),
                plain_text: String::new(),
                has_focusable_content: false,
                links: Vec::new(),
                scripts: Vec::new(),
                elements: Vec::new(),
                history: Vec::new(),
                history_position: 0,
            },
        );
        id
    }

    pub fn dispose_session(&mut self, id: u32) {
        self.sessions.remove(&id);
    }

    pub fn load_string(&mut self, id: u32, html: &str, base_url: &str) -> Result<String, AirError> {
        let loc = if base_url.trim().is_empty() {
            "about:blank"
        } else {
            base_url.trim()
        };
        self.load_document(id, html, loc, loc, false, true)
    }

    fn load_document(
        &mut self,
        id: u32,
        html: &str,
        url: &str,
        original_url: &str,
        is_post: bool,
        push_history: bool,
    ) -> Result<String, AirError> {
        let sess = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
        let parsed = parse_html_document(html, sess.width, sess.height);
        sess.loaded = true;
        sess.location = url.to_owned();
        sess.title = parsed.title.clone();
        sess.html_source = html.to_owned();
        sess.plain_text = parsed.plain_text;
        sess.has_focusable_content = parsed.has_focusable;
        sess.links = parsed.links;
        sess.scripts = parsed.scripts;
        sess.elements = parsed.elements;
        sess.content_width = parsed.content_width;
        sess.content_height = parsed.content_height;
        sess.scroll_h = 0.0;
        sess.scroll_v = 0.0;

        if push_history {
            if !sess.history.is_empty() && sess.history_position + 1 < sess.history.len() {
                sess.history.truncate(sess.history_position + 1);
            }
            sess.history.push(AirHtmlHistoryEntry {
                url: url.to_owned(),
                original_url: original_url.to_owned(),
                title: parsed.title,
                is_post,
                html_source: html.to_owned(),
            });
            sess.history_position = sess.history.len() - 1;
        }
        self.state_json(id)
    }

    pub fn history_go(&mut self, id: u32, steps: i32) -> Result<String, AirError> {
        let (target_idx, entry) = {
            let sess = self
                .sessions
                .get(&id)
                .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
            if sess.history.is_empty() {
                return self.state_json(id);
            }
            let target = (sess.history_position as i64) + i64::from(steps);
            if target < 0 || (target as usize) >= sess.history.len() {
                return self.state_json(id);
            }
            let idx = target as usize;
            (idx, sess.history[idx].clone())
        };
        let _ = self.load_document(
            id,
            &entry.html_source,
            &entry.url,
            &entry.original_url,
            entry.is_post,
            false,
        )?;
        if let Some(sess) = self.sessions.get_mut(&id) {
            sess.history_position = target_idx;
        }
        self.state_json(id)
    }

    pub fn reload(&mut self, id: u32) -> Result<String, AirError> {
        let current = {
            let sess = self
                .sessions
                .get(&id)
                .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
            if !sess.loaded {
                return self.state_json(id);
            }
            (sess.html_source.clone(), sess.location.clone())
        };
        self.load_document(id, &current.0, &current.1, &current.1, false, false)
    }

    pub fn cancel(&mut self, id: u32) -> Result<String, AirError> {
        if let Some(sess) = self.sessions.get_mut(&id) {
            sess.loaded = !sess.html_source.is_empty() || !sess.location.is_empty();
        }
        self.state_json(id)
    }

    pub fn set_viewport(
        &mut self,
        id: u32,
        width: f64,
        height: f64,
        scroll_h: f64,
        scroll_v: f64,
    ) -> Result<String, AirError> {
        let sess = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
        sess.width = width.max(0.0);
        sess.height = height.max(0.0);
        if sess.loaded {
            let parsed = parse_html_document(&sess.html_source, sess.width, sess.height);
            sess.content_width = parsed.content_width;
            sess.content_height = parsed.content_height;
        } else {
            sess.content_width = sess.width;
            sess.content_height = sess.height;
        }
        let max_h = (sess.content_width - sess.width).max(0.0);
        let max_v = (sess.content_height - sess.height).max(0.0);
        sess.scroll_h = scroll_h.clamp(0.0, max_h);
        sess.scroll_v = scroll_v.clamp(0.0, max_v);
        self.state_json(id)
    }

    pub fn state_json(&self, id: u32) -> Result<String, AirError> {
        let sess = self
            .sessions
            .get(&id)
            .ok_or_else(|| AirError::Argument(format!("Unknown HTML session {id}.")))?;
        let elems: Vec<serde_json::Value> = sess
            .elements
            .iter()
            .map(|e| {
                serde_json::json!({
                    "tag": e.tag,
                    "id": e.id,
                    "text": e.text,
                    "href": e.href,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "width": sess.width,
            "height": sess.height,
            "scrollH": sess.scroll_h,
            "scrollV": sess.scroll_v,
            "contentWidth": sess.content_width,
            "contentHeight": sess.content_height,
            "loaded": sess.loaded,
            "location": sess.location,
            "title": sess.title,
            "plainText": sess.plain_text,
            "htmlSource": sess.html_source,
            "hasFocusableContent": sess.has_focusable_content,
            "links": sess.links,
            "scripts": sess.scripts,
            "elements": elems,
            "history": sess.history,
            "historyLength": sess.history.len(),
            "historyPosition": sess.history_position,
        })
        .to_string())
    }
}

// ---------------------------------------------------------------------------
// Crisp 5x7 Bitmap Text Viewport Rasterizer (`StageText`, `StageWebView`)
// ---------------------------------------------------------------------------

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '*' => [
            0b00000, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00100, 0b01000,
        ],
        ':' => [
            0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '?' => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [
            0b11111, 0b10001, 0b10101, 0b10101, 0b10101, 0b10001, 0b11111,
        ],
    }
}

/// Computes the 3D orientation unit quaternion `[x, y, z, w, roll, pitch, yaw]` from W3C
/// DeviceOrientation Euler angles `(alpha_deg, beta_deg, gamma_deg)` (Z-X'-Y'' intrinsic Tait-Bryan
/// angles) for `flash.sensors.DeviceRotation` / `flash.events.DeviceRotationEvent`.
pub fn device_rotation_quaternion(alpha_deg: f64, beta_deg: f64, gamma_deg: f64) -> Vec<f64> {
    let deg_to_rad = std::f64::consts::PI / 180.0;
    let z = (if alpha_deg.is_finite() {
        alpha_deg
    } else {
        0.0
    }) * deg_to_rad
        * 0.5;
    let x = (if beta_deg.is_finite() { beta_deg } else { 0.0 }) * deg_to_rad * 0.5;
    let y = (if gamma_deg.is_finite() {
        gamma_deg
    } else {
        0.0
    }) * deg_to_rad
        * 0.5;

    let (cz, sz) = (z.cos(), z.sin());
    let (cx, sx) = (x.cos(), x.sin());
    let (cy, sy) = (y.cos(), y.sin());

    let qx = sx * cy * cz - cx * sy * sz;
    let qy = cx * sy * cz + sx * cy * sz;
    let qz = cx * cy * sz + sx * sy * cz;
    let qw = cx * cy * cz - sx * sy * sz;

    let norm = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
    let (nx, ny, nz, nw) = if norm > 1e-12 {
        (qx / norm, qy / norm, qz / norm, qw / norm)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    vec![nx, ny, nz, nw, gamma_deg, beta_deg, alpha_deg]
}

fn parse_selection_range(flags_align: &str) -> Option<(usize, usize)> {
    for part in flags_align.split(':') {
        if let Some(sel) = part.strip_prefix("sel=")
            && let Some((a, b)) = sel.split_once(',')
            && let (Ok(i1), Ok(i2)) = (a.parse::<usize>(), b.parse::<usize>())
        {
            let lo = i1.min(i2);
            let hi = i1.max(i2);
            if lo < hi {
                return Some((lo, hi));
            }
        }
    }
    None
}

pub fn rasterize_text_rgba(
    width: u32,
    height: u32,
    text: &str,
    fg_rgb: u32,
    bg_argb: u32,
    font_size: u32,
    flags_align: &str,
) -> Vec<u8> {
    let w = width.clamp(1, 4096) as usize;
    let h = height.clamp(1, 4096) as usize;
    let bg_a = ((bg_argb >> 24) & 0xff) as u8;
    let bg_r = ((bg_argb >> 16) & 0xff) as u8;
    let bg_g = ((bg_argb >> 8) & 0xff) as u8;
    let bg_b = (bg_argb & 0xff) as u8;

    let fg_r = ((fg_rgb >> 16) & 0xff) as u8;
    let fg_g = ((fg_rgb >> 8) & 0xff) as u8;
    let fg_b = (fg_rgb & 0xff) as u8;

    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        *px = [bg_r, bg_g, bg_b, bg_a];
    }

    let password = flags_align.contains("password");
    let multiline = !flags_align.contains("single");
    let align_center = flags_align.contains("center");
    let align_right = flags_align.contains("right");
    let sel_range = parse_selection_range(flags_align);

    let scale = (font_size.max(8) / 8).clamp(1, 6) as usize;
    let adv_x = 6 * scale;
    let adv_y = 9 * scale;
    let pad = 2 * scale;
    let max_cols = ((w.saturating_sub(pad * 2)) / adv_x).max(1);

    let display_text: String = if password {
        text.chars()
            .map(|c| if c == '\n' { '\n' } else { '*' })
            .collect()
    } else {
        text.to_owned()
    };

    let mut lines: Vec<Vec<(usize, char)>> = Vec::new();
    let mut global_idx = 0usize;
    for (raw_idx, raw_line) in display_text.split('\n').enumerate() {
        if raw_idx > 0 {
            global_idx += 1; // account for '\n'
        }
        let chars: Vec<(usize, char)> = raw_line
            .chars()
            .map(|ch| {
                let idx = global_idx;
                global_idx += 1;
                (idx, ch)
            })
            .collect();
        if !multiline {
            lines.push(chars);
            break;
        }
        if chars.is_empty() {
            lines.push(Vec::new());
        } else {
            for chunk in chars.chunks(max_cols) {
                lines.push(chunk.to_vec());
            }
        }
    }

    for (line_idx, line_chars) in lines.iter().enumerate() {
        let base_y = pad + line_idx * adv_y;
        if base_y >= h {
            break;
        }
        let line_px_w = line_chars.len() * adv_x;
        let base_x = if align_center && line_px_w + pad * 2 < w {
            (w - line_px_w) / 2
        } else if align_right && line_px_w + pad < w {
            w - line_px_w - pad
        } else {
            pad
        };

        for (ci, &(char_idx, ch)) in line_chars.iter().enumerate() {
            let gx = base_x + ci * adv_x;
            if gx >= w {
                break;
            }
            let selected = sel_range.is_some_and(|(lo, hi)| char_idx >= lo && char_idx < hi);
            if selected {
                for py in base_y..(base_y + 7 * scale).min(h) {
                    for px in gx..(gx + adv_x).min(w) {
                        let off = (py * w + px) * 4;
                        rgba[off] = 0x33;
                        rgba[off + 1] = 0x90;
                        rgba[off + 2] = 0xff;
                        rgba[off + 3] = 0xff;
                    }
                }
            }
            let rows = glyph_5x7(ch);
            for (ry, &bits) in rows.iter().enumerate() {
                for rx in 0..5usize {
                    if (bits & (1 << (4 - rx))) != 0 {
                        for sy in 0..scale {
                            let py = base_y + ry * scale + sy;
                            if py >= h {
                                continue;
                            }
                            for sx in 0..scale {
                                let px = gx + rx * scale + sx;
                                if px >= w {
                                    continue;
                                }
                                let off = (py * w + px) * 4;
                                if selected {
                                    rgba[off] = 0xff;
                                    rgba[off + 1] = 0xff;
                                    rgba[off + 2] = 0xff;
                                    rgba[off + 3] = 0xff;
                                } else {
                                    rgba[off] = fg_r;
                                    rgba[off + 1] = fg_g;
                                    rgba[off + 2] = fg_b;
                                    rgba[off + 3] = 0xff;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    rgba
}
