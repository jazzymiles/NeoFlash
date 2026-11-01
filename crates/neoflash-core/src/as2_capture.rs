//! AS2 local capture policy. Browser discovery, permissions and samples are facts;
//! device identity, constraints, levels, activity hysteresis and FPS are Rust-owned.
//! Public contracts and pinned references: docs/AS2-CAPTURE-BATCH.md.
use crate::as2_media::ScriptAS2Stream;
use serde_json::{Value, json};
use std::collections::BTreeMap;

struct Device {
    kind: String,
    key: String,
    label: String,
    index: i32,
    present: bool,
    allowed: bool,
    running: bool,
    pending: bool,
    width: f64,
    height: f64,
    requested_width: f64,
    requested_height: f64,
    requested_fps: f64,
    favor_area: bool,
    bandwidth: i32,
    quality: i32,
    key_frame_interval: i32,
    loopback: bool,
    gain: f64,
    requested_rate: f64,
    actual_rate: Option<f64>,
    echo: bool,
    actual_echo: Option<bool>,
    codec: String,
    encode_quality: i32,
    frames_per_packet: i32,
    threshold: f64,
    timeout: f64,
    activity: f64,
    active: bool,
    last_signal: f64,
    previous_pixels: Vec<u8>,
    meter: ScriptAS2Stream,
}
impl Device {
    fn new(kind: &str, key: String, label: String, index: i32) -> Self {
        Self {
            kind: kind.into(),
            key,
            label,
            index,
            present: true,
            allowed: false,
            running: false,
            pending: false,
            width: 0.,
            height: 0.,
            requested_width: 160.,
            requested_height: 120.,
            requested_fps: 15.,
            favor_area: true,
            bandwidth: 16384,
            quality: 0,
            key_frame_interval: 15,
            loopback: false,
            gain: 50.,
            requested_rate: 8000.,
            actual_rate: None,
            echo: false,
            actual_echo: None,
            codec: "NellyMoser".into(),
            encode_quality: 6,
            frames_per_packet: 2,
            threshold: if kind == "camera" { 50. } else { 10. },
            timeout: 2000.,
            activity: -1.,
            active: false,
            last_signal: 0.,
            previous_pixels: Vec::new(),
            meter: ScriptAS2Stream::new(),
        }
    }
    fn constraints(&self) -> Value {
        let mut facts = if self.kind == "camera" {
            json!({"width":{"ideal":self.requested_width,"max":1920},"height":{"ideal":self.requested_height,"max":1080},"frameRate":{"ideal":self.requested_fps,"max":120},"advanced":if self.favor_area {vec![json!({"width":self.requested_width,"height":self.requested_height}),json!({"frameRate":self.requested_fps})]}else{vec![json!({"frameRate":self.requested_fps}),json!({"width":self.requested_width,"height":self.requested_height})]}})
        } else {
            json!({"sampleRate":{"ideal":self.requested_rate},"echoCancellation":self.echo,"channelCount":{"ideal":1}})
        };
        if !self.key.is_empty() {
            facts["deviceId"] = json!({"exact":self.key});
        }
        facts
    }
    fn snapshot(&mut self, now: f64) -> Result<Value, String> {
        let fps: Value = serde_json::from_str(
            &self
                .meter
                .execute(&json!({"op":"fps","now":now,"playing":self.running}).to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(
            json!({"kind":self.kind,"name":self.label,"index":self.index,"present":self.present,"muted":!self.allowed,"running":self.running,"pending":self.pending,"width":self.width,"height":self.height,"fps":self.requested_fps,"currentFps":fps["value"],"bandwidth":self.bandwidth,"quality":self.quality,"keyFrameInterval":self.key_frame_interval,"loopback":self.loopback,"activityLevel":self.activity,"motionLevel":self.threshold,"motionTimeOut":self.timeout,"gain":self.gain,"gainMultiplier":self.gain/50.,"rate":self.actual_rate.unwrap_or(self.requested_rate)/1000.,"silenceLevel":self.threshold,"silenceTimeOut":self.timeout,"useEchoSuppression":self.actual_echo.unwrap_or(self.echo),"codec":self.codec,"encodeQuality":self.encode_quality,"framesPerPacket":self.frames_per_packet}),
        )
    }
    fn level(&mut self, now: f64, value: f64) -> Vec<Value> {
        self.activity = value.clamp(0., 100.);
        let mut events = vec![];
        if self.threshold == 100. {
            self.active = false;
            return events;
        }
        if self.activity > self.threshold {
            self.last_signal = now;
            if !self.active {
                self.active = true;
                events.push(json!({"type":"activity","active":true}));
            }
        } else if self.active && now - self.last_signal >= self.timeout {
            self.active = false;
            events.push(json!({"type":"activity","active":false}));
        }
        events
    }
    fn stopped(&mut self) {
        self.running = false;
        self.pending = false;
        self.activity = -1.;
        self.active = false;
        self.previous_pixels.clear();
        let _ = self.meter.execute(r#"{"op":"reset"}"#);
    }
}
#[derive(Default)]
pub struct ScriptAS2Capture {
    devices: BTreeMap<u32, Device>,
    next: u32,
}
fn num(v: &Value, label: &str, min: f64, max: f64) -> Result<f64, String> {
    v.as_f64()
        .filter(|x| x.is_finite() && *x >= min && *x <= max)
        .ok_or_else(|| format!("{label} requires a finite value from {min} through {max}"))
}
fn text(v: &Value, label: &str, max: usize) -> Result<String, String> {
    v.as_str()
        .filter(|s| s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} requires a bounded string"))
}
impl ScriptAS2Capture {
    pub fn new() -> Self {
        Self::default()
    }
    fn device(&mut self, id: u32) -> Result<&mut Device, String> {
        self.devices
            .get_mut(&id)
            .ok_or_else(|| "Capture object is stale; reacquire it after Restart".into())
    }
    pub fn execute(&mut self, input: &str) -> Result<String, String> {
        if input.len() > 65536 {
            return Err("Capture command exceeds 64KiB".into());
        }
        let q: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        let op = q["op"].as_str().unwrap_or("");
        let now = q["now"].as_f64().unwrap_or(0.);
        num(&json!(now), "Capture clock", 0., 1e16)?;
        if op == "reset" {
            self.devices.clear();
            return Ok("{}".into());
        }
        if op == "discover" {
            let list = q["devices"]
                .as_array()
                .filter(|v| v.len() <= 32)
                .ok_or("Capture discovery supports at most 32 input devices")?;
            let mut parsed = vec![];
            let mut indices = BTreeMap::new();
            for item in list {
                let kind = text(&item["kind"], "Device kind", 16)?;
                if !matches!(kind.as_str(), "camera" | "microphone") {
                    return Err("Unknown capture kind".into());
                }
                let index = indices.entry(kind.clone()).or_insert(0);
                parsed.push((
                    kind,
                    text(&item["key"], "Device identifier", 4096)?,
                    text(&item["label"], "Device label", 4096)?,
                    *index,
                ));
                *index += 1;
            }
            for d in self.devices.values_mut() {
                d.present = false;
            }
            let mut result = vec![];
            for (kind, key, label, index) in parsed {
                let id = if let Some((&id, _)) = self.devices.iter().find(|(_, d)| {
                    d.kind == kind
                        && (d.key == key || (d.key.is_empty() && d.index == index))
                        && !d.present
                }) {
                    id
                } else {
                    if self.devices.len() >= 64 {
                        return Err("Capture device identities exceed 64 before Restart".into());
                    }
                    self.next = self
                        .next
                        .checked_add(1)
                        .ok_or("Capture identity overflow")?;
                    self.devices.insert(
                        self.next,
                        Device::new(&kind, key.clone(), label.clone(), index),
                    );
                    self.next
                };
                let d = self.devices.get_mut(&id).unwrap();
                d.present = true;
                d.index = index;
                d.key = key;
                d.label = label;
                result.push(json!({"id":id,"kind":kind,"index":index,"name":d.label}));
            }
            for d in self.devices.values_mut() {
                if !d.present {
                    d.index = -1;
                }
            }
            return Ok(json!({"value":result}).to_string());
        }
        if op == "names" || op == "get" {
            let kind = q["kind"].as_str().ok_or("Capture kind required")?;
            let mut list: Vec<_> = self
                .devices
                .iter()
                .filter(|(_, d)| d.kind == kind && d.present)
                .collect();
            list.sort_by_key(|(_, d)| d.index);
            let value = if op == "names" {
                json!(
                    list.iter()
                        .map(|(_, d)| d.label.clone())
                        .collect::<Vec<_>>()
                )
            } else {
                let index = q["index"].as_i64().unwrap_or(0);
                list.iter()
                    .find(|(_, d)| i64::from(d.index) == index)
                    .map(|(id, _)| json!(id))
                    .unwrap_or(Value::Null)
            };
            return Ok(json!({"value":value}).to_string());
        }
        let id = q["id"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("Capture device id required")?;
        let d = self.device(id)?;
        let mut events = vec![];
        let mut effect = Value::Null;
        match op {
            "state" => {}
            "request" => {
                if !d.present {
                    return Err("Capture device is no longer enumerated".into());
                }
                if q["gesture"] != true {
                    return Err("Capture requires an explicit user gesture before the browser permission request".into());
                }
                d.pending = true;
                effect = json!({"type":"request","kind":d.kind,"constraints":d.constraints()});
            }
            "mode" => {
                if d.kind != "camera" {
                    return Err("setMode requires Camera".into());
                }
                let w = num(&q["width"], "Capture width", 1., 1920.)?;
                let h = num(&q["height"], "Capture height", 1., 1080.)?;
                let fps = num(&q["fps"], "Capture fps", 1., 120.)?;
                let favor = q["favorArea"]
                    .as_bool()
                    .ok_or("favorArea requires Boolean")?;
                d.requested_width = w.round();
                d.requested_height = h.round();
                d.requested_fps = fps;
                d.favor_area = favor;
                effect = json!({"type":"constraints","constraints":d.constraints()});
            }
            "threshold" => {
                let level = num(&q["level"], "Activity threshold", 0., 100.)?;
                let timeout = num(&q["timeout"], "Activity timeout", 0., 60000.)?;
                d.threshold = level;
                d.timeout = timeout;
            }
            "gain" => {
                if d.kind != "microphone" {
                    return Err("setGain requires Microphone".into());
                }
                d.gain = num(&q["value"], "Microphone gain", 0., 100.)?;
                effect = json!({"type":"gain","value":d.gain/50.});
            }
            "rate" => {
                if d.kind != "microphone" {
                    return Err("setRate requires Microphone".into());
                }
                d.requested_rate=match q["value"].as_f64(){Some(5.)=>5512.5,Some(8.)=>8000.,Some(11.)=>11025.,Some(22.)=>22050.,Some(44.)=>44100.,_=>return Err("Microphone.setRate supports 5, 8, 11, 22, 44 kHz requests; actual browser rate is reported after acquisition".into())};
                effect = json!({"type":"constraints","constraints":d.constraints()});
            }
            "echo" => {
                if d.kind != "microphone" {
                    return Err("Echo suppression requires Microphone".into());
                }
                d.echo = q["value"]
                    .as_bool()
                    .ok_or("Echo suppression requires Boolean")?;
                effect = json!({"type":"constraints","constraints":d.constraints()});
            }
            "quality" => {
                if d.kind != "camera" {
                    return Err("setQuality requires Camera".into());
                }
                let bw = num(&q["bandwidth"], "Camera bandwidth", 0., 100_000_000.)?;
                let qual = num(&q["quality"], "Camera quality", 0., 100.)?;
                d.bandwidth = bw.trunc() as i32;
                d.quality = qual.trunc() as i32;
            }
            "keyframe" => {
                if d.kind != "camera" {
                    return Err("setKeyFrameInterval requires Camera".into());
                }
                let interval = num(&q["keyFrameInterval"], "Camera keyFrameInterval", 1., 48.)?;
                d.key_frame_interval = interval.trunc() as i32;
            }
            "loopback" => {
                if d.kind != "camera" {
                    return Err("setLoopback requires Camera".into());
                }
                d.loopback = q["compress"]
                    .as_bool()
                    .ok_or("Camera loopback compress requires Boolean")?;
            }
            "codec" => {
                if d.kind != "microphone" {
                    return Err("setCodec requires Microphone".into());
                }
                let raw = text(&q["codec"], "Microphone codec", 32)?;
                d.codec = if raw.eq_ignore_ascii_case("nellymoser") {
                    "NellyMoser".into()
                } else if raw.eq_ignore_ascii_case("speex") {
                    "Speex".into()
                } else {
                    return Err("Microphone.setCodec supports NellyMoser or Speex".into());
                };
            }
            "encode_quality" => {
                if d.kind != "microphone" {
                    return Err("setEncodeQuality requires Microphone".into());
                }
                let qual = num(&q["quality"], "Microphone encodeQuality", 0., 10.)?;
                d.encode_quality = qual.trunc() as i32;
            }
            "frames_per_packet" => {
                if d.kind != "microphone" {
                    return Err("setFramesPerPacket requires Microphone".into());
                }
                let frames = num(&q["frames"], "Microphone framesPerPacket", 1., 10.)?;
                d.frames_per_packet = frames.trunc() as i32;
            }
            "settings" => {
                let s = &q["settings"];
                if d.kind == "camera" {
                    d.width = num(&s["width"], "Actual camera width", 1., 1920.)?;
                    d.height = num(&s["height"], "Actual camera height", 1., 1080.)?;
                } else {
                    d.actual_rate = s["sampleRate"]
                        .as_f64()
                        .filter(|n| n.is_finite() && *n >= 1000. && *n <= 192000.);
                    d.actual_echo = s["echoCancellation"].as_bool();
                }
                if let Some(label) = s["label"].as_str().filter(|s| s.len() <= 4096) {
                    d.label = label.into();
                }
            }
            "status" => {
                let status = q["status"].as_str().ok_or("Capture status required")?;
                let prefix = if d.kind == "camera" {
                    "Camera"
                } else {
                    "Microphone"
                };
                let (code, level) = match status {
                    "allowed" => {
                        d.allowed = true;
                        d.running = true;
                        d.pending = false;
                        (format!("{prefix}.Unmuted"), "status")
                    }
                    "denied" => {
                        d.allowed = false;
                        d.stopped();
                        (format!("{prefix}.Muted"), "status")
                    }
                    "ended" => {
                        d.allowed = false;
                        d.stopped();
                        (format!("{prefix}.Muted"), "status")
                    }
                    "error" => {
                        d.stopped();
                        (format!("{prefix}.Unavailable"), "error")
                    }
                    _ => return Err("Unknown capture status".into()),
                };
                events.push(json!({"type":"status","info":{"code":code,"level":level,"description":q["description"].as_str().unwrap_or("")}}));
            }
            "stop" => d.stopped(),
            "frame" => {
                if !d.running {
                    return Ok(
                        json!({"state":d.snapshot(now)?,"events":[],"effect":null}).to_string()
                    );
                }
                d.meter.execute(
                    &json!({"op":"frame","now":now,"presented":q["presented"]}).to_string(),
                )?;
            }
            _ => return Err("Unknown AS2 capture operation".into()),
        }
        Ok(json!({"state":d.snapshot(now)?,"events":events,"effect":effect}).to_string())
    }
    pub fn audio_samples(&mut self, id: u32, now: f64, samples: &[f32]) -> Result<String, String> {
        if !now.is_finite()
            || now < 0.
            || samples.is_empty()
            || samples.len() > 4096
            || samples.iter().any(|x| !x.is_finite())
        {
            return Err(
                "Audio measurement requires 1..4096 finite PCM samples and a valid clock".into(),
            );
        }
        let d = self.device(id)?;
        if d.kind != "microphone" || !d.running {
            return Err("Audio measurement requires an active Microphone".into());
        }
        let rms = (samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>()
            / samples.len() as f64)
            .sqrt();
        let events = d.level(now, rms * 100.);
        Ok(json!({"state":d.snapshot(now)?,"events":events,"effect":null}).to_string())
    }
    pub fn video_pixels(&mut self, id: u32, now: f64, pixels: &[u8]) -> Result<String, String> {
        if !now.is_finite() || now < 0. || pixels.len() != 32 * 24 * 4 {
            return Err(
                "Camera motion measurement requires 32×24 RGBA pixels and a valid clock".into(),
            );
        }
        let d = self.device(id)?;
        if d.kind != "camera" || !d.running {
            return Err("Motion measurement requires an active Camera".into());
        }
        let mut difference = 0.;
        if d.previous_pixels.len() == pixels.len() {
            for (a, b) in pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(d.previous_pixels.as_chunks::<4>().0.iter())
            {
                difference += ((f64::from(a[0]) - f64::from(b[0])).abs()
                    + (f64::from(a[1]) - f64::from(b[1])).abs()
                    + (f64::from(a[2]) - f64::from(b[2])).abs())
                    / 3.;
            }
        }
        d.previous_pixels.clear();
        d.previous_pixels.extend_from_slice(pixels);
        let events = d.level(now, difference / (32. * 24. * 255.) * 100.);
        Ok(json!({"state":d.snapshot(now)?,"events":events,"effect":null}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(core: &mut ScriptAS2Capture, value: Value) -> Value {
        serde_json::from_str(&core.execute(&value.to_string()).unwrap()).unwrap()
    }
    #[test]
    fn grouped_capture_policy_identity_samples_and_lifecycle() {
        let mut core = ScriptAS2Capture::new();
        run(
            &mut core,
            json!({"op":"discover","devices":[{"kind":"camera","key":"","label":""},{"kind":"microphone","key":"mic","label":"Synthetic microphone"}]}),
        );
        let camera = run(&mut core, json!({"op":"get","kind":"camera","index":0}))["value"]
            .as_u64()
            .unwrap() as u32;
        let microphone = run(&mut core, json!({"op":"get","kind":"microphone","index":0}))["value"]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(
            run(&mut core, json!({"op":"names","kind":"camera"}))["value"],
            json!([""])
        );
        assert_eq!(
            run(&mut core, json!({"op":"get","kind":"camera","index":2}))["value"],
            Value::Null
        );
        let initial = run(&mut core, json!({"op":"state","id":camera}));
        assert_eq!(initial["state"]["activityLevel"], -1.);
        assert_eq!(initial["state"]["width"], 0.);
        assert_eq!(initial["state"]["muted"], true);
        assert_eq!(initial["state"]["bandwidth"], 16384);
        assert_eq!(initial["state"]["quality"], 0);
        assert_eq!(initial["state"]["keyFrameInterval"], 15);
        assert_eq!(initial["state"]["loopback"], false);
        let q_cam = run(
            &mut core,
            json!({"op":"quality","id":camera,"bandwidth":32768,"quality":85}),
        );
        assert_eq!(q_cam["state"]["bandwidth"], 32768);
        assert_eq!(q_cam["state"]["quality"], 85);
        assert_eq!(
            run(
                &mut core,
                json!({"op":"keyframe","id":camera,"keyFrameInterval":24})
            )["state"]["keyFrameInterval"],
            24
        );
        assert_eq!(
            run(
                &mut core,
                json!({"op":"loopback","id":camera,"compress":true})
            )["state"]["loopback"],
            true
        );
        let mic_state = run(&mut core, json!({"op":"state","id":microphone}));
        assert_eq!(mic_state["state"]["codec"], "NellyMoser");
        assert_eq!(mic_state["state"]["encodeQuality"], 6);
        assert_eq!(mic_state["state"]["framesPerPacket"], 2);
        assert_eq!(
            run(
                &mut core,
                json!({"op":"codec","id":microphone,"codec":"speex"})
            )["state"]["codec"],
            "Speex"
        );
        assert_eq!(
            run(
                &mut core,
                json!({"op":"encode_quality","id":microphone,"quality":9})
            )["state"]["encodeQuality"],
            9
        );
        assert_eq!(
            run(
                &mut core,
                json!({"op":"frames_per_packet","id":microphone,"frames":4})
            )["state"]["framesPerPacket"],
            4
        );
        assert!(
            core.execute(&json!({"op":"request","id":camera,"gesture":false}).to_string())
                .is_err()
        );
        run(
            &mut core,
            json!({"op":"discover","devices":[{"kind":"camera","key":"real-camera-id","label":"Synthetic camera"},{"kind":"microphone","key":"mic","label":"Synthetic microphone"}]}),
        );
        assert_eq!(
            run(&mut core, json!({"op":"get","kind":"camera","index":0}))["value"],
            camera
        );
        let mode = run(
            &mut core,
            json!({"op":"mode","id":camera,"width":320,"height":240,"fps":20,"favorArea":false}),
        );
        assert_eq!(mode["effect"]["constraints"]["frameRate"]["ideal"], 20.);
        let request = run(
            &mut core,
            json!({"op":"request","id":camera,"gesture":true}),
        );
        assert_eq!(
            request["effect"]["constraints"]["deviceId"]["exact"],
            "real-camera-id"
        );
        run(
            &mut core,
            json!({"op":"settings","id":camera,"settings":{"width":320,"height":240,"frameRate":20}}),
        );
        run(
            &mut core,
            json!({"op":"status","id":camera,"status":"allowed"}),
        );
        run(
            &mut core,
            json!({"op":"threshold","id":camera,"level":20,"timeout":100}),
        );
        let black = vec![0; 32 * 24 * 4];
        let white = vec![255; 32 * 24 * 4];
        core.video_pixels(camera, 0., &black).unwrap();
        let motion: Value =
            serde_json::from_str(&core.video_pixels(camera, 50., &white).unwrap()).unwrap();
        assert_eq!(motion["state"]["activityLevel"], 100.);
        assert_eq!(motion["events"][0]["active"], true);
        let quiet: Value =
            serde_json::from_str(&core.video_pixels(camera, 151., &white).unwrap()).unwrap();
        assert_eq!(quiet["events"][0]["active"], false);
        run(
            &mut core,
            json!({"op":"frame","id":camera,"now":200,"presented":1}),
        );
        let frames = run(
            &mut core,
            json!({"op":"frame","id":camera,"now":700,"presented":11}),
        );
        assert_eq!(frames["state"]["currentFps"], 20.);
        assert_eq!(
            run(&mut core, json!({"op":"gain","id":microphone,"value":75}))["effect"]["value"],
            1.5
        );
        assert_eq!(
            run(&mut core, json!({"op":"rate","id":microphone,"value":22}))["effect"]["constraints"]
                ["sampleRate"]["ideal"],
            22050.
        );
        run(&mut core, json!({"op":"echo","id":microphone,"value":true}));
        run(
            &mut core,
            json!({"op":"settings","id":microphone,"settings":{"sampleRate":48000,"echoCancellation":true}}),
        );
        run(
            &mut core,
            json!({"op":"status","id":microphone,"status":"allowed"}),
        );
        run(
            &mut core,
            json!({"op":"threshold","id":microphone,"level":10,"timeout":100}),
        );
        let rms: Value =
            serde_json::from_str(&core.audio_samples(microphone, 0., &[0.5, -0.5]).unwrap())
                .unwrap();
        assert_eq!(rms["state"]["activityLevel"], 50.);
        assert_eq!(rms["state"]["rate"], 48.);
        assert_eq!(rms["events"][0]["active"], true);
        let silence: Value =
            serde_json::from_str(&core.audio_samples(microphone, 101., &[0., 0.]).unwrap())
                .unwrap();
        assert_eq!(silence["events"][0]["active"], false);
        assert!(core.audio_samples(microphone, 0., &[f32::NAN]).is_err());
        let stop = run(
            &mut core,
            json!({"op":"status","id":camera,"status":"denied"}),
        );
        assert_eq!(stop["state"]["activityLevel"], -1.);
        assert_eq!(stop["events"][0]["info"]["code"], "Camera.Muted");
        run(&mut core, json!({"op":"reset"}));
        assert!(
            core.execute(&json!({"op":"state","id":camera}).to_string())
                .is_err()
        );
    }
}
