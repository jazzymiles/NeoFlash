//! Browser transport around AS2/AS3 Sound metadata, PCM/compressed audio, and spectrum/channel transforms.
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
pub fn as3_sound_inspect_id3_json(bytes: &[u8]) -> Result<String, JsValue> {
    serde_json::to_string(&neoflash_core::as3_sound_media::inspect_id3(bytes))
        .map_err(display_error)
}
#[wasm_bindgen]
pub fn as3_sound_decode_compressed(bytes: &[u8]) -> Result<Vec<u8>, JsValue> {
    let decoded = neoflash_core::as3_sound_media::decode_compressed_to_canonical(bytes)
        .map_err(|e| JsValue::from_str(&e))?;
    let meta_json = serde_json::to_vec(&decoded.meta).map_err(display_error)?;
    let mut out = Vec::with_capacity(4 + meta_json.len() + decoded.pcm_le_f32.len());
    out.extend_from_slice(&(meta_json.len() as u32).to_le_bytes());
    out.extend_from_slice(&meta_json);
    out.extend_from_slice(&decoded.pcm_le_f32);
    Ok(out)
}
#[wasm_bindgen]
pub fn as3_sound_decode_pcm(
    bytes: &[u8],
    samples: usize,
    format: &str,
    stereo: bool,
    sample_rate: f64,
    little_endian: bool,
) -> Result<Vec<u8>, JsValue> {
    neoflash_core::as3_sound_media::decode_pcm_to_canonical(
        bytes,
        samples,
        format,
        stereo,
        sample_rate,
        little_endian,
    )
    .map_err(|e| JsValue::from_str(&e))
}
#[wasm_bindgen]
pub fn as3_sound_extract_f32(
    pcm_le_f32: &[u8],
    length_frames: f64,
    start_position: f64,
    cursor_frame: usize,
    target_little_endian: bool,
) -> Result<Vec<u8>, JsValue> {
    let (pcm, meta) = neoflash_core::as3_sound_media::extract_canonical_f32(
        pcm_le_f32,
        length_frames,
        start_position,
        cursor_frame,
        target_little_endian,
    )
    .map_err(|e| JsValue::from_str(&e))?;
    let mut out = Vec::with_capacity(16 + pcm.len());
    out.extend_from_slice(&(meta.frames_extracted as f64).to_le_bytes());
    out.extend_from_slice(&(meta.next_cursor as f64).to_le_bytes());
    out.extend_from_slice(&pcm);
    Ok(out)
}
#[wasm_bindgen]
pub fn as3_sound_channel_peaks_json(
    pcm_le_f32: &[u8],
    position_ms: f64,
    channel_tf_json: &str,
    mixer_tf_json: &str,
) -> Result<String, JsValue> {
    if channel_tf_json.len() > 2048 || mixer_tf_json.len() > 2048 {
        return Err(JsValue::from_str("SoundTransform JSON exceeds 2 KiB"));
    }
    let ch_tf: neoflash_core::as3_sound_media::As3SoundTransform =
        serde_json::from_str(channel_tf_json).map_err(display_error)?;
    let mx_tf: neoflash_core::as3_sound_media::As3SoundTransform =
        serde_json::from_str(mixer_tf_json).map_err(display_error)?;
    let (left_peak, right_peak) =
        neoflash_core::as3_sound_media::channel_peaks(pcm_le_f32, position_ms, &ch_tf, &mx_tf);
    Ok(format!(
        "{{\"leftPeak\":{left_peak},\"rightPeak\":{right_peak}}}"
    ))
}
#[wasm_bindgen]
pub fn as3_sound_compute_spectrum(
    mixed_le_f32: &[u8],
    fft_mode: bool,
    stretch_factor: i32,
    target_little_endian: bool,
) -> Vec<u8> {
    neoflash_core::as3_sound_media::compute_spectrum(
        mixed_le_f32,
        fft_mode,
        stretch_factor,
        target_little_endian,
    )
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
