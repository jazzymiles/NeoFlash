//! Owned URL-encoded variable decoder for AS2 timeline loading.
//! This explicit UTF-8 profile shares browser transport limits, not an AVM VM.
use std::collections::BTreeMap;
#[derive(Debug, thiserror::Error)]
#[error("AS2 variables: {0}")]
pub struct VariableError(pub String);
fn component(source: &str) -> Result<String, VariableError> {
    let bytes = source.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => result.push(b' '),
            b'%' => {
                let pair = bytes
                    .get(index + 1..index + 3)
                    .ok_or_else(|| VariableError("incomplete percent escape".into()))?;
                let high = (pair[0] as char)
                    .to_digit(16)
                    .ok_or_else(|| VariableError("invalid percent escape".into()))?;
                let low = (pair[1] as char)
                    .to_digit(16)
                    .ok_or_else(|| VariableError("invalid percent escape".into()))?;
                result.push((high * 16 + low) as u8);
                index += 2;
            }
            byte => result.push(byte),
        }
        index += 1;
    }
    String::from_utf8(result).map_err(|_| VariableError("form data is not valid UTF-8".into()))
}
pub fn decode(source: &str) -> Result<BTreeMap<String, serde_json::Value>, VariableError> {
    if source.len() > 16 * 1024 * 1024 {
        return Err(VariableError("response exceeds 16 MiB".into()));
    }
    let mut result = BTreeMap::new();
    let mut count = 0;
    for pair in source.split('&').filter(|pair| !pair.is_empty()) {
        count += 1;
        if count > 1024 {
            return Err(VariableError("response exceeds 1024 pairs".into()));
        }
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| VariableError("expected name=value pairs".into()))?;
        let key = component(key)?;
        let value = serde_json::Value::String(component(value)?);
        match result.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(value);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let previous = entry.get_mut();
                if let serde_json::Value::Array(items) = previous {
                    items.push(value);
                } else {
                    *previous = serde_json::Value::Array(vec![previous.take(), value]);
                }
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeline_variables_family() {
        let values =
            decode("title=Hello+web&tag=rust&tag=as2&unicode=%D0%BC%D0%B8%D1%80&__proto__=data")
                .unwrap();
        assert_eq!(values["title"], "Hello web");
        assert_eq!(values["unicode"], "мир");
        assert_eq!(values["tag"], serde_json::json!(["rust", "as2"]));
        assert_eq!(values["__proto__"], "data");
        assert!(decode("x=%ZZ").is_err());
        assert!(decode("x=%ff").is_err());
        assert!(decode("missing").is_err());
        assert!(decode(&"x=1&".repeat(1025)).is_err());
    }
}
