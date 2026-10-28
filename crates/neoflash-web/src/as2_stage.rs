use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Stage(neoflash_core::as2_stage::StageState);
#[wasm_bindgen]
impl ScriptAS2Stage {
    #[wasm_bindgen(constructor)]
    pub fn new(width: f64, height: f64) -> Result<Self, JsValue> {
        neoflash_core::as2_stage::StageState::new(width, height)
            .map(Self)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn command(&mut self, input: &str) -> Result<String, JsValue> {
        self.0
            .command(input)
            .map(|v| v.to_string())
            .map_err(|e| JsValue::from_str(&e))
    }
}
