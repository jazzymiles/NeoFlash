//! Component instance parameter and per-instance animation setting helpers for binary FLA import.

use crate::legacy::timeline::{
    TimelineComponentProperties, TimelineComponentProperty, TimelinePropertyValue,
};
use neoflash_core::{ObjectEvent, ObjectScript, Script, ScriptLanguage};

fn is_valid_as_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
        return false;
    }
    !matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "new"
            | "null"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}

fn quote_as_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn format_color_literal(raw: &str) -> String {
    let trimmed = raw.trim();
    let hex = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix("0x"))
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hex.len() >= 6 && hex[..6].chars().all(|c| c.is_ascii_hexdigit()) {
        format!("0x{}", &hex[..6])
    } else {
        "0x000000".into()
    }
}

fn format_number_literal(raw: &str) -> String {
    let normalized = raw.trim().replace(',', ".");
    if let Ok(v) = normalized.parse::<f64>()
        && v.is_finite()
    {
        if v.fract() == 0.0 && v.abs() < 1e15 {
            format!("{}", v as i64)
        } else {
            format!("{v}")
        }
    } else {
        "0".into()
    }
}

fn format_legacy_property_expr(
    stream_bytes: &[u8],
    prop: &TimelineComponentProperty,
) -> Result<Option<String>, ()> {
    let t = prop.legacy_type.unwrap_or(0);
    let raw = match prop.legacy_value.as_ref() {
        Some(span) => super::read_string(stream_bytes, span).map_err(|_| ())?,
        None => String::new(),
    };
    match t {
        // Editor-only video preview cache
        19 => Ok(None),
        // Collection table cannot be faithfully reconstructed without schema
        13 => Err(()),
        2 | 16 => Ok(Some("[]".into())),
        3 => Ok(Some("{}".into())),
        4 => Ok(Some(quote_as_string(""))),
        6 => Ok(Some(format_number_literal(&raw))),
        7 => Ok(Some(
            if raw.trim() == "true" {
                "true"
            } else {
                "false"
            }
            .into(),
        )),
        9 => Ok(Some(format_color_literal(&raw))),
        1 | 5 | 8 | 10 | 11 | 12 | 14 | 15 | 17 | 18 => Ok(Some(quote_as_string(&raw))),
        _ => Err(()),
    }
}

fn format_typed_property_expr(
    stream_bytes: &[u8],
    val: &TimelinePropertyValue,
) -> Result<Option<String>, ()> {
    if val.value_type == 19 {
        // Authoring-only video preview image/path cache
        return Ok(None);
    }
    if val.table.is_some() || val.value_type == 13 || val.data.is_some_and(|sp| sp.end > sp.start) {
        return Err(());
    }
    match val.value_type {
        // Array
        2 => {
            let mut items = Vec::with_capacity(val.entries.len());
            for entry in &val.entries {
                let s = super::read_string(stream_bytes, &entry.value).map_err(|_| ())?;
                items.push(quote_as_string(&s));
            }
            Ok(Some(format!("[{}]", items.join(", "))))
        }
        // Object (`{ key: "value", ... }`)
        3 => {
            let mut pairs = Vec::with_capacity(val.entries.len());
            for entry in &val.entries {
                let k = match entry.name.as_ref() {
                    Some(n) => super::read_string(stream_bytes, n).map_err(|_| ())?,
                    None => continue,
                };
                let v = super::read_string(stream_bytes, &entry.value).map_err(|_| ())?;
                pairs.push(format!("{}: {}", quote_as_string(&k), quote_as_string(&v)));
            }
            Ok(Some(format!("{{{}}}", pairs.join(", "))))
        }
        // Video Cue Points (`[{ name: ..., time: ... }]` or empty `[]`)
        16 => {
            if val.entries.is_empty() {
                return Ok(Some("[]".into()));
            }
            let mut cues = Vec::with_capacity(val.entries.len());
            for entry in &val.entries {
                let cname = match entry.name.as_ref() {
                    Some(n) => super::read_string(stream_bytes, n).map_err(|_| ())?,
                    None => String::new(),
                };
                let cval = super::read_string(stream_bytes, &entry.value).map_err(|_| ())?;
                cues.push(format!(
                    "{{name: {}, value: {}}}",
                    quote_as_string(&cname),
                    quote_as_string(&cval)
                ));
            }
            Ok(Some(format!("[{}]", cues.join(", "))))
        }
        // List (`selected_index` selects entry)
        4 => {
            let idx = val.selected_index.unwrap_or(0) as usize;
            let chosen = match val.entries.get(idx) {
                Some(entry) => super::read_string(stream_bytes, &entry.value).map_err(|_| ())?,
                None => String::new(),
            };
            Ok(Some(quote_as_string(&chosen)))
        }
        // Scalar types
        1 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 14 | 15 | 17 | 18 => {
            if val.entries.len() > 1 {
                return Err(());
            }
            let raw = match val.entries.first() {
                Some(entry) => super::read_string(stream_bytes, &entry.value).map_err(|_| ())?,
                None => String::new(),
            };
            let expr = match val.value_type {
                6 => format_number_literal(&raw),
                7 => {
                    if raw.trim().eq_ignore_ascii_case("true") || raw.trim() == "1" {
                        "true".into()
                    } else {
                        "false".into()
                    }
                }
                9 => format_color_literal(&raw),
                _ => quote_as_string(&raw),
            };
            Ok(Some(expr))
        }
        _ => Err(()),
    }
}

/// Translate binary FLA component inspectable parameters into an `ObjectEvent::Load`
/// initializer script when representable, returning `(optional_script, had_unrepresented)`.
pub(super) fn translate_component_properties(
    stream_bytes: &[u8],
    props: Option<&TimelineComponentProperties>,
    property_count: Option<u32>,
    doc_language: ScriptLanguage,
) -> (Option<ObjectScript>, bool) {
    let count = property_count.unwrap_or(0);
    let Some(list) = props else {
        return (None, count != 0);
    };
    if list.entries.is_empty() {
        return (None, false);
    }
    let mut statements = Vec::with_capacity(list.entries.len());
    let mut had_lossy = false;

    for entry in &list.entries {
        let var_name = entry
            .variable_name
            .as_ref()
            .and_then(|s| super::read_string(stream_bytes, s).ok())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                entry
                    .name
                    .as_ref()
                    .and_then(|s| super::read_string(stream_bytes, s).ok())
            })
            .unwrap_or_default();
        let ident = var_name.trim();
        if !is_valid_as_identifier(ident) {
            had_lossy = true;
            continue;
        }
        let expr_res = if list.revision == 0 && entry.value.is_none() {
            format_legacy_property_expr(stream_bytes, entry)
        } else if let Some(val) = entry.value.as_ref() {
            format_typed_property_expr(stream_bytes, val)
        } else {
            Err(())
        };
        match expr_res {
            Ok(Some(expr)) => statements.push(format!("this.{ident} = {expr};")),
            Ok(None) => {}
            Err(()) => had_lossy = true,
        }
    }

    let script_obj = (!statements.is_empty()).then(|| {
        let lang = match doc_language {
            ScriptLanguage::ActionScript1 => ScriptLanguage::ActionScript1,
            _ => ScriptLanguage::ActionScript2,
        };
        ObjectScript {
            event: ObjectEvent::Load,
            script: Script {
                language: lang,
                source: statements.join("\n"),
            },
        }
    });
    (script_obj, had_lossy)
}

/// Returns `true` only when `animation_frame_settings` carries a frame/mode override
/// that differs from the instance's base `(loop_mode, first_frame)`.
pub(super) fn has_nonredundant_animation_settings(
    settings: &[[u16; 2]],
    loop_mode: Option<u16>,
    first_frame: Option<u16>,
) -> bool {
    let base_lm = loop_mode.unwrap_or(0);
    let base_ff = first_frame.unwrap_or(0);
    settings
        .iter()
        .any(|&[lm, ff]| lm != base_lm || ff != base_ff)
}
