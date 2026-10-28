use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptNumberFormatter {
    inner: neoflash_core::as_number_format::NumberFormatter,
}
#[wasm_bindgen]
impl ScriptNumberFormatter {
    #[wasm_bindgen(constructor)]
    pub fn new(metadata: &str) -> Result<Self, JsValue> {
        if metadata.len() > 16384 {
            return Err(JsValue::from_str(
                "NumberFormatter metadata exceeds 16384 bytes",
            ));
        }
        let value =
            serde_json::from_str(metadata).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self {
            inner: neoflash_core::as_number_format::NumberFormatter::new(&value)
                .map_err(|e| JsValue::from_str(&e))?,
        })
    }
    pub fn snapshot(&self) -> String {
        serde_json::to_string(&self.inner).expect("finite NumberFormatter state")
    }
    pub fn set_property(&mut self, name: &str, value: &str) -> Result<(), JsValue> {
        if value.len() > 16384 {
            return Err(JsValue::from_str(
                "NumberFormatter property exceeds 16384 bytes",
            ));
        }
        let value = serde_json::from_str(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.inner
            .set_property(name, value)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn format(&mut self, value: f64, kind: &str) -> Result<String, JsValue> {
        self.inner
            .format(value, kind)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn parse(&mut self, text: &str, strict: bool) -> Result<String, JsValue> {
        self.inner
            .parse(text, strict)
            .map(|v| v.to_string())
            .map_err(|e| JsValue::from_str(&e))
    }
}
