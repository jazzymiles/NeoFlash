//! Thin WASM transport for AS2/AS3 video stream and component policy.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ScriptAS2Media {
    inner: neoflash_core::as2_media::ScriptAS2Media,
}
#[wasm_bindgen]
impl ScriptAS2Media {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_media::ScriptAS2Media::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
}

#[wasm_bindgen]
pub struct ScriptAS2Stream {
    inner: neoflash_core::as2_media::ScriptAS2Stream,
}
#[wasm_bindgen]
impl ScriptAS2Stream {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as2_media::ScriptAS2Stream::new(),
        }
    }
    pub fn execute(&mut self, request: &str) -> Result<String, JsValue> {
        self.inner
            .execute(request)
            .map_err(|e| JsValue::from_str(&e))
    }
}

#[wasm_bindgen]
pub fn netstream_play_plan_json(request_json: &str) -> Result<String, JsValue> {
    if request_json.len() > 16384 {
        return Err(JsValue::from_str("NetStream.play request exceeds 16 KiB"));
    }
    let req: serde_json::Value =
        serde_json::from_str(request_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let stop_flag = req.get("stop").and_then(|v| v.as_bool()).unwrap_or(false);
    let is_null = req.get("isNull").and_then(|v| v.as_bool()).unwrap_or(false);
    let url = req.get("url").and_then(|v| v.as_str());
    let start = req.get("start").and_then(|v| v.as_f64());
    let len = req.get("len").and_then(|v| v.as_f64());
    let reset_val = req.get("reset");
    let plan = neoflash_core::as3_sound_media::plan_netstream_play(
        url, stop_flag, is_null, start, len, reset_val,
    );
    serde_json::to_string(&plan).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn netstream_seek_decision_json(
    offset: f64,
    metadata_known: bool,
    duration: f64,
) -> Result<String, JsValue> {
    let decision =
        neoflash_core::as3_sound_media::decide_netstream_seek(offset, metadata_known, duration);
    serde_json::to_string(&decision).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn netstream_inspect_container_json(bytes: &[u8]) -> Result<String, JsValue> {
    let probe = neoflash_core::as3_sound_media::inspect_media_container(bytes);
    serde_json::to_string(&probe).map_err(|e| JsValue::from_str(&e.to_string()))
}
