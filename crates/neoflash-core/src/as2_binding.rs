//! Primitive field conversion, safe endpoint paths and binding-cycle state.
//! Public AS2 binding contracts and partial limits: docs/AS2-DATA-BATCH.md.
use serde_json::{Value, json};

pub struct ScriptAS2Binding {
    plan: Value,
    two_way: bool,
    entered: bool,
}
impl ScriptAS2Binding {
    pub fn new(config: &str) -> Result<Self, String> {
        if config.len() > 32_768 {
            return Err("Binding configuration exceeds 32768 bytes".into());
        }
        let config: Value = serde_json::from_str(config)
            .map_err(|e| format!("Invalid binding configuration: {e}"))?;
        let source = &config["source"];
        let destination = &config["destination"];
        let constant = source["constant"].as_bool().unwrap_or(false);
        if destination["constant"].as_bool().unwrap_or(false) {
            return Err("A binding destination cannot be a constant".into());
        }
        let two_way = config["twoWay"].as_bool().unwrap_or(false);
        if constant && two_way {
            return Err("A constant binding cannot be two-way".into());
        }
        let source_path = if constant {
            Vec::new()
        } else {
            endpoint_path(source)?
        };
        let source_events = if constant {
            Vec::new()
        } else {
            event_names(&source["event"])?
        };
        let plan = json!({"sourcePath":source_path,"destinationPath":endpoint_path(destination)?,
            "sourceEvents":source_events,"destinationEvents":if two_way{event_names(&destination["event"])?}else{Vec::new()},"twoWay":two_way});
        Ok(Self {
            plan,
            two_way,
            entered: false,
        })
    }
    pub fn plan(&self) -> String {
        self.plan.to_string()
    }
    /// Zero means a synchronous cycle already contains this binding. Direction
    /// is decided here so two-way propagation does not grow a second host policy.
    pub fn enter(&mut self, reverse: bool) -> u32 {
        if self.entered {
            return 0;
        }
        self.entered = true;
        if reverse && self.two_way { 2 } else { 1 }
    }
    pub fn leave(&mut self) {
        self.entered = false;
    }
}

fn path_segment(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 256
        || matches!(value, "__proto__" | "prototype" | "constructor")
    {
        return Err(
            "Binding path segments require 1–256 characters and cannot change prototypes".into(),
        );
    }
    Ok(value.into())
}
fn endpoint_path(input: &Value) -> Result<Vec<String>, String> {
    let property = input["property"]
        .as_str()
        .ok_or("An endpoint requires a string property")?;
    let mut result = vec![path_segment(property)?];
    match &input["location"] {
        Value::Null => {}
        Value::String(value) if value.is_empty() => {}
        Value::String(value) => {
            if value.contains(['/', '[', ']']) {
                return Err("Binding location supports dotted field paths or string arrays; XPath and indexed expressions are unavailable".into());
            }
            for part in value.split('.') {
                result.push(path_segment(part)?);
            }
        }
        Value::Array(values) => {
            for value in values {
                result.push(path_segment(
                    value
                        .as_str()
                        .ok_or("Binding location array requires strings")?,
                )?);
            }
        }
        _ => {
            return Err(
                "Binding location supports dotted field paths or arrays of field names".into(),
            );
        }
    }
    if result.len() > 32 {
        return Err("A binding path supports at most 32 segments".into());
    }
    Ok(result)
}
fn event_names(input: &Value) -> Result<Vec<String>, String> {
    let values = match input {
        Value::Null => Vec::new(),
        Value::String(value) => vec![Value::String(value.clone())],
        Value::Array(values) => values.clone(),
        _ => return Err("Endpoint events require a name or an array of names".into()),
    };
    if values.len() > 32 {
        return Err("A binding endpoint supports at most 32 events".into());
    }
    values
        .iter()
        .map(|v| path_segment(v.as_str().ok_or("Endpoint events require string names")?))
        .collect()
}
pub fn execute(input: &str) -> Result<String, String> {
    if input.len() > 1_048_576 {
        return Err("Binding value exceeds 1 MiB".into());
    }
    let request: Value =
        serde_json::from_str(input).map_err(|e| format!("Invalid binding value: {e}"))?;
    match request["op"].as_str() {
        Some("path") => Ok(json!(endpoint_path(&request)?).to_string()),
        Some("convert") => {
            convert(&request["source"], request["target"].as_str()).map(|v| v.to_string())
        }
        _ => Err("Unknown AS2 binding request".into()),
    }
}
fn convert(source: &Value, target: Option<&str>) -> Result<Value, String> {
    let kind = source["kind"]
        .as_str()
        .ok_or("Binding source has no type")?;
    let target = target.unwrap_or(kind);
    if !matches!(
        kind,
        "String" | "Number" | "Boolean" | "Null" | "Undefined" | "Object" | "Array" | "Function"
    ) {
        return Err("Unsupported binding source type".into());
    }
    if target == kind || target == "*" {
        return Ok(source.clone());
    }
    if target == "Object" && matches!(kind, "Object" | "Array" | "Function" | "Null") {
        return Ok(source.clone());
    }
    match target {
        "String" => {
            let value = match kind {
                "String" => source["value"]
                    .as_str()
                    .ok_or("Invalid String binding value")?
                    .to_owned(),
                "Number" => {
                    let v = source["value"]
                        .as_f64()
                        .ok_or("Binding Number must be finite")?;
                    if v == 0.0 { "0".into() } else { v.to_string() }
                }
                "Boolean" => source["value"]
                    .as_bool()
                    .ok_or("Invalid Boolean binding value")?
                    .to_string(),
                "Null" => "null".into(),
                "Undefined" => "undefined".into(),
                _ => return Err(
                    "Object-to-string binding coercion is unavailable; select a primitive field"
                        .into(),
                ),
            };
            Ok(json!({"kind":"String","value":value}))
        }
        "Number" => {
            let value = match kind {
                "Number" => source["value"]
                    .as_f64()
                    .ok_or("Binding Number must be finite")?,
                "Boolean" => {
                    if source["value"]
                        .as_bool()
                        .ok_or("Invalid Boolean binding value")?
                    {
                        1.0
                    } else {
                        0.0
                    }
                }
                "Null" => 0.0,
                "String" => {
                    let s = source["value"]
                        .as_str()
                        .ok_or("Invalid String binding value")?
                        .trim();
                    if s.is_empty() {
                        0.0
                    } else if let Some(s) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                        u64::from_str_radix(s, 16)
                            .map(|n| n as f64)
                            .map_err(|_| "The field cannot convert this string to Number")?
                    } else {
                        s.parse::<f64>()
                            .map_err(|_| "The field cannot convert this string to Number")?
                    }
                }
                _ => return Err("The field cannot convert this value to Number".into()),
            };
            if !value.is_finite() {
                return Err("Binding Number conversion requires a finite result".into());
            }
            Ok(json!({"kind":"Number","value":value}))
        }
        "Boolean" => {
            let value = match kind {
                "Boolean" => source["value"]
                    .as_bool()
                    .ok_or("Invalid Boolean binding value")?,
                "Number" => {
                    source["value"]
                        .as_f64()
                        .ok_or("Binding Number must be finite")?
                        != 0.0
                }
                "String" => !source["value"]
                    .as_str()
                    .ok_or("Invalid String binding value")?
                    .is_empty(),
                "Null" | "Undefined" => false,
                _ => true,
            };
            Ok(json!({"kind":"Boolean","value":value}))
        }
        _ => Err(format!("The field cannot supply binding type '{target}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_binding_family() {
        let mut binding=ScriptAS2Binding::new(r#"{"source":{"property":"data","location":"person.name","event":["change","commit"]},"destination":{"property":"value","event":"change"},"twoWay":true}"#).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&binding.plan()).unwrap()["sourcePath"],
            json!(["data", "person", "name"])
        );
        assert_eq!(binding.enter(false), 1);
        assert_eq!(binding.enter(true), 0);
        binding.leave();
        assert_eq!(binding.enter(true), 2);
        assert_eq!(
            convert(&json!({"kind":"String","value":" 42 "}), Some("Number")).unwrap()["value"],
            42.0
        );
        assert_eq!(
            convert(&json!({"kind":"Boolean","value":true}), Some("String")).unwrap()["value"],
            "true"
        );
        assert_eq!(
            convert(&json!({"kind":"Null"}), Some("Boolean")).unwrap()["value"],
            false
        );
        assert_eq!(
            convert(&json!({"kind":"Array","ref":8}), Some("Object")).unwrap()["ref"],
            8
        );
        assert!(convert(&json!({"kind":"String","value":"wrong"}), Some("Number")).is_err());
        assert!(
            execute(r#"{"op":"path","property":"data","location":"__proto__.secret"}"#).is_err()
        );
        assert!(
            ScriptAS2Binding::new(
                r#"{"source":{"constant":true},"destination":{"property":"x"},"twoWay":true}"#
            )
            .is_err()
        );
    }
}
