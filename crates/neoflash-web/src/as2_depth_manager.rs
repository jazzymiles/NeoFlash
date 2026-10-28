//! AS2 DepthManager transport over the existing authoritative display graph.
use super::*;
#[wasm_bindgen]
impl WebPlayer {
    pub fn as2_depth_manager(&mut self, reference: &str, request: &str) -> Result<String, JsValue> {
        if request.len() > 4096 {
            return Err(JsValue::from_str("DepthManager request exceeds 4096 bytes"));
        }
        let value = serde_json::from_str(request).map_err(display_error)?;
        self.player
            .as2_depth_manager(&object_ref(reference)?, value)
            .map(|v| v.to_string())
            .map_err(display_error)
    }
}
