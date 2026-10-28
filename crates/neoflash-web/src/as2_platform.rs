//! Browser data marshalling for the AS2-only Rust platform policy.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ScriptAS2Platform {
    inner: neoflash_core::as2_platform::ScriptAS2Platform,
}

#[wasm_bindgen]
impl ScriptAS2Platform {
    #[wasm_bindgen(constructor)]
    pub fn new(facts_json: &str) -> Result<Self, JsValue> {
        Ok(Self {
            inner: neoflash_core::as2_platform::ScriptAS2Platform::new(facts_json)
                .map_err(|error| JsValue::from_str(&error))?,
        })
    }
    pub fn capabilities_json(&self) -> String {
        self.inner.capabilities_json()
    }
    pub fn server_string(&self) -> String {
        self.inner.server_string().into()
    }
    pub fn sandbox_type(&self) -> String {
        self.inner.sandbox_type().into()
    }
    pub fn key_toggle(&mut self, code: u32, enabled: bool) {
        self.inner.key_toggle(code, enabled);
    }
    pub fn is_toggled(&self, code: u32) -> bool {
        self.inner.is_toggled(code)
    }
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

/// PrintJob policy uses the same AS2 platform module and actual exported PNGs.
#[wasm_bindgen]
pub struct ScriptAS2Print {
    inner: neoflash_core::as2_platform::ScriptAS2Print,
}
#[wasm_bindgen]
impl ScriptAS2Print {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_platform::ScriptAS2Print::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
}
