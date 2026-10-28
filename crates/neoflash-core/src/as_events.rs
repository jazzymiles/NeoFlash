//! Bounded event payloads and formatting, adapted from individual Ruffle event
//! contracts at 89a7049965f3090e706265670df309a5128233ac (MIT).
//! Browser objects remain opaque slots in the host. No dispatcher/VM is imported.
//! See docs/RUNTIME-EVENTS-PROVENANCE.md for scope and deliberate boundaries.
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::OnceLock;

pub const SCHEMA: &str = include_str!("as_events_schema.json");
pub const CONSTANTS: &str = include_str!("as_events_constants.json");
const MAX_TEXT: usize = 1_048_576;

#[derive(Clone, Deserialize)]
struct Field {
    name: String,
    kind: String,
    default: Value,
    readonly: bool,
    alias: Option<String>,
}
#[derive(Clone, Deserialize)]
struct Schema {
    name: String,
    fields: Vec<Field>,
}
fn schemas() -> &'static [Schema] {
    static SCHEMAS: OnceLock<Vec<Schema>> = OnceLock::new();
    SCHEMAS.get_or_init(|| serde_json::from_str(SCHEMA).expect("authored event schema"))
}
fn schema(kind: &str) -> Result<&'static Schema, String> {
    schemas()
        .iter()
        .find(|s| s.name == kind)
        .ok_or_else(|| format!("Unsupported event payload: {kind}"))
}
fn checked_json(text: &str) -> Result<Value, String> {
    if text.len() > MAX_TEXT {
        return Err("Event payload exceeds 1 MiB".into());
    }
    serde_json::from_str(text).map_err(|e| format!("Invalid event payload JSON: {e}"))
}
fn validate(field: &Field, value: Value) -> Result<Value, String> {
    let valid = match field.kind.as_str() {
        "string" => value.is_null() || value.as_str().is_some_and(|s| s.len() <= MAX_TEXT),
        "bool" => value.is_boolean(),
        "int" => value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
        "number" | "coordinate" => {
            value.is_number() || matches!(value.as_str(), Some("NaN" | "Infinity" | "-Infinity"))
        }
        "ref" | "array" | "error" | "display" | "byteArray" | "any" => {
            value.is_null() || value.as_u64() == Some(0)
        }
        _ => false,
    };
    if !valid {
        return Err(format!("Invalid {} event field {}", field.kind, field.name));
    }
    Ok(value)
}

#[derive(Clone)]
pub struct EventPayload {
    schema: &'static Schema,
    values: Map<String, Value>,
}
impl EventPayload {
    pub fn new(kind: &str, input: &str) -> Result<Self, String> {
        let schema = schema(kind)?;
        let input = checked_json(input)?;
        let input = input
            .as_object()
            .ok_or("Event payload requires a field object")?;
        let mut payload = Self {
            schema,
            values: Map::new(),
        };
        for field in &schema.fields {
            if field.alias.is_none() {
                payload
                    .values
                    .insert(field.name.clone(), validate(field, field.default.clone())?);
            }
        }
        for (name, value) in input {
            let field = payload.field(name)?;
            let key = field.alias.as_ref().unwrap_or(&field.name).clone();
            let value = validate(field, value.clone())?;
            payload.values.insert(key, value);
        }
        Ok(payload)
    }
    fn field(&self, name: &str) -> Result<&'static Field, String> {
        self.schema
            .fields
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| format!("Unknown {} field {name}", self.schema.name))
    }
    pub fn get(&self, name: &str) -> Result<Value, String> {
        let field = self.field(name)?;
        Ok(self.values[field.alias.as_ref().unwrap_or(&field.name)].clone())
    }
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        let field = self.field(name)?;
        if field.readonly {
            return Err(format!("{}.{} is read-only", self.schema.name, name));
        }
        let key = field.alias.as_ref().unwrap_or(&field.name).clone();
        let value = validate(field, checked_json(value)?)?;
        self.values.insert(key, value);
        Ok(())
    }
}

/// Coercion/property reads are performed exactly once by the host, before this
/// bounded formatter. String quotes intentionally match Event.formatToString,
/// rather than JSON escaping: this is a display string, not serialization.
pub fn format_event(class_name: &str, entries_json: &str) -> Result<String, String> {
    if class_name.len() > 256 {
        return Err("Event format class name exceeds 256 bytes".into());
    }
    let entries = checked_json(entries_json)?;
    let entries = entries
        .as_array()
        .ok_or("Event formatting requires entries")?;
    if entries.len() > 64 {
        return Err("Event formatting exceeds 64 fields".into());
    }
    let mut out = format!("[{class_name}");
    for entry in entries {
        let entry = entry
            .as_array()
            .filter(|v| v.len() == 3)
            .ok_or("Invalid event format entry")?;
        let name = entry[0]
            .as_str()
            .ok_or("Event format name requires a string")?;
        let value = entry[1]
            .as_str()
            .ok_or("Event format value requires a string")?;
        let quote = entry[2]
            .as_bool()
            .ok_or("Event format quote flag requires a Boolean")?;
        if out.len() + name.len() + value.len() + 5 > MAX_TEXT {
            return Err("Event formatted text exceeds 1 MiB".into());
        }
        out.push(' ');
        out.push_str(name);
        out.push('=');
        if quote {
            out.push('"');
        }
        out.push_str(value);
        if quote {
            out.push('"');
        }
    }
    out.push(']');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_payload_family() {
        for schema in schemas() {
            let mut event = EventPayload::new(&schema.name, "{}").unwrap();
            for field in &schema.fields {
                assert_eq!(event.get(&field.name).unwrap(), field.default);
                let old = event.get(&field.name).unwrap();
                let result = event.set(&field.name, &old.to_string());
                assert_eq!(result.is_err(), field.readonly);
            }
        }
        let mut data = EventPayload::new("DataEvent", r#"{"data":"one"}"#).unwrap();
        data.set("text", r#""two""#).unwrap();
        assert_eq!(data.get("data").unwrap(), "two");
        let copied = data.clone();
        data.set("data", r#""three""#).unwrap();
        assert_eq!(copied.get("text").unwrap(), "two");
        let mut error = EventPayload::new("ErrorEvent", r#"{"text":"Oops","errorID":42}"#).unwrap();
        assert!(error.set("errorID", "7").is_err());
        assert_eq!(error.get("errorID").unwrap(), 42);
        let mut sample =
            EventPayload::new("SampleDataEvent", r#"{"data":0,"position":2048}"#).unwrap();
        assert!(
            sample
                .set("data", r#"{"unexpected":"serialized object"}"#)
                .is_err()
        );
        assert_eq!(sample.get("data").unwrap(), 0);
        assert_eq!(
            format_event(
                "DataEvent",
                r#"[["type","data",true],["bubbles","false",false],["data","sample",true]]"#
            )
            .unwrap(),
            "[DataEvent type=\"data\" bubbles=false data=\"sample\"]"
        );
        assert!(EventPayload::new("MissingEvent", "{}").is_err());
    }
}
