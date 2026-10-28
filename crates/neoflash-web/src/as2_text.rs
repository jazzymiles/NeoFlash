//! Bounded JSON transport for owned AS2 text/style kernels.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as2_text_json(request: &str) -> Result<String, JsValue> {
    if request.len() > 8 * 1024 * 1024 {
        return Err(JsValue::from_str("AS2 text request exceeds 8 MiB"));
    }
    let request: neoflash_core::as2_text::Request =
        serde_json::from_str(request).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let result =
        neoflash_core::as2_text::execute(request).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(result.to_string())
}

use super::{WebPlayer, display_error, object_ref};
#[wasm_bindgen]
impl WebPlayer {
    pub fn as2_snapshot_create(
        &mut self,
        reference: &str,
        measure: &js_sys::Function,
    ) -> Result<String, JsValue> {
        self.player
            .as2_snapshot_create(&self.project, &object_ref(reference)?, |strings, format| {
                let strings = serde_json::to_string(strings)
                    .map_err(|e| neoflash_core::TextError(e.to_string()))?;
                let format = serde_json::to_string(format)
                    .map_err(|e| neoflash_core::TextError(e.to_string()))?;
                let result = measure
                    .call2(
                        &JsValue::NULL,
                        &JsValue::from_str(&strings),
                        &JsValue::from_str(&format),
                    )
                    .map_err(|_| {
                        neoflash_core::TextError("TextSnapshot font measurement failed".into())
                    })?;
                let json = result.as_string().ok_or_else(|| {
                    neoflash_core::TextError(
                        "TextSnapshot measurement must return JSON widths".into(),
                    )
                })?;
                if json.len() > 1024 * 1024 {
                    return Err(neoflash_core::TextError(
                        "TextSnapshot measurement exceeds 1 MiB".into(),
                    ));
                }
                serde_json::from_str(&json).map_err(|e| neoflash_core::TextError(e.to_string()))
            })
            .map_err(display_error)
    }
    pub fn as2_snapshot_command(&mut self, handle: &str, request: &str) -> Result<String, JsValue> {
        if handle.len() > 128 || request.len() > 1024 * 1024 {
            return Err(JsValue::from_str(
                "TextSnapshot request exceeds its transport bounds",
            ));
        }
        let value = serde_json::from_str(request).map_err(display_error)?;
        self.player
            .as2_snapshot_command(handle, value)
            .map(|value| value.to_string())
            .map_err(display_error)
    }
}
