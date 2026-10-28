//! Thin WASM bridge for AS2 Locale tables and XLIFF.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Locale {
    inner: neoflash_core::as2_locale::ScriptAS2Locale,
}
#[wasm_bindgen]
impl ScriptAS2Locale {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_locale::ScriptAS2Locale::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
}
