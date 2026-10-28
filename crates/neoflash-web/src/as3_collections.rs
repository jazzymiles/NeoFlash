//! Thin transport for the shared Rust Array mutation policy.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn as3_array_plan_json(method: &str, length: u32, index: i32) -> Result<String, JsValue> {
    let plan = neoflash_core::as3_collections::mutation_plan(method, length, index)
        .map_err(|error| JsValue::from_str(&error))?;
    serde_json::to_string(&plan).map_err(|error| JsValue::from_str(&error.to_string()))
}
