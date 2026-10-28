//! Bounded AS2 primitive-box construction, sharing primitive binding conversions.
//! The browser retains object identity; values and formatting are Rust-owned.
use crate::as2_strings::MAX_AS2_STRING_UNITS;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum Primitive {
    Undefined,
    Null,
    Object,
    Boolean(bool),
    Number(String),
    String(Vec<u16>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    target: String,
    supplied: bool,
    source: Primitive,
}
fn number(value: &str) -> Result<f64, String> {
    match value {
        "NaN" => Ok(f64::NAN),
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        _ => value
            .parse()
            .map_err(|_| "Invalid boxed numeric transport".into()),
    }
}
fn number_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value == f64::INFINITY {
        "Infinity".into()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".into()
    } else if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}
fn packed_number(value: f64) -> String {
    if value == 0.0 && value.is_sign_negative() {
        "-0".into()
    } else {
        number_text(value)
    }
}
fn binding(source: Value, target: &str) -> Result<Value, String> {
    let encoded = crate::as2_binding::execute(
        &json!({"op":"convert","source":source,"target":target}).to_string(),
    )?;
    serde_json::from_str(&encoded).map_err(|e| e.to_string())
}
fn boolean(source: &Primitive) -> Result<bool, String> {
    let source = match source {
        Primitive::Number(value) => {
            let n = number(value)?;
            return Ok(n != 0.0 && !n.is_nan());
        }
        Primitive::String(value) => return Ok(!value.is_empty()),
        Primitive::Boolean(value) => json!({"kind":"Boolean","value":value}),
        Primitive::Undefined => json!({"kind":"Undefined"}),
        Primitive::Null => json!({"kind":"Null"}),
        Primitive::Object => json!({"kind":"Object"}),
    };
    Ok(binding(source, "Boolean")?["value"]
        .as_bool()
        .unwrap_or(false))
}
fn numeric(source: &Primitive) -> Result<f64, String> {
    match source {
        Primitive::Number(value) => number(value),
        Primitive::Undefined => Ok(f64::NAN),
        Primitive::Object => Err("AS2 boxed Number requires a primitive or primitive box; custom coercion is unsupported".into()),
        Primitive::Null => Ok(0.0),
        Primitive::Boolean(value) => Ok(if *value { 1.0 } else { 0.0 }),
        Primitive::String(units) => {
            let Ok(value) = String::from_utf16(units) else {
                return Ok(f64::NAN);
            };
            let trimmed = value.trim();
            if matches!(trimmed, "Infinity" | "+Infinity") {
                return Ok(f64::INFINITY);
            }
            if trimmed == "-Infinity" {
                return Ok(f64::NEG_INFINITY);
            }
            // Reuse the existing finite/hex binding conversion. Failed scalar
            // parses become NaN for a Number box instead of binding failures.
            Ok(binding(json!({"kind":"String","value":value}), "Number")
                .ok()
                .and_then(|v| v["value"].as_f64())
                .unwrap_or(f64::NAN))
        }
    }
}
fn string(source: &Primitive) -> Result<Vec<u16>, String> {
    match source {
        Primitive::String(value) => Ok(value.clone()),
        Primitive::Number(value) => Ok(number_text(number(value)?).encode_utf16().collect()),
        Primitive::Boolean(value) => Ok(binding(
            json!({"kind":"Boolean","value":value}),
            "String",
        )?["value"]
            .as_str()
            .ok_or("Invalid Boolean conversion")?
            .encode_utf16()
            .collect()),
        Primitive::Null => Ok("null".encode_utf16().collect()),
        Primitive::Object => Err("AS2 boxed String requires a primitive or primitive box; custom coercion is unsupported".into()),
        Primitive::Undefined => Ok("undefined".encode_utf16().collect()),
    }
}
pub fn create(input: &str) -> Result<String, String> {
    if input.len() > 16 * 1024 * 1024 {
        return Err("AS2 boxed value exceeds 16 MiB".into());
    }
    let r: Request =
        serde_json::from_str(input).map_err(|e| format!("Invalid AS2 boxing request: {e}"))?;
    if matches!(&r.source,Primitive::String(s) if s.len()>MAX_AS2_STRING_UNITS) {
        return Err("AS2 boxed String exceeds 1000000 UTF-16 units".into());
    }
    let value = match r.target.as_str() {
        "String" => Primitive::String(if r.supplied {
            string(&r.source)?
        } else {
            vec![]
        }),
        "Boolean" => Primitive::Boolean(r.supplied && boolean(&r.source)?),
        "Number" => Primitive::Number(packed_number(if r.supplied {
            numeric(&r.source)?
        } else {
            0.0
        })),
        _ => {
            return Err("Only AS2 Boolean, Number and String primitive boxes are supported".into());
        }
    };
    let text = string(&value)?;
    let length = if let Primitive::String(units) = &value {
        Some(units.len())
    } else {
        None
    };
    Ok(json!({"primitive":value,"text":text,"length":length}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_boxing_family() {
        let box_value = |target: &str, supplied: bool, source: Value| -> Value {
            serde_json::from_str(
                &create(&json!({"target":target,"supplied":supplied,"source":source}).to_string())
                    .unwrap(),
            )
            .unwrap()
        };
        assert_eq!(
            box_value("Number", false, json!({"kind":"Undefined"}))["primitive"]["value"],
            "0"
        );
        assert_eq!(
            box_value("Number", true, json!({"kind":"Number","value":"-0"}))["primitive"]["value"],
            "-0"
        );
        assert_eq!(
            box_value(
                "Number",
                true,
                json!({"kind":"String","value":[48,120,50,97]})
            )["primitive"]["value"],
            "42"
        );
        assert_eq!(
            box_value("Number", true, json!({"kind":"String","value":[110,111]}))["primitive"]["value"],
            "NaN"
        );
        assert_eq!(
            box_value("Boolean", true, json!({"kind":"Number","value":"NaN"}))["primitive"]["value"],
            false
        );
        assert_eq!(
            box_value(
                "Boolean",
                true,
                json!({"kind":"String","value":[102,97,108,115,101]})
            )["primitive"]["value"],
            true
        );
        assert_eq!(
            box_value("String", true, json!({"kind":"String","value":[0xd800,65]}))["primitive"]["value"],
            json!([0xd800, 65])
        );
        assert_eq!(
            box_value("String", true, json!({"kind":"Null"}))["text"],
            json!([110, 117, 108, 108])
        );
        assert_eq!(
            box_value("Boolean", true, json!({"kind":"Object"}))["primitive"]["value"],
            true
        );
        assert!(
            create(r#"{"target":"Object","supplied":false,"source":{"kind":"Undefined"}}"#)
                .is_err()
        );
    }
}
