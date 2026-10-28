//! Thin WASM request transport for nonvisual AS2 data models.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Data {
    inner: neoflash_core::as2_data::ScriptAS2Data,
}
#[wasm_bindgen]
impl ScriptAS2Data {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_data::ScriptAS2Data::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|error| JsValue::from_str(&error))
    }
}
impl Default for ScriptAS2Data {
    fn default() -> Self {
        Self::new()
    }
}
