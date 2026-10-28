//! Thin transport for the shared Rust 2D geometry value family.
use neoflash_core::as2d_geom::Geometry2dValue;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Geom2dValue {
    inner: Geometry2dValue,
}
fn error(value: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&value.to_string())
}
fn input(value: &str) -> Result<serde_json::Value, JsValue> {
    if value.len() > 16 * 1024 {
        return Err(error("2D geometry transport exceeds 16 KiB"));
    }
    serde_json::from_str(value).map_err(error)
}
#[wasm_bindgen]
impl Geom2dValue {
    #[wasm_bindgen(constructor)]
    pub fn new(kind: &str, data_json: &str) -> Result<Geom2dValue, JsValue> {
        Ok(Self {
            inner: Geometry2dValue::new(kind, &input(data_json)?).map_err(error)?,
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
    pub fn calculate(
        kind: &str,
        data_json: &str,
        operation: &str,
        args_json: &str,
    ) -> Result<String, JsValue> {
        Self::new(kind, data_json)?.call(operation, args_json)
    }
}
