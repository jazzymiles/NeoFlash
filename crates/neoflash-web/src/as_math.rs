use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn as_math_value(
    as3: bool,
    operation: &str,
    args: Vec<f64>,
    entropy: f64,
) -> Result<f64, JsValue> {
    neoflash_core::as_math::evaluate(as3, operation, &args, entropy)
        .map_err(|error| JsValue::from_str(&error))
}

#[wasm_bindgen]
pub fn as_math_constant(name: &str) -> Result<f64, JsValue> {
    neoflash_core::as_math::constant(name)
        .ok_or_else(|| JsValue::from_str("Unknown ActionScript Math constant"))
}
