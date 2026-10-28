//! Browser-facing marshaling for the AS2 SOAP/WSDL kernel.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as2_service_command(input: &str) -> Result<String, JsValue> {
    neoflash_core::as2_services::command(input)
        .map(|value| value.to_string())
        .map_err(crate::display_error)
}
