use neoflash_core::as2_object::As2ObjectState;
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Object {
    inner: As2ObjectState,
}
#[wasm_bindgen]
impl ScriptAS2Object {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: As2ObjectState::default(),
        }
    }
    pub fn new_id(&mut self) -> Result<u32, JsValue> {
        self.inner.new_id().map_err(crate::display_error)
    }
    pub fn watch(&mut self, object: u32, name: &str) -> Result<u32, JsValue> {
        self.inner.watch(object, name).map_err(crate::display_error)
    }
    pub fn watched(&self, object: u32, name: &str) -> u32 {
        self.inner.watched(object, name)
    }
    pub fn unwatch(&mut self, object: u32, name: &str) -> u32 {
        self.inner.unwatch(object, name)
    }
    pub fn begin_write(&mut self, object: u32, name: &str) -> u32 {
        self.inner.begin_write(object, name)
    }
    pub fn end_write(&mut self, object: u32, name: &str) {
        self.inner.end_write(object, name);
    }
    pub fn begin_resolve(&mut self, object: u32, name: &str) -> Result<bool, JsValue> {
        self.inner
            .begin_resolve(object, name)
            .map_err(crate::display_error)
    }
    pub fn end_resolve(&mut self, object: u32, name: &str) {
        self.inner.end_resolve(object, name);
    }
    pub fn add_accessor(&mut self, object: u32, name: &str) -> Result<bool, JsValue> {
        self.inner
            .add_accessor(object, name)
            .map_err(crate::display_error)
    }
    pub fn deleted(&mut self, object: u32, name: &str) {
        self.inner.deleted(object, name);
    }
    pub fn text_binding(&mut self, input: &str) -> Result<String, JsValue> {
        self.inner
            .text_binding(input)
            .map(|value| value.to_string())
            .map_err(crate::display_error)
    }
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}
impl Default for ScriptAS2Object {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub fn as2_reference_path_json(source: &str) -> Result<String, JsValue> {
    serde_json::to_string(
        &neoflash_core::as2_object::reference_path(source).map_err(crate::display_error)?,
    )
    .map_err(crate::display_error)
}
