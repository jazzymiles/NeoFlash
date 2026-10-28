//! JSON/metadata transport only; AS3 reflection and trait policy execute in Rust core.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as3_object_json(request_json: &str) -> Result<String, JsValue> {
    if request_json.len() > neoflash_core::as3_object::MAX_OBJECT_JSON_BYTES {
        return Err(JsValue::from_str(
            "AS3 Object metadata transport exceeds 8 MiB",
        ));
    }
    let request = serde_json::from_str(request_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid AS3 Object request: {e}")))?;
    let result = neoflash_core::as3_object::execute(request).map_err(|e| JsValue::from_str(&e))?;
    serde_json::to_string(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}
