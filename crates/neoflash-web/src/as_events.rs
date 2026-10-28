use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn as_event_schema_json() -> String {
    neoflash_core::as_events::SCHEMA.into()
}

#[wasm_bindgen]
pub fn as_event_constants_json() -> String {
    neoflash_core::as_events::CONSTANTS.into()
}

#[wasm_bindgen]
pub fn as_event_format_json(class_name: &str, entries: &str) -> Result<String, JsValue> {
    neoflash_core::as_events::format_event(class_name, entries).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub struct ScriptEventPayload {
    payload: neoflash_core::as_events::EventPayload,
}
#[wasm_bindgen]
impl ScriptEventPayload {
    #[wasm_bindgen(constructor)]
    pub fn new(kind: &str, input: &str) -> Result<Self, JsValue> {
        Ok(Self {
            payload: neoflash_core::as_events::EventPayload::new(kind, input)
                .map_err(|e| JsValue::from_str(&e))?,
        })
    }
    pub fn get(&self, name: &str) -> Result<String, JsValue> {
        self.payload
            .get(name)
            .map(|v| v.to_string())
            .map_err(|e| JsValue::from_str(&e))
    }
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), JsValue> {
        self.payload
            .set(name, value)
            .map_err(|e| JsValue::from_str(&e))
    }
}
