//! Thin WASM transport for AS2 video component policy.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Media {
    inner: neoflash_core::as2_media::ScriptAS2Media,
}
#[wasm_bindgen]
impl ScriptAS2Media {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_media::ScriptAS2Media::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
}

#[wasm_bindgen]
pub struct ScriptAS2Stream {
    inner: neoflash_core::as2_media::ScriptAS2Stream,
}
#[wasm_bindgen]
impl ScriptAS2Stream {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_media::ScriptAS2Stream::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
}
