//! Thin UTF-16/JSON bridge into the shared Rust AS3 String implementation.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as3_string_json(request_json: &str) -> Result<String, JsValue> {
    use neoflash_core::as3_strings::{
        As3StringRequest, MAX_AS3_STRING_JSON_BYTES, as3_string_execute,
    };
    if request_json.len() > MAX_AS3_STRING_JSON_BYTES {
        return Err(JsValue::from_str("AS3 String transport exceeds 32 MiB"));
    }
    let request: As3StringRequest =
        serde_json::from_str(request_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let result = as3_string_execute(request).map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_json::to_string(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}
