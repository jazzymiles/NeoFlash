//! Deterministic, bounded SWF timeline player and AVM1/AVM2 bytecode interpreter.
//!
//! Executes SWF frame tags (`PlaceObject*`, `RemoveObject*`, `DoAction`, `DoInitAction`,
//! `DoABC`/`DoABC2`, `StartSound`, `SoundStream*`, `DefineSprite`, `DefineButton*`)
//! step-by-step without external runtimes, recording display list states, `trace()`
//! output, and audio events across frames.

use super::avm2::{AbcFile, AbcReader, AbcTraitData, parse_abc_file};
use super::bits::SwfSlice;
use super::{SwfContainer, read_container, read_container_from_bytes};
use crate::ImportError;
use neoflash_core::{BlendMode, ColorTransform, FilterSpec, Transform};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Read,
    path::Path,
};

const MAX_VM_STEPS_PER_FRAME: usize = 50_000;
const MAX_CALL_DEPTH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwfVmKind {
    Avm1,
    Avm2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwfDisplayEntry {
    pub depth: u16,
    pub character_id: u16,
    pub name: String,
    pub matrix: [f64; 6],
    pub x: f64,
    pub y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation: f64,
    pub alpha: f64,
    pub visible: bool,
    pub blend_mode: BlendMode,
    pub filters: Vec<FilterSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_depth: Option<u16>,
    pub current_frame: u32,
    pub total_frames: u32,
    pub playing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwfAudioEvent {
    pub frame: u32,
    pub sound_id: u16,
    pub sync: String,
    pub loops: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwfFrameState {
    pub frame: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub playing: bool,
    pub display_list: Vec<SwfDisplayEntry>,
    pub traces: Vec<String>,
    pub audio_events: Vec<SwfAudioEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwfPlaybackResult {
    pub swf_version: u8,
    pub vm_kind: SwfVmKind,
    pub total_frames: u32,
    pub frames_executed: Vec<SwfFrameState>,
    pub trace_output: Vec<String>,
    pub audio_events: Vec<SwfAudioEvent>,
    pub variables: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
struct RawFrameTag {
    code: u16,
    payload: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct TimelineDef {
    total_frames: u32,
    frames: Vec<Vec<RawFrameTag>>,
    labels: Vec<(String, u32)>,
}

impl TimelineDef {
    fn resolve_label(&self, query: &str) -> Option<u32> {
        self.labels
            .iter()
            .find(|(lbl, _)| lbl.eq_ignore_ascii_case(query))
            .map(|(_, frame)| *frame)
    }
}

#[derive(Clone, Debug)]
struct ClipEventHandler {
    event_flags: u32,
    #[allow(dead_code)]
    key_code: Option<u8>,
    actions: Vec<u8>,
}

#[derive(Clone, Debug)]
struct ButtonCondAction {
    conditions: u16,
    actions: Vec<u8>,
}

#[derive(Clone, Debug)]
struct LiveSlot {
    character_id: u16,
    name: String,
    matrix: [f64; 6],
    x: f64,
    y: f64,
    scale_x: f64,
    scale_y: f64,
    rotation: f64,
    alpha: f64,
    visible: bool,
    blend_mode: BlendMode,
    filters: Vec<FilterSpec>,
    clip_depth: Option<u16>,
    current_frame: u32,
    total_frames: u32,
    playing: bool,
    last_applied_frame: Option<u32>,
    props: HashMap<String, Avm1Value>,
    clip_handlers: Vec<ClipEventHandler>,
}

#[derive(Clone, Debug)]
enum Avm1Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Object(usize),
    MovieClip(String),
}

impl Avm1Value {
    fn to_number(&self) -> f64 {
        match self {
            Self::Undefined | Self::Null => 0.0,
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Number(n) => *n,
            Self::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
            Self::Object(_) | Self::MovieClip(_) => f64::NAN,
        }
    }

    fn to_i32(&self) -> i32 {
        let n = self.to_number();
        if !n.is_finite() || n == 0.0 {
            0
        } else {
            (n.trunc() as i64) as i32
        }
    }

    fn to_bool(&self, swf_version: u8) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::String(s) => {
                if swf_version <= 6 {
                    s.trim().parse::<f64>().is_ok_and(|v| v != 0.0)
                } else {
                    !s.is_empty()
                }
            }
            Self::Object(_) | Self::MovieClip(_) => true,
        }
    }

    fn to_trace_string(&self, swf_version: u8, objects: &[Avm1Object]) -> String {
        match self {
            Self::Undefined => {
                if swf_version <= 6 {
                    String::new()
                } else {
                    "undefined".to_owned()
                }
            }
            Self::Null => "null".to_owned(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            Self::Number(n) => format_avm_number(*n),
            Self::String(s) => s.clone(),
            Self::MovieClip(path) => path.clone(),
            Self::Object(id) => {
                if let Some(obj) = objects.get(*id)
                    && let Some(elements) = &obj.array_elements
                {
                    return elements
                        .iter()
                        .map(|v| v.to_trace_string(swf_version, objects))
                        .collect::<Vec<_>>()
                        .join(",");
                }
                "[object Object]".to_owned()
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Avm1Object {
    props: HashMap<String, Avm1Value>,
    array_elements: Option<Vec<Avm1Value>>,
    function: Option<Avm1Function>,
}

#[derive(Clone, Debug)]
struct Avm1Function {
    params: Vec<(u8, String)>,
    #[allow(dead_code)]
    register_count: u8,
    #[allow(dead_code)]
    flags: u16,
    body: Vec<u8>,
    constant_pool: Vec<String>,
    base_target: String,
}

#[derive(Clone, Debug)]
struct LoadedAbc {
    abc: AbcFile,
}

pub struct SwfBytecodePlayer {
    swf_version: u8,
    vm_kind: SwfVmKind,
    root_timeline: TimelineDef,
    sprites: HashMap<u16, TimelineDef>,
    buttons: HashMap<u16, Vec<ButtonCondAction>>,
    init_actions: Vec<(u16, Vec<u8>)>,
    abc_tags: Vec<LoadedAbc>,
    symbol_classes: HashMap<u16, String>,

    // Runtime state
    current_frame: u32,
    playing: bool,
    executed_frames: u32,
    last_applied_frame: Option<u32>,
    display_list: BTreeMap<u16, LiveSlot>,
    trace_output: Vec<String>,
    frame_traces: Vec<String>,
    audio_events: Vec<SwfAudioEvent>,
    frame_audio_events: Vec<SwfAudioEvent>,
    stream_sound_active: bool,

    // AVM1 state
    avm1_globals: HashMap<String, Avm1Value>,
    avm1_functions: HashMap<String, Avm1Function>,
    avm1_objects: Vec<Avm1Object>,
    avm1_registers: [Avm1Value; 256],
    avm1_constant_pool: Vec<String>,
    avm1_target: String,
    rng_state: u32,

    // AVM2 state
    avm2_initialized: bool,
    avm2_globals: HashMap<String, Avm1Value>,
    avm2_frame_scripts: HashMap<u32, (usize, usize)>, // frame_idx -> (abc_idx, method_idx)
}

impl SwfBytecodePlayer {
    pub fn from_path(path: &Path) -> Result<Self, ImportError> {
        let mut f = File::open(path)?;
        let mut header = [0u8; 16];
        let n = f.read(&mut header)?;
        let container = read_container(path, &header[..n])?;
        Self::from_container(&container)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ImportError> {
        let container = read_container_from_bytes(bytes.to_vec())?;
        Self::from_container(&container)
    }

    pub(crate) fn from_container(container: &SwfContainer) -> Result<Self, ImportError> {
        let Some(body) = container.body.as_deref() else {
            return Err(ImportError::Container(
                container
                    .zws_error
                    .clone()
                    .unwrap_or_else(|| "SWF body could not be decompressed".into()),
            ));
        };
        let mut r = SwfSlice::new(body);
        let _bounds = r
            .rect_twips()
            .ok_or_else(|| ImportError::Container("truncated SWF stage RECT".into()))?;
        let _fps = r
            .ufixed8()
            .ok_or_else(|| ImportError::Container("truncated SWF frame rate".into()))?;
        let declared_frames = r
            .u16()
            .ok_or_else(|| ImportError::Container("truncated SWF frame count".into()))?;
        let tag_stream = r.rest();

        let mut as3_flag = false;
        let mut has_do_abc = false;
        let mut sprites: HashMap<u16, TimelineDef> = HashMap::new();
        let mut buttons: HashMap<u16, Vec<ButtonCondAction>> = HashMap::new();
        let mut init_actions: Vec<(u16, Vec<u8>)> = Vec::new();
        let mut abc_tags: Vec<LoadedAbc> = Vec::new();
        let mut symbol_classes: HashMap<u16, String> = HashMap::new();

        let mut root_frames: Vec<Vec<RawFrameTag>> = Vec::new();
        let mut cur_tags: Vec<RawFrameTag> = Vec::new();
        let mut root_labels: Vec<(String, u32)> = Vec::new();

        walk_swf_tags(tag_stream, |code, payload| {
            match code {
                0 => return false,
                69 => {
                    if let Some(&flags) = payload.first()
                        && (flags & 0x08) != 0
                    {
                        as3_flag = true;
                    }
                }
                39 => {
                    // DefineSprite
                    let mut sr = SwfSlice::new(payload);
                    if let (Some(sprite_id), Some(fc)) = (sr.u16(), sr.u16()) {
                        let def = parse_timeline_tags(sr.rest(), container.swf_version, fc);
                        sprites.insert(sprite_id, def);
                    }
                }
                7 | 34 => {
                    // DefineButton / DefineButton2
                    if let Some((btn_id, conds)) = parse_button_actions(code, payload) {
                        buttons.insert(btn_id, conds);
                    }
                }
                59 => {
                    // DoInitAction
                    if payload.len() >= 2 {
                        let sprite_id = u16::from_le_bytes([payload[0], payload[1]]);
                        init_actions.push((sprite_id, payload[2..].to_vec()));
                    }
                }
                72 | 82 => {
                    // DoABC / DoABC2
                    has_do_abc = true;
                    let abc_bytes = if code == 82 {
                        let mut ar = SwfSlice::new(payload);
                        let _flags = ar.u32();
                        let _name = ar.c_string(container.swf_version);
                        ar.rest()
                    } else {
                        payload
                    };
                    if let Some(abc) = parse_abc_file(abc_bytes) {
                        abc_tags.push(LoadedAbc { abc });
                    }
                }
                76 => {
                    // SymbolClass
                    let mut sr = SwfSlice::new(payload);
                    let count = sr.u16().unwrap_or(0);
                    for _ in 0..count {
                        let Some(tag_id) = sr.u16() else { break };
                        let Some((cls, _)) = sr.c_string(container.swf_version) else {
                            break;
                        };
                        symbol_classes.insert(tag_id, cls);
                    }
                }
                43 => {
                    // FrameLabel
                    let mut lr = SwfSlice::new(payload);
                    if let Some((lbl, _)) = lr.c_string(container.swf_version) {
                        root_labels.push((lbl, root_frames.len() as u32));
                    }
                    cur_tags.push(RawFrameTag {
                        code,
                        payload: payload.to_vec(),
                    });
                }
                1 => {
                    // ShowFrame
                    root_frames.push(std::mem::take(&mut cur_tags));
                }
                4 | 5 | 12 | 15 | 18 | 19 | 26 | 28 | 45 | 70 => {
                    cur_tags.push(RawFrameTag {
                        code,
                        payload: payload.to_vec(),
                    });
                }
                _ => {}
            }
            true
        });

        if !cur_tags.is_empty() || root_frames.is_empty() {
            root_frames.push(cur_tags);
        }
        let total_frames = (root_frames.len() as u32).max(u32::from(declared_frames).max(1));
        let vm_kind = if as3_flag || has_do_abc {
            SwfVmKind::Avm2
        } else {
            SwfVmKind::Avm1
        };

        Ok(Self {
            swf_version: container.swf_version,
            vm_kind,
            root_timeline: TimelineDef {
                total_frames,
                frames: root_frames,
                labels: root_labels,
            },
            sprites,
            buttons,
            init_actions,
            abc_tags,
            symbol_classes,
            current_frame: 0,
            playing: true,
            executed_frames: 0,
            last_applied_frame: None,
            display_list: BTreeMap::new(),
            trace_output: Vec::new(),
            frame_traces: Vec::new(),
            audio_events: Vec::new(),
            frame_audio_events: Vec::new(),
            stream_sound_active: false,
            avm1_globals: HashMap::new(),
            avm1_functions: HashMap::new(),
            avm1_objects: Vec::new(),
            avm1_registers: std::array::from_fn(|_| Avm1Value::Undefined),
            avm1_constant_pool: Vec::new(),
            avm1_target: String::new(),
            rng_state: 0x1234_5678,
            avm2_initialized: false,
            avm2_globals: HashMap::new(),
            avm2_frame_scripts: HashMap::new(),
        })
    }

    pub fn swf_version(&self) -> u8 {
        self.swf_version
    }

    pub fn vm_kind(&self) -> SwfVmKind {
        self.vm_kind
    }

    pub fn total_frames(&self) -> u32 {
        self.root_timeline.total_frames
    }

    pub fn current_frame(&self) -> u32 {
        self.current_frame
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn trace_output(&self) -> &[String] {
        &self.trace_output
    }

    pub fn display_list(&self) -> Vec<SwfDisplayEntry> {
        self.display_list
            .iter()
            .map(|(&depth, s)| SwfDisplayEntry {
                depth,
                character_id: s.character_id,
                name: s.name.clone(),
                matrix: s.matrix,
                x: s.x,
                y: s.y,
                scale_x: s.scale_x,
                scale_y: s.scale_y,
                rotation: s.rotation,
                alpha: s.alpha,
                visible: s.visible,
                blend_mode: s.blend_mode,
                filters: s.filters.clone(),
                clip_depth: s.clip_depth,
                current_frame: s.current_frame,
                total_frames: s.total_frames,
                playing: s.playing,
            })
            .collect()
    }

    /// Execute one frame step on the SWF timeline and return the resulting `SwfFrameState`.
    pub fn step_frame(&mut self) -> SwfFrameState {
        self.frame_traces.clear();
        self.frame_audio_events.clear();

        if self.executed_frames == 0 {
            // Run DoInitAction blocks on frame 0 before frame tags
            let init_blocks = self.init_actions.clone();
            for (_, code) in init_blocks {
                self.execute_avm1_slice(&code);
            }
            if self.vm_kind == SwfVmKind::Avm2 && !self.avm2_initialized {
                self.avm2_initialized = true;
                self.initialize_avm2();
            }
        }

        let entering_frame = self.current_frame;
        let frame_changed = self.last_applied_frame != Some(entering_frame);
        self.last_applied_frame = Some(entering_frame);

        let mut frame_label = None;
        let mut pending_do_actions: Vec<Vec<u8>> = Vec::new();
        let mut newly_loaded_handlers: Vec<Vec<u8>> = Vec::new();

        if frame_changed
            && let Some(tags) = self
                .root_timeline
                .frames
                .get(entering_frame as usize)
                .cloned()
        {
            for tag in tags {
                match tag.code {
                    43 => {
                        let mut lr = SwfSlice::new(&tag.payload);
                        if let Some((lbl, _)) = lr.c_string(self.swf_version) {
                            frame_label = Some(lbl);
                        }
                    }
                    4 | 26 | 70 => {
                        let loads = self.apply_place_tag(tag.code, &tag.payload);
                        newly_loaded_handlers.extend(loads);
                    }
                    5 | 28 => {
                        let unloads = self.apply_remove_tag(tag.code, &tag.payload);
                        for code in unloads {
                            self.execute_avm1_slice(&code);
                        }
                    }
                    12 => {
                        pending_do_actions.push(tag.payload);
                    }
                    15 => {
                        self.apply_start_sound(&tag.payload, entering_frame);
                    }
                    18 | 45 => {
                        self.stream_sound_active = true;
                    }
                    19 if self.stream_sound_active => {
                        let ev = SwfAudioEvent {
                            frame: entering_frame,
                            sound_id: 0,
                            sync: "stream".to_owned(),
                            loops: 1,
                        };
                        self.audio_events.push(ev.clone());
                        self.frame_audio_events.push(ev);
                    }
                    _ => {}
                }
            }
        }

        // 1. Fire ClipEventLoad handlers for newly placed instances
        for code in newly_loaded_handlers {
            self.execute_avm1_slice(&code);
        }

        // 2. Fire ClipEventEnterFrame handlers on active instances (and step nested sprites)
        let depths: Vec<u16> = self.display_list.keys().copied().collect();
        for d in depths {
            let enter_handlers: Vec<Vec<u8>> = self
                .display_list
                .get(&d)
                .map(|slot| {
                    slot.clip_handlers
                        .iter()
                        .filter(|h| (h.event_flags & 0x02) != 0) // ClipEventEnterFrame = 0x02
                        .map(|h| h.actions.clone())
                        .collect()
                })
                .unwrap_or_default();
            for code in enter_handlers {
                self.execute_avm1_slice(&code);
            }
            if let Some(slot) = self.display_list.get_mut(&d)
                && slot.last_applied_frame.is_some()
                && slot.playing
                && slot.total_frames > 1
            {
                slot.current_frame = (slot.current_frame + 1) % slot.total_frames;
            }
            self.step_sprite_frame_actions(d);
        }

        // 3. Execute frame DoAction scripts
        if frame_changed {
            for action_bytes in pending_do_actions {
                self.execute_avm1_slice(&action_bytes);
            }
            // 4. Execute AVM2 addFrameScript callback if registered for this frame
            if self.vm_kind == SwfVmKind::Avm2
                && let Some(&(abc_idx, method_idx)) = self.avm2_frame_scripts.get(&entering_frame)
            {
                self.execute_avm2_method(abc_idx, method_idx, Vec::new());
            }
        }

        let state = SwfFrameState {
            frame: self.current_frame,
            label: frame_label,
            playing: self.playing,
            display_list: self.display_list(),
            traces: self.frame_traces.clone(),
            audio_events: self.frame_audio_events.clone(),
        };

        self.executed_frames += 1;
        // If a script called gotoAndPlay/gotoAndStop during this frame, `self.current_frame` was already updated.
        if self.current_frame == entering_frame && self.playing {
            let total = self.root_timeline.frames.len().max(1) as u32;
            if total > 1 {
                self.current_frame = (self.current_frame + 1) % total;
            }
        }

        state
    }

    fn step_sprite_frame_actions(&mut self, depth: u16) {
        let Some((cid, target_name, frame_idx)) = self.display_list.get_mut(&depth).and_then(|s| {
            if s.last_applied_frame != Some(s.current_frame) {
                s.last_applied_frame = Some(s.current_frame);
                Some((s.character_id, s.name.clone(), s.current_frame))
            } else {
                None
            }
        }) else {
            return;
        };
        let do_actions: Vec<Vec<u8>> = self
            .sprites
            .get(&cid)
            .and_then(|def| def.frames.get(frame_idx as usize))
            .map(|tags| {
                tags.iter()
                    .filter(|t| t.code == 12)
                    .map(|t| t.payload.clone())
                    .collect()
            })
            .unwrap_or_default();
        if !do_actions.is_empty() {
            let saved_target = std::mem::replace(&mut self.avm1_target, target_name);
            for code in do_actions {
                self.execute_avm1_slice(&code);
            }
            self.avm1_target = saved_target;
        }
    }

    /// Trigger button condition / clip mouse press events on placed buttons/clips.
    pub fn trigger_pointer_click(&mut self) {
        let buttons = self.buttons.clone();
        let depths: Vec<u16> = self.display_list.keys().copied().collect();
        for d in depths {
            let Some(cid) = self.display_list.get(&d).map(|s| s.character_id) else {
                continue;
            };
            if let Some(conds) = buttons.get(&cid) {
                for cond in conds {
                    // OverDownToOverUp (release = 0x0008) or OverUpToOverDown (press = 0x0004) or DefineButton (0)
                    if cond.conditions == 0 || (cond.conditions & 0x000C) != 0 {
                        self.execute_avm1_slice(&cond.actions);
                    }
                }
            }
            let handlers: Vec<Vec<u8>> = self
                .display_list
                .get(&d)
                .map(|slot| {
                    slot.clip_handlers
                        .iter()
                        .filter(|h| (h.event_flags & (0x04 | 0x08 | 0x400 | 0x800)) != 0)
                        .map(|h| h.actions.clone())
                        .collect()
                })
                .unwrap_or_default();
            for code in handlers {
                self.execute_avm1_slice(&code);
            }
        }
    }

    pub fn run_frames(&mut self, max_frames: usize) -> SwfPlaybackResult {
        let mut frames_executed = Vec::with_capacity(max_frames.min(1024));
        let limit = max_frames.clamp(1, 10_000);
        for _ in 0..limit {
            let prev_frame = self.current_frame;
            let state = self.step_frame();
            frames_executed.push(state);
            let any_child_playing = self
                .display_list
                .values()
                .any(|s| s.playing && s.total_frames > 1);
            let root_pending = self.last_applied_frame != Some(self.current_frame);
            if !self.playing
                && self.current_frame == prev_frame
                && !root_pending
                && !any_child_playing
            {
                break;
            }
        }
        let mut variables = BTreeMap::new();
        for (k, v) in &self.avm1_globals {
            variables.insert(
                k.clone(),
                v.to_trace_string(self.swf_version, &self.avm1_objects),
            );
        }
        for (k, v) in &self.avm2_globals {
            variables.insert(
                k.clone(),
                v.to_trace_string(self.swf_version, &self.avm1_objects),
            );
        }
        SwfPlaybackResult {
            swf_version: self.swf_version,
            vm_kind: self.vm_kind,
            total_frames: self.root_timeline.total_frames,
            frames_executed,
            trace_output: self.trace_output.clone(),
            audio_events: self.audio_events.clone(),
            variables,
        }
    }

    fn record_trace(&mut self, msg: String) {
        self.trace_output.push(msg.clone());
        self.frame_traces.push(msg);
    }

    fn apply_start_sound(&mut self, payload: &[u8], frame: u32) {
        let mut r = SwfSlice::new(payload);
        let Some(sound_id) = r.u16() else { return };
        let Some(flags) = r.u8() else { return };
        let sync_stop = (flags & 0x20) != 0;
        let sync_no_multiple = (flags & 0x10) != 0;
        let has_in_point = (flags & 0x01) != 0;
        let has_out_point = (flags & 0x02) != 0;
        let has_loops = (flags & 0x04) != 0;
        if has_in_point {
            let _ = r.u32();
        }
        if has_out_point {
            let _ = r.u32();
        }
        let loops = if has_loops {
            r.u16().unwrap_or(1).max(1)
        } else {
            1
        };
        let sync = if sync_stop {
            "stop"
        } else if sync_no_multiple {
            "start"
        } else {
            "event"
        }
        .to_owned();
        let ev = SwfAudioEvent {
            frame,
            sound_id,
            sync,
            loops,
        };
        self.audio_events.push(ev.clone());
        self.frame_audio_events.push(ev);
    }

    fn apply_place_tag(&mut self, tag_code: u16, payload: &[u8]) -> Vec<Vec<u8>> {
        let mut r = SwfSlice::new(payload);
        let mut load_scripts = Vec::new();

        if tag_code == 4 {
            let Some(character_id) = r.u16() else {
                return load_scripts;
            };
            let Some(depth) = r.u16() else {
                return load_scripts;
            };
            let Some(matrix) = r.matrix_px() else {
                return load_scripts;
            };
            let (_, alpha) = if r.remaining() > 0 {
                r.cxform(false)
                    .unwrap_or_else(|| (ColorTransform::default(), 1.0))
            } else {
                (ColorTransform::default(), 1.0)
            };
            let tf = Transform::from_matrix(matrix).unwrap_or_default();
            let sprite_total = self
                .sprites
                .get(&character_id)
                .map_or(1, |s| s.total_frames.max(1));
            self.display_list.insert(
                depth,
                LiveSlot {
                    character_id,
                    name: String::new(),
                    matrix,
                    x: tf.x,
                    y: tf.y,
                    scale_x: tf.scale_x * 100.0,
                    scale_y: tf.scale_y * 100.0,
                    rotation: tf.rotation,
                    alpha: alpha * 100.0,
                    visible: true,
                    blend_mode: BlendMode::Normal,
                    filters: Vec::new(),
                    clip_depth: None,
                    current_frame: 0,
                    total_frames: sprite_total,
                    playing: true,
                    last_applied_frame: None,
                    props: HashMap::new(),
                    clip_handlers: Vec::new(),
                },
            );
            return load_scripts;
        }

        let Some(flags1) = r.u8() else {
            return load_scripts;
        };
        let flags2 = if tag_code == 70 {
            r.u8().unwrap_or(0)
        } else {
            0
        };
        let Some(depth) = r.u16() else {
            return load_scripts;
        };

        let has_clip_actions = (flags1 & 0x80) != 0;
        let has_clip_depth = (flags1 & 0x40) != 0;
        let has_name = (flags1 & 0x20) != 0;
        let has_ratio = (flags1 & 0x10) != 0;
        let has_cxform = (flags1 & 0x08) != 0;
        let has_matrix = (flags1 & 0x04) != 0;
        let has_character = (flags1 & 0x02) != 0;
        let is_move = (flags1 & 0x01) != 0;

        if tag_code == 70 && ((flags2 & 0x08) != 0 || ((flags2 & 0x10) != 0 && has_character)) {
            let _ = r.c_string(self.swf_version);
        }
        let new_char = if has_character { r.u16() } else { None };
        let new_matrix = if has_matrix { r.matrix_px() } else { None };
        let new_cxform = if has_cxform { r.cxform(true) } else { None };
        if has_ratio {
            let _ = r.u16();
        }
        let new_name = if has_name {
            r.c_string(self.swf_version).map(|(s, _)| s)
        } else {
            None
        };
        let new_clip_depth = if has_clip_depth { r.u16() } else { None };
        let mut new_visible = None;
        let mut new_blend = None;
        if tag_code == 70 {
            if (flags2 & 0x01) != 0 {
                skip_filter_bytes(&mut r);
            }
            if (flags2 & 0x02) != 0 {
                new_blend = r.u8();
            }
            if (flags2 & 0x04) != 0 {
                let _ = r.u8();
            }
            if (flags2 & 0x20) != 0 {
                new_visible = r.u8().map(|v| v != 0);
            }
            if (flags2 & 0x40) != 0 {
                let _ = r.rgba();
            }
        }
        let new_handlers = if has_clip_actions {
            parse_raw_clip_handlers(&mut r, self.swf_version)
        } else {
            Vec::new()
        };

        if is_move && let Some(slot) = self.display_list.get_mut(&depth) {
            if let Some(cid) = new_char {
                slot.character_id = cid;
            }
            if let Some(m) = new_matrix {
                slot.matrix = m;
                if let Some(tf) = Transform::from_matrix(m) {
                    slot.x = tf.x;
                    slot.y = tf.y;
                    slot.scale_x = tf.scale_x * 100.0;
                    slot.scale_y = tf.scale_y * 100.0;
                    slot.rotation = tf.rotation;
                }
            }
            if let Some((_, a)) = new_cxform {
                slot.alpha = a * 100.0;
            }
            if let Some(nm) = new_name {
                slot.name = nm;
            }
            if let Some(cd) = new_clip_depth {
                slot.clip_depth = (cd > 0).then_some(cd);
            }
            if let Some(vis) = new_visible {
                slot.visible = vis;
            }
            if let Some(b) = new_blend
                && let Some(bm) = map_blend(b)
            {
                slot.blend_mode = bm;
            }
            if !new_handlers.is_empty() {
                slot.clip_handlers = new_handlers;
            }
            return load_scripts;
        }

        let Some(character_id) = new_char else {
            return load_scripts;
        };
        let matrix = new_matrix.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let tf = Transform::from_matrix(matrix).unwrap_or_default();
        let alpha = new_cxform.map_or(100.0, |(_, a)| a * 100.0);
        let sprite_total = self
            .sprites
            .get(&character_id)
            .map_or(1, |s| s.total_frames.max(1));

        for h in &new_handlers {
            // ClipEventLoad (0x01) or ClipEventInitialize (0x020000)
            if (h.event_flags & (0x01 | 0x0002_0000)) != 0 {
                load_scripts.push(h.actions.clone());
            }
        }

        self.display_list.insert(
            depth,
            LiveSlot {
                character_id,
                name: new_name.unwrap_or_default(),
                matrix,
                x: tf.x,
                y: tf.y,
                scale_x: tf.scale_x * 100.0,
                scale_y: tf.scale_y * 100.0,
                rotation: tf.rotation,
                alpha,
                visible: new_visible.unwrap_or(true),
                blend_mode: new_blend.and_then(map_blend).unwrap_or(BlendMode::Normal),
                filters: Vec::new(),
                clip_depth: new_clip_depth.filter(|&cd| cd > 0),
                current_frame: 0,
                total_frames: sprite_total,
                playing: true,
                last_applied_frame: None,
                props: HashMap::new(),
                clip_handlers: new_handlers,
            },
        );
        load_scripts
    }

    fn apply_remove_tag(&mut self, tag_code: u16, payload: &[u8]) -> Vec<Vec<u8>> {
        let mut r = SwfSlice::new(payload);
        let depth = if tag_code == 5 {
            let _cid = r.u16();
            r.u16()
        } else {
            r.u16()
        };
        let mut unload_scripts = Vec::new();
        if let Some(d) = depth
            && let Some(removed) = self.display_list.remove(&d)
        {
            for h in removed.clip_handlers {
                if (h.event_flags & 0x04) != 0 {
                    unload_scripts.push(h.actions);
                }
            }
        }
        unload_scripts
    }

    // =========================================================================
    // AVM1 Bytecode Interpreter
    // =========================================================================

    fn execute_avm1_slice(&mut self, code: &[u8]) {
        let mut stack: Vec<Avm1Value> = Vec::new();
        let mut locals: HashMap<String, Avm1Value> = HashMap::new();
        let mut steps = 0usize;
        let _ = self.run_avm1_code(code, &mut stack, &mut locals, &mut steps, 0);
    }

    fn run_avm1_code(
        &mut self,
        code: &[u8],
        stack: &mut Vec<Avm1Value>,
        locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Option<Avm1Value> {
        if depth > MAX_CALL_DEPTH {
            return None;
        }
        let mut pc = 0usize;
        while pc < code.len() && *steps < MAX_VM_STEPS_PER_FRAME {
            *steps += 1;
            let op = code[pc];
            pc += 1;
            if op == 0x00 {
                break;
            }
            let payload: &[u8] = if (op & 0x80) != 0 {
                if pc + 2 > code.len() {
                    break;
                }
                let len = usize::from(u16::from_le_bytes([code[pc], code[pc + 1]]));
                pc += 2;
                let end = (pc + len).min(code.len());
                let slice = &code[pc..end];
                pc = end;
                slice
            } else {
                &[]
            };

            match op {
                0x04 => {
                    // ActionNextFrame
                    self.set_target_frame_step(1);
                }
                0x05 => {
                    // ActionPrevFrame
                    self.set_target_frame_step(-1);
                }
                0x06 => {
                    // ActionPlay
                    self.set_target_playing(true);
                }
                0x07 => {
                    // ActionStop
                    self.set_target_playing(false);
                }
                0x09 => {
                    // ActionStopSounds
                    self.stream_sound_active = false;
                }
                0x0A => {
                    // ActionAdd (numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(b + a));
                }
                0x0B => {
                    // ActionSubtract
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(b - a));
                }
                0x0C => {
                    // ActionMultiply
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(b * a));
                }
                0x0D => {
                    // ActionDivide
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(b / a));
                }
                0x0E => {
                    // ActionEquals (SWF 4 numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Bool((b - a).abs() < 1e-12));
                }
                0x0F => {
                    // ActionLess (SWF 4 numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Bool(b < a));
                }
                0x10 => {
                    // ActionAnd
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    stack.push(Avm1Value::Bool(b && a));
                }
                0x11 => {
                    // ActionOr
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    stack.push(Avm1Value::Bool(b || a));
                }
                0x12 => {
                    // ActionNot
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    stack.push(Avm1Value::Bool(!a));
                }
                0x13 => {
                    // ActionStringEquals
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    stack.push(Avm1Value::Bool(b == a));
                }
                0x14 | 0x31 => {
                    // ActionStringLength / ActionMBStringLength
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    stack.push(Avm1Value::Number(s.chars().count() as f64));
                }
                0x15 | 0x35 => {
                    // ActionStringExtract (1-based index, count)
                    let count =
                        stack.pop().unwrap_or(Avm1Value::Undefined).to_i32().max(0) as usize;
                    let index = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .saturating_sub(1)
                        .max(0) as usize;
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let sub: String = s.chars().skip(index).take(count).collect();
                    stack.push(Avm1Value::String(sub));
                }
                0x17 => {
                    // ActionPop
                    let _ = stack.pop();
                }
                0x18 => {
                    // ActionToInteger
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(v)));
                }
                0x1C => {
                    // ActionGetVariable
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let val = self.get_avm1_variable(&name, locals);
                    stack.push(val);
                }
                0x1D => {
                    // ActionSetVariable
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.set_avm1_variable(name, val, locals);
                }
                0x20 => {
                    // ActionSetTarget2
                    let t = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.avm1_target = t;
                }
                0x21 => {
                    // ActionStringAdd
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    stack.push(Avm1Value::String(format!("{b}{a}")));
                }
                0x22 => {
                    // ActionGetProperty
                    let prop_idx = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let target = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let val = self.get_mc_property(&target, prop_idx);
                    stack.push(val);
                }
                0x23 => {
                    // ActionSetProperty
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let prop_idx = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let target = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.set_mc_property(&target, prop_idx, val);
                }
                0x26 => {
                    // ActionTrace
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let msg = v.to_trace_string(self.swf_version, &self.avm1_objects);
                    self.record_trace(msg);
                }
                0x29 => {
                    // ActionStringLess
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    stack.push(Avm1Value::Bool(b < a));
                }
                0x30 => {
                    // ActionRandomNumber (deterministic LCG)
                    let max = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.rng_state = self
                        .rng_state
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    let r = if max > 0 {
                        (self.rng_state % (max as u32)) as i32
                    } else {
                        0
                    };
                    stack.push(Avm1Value::Number(f64::from(r)));
                }
                0x3A => {
                    // ActionDelete
                    let prop = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let Avm1Value::Object(id) = obj
                        && let Some(o) = self.avm1_objects.get_mut(id)
                    {
                        o.props.remove(&prop);
                    }
                    stack.push(Avm1Value::Bool(true));
                }
                0x3B => {
                    // ActionDelete2
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    locals.remove(&name);
                    self.avm1_globals.remove(&name);
                    stack.push(Avm1Value::Bool(true));
                }
                0x3C => {
                    // ActionDefineLocal
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    locals.insert(name.clone(), val.clone());
                    if depth == 0 {
                        self.avm1_globals.insert(name, val);
                    }
                }
                0x3D => {
                    // ActionCallFunction
                    let fn_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let ret = self.call_avm1_function(&fn_name, args, locals, steps, depth);
                    stack.push(ret);
                }
                0x3E => {
                    // ActionReturn
                    return Some(stack.pop().unwrap_or(Avm1Value::Undefined));
                }
                0x40 => {
                    // ActionNewObject
                    let ctor_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let obj_id = self.avm1_objects.len();
                    let mut obj = Avm1Object::default();
                    if ctor_name == "Array" {
                        if nargs == 1
                            && let Some(Avm1Value::Number(n)) = args.first()
                        {
                            let len = (*n as usize).min(4096);
                            obj.array_elements = Some(vec![Avm1Value::Undefined; len]);
                        } else {
                            obj.array_elements = Some(args);
                        }
                    }
                    self.avm1_objects.push(obj);
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x41 => {
                    // ActionDefineLocal2
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    locals.entry(name).or_insert(Avm1Value::Undefined);
                }
                0x42 => {
                    // ActionInitArray
                    let n = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 4096) as usize;
                    let mut elems = Vec::with_capacity(n);
                    for _ in 0..n {
                        elems.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props: HashMap::new(),
                        array_elements: Some(elems),
                        function: None,
                    });
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x43 => {
                    // ActionInitObject
                    let n = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 4096) as usize;
                    let mut props = HashMap::new();
                    for _ in 0..n {
                        let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                        let key = stack
                            .pop()
                            .unwrap_or(Avm1Value::Undefined)
                            .to_trace_string(self.swf_version, &self.avm1_objects);
                        props.insert(key, val);
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props,
                        array_elements: None,
                        function: None,
                    });
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x44 => {
                    // ActionTypeOf
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let t = match v {
                        Avm1Value::Undefined => "undefined",
                        Avm1Value::Null => "null",
                        Avm1Value::Bool(_) => "boolean",
                        Avm1Value::Number(_) => "number",
                        Avm1Value::String(_) => "string",
                        Avm1Value::MovieClip(_) => "movieclip",
                        Avm1Value::Object(id) => {
                            if self
                                .avm1_objects
                                .get(id)
                                .is_some_and(|o| o.function.is_some())
                            {
                                "function"
                            } else {
                                "object"
                            }
                        }
                    };
                    stack.push(Avm1Value::String(t.to_owned()));
                }
                0x47 => {
                    // ActionAdd2 (type-aware)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if matches!(a, Avm1Value::String(_)) || matches!(b, Avm1Value::String(_)) {
                        let sa = a.to_trace_string(self.swf_version, &self.avm1_objects);
                        let sb = b.to_trace_string(self.swf_version, &self.avm1_objects);
                        stack.push(Avm1Value::String(format!("{sb}{sa}")));
                    } else {
                        stack.push(Avm1Value::Number(b.to_number() + a.to_number()));
                    }
                }
                0x48 => {
                    // ActionLess2
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = match (&b, &a) {
                        (Avm1Value::String(sb), Avm1Value::String(sa)) => sb < sa,
                        _ => b.to_number() < a.to_number(),
                    };
                    stack.push(Avm1Value::Bool(res));
                }
                0x49 => {
                    // ActionEquals2
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    stack.push(Avm1Value::Bool(avm1_equals(
                        &b,
                        &a,
                        self.swf_version,
                        &self.avm1_objects,
                    )));
                }
                0x4A => {
                    // ActionToNumber
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(n));
                }
                0x4B => {
                    // ActionToString
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    stack.push(Avm1Value::String(s));
                }
                0x4C => {
                    // ActionPushDuplicate
                    let top = stack.last().cloned().unwrap_or(Avm1Value::Undefined);
                    stack.push(top);
                }
                0x4D => {
                    // ActionSwap
                    let len = stack.len();
                    if len >= 2 {
                        stack.swap(len - 1, len - 2);
                    }
                }
                0x4E => {
                    // ActionGetMember
                    let member = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = self.get_avm1_member(&target, &member);
                    stack.push(val);
                }
                0x4F => {
                    // ActionSetMember
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let member = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.set_avm1_member(&target, &member, val);
                }
                0x50 => {
                    // ActionIncrement
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(n + 1.0));
                }
                0x51 => {
                    // ActionDecrement
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(n - 1.0));
                }
                0x52 => {
                    // ActionCallMethod
                    let method_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let ret =
                        self.call_avm1_method(&receiver, &method_name, args, locals, steps, depth);
                    stack.push(ret);
                }
                0x53 => {
                    // ActionModulo
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(b % a));
                }
                0x55 => {
                    // ActionEnumerate2
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    stack.push(Avm1Value::Null);
                    if let Avm1Value::Object(id) = obj
                        && let Some(o) = self.avm1_objects.get(id)
                    {
                        let mut keys: Vec<String> = o.props.keys().cloned().collect();
                        keys.sort();
                        for k in keys {
                            stack.push(Avm1Value::String(k));
                        }
                    }
                }
                0x60 => {
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(b & a)));
                }
                0x61 => {
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(b.wrapping_shl(a as u32))));
                }
                0x62 => {
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(b | a)));
                }
                0x63 => {
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(b.wrapping_shr(a as u32))));
                }
                0x64 => {
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32() as u32;
                    stack.push(Avm1Value::Number(f64::from(b.wrapping_shr(a as u32))));
                }
                0x65 => {
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(b ^ a)));
                }
                0x66 => {
                    // ActionStrictEquals
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = match (&b, &a) {
                        (Avm1Value::Undefined, Avm1Value::Undefined)
                        | (Avm1Value::Null, Avm1Value::Null) => true,
                        (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
                        (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
                        (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
                        (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
                        (Avm1Value::MovieClip(x), Avm1Value::MovieClip(y)) => x == y,
                        _ => false,
                    };
                    stack.push(Avm1Value::Bool(eq));
                }
                0x67 => {
                    // ActionGreater
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = match (&b, &a) {
                        (Avm1Value::String(sb), Avm1Value::String(sa)) => sb > sa,
                        _ => b.to_number() > a.to_number(),
                    };
                    stack.push(Avm1Value::Bool(res));
                }
                0x68 => {
                    // ActionStringGreater
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let b = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    stack.push(Avm1Value::Bool(b > a));
                }
                0x81 => {
                    // ActionGotoFrame (u16 zero-based frame index)
                    if payload.len() >= 2 {
                        let f = u32::from(u16::from_le_bytes([payload[0], payload[1]]));
                        let total = self.root_timeline.frames.len().max(1) as u32;
                        self.current_frame = f.min(total.saturating_sub(1));
                        self.playing = false;
                    }
                }
                0x87 => {
                    // ActionStoreRegister
                    if let Some(&reg) = payload.first()
                        && let Some(top) = stack.last().cloned()
                    {
                        self.avm1_registers[usize::from(reg)] = top;
                    }
                }
                0x88 => {
                    // ActionConstantPool
                    let mut pr = SwfSlice::new(payload);
                    if let Some(count) = pr.u16() {
                        let mut pool = Vec::with_capacity(usize::from(count));
                        for _ in 0..count {
                            if let Some((s, _)) = pr.c_string(self.swf_version) {
                                pool.push(s);
                            } else {
                                break;
                            }
                        }
                        self.avm1_constant_pool = pool;
                    }
                }
                0x8B => {
                    // ActionSetTarget
                    let mut pr = SwfSlice::new(payload);
                    if let Some((t, _)) = pr.c_string(self.swf_version) {
                        self.avm1_target = t;
                    }
                }
                0x8C => {
                    // ActionGotoLabel
                    let mut pr = SwfSlice::new(payload);
                    if let Some((lbl, _)) = pr.c_string(self.swf_version)
                        && let Some(target_f) = self.root_timeline.resolve_label(&lbl)
                    {
                        self.current_frame = target_f;
                        self.playing = false;
                    }
                }
                0x8E => {
                    // ActionDefineFunction2
                    let mut pr = SwfSlice::new(payload);
                    let Some((fn_name, _)) = pr.c_string(self.swf_version) else {
                        continue;
                    };
                    let Some(num_params) = pr.u16() else {
                        continue;
                    };
                    let Some(register_count) = pr.u8() else {
                        continue;
                    };
                    let Some(flags) = pr.u16() else {
                        continue;
                    };
                    let mut params = Vec::with_capacity(usize::from(num_params));
                    for _ in 0..num_params {
                        let Some(reg) = pr.u8() else { break };
                        let Some((pname, _)) = pr.c_string(self.swf_version) else {
                            break;
                        };
                        params.push((reg, pname));
                    }
                    let Some(code_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let body_end = (pc + code_size).min(code.len());
                    let body = code[pc..body_end].to_vec();
                    pc = body_end;
                    let func = Avm1Function {
                        params,
                        register_count,
                        flags,
                        body,
                        constant_pool: self.avm1_constant_pool.clone(),
                        base_target: self.avm1_target.clone(),
                    };
                    if fn_name.is_empty() {
                        let obj_id = self.avm1_objects.len();
                        self.avm1_objects.push(Avm1Object {
                            props: HashMap::new(),
                            array_elements: None,
                            function: Some(func),
                        });
                        stack.push(Avm1Value::Object(obj_id));
                    } else {
                        self.avm1_functions.insert(fn_name, func);
                    }
                }
                0x96 => {
                    // ActionPush
                    let mut pr = SwfSlice::new(payload);
                    while let Some(ptype) = pr.u8() {
                        match ptype {
                            0 => {
                                if let Some((s, _)) = pr.c_string(self.swf_version) {
                                    stack.push(Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            1 => {
                                if let Some(b) = pr.take(4) {
                                    let f = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                                    stack.push(Avm1Value::Number(f64::from(f)));
                                } else {
                                    break;
                                }
                            }
                            2 => stack.push(Avm1Value::Null),
                            3 => stack.push(Avm1Value::Undefined),
                            4 => {
                                if let Some(reg) = pr.u8() {
                                    stack.push(self.avm1_registers[usize::from(reg)].clone());
                                } else {
                                    break;
                                }
                            }
                            5 => {
                                if let Some(b) = pr.u8() {
                                    stack.push(Avm1Value::Bool(b != 0));
                                } else {
                                    break;
                                }
                            }
                            6 => {
                                // SWF double has the two 32-bit words swapped in little-endian!
                                if let Some(b) = pr.take(8) {
                                    let d = f64::from_le_bytes([
                                        b[4], b[5], b[6], b[7], b[0], b[1], b[2], b[3],
                                    ]);
                                    stack.push(Avm1Value::Number(d));
                                } else {
                                    break;
                                }
                            }
                            7 => {
                                if let Some(i) = pr.i32() {
                                    stack.push(Avm1Value::Number(f64::from(i)));
                                } else {
                                    break;
                                }
                            }
                            8 => {
                                if let Some(idx) = pr.u8() {
                                    let s = self
                                        .avm1_constant_pool
                                        .get(usize::from(idx))
                                        .cloned()
                                        .unwrap_or_default();
                                    stack.push(Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            9 => {
                                if let Some(idx) = pr.u16() {
                                    let s = self
                                        .avm1_constant_pool
                                        .get(usize::from(idx))
                                        .cloned()
                                        .unwrap_or_default();
                                    stack.push(Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
                0x99 => {
                    // ActionJump (i16 signed byte offset relative to end of instruction)
                    if payload.len() >= 2 {
                        let offset = i16::from_le_bytes([payload[0], payload[1]]) as isize;
                        let next_pc = (pc as isize) + offset;
                        if next_pc >= 0 && (next_pc as usize) <= code.len() {
                            pc = next_pc as usize;
                        } else {
                            break;
                        }
                    }
                }
                0x9B => {
                    // ActionDefineFunction
                    let mut pr = SwfSlice::new(payload);
                    let Some((fn_name, _)) = pr.c_string(self.swf_version) else {
                        continue;
                    };
                    let Some(num_params) = pr.u16() else {
                        continue;
                    };
                    let mut params = Vec::with_capacity(usize::from(num_params));
                    for _ in 0..num_params {
                        let Some((pname, _)) = pr.c_string(self.swf_version) else {
                            break;
                        };
                        params.push((0, pname));
                    }
                    let Some(code_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let body_end = (pc + code_size).min(code.len());
                    let body = code[pc..body_end].to_vec();
                    pc = body_end;
                    let func = Avm1Function {
                        params,
                        register_count: 0,
                        flags: 0,
                        body,
                        constant_pool: self.avm1_constant_pool.clone(),
                        base_target: self.avm1_target.clone(),
                    };
                    if fn_name.is_empty() {
                        let obj_id = self.avm1_objects.len();
                        self.avm1_objects.push(Avm1Object {
                            props: HashMap::new(),
                            array_elements: None,
                            function: Some(func),
                        });
                        stack.push(Avm1Value::Object(obj_id));
                    } else {
                        self.avm1_functions.insert(fn_name, func);
                    }
                }
                0x9D => {
                    // ActionIf
                    let cond = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    if cond && payload.len() >= 2 {
                        let offset = i16::from_le_bytes([payload[0], payload[1]]) as isize;
                        let next_pc = (pc as isize) + offset;
                        if next_pc >= 0 && (next_pc as usize) <= code.len() {
                            pc = next_pc as usize;
                        } else {
                            break;
                        }
                    }
                }
                0x9F => {
                    // ActionGotoFrame2
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let flags = payload.first().copied().unwrap_or(0);
                    let play_flag = (flags & 0x01) != 0;
                    let bias = if (flags & 0x02) != 0 && payload.len() >= 3 {
                        u32::from(u16::from_le_bytes([payload[1], payload[2]]))
                    } else {
                        0
                    };
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    let resolved = match target {
                        Avm1Value::Number(n) if n >= 1.0 => {
                            let f = (n as i64).saturating_sub(1).max(0) as u32 + bias;
                            Some(f.min(total.saturating_sub(1)))
                        }
                        Avm1Value::String(s) => {
                            if let Ok(num) = s.trim().parse::<u32>()
                                && num >= 1
                            {
                                let f = num.saturating_sub(1) + bias;
                                Some(f.min(total.saturating_sub(1)))
                            } else {
                                self.root_timeline
                                    .resolve_label(&s)
                                    .map(|f| (f + bias).min(total.saturating_sub(1)))
                            }
                        }
                        _ => None,
                    };
                    if let Some(target_f) = resolved {
                        self.current_frame = target_f;
                        self.playing = play_flag;
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn get_avm1_variable(&self, name: &str, locals: &HashMap<String, Avm1Value>) -> Avm1Value {
        if let Some(v) = locals.get(name) {
            return v.clone();
        }
        if let Some(v) = self.avm1_globals.get(name) {
            return v.clone();
        }
        match name {
            "this" | "_root" | "_level0" => Avm1Value::MovieClip("_root".to_owned()),
            "Math" => Avm1Value::MovieClip("Math".to_owned()),
            "_currentframe" => Avm1Value::Number(f64::from(self.current_frame + 1)),
            "_totalframes" => Avm1Value::Number(f64::from(self.root_timeline.total_frames)),
            _ => {
                if self.display_list.values().any(|s| s.name == name) {
                    Avm1Value::MovieClip(name.to_owned())
                } else {
                    Avm1Value::Undefined
                }
            }
        }
    }

    fn set_avm1_variable(
        &mut self,
        name: String,
        val: Avm1Value,
        locals: &mut HashMap<String, Avm1Value>,
    ) {
        if let Some(slot) = locals.get_mut(&name) {
            *slot = val;
        } else {
            self.avm1_globals.insert(name, val);
        }
    }

    fn get_mc_property(&self, target: &str, prop_idx: i32) -> Avm1Value {
        let slot = if target.is_empty() || target == "_root" || target == "/" {
            None
        } else {
            let clean = target.trim_start_matches("_root.").trim_start_matches('/');
            self.display_list.values().find(|s| s.name == clean)
        };
        match prop_idx {
            0 => Avm1Value::Number(slot.map_or(0.0, |s| s.x)),
            1 => Avm1Value::Number(slot.map_or(0.0, |s| s.y)),
            2 => Avm1Value::Number(slot.map_or(100.0, |s| s.scale_x)),
            3 => Avm1Value::Number(slot.map_or(100.0, |s| s.scale_y)),
            4 => Avm1Value::Number(f64::from(
                slot.map_or(self.current_frame + 1, |s| s.current_frame + 1),
            )),
            5 => Avm1Value::Number(f64::from(
                slot.map_or(self.root_timeline.total_frames, |s| s.total_frames),
            )),
            6 => Avm1Value::Number(slot.map_or(100.0, |s| s.alpha)),
            7 => Avm1Value::Bool(slot.is_none_or(|s| s.visible)),
            10 => Avm1Value::Number(slot.map_or(0.0, |s| s.rotation)),
            13 => Avm1Value::String(slot.map_or_else(String::new, |s| s.name.clone())),
            _ => Avm1Value::Number(0.0),
        }
    }

    fn set_mc_property(&mut self, target: &str, prop_idx: i32, val: Avm1Value) {
        let clean = target.trim_start_matches("_root.").trim_start_matches('/');
        let slot_opt = if clean.is_empty() || clean == "_root" {
            self.display_list.values_mut().next()
        } else {
            self.display_list.values_mut().find(|s| s.name == clean)
        };
        if let Some(slot) = slot_opt {
            match prop_idx {
                0 => {
                    slot.x = val.to_number();
                    slot.matrix[4] = slot.x;
                }
                1 => {
                    slot.y = val.to_number();
                    slot.matrix[5] = slot.y;
                }
                2 => slot.scale_x = val.to_number(),
                3 => slot.scale_y = val.to_number(),
                6 => slot.alpha = val.to_number(),
                7 => slot.visible = val.to_bool(self.swf_version),
                10 => slot.rotation = val.to_number(),
                13 => slot.name = val.to_trace_string(self.swf_version, &self.avm1_objects),
                _ => {}
            }
        }
    }

    fn get_avm1_member(&self, target: &Avm1Value, member: &str) -> Avm1Value {
        match target {
            Avm1Value::Object(id) => {
                if let Some(obj) = self.avm1_objects.get(*id) {
                    if let Some(elems) = &obj.array_elements {
                        if member == "length" {
                            return Avm1Value::Number(elems.len() as f64);
                        }
                        if let Ok(idx) = member.parse::<usize>() {
                            return elems.get(idx).cloned().unwrap_or(Avm1Value::Undefined);
                        }
                    }
                    if let Some(v) = obj.props.get(member) {
                        return v.clone();
                    }
                }
                Avm1Value::Undefined
            }
            Avm1Value::String(s) => {
                if member == "length" {
                    Avm1Value::Number(s.chars().count() as f64)
                } else {
                    Avm1Value::Undefined
                }
            }
            Avm1Value::MovieClip(mc) => {
                if mc == "Math" {
                    return match member {
                        "PI" => Avm1Value::Number(std::f64::consts::PI),
                        "E" => Avm1Value::Number(std::f64::consts::E),
                        "SQRT2" => Avm1Value::Number(std::f64::consts::SQRT_2),
                        _ => Avm1Value::Undefined,
                    };
                }
                match member {
                    "_x" => self.get_mc_property(mc, 0),
                    "_y" => self.get_mc_property(mc, 1),
                    "_xscale" => self.get_mc_property(mc, 2),
                    "_yscale" => self.get_mc_property(mc, 3),
                    "_currentframe" => self.get_mc_property(mc, 4),
                    "_totalframes" => self.get_mc_property(mc, 5),
                    "_alpha" => self.get_mc_property(mc, 6),
                    "_visible" => self.get_mc_property(mc, 7),
                    "_rotation" => self.get_mc_property(mc, 10),
                    "_name" => self.get_mc_property(mc, 13),
                    _ => {
                        if (mc == "_root" || mc.is_empty())
                            && let Some(v) = self.avm1_globals.get(member)
                        {
                            return v.clone();
                        }
                        if let Some(slot) = self.display_list.values().find(|s| s.name == *mc)
                            && let Some(v) = slot.props.get(member)
                        {
                            return v.clone();
                        }
                        if self.display_list.values().any(|s| s.name == member) {
                            return Avm1Value::MovieClip(member.to_owned());
                        }
                        Avm1Value::Undefined
                    }
                }
            }
            _ => Avm1Value::Undefined,
        }
    }

    fn set_avm1_member(&mut self, target: &Avm1Value, member: &str, val: Avm1Value) {
        match target {
            Avm1Value::Object(id) => {
                if let Some(obj) = self.avm1_objects.get_mut(*id) {
                    if let Some(elems) = &mut obj.array_elements
                        && let Ok(idx) = member.parse::<usize>()
                        && idx < 4096
                    {
                        if idx >= elems.len() {
                            elems.resize(idx + 1, Avm1Value::Undefined);
                        }
                        elems[idx] = val;
                        return;
                    }
                    obj.props.insert(member.to_owned(), val);
                }
            }
            Avm1Value::MovieClip(mc) => match member {
                "_x" => self.set_mc_property(mc, 0, val),
                "_y" => self.set_mc_property(mc, 1, val),
                "_xscale" => self.set_mc_property(mc, 2, val),
                "_yscale" => self.set_mc_property(mc, 3, val),
                "_alpha" => self.set_mc_property(mc, 6, val),
                "_visible" => self.set_mc_property(mc, 7, val),
                "_rotation" => self.set_mc_property(mc, 10, val),
                "_name" => self.set_mc_property(mc, 13, val),
                _ => {
                    if mc == "_root" || mc.is_empty() {
                        self.avm1_globals.insert(member.to_owned(), val);
                    } else if let Some(slot) =
                        self.display_list.values_mut().find(|s| s.name == *mc)
                    {
                        slot.props.insert(member.to_owned(), val);
                    }
                }
            },
            _ => {}
        }
    }

    fn set_target_playing(&mut self, playing: bool) {
        let t = self
            .avm1_target
            .trim_start_matches("_root.")
            .trim_start_matches('/');
        if !t.is_empty()
            && t != "_root"
            && let Some(slot) = self.display_list.values_mut().find(|s| s.name == t)
        {
            slot.playing = playing;
            return;
        }
        self.playing = playing;
    }

    fn set_target_frame_step(&mut self, delta: i32) {
        let t = self
            .avm1_target
            .trim_start_matches("_root.")
            .trim_start_matches('/');
        if !t.is_empty()
            && t != "_root"
            && let Some(slot) = self.display_list.values_mut().find(|s| s.name == t)
        {
            let next = (slot.current_frame as i32 + delta)
                .clamp(0, slot.total_frames.saturating_sub(1) as i32) as u32;
            slot.current_frame = next;
            slot.playing = false;
            return;
        }
        let total = self.root_timeline.frames.len().max(1) as u32;
        let next =
            (self.current_frame as i32 + delta).clamp(0, total.saturating_sub(1) as i32) as u32;
        self.current_frame = next;
        self.playing = false;
    }

    fn call_avm1_function(
        &mut self,
        fn_name: &str,
        args: Vec<Avm1Value>,
        _caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let target_clean = self
            .avm1_target
            .trim_start_matches("_root.")
            .trim_start_matches('/');
        if !target_clean.is_empty()
            && target_clean != "_root"
            && let Some(Avm1Value::Object(id)) = self
                .display_list
                .values()
                .find(|s| s.name == target_clean)
                .and_then(|s| s.props.get(fn_name).cloned())
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func(&func, args, steps, depth);
        }
        if let Some(func) = self.avm1_functions.get(fn_name).cloned() {
            return self.invoke_avm1_func(&func, args, steps, depth);
        }
        if let Some(Avm1Value::Object(id)) = self.avm1_globals.get(fn_name).cloned()
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func(&func, args, steps, depth);
        }
        match fn_name {
            "trace" => {
                if let Some(first) = args.first() {
                    let s = first.to_trace_string(self.swf_version, &self.avm1_objects);
                    self.record_trace(s);
                }
                Avm1Value::Undefined
            }
            "int" | "parseInt" => Avm1Value::Number(f64::from(
                args.first().unwrap_or(&Avm1Value::Undefined).to_i32(),
            )),
            "Number" | "parseFloat" => {
                Avm1Value::Number(args.first().unwrap_or(&Avm1Value::Undefined).to_number())
            }
            "String" => Avm1Value::String(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_trace_string(self.swf_version, &self.avm1_objects),
            ),
            "Boolean" => Avm1Value::Bool(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_bool(self.swf_version),
            ),
            "stop" => {
                self.set_target_playing(false);
                Avm1Value::Undefined
            }
            "play" => {
                self.set_target_playing(true);
                Avm1Value::Undefined
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    let resolved = match first {
                        Avm1Value::Number(n) if *n >= 1.0 => {
                            let f = (*n as i64).saturating_sub(1).max(0) as u32;
                            Some(f.min(total.saturating_sub(1)))
                        }
                        Avm1Value::String(lbl) => {
                            if let Ok(num) = lbl.trim().parse::<u32>()
                                && num >= 1
                            {
                                Some(num.saturating_sub(1).min(total.saturating_sub(1)))
                            } else {
                                self.root_timeline.resolve_label(lbl)
                            }
                        }
                        _ => None,
                    };
                    if let Some(f) = resolved {
                        self.current_frame = f;
                        self.playing = fn_name == "gotoAndPlay";
                    }
                }
                Avm1Value::Undefined
            }
            _ => Avm1Value::Undefined,
        }
    }

    fn call_avm1_method(
        &mut self,
        receiver: &Avm1Value,
        method_name: &str,
        args: Vec<Avm1Value>,
        caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match receiver {
            Avm1Value::Object(id) => {
                if method_name.is_empty()
                    && let Some(func) = self.avm1_objects.get(*id).and_then(|o| o.function.clone())
                {
                    return self.invoke_avm1_func(&func, args, steps, depth);
                }
                if let Some(Avm1Value::Object(fn_id)) = self
                    .avm1_objects
                    .get(*id)
                    .and_then(|o| o.props.get(method_name).cloned())
                    && let Some(func) = self
                        .avm1_objects
                        .get(fn_id)
                        .and_then(|o| o.function.clone())
                {
                    return self.invoke_avm1_func(&func, args, steps, depth);
                }
                if let Some(obj) = self.avm1_objects.get_mut(*id)
                    && let Some(elems) = &mut obj.array_elements
                {
                    match method_name {
                        "push" => {
                            for a in args {
                                elems.push(a);
                            }
                            return Avm1Value::Number(elems.len() as f64);
                        }
                        "pop" => {
                            return elems.pop().unwrap_or(Avm1Value::Undefined);
                        }
                        "join" => {
                            let sep = args.first().map_or_else(
                                || ",".to_owned(),
                                |v| v.to_trace_string(self.swf_version, &[]),
                            );
                            let parts: Vec<String> = elems
                                .iter()
                                .map(|v| v.to_trace_string(self.swf_version, &[]))
                                .collect();
                            return Avm1Value::String(parts.join(&sep));
                        }
                        _ => {}
                    }
                }
                Avm1Value::Undefined
            }
            Avm1Value::MovieClip(mc) => {
                if mc == "Math" {
                    let a = args.first().map_or(f64::NAN, Avm1Value::to_number);
                    let b = args.get(1).map_or(f64::NAN, Avm1Value::to_number);
                    return match method_name {
                        "abs" => Avm1Value::Number(a.abs()),
                        "floor" => Avm1Value::Number(a.floor()),
                        "ceil" => Avm1Value::Number(a.ceil()),
                        "round" => Avm1Value::Number(a.round()),
                        "sqrt" => Avm1Value::Number(a.sqrt()),
                        "sin" => Avm1Value::Number(a.sin()),
                        "cos" => Avm1Value::Number(a.cos()),
                        "min" => Avm1Value::Number(a.min(b)),
                        "max" => Avm1Value::Number(a.max(b)),
                        "pow" => Avm1Value::Number(a.powf(b)),
                        _ => Avm1Value::Undefined,
                    };
                }
                if mc == "_root" || mc.is_empty() {
                    return self.call_avm1_function(method_name, args, caller_locals, steps, depth);
                }
                if let Some(slot) = self.display_list.values_mut().find(|s| s.name == *mc) {
                    match method_name {
                        "stop" => slot.playing = false,
                        "play" => slot.playing = true,
                        "gotoAndStop" | "gotoAndPlay" => {
                            if let Some(first) = args.first() {
                                let f = (first.to_i32() - 1).max(0) as u32;
                                slot.current_frame = f.min(slot.total_frames.saturating_sub(1));
                                slot.playing = method_name == "gotoAndPlay";
                            }
                        }
                        _ => {}
                    }
                }
                Avm1Value::Undefined
            }
            Avm1Value::String(s) => match method_name {
                "toUpperCase" => Avm1Value::String(s.to_uppercase()),
                "toLowerCase" => Avm1Value::String(s.to_lowercase()),
                "charAt" => {
                    let idx = args.first().map_or(0, Avm1Value::to_i32).max(0) as usize;
                    Avm1Value::String(
                        s.chars()
                            .nth(idx)
                            .map_or_else(String::new, |c| c.to_string()),
                    )
                }
                _ => Avm1Value::Undefined,
            },
            _ => self.call_avm1_function(method_name, args, caller_locals, steps, depth),
        }
    }

    fn invoke_avm1_func(
        &mut self,
        func: &Avm1Function,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let saved_pool =
            std::mem::replace(&mut self.avm1_constant_pool, func.constant_pool.clone());
        let saved_regs = self.avm1_registers.clone();
        let saved_target = std::mem::replace(&mut self.avm1_target, func.base_target.clone());
        let mut fn_locals = HashMap::new();
        for (i, (reg, name)) in func.params.iter().enumerate() {
            let val = args.get(i).cloned().unwrap_or(Avm1Value::Undefined);
            if *reg != 0 {
                self.avm1_registers[usize::from(*reg)] = val.clone();
            }
            if !name.is_empty() {
                fn_locals.insert(name.clone(), val);
            }
        }
        let mut fn_stack = Vec::new();
        let ret = self
            .run_avm1_code(&func.body, &mut fn_stack, &mut fn_locals, steps, depth + 1)
            .unwrap_or(Avm1Value::Undefined);
        self.avm1_constant_pool = saved_pool;
        self.avm1_registers = saved_regs;
        self.avm1_target = saved_target;
        ret
    }

    // =========================================================================
    // AVM2 Bytecode Interpreter
    // =========================================================================

    fn initialize_avm2(&mut self) {
        let abc_count = self.abc_tags.len();
        for abc_idx in 0..abc_count {
            // 1. Run script initializers (`script.init`)
            let script_inits: Vec<usize> = self.abc_tags[abc_idx]
                .abc
                .scripts
                .iter()
                .map(|(init, _)| *init)
                .collect();
            for init_m in script_inits {
                self.execute_avm2_method(abc_idx, init_m, Vec::new());
            }

            // 2. Run class static initializers (`cinit`) and main timeline / document class constructor (`iinit`)
            let class_count = self.abc_tags[abc_idx].abc.classes.len();
            for c_idx in 0..class_count {
                let cinit = self.abc_tags[abc_idx].abc.classes[c_idx].cinit;
                self.execute_avm2_method(abc_idx, cinit, Vec::new());

                let (pkg, cls_name) = self.abc_tags[abc_idx]
                    .abc
                    .split_qname(self.abc_tags[abc_idx].abc.instances[c_idx].name);
                let full_name = if pkg.is_empty() {
                    cls_name.clone()
                } else {
                    format!("{pkg}.{cls_name}")
                };
                let is_doc_or_timeline = self.symbol_classes.get(&0) == Some(&full_name)
                    || cls_name == "MainTimeline"
                    || class_count == 1;
                if is_doc_or_timeline {
                    let iinit = self.abc_tags[abc_idx].abc.instances[c_idx].iinit;
                    self.execute_avm2_method(abc_idx, iinit, Vec::new());
                }
            }
        }
    }

    fn execute_avm2_method(
        &mut self,
        abc_idx: usize,
        method_idx: usize,
        args: Vec<Avm1Value>,
    ) -> Avm1Value {
        let mut steps = 0usize;
        self.run_avm2_method_bounded(abc_idx, method_idx, args, &mut steps, 0)
    }

    fn run_avm2_method_bounded(
        &mut self,
        abc_idx: usize,
        method_idx: usize,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if depth > MAX_CALL_DEPTH {
            return Avm1Value::Undefined;
        }
        let Some(code) = self
            .abc_tags
            .get(abc_idx)
            .and_then(|l| l.abc.bodies_by_method.get(&method_idx))
            .map(|b| b.code.clone())
        else {
            return Avm1Value::Undefined;
        };

        let mut locals: Vec<Avm1Value> = vec![Avm1Value::Undefined; 64];
        locals[0] = Avm1Value::MovieClip("_root".to_owned());
        for (i, arg) in args.into_iter().enumerate() {
            if i + 1 < locals.len() {
                locals[i + 1] = arg;
            }
        }
        let mut stack: Vec<Avm1Value> = Vec::new();
        let mut r = AbcReader::new(&code);

        while r.pos < code.len() && *steps < MAX_VM_STEPS_PER_FRAME {
            *steps += 1;
            let Some(op) = r.u8() else { break };
            match op {
                0x02 => {} // nop
                0x09 => {} // label
                0x10 => {
                    // jump
                    let Some(rel) = r.s24() else { break };
                    let target = (r.pos as isize) + (rel as isize);
                    if target >= 0 && (target as usize) <= code.len() {
                        r.pos = target as usize;
                    } else {
                        break;
                    }
                }
                0x11 => {
                    // iftrue
                    let Some(rel) = r.s24() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    if v {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x12 => {
                    // iffalse
                    let Some(rel) = r.s24() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    if !v {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x13..=0x1A => {
                    // ifeq (0x13), ifne (0x14), iflt (0x15), ifnlt (0x16), ifle (0x17), ifnle (0x18), ifgt (0x19), ifngt (0x1A)
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    let cond = match op {
                        0x13 => avm1_equals(&a, &b, 9, &self.avm1_objects),
                        0x14 => !avm1_equals(&a, &b, 9, &self.avm1_objects),
                        0x15 => a.to_number() < b.to_number(),
                        0x16 => !(a.to_number() < b.to_number()),
                        0x17 => a.to_number() <= b.to_number(),
                        0x18 => !(a.to_number() <= b.to_number()),
                        0x19 => a.to_number() > b.to_number(),
                        _ => !(a.to_number() > b.to_number()),
                    };
                    if cond {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x1B => {
                    // ifge
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    if a >= b {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x1C => {
                    // ifnge
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    if !(a >= b) {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x1D => {
                    // popscope
                }
                0x20 => stack.push(Avm1Value::Null),
                0x21 => stack.push(Avm1Value::Undefined),
                0x24 => {
                    // pushbyte
                    let Some(b) = r.i8() else { break };
                    stack.push(Avm1Value::Number(f64::from(b)));
                }
                0x25 => {
                    // pushshort
                    let Some(v) = r.s32() else { break };
                    stack.push(Avm1Value::Number(f64::from(v)));
                }
                0x26 => stack.push(Avm1Value::Bool(true)),
                0x27 => stack.push(Avm1Value::Bool(false)),
                0x28 => stack.push(Avm1Value::Number(f64::NAN)),
                0x29 => {
                    let _ = stack.pop();
                }
                0x2A => {
                    let top = stack.last().cloned().unwrap_or(Avm1Value::Undefined);
                    stack.push(top);
                }
                0x2B => {
                    let len = stack.len();
                    if len >= 2 {
                        stack.swap(len - 1, len - 2);
                    }
                }
                0x2C => {
                    // pushstring
                    let Some(idx) = r.u30() else { break };
                    let s = self.abc_tags[abc_idx].abc.string(idx).to_owned();
                    stack.push(Avm1Value::String(s));
                }
                0x2D => {
                    // pushint
                    let Some(idx) = r.u30() else { break };
                    let v = self.abc_tags[abc_idx]
                        .abc
                        .ints
                        .get(idx)
                        .copied()
                        .unwrap_or(0);
                    stack.push(Avm1Value::Number(f64::from(v)));
                }
                0x2E => {
                    // pushuint
                    let Some(idx) = r.u30() else { break };
                    let v = self.abc_tags[abc_idx]
                        .abc
                        .uints
                        .get(idx)
                        .copied()
                        .unwrap_or(0);
                    stack.push(Avm1Value::Number(f64::from(v)));
                }
                0x2F => {
                    // pushdouble
                    let Some(idx) = r.u30() else { break };
                    let v = self.abc_tags[abc_idx]
                        .abc
                        .doubles
                        .get(idx)
                        .copied()
                        .unwrap_or(f64::NAN);
                    stack.push(Avm1Value::Number(v));
                }
                0x30 => {
                    // pushscope
                    let _ = stack.pop();
                }
                0x40 => {
                    // newfunction
                    let Some(midx) = r.u30() else { break };
                    stack.push(Avm1Value::String(format!("__avm2_fn:{abc_idx}:{midx}")));
                }
                0x41 => {
                    // call
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let _receiver = stack.pop();
                    let fn_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret = self.invoke_avm2_callable(&fn_val, cargs, steps, depth);
                    stack.push(ret);
                }
                0x42 => {
                    // construct
                    let Some(argc) = r.u30() else { break };
                    for _ in 0..argc {
                        let _ = stack.pop();
                    }
                    let ctor = stack.pop().unwrap_or(Avm1Value::Undefined);
                    stack.push(ctor);
                }
                0x45 | 0x4E => {
                    // callsuper (0x45), callsupervoid (0x4E)
                    let Some(_mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    for _ in 0..argc {
                        let _ = stack.pop();
                    }
                    let _receiver = stack.pop();
                    if op == 0x45 {
                        stack.push(Avm1Value::Undefined);
                    }
                }
                0x46 | 0x4C | 0x4F => {
                    // callproperty (0x46), callproplex (0x4C), callpropvoid (0x4F)
                    let Some(mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let (_, prop_name) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret = self
                        .call_avm2_property(abc_idx, &receiver, &prop_name, cargs, steps, depth);
                    if op != 0x4F {
                        stack.push(ret);
                    }
                }
                0x47 => {
                    // returnvoid
                    return Avm1Value::Undefined;
                }
                0x48 => {
                    // returnvalue
                    return stack.pop().unwrap_or(Avm1Value::Undefined);
                }
                0x49 => {
                    // constructsuper
                    let Some(argc) = r.u30() else { break };
                    for _ in 0..argc {
                        let _ = stack.pop();
                    }
                    let _ = stack.pop();
                }
                0x4A => {
                    // constructprop
                    let Some(mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let (_, cls_name) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let _receiver = stack.pop();
                    let obj_id = self.avm1_objects.len();
                    let mut obj = Avm1Object::default();
                    if cls_name == "Array" {
                        obj.array_elements = Some(cargs);
                    }
                    self.avm1_objects.push(obj);
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x55 => {
                    // newobject
                    let Some(count) = r.u30() else { break };
                    let mut props = HashMap::new();
                    for _ in 0..count {
                        let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                        let key = stack
                            .pop()
                            .unwrap_or(Avm1Value::Undefined)
                            .to_trace_string(9, &self.avm1_objects);
                        props.insert(key, val);
                    }
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props,
                        array_elements: None,
                        function: None,
                    });
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x56 => {
                    // newarray
                    let Some(count) = r.u30() else { break };
                    let mut elems = Vec::with_capacity(count.min(1024));
                    for _ in 0..count {
                        elems.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    elems.reverse();
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        props: HashMap::new(),
                        array_elements: Some(elems),
                        function: None,
                    });
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x57 => {
                    // newactivation
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object::default());
                    stack.push(Avm1Value::Object(obj_id));
                }
                0x58 => {
                    // newclass
                    let Some(_class_idx) = r.u30() else { break };
                    let base = stack.pop().unwrap_or(Avm1Value::Undefined);
                    stack.push(base);
                }
                0x5D | 0x5E => {
                    // findpropstrict / findproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let (_, name) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    stack.push(Avm1Value::MovieClip(name));
                }
                0x60 => {
                    // getlex
                    let Some(mn_idx) = r.u30() else { break };
                    let (_, name) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let val = self.get_avm2_property(&Avm1Value::MovieClip("_root".into()), &name);
                    stack.push(val);
                }
                0x61 | 0x68 => {
                    // setproperty / initproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (_, has_rt_name) =
                        self.abc_tags[abc_idx].abc.runtime_multiname_parts(mn_idx);
                    let prop_name = if has_rt_name {
                        stack
                            .pop()
                            .unwrap_or(Avm1Value::Undefined)
                            .to_trace_string(9, &self.avm1_objects)
                    } else {
                        self.abc_tags[abc_idx].abc.split_qname(mn_idx).1
                    };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.set_avm2_property(&obj, &prop_name, val);
                }
                0x62 => {
                    // getlocal
                    let Some(reg) = r.u30() else { break };
                    stack.push(locals.get(reg).cloned().unwrap_or(Avm1Value::Undefined));
                }
                0x63 => {
                    // setlocal
                    let Some(reg) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if reg < locals.len() {
                        locals[reg] = val;
                    }
                }
                0x64 => {
                    // getglobalscope
                    stack.push(Avm1Value::MovieClip("_root".into()));
                }
                0x65 => {
                    // getscopeobject
                    let Some(_idx) = r.u8() else { break };
                    stack.push(Avm1Value::MovieClip("_root".into()));
                }
                0x66 => {
                    // getproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let (_, has_rt_name) =
                        self.abc_tags[abc_idx].abc.runtime_multiname_parts(mn_idx);
                    let prop_name = if has_rt_name {
                        stack
                            .pop()
                            .unwrap_or(Avm1Value::Undefined)
                            .to_trace_string(9, &self.avm1_objects)
                    } else {
                        self.abc_tags[abc_idx].abc.split_qname(mn_idx).1
                    };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = self.get_avm2_property(&obj, &prop_name);
                    stack.push(val);
                }
                0x67 => {
                    // getouterscope
                    let Some(_idx) = r.u30() else { break };
                    stack.push(Avm1Value::MovieClip("_root".into()));
                }
                0x6C => {
                    // getslot
                    let Some(slot_idx) = r.u30() else { break };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__slot_{slot_idx}");
                    let val = self.get_avm2_property(&obj, &key);
                    stack.push(val);
                }
                0x6D => {
                    // setslot
                    let Some(slot_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__slot_{slot_idx}");
                    self.set_avm2_property(&obj, &key, val);
                }
                0x6E => {
                    // getglobalslot
                    let Some(slot_idx) = r.u30() else { break };
                    let key = format!("__gslot_{slot_idx}");
                    let val = self
                        .avm2_globals
                        .get(&key)
                        .cloned()
                        .unwrap_or(Avm1Value::Undefined);
                    stack.push(val);
                }
                0x6F => {
                    // setglobalslot
                    let Some(slot_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__gslot_{slot_idx}");
                    self.avm2_globals.insert(key, val);
                }
                0x70 | 0x85 => {
                    // convert_s / coerce_s
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(9, &self.avm1_objects);
                    stack.push(Avm1Value::String(s));
                }
                0x73 => {
                    // convert_i
                    let i = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(i)));
                }
                0x74 => {
                    // convert_u
                    let u = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32() as u32;
                    stack.push(Avm1Value::Number(f64::from(u)));
                }
                0x75 => {
                    // convert_d
                    let d = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(d));
                }
                0x76 => {
                    // convert_b
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    stack.push(Avm1Value::Bool(b));
                }
                0x80 => {
                    // coerce
                    let Some(_mn) = r.u30() else { break };
                }
                0x82 => {
                    // coerce_a
                }
                0x90 | 0xC4 => {
                    // negate / negate_i
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(-n));
                }
                0x91 | 0xC0 => {
                    // increment / increment_i
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(n + 1.0));
                }
                0x92 | 0xC2 => {
                    // inclocal / inclocal_i
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        *slot = Avm1Value::Number(slot.to_number() + 1.0);
                    }
                }
                0x93 | 0xC1 => {
                    // decrement / decrement_i
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(n - 1.0));
                }
                0x94 | 0xC3 => {
                    // declocal / declocal_i
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        *slot = Avm1Value::Number(slot.to_number() - 1.0);
                    }
                }
                0x96 => {
                    // not
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    stack.push(Avm1Value::Bool(!b));
                }
                0xA0 => {
                    // add
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if matches!(a, Avm1Value::String(_)) || matches!(b, Avm1Value::String(_)) {
                        let sa = a.to_trace_string(9, &self.avm1_objects);
                        let sb = b.to_trace_string(9, &self.avm1_objects);
                        stack.push(Avm1Value::String(format!("{sa}{sb}")));
                    } else {
                        stack.push(Avm1Value::Number(a.to_number() + b.to_number()));
                    }
                }
                0xA1 | 0xC6 => {
                    // subtract / subtract_i
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(a - b));
                }
                0xA2 | 0xC7 => {
                    // multiply / multiply_i
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(a * b));
                }
                0xA3 => {
                    // divide
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(a / b));
                }
                0xA4 => {
                    // modulo
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Number(a % b));
                }
                0xA5 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(a.wrapping_shl(b as u32))));
                }
                0xA6 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(a.wrapping_shr(b as u32))));
                }
                0xA7 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32() as u32;
                    stack.push(Avm1Value::Number(f64::from(a.wrapping_shr(b as u32))));
                }
                0xA8 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(a & b)));
                }
                0xA9 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(a | b)));
                }
                0xAA => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(a ^ b)));
                }
                0xAB | 0xAC => {
                    // equals / strictequals
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    stack.push(Avm1Value::Bool(avm1_equals(&a, &b, 9, &self.avm1_objects)));
                }
                0xAD => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Bool(a < b));
                }
                0xAE => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Bool(a <= b));
                }
                0xAF => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Bool(a > b));
                }
                0xB0 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    stack.push(Avm1Value::Bool(a >= b));
                }
                0xC5 => {
                    // add_i
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    stack.push(Avm1Value::Number(f64::from(a.wrapping_add(b))));
                }
                0xD0..=0xD3 => {
                    let idx = usize::from(op - 0xD0);
                    stack.push(locals.get(idx).cloned().unwrap_or(Avm1Value::Undefined));
                }
                0xD4..=0xD7 => {
                    let idx = usize::from(op - 0xD4);
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    locals[idx] = val;
                }
                0xEF => {
                    // debug
                    let _ = r.u8();
                    let _ = r.u30();
                    let _ = r.u8();
                    let _ = r.u30();
                }
                0xF0 | 0xF1 => {
                    // debugline / debugfile
                    let _ = r.u30();
                }
                _ => break,
            }
        }
        Avm1Value::Undefined
    }

    fn get_avm2_property(&self, obj: &Avm1Value, prop: &str) -> Avm1Value {
        if let Avm1Value::Object(_id) = obj {
            return self.get_avm1_member(obj, prop);
        }
        if let Some(v) = self.avm2_globals.get(prop) {
            return v.clone();
        }
        // Check if `prop` is a method on the document/timeline class
        for (abc_idx, loaded) in self.abc_tags.iter().enumerate() {
            for inst in &loaded.abc.instances {
                for tr in &inst.traits {
                    let (_, tname) = loaded.abc.split_qname(tr.name);
                    if tname == prop
                        && let AbcTraitData::Method { method } | AbcTraitData::Function { method } =
                            tr.data
                    {
                        return Avm1Value::String(format!("__avm2_fn:{abc_idx}:{method}"));
                    }
                }
            }
        }
        match prop {
            "currentFrame" => Avm1Value::Number(f64::from(self.current_frame + 1)),
            "totalFrames" => Avm1Value::Number(f64::from(self.root_timeline.total_frames)),
            "Math" => Avm1Value::MovieClip("Math".into()),
            _ => {
                if self.display_list.values().any(|s| s.name == prop) {
                    Avm1Value::MovieClip(prop.to_owned())
                } else {
                    Avm1Value::Undefined
                }
            }
        }
    }

    fn set_avm2_property(&mut self, obj: &Avm1Value, prop: &str, val: Avm1Value) {
        match obj {
            Avm1Value::Object(_) => self.set_avm1_member(obj, prop, val),
            Avm1Value::MovieClip(mc) if mc != "_root" && !mc.is_empty() && mc != prop => match prop
            {
                "x" => self.set_mc_property(mc, 0, val),
                "y" => self.set_mc_property(mc, 1, val),
                "scaleX" => self.set_mc_property(mc, 2, Avm1Value::Number(val.to_number() * 100.0)),
                "scaleY" => self.set_mc_property(mc, 3, Avm1Value::Number(val.to_number() * 100.0)),
                "alpha" => self.set_mc_property(mc, 6, Avm1Value::Number(val.to_number() * 100.0)),
                "visible" => self.set_mc_property(mc, 7, val),
                "rotation" => self.set_mc_property(mc, 10, val),
                _ => {
                    self.avm2_globals.insert(prop.to_owned(), val);
                }
            },
            _ => {
                self.avm2_globals.insert(prop.to_owned(), val);
            }
        }
    }

    fn call_avm2_property(
        &mut self,
        abc_idx: usize,
        receiver: &Avm1Value,
        prop: &str,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match prop {
            "trace" => {
                let line = args
                    .iter()
                    .map(|a| a.to_trace_string(9, &self.avm1_objects))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.record_trace(line);
                return Avm1Value::Undefined;
            }
            "addFrameScript" => {
                for pair in args.as_chunks::<2>().0 {
                    let f_idx = pair[0].to_i32().max(0) as u32;
                    if let Avm1Value::String(s) = &pair[1]
                        && let Some((a_idx, m_idx)) = parse_avm2_fn_token(s)
                    {
                        self.avm2_frame_scripts.insert(f_idx, (a_idx, m_idx));
                    }
                }
                return Avm1Value::Undefined;
            }
            "stop" => {
                self.playing = false;
                return Avm1Value::Undefined;
            }
            "play" => {
                self.playing = true;
                return Avm1Value::Undefined;
            }
            "nextFrame" => {
                let total = self.root_timeline.frames.len().max(1) as u32;
                self.current_frame = (self.current_frame + 1).min(total.saturating_sub(1));
                self.playing = false;
                return Avm1Value::Undefined;
            }
            "prevFrame" => {
                self.current_frame = self.current_frame.saturating_sub(1);
                self.playing = false;
                return Avm1Value::Undefined;
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    let resolved = match first {
                        Avm1Value::Number(n) if *n >= 1.0 => {
                            let f = (*n as i64).saturating_sub(1).max(0) as u32;
                            Some(f.min(total.saturating_sub(1)))
                        }
                        Avm1Value::String(lbl) => self.root_timeline.resolve_label(lbl),
                        _ => None,
                    };
                    if let Some(f) = resolved {
                        self.current_frame = f;
                        self.playing = prop == "gotoAndPlay";
                    }
                }
                return Avm1Value::Undefined;
            }
            _ => {}
        }

        // Check if `prop` is a method on the class in `abc_idx`
        if let Some(loaded) = self.abc_tags.get(abc_idx) {
            for inst in &loaded.abc.instances {
                for tr in &inst.traits {
                    let (_, tname) = loaded.abc.split_qname(tr.name);
                    if tname == prop
                        && let AbcTraitData::Method { method } | AbcTraitData::Function { method } =
                            tr.data
                    {
                        return self.run_avm2_method_bounded(
                            abc_idx,
                            method,
                            args,
                            steps,
                            depth + 1,
                        );
                    }
                }
            }
            for cls in &loaded.abc.classes {
                for tr in &cls.traits {
                    let (_, tname) = loaded.abc.split_qname(tr.name);
                    if tname == prop
                        && let AbcTraitData::Method { method } | AbcTraitData::Function { method } =
                            tr.data
                    {
                        return self.run_avm2_method_bounded(
                            abc_idx,
                            method,
                            args,
                            steps,
                            depth + 1,
                        );
                    }
                }
            }
        }

        let mut dummy_locals = HashMap::new();
        self.call_avm1_method(receiver, prop, args, &mut dummy_locals, steps, depth)
    }

    fn invoke_avm2_callable(
        &mut self,
        fn_val: &Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::String(s) = fn_val
            && let Some((a_idx, m_idx)) = parse_avm2_fn_token(s)
        {
            return self.run_avm2_method_bounded(a_idx, m_idx, args, steps, depth + 1);
        }
        Avm1Value::Undefined
    }
}

fn parse_avm2_fn_token(s: &str) -> Option<(usize, usize)> {
    let rest = s.strip_prefix("__avm2_fn:")?;
    let (a, m) = rest.split_once(':')?;
    Some((a.parse().ok()?, m.parse().ok()?))
}

fn parse_timeline_tags(bytes: &[u8], swf_version: u8, declared_frames: u16) -> TimelineDef {
    let mut frames = Vec::new();
    let mut cur_tags = Vec::new();
    let mut labels = Vec::new();
    walk_swf_tags(bytes, |code, payload| {
        match code {
            0 => return false,
            43 => {
                let mut lr = SwfSlice::new(payload);
                if let Some((lbl, _)) = lr.c_string(swf_version) {
                    labels.push((lbl, frames.len() as u32));
                }
                cur_tags.push(RawFrameTag {
                    code,
                    payload: payload.to_vec(),
                });
            }
            1 => {
                frames.push(std::mem::take(&mut cur_tags));
            }
            4 | 5 | 12 | 15 | 18 | 19 | 26 | 28 | 45 | 70 => {
                cur_tags.push(RawFrameTag {
                    code,
                    payload: payload.to_vec(),
                });
            }
            _ => {}
        }
        true
    });
    if !cur_tags.is_empty() || frames.is_empty() {
        frames.push(cur_tags);
    }
    let total_frames = (frames.len() as u32).max(u32::from(declared_frames).max(1));
    TimelineDef {
        total_frames,
        frames,
        labels,
    }
}

fn parse_button_actions(tag_code: u16, payload: &[u8]) -> Option<(u16, Vec<ButtonCondAction>)> {
    let mut r = SwfSlice::new(payload);
    let btn_id = r.u16()?;
    if tag_code == 7 {
        for _ in 0..1024 {
            let flags = r.u8()?;
            if flags == 0 {
                break;
            }
            let _cid = r.u16()?;
            let _depth = r.u16()?;
            let _mat = r.matrix_px()?;
        }
        let actions = r.rest().to_vec();
        return Some((
            btn_id,
            vec![ButtonCondAction {
                conditions: 0,
                actions,
            }],
        ));
    }
    let _flags = r.u8()?;
    let off_pos = r.pos();
    let rel = usize::from(r.u16()?);
    if rel == 0 || off_pos + rel >= payload.len() {
        return Some((btn_id, Vec::new()));
    }
    let mut cond_slice = &payload[off_pos + rel..];
    let mut conds = Vec::new();
    while cond_slice.len() >= 4 {
        let size = usize::from(u16::from_le_bytes([cond_slice[0], cond_slice[1]]));
        let conditions = u16::from_le_bytes([cond_slice[2], cond_slice[3]]);
        if size == 0 || size > cond_slice.len() {
            conds.push(ButtonCondAction {
                conditions,
                actions: cond_slice[4..].to_vec(),
            });
            break;
        }
        conds.push(ButtonCondAction {
            conditions,
            actions: cond_slice[4..size].to_vec(),
        });
        cond_slice = &cond_slice[size..];
    }
    Some((btn_id, conds))
}

fn parse_raw_clip_handlers(r: &mut SwfSlice<'_>, swf_version: u8) -> Vec<ClipEventHandler> {
    let mut out = Vec::new();
    let _reserved = r.u16();
    let _all_flags = if swf_version >= 6 {
        r.u32()
    } else {
        r.u16().map(u32::from)
    };
    for _ in 0..256 {
        let Some(event_flags) = (if swf_version >= 6 {
            r.u32()
        } else {
            r.u16().map(u32::from)
        }) else {
            break;
        };
        if event_flags == 0 {
            break;
        }
        let Some(rec_size) = r.u32().map(|n| n as usize) else {
            break;
        };
        let has_key = (event_flags & 0x0002_0000) != 0; // ClipEventKeyPress
        let key_code = if has_key && rec_size >= 1 {
            r.u8()
        } else {
            None
        };
        let action_len = if has_key {
            rec_size.saturating_sub(1)
        } else {
            rec_size
        };
        let Some(actions) = r.take(action_len) else {
            break;
        };
        out.push(ClipEventHandler {
            event_flags,
            key_code,
            actions: actions.to_vec(),
        });
    }
    out
}

fn skip_filter_bytes(r: &mut SwfSlice<'_>) {
    let Some(count) = r.u8() else { return };
    for _ in 0..count {
        let Some(fid) = r.u8() else { return };
        match fid {
            0 => {
                let _ = r.take(23);
            }
            1 => {
                let _ = r.take(9);
            }
            2 => {
                let _ = r.take(15);
            }
            3 => {
                let _ = r.take(27);
            }
            4 | 7 => {
                if let Some(n) = r.u8().map(usize::from) {
                    let _ = r.take(n * 5 + 19);
                }
            }
            5 => {
                if let (Some(mx), Some(my)) = (r.u8().map(usize::from), r.u8().map(usize::from)) {
                    let _ = r.take(13 + 4 * mx * my);
                }
            }
            6 => {
                let _ = r.take(80);
            }
            _ => return,
        }
    }
}

fn map_blend(b: u8) -> Option<BlendMode> {
    match b {
        0 | 1 => Some(BlendMode::Normal),
        2 => Some(BlendMode::Layer),
        3 => Some(BlendMode::Multiply),
        4 => Some(BlendMode::Screen),
        5 => Some(BlendMode::Lighten),
        6 => Some(BlendMode::Darken),
        7 => Some(BlendMode::Difference),
        8 => Some(BlendMode::Add),
        9 => Some(BlendMode::Subtract),
        10 => Some(BlendMode::Invert),
        11 => Some(BlendMode::Alpha),
        12 => Some(BlendMode::Erase),
        13 => Some(BlendMode::Overlay),
        14 => Some(BlendMode::Hardlight),
        _ => None,
    }
}

fn format_avm_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_owned()
    } else if n.is_infinite() {
        if n.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        }
    } else if n == 0.0 {
        "0".to_owned()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn avm1_equals(a: &Avm1Value, b: &Avm1Value, swf_version: u8, objects: &[Avm1Object]) -> bool {
    match (a, b) {
        (Avm1Value::Undefined | Avm1Value::Null, Avm1Value::Undefined | Avm1Value::Null) => true,
        (Avm1Value::Number(x), Avm1Value::Number(y)) => (x - y).abs() < 1e-12,
        (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
        (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
        (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
        (Avm1Value::MovieClip(x), Avm1Value::MovieClip(y)) => x == y,
        _ => {
            let na = a.to_number();
            let nb = b.to_number();
            if !na.is_nan() && !nb.is_nan() {
                (na - nb).abs() < 1e-12
            } else {
                a.to_trace_string(swf_version, objects) == b.to_trace_string(swf_version, objects)
            }
        }
    }
}

pub fn play_swf_path(path: &Path, max_frames: usize) -> Result<SwfPlaybackResult, ImportError> {
    let mut player = SwfBytecodePlayer::from_path(path)?;
    Ok(player.run_frames(max_frames))
}

pub fn play_swf_bytes(bytes: &[u8], max_frames: usize) -> Result<SwfPlaybackResult, ImportError> {
    let mut player = SwfBytecodePlayer::from_bytes(bytes)?;
    Ok(player.run_frames(max_frames))
}

fn walk_swf_tags(bytes: &[u8], mut on_tag: impl FnMut(u16, &[u8]) -> bool) {
    let mut pos = 0usize;
    while pos + 2 <= bytes.len() {
        let code_and_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        pos += 2;
        let code = code_and_len >> 6;
        let short_len = usize::from(code_and_len & 0x3f);
        let len = if short_len == 0x3f {
            if pos + 4 > bytes.len() {
                break;
            }
            let l = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                as usize;
            pos += 4;
            l
        } else {
            short_len
        };
        let end = pos.saturating_add(len).min(bytes.len());
        let payload = &bytes[pos..end];
        pos = end;
        if !on_tag(code, payload) {
            break;
        }
    }
}
