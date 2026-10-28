//! Host bindings for AS2 local messages and AMF0 remoting envelopes.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Connections(neoflash_core::as2_connections::ScriptAS2Connections);
#[wasm_bindgen]
impl ScriptAS2Connections {
    #[wasm_bindgen(constructor)]
    pub fn new(input: &str) -> Result<Self, JsValue> {
        neoflash_core::as2_connections::ScriptAS2Connections::new(input)
            .map(Self)
            .map_err(crate::display_error)
    }
    pub fn execute(&mut self, input: &str, bytes: &[u8]) -> Result<String, JsValue> {
        self.0
            .execute(input, bytes)
            .map(|v| v.to_string())
            .map_err(crate::display_error)
    }
}
#[wasm_bindgen]
pub fn as2_remoting_request(target: &str, response: &str, body: &[u8]) -> Result<Vec<u8>, JsValue> {
    neoflash_core::as2_connections::remoting_request(target, response, body)
        .map_err(crate::display_error)
}
#[wasm_bindgen]
pub fn as2_remoting_response(bytes: &[u8], response: &str) -> Result<String, JsValue> {
    neoflash_core::as2_connections::remoting_response(bytes, response)
        .map(|v| v.to_string())
        .map_err(crate::display_error)
}
