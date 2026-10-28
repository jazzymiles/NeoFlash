//! Browser transport around AS2 Sound metadata and channel transforms.
use super::{WebPlayer, display_error};
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn as2_sound_metadata(bytes: &[u8]) -> Result<String, JsValue> {
    let value = neoflash_core::as2_sound::metadata(bytes).map_err(display_error)?;
    serde_json::to_string(&value).map_err(display_error)
}
#[wasm_bindgen]
pub fn as2_sound_transform(input: &str) -> Result<String, JsValue> {
    if input.len() > 4096 {
        return Err(JsValue::from_str("AS2 Sound transform exceeds 4 KiB"));
    }
    let command = serde_json::from_str(input).map_err(display_error)?;
    let value = neoflash_core::as2_sound::transform(command).map_err(display_error)?;
    serde_json::to_string(&value).map_err(display_error)
}
#[wasm_bindgen]
impl WebPlayer {
    pub fn as2_sound_resource(&self, id: &str) -> Result<String, JsValue> {
        serde_json::to_string(
            &neoflash_core::as2_sound::resource_path(&self.project, id).map_err(display_error)?,
        )
        .map_err(display_error)
    }
}
