//! Thin WASM binding for Rust callback timer identifiers.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ScriptTimerIds {
    inner: neoflash_core::as_timers::ScriptTimerIds,
}

#[wasm_bindgen]
impl ScriptTimerIds {
    #[wasm_bindgen(constructor)]
    pub fn new(limit: u32) -> Result<Self, JsValue> {
        Ok(Self {
            inner: neoflash_core::as_timers::ScriptTimerIds::new(limit as usize)
                .map_err(|error| JsValue::from_str(&error))?,
        })
    }
    pub fn register(&mut self, delay: f64, repeating: bool) -> Result<u32, JsValue> {
        self.inner
            .register(delay, repeating)
            .map_err(|error| JsValue::from_str(&error))
    }
    pub fn clear(&mut self, id: f64) -> Result<bool, JsValue> {
        self.inner
            .clear(id)
            .map_err(|error| JsValue::from_str(&error))
    }
    pub fn fire(&mut self, id: u32) -> bool {
        self.inner.fire(id)
    }
    pub fn contains(&self, id: u32) -> bool {
        self.inner.contains(id)
    }
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}
