//! Shared Rust state, validation, and pixel/sample synthesis for AS3 Area 8
//! (`flash.media.*`, `flash.ui.*`, `flash.accessibility.*`, and media/DRM/AV events).
//! Browser and Node hosts marshal values and connect DOM/WebAudio/Gamepad facilities
//! without duplicating validation, codec rules, or frame synthesis.

use serde::{Deserialize, Serialize};

pub const MAX_DEVICES: usize = 16;
pub const STAGE_VIDEO_SLOTS: usize = 4;
pub const GAME_INPUT_MAX_BUFFER_SIZE: usize = 32_000;
const MAX_CAMERA_DIMENSION: i32 = 4_096;
const MAX_CAPTURE_PIXELS: usize = 4_096 * 4_096;
const MAX_ACC_TEXT_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaEventsErrorKind {
    ArgumentError,
    RangeError,
    IllegalOperationError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEventsError {
    pub kind: MediaEventsErrorKind,
    pub code: u32,
    pub message: String,
}

impl MediaEventsError {
    pub fn argument(code: u32, message: impl Into<String>) -> Self {
        Self {
            kind: MediaEventsErrorKind::ArgumentError,
            code,
            message: message.into(),
        }
    }

    pub fn range(code: u32, message: impl Into<String>) -> Self {
        Self {
            kind: MediaEventsErrorKind::RangeError,
            code,
            message: message.into(),
        }
    }

    pub fn illegal(code: u32, message: impl Into<String>) -> Self {
        Self {
            kind: MediaEventsErrorKind::IllegalOperationError,
            code,
            message: message.into(),
        }
    }

    pub fn to_wire_string(&self) -> String {
        let tag = match self.kind {
            MediaEventsErrorKind::ArgumentError => "ArgumentError",
            MediaEventsErrorKind::RangeError => "RangeError",
            MediaEventsErrorKind::IllegalOperationError => "IllegalOperationError",
        };
        format!("{tag}:{}:{}", self.code, self.message)
    }
}

impl std::fmt::Display for MediaEventsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_wire_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraState {
    pub index: usize,
    pub name: String,
    pub position: String,
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub current_fps: f64,
    pub bandwidth: i32,
    pub quality: i32,
    pub key_frame_interval: i32,
    pub loopback: bool,
    pub motion_level: i32,
    pub motion_timeout: i32,
    pub muted: bool,
    pub activity_level: f64,
    pub cursor: bool,
    pub permission_status: String,
    pub frame_revision: u32,
}

impl CameraState {
    pub fn new(index: usize, name: impl Into<String>, position: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            position: position.into(),
            width: 160,
            height: 120,
            fps: 15.0,
            current_fps: 15.0,
            bandwidth: 16_384,
            quality: 0,
            key_frame_interval: 15,
            loopback: false,
            motion_level: 50,
            motion_timeout: 2_000,
            muted: false,
            activity_level: 0.0,
            cursor: false,
            permission_status: "granted".into(),
            frame_revision: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MicrophoneEnhancedOptionsState {
    pub mode: String,
    pub echo_path: i32,
    pub non_linear_processing: bool,
    pub is_voice_detected: i32,
    pub auto_gain: bool,
}

impl Default for MicrophoneEnhancedOptionsState {
    fn default() -> Self {
        Self {
            mode: "fullDuplex".into(),
            echo_path: 128,
            non_linear_processing: true,
            is_voice_detected: -1,
            auto_gain: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrophoneState {
    pub index: usize,
    pub name: String,
    pub activity_level: f64,
    pub codec: String,
    pub enable_vad: bool,
    pub encode_quality: i32,
    pub enhanced_options: Option<MicrophoneEnhancedOptionsState>,
    pub frames_per_packet: i32,
    pub gain: f64,
    pub loopback: bool,
    pub muted: bool,
    pub noise_suppression_level: i32,
    pub rate: i32,
    pub silence_level: f64,
    pub silence_timeout: i32,
    pub use_echo_suppression: bool,
    pub volume: f64,
    pub pan: f64,
    pub permission_status: String,
}

impl MicrophoneState {
    pub fn new(index: usize, name: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            activity_level: 0.0,
            codec: "NellyMoser".into(),
            enable_vad: false,
            encode_quality: 6,
            enhanced_options: None,
            frames_per_packet: 2,
            gain: 50.0,
            loopback: false,
            muted: false,
            noise_suppression_level: -30,
            rate: 8,
            silence_level: 10.0,
            silence_timeout: 2_000,
            use_echo_suppression: false,
            volume: 1.0,
            pan: 0.0,
            permission_status: "granted".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStreamSettingsState {
    pub codec: String,
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub bandwidth: i32,
    pub quality: i32,
    pub key_frame_interval: i32,
    pub profile: Option<String>,
    pub level: Option<String>,
}

impl VideoStreamSettingsState {
    pub fn sorenson() -> Self {
        Self {
            codec: "Sorenson".into(),
            width: -1,
            height: -1,
            fps: -1.0,
            bandwidth: -1,
            quality: -1,
            key_frame_interval: -1,
            profile: None,
            level: None,
        }
    }

    pub fn h264() -> Self {
        Self {
            codec: "H264Avc".into(),
            width: -1,
            height: -1,
            fps: -1.0,
            bandwidth: -1,
            quality: -1,
            key_frame_interval: -1,
            profile: Some("baseline".into()),
            level: Some("2.1".into()),
        }
    }

    pub fn vp6() -> Self {
        Self {
            codec: "VP6".into(),
            width: -1,
            height: -1,
            fps: -1.0,
            bandwidth: -1,
            quality: -1,
            key_frame_interval: -1,
            profile: None,
            level: None,
        }
    }

    pub fn set_mode(&mut self, width: i32, height: i32, fps: f64) -> Result<(), MediaEventsError> {
        if !fps.is_finite() {
            return Err(MediaEventsError::argument(
                2004,
                "VideoStreamSettings.setMode requires a finite fps.",
            ));
        }
        self.width = if width < 0 { -1 } else { width.clamp(1, 8_192) };
        self.height = if height < 0 {
            -1
        } else {
            height.clamp(1, 8_192)
        };
        self.fps = if fps < 0.0 {
            -1.0
        } else {
            fps.clamp(0.0, 120.0)
        };
        Ok(())
    }

    pub fn set_quality(&mut self, bandwidth: i32, quality: i32) {
        self.bandwidth = if bandwidth < 0 { 0 } else { bandwidth };
        self.quality = if quality < 0 {
            0
        } else {
            quality.clamp(0, 100)
        };
    }

    pub fn set_key_frame_interval(&mut self, interval: i32) {
        self.key_frame_interval = if interval < 0 {
            -1
        } else {
            interval.clamp(1, 300)
        };
    }

    pub fn set_profile_level(
        &mut self,
        profile: &str,
        level: &str,
    ) -> Result<(), MediaEventsError> {
        let (p, l) = validate_h264_profile_level(profile, level)?;
        self.profile = Some(p.into());
        self.level = Some(l.into());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageVideoSlot {
    pub index: usize,
    pub view_port: [f64; 4],
    pub pan: [f64; 2],
    pub zoom: [f64; 2],
    pub depth: i32,
    pub video_width: i32,
    pub video_height: i32,
    pub color_spaces: Vec<String>,
}

impl StageVideoSlot {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            view_port: [0.0, 0.0, 0.0, 0.0],
            pan: [0.0, 0.0],
            zoom: [1.0, 1.0],
            depth: 0,
            video_width: 0,
            video_height: 0,
            color_spaces: vec!["BT.601".into(), "BT.709".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInputControlState {
    pub id: String,
    pub min_value: f64,
    pub max_value: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInputDeviceState {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub sample_interval: i32,
    pub controls: Vec<GameInputControlState>,
    #[serde(skip)]
    pub caching_num_samples: usize,
    #[serde(skip)]
    pub caching_controls: Vec<String>,
    #[serde(skip)]
    pub cached_rows: Vec<Vec<f32>>,
}

impl GameInputDeviceState {
    pub fn default_virtual_pad() -> Self {
        Self {
            id: "neoflash-gamepad-0".into(),
            name: "Standard Gamepad".into(),
            enabled: false,
            sample_interval: 16,
            controls: vec![
                GameInputControlState {
                    id: "AXIS_0".into(),
                    min_value: -1.0,
                    max_value: 1.0,
                    value: 0.0,
                },
                GameInputControlState {
                    id: "AXIS_1".into(),
                    min_value: -1.0,
                    max_value: 1.0,
                    value: 0.0,
                },
                GameInputControlState {
                    id: "BUTTON_0".into(),
                    min_value: 0.0,
                    max_value: 1.0,
                    value: 0.0,
                },
                GameInputControlState {
                    id: "BUTTON_1".into(),
                    min_value: 0.0,
                    max_value: 1.0,
                    value: 0.0,
                },
            ],
            caching_num_samples: 0,
            caching_controls: Vec::new(),
            cached_rows: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextMenuBuiltInFlags {
    pub forward_and_back: bool,
    pub loop_item: bool,
    pub play: bool,
    pub print: bool,
    pub quality: bool,
    pub rewind: bool,
    pub save: bool,
    pub zoom: bool,
}

impl Default for ContextMenuBuiltInFlags {
    fn default() -> Self {
        Self {
            forward_and_back: true,
            loop_item: true,
            play: true,
            print: true,
            quality: true,
            rewind: true,
            save: true,
            zoom: true,
        }
    }
}

impl ContextMenuBuiltInFlags {
    pub fn hide_all(&mut self) {
        self.forward_and_back = false;
        self.loop_item = false;
        self.play = false;
        self.print = false;
        self.quality = false;
        self.rewind = false;
        self.save = false;
        self.zoom = false;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextMenuClipboardFlags {
    pub clear: bool,
    pub copy: bool,
    pub cut: bool,
    pub paste: bool,
    pub select_all: bool,
}

impl Default for ContextMenuClipboardFlags {
    fn default() -> Self {
        Self {
            clear: false,
            copy: false,
            cut: false,
            paste: false,
            select_all: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AccessibilityPropertiesState {
    pub name: String,
    pub description: String,
    pub shortcut: String,
    pub silent: bool,
    pub force_simple: bool,
    pub no_auto_labeling: bool,
}

pub fn validate_acc_string(value: &str, field: &str) -> Result<String, MediaEventsError> {
    if value.len() > MAX_ACC_TEXT_BYTES {
        return Err(MediaEventsError::range(
            2006,
            format!("AccessibilityProperties.{field} exceeds 64 KiB."),
        ));
    }
    Ok(value.to_owned())
}

pub fn snap_microphone_rate(rate: i32) -> i32 {
    if rate <= 6 {
        5
    } else if rate <= 9 {
        8
    } else if rate <= 13 {
        11
    } else if rate <= 19 {
        16
    } else if rate <= 33 {
        22
    } else {
        44
    }
}

pub fn validate_sound_codec(codec: &str) -> Result<&'static str, MediaEventsError> {
    match codec {
        "NellyMoser" => Ok("NellyMoser"),
        "Speex" => Ok("Speex"),
        "pcma" => Ok("pcma"),
        "pcmu" => Ok("pcmu"),
        _ => Err(MediaEventsError::argument(
            2008,
            format!(
                "Parameter codec must be one of the accepted SoundCodec values; got '{codec}'."
            ),
        )),
    }
}

pub fn validate_enhanced_mode(mode: &str) -> Result<&'static str, MediaEventsError> {
    match mode {
        "fullDuplex" => Ok("fullDuplex"),
        "halfDuplex" => Ok("halfDuplex"),
        "headset" => Ok("headset"),
        "off" => Ok("off"),
        "speakerMute" => Ok("speakerMute"),
        _ => Err(MediaEventsError::argument(
            2008,
            format!(
                "Parameter mode must be one of the accepted MicrophoneEnhancedMode values; got '{mode}'."
            ),
        )),
    }
}

pub fn validate_echo_path(echo_path: i32) -> Result<i32, MediaEventsError> {
    match echo_path {
        128 | 256 => Ok(echo_path),
        _ => Err(MediaEventsError::argument(
            2008,
            format!("MicrophoneEnhancedOptions.echoPath must be 128 or 256; got {echo_path}."),
        )),
    }
}

pub fn validate_h264_profile_level<'a>(
    profile: &'a str,
    level: &'a str,
) -> Result<(&'static str, &'static str), MediaEventsError> {
    let p = match profile {
        "baseline" => "baseline",
        "main" => "main",
        _ => {
            return Err(MediaEventsError::argument(
                2008,
                format!("Parameter profile must be 'baseline' or 'main'; got '{profile}'."),
            ));
        }
    };
    let l = match level {
        "1" => "1",
        "1b" => "1b",
        "1.1" => "1.1",
        "1.2" => "1.2",
        "1.3" => "1.3",
        "2" => "2",
        "2.1" => "2.1",
        "2.2" => "2.2",
        "3" => "3",
        "3.1" => "3.1",
        "3.2" => "3.2",
        "4" => "4",
        "4.1" => "4.1",
        "4.2" => "4.2",
        "5" => "5",
        "5.1" => "5.1",
        _ => {
            return Err(MediaEventsError::argument(
                2008,
                format!("Parameter level must be a valid H264Level constant; got '{level}'."),
            ));
        }
    };
    Ok((p, l))
}

pub fn validate_multitouch_input_mode(mode: &str) -> Result<&'static str, MediaEventsError> {
    match mode {
        "none" => Ok("none"),
        "gesture" => Ok("gesture"),
        "touchPoint" => Ok("touchPoint"),
        _ => Err(MediaEventsError::argument(
            2008,
            format!(
                "Multitouch.inputMode must be 'none', 'gesture', or 'touchPoint'; got '{mode}'."
            ),
        )),
    }
}

pub struct As3MediaEventsRuntime {
    pub cameras: Vec<CameraState>,
    pub microphones: Vec<MicrophoneState>,
    pub video_settings: Vec<VideoStreamSettingsState>,
    pub stage_videos: Vec<StageVideoSlot>,
    pub audio_device_names: Vec<String>,
    pub selected_audio_device: i32,
    pub multitouch_input_mode: String,
    pub max_touch_points: i32,
    pub map_touch_to_mouse: bool,
    pub supported_gestures: Vec<String>,
    pub game_devices: Vec<GameInputDeviceState>,
    pub accessibility_active: bool,
    pub accessibility_updates: u32,
}

impl Default for As3MediaEventsRuntime {
    fn default() -> Self {
        Self::new(5)
    }
}

impl As3MediaEventsRuntime {
    pub fn new(max_touch_points: i32) -> Self {
        let stage_videos = (0..STAGE_VIDEO_SLOTS).map(StageVideoSlot::new).collect();
        Self {
            cameras: vec![CameraState::new(0, "Default Camera", "front")],
            microphones: vec![MicrophoneState::new(0, "Default Microphone")],
            video_settings: Vec::new(),
            stage_videos,
            audio_device_names: vec!["Default Audio Output".into(), "Headphones".into()],
            selected_audio_device: 0,
            multitouch_input_mode: "none".into(),
            max_touch_points: max_touch_points.clamp(0, 32),
            map_touch_to_mouse: true,
            supported_gestures: vec![
                "gesturePan".into(),
                "gestureRotate".into(),
                "gestureSwipe".into(),
                "gestureTwoFingerTap".into(),
                "gestureZoom".into(),
                "gesturePressAndTap".into(),
            ],
            game_devices: vec![GameInputDeviceState::default_virtual_pad()],
            accessibility_active: false,
            accessibility_updates: 0,
        }
    }

    pub fn reset(&mut self) {
        let touch_points = self.max_touch_points;
        *self = Self::new(touch_points);
    }

    // ---- Camera ----

    pub fn camera_names(&self) -> Vec<String> {
        self.cameras.iter().map(|c| c.name.clone()).collect()
    }

    pub fn resolve_camera(&self, name: Option<&str>) -> Option<usize> {
        if self.cameras.is_empty() {
            return None;
        }
        match name {
            None => Some(0),
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Some(0);
                }
                if let Ok(idx) = trimmed.parse::<usize>() {
                    if idx < self.cameras.len() {
                        return Some(idx);
                    }
                    return None;
                }
                self.cameras.iter().position(|c| c.name == raw)
            }
        }
    }

    fn camera_mut(&mut self, index: usize) -> Result<&mut CameraState, MediaEventsError> {
        self.cameras.get_mut(index).ok_or_else(|| {
            MediaEventsError::range(2006, format!("Camera index {index} is out of bounds."))
        })
    }

    pub fn camera_get(&self, index: usize) -> Result<&CameraState, MediaEventsError> {
        self.cameras.get(index).ok_or_else(|| {
            MediaEventsError::range(2006, format!("Camera index {index} is out of bounds."))
        })
    }

    pub fn camera_set_mode(
        &mut self,
        index: usize,
        width: i32,
        height: i32,
        fps: f64,
        _favor_area: bool,
    ) -> Result<CameraState, MediaEventsError> {
        if !fps.is_finite() {
            return Err(MediaEventsError::argument(
                2004,
                "Camera.setMode requires a finite fps.",
            ));
        }
        let cam = self.camera_mut(index)?;
        cam.width = width.clamp(1, MAX_CAMERA_DIMENSION);
        cam.height = height.clamp(1, MAX_CAMERA_DIMENSION);
        cam.fps = fps.clamp(0.0, 120.0);
        cam.current_fps = if cam.muted { 0.0 } else { cam.fps };
        cam.frame_revision = cam.frame_revision.wrapping_add(1).max(1);
        Ok(cam.clone())
    }

    pub fn camera_set_quality(
        &mut self,
        index: usize,
        bandwidth: i32,
        quality: i32,
    ) -> Result<CameraState, MediaEventsError> {
        let cam = self.camera_mut(index)?;
        cam.bandwidth = bandwidth.max(0);
        cam.quality = quality.clamp(0, 100);
        Ok(cam.clone())
    }

    pub fn camera_set_key_frame_interval(
        &mut self,
        index: usize,
        interval: i32,
    ) -> Result<CameraState, MediaEventsError> {
        let cam = self.camera_mut(index)?;
        cam.key_frame_interval = interval.clamp(1, 48);
        Ok(cam.clone())
    }

    pub fn camera_set_loopback(
        &mut self,
        index: usize,
        compress: bool,
    ) -> Result<CameraState, MediaEventsError> {
        let cam = self.camera_mut(index)?;
        cam.loopback = compress;
        cam.frame_revision = cam.frame_revision.wrapping_add(1).max(1);
        Ok(cam.clone())
    }

    pub fn camera_set_motion_level(
        &mut self,
        index: usize,
        motion_level: i32,
        timeout: i32,
    ) -> Result<CameraState, MediaEventsError> {
        let cam = self.camera_mut(index)?;
        cam.motion_level = motion_level.clamp(0, 100);
        cam.motion_timeout = timeout.max(0);
        Ok(cam.clone())
    }

    pub fn camera_set_cursor(
        &mut self,
        index: usize,
        cursor: bool,
    ) -> Result<CameraState, MediaEventsError> {
        let cam = self.camera_mut(index)?;
        cam.cursor = cursor;
        cam.frame_revision = cam.frame_revision.wrapping_add(1).max(1);
        Ok(cam.clone())
    }

    /// Synthesizes deterministic ARGB32 (`0xAARRGGBB`) camera frame pixels for `(x, y, w, h)`.
    pub fn camera_frame_argb_u32(
        &self,
        index: usize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Vec<u32>, MediaEventsError> {
        let cam = self.camera_get(index)?;
        if x < 0 || y < 0 || w < 0 || h < 0 || x + w > cam.width || y + h > cam.height {
            return Err(MediaEventsError::range(
                2006,
                format!(
                    "Camera capture rectangle ({x},{y},{w},{h}) exceeds camera dimensions {}x{}.",
                    cam.width, cam.height
                ),
            ));
        }
        let count = (w as usize)
            .checked_mul(h as usize)
            .filter(|&n| n <= MAX_CAPTURE_PIXELS)
            .ok_or_else(|| {
                MediaEventsError::range(2006, "Camera capture rectangle exceeds pixel budget.")
            })?;
        let mut out = Vec::with_capacity(count);
        let cw = cam.width.max(1) as u32;
        let ch = cam.height.max(1) as u32;
        for row in 0..h {
            let py = (y + row) as u32;
            for col in 0..w {
                let px = (x + col) as u32;
                let r = ((px * 255) / cw) & 0xFF;
                let g = ((py * 255) / ch) & 0xFF;
                let mut b = (((px + py + cam.frame_revision) * 37) % 256) & 0xFF;
                if cam.cursor && px < 8 && py < 8 {
                    b = 0xFF;
                }
                out.push(0xFF00_0000 | (r << 16) | (g << 8) | b);
            }
        }
        Ok(out)
    }

    pub fn camera_frame_argb_bytes(
        &self,
        index: usize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<Vec<u8>, MediaEventsError> {
        let pixels = self.camera_frame_argb_u32(index, x, y, w, h)?;
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for px in pixels {
            bytes.extend_from_slice(&px.to_be_bytes());
        }
        Ok(bytes)
    }

    // ---- Microphone ----

    pub fn microphone_names(&self) -> Vec<String> {
        self.microphones.iter().map(|m| m.name.clone()).collect()
    }

    pub fn resolve_microphone(&self, index: i32) -> Option<usize> {
        if self.microphones.is_empty() {
            return None;
        }
        if index == -1 {
            return Some(0);
        }
        if index >= 0 && (index as usize) < self.microphones.len() {
            return Some(index as usize);
        }
        None
    }

    fn mic_mut(&mut self, index: usize) -> Result<&mut MicrophoneState, MediaEventsError> {
        self.microphones.get_mut(index).ok_or_else(|| {
            MediaEventsError::range(2006, format!("Microphone index {index} is out of bounds."))
        })
    }

    pub fn mic_get(&self, index: usize) -> Result<&MicrophoneState, MediaEventsError> {
        self.microphones.get(index).ok_or_else(|| {
            MediaEventsError::range(2006, format!("Microphone index {index} is out of bounds."))
        })
    }

    pub fn mic_enable_enhanced(
        &mut self,
        index: usize,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        if mic.enhanced_options.is_none() {
            mic.enhanced_options = Some(MicrophoneEnhancedOptionsState::default());
        }
        Ok(mic.clone())
    }

    pub fn mic_set_codec(
        &mut self,
        index: usize,
        codec: &str,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let valid = validate_sound_codec(codec)?;
        let mic = self.mic_mut(index)?;
        mic.codec = valid.into();
        Ok(mic.clone())
    }

    pub fn mic_set_rate(
        &mut self,
        index: usize,
        rate: i32,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.rate = snap_microphone_rate(rate);
        Ok(mic.clone())
    }

    pub fn mic_set_gain(
        &mut self,
        index: usize,
        gain: f64,
    ) -> Result<MicrophoneState, MediaEventsError> {
        if !gain.is_finite() {
            return Err(MediaEventsError::argument(
                2004,
                "Microphone.gain requires a finite Number.",
            ));
        }
        let mic = self.mic_mut(index)?;
        mic.gain = gain.clamp(0.0, 100.0);
        Ok(mic.clone())
    }

    pub fn mic_set_encode_quality(
        &mut self,
        index: usize,
        quality: i32,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.encode_quality = quality.clamp(0, 10);
        Ok(mic.clone())
    }

    pub fn mic_set_frames_per_packet(
        &mut self,
        index: usize,
        frames: i32,
    ) -> Result<MicrophoneState, MediaEventsError> {
        if frames < 1 {
            return Err(MediaEventsError::argument(
                2004,
                "Microphone.framesPerPacket must be >= 1.",
            ));
        }
        let mic = self.mic_mut(index)?;
        mic.frames_per_packet = frames.clamp(1, 10);
        Ok(mic.clone())
    }

    pub fn mic_set_noise_suppression_level(
        &mut self,
        index: usize,
        level: i32,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.noise_suppression_level = level.clamp(-30, 0);
        Ok(mic.clone())
    }

    pub fn mic_set_enable_vad(
        &mut self,
        index: usize,
        enable: bool,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.enable_vad = enable;
        Ok(mic.clone())
    }

    pub fn mic_set_loop_back(
        &mut self,
        index: usize,
        state: bool,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.loopback = state;
        Ok(mic.clone())
    }

    pub fn mic_set_silence_level(
        &mut self,
        index: usize,
        silence_level: f64,
        timeout: i32,
    ) -> Result<MicrophoneState, MediaEventsError> {
        if !silence_level.is_finite() {
            return Err(MediaEventsError::argument(
                2004,
                "Microphone.setSilenceLevel requires a finite Number.",
            ));
        }
        let mic = self.mic_mut(index)?;
        mic.silence_level = silence_level.clamp(0.0, 100.0);
        if timeout >= 0 {
            mic.silence_timeout = timeout;
        }
        Ok(mic.clone())
    }

    pub fn mic_set_use_echo_suppression(
        &mut self,
        index: usize,
        use_echo: bool,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.use_echo_suppression = use_echo;
        Ok(mic.clone())
    }

    pub fn mic_set_enhanced_options(
        &mut self,
        index: usize,
        options: Option<MicrophoneEnhancedOptionsState>,
    ) -> Result<MicrophoneState, MediaEventsError> {
        if let Some(ref opts) = options {
            validate_enhanced_mode(&opts.mode)?;
            validate_echo_path(opts.echo_path)?;
        }
        let mic = self.mic_mut(index)?;
        mic.enhanced_options = options;
        Ok(mic.clone())
    }

    pub fn mic_set_sound_transform(
        &mut self,
        index: usize,
        volume: f64,
        pan: f64,
    ) -> Result<MicrophoneState, MediaEventsError> {
        let mic = self.mic_mut(index)?;
        mic.volume = if volume.is_finite() { volume } else { 1.0 };
        mic.pan = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        Ok(mic.clone())
    }

    // ---- VideoStreamSettings ----

    pub fn create_video_stream_settings(
        &mut self,
        kind: &str,
    ) -> Result<(usize, VideoStreamSettingsState), MediaEventsError> {
        let state = match kind {
            "Sorenson" => VideoStreamSettingsState::sorenson(),
            "H264Avc" => VideoStreamSettingsState::h264(),
            "VP6" => VideoStreamSettingsState::vp6(),
            _ => {
                return Err(MediaEventsError::argument(
                    2008,
                    format!("Unknown VideoStreamSettings codec kind '{kind}'."),
                ));
            }
        };
        let id = self.video_settings.len();
        self.video_settings.push(state.clone());
        Ok((id, state))
    }

    pub fn video_settings_mut(
        &mut self,
        id: usize,
    ) -> Result<&mut VideoStreamSettingsState, MediaEventsError> {
        self.video_settings.get_mut(id).ok_or_else(|| {
            MediaEventsError::range(2006, format!("Invalid VideoStreamSettings handle {id}."))
        })
    }

    // ---- StageVideo ----

    fn stage_video_mut(&mut self, slot: usize) -> Result<&mut StageVideoSlot, MediaEventsError> {
        self.stage_videos.get_mut(slot).ok_or_else(|| {
            MediaEventsError::range(2006, format!("StageVideo slot {slot} is out of bounds."))
        })
    }

    pub fn stage_video_get(&self, slot: usize) -> Result<&StageVideoSlot, MediaEventsError> {
        self.stage_videos.get(slot).ok_or_else(|| {
            MediaEventsError::range(2006, format!("StageVideo slot {slot} is out of bounds."))
        })
    }

    pub fn stage_video_set_viewport(
        &mut self,
        slot: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<StageVideoSlot, MediaEventsError> {
        if !x.is_finite()
            || !y.is_finite()
            || !w.is_finite()
            || !h.is_finite()
            || w < 0.0
            || h < 0.0
            || x.abs() > 32_768.0
            || y.abs() > 32_768.0
            || w > 32_768.0
            || h > 32_768.0
        {
            return Err(MediaEventsError::range(
                2006,
                "StageVideo.viewPort coordinates are out of range.",
            ));
        }
        let sv = self.stage_video_mut(slot)?;
        sv.view_port = [x, y, w, h];
        Ok(sv.clone())
    }

    pub fn stage_video_set_pan(
        &mut self,
        slot: usize,
        x: f64,
        y: f64,
    ) -> Result<StageVideoSlot, MediaEventsError> {
        if !x.is_finite()
            || !y.is_finite()
            || !(-1.0..=1.0).contains(&x)
            || !(-1.0..=1.0).contains(&y)
        {
            return Err(MediaEventsError::range(
                2006,
                "StageVideo.pan values must be within [-1.0, 1.0].",
            ));
        }
        let sv = self.stage_video_mut(slot)?;
        sv.pan = [x, y];
        Ok(sv.clone())
    }

    pub fn stage_video_set_zoom(
        &mut self,
        slot: usize,
        x: f64,
        y: f64,
    ) -> Result<StageVideoSlot, MediaEventsError> {
        if !x.is_finite()
            || !y.is_finite()
            || !(1.0..=16.0).contains(&x)
            || !(1.0..=16.0).contains(&y)
        {
            return Err(MediaEventsError::range(
                2006,
                "StageVideo.zoom values must be within [1.0, 16.0].",
            ));
        }
        let sv = self.stage_video_mut(slot)?;
        sv.zoom = [x, y];
        Ok(sv.clone())
    }

    pub fn stage_video_set_depth(
        &mut self,
        slot: usize,
        depth: i32,
    ) -> Result<StageVideoSlot, MediaEventsError> {
        let sv = self.stage_video_mut(slot)?;
        sv.depth = depth;
        Ok(sv.clone())
    }

    pub fn stage_video_set_dimensions(
        &mut self,
        slot: usize,
        width: i32,
        height: i32,
    ) -> Result<StageVideoSlot, MediaEventsError> {
        let sv = self.stage_video_mut(slot)?;
        sv.video_width = width.max(0);
        sv.video_height = height.max(0);
        Ok(sv.clone())
    }

    // ---- AudioDeviceManager ----

    pub fn select_audio_device(&mut self, index: i32) -> Result<bool, MediaEventsError> {
        if index < 0 || (index as usize) >= self.audio_device_names.len() {
            return Err(MediaEventsError::range(
                2006,
                format!("AudioDeviceManager.selectedDeviceIndex {index} is out of bounds."),
            ));
        }
        let changed = self.selected_audio_device != index;
        self.selected_audio_device = index;
        Ok(changed)
    }

    // ---- Multitouch ----

    pub fn set_multitouch_input_mode(&mut self, mode: &str) -> Result<String, MediaEventsError> {
        let valid = validate_multitouch_input_mode(mode)?;
        self.multitouch_input_mode = valid.into();
        Ok(self.multitouch_input_mode.clone())
    }

    // ---- GameInput ----

    pub fn game_device_get(&self, index: i32) -> Result<&GameInputDeviceState, MediaEventsError> {
        if index < 0 || (index as usize) >= self.game_devices.len() {
            return Err(MediaEventsError::range(
                2006,
                format!("GameInput device index {index} is out of bounds."),
            ));
        }
        Ok(&self.game_devices[index as usize])
    }

    fn game_device_mut(
        &mut self,
        index: i32,
    ) -> Result<&mut GameInputDeviceState, MediaEventsError> {
        if index < 0 || (index as usize) >= self.game_devices.len() {
            return Err(MediaEventsError::range(
                2006,
                format!("GameInput device index {index} is out of bounds."),
            ));
        }
        Ok(&mut self.game_devices[index as usize])
    }

    pub fn game_device_set_enabled(
        &mut self,
        index: i32,
        enabled: bool,
    ) -> Result<(), MediaEventsError> {
        let dev = self.game_device_mut(index)?;
        dev.enabled = enabled;
        if !enabled {
            dev.caching_num_samples = 0;
            dev.caching_controls.clear();
            dev.cached_rows.clear();
        }
        Ok(())
    }

    pub fn game_device_set_sample_interval(
        &mut self,
        index: i32,
        interval: i32,
    ) -> Result<(), MediaEventsError> {
        if interval < 0 || interval > GAME_INPUT_MAX_BUFFER_SIZE as i32 {
            return Err(MediaEventsError::argument(
                2004,
                format!("GameInputDevice.sampleInterval {interval} is out of range."),
            ));
        }
        let dev = self.game_device_mut(index)?;
        dev.sample_interval = interval;
        Ok(())
    }

    pub fn game_device_start_caching(
        &mut self,
        index: i32,
        num_samples: i32,
        control_ids: Vec<String>,
    ) -> Result<(), MediaEventsError> {
        if num_samples <= 0 || control_ids.is_empty() {
            return Err(MediaEventsError::argument(
                2004,
                "GameInputDevice.startCachingSamples requires numSamples > 0 and non-empty controls.",
            ));
        }
        let bytes_needed = (num_samples as usize)
            .saturating_mul(control_ids.len())
            .saturating_mul(4);
        if bytes_needed > GAME_INPUT_MAX_BUFFER_SIZE {
            return Err(MediaEventsError::IllegalOperationError(
                2004,
                "Requested GameInput sample cache exceeds MAX_BUFFER_SIZE.",
            ));
        }
        let dev = self.game_device_mut(index)?;
        for cid in &control_ids {
            if !dev.controls.iter().any(|c| &c.id == cid) {
                return Err(MediaEventsError::argument(
                    2004,
                    format!("Unknown GameInputControl id '{cid}'."),
                ));
            }
        }
        dev.caching_num_samples = num_samples as usize;
        dev.caching_controls = control_ids;
        dev.cached_rows.clear();
        // Record an initial snapshot row when enabled so getCachedSamples yields deterministic data.
        if dev.enabled {
            let row: Vec<f32> = dev
                .caching_controls
                .iter()
                .map(|cid| {
                    dev.controls
                        .iter()
                        .find(|c| &c.id == cid)
                        .map_or(0.0, |c| c.value as f32)
                })
                .collect();
            dev.cached_rows.push(row);
        }
        Ok(())
    }

    pub fn game_device_stop_caching(&mut self, index: i32) -> Result<(), MediaEventsError> {
        let dev = self.game_device_mut(index)?;
        dev.caching_num_samples = 0;
        Ok(())
    }

    pub fn game_device_drain_samples(
        &mut self,
        index: i32,
    ) -> Result<(usize, Vec<u8>), MediaEventsError> {
        let dev = self.game_device_mut(index)?;
        let count = dev.cached_rows.len();
        let mut bytes = Vec::with_capacity(count * dev.caching_controls.len() * 4);
        for row in dev.cached_rows.drain(..) {
            for val in row {
                bytes.extend_from_slice(&val.to_be_bytes());
            }
        }
        Ok((count, bytes))
    }
}

impl MediaEventsError {
    #[allow(non_snake_case)]
    fn IllegalOperationError(code: u32, message: impl Into<String>) -> Self {
        Self::illegal(code, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_mode_quality_and_pixel_capture_invariants() {
        let mut rt = As3MediaEventsRuntime::default();
        assert_eq!(rt.camera_names(), vec!["Default Camera"]);
        assert_eq!(rt.resolve_camera(None), Some(0));
        assert_eq!(rt.resolve_camera(Some("0")), Some(0));
        assert_eq!(rt.resolve_camera(Some("Default Camera")), Some(0));
        assert_eq!(rt.resolve_camera(Some("3")), None);

        let updated = rt.camera_set_mode(0, 320, 240, 30.0, true).unwrap();
        assert_eq!(
            (updated.width, updated.height, updated.fps),
            (320, 240, 30.0)
        );
        let q = rt.camera_set_quality(0, 65_536, 85).unwrap();
        assert_eq!((q.bandwidth, q.quality), (65_536, 85));
        let k = rt.camera_set_key_frame_interval(0, 99).unwrap();
        assert_eq!(k.key_frame_interval, 48);

        let u32_pixels = rt.camera_frame_argb_u32(0, 0, 0, 4, 3).unwrap();
        assert_eq!(u32_pixels.len(), 12);
        assert_eq!(u32_pixels[0] >> 24, 0xFF);
        let byte_pixels = rt.camera_frame_argb_bytes(0, 0, 0, 4, 3).unwrap();
        assert_eq!(byte_pixels.len(), 48);
        assert_eq!(
            u32::from_be_bytes(byte_pixels[0..4].try_into().unwrap()),
            u32_pixels[0]
        );
        assert!(rt.camera_frame_argb_u32(0, 318, 0, 4, 3).is_err());
    }

    #[test]
    fn microphone_codecs_rates_and_enhanced_options_validate() {
        let mut rt = As3MediaEventsRuntime::default();
        assert_eq!(rt.resolve_microphone(-1), Some(0));
        assert_eq!(rt.resolve_microphone(0), Some(0));
        assert_eq!(rt.resolve_microphone(2), None);

        assert_eq!(rt.mic_set_rate(0, 21).unwrap().rate, 22);
        assert_eq!(rt.mic_set_rate(0, 44).unwrap().rate, 44);
        assert_eq!(rt.mic_set_codec(0, "Speex").unwrap().codec, "Speex");
        assert!(rt.mic_set_codec(0, "OpusInvalid").is_err());

        let enh = rt.mic_enable_enhanced(0).unwrap();
        assert_eq!(enh.enhanced_options.as_ref().unwrap().mode, "fullDuplex");
        assert!(
            rt.mic_set_enhanced_options(
                0,
                Some(MicrophoneEnhancedOptionsState {
                    echo_path: 512,
                    ..Default::default()
                })
            )
            .is_err()
        );
    }

    #[test]
    fn video_stream_settings_stage_video_multitouch_and_game_input() {
        let mut rt = As3MediaEventsRuntime::default();
        let (h264_id, h264) = rt.create_video_stream_settings("H264Avc").unwrap();
        assert_eq!(h264.codec, "H264Avc");
        assert_eq!(h264.profile.as_deref(), Some("baseline"));
        assert_eq!(h264.level.as_deref(), Some("2.1"));
        rt.video_settings_mut(h264_id)
            .unwrap()
            .set_profile_level("main", "4.1")
            .unwrap();
        assert!(
            rt.video_settings_mut(h264_id)
                .unwrap()
                .set_profile_level("high10", "4.1")
                .is_err()
        );

        rt.stage_video_set_viewport(0, 10.0, 20.0, 640.0, 360.0)
            .unwrap();
        rt.stage_video_set_pan(0, -0.5, 0.5).unwrap();
        assert!(rt.stage_video_set_pan(0, 1.5, 0.0).is_err());
        rt.stage_video_set_zoom(0, 2.0, 2.0).unwrap();
        assert!(rt.stage_video_set_zoom(0, 0.5, 1.0).is_err());

        assert_eq!(
            rt.set_multitouch_input_mode("touchPoint").unwrap(),
            "touchPoint"
        );
        assert!(rt.set_multitouch_input_mode("bogus").is_err());

        assert!(rt.select_audio_device(1).unwrap());
        assert!(!rt.select_audio_device(1).unwrap());
        assert!(rt.select_audio_device(9).is_err());

        rt.game_device_set_enabled(0, true).unwrap();
        rt.game_device_start_caching(0, 4, vec!["AXIS_0".into(), "BUTTON_0".into()])
            .unwrap();
        let (rows, bytes) = rt.game_device_drain_samples(0).unwrap();
        assert_eq!(rows, 1);
        assert_eq!(bytes.len(), 8);

        let mut built_in = ContextMenuBuiltInFlags::default();
        assert!(built_in.play && built_in.zoom);
        built_in.hide_all();
        assert!(!built_in.play && !built_in.zoom && !built_in.print);
    }
}
