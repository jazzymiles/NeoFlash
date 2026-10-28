//! Byte transport into the shared Rust AS3 ByteArray codec family.
use wasm_bindgen::prelude::*;

fn utf8_error(error: neoflash_core::as_binary::BinaryUtf8Error) -> JsValue {
    use neoflash_core::as_binary::BinaryUtf8Error;
    match error {
        BinaryUtf8Error::Length => js_sys::RangeError::new(&error.to_string()).into(),
        _ => js_sys::TypeError::new(&error.to_string()).into(),
    }
}

#[wasm_bindgen]
pub fn as_binary_encode_utf8(units: &[u16], limit: usize) -> Result<Vec<u8>, JsValue> {
    neoflash_core::as_binary::binary_encode_utf8(units, limit).map_err(utf8_error)
}

#[wasm_bindgen]
pub fn as_binary_decode_utf8(
    bytes: &[u8],
    stop_at_null: bool,
    limit: usize,
) -> Result<String, JsValue> {
    neoflash_core::as_binary::binary_decode_utf8(bytes, stop_at_null, limit).map_err(utf8_error)
}

#[wasm_bindgen]
pub fn as_binary_transform(
    operation: &str,
    bytes: &[u8],
    text: &str,
    parameter: &str,
    limit: usize,
) -> Result<Vec<u8>, JsValue> {
    neoflash_core::as_binary::binary_transform(operation, bytes, text, parameter, limit)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn as_binary_decode(bytes: &[u8], charset: &str, limit: usize) -> Result<String, JsValue> {
    neoflash_core::as_binary::binary_decode(bytes, charset, limit)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn as_binary_to_text(operation: &str, bytes: &[u8], limit: usize) -> Result<String, JsValue> {
    neoflash_core::as_binary::binary_to_text(operation, bytes, limit)
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
