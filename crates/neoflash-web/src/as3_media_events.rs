//! WASM bindings for AS3 camera, microphone, StageVideo, multitouch, GameInput, and accessibility (`As3MediaEventsKernel`).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct As3MediaEventsKernel {
    inner: neoflash_core::as3_media_events::As3MediaEventsRuntime,
}

fn media_events_js_err(err: neoflash_core::as3_media_events::MediaEventsError) -> JsValue {
    JsValue::from_str(&err.to_wire_string())
}

fn media_events_json<T: serde::Serialize>(val: &T) -> Result<String, JsValue> {
    serde_json::to_string(val).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
impl As3MediaEventsKernel {
    #[wasm_bindgen(constructor)]
    pub fn new(max_touch_points: i32) -> As3MediaEventsKernel {
        Self {
            inner: neoflash_core::as3_media_events::As3MediaEventsRuntime::new(max_touch_points),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn camera_names_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.camera_names())
    }

    pub fn resolve_camera(&self, name: Option<String>) -> i32 {
        self.inner
            .resolve_camera(name.as_deref())
            .map_or(-1, |idx| idx as i32)
    }

    pub fn camera_state_json(&self, index: u32) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_get(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(state)
    }

    pub fn camera_set_mode_json(
        &mut self,
        index: u32,
        width: i32,
        height: i32,
        fps: f64,
        favor_area: bool,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_mode(index as usize, width, height, fps, favor_area)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_quality_json(
        &mut self,
        index: u32,
        bandwidth: i32,
        quality: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_quality(index as usize, bandwidth, quality)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_key_frame_interval_json(
        &mut self,
        index: u32,
        interval: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_key_frame_interval(index as usize, interval)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_loopback_json(
        &mut self,
        index: u32,
        compress: bool,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_loopback(index as usize, compress)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_motion_level_json(
        &mut self,
        index: u32,
        motion_level: i32,
        timeout: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_motion_level(index as usize, motion_level, timeout)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_set_cursor_json(&mut self, index: u32, cursor: bool) -> Result<String, JsValue> {
        let state = self
            .inner
            .camera_set_cursor(index as usize, cursor)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn camera_frame_argb_u32(
        &self,
        index: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Vec<u32>, JsValue> {
        self.inner
            .camera_frame_argb_u32(index as usize, x, y, w, h)
            .map_err(media_events_js_err)
    }

    pub fn camera_frame_argb_bytes(
        &self,
        index: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Vec<u8>, JsValue> {
        self.inner
            .camera_frame_argb_bytes(index as usize, x, y, w, h)
            .map_err(media_events_js_err)
    }

    pub fn microphone_names_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.microphone_names())
    }

    pub fn resolve_microphone(&self, index: i32) -> i32 {
        self.inner
            .resolve_microphone(index)
            .map_or(-1, |idx| idx as i32)
    }

    pub fn mic_state_json(&self, index: u32) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_get(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(state)
    }

    pub fn mic_enable_enhanced_json(&mut self, index: u32) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_enable_enhanced(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_codec_json(&mut self, index: u32, codec: &str) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_codec(index as usize, codec)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_rate_json(&mut self, index: u32, rate: i32) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_rate(index as usize, rate)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_gain_json(&mut self, index: u32, gain: f64) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_gain(index as usize, gain)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_encode_quality_json(
        &mut self,
        index: u32,
        quality: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_encode_quality(index as usize, quality)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_frames_per_packet_json(
        &mut self,
        index: u32,
        frames: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_frames_per_packet(index as usize, frames)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_noise_suppression_level_json(
        &mut self,
        index: u32,
        level: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_noise_suppression_level(index as usize, level)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_enable_vad_json(&mut self, index: u32, enable: bool) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_enable_vad(index as usize, enable)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_loop_back_json(&mut self, index: u32, state: bool) -> Result<String, JsValue> {
        let st = self
            .inner
            .mic_set_loop_back(index as usize, state)
            .map_err(media_events_js_err)?;
        media_events_json(&st)
    }

    pub fn mic_set_silence_level_json(
        &mut self,
        index: u32,
        silence_level: f64,
        timeout: i32,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_silence_level(index as usize, silence_level, timeout)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_use_echo_suppression_json(
        &mut self,
        index: u32,
        use_echo: bool,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_use_echo_suppression(index as usize, use_echo)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_enhanced_options_json(
        &mut self,
        index: u32,
        options_json: &str,
    ) -> Result<String, JsValue> {
        let opts: Option<neoflash_core::as3_media_events::MicrophoneEnhancedOptionsState> =
            if options_json.trim().is_empty() || options_json.trim() == "null" {
                None
            } else {
                Some(
                    serde_json::from_str(options_json).map_err(|e| {
                        JsValue::from_str(&format!("Invalid enhanced options: {e}"))
                    })?,
                )
            };
        let state = self
            .inner
            .mic_set_enhanced_options(index as usize, opts)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn mic_set_sound_transform_json(
        &mut self,
        index: u32,
        volume: f64,
        pan: f64,
    ) -> Result<String, JsValue> {
        let state = self
            .inner
            .mic_set_sound_transform(index as usize, volume, pan)
            .map_err(media_events_js_err)?;
        media_events_json(&state)
    }

    pub fn validate_enhanced_mode(&self, mode: &str) -> Result<String, JsValue> {
        neoflash_core::as3_media_events::validate_enhanced_mode(mode)
            .map(String::from)
            .map_err(media_events_js_err)
    }

    pub fn validate_echo_path(&self, echo_path: i32) -> Result<i32, JsValue> {
        neoflash_core::as3_media_events::validate_echo_path(echo_path).map_err(media_events_js_err)
    }

    pub fn create_video_stream_settings_json(&mut self, kind: &str) -> Result<String, JsValue> {
        let (id, state) = self
            .inner
            .create_video_stream_settings(kind)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({ "id": id, "state": state }))
    }

    pub fn video_settings_set_mode_json(
        &mut self,
        id: u32,
        width: i32,
        height: i32,
        fps: f64,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_mode(width, height, fps)
            .map_err(media_events_js_err)?;
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn video_settings_set_quality_json(
        &mut self,
        id: u32,
        bandwidth: i32,
        quality: i32,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_quality(bandwidth, quality);
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn video_settings_set_key_frame_interval_json(
        &mut self,
        id: u32,
        interval: i32,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_key_frame_interval(interval);
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn video_settings_set_profile_level_json(
        &mut self,
        id: u32,
        profile: &str,
        level: &str,
    ) -> Result<String, JsValue> {
        let vs = self
            .inner
            .video_settings_mut(id as usize)
            .map_err(media_events_js_err)?;
        vs.set_profile_level(profile, level)
            .map_err(media_events_js_err)?;
        let cloned = vs.clone();
        media_events_json(&cloned)
    }

    pub fn stage_video_slot_count(&self) -> u32 {
        self.inner.stage_videos.len() as u32
    }

    pub fn stage_video_state_json(&self, slot: u32) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_get(slot as usize)
            .map_err(media_events_js_err)?;
        media_events_json(sv)
    }

    pub fn stage_video_set_viewport_json(
        &mut self,
        slot: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_viewport(slot as usize, x, y, w, h)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_pan_json(
        &mut self,
        slot: u32,
        x: f64,
        y: f64,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_pan(slot as usize, x, y)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_zoom_json(
        &mut self,
        slot: u32,
        x: f64,
        y: f64,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_zoom(slot as usize, x, y)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_depth_json(&mut self, slot: u32, depth: i32) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_depth(slot as usize, depth)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn stage_video_set_dimensions_json(
        &mut self,
        slot: u32,
        width: i32,
        height: i32,
    ) -> Result<String, JsValue> {
        let sv = self
            .inner
            .stage_video_set_dimensions(slot as usize, width, height)
            .map_err(media_events_js_err)?;
        media_events_json(&sv)
    }

    pub fn audio_device_names_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.audio_device_names)
    }

    pub fn selected_audio_device(&self) -> i32 {
        self.inner.selected_audio_device
    }

    pub fn select_audio_device(&mut self, index: i32) -> Result<bool, JsValue> {
        self.inner
            .select_audio_device(index)
            .map_err(media_events_js_err)
    }

    pub fn multitouch_input_mode(&self) -> String {
        self.inner.multitouch_input_mode.clone()
    }

    pub fn set_multitouch_input_mode(&mut self, mode: &str) -> Result<String, JsValue> {
        self.inner
            .set_multitouch_input_mode(mode)
            .map_err(media_events_js_err)
    }

    pub fn max_touch_points(&self) -> i32 {
        self.inner.max_touch_points
    }

    pub fn map_touch_to_mouse(&self) -> bool {
        self.inner.map_touch_to_mouse
    }

    pub fn set_map_touch_to_mouse(&mut self, value: bool) {
        self.inner.map_touch_to_mouse = value;
    }

    pub fn supported_gestures_json(&self) -> Result<String, JsValue> {
        media_events_json(&self.inner.supported_gestures)
    }

    pub fn game_device_count(&self) -> u32 {
        self.inner.game_devices.len() as u32
    }

    pub fn game_device_state_json(&self, index: i32) -> Result<String, JsValue> {
        let dev = self
            .inner
            .game_device_get(index)
            .map_err(media_events_js_err)?;
        media_events_json(dev)
    }

    pub fn game_device_set_enabled(&mut self, index: i32, enabled: bool) -> Result<(), JsValue> {
        self.inner
            .game_device_set_enabled(index, enabled)
            .map_err(media_events_js_err)
    }

    pub fn game_device_set_sample_interval(
        &mut self,
        index: i32,
        interval: i32,
    ) -> Result<(), JsValue> {
        self.inner
            .game_device_set_sample_interval(index, interval)
            .map_err(media_events_js_err)
    }

    pub fn game_device_start_caching(
        &mut self,
        index: i32,
        num_samples: i32,
        control_ids_json: &str,
    ) -> Result<(), JsValue> {
        let control_ids: Vec<String> = serde_json::from_str(control_ids_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid control IDs JSON: {e}")))?;
        self.inner
            .game_device_start_caching(index, num_samples, control_ids)
            .map_err(media_events_js_err)
    }

    pub fn game_device_stop_caching(&mut self, index: i32) -> Result<(), JsValue> {
        self.inner
            .game_device_stop_caching(index)
            .map_err(media_events_js_err)
    }

    pub fn game_device_drain_samples_json(&mut self, index: i32) -> Result<String, JsValue> {
        let (rows, bytes) = self
            .inner
            .game_device_drain_samples(index)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({ "rows": rows, "bytes": bytes }))
    }

    pub fn mic_sample_pcm_json(
        &mut self,
        index: u32,
        sample_count: u32,
    ) -> Result<String, JsValue> {
        let (position, bytes) = self
            .inner
            .mic_sample_pcm_bytes(index as usize, sample_count as usize)
            .map_err(media_events_js_err)?;
        let state = self
            .inner
            .mic_get(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({
            "position": position,
            "bytes": bytes,
            "activity_level": state.activity_level,
            "sample_position": state.sample_position,
        }))
    }

    pub fn sync_hardware_devices_json(
        &mut self,
        cameras_json: &str,
        microphones_json: &str,
        audio_outputs_json: &str,
    ) -> Result<String, JsValue> {
        let cameras: Vec<String> = serde_json::from_str(cameras_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid cameras JSON: {e}")))?;
        let microphones: Vec<String> = serde_json::from_str(microphones_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid microphones JSON: {e}")))?;
        let audio_outputs: Vec<String> = serde_json::from_str(audio_outputs_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid audio_outputs JSON: {e}")))?;
        self.inner
            .sync_hardware_devices(cameras, microphones, audio_outputs);
        media_events_json(&serde_json::json!({
            "cameras": self.inner.camera_names(),
            "microphones": self.inner.microphone_names(),
            "audio_outputs": self.inner.audio_device_names,
        }))
    }

    pub fn game_device_sync_json(
        &mut self,
        index: u32,
        id: &str,
        name: &str,
        axes_json: &str,
        buttons_json: &str,
    ) -> Result<String, JsValue> {
        let axes: Vec<f64> = serde_json::from_str(axes_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid axes JSON: {e}")))?;
        let buttons: Vec<f64> = serde_json::from_str(buttons_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid buttons JSON: {e}")))?;
        let (is_new, changed_controls) = self
            .inner
            .game_device_sync(index as usize, id, name, &axes, &buttons)
            .map_err(media_events_js_err)?;
        let device = self
            .inner
            .game_device_get(index as i32)
            .map_err(media_events_js_err)?;
        media_events_json(&serde_json::json!({
            "is_new": is_new,
            "changed_controls": changed_controls,
            "device": device,
        }))
    }

    pub fn game_device_remove_json(&mut self, index: u32) -> Result<String, JsValue> {
        let removed = self
            .inner
            .game_device_remove(index as usize)
            .map_err(media_events_js_err)?;
        media_events_json(&removed)
    }

    pub fn validate_acc_string(&self, value: &str, field: &str) -> Result<String, JsValue> {
        neoflash_core::as3_media_events::validate_acc_string(value, field)
            .map_err(media_events_js_err)
    }

    pub fn record_accessibility_update(&mut self) -> u32 {
        self.inner.accessibility_updates = self.inner.accessibility_updates.saturating_add(1);
        self.inner.accessibility_updates
    }

    pub fn msaa_role_to_aria(&self, role: u32) -> String {
        neoflash_core::accessibility::msaa_role_to_aria(role).to_owned()
    }

    pub fn decode_msaa_state_json(&self, state: u32, aria_role: &str) -> Result<String, JsValue> {
        media_events_json(&neoflash_core::accessibility::decode_msaa_state(
            state, aria_role,
        ))
    }

    pub fn validate_context_menu_insert_index(
        &self,
        index: i32,
        num_items: u32,
    ) -> Result<u32, JsValue> {
        neoflash_core::as3_media_events::validate_context_menu_insert_index(
            index,
            num_items as usize,
        )
        .map(|idx| idx as u32)
        .map_err(media_events_js_err)
    }

    pub fn validate_context_menu_item_index(
        &self,
        index: i32,
        num_items: u32,
    ) -> Result<u32, JsValue> {
        neoflash_core::as3_media_events::validate_context_menu_item_index(index, num_items as usize)
            .map(|idx| idx as u32)
            .map_err(media_events_js_err)
    }
}
