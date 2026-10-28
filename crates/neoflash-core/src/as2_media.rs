//! AS2 video component policy: states, cue ordering, timing and viewport sizing.
//! Browser media handles/decoder and authored object references remain in the host.
//! Contract references and pinned reuse: docs/AS2-MEDIA-BATCH.md.
use serde_json::{Value, json};
use std::collections::VecDeque;

/// AS2-only progressive stream receive policy and measurements. The browser
/// supplies TimeRanges and presented-frame timestamps; no decoder is emulated.
pub struct ScriptAS2Stream {
    audio: bool,
    video: bool,
    frames: VecDeque<(f64, f64)>,
}
impl Default for ScriptAS2Stream {
    fn default() -> Self {
        Self {
            audio: true,
            video: true,
            frames: VecDeque::new(),
        }
    }
}
impl ScriptAS2Stream {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn execute(&mut self, request: &str) -> Result<String, String> {
        let q: Value = serde_json::from_str(request).map_err(|_| "Invalid AS2 stream request")?;
        let value = match q["op"].as_str().unwrap_or("") {
            "receive" => {
                let enabled = q["enabled"].as_bool().ok_or("AS2 receiveAudio/receiveVideo requires Boolean; numeric FPS selection is unsupported")?;
                match q["kind"].as_str() {
                    Some("audio") => self.audio = enabled,
                    Some("video") => {
                        self.video = enabled;
                        self.frames.clear();
                    }
                    _ => return Err("Unknown AS2 receiving channel".into()),
                }
                Value::Null
            }
            "reset" => {
                self.frames.clear();
                Value::Null
            }
            "frame" => {
                let now = finite(&q["now"], "Frame timestamp", 0., 1e16)?;
                let count = finite(&q["presented"], "Presented frames", 0., 1e16)?;
                if self.video {
                    if self
                        .frames
                        .back()
                        .is_some_and(|&(last, n)| now <= last || count < n)
                    {
                        self.frames.clear();
                    }
                    self.frames.push_back((now, count));
                    while self.frames.len() > 64
                        || self.frames.len() > 2
                            && self.frames.get(1).is_some_and(|&(t, _)| now - t > 1000.)
                    {
                        self.frames.pop_front();
                    }
                }
                Value::Null
            }
            "fps" => {
                let now = finite(&q["now"], "Frame timestamp", 0., 1e16)?;
                let mut fps = 0.;
                if self.video
                    && q["playing"] == true
                    && let (Some(&(first, a)), Some(&(last, b))) =
                        (self.frames.front(), self.frames.back())
                    && last - first >= 100.
                    && now - last <= 1000.
                {
                    fps = (b - a) * 1000. / (last - first);
                }
                json!(fps)
            }
            "buffer" => {
                let time = finite(&q["time"], "Stream time", 0., 86400. * 365.)?;
                let ranges = q["ranges"]
                    .as_array()
                    .filter(|v| v.len() <= 256)
                    .ok_or("Browser buffered ranges exceed the AS2 profile")?;
                let mut end = time;
                for range in ranges {
                    let start = finite(&range[0], "Buffer start", 0., 86400. * 365.)?;
                    let stop = finite(&range[1], "Buffer end", start, 86400. * 365.)?;
                    if start <= end && stop > end {
                        end = stop;
                    }
                }
                json!(end - time)
            }
            _ => return Err("Unknown AS2 stream operation".into()),
        };
        Ok(json!({"value":value,"audio":self.audio,"video":self.video}).to_string())
    }
}

#[derive(Clone)]
struct Cue {
    id: u32,
    time: f64,
    name: String,
}
impl Cue {
    fn json(&self) -> Value {
        json!({"id":self.id,"time":self.time,"name":self.name,"type":"actionscript"})
    }
}
pub struct ScriptAS2Media {
    state: String,
    url: String,
    metadata: Option<Value>,
    time: f64,
    duration: f64,
    desired: String,
    pending_seek: Option<f64>,
    after_seek: String,
    rewind_seek: bool,
    auto_play: bool,
    auto_rewind: bool,
    auto_size: bool,
    aspect: bool,
    volume: f64,
    width: f64,
    height: f64,
    preferred_width: f64,
    preferred_height: f64,
    playhead_interval: f64,
    progress_interval: f64,
    last_tick: f64,
    last_progress: f64,
    progress_complete: bool,
    bytes_loaded: Option<u64>,
    bytes_total: Option<u64>,
    last_cue_time: f64,
    cues: Vec<Cue>,
    next_cue: u32,
    events: Vec<Value>,
    effects: Vec<Value>,
}
impl Default for ScriptAS2Media {
    fn default() -> Self {
        Self {
            state: "disconnected".into(),
            url: String::new(),
            metadata: None,
            time: 0.,
            duration: 0.,
            desired: "paused".into(),
            pending_seek: None,
            after_seek: "paused".into(),
            rewind_seek: false,
            auto_play: true,
            auto_rewind: true,
            auto_size: false,
            aspect: true,
            volume: 100.,
            width: 320.,
            height: 240.,
            preferred_width: 0.,
            preferred_height: 0.,
            playhead_interval: 250.,
            progress_interval: 250.,
            last_tick: f64::NEG_INFINITY,
            last_progress: f64::NEG_INFINITY,
            progress_complete: false,
            bytes_loaded: None,
            bytes_total: None,
            last_cue_time: -0.001,
            cues: Vec::new(),
            next_cue: 0,
            events: Vec::new(),
            effects: Vec::new(),
        }
    }
}
fn fail(code: u32, message: &str) -> String {
    format!("{code}|{message}")
}
fn finite(value: &Value, name: &str, min: f64, max: f64) -> Result<f64, String> {
    let n = value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= min && *v <= max)
        .ok_or_else(|| {
            fail(
                1003,
                &format!("{name} requires a finite number in {min}..{max}"),
            )
        })?;
    Ok(n)
}
fn millis(value: f64) -> f64 {
    (value * 1000.).round() / 1000.
}
impl ScriptAS2Media {
    pub fn new() -> Self {
        Self::default()
    }
    fn event(&mut self, name: &str, extra: Value) {
        let mut v = json!({"type":name,"state":self.state,"playheadTime":self.time});
        if let Some(extra) = extra.as_object() {
            v.as_object_mut().unwrap().extend(extra.clone());
        }
        self.events.push(v);
    }
    fn transition(&mut self, state: &str) {
        if self.state != state {
            self.state = state.into();
            self.event("stateChange", json!({}));
            if matches!(state, "playing" | "paused" | "stopped" | "buffering") {
                self.event(state, json!({}));
            }
        }
    }
    fn snapshot(&self) -> Value {
        json!({"state":self.state,"stateResponsive":matches!(self.state.as_str(),"disconnected"|"stopped"|"playing"|"paused"|"buffering"),"url":self.url,"metadata":self.metadata,"metadataLoaded":self.metadata.is_some(),"connected":self.metadata.is_some()&&!matches!(self.state.as_str(),"disconnected"|"connectionError"),"playheadTime":self.time,"totalTime":self.duration,"playheadPercentage":if self.duration>0.{self.time/self.duration*100.}else{0.},"autoPlay":self.auto_play,"autoRewind":self.auto_rewind,"autoSize":self.auto_size,"maintainAspectRatio":self.aspect,"volume":self.volume,"width":self.width,"height":self.height,"preferredWidth":self.preferred_width,"preferredHeight":self.preferred_height,"videoWidth":self.preferred_width,"videoHeight":self.preferred_height,"playheadUpdateInterval":self.playhead_interval,"progressInterval":self.progress_interval,"bytesLoaded":self.bytes_loaded,"bytesTotal":self.bytes_total,"playing":self.state=="playing","paused":self.state=="paused","stopped":self.state=="stopped","buffering":self.state=="buffering","isRTMP":false,"isLive":false})
    }
    fn size(&mut self, width: f64, height: f64) {
        let (width, height) =
            if self.aspect && self.preferred_width > 0. && self.preferred_height > 0. {
                let scale = (width / self.preferred_width).min(height / self.preferred_height);
                (self.preferred_width * scale, self.preferred_height * scale)
            } else {
                (width, height)
            };
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.effects
                .push(json!({"op":"size","width":width,"height":height}));
            self.event("resize", json!({"width":width,"height":height}));
        }
    }
    fn begin(&mut self, url: &str, play: bool, clear_cues: bool) -> Result<(), String> {
        if url.is_empty() || url.len() > 8192 || url.chars().any(|c| c.is_control()) {
            return Err(fail(
                1004,
                "Video content path requires 1–8192 non-control bytes",
            ));
        }
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("rtmp")
            || lower.starts_with("file:")
            || lower
                .split(['?', '#'])
                .next()
                .is_some_and(|s| s.ends_with(".flv") || s.ends_with(".f4v") || s.ends_with(".xml"))
        {
            return Err(fail(
                1004,
                "Use progressive browser-native MP4/WebM or a bundled video path; RTMP, FLV and SMIL are unavailable",
            ));
        }
        self.url = url.into();
        self.metadata = None;
        self.time = 0.;
        self.duration = 0.;
        self.bytes_loaded = None;
        self.bytes_total = None;
        self.pending_seek = None;
        self.last_cue_time = -0.001;
        self.last_tick = f64::NEG_INFINITY;
        self.last_progress = f64::NEG_INFINITY;
        self.progress_complete = false;
        self.desired = if play { "playing" } else { "paused" }.into();
        if clear_cues {
            self.cues.clear();
        }
        self.transition("loading");
        self.effects
            .push(json!({"op":"load","url":url,"play":play}));
        Ok(())
    }
    fn seek(&mut self, value: f64, final_state: Option<&str>) -> Result<(), String> {
        let value = millis(value);
        if self.url.is_empty() {
            return Err(fail(1003, "Load a video before seeking"));
        }
        if self.metadata.is_some() && value > self.duration {
            return Err(fail(1003, "Seek exceeds the finite media duration"));
        }
        self.after_seek = final_state.unwrap_or(&self.desired).to_string();
        self.rewind_seek = value < self.time;
        self.pending_seek = Some(value);
        if self.metadata.is_some() {
            self.transition(if self.rewind_seek {
                "rewinding"
            } else {
                "seeking"
            });
            self.effects.push(json!({"op":"seek","time":value}));
        }
        Ok(())
    }
    fn observe(&mut self, kind: &str, sample: &Value) -> Result<(), String> {
        let now = sample["now"]
            .as_f64()
            .filter(|v| v.is_finite())
            .unwrap_or(0.);
        if let Some(value) = sample["time"]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.)
        {
            self.time = value;
        }
        self.bytes_loaded = sample["bytesLoaded"].as_u64();
        self.bytes_total = sample["bytesTotal"].as_u64();
        match kind {
            "metadata" => {
                self.preferred_width = finite(&sample["width"], "Video width", 1., 1920.)?;
                self.preferred_height = finite(&sample["height"], "Video height", 1., 1080.)?;
                self.duration = finite(&sample["duration"], "Video duration", 0., 86400. * 365.)?;
                self.metadata = Some(
                    json!({"width":self.preferred_width,"height":self.preferred_height,"duration":self.duration}),
                );
                self.event("metadataReceived", json!({"info":self.metadata}));
                if self.auto_size {
                    self.size(self.preferred_width, self.preferred_height);
                } else {
                    self.size(self.width, self.height);
                }
                if let Some(at) = self.pending_seek {
                    self.seek(at, None)?;
                }
            }
            "ready" => {
                if self.pending_seek.is_none() {
                    if self.desired == "playing" {
                        self.transition("buffering");
                    } else {
                        let next = self.desired.clone();
                        self.transition(&next);
                    }
                }
                self.event("ready", json!({}));
            }
            "playing" => {
                if self.desired == "playing" {
                    self.transition("playing");
                } else {
                    self.effects.push(json!({"op":"pause"}));
                }
            }
            "waiting" => {
                if self.desired == "playing" {
                    self.transition("buffering");
                }
            }
            "paused" => {
                if self.pending_seek.is_none() && self.desired != "playing" {
                    let next = self.desired.clone();
                    self.transition(&next);
                }
            }
            "seeked" => {
                if self.pending_seek.take().is_some() {
                    self.last_cue_time = if self.rewind_seek && self.time == 0. {
                        -0.001
                    } else {
                        self.time
                    };
                    let next = self.after_seek.clone();
                    self.transition(&next);
                    self.event("seek", json!({}));
                    if self.rewind_seek {
                        self.event("rewind", json!({}));
                    }
                    self.rewind_seek = false;
                }
            }
            "ended" => {
                self.desired = "stopped".into();
                self.transition("stopped");
                self.event("complete", json!({}));
                if self.auto_rewind {
                    self.seek(0., Some("stopped"))?;
                }
            }
            "error" => {
                self.desired = "paused".into();
                self.pending_seek = None;
                self.transition("connectionError");
                self.event("error",json!({"code":1000,"message":sample["message"].as_str().unwrap_or("Browser media loading failed")}));
            }
            "time" => {
                if self.state == "playing" && self.pending_seek.is_none() {
                    if self.time < self.last_cue_time {
                        self.last_cue_time = self.time;
                    }
                    let cues: Vec<_> = self
                        .cues
                        .iter()
                        .filter(|c| c.time > self.last_cue_time && c.time <= self.time)
                        .map(Cue::json)
                        .collect();
                    for cue in cues {
                        self.event("cuePoint", json!({"info":cue}));
                    }
                    self.last_cue_time = self.time;
                    if now - self.last_tick >= self.playhead_interval {
                        self.last_tick = now;
                        self.event("playheadUpdate", json!({}));
                    }
                }
            }
            "sample" | "progress" => {}
            _ => return Err(fail(1000, "Unknown browser video observation")),
        }
        if kind != "sample"
            && !self.progress_complete
            && self.bytes_total.is_some()
            && now - self.last_progress >= self.progress_interval
        {
            self.last_progress = now;
            self.progress_complete = self.bytes_loaded == self.bytes_total;
            self.event(
                "progress",
                json!({"bytesLoaded":self.bytes_loaded,"bytesTotal":self.bytes_total}),
            );
        }
        Ok(())
    }
    fn cue_result(&self, index: Option<usize>) -> Value {
        match index {
            Some(index) => {
                json!({"index":index,"points":self.cues.iter().map(Cue::json).collect::<Vec<_>>()})
            }
            None => Value::Null,
        }
    }
    fn cue_query(&self, v: &Value, nearest: bool) -> Result<Option<usize>, String> {
        let time = v["time"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.)
            .map(millis);
        let name = v["name"].as_str();
        if time.is_none() && name.is_none() {
            return Err(fail(
                1002,
                "Cue lookup requires a name or nonnegative finite time",
            ));
        }
        match v["type"].as_str().unwrap_or("all") {
            "all" | "actionscript" => {}
            "event" | "flv" | "navigation" => return Ok(None),
            _ => return Err(fail(1002, "Unknown cue point type")),
        }
        let matches: Vec<_> = self
            .cues
            .iter()
            .enumerate()
            .filter(|(_, c)| name.is_none_or(|n| c.name == n))
            .collect();
        if let Some(time) = time {
            if let Some((i, _)) = matches.iter().find(|(_, c)| c.time == time) {
                return Ok(Some(*i));
            }
            if nearest {
                if let Some((_, c)) = matches.iter().rev().find(|(_, c)| c.time < time) {
                    return Ok(matches
                        .iter()
                        .find(|(_, first)| first.time == c.time)
                        .map(|(i, _)| *i));
                }
                if name.is_some() {
                    return Ok(matches.first().map(|(i, _)| *i));
                }
            }
            Ok(None)
        } else {
            Ok(matches.first().map(|(i, _)| *i))
        }
    }
    pub fn execute(&mut self, input: &str) -> Result<String, String> {
        if input.len() > 512 * 1024 {
            return Err(fail(1000, "Video component request exceeds 512 KiB"));
        }
        let v: Value = serde_json::from_str(input)
            .map_err(|e| fail(1000, &format!("Invalid video request: {e}")))?;
        self.events.clear();
        self.effects.clear();
        let value = match v["op"].as_str().unwrap_or("") {
            "definitions" => {
                json!({"states":{"BUFFERING":"buffering","CONNECTION_ERROR":"connectionError","DISCONNECTED":"disconnected","LOADING":"loading","PAUSED":"paused","PLAYING":"playing","RESIZING":"resizing","REWINDING":"rewinding","SEEKING":"seeking","STOPPED":"stopped"},"cueTypes":{"ACTIONSCRIPT":"actionscript","ALL":"all","EVENT":"event","FLV":"flv","NAVIGATION":"navigation"},"errors":{"NO_CONNECTION":1000,"NO_CUE_POINT_MATCH":1001,"ILLEGAL_CUE_POINT":1002,"INVALID_SEEK":1003,"INVALID_CONTENT_PATH":1004,"INVALID_XML":1005,"NO_BITRATE_MATCH":1006,"DELETE_DEFAULT_PLAYER":1007}})
            }
            "state" => self.snapshot(),
            "init" => {
                self.width = finite(&v["width"], "Viewport width", 1., 8192.)?;
                self.height = finite(&v["height"], "Viewport height", 1., 8192.)?;
                Value::Null
            }
            "observe" => {
                self.observe(v["kind"].as_str().unwrap_or("sample"), &v["sample"])?;
                Value::Null
            }
            "load" => {
                let url = v["url"]
                    .as_str()
                    .ok_or_else(|| fail(1004, "Video load requires a URL string"))?;
                self.begin(url, v["play"].as_bool().unwrap_or(self.auto_play), true)?;
                Value::Null
            }
            "play" => {
                self.desired = "playing".into();
                if self.metadata.is_none() && self.state == "disconnected" && !self.url.is_empty() {
                    let url = self.url.clone();
                    self.begin(&url, true, false)?;
                } else if !self.url.is_empty() {
                    if self.metadata.is_some() && self.time >= self.duration {
                        self.seek(0., Some("playing"))?;
                    }
                    self.effects.push(json!({"op":"play"}));
                }
                Value::Null
            }
            "pause" => {
                self.desired = "paused".into();
                self.effects.push(json!({"op":"pause"}));
                if self.metadata.is_some() && self.pending_seek.is_none() {
                    self.transition("paused");
                }
                Value::Null
            }
            "stop" => {
                self.desired = "stopped".into();
                self.effects.push(json!({"op":"pause"}));
                if self.metadata.is_some() {
                    self.transition("stopped");
                    if self.auto_rewind {
                        self.seek(0., Some("stopped"))?;
                    }
                }
                Value::Null
            }
            "close" => {
                self.desired = "paused".into();
                self.pending_seek = None;
                self.metadata = None;
                self.time = 0.;
                self.effects.push(json!({"op":"close"}));
                self.transition("disconnected");
                self.event("close", json!({}));
                Value::Null
            }
            "seek" => {
                let mut time = finite(&v["time"], "Seek", 0., 86400. * 365.)?;
                if v["percent"] == true {
                    if self.metadata.is_none() {
                        return Err(fail(1003, "Percentage seeking requires loaded metadata"));
                    }
                    if time > 100. {
                        return Err(fail(1003, "Seek percent requires 0..100"));
                    }
                    time = self.duration * time / 100.;
                }
                self.seek(time, None)?;
                Value::Null
            }
            "setting" => {
                let name = v["name"]
                    .as_str()
                    .ok_or_else(|| fail(1000, "Setting name required"))?;
                let value = &v["value"];
                match name {
                    "autoPlay" => {
                        self.auto_play = value
                            .as_bool()
                            .ok_or_else(|| fail(1000, "autoPlay requires Boolean"))?
                    }
                    "autoRewind" => {
                        self.auto_rewind = value
                            .as_bool()
                            .ok_or_else(|| fail(1000, "autoRewind requires Boolean"))?
                    }
                    "autoSize" => {
                        self.auto_size = value
                            .as_bool()
                            .ok_or_else(|| fail(1000, "autoSize requires Boolean"))?;
                        if self.auto_size && self.metadata.is_some() {
                            self.size(self.preferred_width, self.preferred_height);
                        }
                    }
                    "maintainAspectRatio" => {
                        self.aspect = value
                            .as_bool()
                            .ok_or_else(|| fail(1000, "maintainAspectRatio requires Boolean"))?;
                        self.size(self.width, self.height);
                    }
                    "volume" => {
                        self.volume = finite(value, "Volume", 0., 100.)?;
                        self.effects
                            .push(json!({"op":"volume","value":self.volume/100.}));
                        self.event("volumeUpdate", json!({}));
                    }
                    "playheadUpdateInterval" => {
                        self.playhead_interval =
                            finite(value, "Playhead interval milliseconds", 16., 60000.)?
                    }
                    "progressInterval" => {
                        self.progress_interval =
                            finite(value, "Progress interval milliseconds", 16., 60000.)?
                    }
                    _ => return Err(fail(1000, "Unsupported video component setting")),
                }
                Value::Null
            }
            "size" => {
                let width = finite(&v["width"], "Viewport width", 1., 8192.)?;
                let height = finite(&v["height"], "Viewport height", 1., 8192.)?;
                self.auto_size = false;
                self.size(width, height);
                Value::Null
            }
            "scale" => {
                let x = finite(&v["x"], "Horizontal scale", 0.01, 10000.)?;
                let y = finite(&v["y"], "Vertical scale", 0.01, 10000.)?;
                let width = finite(&v["width"], "Current viewport width", 0.01, 8192.)?;
                let height = finite(&v["height"], "Current viewport height", 0.01, 8192.)?;
                let old_x = finite(&v["oldX"], "Current horizontal scale", 0.01, 10000.)?;
                let old_y = finite(&v["oldY"], "Current vertical scale", 0.01, 10000.)?;
                let next_w = width * x / old_x;
                let next_h = height * y / old_y;
                if next_w > 8192. || next_h > 8192. {
                    return Err(fail(1003, "Scaled video viewport exceeds 8192 pixels"));
                }
                self.auto_size = false;
                self.size(next_w, next_h);
                Value::Null
            }
            "cue_add" => {
                if self.cues.len() >= 1024 || self.next_cue >= 4096 {
                    return Err(fail(
                        1002,
                        "Cue points exceed 1024 live / 4096 lifetime per controller",
                    ));
                }
                let time = millis(finite(&v["time"], "Cue time", 0., 86400. * 365.)?);
                let name = v["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 1024)
                    .ok_or_else(|| fail(1002, "Cue name requires 1–1024 UTF-8 bytes"))?;
                self.next_cue += 1;
                let id = self.next_cue;
                self.cues.push(Cue {
                    id,
                    time,
                    name: name.into(),
                });
                self.cues.sort_by(|a, b| {
                    a.time
                        .total_cmp(&b.time)
                        .then_with(|| a.name.encode_utf16().cmp(b.name.encode_utf16()))
                        .then(a.id.cmp(&b.id))
                });
                self.cue_result(self.cues.iter().position(|c| c.id == id))
            }
            "cue_find" => self.cue_result(self.cue_query(&v, v["nearest"] == true)?),
            "cue_remove" => {
                if let Some(index) = self.cue_query(&v, false)? {
                    let result = self.cue_result(Some(index));
                    self.cues.remove(index);
                    result
                } else {
                    Value::Null
                }
            }
            "cue_next" => {
                let points = v["points"]
                    .as_array()
                    .filter(|p| p.len() <= 1024)
                    .ok_or_else(|| fail(1002, "Cue snapshot required"))?;
                let index = v["index"]
                    .as_u64()
                    .map(|n| n as usize)
                    .filter(|i| *i < points.len())
                    .ok_or_else(|| fail(1002, "Valid cue snapshot index required"))?;
                let name = points[index]["name"].as_str();
                points
                    .iter()
                    .enumerate()
                    .skip(index + 1)
                    .find(|(_, point)| point["name"].as_str() == name)
                    .map(|(i, _)| json!({"index":i,"points":points}))
                    .unwrap_or(Value::Null)
            }
            _ => return Err(fail(1000, "Unknown AS2 video component operation")),
        };
        Ok(json!({"value":value,"state":self.snapshot(),"events":self.events,"effects":self.effects}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(p: &mut ScriptAS2Media, v: Value) -> Value {
        serde_json::from_str(&p.execute(&v.to_string()).unwrap()).unwrap()
    }
    #[test]
    fn as2_media_family() {
        let mut p = ScriptAS2Media::new();
        call(&mut p, json!({"op":"init","width":400,"height":300}));
        call(
            &mut p,
            json!({"op":"load","url":"media/movie.mp4","play":false}),
        );
        let sample = json!({"now":0,"time":0,"width":320,"height":180,"duration":4,"bytesLoaded":500,"bytesTotal":500});
        let ready = call(
            &mut p,
            json!({"op":"observe","kind":"metadata","sample":sample}),
        );
        assert_eq!(ready["state"]["height"], 225.);
        call(
            &mut p,
            json!({"op":"observe","kind":"ready","sample":sample}),
        );
        assert_eq!(p.state, "paused");
        let cue = call(
            &mut p,
            json!({"op":"cue_add","time":0.50049,"name":"hello"}),
        );
        assert_eq!(cue["value"]["points"][0]["time"], 0.5);
        call(&mut p, json!({"op":"cue_add","time":1.2,"name":"hello"}));
        call(&mut p, json!({"op":"cue_add","time":1.2,"name":"alpha"}));
        let found = call(&mut p, json!({"op":"cue_find","time":1.9,"nearest":true}));
        assert_eq!(
            found["value"]["points"][found["value"]["index"].as_u64().unwrap() as usize]["name"],
            "alpha"
        );
        let first = call(&mut p, json!({"op":"cue_find","name":"hello"}))["value"].clone();
        let next = call(
            &mut p,
            json!({"op":"cue_next","points":first["points"],"index":first["index"]}),
        );
        assert_eq!(next["value"]["index"], 2);
        call(&mut p, json!({"op":"play"}));
        call(
            &mut p,
            json!({"op":"observe","kind":"playing","sample":sample}),
        );
        let tick = call(
            &mut p,
            json!({"op":"observe","kind":"time","sample":{"now":700,"time":0.7,"bytesLoaded":500,"bytesTotal":500}}),
        );
        assert!(
            tick["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "cuePoint")
        );
        let again = call(
            &mut p,
            json!({"op":"observe","kind":"time","sample":{"now":710,"time":0.71,"bytesLoaded":500,"bytesTotal":500}}),
        );
        assert!(again["events"].as_array().unwrap().is_empty());
        assert!(
            p.execute(&json!({"op":"seek","time":5}).to_string())
                .unwrap_err()
                .starts_with("1003|")
        );
        call(&mut p, json!({"op":"seek","time":50,"percent":true}));
        call(
            &mut p,
            json!({"op":"observe","kind":"seeked","sample":{"now":800,"time":2}}),
        );
        assert_eq!(p.state, "playing");
        let end = call(
            &mut p,
            json!({"op":"observe","kind":"ended","sample":{"now":4000,"time":4}}),
        );
        assert!(
            end["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["op"] == "seek" && e["time"] == 0.)
        );
        call(
            &mut p,
            json!({"op":"observe","kind":"seeked","sample":{"now":4010,"time":0}}),
        );
        assert_eq!(p.state, "stopped");
        call(&mut p, json!({"op":"close"}));
        assert_eq!(p.state, "disconnected");
        assert!(
            p.execute(&json!({"op":"load","url":"rtmp://example/live","play":true}).to_string())
                .unwrap_err()
                .starts_with("1004|")
        );
        let mut stream = ScriptAS2Stream::new();
        let invoke = |s: &mut ScriptAS2Stream, q: Value| -> Value {
            serde_json::from_str(&s.execute(&q.to_string()).unwrap()).unwrap()
        };
        let muted = invoke(
            &mut stream,
            json!({"op":"receive","kind":"audio","enabled":false}),
        );
        assert_eq!(muted["audio"], false);
        assert_eq!(
            invoke(
                &mut stream,
                json!({"op":"buffer","time":2,"ranges":[[0,3],[3,4],[5,8]]})
            )["value"],
            2.
        );
        invoke(&mut stream, json!({"op":"frame","now":0,"presented":1}));
        invoke(&mut stream, json!({"op":"frame","now":500,"presented":13}));
        assert_eq!(
            invoke(&mut stream, json!({"op":"fps","now":510,"playing":true}))["value"],
            24.
        );
        invoke(
            &mut stream,
            json!({"op":"receive","kind":"video","enabled":false}),
        );
        assert_eq!(
            invoke(&mut stream, json!({"op":"fps","now":510,"playing":true}))["value"],
            0.
        );
        assert!(
            stream
                .execute(r#"{"op":"receive","kind":"video","enabled":24}"#)
                .is_err()
        );
    }
}
