//! Thin WASM transport for the AS2 binding policy and conversions.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Binding {
    inner: neoflash_core::as2_binding::ScriptAS2Binding,
}
#[wasm_bindgen]
impl ScriptAS2Binding {
    #[wasm_bindgen(constructor)]
    pub fn new(config: &str) -> Result<Self, JsValue> {
        Ok(Self {
            inner: neoflash_core::as2_binding::ScriptAS2Binding::new(config)
                .map_err(|e| JsValue::from_str(&e))?,
        })
    }
    pub fn plan(&self) -> String {
        self.inner.plan()
    }
    pub fn enter(&mut self, reverse: bool) -> u32 {
        self.inner.enter(reverse)
    }
    pub fn leave(&mut self) {
        self.inner.leave();
    }
}
#[wasm_bindgen]
pub fn script_as2_binding_value(request: &str) -> Result<String, JsValue> {
    neoflash_core::as2_binding::execute(request).map_err(|e| JsValue::from_str(&e))
}
