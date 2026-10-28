//! Marshaling for AS2 display-transition state.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub struct ScriptAS2Transition(neoflash_core::as2_transitions::ScriptAS2Transition);
#[wasm_bindgen]
impl ScriptAS2Transition {
    #[wasm_bindgen(constructor)]
    pub fn new(input: &str, now: f64) -> Result<Self, JsValue> {
        neoflash_core::as2_transitions::ScriptAS2Transition::new(input, now)
            .map(Self)
            .map_err(crate::display_error)
    }
    pub fn sample(&self, now: f64) -> Result<String, JsValue> {
        self.0
            .sample(now)
            .map(|v| v.to_string())
            .map_err(crate::display_error)
    }
}
