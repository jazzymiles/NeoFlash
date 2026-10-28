//! Thin WASM binding for the AS2 broadcaster's Rust listener state.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ScriptBroadcaster {
    inner: neoflash_core::as_broadcaster::ScriptBroadcaster,
}

#[wasm_bindgen]
impl ScriptBroadcaster {
    #[wasm_bindgen(constructor)]
    pub fn new(limit: u32) -> Result<Self, JsValue> {
        Ok(Self {
            inner: neoflash_core::as_broadcaster::ScriptBroadcaster::new(limit as usize)
                .map_err(|error| JsValue::from_str(&error))?,
        })
    }
    pub fn add(&mut self, existing_id: u32) -> Result<u32, JsValue> {
        self.inner
            .add(existing_id)
            .map_err(|error| JsValue::from_str(&error))
    }
    pub fn remove(&mut self, id: u32) -> bool {
        self.inner.remove(id)
    }
    pub fn listeners(&self) -> Vec<u32> {
        self.inner.listeners().to_vec()
    }
    pub fn at(&self, index: u32) -> u32 {
        self.inner.at(index as usize)
    }
    pub fn len(&self) -> u32 {
        self.inner.listeners().len() as u32
    }
    pub fn is_empty(&self) -> bool {
        self.inner.listeners().is_empty()
    }
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}
