use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as2_variables_json(source: &str) -> Result<String, JsValue> {
    serde_json::to_string(
        &neoflash_core::as2_variables::decode(source).map_err(crate::display_error)?,
    )
    .map_err(crate::display_error)
}
