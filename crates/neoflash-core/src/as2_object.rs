//! AS2 property watch/resolution ownership, independent of browser object storage.
//! Individual AVM1 contracts adapted from Ruffle (MIT), pinned in
//! docs/AS2-OBJECT-BATCH.md. No VM or garbage collector is embedded.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const LIMIT: usize = 4096;
const NAME_LIMIT: usize = 4096;
#[derive(Debug, thiserror::Error)]
#[error("AS2 Object: {0}")]
pub struct As2ObjectError(pub String);
fn valid_name(name: &str) -> Result<(), As2ObjectError> {
    if name.encode_utf16().count() > NAME_LIMIT {
        return Err(As2ObjectError(
            "property name exceeds 4096 UTF-16 units".into(),
        ));
    }
    Ok(())
}
#[derive(Default)]
struct TextVariableBinding {
    path: String,
    owner: u32,
    last: Option<String>,
    initialized: bool,
    entered: bool,
}
#[derive(Default)]
pub struct As2ObjectState {
    next: u32,
    text_bindings: BTreeMap<u32, TextVariableBinding>,
    watches: BTreeMap<(u32, String), u32>,
    writing: BTreeSet<(u32, String)>,
    resolving: BTreeSet<(u32, String)>,
    accessors: BTreeSet<(u32, String)>,
}
impl As2ObjectState {
    pub fn new_id(&mut self) -> Result<u32, As2ObjectError> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| As2ObjectError("publication identity limit reached".into()))?;
        Ok(self.next)
    }
    pub fn watch(&mut self, object: u32, name: &str) -> Result<u32, As2ObjectError> {
        valid_name(name)?;
        let key = (object, name.to_owned());
        if self.accessors.contains(&key) {
            return Ok(0);
        }
        if !self.watches.contains_key(&key) && self.watches.len() >= LIMIT {
            return Err(As2ObjectError(
                "publication exceeds 4096 watchpoints".into(),
            ));
        }
        let token = self.new_id()?;
        self.watches.insert(key, token);
        Ok(token)
    }
    pub fn watched(&self, object: u32, name: &str) -> u32 {
        self.watches
            .get(&(object, name.to_owned()))
            .copied()
            .unwrap_or(0)
    }
    pub fn unwatch(&mut self, object: u32, name: &str) -> u32 {
        self.watches.remove(&(object, name.to_owned())).unwrap_or(0)
    }
    pub fn begin_write(&mut self, object: u32, name: &str) -> u32 {
        let key = (object, name.to_owned());
        let token = self.watches.get(&key).copied().unwrap_or(0);
        if token == 0 || !self.writing.insert(key) {
            0
        } else {
            token
        }
    }
    pub fn end_write(&mut self, object: u32, name: &str) {
        self.writing.remove(&(object, name.to_owned()));
    }
    pub fn begin_resolve(&mut self, object: u32, name: &str) -> Result<bool, As2ObjectError> {
        valid_name(name)?;
        if self.resolving.len() >= 128 {
            return Err(As2ObjectError(
                "__resolve nesting exceeds 128 properties".into(),
            ));
        }
        Ok(self.resolving.insert((object, name.to_owned())))
    }
    pub fn end_resolve(&mut self, object: u32, name: &str) {
        self.resolving.remove(&(object, name.to_owned()));
    }
    pub fn add_accessor(&mut self, object: u32, name: &str) -> Result<bool, As2ObjectError> {
        valid_name(name)?;
        let key = (object, name.to_owned());
        if name.is_empty() || self.watches.contains_key(&key) {
            return Ok(false);
        }
        if !self.accessors.contains(&key) && self.accessors.len() >= LIMIT {
            return Err(As2ObjectError(
                "publication exceeds 4096 dynamic accessors".into(),
            ));
        }
        self.accessors.insert(key);
        Ok(true)
    }
    pub fn deleted(&mut self, object: u32, name: &str) {
        // Deletion deliberately leaves the watchpoint alive, matching AVM1.
        self.accessors.remove(&(object, name.to_owned()));
    }
    /// TextField.variable shares this object's identities, path parser and the
    /// existing primitive binding conversion. Browser references never enter Rust.
    pub fn text_binding(&mut self, input: &str) -> Result<Value, As2ObjectError> {
        if input.len() > 2 * 1024 * 1024 {
            return Err(As2ObjectError("text-variable command exceeds 2 MiB".into()));
        }
        let value: Value = serde_json::from_str(input)
            .map_err(|e| As2ObjectError(format!("invalid text binding: {e}")))?;
        let id = value["field"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| As2ObjectError("invalid text field identity".into()))?;
        match value["op"].as_str().unwrap_or("") {
            "get" => Ok(self
                .text_bindings
                .get(&id)
                .map(|v| json!(v.path))
                .unwrap_or(Value::Null)),
            "bind" => {
                if value["path"].is_null() || value["path"] == "" {
                    self.text_bindings.remove(&id);
                    return Ok(Value::Null);
                }
                let path = value["path"]
                    .as_str()
                    .ok_or_else(|| As2ObjectError("variable requires a string or null".into()))?;
                let parts = reference_path(path)?;
                if parts.is_empty()
                    || parts
                        .iter()
                        .any(|p| matches!(p.as_str(), "__proto__" | "prototype" | "constructor"))
                {
                    return Err(As2ObjectError(
                        "variable requires an ordinary nonempty variable path".into(),
                    ));
                }
                if !self.text_bindings.contains_key(&id) && self.text_bindings.len() >= 256 {
                    return Err(As2ObjectError(
                        "publication exceeds 256 text-variable bindings".into(),
                    ));
                }
                self.text_bindings.insert(
                    id,
                    TextVariableBinding {
                        path: path.into(),
                        ..Default::default()
                    },
                );
                Ok(json!(parts))
            }
            "finish" => {
                if let Some(binding) = self.text_bindings.get_mut(&id) {
                    binding.entered = false;
                    if value["accepted"] != true {
                        binding.last = None;
                        binding.initialized = false;
                    }
                }
                Ok(Value::Null)
            }
            "plan" => {
                let Some(binding) = self.text_bindings.get_mut(&id) else {
                    return Ok(Value::Null);
                };
                if binding.entered {
                    return Ok(Value::Null);
                }
                let owner = value["owner"]
                    .as_u64()
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0);
                if owner == 0 {
                    binding.owner = 0;
                    binding.last = None;
                    binding.initialized = false;
                    return Ok(Value::Null);
                }
                if binding.owner != owner {
                    binding.owner = owner;
                    binding.last = None;
                    binding.initialized = false;
                }
                let text = value["text"]
                    .as_str()
                    .ok_or_else(|| As2ObjectError("text binding needs field text".into()))?;
                if text.len() > 1024 * 1024 {
                    return Err(As2ObjectError("bound text exceeds 1 MiB".into()));
                }
                let exists = value["exists"] == true;
                let from_field = value["fromField"] == true;
                let action = if from_field || (!binding.initialized && !exists && !text.is_empty())
                {
                    binding.last = Some(text.into());
                    json!({"kind":"variable","value":text})
                } else if exists {
                    let converted = crate::as2_binding::execute(
                        &json!({"op":"convert","source":value["source"],"target":"String"})
                            .to_string(),
                    )
                    .map_err(As2ObjectError)?;
                    let converted: Value = serde_json::from_str(&converted)
                        .map_err(|e| As2ObjectError(e.to_string()))?;
                    let string = converted["value"].as_str().ok_or_else(|| {
                        As2ObjectError("binding conversion did not return String".into())
                    })?;
                    if string.len() > 1024 * 1024 {
                        return Err(As2ObjectError("bound variable string exceeds 1 MiB".into()));
                    }
                    if binding.last.as_deref() == Some(string) {
                        Value::Null
                    } else {
                        binding.last = Some(string.into());
                        json!({"kind":"text","value":string})
                    }
                } else {
                    binding.last = None;
                    Value::Null
                };
                binding.initialized = true;
                if !action.is_null() {
                    binding.entered = true;
                }
                Ok(action)
            }
            _ => Err(As2ObjectError("unknown text-variable command".into())),
        }
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Parse the legacy variable-path grammar; never parse or execute source code.
pub fn reference_path(source: &str) -> Result<Vec<String>, As2ObjectError> {
    valid_name(source)?;
    if source.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    if source.starts_with('/') {
        result.push("_root".to_owned());
    }
    for slash_part in source.split('/') {
        if slash_part.is_empty() {
            continue;
        }
        if slash_part == ".." {
            result.push("_parent".to_owned());
            continue;
        }
        for component in slash_part.split(['.', ':']) {
            if component.is_empty() {
                return Err(As2ObjectError(
                    "variable path contains an empty component".into(),
                ));
            }
            result.push(component.to_owned());
            if result.len() > 128 {
                return Err(As2ObjectError(
                    "variable path exceeds 128 components".into(),
                ));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_dynamic_property_family() {
        assert_eq!(
            reference_path("/clip:score").unwrap(),
            ["_root", "clip", "score"]
        );
        assert_eq!(
            reference_path("../item.value").unwrap(),
            ["_parent", "item", "value"]
        );
        assert!(reference_path("a..b").is_err());
        let mut state = As2ObjectState::default();
        let object = state.new_id().unwrap();
        let first = state.watch(object, "speed").unwrap();
        assert_eq!(state.begin_write(object, "speed"), first);
        assert_eq!(state.begin_write(object, "speed"), 0);
        state.end_write(object, "speed");
        state.deleted(object, "speed");
        assert_eq!(state.watched(object, "speed"), first);
        let replacement = state.watch(object, "speed").unwrap();
        assert_ne!(first, replacement);
        assert!(!state.add_accessor(object, "speed").unwrap());
        assert_eq!(state.unwatch(object, "speed"), replacement);
        assert_eq!(state.unwatch(object, "speed"), 0);
        assert!(state.add_accessor(object, "speed").unwrap());
        assert_eq!(state.watch(object, "speed").unwrap(), 0);
        state.deleted(object, "speed");
        assert_ne!(state.watch(object, "speed").unwrap(), 0);
        assert!(state.begin_resolve(object, "missing").unwrap());
        assert!(!state.begin_resolve(object, "missing").unwrap());
        state.end_resolve(object, "missing");
        assert!(state.begin_resolve(object, "missing").unwrap());
        let field = state.new_id().unwrap();
        assert_eq!(
            state
                .text_binding(&json!({"op":"bind","field":field,"path":"_root.score"}).to_string())
                .unwrap(),
            json!(["_root", "score"])
        );
        let initial = json!({"op":"plan","field":field,"owner":object,"exists":true,"source":{"kind":"Number","value":42},"text":"old","fromField":false});
        assert_eq!(
            state.text_binding(&initial.to_string()).unwrap(),
            json!({"kind":"text","value":"42"})
        );
        assert_eq!(
            state.text_binding(&initial.to_string()).unwrap(),
            Value::Null
        );
        state
            .text_binding(&json!({"op":"finish","field":field,"accepted":true}).to_string())
            .unwrap();
        assert_eq!(
            state.text_binding(&initial.to_string()).unwrap(),
            Value::Null
        );
        let input = json!({"op":"plan","field":field,"owner":object,"exists":true,"source":{"kind":"Number","value":42},"text":"edited","fromField":true});
        assert_eq!(
            state.text_binding(&input.to_string()).unwrap(),
            json!({"kind":"variable","value":"edited"})
        );
        state
            .text_binding(&json!({"op":"finish","field":field,"accepted":true}).to_string())
            .unwrap();
        state
            .text_binding(&json!({"op":"bind","field":field,"path":"missing"}).to_string())
            .unwrap();
        assert_eq!(state.text_binding(&json!({"op":"plan","field":field,"owner":object,"exists":false,"text":"seed","fromField":false}).to_string()).unwrap(),json!({"kind":"variable","value":"seed"}));
        state
            .text_binding(&json!({"op":"bind","field":field,"path":null}).to_string())
            .unwrap();
        assert_eq!(
            state
                .text_binding(&json!({"op":"get","field":field}).to_string())
                .unwrap(),
            Value::Null
        );
        state.reset();
        assert_eq!(state.watched(object, "speed"), 0);
    }
}
