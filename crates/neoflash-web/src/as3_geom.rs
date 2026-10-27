//! Thin transport for the Rust-owned AS3 geometry implementation.
use neoflash_core::as3_geom::GeometryValue;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Geom3dValue {
    inner: GeometryValue,
}
fn error(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}
fn input(value: &str) -> Result<serde_json::Value, JsValue> {
    if value.len() > 512 * 1024 {
        return Err(error("Geometry JSON exceeds 512 KiB"));
    }
    serde_json::from_str(value).map_err(error)
}
#[wasm_bindgen]
impl Geom3dValue {
    #[wasm_bindgen(constructor)]
    pub fn new(kind: &str, data_json: &str) -> Result<Geom3dValue, JsValue> {
        Ok(Self {
            inner: GeometryValue::new(kind, &input(data_json)?).map_err(error)?,
        })
    }
    pub fn snapshot(&self) -> String {
        self.inner.snapshot().to_string()
    }
    pub fn call(&mut self, operation: &str, args_json: &str) -> Result<String, JsValue> {
        Ok(self
            .inner
            .call(operation, &input(args_json)?)
            .map_err(error)?
            .to_string())
    }
}
