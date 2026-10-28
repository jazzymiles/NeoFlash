use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn as_date_get(operation: &str, time: f64, timezone_offset: f64) -> Result<f64, JsValue> {
    neoflash_core::as_date::get(operation, time, timezone_offset).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn as_date_set(
    operation: &str,
    time: f64,
    args: Vec<f64>,
    timezone_offset: f64,
) -> Result<f64, JsValue> {
    neoflash_core::as_date::set(operation, time, &args, timezone_offset)
        .map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn as_date_construct(args: Vec<f64>) -> f64 {
    neoflash_core::as_date::construct(&args)
}

#[wasm_bindgen]
pub fn as_date_parts(time: f64) -> Vec<f64> {
    neoflash_core::as_date::parts(time).to_vec()
}

#[wasm_bindgen]
pub fn as_date_clip(time: f64) -> f64 {
    neoflash_core::as_date::clip(time)
}

#[wasm_bindgen]
pub fn as_date_utc_string(time: f64) -> String {
    neoflash_core::as_date::utc_string(time)
}

#[wasm_bindgen]
pub struct DateFormatterState(neoflash_core::as_date::DateFormatter);

#[wasm_bindgen]
impl DateFormatterState {
    #[wasm_bindgen(constructor)]
    pub fn new(
        profile_json: &str,
        date_style: &str,
        time_style: &str,
    ) -> Result<DateFormatterState, JsValue> {
        neoflash_core::as_date::DateFormatter::new(profile_json, date_style, time_style)
            .map(Self)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn snapshot(&self) -> String {
        self.0.snapshot().to_string()
    }
    pub fn set_styles(&mut self, date_style: &str, time_style: &str) -> Result<(), JsValue> {
        self.0
            .set_styles(date_style, time_style)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn set_pattern(&mut self, pattern: &str) -> Result<(), JsValue> {
        self.0
            .set_pattern(pattern)
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn names(&mut self, months: bool, style: &str, context: &str) -> Result<String, JsValue> {
        self.0
            .names(months, style, context)
            .map(|names| serde_json::to_string(&names).unwrap())
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn first_weekday(&mut self) -> u8 {
        self.0.first_weekday()
    }
    pub fn note_read(&mut self) {
        self.0.note_read()
    }
    pub fn format(
        &mut self,
        time: f64,
        offset: f64,
        zone_short: &str,
        zone_long: &str,
    ) -> Result<String, JsValue> {
        self.0
            .format(time, offset, zone_short, zone_long)
            .map_err(|e| JsValue::from_str(&e))
    }
}
