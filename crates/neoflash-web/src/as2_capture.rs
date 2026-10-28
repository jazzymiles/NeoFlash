//! Thin capture facts/sample transport; policy and measurements remain Rust.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Capture {
    inner: neoflash_core::as2_capture::ScriptAS2Capture,
}
#[wasm_bindgen]
impl ScriptAS2Capture {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_capture::ScriptAS2Capture::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn audio_samples(&mut self, id: u32, now: f64, samples: &[f32]) -> Result<String, JsValue> {
        self.inner
            .audio_samples(id, now, samples)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn video_pixels(&mut self, id: u32, now: f64, pixels: &[u8]) -> Result<String, JsValue> {
        self.inner
            .video_pixels(id, now, pixels)
            .map_err(|e| JsValue::from_str(&e))
    }
}
