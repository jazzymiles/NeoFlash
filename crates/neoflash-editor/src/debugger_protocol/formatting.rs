use super::*;

/// Parse a scalar CDP `CallArgument` (`{"value": ...}` or `{"unserializableValue": ...}`)
/// supporting JSON scalars (`Number`, `Boolean`, `String`, `null`), single-quoted strings,
/// and IEEE-754 special numbers (`NaN`, `Infinity`, `-Infinity`, `-0`).
pub(crate) fn parse_debug_call_argument(input: &str) -> Result<Value, String> {
    let trimmed = input.trim();
    match trimmed {
        "NaN" => return Ok(json!({ "unserializableValue": "NaN" })),
        "Infinity" | "+Infinity" => return Ok(json!({ "unserializableValue": "Infinity" })),
        "-Infinity" => return Ok(json!({ "unserializableValue": "-Infinity" })),
        "-0" | "-0.0" => return Ok(json!({ "unserializableValue": "-0" })),
        _ => {}
    }
    parse_debug_scalar_value(trimmed).map(|value| json!({ "value": value }))
}

/// Parse a scalar JSON value (`Number`, `Boolean`, `String`, or `null`) for paused variable/property editing.
pub(crate) fn parse_debug_scalar_value(input: &str) -> Result<Value, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter a scalar JSON value (number, boolean, string, or null).".into());
    }
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut unescaped = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('\\') => unescaped.push('\\'),
                    Some('\'') => unescaped.push('\''),
                    Some('"') => unescaped.push('"'),
                    Some('n') => unescaped.push('\n'),
                    Some('r') => unescaped.push('\r'),
                    Some('t') => unescaped.push('\t'),
                    Some(other) => {
                        unescaped.push('\\');
                        unescaped.push(other);
                    }
                    None => return Err("Unterminated string escape.".into()),
                }
            } else {
                unescaped.push(ch);
            }
        }
        return Ok(Value::String(bounded(&unescaped, MAX_TEXT)));
    }
    let parsed: Value = serde_json::from_str(trimmed).map_err(|_| {
        "Value must be a scalar JSON literal (e.g. 42, -3.5, true, false, \"text\", or null)."
            .to_owned()
    })?;
    match parsed {
        Value::Null | Value::Bool(_) => Ok(parsed),
        Value::Number(ref num) if num.as_f64().is_some_and(f64::is_finite) => Ok(parsed),
        Value::String(text) => Ok(Value::String(bounded(&text, MAX_TEXT))),
        _ => Err(
            "Only scalar JSON values (Number, Boolean, String, or null) can be assigned.".into(),
        ),
    }
}

pub(super) fn remote_inspectable_object_id(remote: &Value, accessor: bool) -> Option<String> {
    if accessor {
        return None;
    }
    let subtype = remote.get("subtype").and_then(Value::as_str);
    if matches!(subtype, Some("null" | "proxy")) {
        return None;
    }
    id_string(remote.get("objectId"))
}

pub(super) fn extract_preview_scalar_fields(
    remote: &Value,
) -> Option<Vec<(String, String, String)>> {
    if remote.get("subtype").and_then(Value::as_str) == Some("proxy") {
        return None;
    }
    let props = remote
        .get("preview")
        .and_then(|p| p.get("properties"))
        .and_then(Value::as_array)?;
    let mut fields = Vec::new();
    for prop in props.iter().take(64) {
        let Some(name) = prop.get("name").and_then(Value::as_str) else {
            continue;
        };
        let kind = prop.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "accessor" {
            continue;
        }
        let subtype = prop.get("subtype").and_then(Value::as_str);
        if subtype == Some("null") {
            fields.push((bounded(name, 256), "null".into(), "Null".into()));
            continue;
        }
        if kind == "undefined" {
            fields.push((bounded(name, 256), "undefined".into(), "Undefined".into()));
            continue;
        }
        if let Some(val) = prop.get("value").and_then(Value::as_str)
            && matches!(kind, "number" | "boolean" | "string")
        {
            fields.push((bounded(name, 256), bounded(val, MAX_TEXT), kind.to_owned()));
        }
    }
    Some(fields)
}

pub(super) fn decode_cdp_property(property: &Value) -> Option<DebugObjectProperty> {
    let name = property.get("name").and_then(Value::as_str)?;
    let was_thrown = property.get("wasThrown").and_then(Value::as_bool) == Some(true);
    let accessor = property.get("get").is_some() || property.get("set").is_some();
    let remote = property.get("value").unwrap_or(&Value::Null);
    let object_id = if was_thrown {
        None
    } else {
        remote_inspectable_object_id(remote, accessor)
    };
    let prop_writable = property
        .get("writable")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let value_truncated = remote
        .get("value")
        .and_then(Value::as_str)
        .is_some_and(|s| s.chars().count() > MAX_INLINE_EDIT_STRING_CHARS);
    let value = if was_thrown {
        "<unavailable>".into()
    } else if accessor {
        "<accessor; not invoked>".into()
    } else {
        remote_summary(remote)
    };
    let kind = bounded(
        remote
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("accessor"),
        80,
    );
    let subtype = remote
        .get("subtype")
        .and_then(Value::as_str)
        .map(|s| bounded(s, 80));
    let writable =
        !was_thrown && !accessor && prop_writable && object_id.is_none() && !value_truncated;
    Some(DebugObjectProperty {
        name: bounded(name, 65_536),
        value,
        kind,
        subtype,
        object_id,
        accessor,
        writable,
    })
}

pub(super) fn format_watch_exception(details: &Value, remote: Option<&Value>) -> String {
    if let Some(desc) = details
        .get("exception")
        .and_then(|ex| ex.get("description"))
        .and_then(Value::as_str)
        .or_else(|| {
            remote
                .and_then(|r| r.get("description"))
                .and_then(Value::as_str)
        })
        .and_then(|d| d.lines().next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("<{}>", bounded(desc, 240));
    }
    if let Some(ex) = details.get("exception")
        && ex.get("value").is_some()
    {
        return format!("<Uncaught {}>", bounded(&remote_summary(ex), 220));
    }
    if let Some(text) = details
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("<{}>", bounded(text, 240));
    }
    "<unavailable>".into()
}

// Retain the browser's actionable lifecycle explanation without dumping authored data.
pub(super) fn lifecycle_message(base: &str, params: &Value) -> String {
    let mut message = base.to_owned();
    for field in ["reason", "status", "errorCode"] {
        let value = match params.get(field) {
            Some(Value::String(value)) => bounded(value, MAX_TEXT),
            Some(Value::Number(value)) => value.to_string(),
            _ => continue,
        };
        message.push_str(&format!("; {field}: {value}"));
    }
    if message.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    message
}

pub(super) fn number(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}
pub(super) fn id_string(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?;
    (!value.is_empty() && value.len() <= 4_096).then(|| value.to_owned())
}
pub(super) fn bounded(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}
pub(super) fn console_arg_text(value: &Value) -> String {
    if let Some(Value::String(text)) = value.get("value") {
        return bounded(text, MAX_TEXT);
    }
    remote_summary_inner(value, true)
}
pub(super) fn remote_summary(value: &Value) -> String {
    remote_summary_inner(value, false)
}
pub(super) fn format_cdp_key(name: &str) -> String {
    let valid_ident = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if valid_ident {
        bounded(name, 256)
    } else {
        format!("{:?}", bounded(name, 256))
    }
}
pub(super) fn format_cdp_property_preview(
    prop: &Value,
    depth: usize,
    multiline: bool,
) -> (String, bool) {
    if let Some(sub) = prop.get("valuePreview")
        && let Some(nested) = format_cdp_object_preview(sub, depth + 1, multiline)
    {
        return (nested, true);
    }
    let kind = prop.get("type").and_then(Value::as_str).unwrap_or("");
    let subtype = prop.get("subtype").and_then(Value::as_str);
    if kind == "accessor" {
        return ("[Getter]".into(), false);
    }
    if subtype == Some("null") {
        return ("null".into(), false);
    }
    if let Some(val) = prop.get("value").and_then(Value::as_str) {
        if kind == "string" {
            let s = bounded(val, 256);
            return (
                serde_json::to_string(&s).unwrap_or_else(|_| format!("{s:?}")),
                false,
            );
        }
        return (bounded(val, 256), false);
    }
    if kind == "undefined" {
        return ("undefined".into(), false);
    }
    let fallback = subtype.unwrap_or(if kind.is_empty() { "object" } else { kind });
    (format!("<{}>", bounded(fallback, 64)), false)
}
pub(super) fn format_cdp_object_preview(
    preview: &Value,
    depth: usize,
    multiline: bool,
) -> Option<String> {
    if depth > 6 || !preview.is_object() {
        return None;
    }
    let subtype = preview.get("subtype").and_then(Value::as_str);
    if subtype == Some("proxy") {
        return None;
    }
    let overflow = preview
        .get("overflow")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let desc = preview
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");

    if matches!(subtype, Some("array" | "typedarray")) {
        let props = preview.get("properties").and_then(Value::as_array)?;
        let mut indexed: BTreeMap<usize, (String, bool)> = BTreeMap::new();
        let mut extra: Vec<(String, bool)> = Vec::new();
        for prop in props.iter().take(128) {
            let name = prop.get("name").and_then(Value::as_str).unwrap_or("");
            let (val_s, is_nested) = format_cdp_property_preview(prop, depth, multiline);
            if let Ok(idx) = name.parse::<usize>() {
                indexed.insert(idx, (val_s, is_nested));
            } else if !name.is_empty() {
                extra.push((format!("{}: {}", format_cdp_key(name), val_s), is_nested));
            }
        }
        let declared_len = desc
            .strip_prefix("Array(")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.parse::<usize>().ok());
        let max_idx = indexed
            .keys()
            .next_back()
            .map(|k| k.saturating_add(1))
            .unwrap_or(0);
        let dense_max_idx = indexed
            .range(..128)
            .next_back()
            .map(|(k, _)| k + 1)
            .unwrap_or(0);
        let has_sparse_high_idx = indexed.range(128..).next().is_some();
        let len = declared_len.unwrap_or(max_idx).min(max_idx.max(128));
        let mut items: Vec<String> = Vec::new();
        let mut has_nested = false;
        let render_len = if overflow || has_sparse_high_idx {
            dense_max_idx.min(128)
        } else {
            len.min(128)
        };
        for idx in 0..render_len {
            if let Some((val_s, nested)) = indexed.remove(&idx) {
                has_nested |= nested;
                items.push(val_s);
            } else {
                items.push("<empty>".into());
            }
        }
        for (idx, (val_s, nested)) in indexed {
            has_nested |= nested;
            items.push(format!("{idx}: {val_s}"));
        }
        for (pair, nested) in extra {
            has_nested |= nested;
            items.push(pair);
        }
        if overflow || declared_len.is_some_and(|dl| dl > render_len && render_len == 128) {
            items.push("…".into());
        }
        let prefix = if subtype == Some("typedarray") && !desc.is_empty() {
            format!("{} ", bounded(desc, 64))
        } else {
            String::new()
        };
        if items.is_empty() {
            return Some(format!("{prefix}[]"));
        }
        if multiline && (has_nested || items.iter().any(|s| s.contains('\n'))) {
            let child_indent = "  ".repeat(depth + 1);
            let close_indent = "  ".repeat(depth);
            let body = items
                .into_iter()
                .map(|s| format!("{child_indent}{s}"))
                .collect::<Vec<_>>()
                .join(",\n");
            return Some(bounded(
                &format!("{prefix}[\n{body}\n{close_indent}]"),
                MAX_TEXT,
            ));
        }
        return Some(bounded(
            &format!("{prefix}[{}]", items.join(", ")),
            MAX_TEXT,
        ));
    }

    if matches!(subtype, Some("map" | "set")) {
        let entries = preview.get("entries").and_then(Value::as_array)?;
        let mut items = Vec::new();
        for entry in entries.iter().take(64) {
            let val_s = entry
                .get("value")
                .and_then(|v| format_cdp_object_preview(v, depth + 1, false))
                .or_else(|| {
                    entry
                        .get("value")
                        .map(|v| format_cdp_property_preview(v, depth, false).0)
                })
                .unwrap_or_else(|| "undefined".into());
            if subtype == Some("map") {
                let key_s = entry
                    .get("key")
                    .and_then(|k| format_cdp_object_preview(k, depth + 1, false))
                    .or_else(|| {
                        entry
                            .get("key")
                            .map(|k| format_cdp_property_preview(k, depth, false).0)
                    })
                    .unwrap_or_else(|| "undefined".into());
                items.push(format!("{key_s} => {val_s}"));
            } else {
                items.push(val_s);
            }
        }
        if overflow {
            items.push("…".into());
        }
        let label = if !desc.is_empty() {
            bounded(desc, 64)
        } else if subtype == Some("map") {
            "Map".into()
        } else {
            "Set".into()
        };
        return Some(if items.is_empty() {
            format!("{label} {{}}")
        } else {
            bounded(&format!("{label} {{ {} }}", items.join(", ")), MAX_TEXT)
        });
    }

    if matches!(subtype, Some("date" | "regexp" | "error")) && !desc.is_empty() {
        return Some(bounded(desc, MAX_TEXT));
    }

    let props = preview.get("properties").and_then(Value::as_array)?;
    let mut items = Vec::new();
    let mut has_nested = false;
    for prop in props.iter().take(64) {
        let Some(name) = prop.get("name").and_then(Value::as_str) else {
            continue;
        };
        let (val_s, nested) = format_cdp_property_preview(prop, depth, multiline);
        has_nested |= nested;
        items.push(format!("{}: {}", format_cdp_key(name), val_s));
    }
    if overflow {
        items.push("…".into());
    }
    let prefix = if !desc.is_empty() && desc != "Object" && !desc.starts_with('[') {
        format!("{} ", bounded(desc, 64))
    } else {
        String::new()
    };
    if items.is_empty() {
        return Some(format!("{prefix}{{}}"));
    }
    if multiline && (has_nested || items.iter().any(|s| s.contains('\n'))) {
        let child_indent = "  ".repeat(depth + 1);
        let close_indent = "  ".repeat(depth);
        let body = items
            .into_iter()
            .map(|s| format!("{child_indent}{s}"))
            .collect::<Vec<_>>()
            .join(",\n");
        return Some(bounded(
            &format!("{prefix}{{\n{body}\n{close_indent}}}"),
            MAX_TEXT,
        ));
    }
    Some(bounded(
        &format!("{prefix}{{ {} }}", items.join(", ")),
        MAX_TEXT,
    ))
}
pub(super) fn remote_summary_inner(value: &Value, multiline: bool) -> String {
    if let Some(value) = value.get("value") {
        return match value {
            Value::String(value) => {
                let s = bounded(value, MAX_TEXT);
                serde_json::to_string(&s).unwrap_or_else(|_| format!("{s:?}"))
            }
            Value::Bool(_) | Value::Number(_) | Value::Null => value.to_string(),
            _ => "<object>".into(),
        };
    }
    if let Some(value) = value.get("unserializableValue").and_then(Value::as_str) {
        return bounded(value, MAX_TEXT);
    }
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    if kind == "undefined" {
        return "undefined".into();
    }
    let subtype = value.get("subtype").and_then(Value::as_str);
    if subtype != Some("proxy")
        && let Some(preview) = value.get("preview")
        && let Some(formatted) = format_cdp_object_preview(preview, 0, multiline)
    {
        return formatted;
    }
    format!(
        "<{}>",
        bounded(
            value
                .get("className")
                .or_else(|| value.get("subtype"))
                .and_then(Value::as_str)
                .unwrap_or(kind),
            128
        )
    )
}

/// Verify the full known generated body inside Chrome's Function wrapper. A matching sourceURL
/// alone is insufficient: authored strings and unrelated scripts must not gain forged locations.
pub(super) fn wrapper_offset(source: &DebugSource, script: &str) -> Option<(usize, usize)> {
    if source.code.is_empty() || script.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let index = script.find(&source.code)?;
    if script[index + source.code.len()..].contains(&source.code) {
        return None;
    }
    let prefix = &script[..index];
    let suffix = &script[index + source.code.len()..];
    let function = prefix
        .trim_start_matches('(')
        .strip_prefix("function anonymous(")?;
    let (parameters, body_prefix) = function.split_once(") {\n")?;
    if !parameters
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ',' || c.is_ascii_whitespace())
    {
        return None;
    }
    let (wanted_prefix, wanted_suffix) = if source.source_url == "neoflash-project.js" {
        ("", format!("\n//# sourceURL={}\n}}", source.source_url))
    } else {
        (
            "\"use strict\";\nreturn (function () {\n",
            format!("\n}}).call(this);\n//# sourceURL={}\n}}", source.source_url),
        )
    };
    if body_prefix != wanted_prefix || suffix.trim_end_matches(')').trim_end() != wanted_suffix {
        return None;
    }
    Some((
        prefix.bytes().filter(|c| *c == b'\n').count(),
        prefix.rsplit('\n').next()?.encode_utf16().count(),
    ))
}
