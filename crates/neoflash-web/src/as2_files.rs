//! Browser file API marshaling around the owned Rust file/multipart kernel.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as2_file_command(input: &str) -> Result<String, JsValue> {
    neoflash_core::as2_files::command(input)
        .map(|value| value.to_string())
        .map_err(crate::display_error)
}
#[wasm_bindgen]
pub fn as2_file_multipart(input: &str, bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    neoflash_core::as2_files::multipart(input, bytes).map_err(crate::display_error)
}
