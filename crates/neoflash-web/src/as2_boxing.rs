use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as2_box_json(input: &str) -> Result<String, JsValue> {
    neoflash_core::as2_boxing::create(input).map_err(crate::display_error)
}
