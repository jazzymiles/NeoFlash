//! Thin numeric ABI for the shared Rust scripted easing calculations.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn as_custom_ease_value(
    points: &[f64],
    time: f64,
    begin: f64,
    change: f64,
    duration: f64,
) -> Result<f64, JsValue> {
    neoflash_core::as_easing::custom_ease(points, time, begin, change, duration)
        .map_err(|error| JsValue::from_str(&error))
}

#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn as_easing_value(
    family: &str,
    mode: &str,
    time: f64,
    begin: f64,
    change: f64,
    duration: f64,
    parameter: f64,
    period: f64,
) -> Result<f64, JsValue> {
    neoflash_core::as_easing::ease(
        family, mode, time, begin, change, duration, parameter, period,
    )
    .map_err(|error| JsValue::from_str(&error))
}

#[wasm_bindgen]
pub struct ScriptTween {
    inner: neoflash_core::as_easing::ScriptTweenState,
}
#[wasm_bindgen]
impl ScriptTween {
    #[wasm_bindgen(constructor)]
    pub fn new(settings: &str, now: f64) -> Result<Self, JsValue> {
        if settings.len() > 16384 {
            return Err(JsValue::from_str("Tween settings exceed 16384 bytes"));
        }
        let settings = serde_json::from_str(settings)
            .map_err(|error| JsValue::from_str(&format!("Invalid Tween settings: {error}")))?;
        Ok(Self {
            inner: neoflash_core::as_easing::ScriptTweenState::new(&settings, now)
                .map_err(|error| JsValue::from_str(&error))?,
        })
    }
    pub fn snapshot(&self) -> String {
        serde_json::to_string(&self.inner).expect("finite Tween state")
    }
    pub fn command(&mut self, operation: &str, args: &str, now: f64) -> Result<String, JsValue> {
        if args.len() > 16384 {
            return Err(JsValue::from_str("Tween arguments exceed 16384 bytes"));
        }
        let args = serde_json::from_str(args)
            .map_err(|error| JsValue::from_str(&format!("Invalid Tween command: {error}")))?;
        self.inner
            .command(operation, &args, now)
            .map(|result| result.to_string())
            .map_err(|error| JsValue::from_str(&error))
    }
}
