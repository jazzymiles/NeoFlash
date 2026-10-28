//! Thin JSON/UTF-16 transport for the shared Rust AS2 String family.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn as2_string_json(request_json: &str) -> Result<String, JsValue> {
    use neoflash_core::as2_strings::{
        As2StringRequest, MAX_AS2_STRING_JSON_BYTES, as2_string_execute,
    };
    if request_json.len() > MAX_AS2_STRING_JSON_BYTES {
        return Err(JsValue::from_str("AS2 String transport exceeds 32 MiB"));
    }
    let request: As2StringRequest = serde_json::from_str(request_json)
        .map_err(|error| JsValue::from_str(&format!("Invalid AS2 String request: {error}")))?;
    let result =
        as2_string_execute(request).map_err(|error| JsValue::from_str(&error.to_string()))?;
    serde_json::to_string(&result).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn legacy_case_name_json(name: &str, candidates: &str) -> Result<String, JsValue> {
    if candidates.len() > 8_000_000 {
        return Err(JsValue::from_str("legacy property lookup exceeds 8 MB"));
    }
    let names: Vec<String> =
        serde_json::from_str(candidates).map_err(|error| JsValue::from_str(&error.to_string()))?;
    neoflash_core::as2_strings::legacy_case_name(name, &names)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
