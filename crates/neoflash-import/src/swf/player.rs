//! Deterministic, bounded SWF timeline player and AVM1/AVM2 bytecode interpreter.
//!
//! Executes SWF frame tags (`PlaceObject*`, `RemoveObject*`, `DoAction`, `DoInitAction`,
//! `DoABC`/`DoABC2`, `StartSound`, `SoundStream*`, `DefineSprite`, `DefineButton*`)
//! step-by-step without external runtimes, recording display list states, `trace()`
//! output, and audio events across frames.

use super::avm2::{AbcFile, AbcReader, AbcTraitData, parse_abc_file};
use super::bits::SwfSlice;
use super::timeline::parse_swf_filter_list;
use super::{SwfContainer, read_container, read_container_from_bytes};
use crate::ImportError;
use neoflash_core::{
    AuthoredScene, BlendMode, ColorTransform, Element, ElementKind, FilterSpec, InstancePlayback,
    Keyframe, Layer, LayerProperties, LayerRole, MaskMode, Project, SoundCommand, SoundSync,
    Symbol, SymbolKind, Timeline, Transform, evaluate_scene_with_symbol_overrides,
    transform_matrix,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Read,
    path::Path,
};

const MAX_VM_STEPS_PER_FRAME: usize = 50_000;
const MAX_CALL_DEPTH: usize = 32;
const MAX_AVM_STACK: usize = 4096;
const MAX_RUNTIME_SPRITE_DEPTH: usize = 64;
const MAX_FRAME_JUMPS_PER_STEP: usize = 32;

const CLIP_EVENT_LOAD: u32 = 0x0000_0001;
const CLIP_EVENT_ENTER_FRAME: u32 = 0x0000_0002;
const CLIP_EVENT_UNLOAD: u32 = 0x0000_0004;
const CLIP_EVENT_MOUSE_DOWN: u32 = 0x0000_0010;
const CLIP_EVENT_MOUSE_UP: u32 = 0x0000_0020;
const CLIP_EVENT_INITIALIZE: u32 = 0x0000_0200;
const CLIP_EVENT_PRESS: u32 = 0x0000_0400;
const CLIP_EVENT_RELEASE: u32 = 0x0000_0800;
const CLIP_EVENT_KEY_PRESS: u32 = 0x0002_0000;
const CLIP_EVENT_CONSTRUCT: u32 = 0x0004_0000;

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
    #[serde(default)]
    pub skew: f64,
    pub alpha: f64,
    #[serde(default, skip_serializing_if = "ColorTransform::is_identity")]
    pub color_transform: ColorTransform,
    pub visible: bool,
    pub blend_mode: BlendMode,
    pub filters: Vec<FilterSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip_depth: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<u16>,
    #[serde(default)]
    pub cache_as_bitmap: bool,
    pub current_frame: u32,
    pub total_frames: u32,
    pub playing: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SwfDisplayEntry>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_diagnostics: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_diagnostics: Vec<String>,
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
enum DetachedAvm1Item {
    Bytes(String, Vec<u8>),
    Callback(String, usize),
}

#[derive(Clone, Debug)]
enum PendingAvm1Work {
    Action {
        target: String,
        actions: Vec<u8>,
    },
    Callback {
        target: String,
        fn_obj_id: usize,
    },
    Detached {
        root_target: String,
        root_slot: Box<LiveSlot>,
        actions: Vec<DetachedAvm1Item>,
    },
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
    skew: f64,
    alpha: f64,
    color_transform: ColorTransform,
    visible: bool,
    blend_mode: BlendMode,
    filters: Vec<FilterSpec>,
    clip_depth: Option<u16>,
    masked_by: Option<String>,
    is_dynamic: bool,
    ratio: Option<u16>,
    cache_as_bitmap: bool,
    current_frame: u32,
    total_frames: u32,
    playing: bool,
    last_applied_frame: Option<u32>,
    children: BTreeMap<u16, LiveSlot>,
    props: HashMap<String, Avm1Value>,
    clip_handlers: Vec<ClipEventHandler>,
}

impl LiveSlot {
    fn sync_matrix_from_props(&mut self) {
        let tf = Transform {
            x: self.x,
            y: self.y,
            scale_x: self.scale_x / 100.0,
            scale_y: self.scale_y / 100.0,
            rotation: self.rotation,
            alpha: (self.alpha / 100.0).clamp(0.0, 1.0),
            skew: self.skew,
            color_transform: self.color_transform,
            pivot: None,
            primitive: None,
        };
        self.matrix = transform_matrix(tf);
    }

    fn to_display_entry(&self, depth: u16) -> SwfDisplayEntry {
        SwfDisplayEntry {
            depth,
            character_id: self.character_id,
            name: self.name.clone(),
            matrix: self.matrix,
            x: self.x,
            y: self.y,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            rotation: self.rotation,
            skew: self.skew,
            alpha: self.alpha,
            color_transform: self.color_transform,
            visible: self.visible,
            blend_mode: self.blend_mode,
            filters: self.filters.clone(),
            clip_depth: self.clip_depth,
            masked_by: self.masked_by.clone(),
            ratio: self.ratio,
            cache_as_bitmap: self.cache_as_bitmap,
            current_frame: self.current_frame,
            total_frames: self.total_frames,
            playing: self.playing,
            children: self
                .children
                .iter()
                .map(|(&cd, cs)| cs.to_display_entry(cd))
                .collect(),
        }
    }
}

fn live_slot_segment(depth: u16, _slot: &LiveSlot) -> String {
    format!("depth{depth}")
}

fn join_target_path(parent: &str, segment: &str) -> String {
    if parent.is_empty() || parent == "_root" || parent == "/" {
        segment.to_owned()
    } else {
        format!("{}/{segment}", parent.trim_end_matches('/'))
    }
}

fn target_segments(target: &str) -> Vec<&str> {
    target
        .trim()
        .trim_start_matches('/')
        .split(['/', '.'])
        .filter(|segment| !segment.is_empty() && *segment != "_root" && *segment != "_level0")
        .collect()
}

fn slot_matches_segment(depth: u16, slot: &LiveSlot, segment: &str) -> bool {
    slot.name == segment || segment == format!("depth{depth}")
}

fn resolve_target_identity_by_segments(
    display_list: &BTreeMap<u16, LiveSlot>,
    segments: &[&str],
    parent_identity: &str,
) -> Option<String> {
    let (segment, rest) = segments.split_first()?;
    let (depth, slot) = display_list
        .iter()
        .find(|(depth, slot)| slot_matches_segment(**depth, slot, segment))?;
    let identity = join_target_path(parent_identity, &format!("depth{depth}"));
    if rest.is_empty() {
        Some(identity)
    } else {
        resolve_target_identity_by_segments(&slot.children, rest, &identity)
    }
}

fn resolve_target_identity(display_list: &BTreeMap<u16, LiveSlot>, target: &str) -> Option<String> {
    let segments = target_segments(target);
    resolve_target_identity_by_segments(display_list, &segments, "")
}

fn identity_depth(segment: &str) -> Option<u16> {
    segment.strip_prefix("depth")?.parse().ok()
}

fn find_slot_by_identity_segments<'a>(
    display_list: &'a BTreeMap<u16, LiveSlot>,
    segments: &[&str],
) -> Option<&'a LiveSlot> {
    let (segment, rest) = segments.split_first()?;
    let slot = display_list.get(&identity_depth(segment)?)?;
    if rest.is_empty() {
        Some(slot)
    } else {
        find_slot_by_identity_segments(&slot.children, rest)
    }
}

fn find_slot_by_identity_segments_mut<'a>(
    display_list: &'a mut BTreeMap<u16, LiveSlot>,
    segments: &[&str],
) -> Option<&'a mut LiveSlot> {
    let (segment, rest) = segments.split_first()?;
    let slot = display_list.get_mut(&identity_depth(segment)?)?;
    if rest.is_empty() {
        Some(slot)
    } else {
        find_slot_by_identity_segments_mut(&mut slot.children, rest)
    }
}

fn find_slot_by_identity<'a>(
    display_list: &'a BTreeMap<u16, LiveSlot>,
    target: &str,
) -> Option<&'a LiveSlot> {
    let segments = target_segments(target);
    find_slot_by_identity_segments(display_list, &segments)
}

fn find_slot_by_identity_mut<'a>(
    display_list: &'a mut BTreeMap<u16, LiveSlot>,
    target: &str,
) -> Option<&'a mut LiveSlot> {
    let segments = target_segments(target);
    find_slot_by_identity_segments_mut(display_list, &segments)
}

fn find_slot_in_detached<'a>(
    root_target: &str,
    root_slot: &'a LiveSlot,
    target: &str,
) -> Option<&'a LiveSlot> {
    if target == root_target {
        return Some(root_slot);
    }
    let suffix = target.strip_prefix(root_target)?.strip_prefix('/')?;
    find_slot_by_identity(&root_slot.children, suffix)
}

fn find_slot_in_detached_mut<'a>(
    root_target: &str,
    root_slot: &'a mut LiveSlot,
    target: &str,
) -> Option<&'a mut LiveSlot> {
    if target == root_target {
        return Some(root_slot);
    }
    let suffix = target.strip_prefix(root_target)?.strip_prefix('/')?;
    find_slot_by_identity_mut(&mut root_slot.children, suffix)
}

fn collect_pointer_actions(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_target: &str,
    buttons: &HashMap<u16, Vec<ButtonCondAction>>,
    actions: &mut Vec<(String, Vec<u8>)>,
) {
    for (&depth, slot) in display_list {
        let target = join_target_path(parent_target, &live_slot_segment(depth, slot));
        if let Some(conditions) = buttons.get(&slot.character_id) {
            actions.extend(
                conditions
                    .iter()
                    .filter(|condition| {
                        condition.conditions == 0 || (condition.conditions & 0x000C) != 0
                    })
                    .map(|condition| (target.clone(), condition.actions.clone())),
            );
        }
        actions.extend(
            slot.clip_handlers
                .iter()
                .filter(|handler| {
                    (handler.event_flags
                        & (CLIP_EVENT_MOUSE_UP
                            | CLIP_EVENT_MOUSE_DOWN
                            | CLIP_EVENT_RELEASE
                            | CLIP_EVENT_PRESS))
                        != 0
                })
                .map(|handler| (target.clone(), handler.actions.clone())),
        );
        collect_pointer_actions(&slot.children, &target, buttons, actions);
    }
}

fn collect_unload_actions(slot: &LiveSlot, target: &str, actions: &mut Vec<DetachedAvm1Item>) {
    for (&depth, child) in &slot.children {
        let child_target = join_target_path(target, &live_slot_segment(depth, child));
        collect_unload_actions(child, &child_target, actions);
    }
    actions.extend(
        slot.clip_handlers
            .iter()
            .filter(|handler| (handler.event_flags & CLIP_EVENT_UNLOAD) != 0)
            .map(|handler| DetachedAvm1Item::Bytes(target.to_owned(), handler.actions.clone())),
    );
    if let Some(Avm1Value::Object(fn_id)) = slot.props.get("onUnload") {
        actions.push(DetachedAvm1Item::Callback(target.to_owned(), *fn_id));
    }
}

fn detached_unload_work(slot: &LiveSlot, target: &str) -> Option<PendingAvm1Work> {
    let mut actions = Vec::new();
    collect_unload_actions(slot, target, &mut actions);
    (!actions.is_empty()).then(|| PendingAvm1Work::Detached {
        root_target: target.to_owned(),
        root_slot: Box::new(slot.clone()),
        actions,
    })
}

fn pending_action(target: String, actions: Vec<u8>) -> PendingAvm1Work {
    PendingAvm1Work::Action { target, actions }
}

fn prune_dangling_masks_in_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    root_display_list: &BTreeMap<u16, LiveSlot>,
) {
    let depths: Vec<u16> = display_list.keys().copied().collect();
    for depth in depths {
        let clear_mask = display_list
            .get(&depth)
            .and_then(|s| s.masked_by.as_deref())
            .is_some_and(|mask_id| find_slot_by_identity(root_display_list, mask_id).is_none());
        if let Some(slot) = display_list.get_mut(&depth) {
            if clear_mask {
                slot.masked_by = None;
            }
            prune_dangling_masks_in_map(&mut slot.children, root_display_list);
        }
    }
}

fn ecma_f64_to_i32(n: f64) -> i32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let int = n.trunc();
    let two32 = 4_294_967_296.0_f64;
    let mut m = int % two32;
    if m < 0.0 {
        m += two32;
    }
    (m as u32) as i32
}

fn ecma_f64_to_u32(n: f64) -> u32 {
    ecma_f64_to_i32(n) as u32
}

fn any_playing_descendant(display_list: &BTreeMap<u16, LiveSlot>) -> bool {
    display_list.values().any(|slot| {
        (slot.playing && slot.total_frames > 1) || any_playing_descendant(&slot.children)
    })
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
    MovieClipIdentity(String),
    Super {
        this_val: Box<Avm1Value>,
        super_ctor: Option<usize>,
    },
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
            Self::Object(_)
            | Self::MovieClip(_)
            | Self::MovieClipIdentity(_)
            | Self::Super { .. } => f64::NAN,
        }
    }

    fn to_i32(&self) -> i32 {
        ecma_f64_to_i32(self.to_number())
    }

    fn to_u32(&self) -> u32 {
        ecma_f64_to_u32(self.to_number())
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
            Self::Object(_)
            | Self::MovieClip(_)
            | Self::MovieClipIdentity(_)
            | Self::Super { .. } => true,
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
            Self::MovieClip(path) | Self::MovieClipIdentity(path) => path.clone(),
            Self::Super { .. } => "[object Object]".to_owned(),
            Self::Object(id) => {
                if let Some(obj) = objects.get(*id) {
                    if let Some(elements) = &obj.array_elements {
                        return elements
                            .iter()
                            .map(|v| v.to_trace_string(swf_version, objects))
                            .collect::<Vec<_>>()
                            .join(",");
                    }
                    if obj.function.is_some() {
                        return "[type Function]".to_owned();
                    }
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
    interfaces: Vec<usize>,
    avm2_class: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
struct Avm1Function {
    params: Vec<(u8, String)>,
    register_count: u8,
    flags: u16,
    is_v2: bool,
    body: Vec<u8>,
    constant_pool: Vec<String>,
    base_target: String,
    self_obj_id: Option<usize>,
}

#[derive(Clone, Debug)]
struct LoadedAbc {
    abc: AbcFile,
}

#[derive(Clone, Debug)]
struct RootClipProps {
    x: f64,
    y: f64,
    scale_x: f64,
    scale_y: f64,
    alpha: f64,
    visible: bool,
    rotation: f64,
    name: String,
}

impl Default for RootClipProps {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 100.0,
            scale_y: 100.0,
            alpha: 100.0,
            visible: true,
            rotation: 0.0,
            name: String::new(),
        }
    }
}

pub struct SwfBytecodePlayer {
    swf_version: u8,
    vm_kind: SwfVmKind,
    stage_offset: (f64, f64),
    root_timeline: TimelineDef,
    sprites: HashMap<u16, TimelineDef>,
    buttons: HashMap<u16, Vec<ButtonCondAction>>,
    exports: HashMap<String, u16>,
    init_actions: Vec<(u16, Vec<u8>)>,
    abc_tags: Vec<LoadedAbc>,
    symbol_classes: HashMap<u16, String>,
    scenes: Vec<(u32, String)>,

    // Runtime state
    current_frame: u32,
    presented_frame: u32,
    playing: bool,
    executed_frames: u32,
    last_applied_frame: Option<u32>,
    root_loop_rewind_pending: bool,
    root_props: RootClipProps,
    display_list: BTreeMap<u16, LiveSlot>,
    trace_output: Vec<String>,
    frame_traces: Vec<String>,
    audio_events: Vec<SwfAudioEvent>,
    frame_audio_events: Vec<SwfAudioEvent>,
    unsupported_diagnostics: Vec<String>,
    frame_unsupported_diagnostics: Vec<String>,
    stream_sound_active: bool,
    stop_all_sounds_pending: bool,

    // AVM1 state
    avm1_globals: HashMap<String, Avm1Value>,
    avm1_global_obj: HashMap<String, Avm1Value>,
    avm1_functions: HashMap<String, Avm1Function>,
    avm1_objects: Vec<Avm1Object>,
    avm1_registers: [Avm1Value; 256],
    avm1_constant_pool: Vec<String>,
    avm1_target: String,
    avm1_detached_target: Option<(String, LiveSlot)>,
    avm1_with_scopes: Vec<Avm1Value>,
    avm1_thrown: Option<Avm1Value>,
    drag_target: Option<String>,
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
        let bounds = r
            .rect_twips()
            .ok_or_else(|| ImportError::Container("truncated SWF stage RECT".into()))?;
        let stage_dx = if bounds[1] < bounds[0] {
            0.0
        } else {
            -(f64::from(bounds[0]) / 20.0)
        };
        let stage_dy = if bounds[3] < bounds[2] {
            0.0
        } else {
            -(f64::from(bounds[2]) / 20.0)
        };
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
        let mut exports: HashMap<String, u16> = HashMap::new();
        let mut init_actions: Vec<(u16, Vec<u8>)> = Vec::new();
        let mut abc_tags: Vec<LoadedAbc> = Vec::new();
        let mut symbol_classes: HashMap<u16, String> = HashMap::new();
        let mut scenes: Vec<(u32, String)> = Vec::new();

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
                56 => {
                    // ExportAssets
                    let mut er = SwfSlice::new(payload);
                    let count = er.u16().unwrap_or(0);
                    for _ in 0..count {
                        let Some(tag_id) = er.u16() else { break };
                        let Some((name, _)) = er.c_string(container.swf_version) else {
                            break;
                        };
                        exports.insert(name, tag_id);
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
                86 => {
                    // DefineSceneAndFrameLabelData
                    let mut sr = AbcReader::new(payload);
                    if let Some(scene_count) = sr.u30() {
                        for _ in 0..scene_count.min(1024) {
                            let Some(off) = sr.u30() else { break };
                            let Some(nul_pos) = payload[sr.pos..].iter().position(|&b| b == 0)
                            else {
                                break;
                            };
                            let name = String::from_utf8_lossy(&payload[sr.pos..sr.pos + nul_pos])
                                .into_owned();
                            sr.pos += nul_pos + 1;
                            scenes.push((off as u32, name));
                        }
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
            stage_offset: (stage_dx, stage_dy),
            root_timeline: TimelineDef {
                total_frames,
                frames: root_frames,
                labels: root_labels,
            },
            sprites,
            buttons,
            exports,
            init_actions,
            abc_tags,
            symbol_classes,
            scenes,
            current_frame: 0,
            presented_frame: 0,
            playing: true,
            executed_frames: 0,
            last_applied_frame: None,
            root_loop_rewind_pending: false,
            root_props: RootClipProps::default(),
            display_list: BTreeMap::new(),
            trace_output: Vec::new(),
            frame_traces: Vec::new(),
            audio_events: Vec::new(),
            frame_audio_events: Vec::new(),
            unsupported_diagnostics: Vec::new(),
            frame_unsupported_diagnostics: Vec::new(),
            stream_sound_active: false,
            stop_all_sounds_pending: false,
            avm1_globals: HashMap::new(),
            avm1_global_obj: HashMap::new(),
            avm1_functions: HashMap::new(),
            avm1_objects: Vec::new(),
            avm1_registers: std::array::from_fn(|_| Avm1Value::Undefined),
            avm1_constant_pool: Vec::new(),
            avm1_target: String::new(),
            avm1_detached_target: None,
            avm1_with_scopes: Vec::new(),
            avm1_thrown: None,
            drag_target: None,
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

    /// Frame index whose display list and scripts were most recently evaluated.
    pub fn presented_frame(&self) -> u32 {
        self.presented_frame
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    pub fn trace_output(&self) -> &[String] {
        &self.trace_output
    }

    pub fn unsupported_diagnostics(&self) -> &[String] {
        &self.unsupported_diagnostics
    }

    pub fn display_list(&self) -> Vec<SwfDisplayEntry> {
        self.display_list
            .iter()
            .map(|(&depth, s)| s.to_display_entry(depth))
            .collect()
    }

    /// Reset all runtime display-list and VM state back to the initial unstepped frame 0 state.
    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.presented_frame = 0;
        self.playing = true;
        self.executed_frames = 0;
        self.last_applied_frame = None;
        self.root_loop_rewind_pending = false;
        self.root_props = RootClipProps::default();
        self.display_list.clear();
        self.trace_output.clear();
        self.frame_traces.clear();
        self.audio_events.clear();
        self.frame_audio_events.clear();
        self.unsupported_diagnostics.clear();
        self.frame_unsupported_diagnostics.clear();
        self.stream_sound_active = false;
        self.stop_all_sounds_pending = false;
        self.avm1_globals.clear();
        self.avm1_global_obj.clear();
        self.avm1_functions.clear();
        self.avm1_objects.clear();
        self.avm1_registers = std::array::from_fn(|_| Avm1Value::Undefined);
        self.avm1_constant_pool.clear();
        self.avm1_target.clear();
        self.avm1_detached_target = None;
        self.avm1_with_scopes.clear();
        self.avm1_thrown = None;
        self.drag_target = None;
        self.rng_state = 0x1234_5678;
        self.avm2_initialized = false;
        self.avm2_globals.clear();
        self.avm2_frame_scripts.clear();
    }

    /// Deterministically replay the SWF timeline from frame 0 through `target_frame` (inclusive),
    /// returning the `SwfFrameState` at `target_frame`.
    pub fn seek_to_frame(&mut self, target_frame: u32) -> SwfFrameState {
        let total = self.root_timeline.frames.len().max(1) as u32;
        let clamped = target_frame.min(total.saturating_sub(1));
        self.reset();
        let mut last_state = None;
        for f in 0..=clamped {
            self.current_frame = f;
            last_state = Some(self.step_frame_inner(0, f == clamped));
        }
        let mut state = last_state.unwrap_or_else(|| SwfFrameState {
            frame: 0,
            label: None,
            playing: self.playing,
            display_list: Vec::new(),
            traces: Vec::new(),
            audio_events: Vec::new(),
            unsupported_diagnostics: Vec::new(),
        });
        state.unsupported_diagnostics = self.unsupported_diagnostics.clone();
        state
    }

    /// Drain pending audio events from the most recent frame step into host `SoundCommand`s.
    pub fn drain_sound_commands(&mut self, project: &Project) -> Vec<SoundCommand> {
        let mut out = Vec::new();
        if self.stop_all_sounds_pending {
            self.stop_all_sounds_pending = false;
            out.push(SoundCommand::StopAll);
        }
        let events = std::mem::take(&mut self.frame_audio_events);
        for ev in events {
            if ev.sound_id > 0 {
                let resource = format!("swf-sound-{}", ev.sound_id);
                if project.resources.iter().any(|r| r.id == resource) {
                    if ev.sync == "stop" {
                        out.push(SoundCommand::Stop { resource });
                    } else {
                        out.push(SoundCommand::Play {
                            resource,
                            sync: if ev.sync == "start" {
                                SoundSync::Start
                            } else {
                                SoundSync::Event
                            },
                            loops: u32::from(ev.loops.max(1)),
                            loop_forever: false,
                            envelope: Vec::new(),
                            in_point: None,
                            out_point: None,
                        });
                    }
                }
            } else if ev.sync == "stream"
                && let Some(res) = project
                    .resources
                    .iter()
                    .find(|r| r.id.starts_with("swf-stream-"))
            {
                out.push(SoundCommand::StreamStart {
                    channel: "swf-root-stream".to_owned(),
                    resource: res.id.clone(),
                    offset: f64::from(ev.frame) / f64::from(project.fps.max(1)),
                    loops: 1,
                    loop_forever: false,
                    envelope: Vec::new(),
                    in_point: None,
                    out_point: None,
                });
            }
        }
        out
    }

    /// Build the transient runtime `Symbol` override table for `symbol_id` and any nested
    /// `SwfSprite` instances without cloning `project` (`T-230`).
    pub fn build_display_list_symbol_overrides(
        &self,
        project: &Project,
        symbol_id: &str,
        frame: u32,
    ) -> HashMap<String, Symbol> {
        let mut overrides = HashMap::new();
        let entries = self.display_list();
        let mut next_runtime_symbol = 0usize;
        let instance_symbols = self.materialize_runtime_symbol_overrides(
            project,
            &mut overrides,
            &entries,
            "",
            0,
            &mut next_runtime_symbol,
        );
        let offset = if symbol_id == project.root {
            self.stage_offset
        } else {
            (0.0, 0.0)
        };
        if let Some(target_sym) = Self::build_overridden_symbol(
            project,
            &overrides,
            symbol_id,
            symbol_id,
            frame,
            &entries,
            "",
            offset,
            &instance_symbols,
            true,
        ) {
            overrides.insert(symbol_id.to_owned(), target_sym);
        }
        overrides
    }

    /// Project the current runtime display-list state onto an ephemeral clone of `project`
    /// without mutating the authored `Project`.
    pub fn project_with_display_list_overlay(
        &self,
        project: &Project,
        symbol_id: &str,
        frame: u32,
    ) -> Project {
        let mut overlay = project.clone();
        let mut overrides = self.build_display_list_symbol_overrides(project, symbol_id, frame);
        if let Some(target_sym) = overrides.remove(symbol_id)
            && let Some(slot) = overlay.symbol_mut(symbol_id)
        {
            *slot = target_sym;
        }
        let mut extra: Vec<(String, Symbol)> = overrides.into_iter().collect();
        extra.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, sym) in extra {
            overlay.symbols.push(sym);
        }
        if overlay.uses_format_7() && overlay.format_version < 7 {
            overlay.format_version = 7;
        }
        if overlay.uses_format_8() && overlay.format_version < 8 {
            overlay.format_version = 8;
        }
        if overlay.uses_format_9() && overlay.format_version < 9 {
            overlay.format_version = 9;
        }
        overlay
    }

    /// Evaluate the native Stage `AuthoredScene` for the current `SwfBytecodePlayer` display-list
    /// state directly against `project` + transient symbol overrides without cloning `project`
    /// (`T-230`).
    pub fn evaluate_stage_scene(
        &self,
        project: &Project,
        symbol_id: &str,
        frame: u32,
        show_masking: bool,
    ) -> AuthoredScene {
        let overrides = self.build_display_list_symbol_overrides(project, symbol_id, frame);
        evaluate_scene_with_symbol_overrides(
            project,
            &overrides,
            symbol_id,
            frame,
            true,
            show_masking,
        )
    }

    /// Synthesize transient runtime `Symbol` overrides per active `SwfSprite` instance without
    /// cloning `project` (`T-230`). Reusing the authored `swf-sym-*` symbol would make two
    /// instances of the same character overwrite each other's frame and descendants.
    fn materialize_runtime_symbol_overrides(
        &self,
        project: &Project,
        overrides: &mut HashMap<String, Symbol>,
        entries: &[SwfDisplayEntry],
        parent_identity: &str,
        runtime_depth: usize,
        next_runtime_symbol: &mut usize,
    ) -> BTreeMap<u16, String> {
        let mut symbols = BTreeMap::new();
        for entry in entries {
            let entry_identity =
                join_target_path(parent_identity, &format!("depth{}", entry.depth));
            if entry.character_id == 0 {
                if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH {
                    continue;
                }
                let runtime_symbol_id = loop {
                    *next_runtime_symbol = next_runtime_symbol.saturating_add(1);
                    let candidate = format!("swf-runtime-overlay-{}", *next_runtime_symbol);
                    if project.symbol(&candidate).is_none() && !overrides.contains_key(&candidate) {
                        break candidate;
                    }
                };
                let template_id = format!("{runtime_symbol_id}-base");
                overrides.insert(
                    template_id.clone(),
                    Symbol {
                        id: template_id.clone(),
                        name: if entry.name.is_empty() {
                            format!("DynamicClip {}", entry.depth)
                        } else {
                            entry.name.clone()
                        },
                        kind: SymbolKind::MovieClip,
                        folder: String::new(),
                        class_name: None,
                        linkage_id: None,
                        timeline: Timeline {
                            frame_count: 1,
                            layers: vec![Layer {
                                id: format!("{template_id}-empty"),
                                name: "Layer 1".into(),
                                visible: true,
                                locked: false,
                                keyframes: vec![Keyframe {
                                    frame: 0,
                                    ..Default::default()
                                }],
                                properties: LayerProperties::default(),
                            }],
                            scripts: Vec::new(),
                            guides: Vec::new(),
                        },
                    },
                );
                let child_symbols = self.materialize_runtime_symbol_overrides(
                    project,
                    overrides,
                    &entry.children,
                    &entry_identity,
                    runtime_depth + 1,
                    next_runtime_symbol,
                );
                let built = Self::build_overridden_symbol(
                    project,
                    overrides,
                    &template_id,
                    &runtime_symbol_id,
                    entry.current_frame,
                    &entry.children,
                    &entry_identity,
                    (0.0, 0.0),
                    &child_symbols,
                    false,
                );
                overrides.remove(&template_id);
                if let Some(mut runtime_symbol) = built {
                    runtime_symbol.class_name = None;
                    runtime_symbol.linkage_id = None;
                    overrides.insert(runtime_symbol_id.clone(), runtime_symbol);
                    symbols.insert(entry.depth, runtime_symbol_id);
                }
                continue;
            }

            let base_symbol_id = format!("swf-sym-{}", entry.character_id);
            let has_dynamic_children = !entry.children.is_empty();
            if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH
                || (!self.sprites.contains_key(&entry.character_id) && !has_dynamic_children)
            {
                symbols.insert(entry.depth, base_symbol_id);
                continue;
            }
            if project.symbol(&base_symbol_id).is_none() && !overrides.contains_key(&base_symbol_id)
            {
                continue;
            }
            let runtime_symbol_id = loop {
                *next_runtime_symbol = next_runtime_symbol.saturating_add(1);
                let candidate = format!("swf-runtime-overlay-{}", *next_runtime_symbol);
                if project.symbol(&candidate).is_none() && !overrides.contains_key(&candidate) {
                    break candidate;
                }
            };

            let child_symbols = self.materialize_runtime_symbol_overrides(
                project,
                overrides,
                &entry.children,
                &entry_identity,
                runtime_depth + 1,
                next_runtime_symbol,
            );
            if let Some(mut runtime_symbol) = Self::build_overridden_symbol(
                project,
                overrides,
                &base_symbol_id,
                &runtime_symbol_id,
                entry.current_frame,
                &entry.children,
                &entry_identity,
                (0.0, 0.0),
                &child_symbols,
                false,
            ) {
                runtime_symbol.class_name = None;
                runtime_symbol.linkage_id = None;
                overrides.insert(runtime_symbol_id.clone(), runtime_symbol);
                symbols.insert(entry.depth, runtime_symbol_id);
            }
        }
        symbols
    }

    #[allow(clippy::too_many_arguments)]
    fn build_overridden_symbol(
        project: &Project,
        overrides: &HashMap<String, Symbol>,
        source_symbol_id: &str,
        target_symbol_id: &str,
        frame: u32,
        entries: &[SwfDisplayEntry],
        parent_identity: &str,
        offset: (f64, f64),
        instance_symbols: &BTreeMap<u16, String>,
        match_existing_layer_elements: bool,
    ) -> Option<Symbol> {
        let base_sym = overrides
            .get(source_symbol_id)
            .or_else(|| project.symbol(source_symbol_id))?;
        let mut layers: Vec<Layer> = Vec::with_capacity(entries.len().max(1));
        let mut depth_to_layer_idx: BTreeMap<u16, usize> = BTreeMap::new();
        let mut identity_to_layer_idx: HashMap<String, usize> = HashMap::new();
        let mut layer_char_sym_ids: Vec<String> = Vec::with_capacity(entries.len());
        let mut mask_requests: Vec<(u16, u16, usize)> = Vec::new();
        let mut dynamic_mask_requests: Vec<(usize, String)> = Vec::new();

        for entry in entries {
            let char_sym_id = instance_symbols
                .get(&entry.depth)
                .cloned()
                .unwrap_or_else(|| format!("swf-sym-{}", entry.character_id));
            let Some(child_sym) = overrides
                .get(&char_sym_id)
                .or_else(|| project.symbol(&char_sym_id))
            else {
                continue;
            };
            let playback = if let Some(ratio) = entry.ratio
                && child_sym.kind == SymbolKind::Graphic
                && child_sym.timeline.frame_count > 1
            {
                let max_idx = child_sym.timeline.frame_count.saturating_sub(1);
                let morph_frame = ((u32::from(ratio) * max_idx + 32767) / 65535).min(max_idx);
                InstancePlayback::SingleFrame(morph_frame)
            } else {
                let max_f = child_sym.timeline.frame_count.max(1);
                InstancePlayback::SingleFrame(entry.current_frame.min(max_f.saturating_sub(1)))
            };
            let depth_layer_id = format!("{target_symbol_id}-d{}", entry.depth);
            let existing_elem_id = if match_existing_layer_elements {
                base_sym
                    .timeline
                    .layers
                    .iter()
                    .find(|l| l.id == depth_layer_id)
                    .and_then(|layer| {
                        layer
                            .key_at(frame)
                            .and_then(|k| k.elements.first())
                            .or_else(|| layer.keyframes.iter().flat_map(|k| &k.elements).next())
                    })
                    .map(|e| e.id.clone())
            } else {
                None
            }
            .unwrap_or_else(|| format!("swf-el-{target_symbol_id}-d{}-s1", entry.depth));
            let el = Element {
                id: existing_elem_id,
                name: entry.name.clone(),
                visible: entry.visible,
                kind: ElementKind::Instance {
                    symbol_id: char_sym_id.clone(),
                    playback,
                },
                transform: Transform {
                    x: entry.x + offset.0,
                    y: entry.y + offset.1,
                    scale_x: entry.scale_x / 100.0,
                    scale_y: entry.scale_y / 100.0,
                    rotation: entry.rotation,
                    alpha: (entry.alpha / 100.0).clamp(0.0, 1.0),
                    skew: entry.skew,
                    color_transform: entry.color_transform,
                    pivot: None,
                    primitive: None,
                },
                filters: entry.filters.clone(),
                blend_mode: entry.blend_mode,
                cache_as_bitmap: entry.cache_as_bitmap,
                scripts: Vec::new(),
                accessibility: None,
                ..Element::default()
            };
            let mut keyframes = Vec::with_capacity(2);
            if frame > 0 {
                keyframes.push(Keyframe {
                    frame: 0,
                    ..Default::default()
                });
            }
            keyframes.push(Keyframe {
                frame,
                elements: vec![el],
                ..Default::default()
            });
            let layer_idx = layers.len();
            layers.push(Layer {
                id: depth_layer_id,
                name: format!("Layer {}", layer_idx + 1),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            });
            layer_char_sym_ids.push(char_sym_id);
            depth_to_layer_idx.insert(entry.depth, layer_idx);
            let entry_identity =
                join_target_path(parent_identity, &format!("depth{}", entry.depth));
            identity_to_layer_idx.insert(entry_identity, layer_idx);
            if let Some(clip_end) = entry.clip_depth
                && clip_end > entry.depth
            {
                mask_requests.push((entry.depth, clip_end, layer_idx));
            }
            if let Some(mask_id) = &entry.masked_by {
                dynamic_mask_requests.push((layer_idx, mask_id.clone()));
            }
        }

        for (mask_depth, clip_end, mask_lidx) in mask_requests {
            if layers[mask_lidx].properties.mask.is_some() {
                continue;
            }
            let Some(mask_mode) = runtime_symbol_mask_mode(
                project,
                overrides,
                &layer_char_sym_ids[mask_lidx],
                &mut std::collections::HashSet::new(),
            ) else {
                continue;
            };
            let masked_indices: Vec<usize> = depth_to_layer_idx
                .range((
                    std::ops::Bound::Excluded(mask_depth),
                    std::ops::Bound::Included(clip_end),
                ))
                .map(|(_, &idx)| idx)
                .filter(|&idx| {
                    layers[idx].properties.role == LayerRole::Normal
                        && layers[idx].properties.mask.is_none()
                })
                .collect();
            if masked_indices.is_empty() {
                continue;
            }
            let mask_id = layers[mask_lidx].id.clone();
            layers[mask_lidx].properties.role = LayerRole::Mask;
            layers[mask_lidx].properties.mask_mode = mask_mode;
            for idx in masked_indices {
                layers[idx].properties.mask = Some(mask_id.clone());
            }
        }

        for (maskee_lidx, mask_ident) in dynamic_mask_requests {
            let Some(&mask_lidx) = identity_to_layer_idx.get(&mask_ident) else {
                continue;
            };
            if mask_lidx == maskee_lidx
                || layers[maskee_lidx].properties.role != LayerRole::Normal
                || layers[maskee_lidx].properties.mask.is_some()
                || layers[mask_lidx].properties.mask.is_some()
            {
                continue;
            }
            let Some(mask_mode) = runtime_symbol_mask_mode(
                project,
                overrides,
                &layer_char_sym_ids[mask_lidx],
                &mut std::collections::HashSet::new(),
            ) else {
                continue;
            };
            let mask_id = layers[mask_lidx].id.clone();
            layers[mask_lidx].properties.role = LayerRole::Mask;
            if mask_mode == MaskMode::Alpha {
                layers[mask_lidx].properties.mask_mode = MaskMode::Alpha;
            }
            layers[maskee_lidx].properties.mask = Some(mask_id);
        }

        if layers.is_empty() {
            layers.push(Layer {
                id: format!("{target_symbol_id}-rt-empty"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            });
        }

        Some(Symbol {
            id: target_symbol_id.to_owned(),
            name: base_sym.name.clone(),
            kind: base_sym.kind,
            folder: base_sym.folder.clone(),
            class_name: base_sym.class_name.clone(),
            linkage_id: base_sym.linkage_id.clone(),
            timeline: Timeline {
                frame_count: base_sym.timeline.frame_count.max(frame + 1),
                layers,
                scripts: base_sym.timeline.scripts.clone(),
                guides: base_sym.timeline.guides.clone(),
            },
        })
    }

    /// Execute one frame step on the SWF timeline and return the resulting `SwfFrameState`.
    pub fn step_frame(&mut self) -> SwfFrameState {
        self.step_frame_inner(0, true)
    }

    fn step_frame_inner(&mut self, jump_depth: usize, follow_frame_control: bool) -> SwfFrameState {
        self.frame_traces.clear();
        self.frame_audio_events.clear();
        self.frame_unsupported_diagnostics.clear();

        if self.root_loop_rewind_pending {
            self.root_loop_rewind_pending = false;
            let source_frame = self.root_timeline.frames.len().saturating_sub(1) as u32;
            let destination_frame = self.current_frame;
            if destination_frame != source_frame {
                self.prepare_root_display_list_jump(source_frame, destination_frame);
                self.last_applied_frame = None;
            }
        }

        if self.executed_frames == 0 {
            // Run DoInitAction blocks on frame 0 before frame tags
            let init_blocks = self.init_actions.clone();
            for (_, code) in init_blocks {
                self.execute_avm1_slice(&code);
            }
        }

        let entering_frame = self.current_frame;
        let frame_changed = self.last_applied_frame != Some(entering_frame);
        self.last_applied_frame = Some(entering_frame);

        let mut frame_label = None;
        let mut pending_do_actions: Vec<Vec<u8>> = Vec::new();
        let mut pending_lifecycle = Vec::new();

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
                        pending_lifecycle.extend(self.apply_place_tag(tag.code, &tag.payload));
                    }
                    5 | 28 => {
                        pending_lifecycle.extend(self.apply_remove_tag(tag.code, &tag.payload));
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
        self.execute_avm1_work_list(pending_lifecycle);

        // 2. Fire ClipEventEnterFrame handlers on active instances (and step nested sprites)
        let depths: Vec<u16> = self.display_list.keys().copied().collect();
        for d in depths {
            self.step_sprite_frame_actions(d, true);
        }

        // 3. Execute frame DoAction scripts
        if frame_changed {
            for action_bytes in pending_do_actions {
                self.execute_avm1_slice(&action_bytes);
            }
            if self.vm_kind == SwfVmKind::Avm2 && !self.avm2_initialized {
                self.avm2_initialized = true;
                self.initialize_avm2();
            }
            // 4. Execute AVM2 addFrameScript callback if registered for this frame
            if self.vm_kind == SwfVmKind::Avm2
                && let Some(&(abc_idx, method_idx)) = self.avm2_frame_scripts.get(&entering_frame)
            {
                self.execute_avm2_method(abc_idx, method_idx, Vec::new());
            }
        }

        // Sync any nested sprites whose `current_frame` was modified by root frame scripts
        let post_depths: Vec<u16> = self.display_list.keys().copied().collect();
        for d in post_depths {
            self.step_sprite_frame_actions(d, false);
        }

        // A root goto changes the displayed frame immediately. Apply skipped timeline-display
        // tags, then enter the destination frame in this same step so the returned frame number,
        // display list, scripts, sounds, and diagnostics all describe one coherent presentation.
        if follow_frame_control && self.current_frame != entering_frame {
            if jump_depth < MAX_FRAME_JUMPS_PER_STEP {
                let destination = self.current_frame;
                self.prepare_root_display_list_jump(entering_frame, destination);
                let source_traces = self.frame_traces.clone();
                let source_audio = self.frame_audio_events.clone();
                let source_diagnostics = self.frame_unsupported_diagnostics.clone();
                self.executed_frames = self.executed_frames.saturating_add(1);

                let mut destination_state = self.step_frame_inner(jump_depth + 1, true);
                let mut merged_traces = source_traces;
                merged_traces.append(&mut destination_state.traces);
                let mut merged_audio = source_audio;
                merged_audio.append(&mut destination_state.audio_events);
                let mut merged_diagnostics = source_diagnostics;
                for diagnostic in destination_state.unsupported_diagnostics.drain(..) {
                    if !merged_diagnostics.contains(&diagnostic) {
                        merged_diagnostics.push(diagnostic);
                    }
                }
                destination_state.traces = merged_traces.clone();
                destination_state.audio_events = merged_audio.clone();
                destination_state.unsupported_diagnostics = merged_diagnostics.clone();
                self.frame_traces = merged_traces;
                self.frame_audio_events = merged_audio;
                self.frame_unsupported_diagnostics = merged_diagnostics;
                return destination_state;
            }
            self.record_unsupported_diagnostic(format!(
                "SWF frame-control jump chain exceeded the {MAX_FRAME_JUMPS_PER_STEP}-jump runtime limit"
            ));
            self.current_frame = entering_frame;
            self.playing = false;
        }

        let displayed_frame = if follow_frame_control {
            self.current_frame
        } else {
            entering_frame
        };
        self.presented_frame = displayed_frame;
        let state = SwfFrameState {
            frame: displayed_frame,
            label: frame_label,
            playing: self.playing,
            display_list: self.display_list(),
            traces: self.frame_traces.clone(),
            audio_events: self.frame_audio_events.clone(),
            unsupported_diagnostics: self.frame_unsupported_diagnostics.clone(),
        };

        self.executed_frames += 1;
        // If a script called gotoAndPlay/gotoAndStop during this frame, `self.current_frame` was already updated.
        if self.current_frame == entering_frame && self.playing {
            let total = self.root_timeline.frames.len().max(1) as u32;
            if total > 1 {
                let next_frame = (self.current_frame + 1) % total;
                self.root_loop_rewind_pending = next_frame == 0;
                self.current_frame = next_frame;
            }
        }

        state
    }

    fn prepare_root_display_list_jump(&mut self, source_frame: u32, destination_frame: u32) {
        let rewinding = destination_frame < source_frame;
        let start_frame = if rewinding {
            let mut work = Vec::new();
            for (&depth, slot) in &self.display_list {
                if let Some(unload) = detached_unload_work(slot, &live_slot_segment(depth, slot)) {
                    work.push(unload);
                }
            }
            self.display_list.clear();
            self.execute_avm1_work_list(work);
            0
        } else {
            source_frame.saturating_add(1)
        };
        // An Unload handler can redirect the root while the old display tree is detached. Honor
        // that final target and rebuild every intermediate frame exactly once before entering it.
        let destination_frame = self.current_frame;
        for frame in start_frame..destination_frame {
            let Some(tags) = self.root_timeline.frames.get(frame as usize).cloned() else {
                continue;
            };
            for tag in tags {
                match tag.code {
                    4 | 26 | 70 => {
                        let work = self.apply_place_tag(tag.code, &tag.payload);
                        self.execute_avm1_work_list(work);
                    }
                    5 | 28 => {
                        let work = self.apply_remove_tag(tag.code, &tag.payload);
                        self.execute_avm1_work_list(work);
                    }
                    _ => {}
                }
            }
        }
    }

    fn prune_dangling_masks(&mut self) {
        let snapshot = self.display_list.clone();
        prune_dangling_masks_in_map(&mut self.display_list, &snapshot);
    }

    fn step_sprite_frame_actions(&mut self, depth: u16, advance_timeline: bool) {
        let mut pending_actions = Vec::new();
        let mut diagnostics = Vec::new();
        if let Some(slot) = self.display_list.get_mut(&depth) {
            let target_name = live_slot_segment(depth, slot);
            Self::sync_sprite_slot_recursive(
                slot,
                &self.sprites,
                self.swf_version,
                advance_timeline,
                0,
                &target_name,
                &mut pending_actions,
                &mut diagnostics,
            );
        }
        self.prune_dangling_masks();
        for diagnostic in diagnostics {
            self.record_unsupported_diagnostic(diagnostic);
        }
        self.execute_avm1_work_list(pending_actions);
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_sprite_slot_recursive(
        slot: &mut LiveSlot,
        sprites: &HashMap<u16, TimelineDef>,
        swf_version: u8,
        advance_current: bool,
        runtime_depth: usize,
        target_path: &str,
        pending_actions: &mut Vec<PendingAvm1Work>,
        diagnostics: &mut Vec<String>,
    ) {
        if runtime_depth >= MAX_RUNTIME_SPRITE_DEPTH {
            diagnostics.push(format!(
                "SWF nested sprite depth exceeded the {MAX_RUNTIME_SPRITE_DEPTH}-level runtime limit"
            ));
            return;
        }

        if advance_current {
            pending_actions.extend(
                slot.clip_handlers
                    .iter()
                    .filter(|handler| (handler.event_flags & CLIP_EVENT_ENTER_FRAME) != 0)
                    .map(|handler| pending_action(target_path.to_owned(), handler.actions.clone())),
            );
            if let Some(Avm1Value::Object(fn_obj_id)) = slot.props.get("onEnterFrame") {
                pending_actions.push(PendingAvm1Work::Callback {
                    target: target_path.to_owned(),
                    fn_obj_id: *fn_obj_id,
                });
            }
        }

        if advance_current
            && slot.last_applied_frame.is_some()
            && slot.playing
            && slot.total_frames > 1
        {
            slot.current_frame = (slot.current_frame + 1) % slot.total_frames;
        }

        if let Some(def) = sprites.get(&slot.character_id).cloned() {
            let previous_frame = slot.last_applied_frame;
            let frame_idx = slot.current_frame;
            if previous_frame != Some(frame_idx) {
                slot.last_applied_frame = Some(frame_idx);
                let start_frame = match previous_frame {
                    Some(previous) if frame_idx > previous => previous + 1,
                    _ => {
                        for (&depth, child) in &slot.children {
                            let child_target =
                                join_target_path(target_path, &live_slot_segment(depth, child));
                            if let Some(work) = detached_unload_work(child, &child_target) {
                                pending_actions.push(work);
                            }
                        }
                        slot.children.clear();
                        0
                    }
                };
                let max_frame = frame_idx.min(def.frames.len().saturating_sub(1) as u32);
                for frame in start_frame..=max_frame {
                    if let Some(tags) = def.frames.get(frame as usize) {
                        for tag in tags {
                            match tag.code {
                                4 | 26 | 70 => {
                                    if let Some(depth) = place_tag_depth(tag.code, &tag.payload)
                                        && let Some(child) = slot.children.get(&depth)
                                        && place_tag_replaces_slot(tag.code, &tag.payload)
                                    {
                                        let replaced_target = join_target_path(
                                            target_path,
                                            &live_slot_segment(depth, child),
                                        );
                                        if let Some(work) =
                                            detached_unload_work(child, &replaced_target)
                                        {
                                            pending_actions.push(work);
                                        }
                                    }
                                    let (load_actions, unmodeled_filter) = apply_place_tag_to_map(
                                        &mut slot.children,
                                        sprites,
                                        swf_version,
                                        tag.code,
                                        &tag.payload,
                                    );
                                    if let Some(depth) = place_tag_depth(tag.code, &tag.payload)
                                        && let Some(child) = slot.children.get(&depth)
                                    {
                                        let child_path = join_target_path(
                                            target_path,
                                            &live_slot_segment(depth, child),
                                        );
                                        pending_actions.extend(load_actions.into_iter().map(
                                            |actions| pending_action(child_path.clone(), actions),
                                        ));
                                    }
                                    if unmodeled_filter {
                                        diagnostics.push(
                                            "SWF PlaceObject3 filter list contained an unmodeled filter"
                                                .to_owned(),
                                        );
                                    }
                                }
                                5 | 28 => {
                                    if let Some(depth) = remove_tag_depth(tag.code, &tag.payload)
                                        && let Some(child) = slot.children.get(&depth)
                                    {
                                        let removed_target = join_target_path(
                                            target_path,
                                            &live_slot_segment(depth, child),
                                        );
                                        if let Some(work) =
                                            detached_unload_work(child, &removed_target)
                                        {
                                            pending_actions.push(work);
                                        }
                                    }
                                    let _ = apply_remove_tag_to_map(
                                        &mut slot.children,
                                        tag.code,
                                        &tag.payload,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(tags) = def.frames.get(frame_idx as usize) {
                    pending_actions.extend(
                        tags.iter()
                            .filter(|tag| tag.code == 12)
                            .map(|tag| pending_action(target_path.to_owned(), tag.payload.clone())),
                    );
                }
            }
        }

        for (&child_depth, child) in &mut slot.children {
            let child_path = join_target_path(target_path, &live_slot_segment(child_depth, child));
            Self::sync_sprite_slot_recursive(
                child,
                sprites,
                swf_version,
                advance_current,
                runtime_depth + 1,
                &child_path,
                pending_actions,
                diagnostics,
            );
        }
    }

    /// Trigger button condition / clip mouse press events on placed buttons/clips.
    pub fn trigger_pointer_click(&mut self) {
        let buttons = self.buttons.clone();
        let mut actions = Vec::new();
        collect_pointer_actions(&self.display_list, "", &buttons, &mut actions);
        for (target, code) in actions {
            self.execute_avm1_for_target(&target, &code);
        }
    }

    pub fn run_frames(&mut self, max_frames: usize) -> SwfPlaybackResult {
        let mut frames_executed = Vec::with_capacity(max_frames.min(1024));
        let limit = max_frames.clamp(1, 10_000);
        for _ in 0..limit {
            let prev_frame = self.current_frame;
            let state = self.step_frame();
            frames_executed.push(state);
            let any_child_playing = any_playing_descendant(&self.display_list);
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
            unsupported_diagnostics: self.unsupported_diagnostics.clone(),
        }
    }

    fn record_trace(&mut self, msg: String) {
        self.trace_output.push(msg.clone());
        self.frame_traces.push(msg);
    }

    fn record_unsupported_diagnostic(&mut self, msg: String) {
        if !self.unsupported_diagnostics.contains(&msg) && self.unsupported_diagnostics.len() < 64 {
            self.unsupported_diagnostics.push(msg.clone());
        }
        if !self.frame_unsupported_diagnostics.contains(&msg)
            && self.frame_unsupported_diagnostics.len() < 64
        {
            self.frame_unsupported_diagnostics.push(msg);
        }
    }

    fn push_avm_stack(&mut self, stack: &mut Vec<Avm1Value>, val: Avm1Value) {
        if stack.len() >= MAX_AVM_STACK {
            self.record_unsupported_diagnostic(format!(
                "AVM operand stack exceeded bounded limit ({MAX_AVM_STACK})"
            ));
            return;
        }
        stack.push(val);
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

    fn apply_place_tag(&mut self, tag_code: u16, payload: &[u8]) -> Vec<PendingAvm1Work> {
        let mut work = Vec::new();
        let replaced = place_tag_replaces_slot(tag_code, payload);
        if let Some(depth) = place_tag_depth(tag_code, payload)
            && let Some(slot) = self.display_list.get(&depth)
            && replaced
            && let Some(unload) = detached_unload_work(slot, &live_slot_segment(depth, slot))
        {
            work.push(unload);
        }
        let (load_scripts, unmodeled_filter) = apply_place_tag_to_map(
            &mut self.display_list,
            &self.sprites,
            self.swf_version,
            tag_code,
            payload,
        );
        if replaced {
            self.prune_dangling_masks();
        }
        if unmodeled_filter {
            self.record_unsupported_diagnostic(
                "SWF PlaceObject3 filter list contained an unmodeled filter".to_owned(),
            );
        }
        let Some(depth) = place_tag_depth(tag_code, payload) else {
            return work;
        };
        let Some(slot) = self.display_list.get(&depth) else {
            return work;
        };
        let target = live_slot_segment(depth, slot);
        work.extend(
            load_scripts
                .into_iter()
                .map(|actions| pending_action(target.clone(), actions)),
        );
        work
    }

    fn apply_remove_tag(&mut self, tag_code: u16, payload: &[u8]) -> Vec<PendingAvm1Work> {
        let mut work = Vec::new();
        if let Some(depth) = remove_tag_depth(tag_code, payload)
            && let Some(slot) = self.display_list.get(&depth)
        {
            let target = live_slot_segment(depth, slot);
            if let Some(unload) = detached_unload_work(slot, &target) {
                work.push(unload);
            }
        }
        let _ = apply_remove_tag_to_map(&mut self.display_list, tag_code, payload);
        self.prune_dangling_masks();
        work
    }

    // =========================================================================
    // AVM1 Bytecode Interpreter
    // =========================================================================

    fn execute_avm1_slice(&mut self, code: &[u8]) {
        let mut stack: Vec<Avm1Value> = Vec::new();
        let mut locals: HashMap<String, Avm1Value> = HashMap::new();
        let mut steps = 0usize;
        self.avm1_thrown = None;
        let _ = self.run_avm1_code(code, &mut stack, &mut locals, &mut steps, 0);
        if let Some(uncaught) = self.avm1_thrown.take() {
            let desc = uncaught.to_trace_string(self.swf_version, &self.avm1_objects);
            self.record_unsupported_diagnostic(format!("Uncaught AVM1 exception: {desc}"));
        }
    }

    fn execute_avm1_for_target(&mut self, target: &str, code: &[u8]) {
        let saved_target = std::mem::replace(&mut self.avm1_target, target.to_owned());
        self.execute_avm1_slice(code);
        self.avm1_target = saved_target;
    }

    fn execute_avm1_callback_for_target(&mut self, target: &str, fn_obj_id: usize) {
        let Some(func) = self
            .avm1_objects
            .get(fn_obj_id)
            .and_then(|o| o.function.clone())
        else {
            return;
        };
        let saved_target = std::mem::replace(&mut self.avm1_target, target.to_owned());
        let this_val = Avm1Value::MovieClipIdentity(self.canonical_movie_target(target));
        let mut steps = 0usize;
        self.avm1_thrown = None;
        let _ = self.invoke_avm1_func_with_this(&func, this_val, None, Vec::new(), &mut steps, 0);
        self.avm1_thrown = None;
        self.avm1_target = saved_target;
    }

    fn execute_avm1_work(&mut self, work: PendingAvm1Work) {
        match work {
            PendingAvm1Work::Action { target, actions } => {
                self.execute_avm1_for_target(&target, &actions);
            }
            PendingAvm1Work::Callback { target, fn_obj_id } => {
                self.execute_avm1_callback_for_target(&target, fn_obj_id);
            }
            PendingAvm1Work::Detached {
                root_target,
                root_slot,
                actions,
            } => {
                debug_assert!(self.avm1_detached_target.is_none());
                self.avm1_detached_target = Some((root_target, *root_slot));
                for item in actions {
                    match item {
                        DetachedAvm1Item::Bytes(target, code) => {
                            self.execute_avm1_for_target(&target, &code);
                        }
                        DetachedAvm1Item::Callback(target, fn_obj_id) => {
                            self.execute_avm1_callback_for_target(&target, fn_obj_id);
                        }
                    }
                }
                self.avm1_detached_target = None;
            }
        }
    }

    fn execute_avm1_work_list(&mut self, work: Vec<PendingAvm1Work>) {
        for item in work {
            self.execute_avm1_work(item);
        }
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
            self.record_unsupported_diagnostic(format!(
                "AVM1 call depth exceeded bounded limit ({MAX_CALL_DEPTH})"
            ));
            return None;
        }
        let mut pc = 0usize;
        while pc < code.len() && *steps < MAX_VM_STEPS_PER_FRAME && self.avm1_thrown.is_none() {
            *steps += 1;
            let insn_pc = pc;
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
                0x08 => {
                    // ActionToggleQuality (visual hint; valid SWF4 no-op)
                }
                0x09 => {
                    // ActionStopSounds
                    self.stream_sound_active = false;
                    self.stop_all_sounds_pending = true;
                }
                0x0A => {
                    // ActionAdd (numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b + a));
                }
                0x0B => {
                    // ActionSubtract
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b - a));
                }
                0x0C => {
                    // ActionMultiply
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b * a));
                }
                0x0D => {
                    // ActionDivide
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b / a));
                }
                0x0E => {
                    // ActionEquals (SWF 4 numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Bool(b == a));
                }
                0x0F => {
                    // ActionLess (SWF 4 numeric)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Bool(b < a));
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
                    self.push_avm_stack(stack, Avm1Value::Bool(b && a));
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
                    self.push_avm_stack(stack, Avm1Value::Bool(b || a));
                }
                0x12 => {
                    // ActionNot
                    let a = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    self.push_avm_stack(stack, Avm1Value::Bool(!a));
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
                    self.push_avm_stack(stack, Avm1Value::Bool(b == a));
                }
                0x14 | 0x31 => {
                    // ActionStringLength / ActionMBStringLength
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Number(s.chars().count() as f64));
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
                    self.push_avm_stack(stack, Avm1Value::String(sub));
                }
                0x17 => {
                    // ActionPop
                    let _ = stack.pop();
                }
                0x18 => {
                    // ActionToInteger
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(v)));
                }
                0x1C => {
                    // ActionGetVariable
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let val = self.get_avm1_variable(&name, locals);
                    self.push_avm_stack(stack, val);
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
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.avm1_target = if let Avm1Value::MovieClipIdentity(identity) = target {
                        identity
                    } else {
                        let authored = target.to_trace_string(self.swf_version, &self.avm1_objects);
                        if authored.is_empty() {
                            String::new()
                        } else {
                            self.canonical_movie_target(&authored)
                        }
                    };
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
                    self.push_avm_stack(stack, Avm1Value::String(format!("{b}{a}")));
                }
                0x22 => {
                    // ActionGetProperty
                    let prop_idx = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = if let Avm1Value::MovieClipIdentity(identity) = target {
                        self.get_mc_property_identity(&identity, prop_idx)
                    } else {
                        let target = target.to_trace_string(self.swf_version, &self.avm1_objects);
                        self.get_mc_property(&target, prop_idx)
                    };
                    self.push_avm_stack(stack, val);
                }
                0x23 => {
                    // ActionSetProperty
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let prop_idx = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let Avm1Value::MovieClipIdentity(identity) = target {
                        self.set_mc_property_identity(&identity, prop_idx, val);
                    } else {
                        let target = target.to_trace_string(self.swf_version, &self.avm1_objects);
                        self.set_mc_property(&target, prop_idx, val);
                    }
                }
                0x24 => {
                    // ActionCloneSprite: [source, new_name, depth] (depth on top)
                    let depth_raw = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let new_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let source = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let src_target = self.resolve_avm1_target_value(&source);
                    let swf_depth = depth_raw.clamp(0, 65535) as u16;
                    let _ =
                        self.duplicate_movie_clip_at_swf_depth(&src_target, &new_name, swf_depth);
                }
                0x25 => {
                    // ActionRemoveSprite: [target]
                    let target = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let resolved = self.resolve_avm1_target_value(&target);
                    self.remove_movie_clip_identity(&resolved);
                }
                0x26 => {
                    // ActionTrace
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let msg = v.to_trace_string(self.swf_version, &self.avm1_objects);
                    self.record_trace(msg);
                }
                0x27 => {
                    // ActionStartDrag: [x1, y1, x2, y2 (if constrain)], constrain, lockCenter, target
                    let target_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let _lock_center = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    let constrain = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_bool(self.swf_version);
                    if constrain {
                        let _y2 = stack.pop();
                        let _x2 = stack.pop();
                        let _y1 = stack.pop();
                        let _x1 = stack.pop();
                    }
                    let resolved = self.resolve_avm1_target_value(&target_val);
                    self.drag_target = Some(resolved);
                }
                0x28 => {
                    // ActionEndDrag
                    self.drag_target = None;
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
                    self.push_avm_stack(stack, Avm1Value::Bool(b < a));
                }
                0x2A => {
                    // ActionThrow
                    let err = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.avm1_thrown = Some(err);
                    return None;
                }
                0x2B => {
                    // ActionCastOp: [constr, obj] (obj on top)
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let constr = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = if self.avm1_is_instance_of(&obj, &constr) {
                        obj
                    } else {
                        Avm1Value::Null
                    };
                    self.push_avm_stack(stack, out);
                }
                0x2C => {
                    // ActionImplementsOp: [iface_1..iface_n, count, ctor] (ctor on top)
                    let ctor = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let count = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut iface_ids = Vec::with_capacity(count);
                    for _ in 0..count {
                        if let Some(Avm1Value::Object(iface_id)) = stack.pop() {
                            if let Some(Avm1Value::Object(proto_id)) = self
                                .avm1_objects
                                .get(iface_id)
                                .and_then(|o| o.props.get("prototype").cloned())
                            {
                                iface_ids.push(proto_id);
                            }
                            iface_ids.push(iface_id);
                        }
                    }
                    if let Avm1Value::Object(ctor_id) = ctor
                        && let Some(Avm1Value::Object(proto_id)) = self
                            .avm1_objects
                            .get(ctor_id)
                            .and_then(|o| o.props.get("prototype").cloned())
                        && let Some(proto_obj) = self.avm1_objects.get_mut(proto_id)
                    {
                        for iid in iface_ids {
                            if !proto_obj.interfaces.contains(&iid) {
                                proto_obj.interfaces.push(iid);
                            }
                        }
                    }
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
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(r)));
                }
                0x32 | 0x36 => {
                    // ActionCharToAscii / ActionMBCharToAscii
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let code_pt = s.chars().next().map_or(0u32, u32::from);
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(code_pt)));
                }
                0x33 | 0x37 => {
                    // ActionAsciiToChar / ActionMBAsciiToChar
                    let code_pt = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let s = if code_pt == 0 {
                        String::new()
                    } else {
                        char::from_u32(code_pt).map_or_else(String::new, |c| c.to_string())
                    };
                    self.push_avm_stack(stack, Avm1Value::String(s));
                }
                0x34 => {
                    // ActionGetTime (deterministic frame-based elapsed milliseconds)
                    let ms = f64::from(self.executed_frames) * 40.0;
                    self.push_avm_stack(stack, Avm1Value::Number(ms));
                }
                0x3A => {
                    // ActionDelete
                    let prop = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let deleted = match obj {
                        Avm1Value::Object(id) => self
                            .avm1_objects
                            .get_mut(id)
                            .is_some_and(|o| o.props.remove(&prop).is_some() || true),
                        Avm1Value::MovieClip(mc) => {
                            let target = self.canonical_movie_target(&mc);
                            if mc == "_global" {
                                self.avm1_global_obj.remove(&prop);
                                true
                            } else if target == "_root" {
                                self.avm1_globals.remove(&prop);
                                true
                            } else if let Some(slot) = self.target_slot_mut(&target) {
                                slot.props.remove(&prop);
                                true
                            } else {
                                false
                            }
                        }
                        Avm1Value::MovieClipIdentity(target) => {
                            if target == "_root" {
                                self.avm1_globals.remove(&prop);
                                true
                            } else if let Some(slot) = self.target_slot_mut(&target) {
                                slot.props.remove(&prop);
                                true
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    self.push_avm_stack(stack, Avm1Value::Bool(deleted));
                }
                0x3B => {
                    // ActionDelete2
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let mut deleted = false;
                    for scope in self.avm1_with_scopes.clone().into_iter().rev() {
                        if let Avm1Value::Object(id) = scope
                            && let Some(o) = self.avm1_objects.get_mut(id)
                            && o.props.remove(&name).is_some()
                        {
                            deleted = true;
                            break;
                        }
                    }
                    if !deleted {
                        deleted |= locals.remove(&name).is_some();
                        let cur = self.current_target_path();
                        if cur != "_root"
                            && let Some(slot) = self.target_slot_mut(&cur)
                        {
                            deleted |= slot.props.remove(&name).is_some();
                        }
                        deleted |= self.avm1_globals.remove(&name).is_some();
                        deleted |= self.avm1_global_obj.remove(&name).is_some();
                        deleted |= self.avm1_functions.remove(&name).is_some();
                    }
                    self.push_avm_stack(stack, Avm1Value::Bool(deleted));
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
                        let cur = self.current_target_path();
                        if cur == "_root" {
                            self.avm1_globals.insert(name, val);
                        } else if let Some(slot) = self.target_slot_mut(&cur) {
                            slot.props.insert(name, val);
                        }
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
                    self.push_avm_stack(stack, ret);
                }
                0x3E => {
                    // ActionReturn
                    return Some(stack.pop().unwrap_or(Avm1Value::Undefined));
                }
                0x3F => {
                    // ActionModulo
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(b % a));
                }
                0x40 => {
                    // ActionNewObject
                    let ctor_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ctor_name = ctor_val.to_trace_string(self.swf_version, &self.avm1_objects);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let resolved_ctor = if matches!(ctor_val, Avm1Value::Object(_)) {
                        ctor_val
                    } else {
                        self.get_avm1_variable(&ctor_name, locals)
                    };
                    let constructed =
                        self.construct_avm1_object(&ctor_name, &resolved_ctor, args, steps, depth);
                    self.push_avm_stack(stack, constructed);
                }
                0x41 => {
                    // ActionDefineLocal2
                    let name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    locals.entry(name.clone()).or_insert(Avm1Value::Undefined);
                    if depth == 0 {
                        let cur = self.current_target_path();
                        if cur == "_root" {
                            self.avm1_globals
                                .entry(name)
                                .or_insert(Avm1Value::Undefined);
                        } else if let Some(slot) = self.target_slot_mut(&cur) {
                            slot.props.entry(name).or_insert(Avm1Value::Undefined);
                        }
                    }
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
                        array_elements: Some(elems),
                        ..Default::default()
                    });
                    self.push_avm_stack(stack, Avm1Value::Object(obj_id));
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
                        ..Default::default()
                    });
                    self.push_avm_stack(stack, Avm1Value::Object(obj_id));
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
                        Avm1Value::MovieClip(ref mc) if mc.starts_with("__builtin_") => "function",
                        Avm1Value::MovieClip(ref mc) if mc == "Math" || mc == "_global" => "object",
                        Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) => "movieclip",
                        Avm1Value::Super { .. } => "object",
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
                    self.push_avm_stack(stack, Avm1Value::String(t.to_owned()));
                }
                0x45 => {
                    // ActionTargetPath: returns _level0 dot-path ONLY for MovieClip values
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = match v {
                        Avm1Value::MovieClip(mc)
                            if mc != "Math" && mc != "_global" && !mc.starts_with("__builtin_") =>
                        {
                            let canonical = self.canonical_movie_target(&mc);
                            self.target_dot_path(&canonical)
                                .map_or(Avm1Value::Undefined, Avm1Value::String)
                        }
                        Avm1Value::MovieClipIdentity(target) => self
                            .target_dot_path(&target)
                            .map_or(Avm1Value::Undefined, Avm1Value::String),
                        _ => Avm1Value::Undefined,
                    };
                    self.push_avm_stack(stack, out);
                }
                0x46 => {
                    // ActionEnumerate (variable path string on stack)
                    let var_name = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    let obj = self.get_avm1_variable(&var_name, locals);
                    self.enumerate_avm1_value(&obj, stack);
                }
                0x47 => {
                    // ActionAdd2 (type-aware)
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if matches!(a, Avm1Value::String(_)) || matches!(b, Avm1Value::String(_)) {
                        let sa = a.to_trace_string(self.swf_version, &self.avm1_objects);
                        let sb = b.to_trace_string(self.swf_version, &self.avm1_objects);
                        self.push_avm_stack(stack, Avm1Value::String(format!("{sb}{sa}")));
                    } else {
                        self.push_avm_stack(
                            stack,
                            Avm1Value::Number(b.to_number() + a.to_number()),
                        );
                    }
                }
                0x48 => {
                    // ActionLess2
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = match (&b, &a) {
                        (Avm1Value::String(sb), Avm1Value::String(sa)) => Avm1Value::Bool(sb < sa),
                        _ => {
                            let nb = b.to_number();
                            let na = a.to_number();
                            if nb.is_nan() || na.is_nan() {
                                Avm1Value::Undefined
                            } else {
                                Avm1Value::Bool(nb < na)
                            }
                        }
                    };
                    self.push_avm_stack(stack, res);
                }
                0x49 => {
                    // ActionEquals2
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = avm1_equals(&b, &a, self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::Bool(eq));
                }
                0x4A => {
                    // ActionToNumber
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(n));
                }
                0x4B => {
                    // ActionToString
                    let s = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(self.swf_version, &self.avm1_objects);
                    self.push_avm_stack(stack, Avm1Value::String(s));
                }
                0x4C => {
                    // ActionPushDuplicate
                    let top = stack.last().cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(stack, top);
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
                    self.push_avm_stack(stack, val);
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
                    self.push_avm_stack(stack, Avm1Value::Number(n + 1.0));
                }
                0x51 => {
                    // ActionDecrement
                    let n = stack.pop().unwrap_or(Avm1Value::Undefined).to_number();
                    self.push_avm_stack(stack, Avm1Value::Number(n - 1.0));
                }
                0x52 => {
                    // ActionCallMethod
                    let method_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let method_name = if matches!(method_val, Avm1Value::Undefined) {
                        String::new()
                    } else {
                        method_val.to_trace_string(self.swf_version, &self.avm1_objects)
                    };
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
                    self.push_avm_stack(stack, ret);
                }
                0x53 => {
                    // ActionNewMethod
                    let method_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let method_name = if matches!(method_val, Avm1Value::Undefined) {
                        String::new()
                    } else {
                        method_val.to_trace_string(self.swf_version, &self.avm1_objects)
                    };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let nargs = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_i32()
                        .clamp(0, 256) as usize;
                    let mut args = Vec::with_capacity(nargs);
                    for _ in 0..nargs {
                        args.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    let ctor_val = if method_name.is_empty() {
                        obj
                    } else {
                        self.get_avm1_member(&obj, &method_name)
                    };
                    let constructed =
                        self.construct_avm1_object(&method_name, &ctor_val, args, steps, depth);
                    self.push_avm_stack(stack, constructed);
                }
                0x54 => {
                    // ActionInstanceOf: [obj, constr] (constr on top)
                    let constr = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let is_inst = self.avm1_is_instance_of(&obj, &constr);
                    self.push_avm_stack(stack, Avm1Value::Bool(is_inst));
                }
                0x55 => {
                    // ActionEnumerate2
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.enumerate_avm1_value(&obj, stack);
                }
                0x60 => {
                    // ActionBitAnd
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b & a)));
                }
                0x61 => {
                    // ActionBitOr
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b | a)));
                }
                0x62 => {
                    // ActionBitXor
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b ^ a)));
                }
                0x63 => {
                    // ActionBitLShift
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b.wrapping_shl(a))));
                }
                0x64 => {
                    // ActionBitRShift
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_i32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b.wrapping_shr(a))));
                }
                0x65 => {
                    // ActionBitURShift
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_u32();
                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(b.wrapping_shr(a))));
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
                        (Avm1Value::MovieClip(x), Avm1Value::MovieClip(y))
                        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClipIdentity(y))
                        | (Avm1Value::MovieClip(x), Avm1Value::MovieClipIdentity(y))
                        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClip(y)) => {
                            self.canonical_movie_target(x) == self.canonical_movie_target(y)
                        }
                        _ => false,
                    };
                    self.push_avm_stack(stack, Avm1Value::Bool(eq));
                }
                0x67 => {
                    // ActionGreater
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = match (&b, &a) {
                        (Avm1Value::String(sb), Avm1Value::String(sa)) => Avm1Value::Bool(sb > sa),
                        _ => {
                            let nb = b.to_number();
                            let na = a.to_number();
                            if nb.is_nan() || na.is_nan() {
                                Avm1Value::Undefined
                            } else {
                                Avm1Value::Bool(nb > na)
                            }
                        }
                    };
                    self.push_avm_stack(stack, res);
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
                    self.push_avm_stack(stack, Avm1Value::Bool(b > a));
                }
                0x69 => {
                    // ActionExtends: [subclass, superclass] (superclass on top)
                    let sup = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let sub = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let (Avm1Value::Object(sub_id), Avm1Value::Object(sup_id)) = (sub, sup) {
                        let sup_proto =
                            self.get_avm1_member(&Avm1Value::Object(sup_id), "prototype");
                        let new_proto_id = self.avm1_objects.len();
                        let mut proto_props = HashMap::new();
                        proto_props.insert("__proto__".to_owned(), sup_proto);
                        proto_props.insert("__constructor__".to_owned(), Avm1Value::Object(sup_id));
                        proto_props.insert("constructor".to_owned(), Avm1Value::Object(sub_id));
                        self.avm1_objects.push(Avm1Object {
                            props: proto_props,
                            ..Default::default()
                        });
                        if let Some(sub_obj) = self.avm1_objects.get_mut(sub_id) {
                            sub_obj
                                .props
                                .insert("prototype".to_owned(), Avm1Value::Object(new_proto_id));
                            sub_obj
                                .props
                                .insert("__super_ctor__".to_owned(), Avm1Value::Object(sup_id));
                        }
                    }
                }
                0x81 => {
                    // ActionGotoFrame (u16 zero-based frame index)
                    if payload.len() >= 2 {
                        let f = u32::from(u16::from_le_bytes([payload[0], payload[1]]));
                        let target = self.current_target_path();
                        self.set_target_goto_value_identity(
                            &target,
                            &Avm1Value::Number(f64::from(f + 1)),
                            false,
                        );
                    }
                }
                0x83 => {
                    // ActionGetURL (url, target c-strings — safe sandboxed no-op)
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
                0x8A => {
                    // ActionWaitForFrame (all frames loaded in local SWF buffer)
                }
                0x8B => {
                    // ActionSetTarget
                    let mut pr = SwfSlice::new(payload);
                    if let Some((t, _)) = pr.c_string(self.swf_version) {
                        self.avm1_target = if t.is_empty() {
                            String::new()
                        } else {
                            self.canonical_movie_target(&t)
                        };
                    }
                }
                0x8C => {
                    // ActionGotoLabel
                    let mut pr = SwfSlice::new(payload);
                    if let Some((label, _)) = pr.c_string(self.swf_version) {
                        let target = self.current_target_path();
                        self.set_target_goto_value_identity(
                            &target,
                            &Avm1Value::String(label),
                            false,
                        );
                    }
                }
                0x8D => {
                    // ActionWaitForFrame2 (pops frame expression; local SWF is fully loaded)
                    let _ = stack.pop();
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
                    let fn_obj_id = self.register_avm1_function_object(
                        &fn_name,
                        params,
                        register_count,
                        flags,
                        true,
                        body,
                        locals,
                        depth,
                    );
                    if fn_name.is_empty() {
                        self.push_avm_stack(stack, Avm1Value::Object(fn_obj_id));
                    }
                }
                0x8F => {
                    // ActionTry
                    let mut pr = SwfSlice::new(payload);
                    let Some(flags) = pr.u8() else { continue };
                    let Some(try_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let Some(catch_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let Some(finally_size) = pr.u16().map(usize::from) else {
                        continue;
                    };
                    let has_catch = (flags & 0x01) != 0;
                    let has_finally = (flags & 0x02) != 0;
                    let catch_in_reg = (flags & 0x04) != 0;
                    let (catch_var, catch_reg) = if catch_in_reg {
                        (None, pr.u8())
                    } else {
                        (pr.c_string(self.swf_version).map(|(s, _)| s), None)
                    };
                    let try_end = (pc + try_size).min(code.len());
                    let try_bytes = code[pc..try_end].to_vec();
                    pc = try_end;
                    let catch_end = (pc + catch_size).min(code.len());
                    let catch_bytes = code[pc..catch_end].to_vec();
                    pc = catch_end;
                    let finally_end = (pc + finally_size).min(code.len());
                    let finally_bytes = code[pc..finally_end].to_vec();
                    pc = finally_end;

                    let mut ret = self.run_avm1_code(&try_bytes, stack, locals, steps, depth);
                    if let Some(thrown_val) = self.avm1_thrown.take() {
                        if has_catch {
                            if let Some(reg) = catch_reg {
                                self.avm1_registers[usize::from(reg)] = thrown_val;
                            } else if let Some(var_name) = catch_var
                                && !var_name.is_empty()
                            {
                                locals.insert(var_name.clone(), thrown_val.clone());
                                if depth == 0 {
                                    self.avm1_globals.insert(var_name, thrown_val);
                                }
                            }
                            ret = self.run_avm1_code(&catch_bytes, stack, locals, steps, depth);
                        } else {
                            self.avm1_thrown = Some(thrown_val);
                        }
                    }
                    if has_finally {
                        let pending_throw = self.avm1_thrown.take();
                        let finally_ret =
                            self.run_avm1_code(&finally_bytes, stack, locals, steps, depth);
                        if self.avm1_thrown.is_none() {
                            if finally_ret.is_some() {
                                ret = finally_ret;
                            } else {
                                self.avm1_thrown = pending_throw;
                            }
                        }
                    }
                    if self.avm1_thrown.is_some() || ret.is_some() {
                        return ret;
                    }
                }
                0x94 => {
                    // ActionWith
                    let scope_obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if payload.len() >= 2 {
                        let with_size = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
                        let end = (pc + with_size).min(code.len());
                        let body = code[pc..end].to_vec();
                        pc = end;
                        let pushed = !matches!(scope_obj, Avm1Value::Undefined | Avm1Value::Null);
                        if pushed {
                            self.avm1_with_scopes.push(scope_obj);
                        }
                        let ret = self.run_avm1_code(&body, stack, locals, steps, depth);
                        if pushed {
                            let _ = self.avm1_with_scopes.pop();
                        }
                        if self.avm1_thrown.is_some() || ret.is_some() {
                            return ret;
                        }
                    }
                }
                0x96 => {
                    // ActionPush
                    let mut pr = SwfSlice::new(payload);
                    while let Some(ptype) = pr.u8() {
                        match ptype {
                            0 => {
                                if let Some((s, _)) = pr.c_string(self.swf_version) {
                                    self.push_avm_stack(stack, Avm1Value::String(s));
                                } else {
                                    break;
                                }
                            }
                            1 => {
                                if let Some(b) = pr.take(4) {
                                    let f = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(f)));
                                } else {
                                    break;
                                }
                            }
                            2 => self.push_avm_stack(stack, Avm1Value::Null),
                            3 => self.push_avm_stack(stack, Avm1Value::Undefined),
                            4 => {
                                if let Some(reg) = pr.u8() {
                                    let v = self.avm1_registers[usize::from(reg)].clone();
                                    self.push_avm_stack(stack, v);
                                } else {
                                    break;
                                }
                            }
                            5 => {
                                if let Some(b) = pr.u8() {
                                    self.push_avm_stack(stack, Avm1Value::Bool(b != 0));
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
                                    self.push_avm_stack(stack, Avm1Value::Number(d));
                                } else {
                                    break;
                                }
                            }
                            7 => {
                                if let Some(i) = pr.i32() {
                                    self.push_avm_stack(stack, Avm1Value::Number(f64::from(i)));
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
                                    self.push_avm_stack(stack, Avm1Value::String(s));
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
                                    self.push_avm_stack(stack, Avm1Value::String(s));
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
                0x9A => {
                    // ActionGetURL2 (pops target_window, url — safe sandboxed no-op)
                    let _target_window = stack.pop();
                    let _url = stack.pop();
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
                    let fn_obj_id = self.register_avm1_function_object(
                        &fn_name, params, 0, 0, false, body, locals, depth,
                    );
                    if fn_name.is_empty() {
                        self.push_avm_stack(stack, Avm1Value::Object(fn_obj_id));
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
                0x9E => {
                    // ActionCall (Flash 4 call frame actions)
                    let frame_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let movie_target = self.current_target_path();
                    let resolved = match &frame_val {
                        Avm1Value::Number(n) if *n >= 1.0 => Some((*n as u32).saturating_sub(1)),
                        Avm1Value::String(s) => {
                            if let Ok(num) = s.trim().parse::<u32>()
                                && num >= 1
                            {
                                Some(num - 1)
                            } else {
                                self.target_label_frame(&movie_target, s)
                            }
                        }
                        _ => None,
                    };
                    if let Some(f_idx) = resolved {
                        let scripts: Vec<Vec<u8>> = if movie_target == "_root" {
                            self.root_timeline
                                .frames
                                .get(f_idx as usize)
                                .map(|tags| {
                                    tags.iter()
                                        .filter(|t| t.code == 12)
                                        .map(|t| t.payload.clone())
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else if let Some(cid) =
                            self.target_slot(&movie_target).map(|s| s.character_id)
                        {
                            self.sprites
                                .get(&cid)
                                .and_then(|def| def.frames.get(f_idx as usize))
                                .map(|tags| {
                                    tags.iter()
                                        .filter(|t| t.code == 12)
                                        .map(|t| t.payload.clone())
                                        .collect()
                                })
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        for script_bytes in scripts {
                            let mut sub_stack = Vec::new();
                            let _ = self.run_avm1_code(
                                &script_bytes,
                                &mut sub_stack,
                                locals,
                                steps,
                                depth + 1,
                            );
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
                    let movie_target = self.current_target_path();
                    let total = self.target_total_frames(&movie_target);
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
                                self.target_label_frame(&movie_target, &s)
                                    .map(|f| (f + bias).min(total.saturating_sub(1)))
                            }
                        }
                        _ => None,
                    };
                    if let Some(target_f) = resolved {
                        self.set_target_frame_zero_based(&movie_target, target_f, play_flag);
                    }
                }
                _ => {
                    let cur_target = self.current_target_path();
                    self.record_unsupported_diagnostic(format!(
                        "Unsupported AVM1 action opcode 0x{op:02X} at target={cur_target} frame={} offset=0x{insn_pc:x}",
                        self.current_frame
                    ));
                }
            }
        }
        if *steps >= MAX_VM_STEPS_PER_FRAME {
            self.record_unsupported_diagnostic(format!(
                "AVM1 execution reached bounded per-frame step limit ({MAX_VM_STEPS_PER_FRAME})"
            ));
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn register_avm1_function_object(
        &mut self,
        fn_name: &str,
        params: Vec<(u8, String)>,
        register_count: u8,
        flags: u16,
        is_v2: bool,
        body: Vec<u8>,
        locals: &mut HashMap<String, Avm1Value>,
        depth: usize,
    ) -> usize {
        let proto_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object::default());
        let fn_obj_id = self.avm1_objects.len();
        let func = Avm1Function {
            params,
            register_count,
            flags,
            is_v2,
            body,
            constant_pool: self.avm1_constant_pool.clone(),
            base_target: self.avm1_target.clone(),
            self_obj_id: Some(fn_obj_id),
        };
        let mut fn_props = HashMap::new();
        fn_props.insert("prototype".to_owned(), Avm1Value::Object(proto_id));
        self.avm1_objects.push(Avm1Object {
            props: fn_props,
            function: Some(func.clone()),
            ..Default::default()
        });
        if let Some(proto_obj) = self.avm1_objects.get_mut(proto_id) {
            proto_obj
                .props
                .insert("constructor".to_owned(), Avm1Value::Object(fn_obj_id));
        }
        if !fn_name.is_empty() {
            self.avm1_functions.insert(fn_name.to_owned(), func);
            let fn_val = Avm1Value::Object(fn_obj_id);
            if depth > 0 {
                locals.insert(fn_name.to_owned(), fn_val);
            } else {
                let cur = self.current_target_path();
                if cur != "_root"
                    && let Some(slot) = self.target_slot_mut(&cur)
                {
                    slot.props.insert(fn_name.to_owned(), fn_val);
                } else {
                    self.avm1_globals.insert(fn_name.to_owned(), fn_val);
                }
            }
        }
        fn_obj_id
    }

    fn construct_avm1_object(
        &mut self,
        ctor_name: &str,
        ctor_val: &Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(ctor_id) = ctor_val
            && let Some(func) = self
                .avm1_objects
                .get(*ctor_id)
                .and_then(|o| o.function.clone())
        {
            let proto = self.get_avm1_member(&Avm1Value::Object(*ctor_id), "prototype");
            let super_ctor = self
                .avm1_objects
                .get(*ctor_id)
                .and_then(|o| o.props.get("__super_ctor__").cloned())
                .and_then(|v| match v {
                    Avm1Value::Object(id) => Some(id),
                    _ => None,
                });
            let inst_id = self.avm1_objects.len();
            let mut props = HashMap::new();
            props.insert("__proto__".to_owned(), proto);
            props.insert("__constructor__".to_owned(), Avm1Value::Object(*ctor_id));
            self.avm1_objects.push(Avm1Object {
                props,
                ..Default::default()
            });
            let inst_val = Avm1Value::Object(inst_id);
            let ret = self.invoke_avm1_func_with_this(
                &func,
                inst_val.clone(),
                super_ctor,
                args,
                steps,
                depth,
            );
            return if matches!(ret, Avm1Value::Object(_)) {
                ret
            } else {
                inst_val
            };
        }
        if !ctor_name.is_empty()
            && let Some(func) = self.avm1_functions.get(ctor_name).cloned()
        {
            let inst_id = self.avm1_objects.len();
            let mut props = HashMap::new();
            if let Some(fn_id) = func.self_obj_id {
                let proto = self.get_avm1_member(&Avm1Value::Object(fn_id), "prototype");
                props.insert("__proto__".to_owned(), proto);
                props.insert("__constructor__".to_owned(), Avm1Value::Object(fn_id));
            }
            self.avm1_objects.push(Avm1Object {
                props,
                ..Default::default()
            });
            let inst_val = Avm1Value::Object(inst_id);
            let ret =
                self.invoke_avm1_func_with_this(&func, inst_val.clone(), None, args, steps, depth);
            return if matches!(ret, Avm1Value::Object(_)) {
                ret
            } else {
                inst_val
            };
        }
        let obj_id = self.avm1_objects.len();
        let mut obj = Avm1Object::default();
        if ctor_name == "Array"
            || matches!(ctor_val, Avm1Value::MovieClip(s) if s == "__builtin_Array")
        {
            if args.len() == 1
                && let Some(Avm1Value::Number(n)) = args.first()
            {
                let len = (*n as usize).min(4096);
                obj.array_elements = Some(vec![Avm1Value::Undefined; len]);
            } else {
                obj.array_elements = Some(args);
            }
        } else if ctor_name == "Object"
            && args.len() == 1
            && matches!(args[0], Avm1Value::Object(_))
        {
            return args[0].clone();
        }
        self.avm1_objects.push(obj);
        Avm1Value::Object(obj_id)
    }

    fn avm1_is_instance_of(&self, obj: &Avm1Value, constr: &Avm1Value) -> bool {
        match constr {
            Avm1Value::MovieClip(b) if b == "__builtin_MovieClip" => {
                matches!(
                    obj,
                    Avm1Value::MovieClip(mc) if !mc.starts_with("__builtin_") && mc != "Math" && mc != "_global"
                ) || matches!(obj, Avm1Value::MovieClipIdentity(_))
            }
            Avm1Value::MovieClip(b) if b == "__builtin_Array" => {
                matches!(obj, Avm1Value::Object(id) if self.avm1_objects.get(*id).is_some_and(|o| o.array_elements.is_some()))
            }
            Avm1Value::MovieClip(b) if b == "__builtin_Function" => {
                matches!(obj, Avm1Value::Object(id) if self.avm1_objects.get(*id).is_some_and(|o| o.function.is_some()))
                    || matches!(obj, Avm1Value::MovieClip(mc) if mc.starts_with("__builtin_"))
            }
            Avm1Value::MovieClip(b) if b == "__builtin_Object" => {
                matches!(
                    obj,
                    Avm1Value::Object(_)
                        | Avm1Value::MovieClip(_)
                        | Avm1Value::MovieClipIdentity(_)
                )
            }
            Avm1Value::Object(ctor_id) => {
                let Avm1Value::Object(mut cur_id) = *obj else {
                    return false;
                };
                let target_proto_id = self
                    .avm1_objects
                    .get(*ctor_id)
                    .and_then(|o| o.props.get("prototype"))
                    .and_then(|v| match v {
                        Avm1Value::Object(pid) => Some(*pid),
                        _ => None,
                    });
                for _ in 0..32 {
                    let Some(cur_obj) = self.avm1_objects.get(cur_id) else {
                        break;
                    };
                    if cur_obj.interfaces.contains(ctor_id)
                        || target_proto_id.is_some_and(|tp| cur_obj.interfaces.contains(&tp))
                    {
                        return true;
                    }
                    let Some(Avm1Value::Object(proto_id)) = cur_obj.props.get("__proto__") else {
                        break;
                    };
                    if Some(*proto_id) == target_proto_id || proto_id == ctor_id {
                        return true;
                    }
                    if let Some(proto_obj) = self.avm1_objects.get(*proto_id)
                        && (proto_obj.interfaces.contains(ctor_id)
                            || target_proto_id.is_some_and(|tp| proto_obj.interfaces.contains(&tp)))
                    {
                        return true;
                    }
                    cur_id = *proto_id;
                }
                false
            }
            _ => false,
        }
    }

    fn enumerate_avm1_value(&mut self, obj: &Avm1Value, stack: &mut Vec<Avm1Value>) {
        self.push_avm_stack(stack, Avm1Value::Null);
        match obj {
            Avm1Value::Object(id) => {
                let mut keys: Vec<String> = Vec::new();
                if let Some(o) = self.avm1_objects.get(*id) {
                    if let Some(elems) = &o.array_elements {
                        for idx in (0..elems.len()).rev() {
                            keys.push(idx.to_string());
                        }
                    }
                    let mut prop_keys: Vec<String> = o
                        .props
                        .keys()
                        .filter(|k| {
                            !matches!(
                                k.as_str(),
                                "__proto__"
                                    | "__constructor__"
                                    | "__super_ctor__"
                                    | "prototype"
                                    | "constructor"
                                    | "callee"
                            )
                        })
                        .cloned()
                        .collect();
                    prop_keys.sort();
                    keys.extend(prop_keys);
                }
                for k in keys {
                    self.push_avm_stack(stack, Avm1Value::String(k));
                }
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                let target = self.canonical_movie_target(mc);
                let mut keys: Vec<String> = Vec::new();
                if target == "_root" {
                    keys.extend(self.avm1_globals.keys().cloned());
                    for (&d, s) in &self.display_list {
                        keys.push(if s.name.is_empty() {
                            format!("instance{d}")
                        } else {
                            s.name.clone()
                        });
                    }
                } else if let Some(slot) = self.target_slot(&target) {
                    keys.extend(slot.props.keys().cloned());
                    for (&d, s) in &slot.children {
                        keys.push(if s.name.is_empty() {
                            format!("instance{d}")
                        } else {
                            s.name.clone()
                        });
                    }
                }
                keys.sort();
                keys.dedup();
                for k in keys {
                    self.push_avm_stack(stack, Avm1Value::String(k));
                }
            }
            _ => {}
        }
    }

    fn resolve_avm1_target_value(&self, val: &Avm1Value) -> String {
        match val {
            Avm1Value::MovieClipIdentity(id) => id.clone(),
            Avm1Value::MovieClip(mc) => self.canonical_movie_target(mc),
            _ => {
                let s = val.to_trace_string(self.swf_version, &self.avm1_objects);
                self.canonical_movie_target(&s)
            }
        }
    }

    fn target_dot_path(&self, target: &str) -> Option<String> {
        if target.is_empty() || target == "_root" || target == "_level0" {
            return Some("_level0".to_owned());
        }
        let mut cur_map = &self.display_list;
        let mut names = Vec::new();
        for seg in target.split('/').filter(|s| !s.is_empty()) {
            let depth: u16 = seg.strip_prefix("depth")?.parse().ok()?;
            let slot = cur_map.get(&depth)?;
            if slot.name.is_empty() {
                names.push(format!("instance{depth}"));
            } else {
                names.push(slot.name.clone());
            }
            cur_map = &slot.children;
        }
        Some(format!("_level0.{}", names.join(".")))
    }

    fn target_slash_path(&self, target: &str) -> String {
        if target.is_empty() || target == "_root" || target == "_level0" {
            return "/".to_owned();
        }
        let mut cur_map = &self.display_list;
        let mut names = Vec::new();
        for seg in target.split('/').filter(|s| !s.is_empty()) {
            if let Some(d_str) = seg.strip_prefix("depth")
                && let Ok(depth) = d_str.parse::<u16>()
                && let Some(slot) = cur_map.get(&depth)
            {
                if slot.name.is_empty() {
                    names.push(format!("instance{depth}"));
                } else {
                    names.push(slot.name.clone());
                }
                cur_map = &slot.children;
            } else {
                names.push(seg.to_owned());
            }
        }
        format!("/{}", names.join("/"))
    }

    fn apply_dynamic_mask(&mut self, maskee_target: &str, mask_arg: Option<&Avm1Value>) {
        if maskee_target.is_empty() || maskee_target == "_root" {
            return;
        }
        let Some(arg) = mask_arg else {
            return;
        };
        if matches!(arg, Avm1Value::Null | Avm1Value::Undefined)
            || matches!(arg, Avm1Value::String(s) if s.is_empty())
        {
            if let Some(slot) = self.target_slot_mut(maskee_target) {
                slot.masked_by = None;
            }
            return;
        }
        let mask_target = self.resolve_avm1_target_value(arg);
        if mask_target.is_empty()
            || mask_target == "_root"
            || mask_target == maskee_target
            || self.target_slot(&mask_target).is_none()
        {
            return;
        }
        // Flash 1-to-1 dynamic mask invariant: clear any previous maskee using `mask_target`.
        clear_mask_source_in_map(&mut self.display_list, &mask_target);
        if let Some(mask_slot) = self.target_slot_mut(&mask_target) {
            mask_slot.masked_by = None;
        }
        if let Some(slot) = self.target_slot_mut(maskee_target) {
            slot.masked_by = Some(mask_target);
        }
    }

    fn create_empty_movie_clip_in_target(
        &mut self,
        parent_target: &str,
        name: &str,
        script_depth: i32,
    ) -> Avm1Value {
        let swf_depth = script_depth_to_swf(script_depth);
        let new_slot = LiveSlot {
            character_id: 0,
            name: name.to_owned(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            x: 0.0,
            y: 0.0,
            scale_x: 100.0,
            scale_y: 100.0,
            rotation: 0.0,
            skew: 0.0,
            alpha: 100.0,
            color_transform: ColorTransform::default(),
            visible: true,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            clip_depth: None,
            masked_by: None,
            is_dynamic: true,
            ratio: None,
            cache_as_bitmap: false,
            current_frame: 0,
            total_frames: 1,
            playing: true,
            last_applied_frame: Some(0),
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: Vec::new(),
        };
        let seg = format!("depth{swf_depth}");
        let mut unload_work = None;
        if parent_target == "_root" || parent_target.is_empty() {
            if let Some(old) = self.display_list.get(&swf_depth) {
                unload_work = detached_unload_work(old, &seg);
            }
            self.display_list.insert(swf_depth, new_slot);
            self.prune_dangling_masks();
            if let Some(w) = unload_work {
                self.execute_avm1_work(w);
            }
            Avm1Value::MovieClipIdentity(seg)
        } else {
            let new_id = join_target_path(parent_target, &seg);
            if let Some(parent_slot) = self.target_slot_mut(parent_target) {
                if let Some(old) = parent_slot.children.get(&swf_depth) {
                    unload_work = detached_unload_work(old, &new_id);
                }
                parent_slot.children.insert(swf_depth, new_slot);
            } else {
                return Avm1Value::Undefined;
            }
            self.prune_dangling_masks();
            if let Some(w) = unload_work {
                self.execute_avm1_work(w);
            }
            Avm1Value::MovieClipIdentity(new_id)
        }
    }

    fn attach_movie_in_target(
        &mut self,
        parent_target: &str,
        linkage_id: &str,
        new_name: &str,
        script_depth: i32,
        init_obj: Option<&Avm1Value>,
    ) -> Avm1Value {
        let Some(&character_id) = self.exports.get(linkage_id) else {
            return Avm1Value::Undefined;
        };
        let swf_depth = script_depth_to_swf(script_depth);
        let total_frames = self
            .sprites
            .get(&character_id)
            .map_or(1, |s| s.total_frames.max(1));
        let new_slot = LiveSlot {
            character_id,
            name: new_name.to_owned(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            x: 0.0,
            y: 0.0,
            scale_x: 100.0,
            scale_y: 100.0,
            rotation: 0.0,
            skew: 0.0,
            alpha: 100.0,
            color_transform: ColorTransform::default(),
            visible: true,
            blend_mode: BlendMode::Normal,
            filters: Vec::new(),
            clip_depth: None,
            masked_by: None,
            is_dynamic: true,
            ratio: None,
            cache_as_bitmap: false,
            current_frame: 0,
            total_frames,
            playing: true,
            last_applied_frame: None,
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: Vec::new(),
        };
        let seg = format!("depth{swf_depth}");
        let new_id = if parent_target == "_root" || parent_target.is_empty() {
            seg.clone()
        } else {
            join_target_path(parent_target, &seg)
        };
        let mut unload_work = None;
        if parent_target == "_root" || parent_target.is_empty() {
            if let Some(old) = self.display_list.get(&swf_depth) {
                unload_work = detached_unload_work(old, &seg);
            }
            self.display_list.insert(swf_depth, new_slot);
        } else if let Some(parent_slot) = self.target_slot_mut(parent_target) {
            if let Some(old) = parent_slot.children.get(&swf_depth) {
                unload_work = detached_unload_work(old, &new_id);
            }
            parent_slot.children.insert(swf_depth, new_slot);
        } else {
            return Avm1Value::Undefined;
        }
        self.prune_dangling_masks();
        if let Some(w) = unload_work {
            self.execute_avm1_work(w);
        }
        if let Some(Avm1Value::Object(init_id)) = init_obj
            && let Some(props) = self.avm1_objects.get(*init_id).map(|o| o.props.clone())
        {
            let mc_ref = Avm1Value::MovieClipIdentity(new_id.clone());
            for (k, v) in props {
                self.set_avm1_member(&mc_ref, &k, v);
            }
        }
        let mut pending = Vec::new();
        let mut diags = Vec::new();
        let sprites = self.sprites.clone();
        let swf_ver = self.swf_version;
        if let Some(slot) = self.target_slot_mut(&new_id) {
            Self::sync_sprite_slot_recursive(
                slot,
                &sprites,
                swf_ver,
                false,
                0,
                &new_id,
                &mut pending,
                &mut diags,
            );
        }
        for d in diags {
            self.record_unsupported_diagnostic(d);
        }
        self.execute_avm1_work_list(pending);
        Avm1Value::MovieClipIdentity(new_id)
    }

    fn duplicate_movie_clip_at_swf_depth(
        &mut self,
        src_target: &str,
        new_name: &str,
        swf_depth: u16,
    ) -> Avm1Value {
        if src_target.is_empty() || src_target == "_root" {
            return Avm1Value::Undefined;
        }
        let Some(src_slot) = self.target_slot(src_target).cloned() else {
            return Avm1Value::Undefined;
        };
        let parent_target = src_target
            .rsplit_once('/')
            .map_or("_root", |(parent, _)| parent)
            .to_owned();
        let cloned = LiveSlot {
            character_id: src_slot.character_id,
            name: new_name.to_owned(),
            matrix: src_slot.matrix,
            x: src_slot.x,
            y: src_slot.y,
            scale_x: src_slot.scale_x,
            scale_y: src_slot.scale_y,
            rotation: src_slot.rotation,
            skew: src_slot.skew,
            alpha: src_slot.alpha,
            color_transform: src_slot.color_transform,
            visible: src_slot.visible,
            blend_mode: src_slot.blend_mode,
            filters: src_slot.filters,
            clip_depth: None,
            masked_by: None,
            is_dynamic: true,
            ratio: src_slot.ratio,
            cache_as_bitmap: src_slot.cache_as_bitmap,
            current_frame: 0,
            total_frames: src_slot.total_frames,
            playing: true,
            last_applied_frame: if src_slot.character_id == 0 {
                Some(0)
            } else {
                None
            },
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: src_slot.clip_handlers.clone(),
        };
        let seg = format!("depth{swf_depth}");
        let new_id = if parent_target == "_root" {
            seg.clone()
        } else {
            join_target_path(&parent_target, &seg)
        };
        let mut unload_work = None;
        if parent_target == "_root" {
            if let Some(old) = self.display_list.get(&swf_depth) {
                unload_work = detached_unload_work(old, &seg);
            }
            self.display_list.insert(swf_depth, cloned);
        } else if let Some(parent_slot) = self.target_slot_mut(&parent_target) {
            if let Some(old) = parent_slot.children.get(&swf_depth) {
                unload_work = detached_unload_work(old, &new_id);
            }
            parent_slot.children.insert(swf_depth, cloned);
        } else {
            return Avm1Value::Undefined;
        }
        self.prune_dangling_masks();
        if let Some(w) = unload_work {
            self.execute_avm1_work(w);
        }
        let mut load_work = Vec::new();
        for h in &src_slot.clip_handlers {
            if (h.event_flags & CLIP_EVENT_INITIALIZE) != 0 {
                load_work.push(pending_action(new_id.clone(), h.actions.clone()));
            }
        }
        for h in &src_slot.clip_handlers {
            if (h.event_flags & CLIP_EVENT_CONSTRUCT) != 0 {
                load_work.push(pending_action(new_id.clone(), h.actions.clone()));
            }
        }
        for h in &src_slot.clip_handlers {
            if (h.event_flags & CLIP_EVENT_LOAD) != 0 {
                load_work.push(pending_action(new_id.clone(), h.actions.clone()));
            }
        }
        self.execute_avm1_work_list(load_work);
        let mut pending = Vec::new();
        let mut diags = Vec::new();
        let sprites = self.sprites.clone();
        let swf_ver = self.swf_version;
        if let Some(slot) = self.target_slot_mut(&new_id) {
            Self::sync_sprite_slot_recursive(
                slot,
                &sprites,
                swf_ver,
                false,
                0,
                &new_id,
                &mut pending,
                &mut diags,
            );
        }
        for d in diags {
            self.record_unsupported_diagnostic(d);
        }
        self.execute_avm1_work_list(pending);
        Avm1Value::MovieClipIdentity(new_id)
    }

    fn remove_movie_clip_identity(&mut self, target: &str) {
        if target.is_empty() || target == "_root" {
            return;
        }
        let (parent_target, last_seg) = target
            .rsplit_once('/')
            .map_or(("_root", target), |(p, s)| (p, s));
        let Some(depth) = last_seg
            .strip_prefix("depth")
            .and_then(|d| d.parse::<u16>().ok())
        else {
            return;
        };
        let mut unload_work = None;
        let removed = if parent_target == "_root" {
            if self
                .display_list
                .get(&depth)
                .is_some_and(|s| s.is_dynamic || depth >= 16384)
            {
                if let Some(slot) = self.display_list.remove(&depth) {
                    unload_work = detached_unload_work(&slot, target);
                }
                true
            } else {
                false
            }
        } else if let Some(parent_slot) = self.target_slot_mut(parent_target) {
            if parent_slot
                .children
                .get(&depth)
                .is_some_and(|s| s.is_dynamic || depth >= 16384)
            {
                if let Some(slot) = parent_slot.children.remove(&depth) {
                    unload_work = detached_unload_work(&slot, target);
                }
                true
            } else {
                false
            }
        } else {
            false
        };
        if removed {
            self.prune_dangling_masks();
            if let Some(w) = unload_work {
                self.execute_avm1_work(w);
            }
        }
    }

    fn target_slot(&self, target: &str) -> Option<&LiveSlot> {
        if let Some((root_target, root_slot)) = &self.avm1_detached_target
            && let Some(slot) = find_slot_in_detached(root_target, root_slot, target)
        {
            return Some(slot);
        }
        find_slot_by_identity(&self.display_list, target)
    }

    fn target_slot_mut(&mut self, target: &str) -> Option<&mut LiveSlot> {
        if let Some((root_target, root_slot)) = &mut self.avm1_detached_target
            && (target == root_target || target.starts_with(&format!("{root_target}/")))
        {
            return find_slot_in_detached_mut(root_target, root_slot, target);
        }
        find_slot_by_identity_mut(&mut self.display_list, target)
    }

    fn canonical_movie_target(&self, target: &str) -> String {
        let target = target.trim();
        if target.is_empty() || target == "this" {
            return if self.avm1_target.is_empty() {
                "_root".to_owned()
            } else {
                self.avm1_target.clone()
            };
        }
        if target == "_root" || target == "_level0" || target == "/" {
            return "_root".to_owned();
        }
        if target == "_parent" || target == ".." {
            let cur = if self.avm1_target.is_empty() {
                "_root"
            } else {
                &self.avm1_target
            };
            return if cur == "_root" {
                String::new()
            } else if let Some((parent, _)) = cur.rsplit_once('/') {
                parent.to_owned()
            } else {
                "_root".to_owned()
            };
        }
        let explicitly_rooted = target.starts_with("_root.")
            || target.starts_with("_level0.")
            || target.starts_with('/');
        let normalized = target
            .trim_start_matches("_root.")
            .trim_start_matches("_level0.")
            .trim_start_matches('/');
        if let Some((root_target, root_slot)) = &self.avm1_detached_target
            && find_slot_in_detached(root_target, root_slot, normalized).is_some()
        {
            return normalized.to_owned();
        }
        let segments = target_segments(normalized);
        if !explicitly_rooted
            && !self.avm1_target.is_empty()
            && self.avm1_target != "_root"
            && let Some(current) = self.target_slot(&self.avm1_target)
            && let Some(identity) =
                resolve_target_identity_by_segments(&current.children, &segments, &self.avm1_target)
        {
            return identity;
        }
        if let Some(identity) = resolve_target_identity(&self.display_list, normalized) {
            return identity;
        }
        normalized.to_owned()
    }

    fn current_target_path(&self) -> String {
        self.canonical_movie_target("")
    }

    fn target_total_frames(&self, target: &str) -> u32 {
        if target == "_root" || target.is_empty() {
            self.root_timeline.frames.len().max(1) as u32
        } else {
            self.target_slot(target).map_or(1, |slot| slot.total_frames)
        }
    }

    fn target_label_frame(&self, target: &str, label: &str) -> Option<u32> {
        if target == "_root" || target.is_empty() {
            self.root_timeline.resolve_label(label)
        } else {
            let character_id = self.target_slot(target)?.character_id;
            self.sprites.get(&character_id)?.resolve_label(label)
        }
    }

    fn set_target_goto_value_identity(&mut self, target: &str, value: &Avm1Value, playing: bool) {
        let total = self.target_total_frames(target);
        let resolved = match value {
            Avm1Value::Number(number) if *number >= 1.0 => {
                Some((*number as i64).saturating_sub(1).max(0) as u32)
            }
            Avm1Value::String(label) => {
                if let Ok(number) = label.trim().parse::<u32>()
                    && number >= 1
                {
                    Some(number.saturating_sub(1))
                } else {
                    self.target_label_frame(target, label)
                }
            }
            _ => None,
        };
        let Some(frame) = resolved.map(|frame| frame.min(total.saturating_sub(1))) else {
            return;
        };
        self.set_target_frame_zero_based(target, frame, playing);
    }

    fn set_target_frame_zero_based(&mut self, target: &str, frame: u32, playing: bool) {
        if target == "_root" || target.is_empty() {
            self.current_frame = frame;
            self.playing = playing;
        } else if let Some(slot) = self.target_slot_mut(target) {
            slot.current_frame = frame;
            slot.playing = playing;
        }
    }

    fn avm1_has_member(&self, target: &Avm1Value, member: &str) -> bool {
        match target {
            Avm1Value::Object(id) => {
                let mut cur_id = *id;
                for _ in 0..32 {
                    let Some(obj) = self.avm1_objects.get(cur_id) else {
                        break;
                    };
                    if obj.props.contains_key(member) {
                        return true;
                    }
                    if let Some(elems) = &obj.array_elements
                        && (member == "length"
                            || member.parse::<usize>().is_ok_and(|idx| idx < elems.len()))
                    {
                        return true;
                    }
                    if let Some(Avm1Value::Object(proto_id)) = obj.props.get("__proto__") {
                        cur_id = *proto_id;
                    } else {
                        break;
                    }
                }
                false
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                if matches!(
                    member,
                    "_x" | "_y"
                        | "_xscale"
                        | "_yscale"
                        | "_currentframe"
                        | "_totalframes"
                        | "_framesloaded"
                        | "_alpha"
                        | "_visible"
                        | "_rotation"
                        | "_name"
                        | "_target"
                        | "_parent"
                ) {
                    return true;
                }
                let canonical = self.canonical_movie_target(mc);
                if canonical == "_root" {
                    self.avm1_globals.contains_key(member)
                        || self
                            .display_list
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, member))
                } else if let Some(slot) = self.target_slot(&canonical) {
                    slot.props.contains_key(member)
                        || slot
                            .children
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, member))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn get_avm1_variable(&self, name: &str, locals: &HashMap<String, Avm1Value>) -> Avm1Value {
        for scope in self.avm1_with_scopes.iter().rev() {
            if self.avm1_has_member(scope, name) {
                return self.get_avm1_member(scope, name);
            }
        }
        if let Some(v) = locals.get(name) {
            return v.clone();
        }
        let cur_target = self.current_target_path();
        if cur_target != "_root"
            && let Some(slot) = self.target_slot(&cur_target)
            && let Some(v) = slot.props.get(name)
        {
            return v.clone();
        }
        if let Some(v) = self.avm1_globals.get(name) {
            return v.clone();
        }
        if let Some(v) = self.avm1_global_obj.get(name) {
            return v.clone();
        }
        if let Some(func) = self.avm1_functions.get(name)
            && let Some(fn_id) = func.self_obj_id
        {
            return Avm1Value::Object(fn_id);
        }
        match name {
            "this" => {
                if cur_target == "_root" {
                    Avm1Value::MovieClip("_root".to_owned())
                } else {
                    Avm1Value::MovieClipIdentity(cur_target)
                }
            }
            "_root" | "_level0" => Avm1Value::MovieClip("_root".to_owned()),
            "_global" => Avm1Value::MovieClip("_global".to_owned()),
            "_parent" => {
                if cur_target == "_root" {
                    Avm1Value::Undefined
                } else if let Some((parent, _)) = cur_target.rsplit_once('/') {
                    Avm1Value::MovieClipIdentity(parent.to_owned())
                } else {
                    Avm1Value::MovieClip("_root".to_owned())
                }
            }
            "Math" => Avm1Value::MovieClip("Math".to_owned()),
            "Object" => Avm1Value::MovieClip("__builtin_Object".to_owned()),
            "Array" => Avm1Value::MovieClip("__builtin_Array".to_owned()),
            "Function" => Avm1Value::MovieClip("__builtin_Function".to_owned()),
            "MovieClip" => Avm1Value::MovieClip("__builtin_MovieClip".to_owned()),
            "NaN" => Avm1Value::Number(f64::NAN),
            "Infinity" => Avm1Value::Number(f64::INFINITY),
            "_x" => self.get_mc_property("", 0),
            "_y" => self.get_mc_property("", 1),
            "_xscale" => self.get_mc_property("", 2),
            "_yscale" => self.get_mc_property("", 3),
            "_currentframe" => self.get_mc_property("", 4),
            "_totalframes" => self.get_mc_property("", 5),
            "_alpha" => self.get_mc_property("", 6),
            "_visible" => self.get_mc_property("", 7),
            "_rotation" => self.get_mc_property("", 10),
            "_target" => self.get_mc_property("", 11),
            "_framesloaded" => self.get_mc_property("", 12),
            "_name" => self.get_mc_property("", 13),
            _ => {
                let target = self.canonical_movie_target(name);
                if self.target_slot(&target).is_some() {
                    Avm1Value::MovieClipIdentity(target)
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
        for scope in self.avm1_with_scopes.clone().into_iter().rev() {
            if self.avm1_has_member(&scope, &name) {
                self.set_avm1_member(&scope, &name, val);
                return;
            }
        }
        if let Some(slot) = locals.get_mut(&name) {
            *slot = val;
            return;
        }
        let cur_target = self.current_target_path();
        match name.as_str() {
            "_x" => return self.set_mc_property_identity(&cur_target, 0, val),
            "_y" => return self.set_mc_property_identity(&cur_target, 1, val),
            "_xscale" => return self.set_mc_property_identity(&cur_target, 2, val),
            "_yscale" => return self.set_mc_property_identity(&cur_target, 3, val),
            "_alpha" => return self.set_mc_property_identity(&cur_target, 6, val),
            "_visible" => return self.set_mc_property_identity(&cur_target, 7, val),
            "_rotation" => return self.set_mc_property_identity(&cur_target, 10, val),
            "_name" => return self.set_mc_property_identity(&cur_target, 13, val),
            _ => {}
        }
        if cur_target != "_root"
            && let Some(slot) = self.target_slot_mut(&cur_target)
        {
            slot.props.insert(name, val);
        } else {
            self.avm1_globals.insert(name, val);
        }
    }

    fn get_mc_property(&self, target: &str, prop_idx: i32) -> Avm1Value {
        let target = self.canonical_movie_target(target);
        self.get_mc_property_identity(&target, prop_idx)
    }

    fn get_mc_property_identity(&self, target: &str, prop_idx: i32) -> Avm1Value {
        if target == "_root" || target.is_empty() {
            return match prop_idx {
                0 => Avm1Value::Number(self.root_props.x),
                1 => Avm1Value::Number(self.root_props.y),
                2 => Avm1Value::Number(self.root_props.scale_x),
                3 => Avm1Value::Number(self.root_props.scale_y),
                4 => Avm1Value::Number(f64::from(self.current_frame + 1)),
                5 | 12 => Avm1Value::Number(f64::from(self.root_timeline.total_frames)),
                6 => Avm1Value::Number(self.root_props.alpha),
                7 => Avm1Value::Bool(self.root_props.visible),
                8 => Avm1Value::Number(self.root_props.scale_x.abs()),
                9 => Avm1Value::Number(self.root_props.scale_y.abs()),
                10 => Avm1Value::Number(self.root_props.rotation),
                11 => Avm1Value::String("/".to_owned()),
                13 => Avm1Value::String(self.root_props.name.clone()),
                14 | 15 => Avm1Value::String(String::new()),
                16 => Avm1Value::Number(1.0),
                17 => Avm1Value::Bool(true),
                18 => Avm1Value::Number(5.0),
                19 => Avm1Value::String("HIGH".to_owned()),
                20 | 21 => Avm1Value::Number(0.0),
                _ => Avm1Value::Undefined,
            };
        }
        let slot = self.target_slot(target);
        match prop_idx {
            0 => Avm1Value::Number(slot.map_or(0.0, |s| s.x)),
            1 => Avm1Value::Number(slot.map_or(0.0, |s| s.y)),
            2 => Avm1Value::Number(slot.map_or(100.0, |s| s.scale_x)),
            3 => Avm1Value::Number(slot.map_or(100.0, |s| s.scale_y)),
            4 => Avm1Value::Number(f64::from(
                slot.map_or(self.current_frame + 1, |s| s.current_frame + 1),
            )),
            5 | 12 => Avm1Value::Number(f64::from(
                slot.map_or(self.root_timeline.total_frames, |s| s.total_frames),
            )),
            6 => Avm1Value::Number(slot.map_or(100.0, |s| s.alpha)),
            7 => Avm1Value::Bool(slot.is_none_or(|s| s.visible)),
            8 => Avm1Value::Number(slot.map_or(0.0, |s| s.scale_x.abs())),
            9 => Avm1Value::Number(slot.map_or(0.0, |s| s.scale_y.abs())),
            10 => Avm1Value::Number(slot.map_or(0.0, |s| s.rotation)),
            11 => Avm1Value::String(self.target_slash_path(target)),
            13 => Avm1Value::String(slot.map_or_else(String::new, |s| s.name.clone())),
            14 | 15 => Avm1Value::String(String::new()),
            16 => Avm1Value::Number(1.0),
            17 => Avm1Value::Bool(true),
            18 => Avm1Value::Number(5.0),
            19 => Avm1Value::String("HIGH".to_owned()),
            20 | 21 => Avm1Value::Number(0.0),
            _ => Avm1Value::Undefined,
        }
    }

    fn set_mc_property(&mut self, target: &str, prop_idx: i32, val: Avm1Value) {
        let target = self.canonical_movie_target(target);
        self.set_mc_property_identity(&target, prop_idx, val);
    }

    fn set_mc_property_identity(&mut self, target: &str, prop_idx: i32, val: Avm1Value) {
        let num = val.to_number();
        let bval = val.to_bool(self.swf_version);
        let sval = val.to_trace_string(self.swf_version, &self.avm1_objects);
        if target == "_root" || target.is_empty() {
            match prop_idx {
                0 if num.is_finite() => self.root_props.x = num,
                1 if num.is_finite() => self.root_props.y = num,
                2 if num.is_finite() => self.root_props.scale_x = num,
                3 if num.is_finite() => self.root_props.scale_y = num,
                6 if num.is_finite() => self.root_props.alpha = num,
                7 => self.root_props.visible = bval,
                10 if num.is_finite() => self.root_props.rotation = num,
                13 => self.root_props.name = sval,
                _ => {}
            }
            return;
        }
        if let Some(slot) = self.target_slot_mut(target) {
            match prop_idx {
                0 if num.is_finite() => {
                    slot.x = num;
                    slot.sync_matrix_from_props();
                }
                1 if num.is_finite() => {
                    slot.y = num;
                    slot.sync_matrix_from_props();
                }
                2 if num.is_finite() => {
                    slot.scale_x = num;
                    slot.sync_matrix_from_props();
                }
                3 if num.is_finite() => {
                    slot.scale_y = num;
                    slot.sync_matrix_from_props();
                }
                6 if num.is_finite() => slot.alpha = num,
                7 => slot.visible = bval,
                10 if num.is_finite() => {
                    slot.rotation = num;
                    slot.sync_matrix_from_props();
                }
                13 => slot.name = sval,
                _ => {}
            }
        }
    }

    fn get_avm1_object_prop_chain(&self, start_id: usize, member: &str) -> Option<Avm1Value> {
        let mut cur_id = start_id;
        for _ in 0..32 {
            let obj = self.avm1_objects.get(cur_id)?;
            if let Some(elems) = &obj.array_elements {
                if member == "length" {
                    return Some(Avm1Value::Number(elems.len() as f64));
                }
                if let Ok(idx) = member.parse::<usize>() {
                    return Some(elems.get(idx).cloned().unwrap_or(Avm1Value::Undefined));
                }
            }
            if let Some(v) = obj.props.get(member) {
                return Some(v.clone());
            }
            if let Some(Avm1Value::Object(proto_id)) = obj.props.get("__proto__") {
                cur_id = *proto_id;
            } else {
                break;
            }
        }
        None
    }

    fn get_avm1_member(&self, target: &Avm1Value, member: &str) -> Avm1Value {
        match target {
            Avm1Value::Object(id) => self
                .get_avm1_object_prop_chain(*id, member)
                .unwrap_or(Avm1Value::Undefined),
            Avm1Value::Super { super_ctor, .. } => {
                if let Some(sup_id) = super_ctor
                    && let Some(Avm1Value::Object(proto_id)) = self
                        .avm1_objects
                        .get(*sup_id)
                        .and_then(|o| o.props.get("prototype"))
                {
                    self.get_avm1_object_prop_chain(*proto_id, member)
                        .unwrap_or(Avm1Value::Undefined)
                } else {
                    Avm1Value::Undefined
                }
            }
            Avm1Value::String(s) => {
                if member == "length" {
                    Avm1Value::Number(s.chars().count() as f64)
                } else {
                    Avm1Value::Undefined
                }
            }
            Avm1Value::MovieClipIdentity(target) => self.get_avm1_mc_member(target, member),
            Avm1Value::MovieClip(mc) => {
                if mc == "_global" {
                    return self
                        .avm1_global_obj
                        .get(member)
                        .or_else(|| self.avm1_globals.get(member))
                        .cloned()
                        .unwrap_or(Avm1Value::Undefined);
                }
                if mc == "Math" {
                    return match member {
                        "PI" => Avm1Value::Number(std::f64::consts::PI),
                        "E" => Avm1Value::Number(std::f64::consts::E),
                        "SQRT2" => Avm1Value::Number(std::f64::consts::SQRT_2),
                        "SQRT1_2" => Avm1Value::Number(std::f64::consts::FRAC_1_SQRT_2),
                        "LN2" => Avm1Value::Number(std::f64::consts::LN_2),
                        "LN10" => Avm1Value::Number(std::f64::consts::LN_10),
                        _ => Avm1Value::Undefined,
                    };
                }
                let canonical = self.canonical_movie_target(mc);
                self.get_avm1_mc_member(&canonical, member)
            }
            _ => Avm1Value::Undefined,
        }
    }

    fn get_avm1_mc_member(&self, target: &str, member: &str) -> Avm1Value {
        match member {
            "_x" => self.get_mc_property_identity(target, 0),
            "_y" => self.get_mc_property_identity(target, 1),
            "_xscale" => self.get_mc_property_identity(target, 2),
            "_yscale" => self.get_mc_property_identity(target, 3),
            "_currentframe" => self.get_mc_property_identity(target, 4),
            "_totalframes" => self.get_mc_property_identity(target, 5),
            "_alpha" => self.get_mc_property_identity(target, 6),
            "_visible" => self.get_mc_property_identity(target, 7),
            "_width" => self.get_mc_property_identity(target, 8),
            "_height" => self.get_mc_property_identity(target, 9),
            "_rotation" => self.get_mc_property_identity(target, 10),
            "_target" => self.get_mc_property_identity(target, 11),
            "_framesloaded" => self.get_mc_property_identity(target, 12),
            "_name" => self.get_mc_property_identity(target, 13),
            "_quality" => self.get_mc_property_identity(target, 19),
            "_xmouse" => self.get_mc_property_identity(target, 20),
            "_ymouse" => self.get_mc_property_identity(target, 21),
            "_root" | "_level0" => Avm1Value::MovieClip("_root".to_owned()),
            "_global" => Avm1Value::MovieClip("_global".to_owned()),
            "_parent" => {
                if target == "_root" || target.is_empty() {
                    Avm1Value::Undefined
                } else if let Some((parent, _)) = target.rsplit_once('/') {
                    Avm1Value::MovieClipIdentity(parent.to_owned())
                } else {
                    Avm1Value::MovieClip("_root".to_owned())
                }
            }
            _ => {
                if target == "_root" {
                    if let Some(v) = self.avm1_globals.get(member) {
                        return v.clone();
                    }
                    if let Some(func) = self.avm1_functions.get(member)
                        && let Some(fn_id) = func.self_obj_id
                    {
                        return Avm1Value::Object(fn_id);
                    }
                    if let Some((depth, slot)) =
                        self.display_list.iter().find_map(|(depth, slot)| {
                            slot_matches_segment(*depth, slot, member).then_some((*depth, slot))
                        })
                    {
                        return Avm1Value::MovieClipIdentity(live_slot_segment(depth, slot));
                    }
                    return Avm1Value::Undefined;
                }
                if let Some(slot) = self.target_slot(target) {
                    if let Some(value) = slot.props.get(member) {
                        return value.clone();
                    }
                    if let Some((depth, child)) = slot.children.iter().find_map(|(depth, child)| {
                        slot_matches_segment(*depth, child, member).then_some((*depth, child))
                    }) {
                        return Avm1Value::MovieClipIdentity(join_target_path(
                            target,
                            &live_slot_segment(depth, child),
                        ));
                    }
                }
                Avm1Value::Undefined
            }
        }
    }

    fn set_avm1_member(&mut self, target: &Avm1Value, member: &str, val: Avm1Value) {
        match target {
            Avm1Value::Object(id) => {
                if let Some(obj) = self.avm1_objects.get_mut(*id) {
                    if let Some(elems) = &mut obj.array_elements {
                        if member == "length" {
                            let new_len = val.to_i32().clamp(0, 4096) as usize;
                            elems.resize(new_len, Avm1Value::Undefined);
                            return;
                        }
                        if let Ok(idx) = member.parse::<usize>()
                            && idx < 4096
                        {
                            if idx >= elems.len() {
                                elems.resize(idx + 1, Avm1Value::Undefined);
                            }
                            elems[idx] = val;
                            return;
                        }
                    }
                    obj.props.insert(member.to_owned(), val);
                }
            }
            Avm1Value::MovieClipIdentity(target) => self.set_avm1_mc_member(target, member, val),
            Avm1Value::MovieClip(mc) => {
                if mc == "_global" {
                    self.avm1_global_obj.insert(member.to_owned(), val);
                    return;
                }
                let canonical = self.canonical_movie_target(mc);
                self.set_avm1_mc_member(&canonical, member, val);
            }
            _ => {}
        }
    }

    fn set_avm1_mc_member(&mut self, target: &str, member: &str, val: Avm1Value) {
        match member {
            "_x" => self.set_mc_property_identity(target, 0, val),
            "_y" => self.set_mc_property_identity(target, 1, val),
            "_xscale" => self.set_mc_property_identity(target, 2, val),
            "_yscale" => self.set_mc_property_identity(target, 3, val),
            "_alpha" => self.set_mc_property_identity(target, 6, val),
            "_visible" => self.set_mc_property_identity(target, 7, val),
            "_rotation" => self.set_mc_property_identity(target, 10, val),
            "_name" => self.set_mc_property_identity(target, 13, val),
            _ => {
                if target == "_root" {
                    self.avm1_globals.insert(member.to_owned(), val);
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.props.insert(member.to_owned(), val);
                }
            }
        }
    }

    fn set_target_playing(&mut self, playing: bool) {
        let target = self.current_target_path();
        if target != "_root" {
            if let Some(slot) = self.target_slot_mut(&target) {
                slot.playing = playing;
            }
            return;
        }
        self.playing = playing;
    }

    fn set_target_frame_step(&mut self, delta: i32) {
        let target = self.current_target_path();
        if target != "_root" {
            if let Some(slot) = self.target_slot_mut(&target) {
                let next = (slot.current_frame as i32 + delta)
                    .clamp(0, slot.total_frames.saturating_sub(1) as i32)
                    as u32;
                slot.current_frame = next;
                slot.playing = false;
            }
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
        caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let target = self.current_target_path();
        let this_val = if target == "_root" {
            Avm1Value::MovieClip("_root".to_owned())
        } else {
            Avm1Value::MovieClipIdentity(target.clone())
        };
        if let Some(Avm1Value::Object(id)) = caller_locals.get(fn_name).cloned()
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        if target != "_root"
            && let Some(Avm1Value::Object(id)) = self
                .target_slot(&target)
                .and_then(|s| s.props.get(fn_name).cloned())
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        if let Some(func) = self.avm1_functions.get(fn_name).cloned() {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        if let Some(Avm1Value::Object(id)) = self
            .avm1_globals
            .get(fn_name)
            .or_else(|| self.avm1_global_obj.get(fn_name))
            .cloned()
            && let Some(func) = self.avm1_objects.get(id).and_then(|o| o.function.clone())
        {
            return self.invoke_avm1_func_with_this(&func, this_val, None, args, steps, depth);
        }
        match fn_name {
            "trace" => {
                if let Some(first) = args.first() {
                    let s = first.to_trace_string(self.swf_version, &self.avm1_objects);
                    self.record_trace(s);
                }
                Avm1Value::Undefined
            }
            "int" => Avm1Value::Number(f64::from(
                args.first().unwrap_or(&Avm1Value::Undefined).to_i32(),
            )),
            "parseInt" => {
                let s = args
                    .first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_trace_string(self.swf_version, &self.avm1_objects);
                let radix = args.get(1).map_or(0, Avm1Value::to_i32);
                Avm1Value::Number(parse_avm_int_radix(&s, radix))
            }
            "Number" | "parseFloat" => {
                Avm1Value::Number(args.first().unwrap_or(&Avm1Value::Undefined).to_number())
            }
            "isNaN" => Avm1Value::Bool(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_number()
                    .is_nan(),
            ),
            "isFinite" => Avm1Value::Bool(
                args.first()
                    .unwrap_or(&Avm1Value::Undefined)
                    .to_number()
                    .is_finite(),
            ),
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
            "targetPath" => match args.first() {
                Some(Avm1Value::MovieClip(mc)) => {
                    let canonical = self.canonical_movie_target(mc);
                    self.target_dot_path(&canonical)
                        .map_or(Avm1Value::Undefined, Avm1Value::String)
                }
                Some(Avm1Value::MovieClipIdentity(t)) => self
                    .target_dot_path(t)
                    .map_or(Avm1Value::Undefined, Avm1Value::String),
                _ => Avm1Value::Undefined,
            },
            "stop" => {
                self.set_target_playing(false);
                Avm1Value::Undefined
            }
            "play" => {
                self.set_target_playing(true);
                Avm1Value::Undefined
            }
            "nextFrame" => {
                self.set_target_frame_step(1);
                Avm1Value::Undefined
            }
            "prevFrame" => {
                self.set_target_frame_step(-1);
                Avm1Value::Undefined
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    let target = self.current_target_path();
                    self.set_target_goto_value_identity(&target, first, fn_name == "gotoAndPlay");
                }
                Avm1Value::Undefined
            }
            "createEmptyMovieClip" => {
                let name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(1).map_or(0, Avm1Value::to_i32);
                self.create_empty_movie_clip_in_target(&target, &name, d)
            }
            "attachMovie" => {
                let id_name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let new_name = args.get(1).map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(2).map_or(0, Avm1Value::to_i32);
                self.attach_movie_in_target(&target, &id_name, &new_name, d, args.get(3))
            }
            "removeMovieClip" => {
                if let Some(first) = args.first() {
                    let resolved = self.resolve_avm1_target_value(first);
                    self.remove_movie_clip_identity(&resolved);
                } else {
                    self.remove_movie_clip_identity(&target);
                }
                Avm1Value::Undefined
            }
            _ => Avm1Value::Undefined,
        }
    }

    fn resolve_avm1_object_method_with_super(
        &self,
        obj_id: usize,
        method_name: &str,
    ) -> Option<(Avm1Function, Option<usize>)> {
        let mut cur_id = obj_id;
        for _ in 0..32 {
            let obj = self.avm1_objects.get(cur_id)?;
            if let Some(Avm1Value::Object(fn_id)) = obj.props.get(method_name)
                && let Some(func) = self
                    .avm1_objects
                    .get(*fn_id)
                    .and_then(|o| o.function.clone())
            {
                let next_super = obj
                    .props
                    .get("__proto__")
                    .and_then(|v| match v {
                        Avm1Value::Object(pid) => self.avm1_objects.get(*pid),
                        _ => None,
                    })
                    .and_then(|p| {
                        p.props
                            .get("constructor")
                            .or_else(|| p.props.get("__constructor__"))
                    })
                    .and_then(|v| match v {
                        Avm1Value::Object(cid) => Some(*cid),
                        _ => None,
                    });
                return Some((func, next_super));
            }
            if let Some(Avm1Value::Object(proto_id)) = obj.props.get("__proto__") {
                cur_id = *proto_id;
            } else {
                break;
            }
        }
        None
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
            Avm1Value::Super {
                this_val,
                super_ctor,
            } => {
                let Some(sup_id) = super_ctor else {
                    return Avm1Value::Undefined;
                };
                if method_name.is_empty() {
                    if let Some(func) = self
                        .avm1_objects
                        .get(*sup_id)
                        .and_then(|o| o.function.clone())
                    {
                        let next_super = self
                            .avm1_objects
                            .get(*sup_id)
                            .and_then(|o| o.props.get("__super_ctor__"))
                            .and_then(|v| match v {
                                Avm1Value::Object(id) => Some(*id),
                                _ => None,
                            });
                        return self.invoke_avm1_func_with_this(
                            &func,
                            (**this_val).clone(),
                            next_super,
                            args,
                            steps,
                            depth,
                        );
                    }
                } else if let Some(Avm1Value::Object(proto_id)) = self
                    .avm1_objects
                    .get(*sup_id)
                    .and_then(|o| o.props.get("prototype").cloned())
                    && let Some((func, next_super)) =
                        self.resolve_avm1_object_method_with_super(proto_id, method_name)
                {
                    return self.invoke_avm1_func_with_this(
                        &func,
                        (**this_val).clone(),
                        next_super,
                        args,
                        steps,
                        depth,
                    );
                }
                Avm1Value::Undefined
            }
            Avm1Value::Object(id) => {
                if method_name.is_empty()
                    && let Some(func) = self.avm1_objects.get(*id).and_then(|o| o.function.clone())
                {
                    let cur_t = self.current_target_path();
                    let this_v = if cur_t == "_root" {
                        Avm1Value::MovieClip("_root".to_owned())
                    } else {
                        Avm1Value::MovieClipIdentity(cur_t)
                    };
                    return self
                        .invoke_avm1_func_with_this(&func, this_v, None, args, steps, depth);
                }
                if (method_name == "call" || method_name == "apply")
                    && let Some(func) = self.avm1_objects.get(*id).and_then(|o| o.function.clone())
                {
                    let this_arg = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".to_owned()));
                    let call_args = if method_name == "call" {
                        args.into_iter().skip(1).collect()
                    } else if let Some(Avm1Value::Object(arr_id)) = args.get(1) {
                        self.avm1_objects
                            .get(*arr_id)
                            .and_then(|o| o.array_elements.clone())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    return self.invoke_avm1_func_with_this(
                        &func, this_arg, None, call_args, steps, depth,
                    );
                }
                if let Some((func, super_ctor)) =
                    self.resolve_avm1_object_method_with_super(*id, method_name)
                {
                    return self.invoke_avm1_func_with_this(
                        &func,
                        Avm1Value::Object(*id),
                        super_ctor,
                        args,
                        steps,
                        depth,
                    );
                }
                let swf_ver = self.swf_version;
                let has_array = self
                    .avm1_objects
                    .get(*id)
                    .is_some_and(|o| o.array_elements.is_some());
                if has_array {
                    match method_name {
                        "push" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                for a in args {
                                    if elems.len() < 4096 {
                                        elems.push(a);
                                    }
                                }
                                return Avm1Value::Number(elems.len() as f64);
                            }
                        }
                        "pop" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                return elems.pop().unwrap_or(Avm1Value::Undefined);
                            }
                        }
                        "shift" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                return if elems.is_empty() {
                                    Avm1Value::Undefined
                                } else {
                                    elems.remove(0)
                                };
                            }
                        }
                        "unshift" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                for (i, a) in args.into_iter().enumerate() {
                                    if elems.len() < 4096 {
                                        elems.insert(i, a);
                                    }
                                }
                                return Avm1Value::Number(elems.len() as f64);
                            }
                        }
                        "reverse" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get_mut(*id)
                                .and_then(|o| o.array_elements.as_mut())
                            {
                                elems.reverse();
                            }
                            return Avm1Value::Object(*id);
                        }
                        "slice" => {
                            if let Some(elems) = self
                                .avm1_objects
                                .get(*id)
                                .and_then(|o| o.array_elements.clone())
                            {
                                let len = elems.len() as i32;
                                let mut s = args.first().map_or(0, Avm1Value::to_i32);
                                let mut e = args.get(1).map_or(len, Avm1Value::to_i32);
                                if s < 0 {
                                    s = (len + s).max(0);
                                }
                                if e < 0 {
                                    e = (len + e).max(0);
                                }
                                let su = (s.clamp(0, len)) as usize;
                                let eu = (e.clamp(su as i32, len)) as usize;
                                let out_id = self.avm1_objects.len();
                                self.avm1_objects.push(Avm1Object {
                                    array_elements: Some(elems[su..eu].to_vec()),
                                    ..Default::default()
                                });
                                return Avm1Value::Object(out_id);
                            }
                        }
                        "join" | "toString" => {
                            let sep = if method_name == "join" {
                                args.first().map_or_else(
                                    || ",".to_owned(),
                                    |v| v.to_trace_string(swf_ver, &self.avm1_objects),
                                )
                            } else {
                                ",".to_owned()
                            };
                            if let Some(elems) = self
                                .avm1_objects
                                .get(*id)
                                .and_then(|o| o.array_elements.clone())
                            {
                                let parts: Vec<String> = elems
                                    .iter()
                                    .map(|v| v.to_trace_string(swf_ver, &self.avm1_objects))
                                    .collect();
                                return Avm1Value::String(parts.join(&sep));
                            }
                        }
                        _ => {}
                    }
                }
                if method_name == "hasOwnProperty" {
                    let key = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(swf_ver, &self.avm1_objects)
                    });
                    let has = self
                        .avm1_objects
                        .get(*id)
                        .is_some_and(|o| o.props.contains_key(&key));
                    return Avm1Value::Bool(has);
                }
                if method_name == "toString" {
                    return Avm1Value::String("[object Object]".to_owned());
                }
                Avm1Value::Undefined
            }
            Avm1Value::MovieClipIdentity(target) => self.call_avm1_identity_method(
                target,
                method_name,
                args,
                caller_locals,
                steps,
                depth,
            ),
            Avm1Value::MovieClip(mc) => {
                if mc == "Math" {
                    let a = args.first().map_or(f64::NAN, Avm1Value::to_number);
                    let b = args.get(1).map_or(f64::NAN, Avm1Value::to_number);
                    return match method_name {
                        "abs" => Avm1Value::Number(a.abs()),
                        "floor" => Avm1Value::Number(a.floor()),
                        "ceil" => Avm1Value::Number(a.ceil()),
                        "round" => Avm1Value::Number(if a.is_nan() {
                            f64::NAN
                        } else {
                            (a + 0.5).floor()
                        }),
                        "sqrt" => Avm1Value::Number(a.sqrt()),
                        "sin" => Avm1Value::Number(a.sin()),
                        "cos" => Avm1Value::Number(a.cos()),
                        "tan" => Avm1Value::Number(a.tan()),
                        "atan2" => Avm1Value::Number(a.atan2(b)),
                        "asin" => Avm1Value::Number(a.asin()),
                        "acos" => Avm1Value::Number(a.acos()),
                        "atan" => Avm1Value::Number(a.atan()),
                        "exp" => Avm1Value::Number(a.exp()),
                        "log" => Avm1Value::Number(a.ln()),
                        "min" => Avm1Value::Number(if a.is_nan() || b.is_nan() {
                            f64::NAN
                        } else {
                            a.min(b)
                        }),
                        "max" => Avm1Value::Number(if a.is_nan() || b.is_nan() {
                            f64::NAN
                        } else {
                            a.max(b)
                        }),
                        "pow" => Avm1Value::Number(a.powf(b)),
                        "random" => {
                            self.rng_state = self
                                .rng_state
                                .wrapping_mul(1_664_525)
                                .wrapping_add(1_013_904_223);
                            Avm1Value::Number(f64::from(self.rng_state) / 4_294_967_296.0)
                        }
                        _ => Avm1Value::Undefined,
                    };
                }
                if mc == "__builtin_String" && method_name == "fromCharCode" {
                    let s: String = args
                        .iter()
                        .filter_map(|v| char::from_u32(v.to_u32() & 0xFFFF))
                        .collect();
                    return Avm1Value::String(s);
                }
                let target = self.canonical_movie_target(mc);
                self.call_avm1_identity_method(
                    &target,
                    method_name,
                    args,
                    caller_locals,
                    steps,
                    depth,
                )
            }
            Avm1Value::String(s) => match method_name {
                "toUpperCase" => Avm1Value::String(s.to_uppercase()),
                "toLowerCase" => Avm1Value::String(s.to_lowercase()),
                "charAt" => {
                    let idx = args.first().map_or(0, Avm1Value::to_i32);
                    if idx < 0 {
                        Avm1Value::String(String::new())
                    } else {
                        Avm1Value::String(
                            s.chars()
                                .nth(idx as usize)
                                .map_or_else(String::new, |c| c.to_string()),
                        )
                    }
                }
                "charCodeAt" => {
                    let idx = args.first().map_or(0, Avm1Value::to_i32);
                    if idx < 0 {
                        Avm1Value::Number(f64::NAN)
                    } else {
                        s.chars()
                            .nth(idx as usize)
                            .map_or(Avm1Value::Number(f64::NAN), |c| {
                                Avm1Value::Number(f64::from(u32::from(c)))
                            })
                    }
                }
                "indexOf" => {
                    let needle = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    let start = args.get(1).map_or(0, Avm1Value::to_i32).max(0) as usize;
                    let chars: Vec<char> = s.chars().collect();
                    let nchars: Vec<char> = needle.chars().collect();
                    let found = if nchars.is_empty() {
                        Some(start.min(chars.len()))
                    } else if start + nchars.len() > chars.len() {
                        None
                    } else {
                        (start..=chars.len() - nchars.len())
                            .find(|&i| chars[i..i + nchars.len()] == nchars[..])
                    };
                    Avm1Value::Number(found.map_or(-1.0, |i| i as f64))
                }
                "substr" => {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i32;
                    let mut start = args.first().map_or(0, Avm1Value::to_i32);
                    if start < 0 {
                        start = (len + start).max(0);
                    }
                    let count = args.get(1).map_or(len, Avm1Value::to_i32).max(0);
                    let su = start.clamp(0, len) as usize;
                    let eu = (start.saturating_add(count)).clamp(su as i32, len) as usize;
                    Avm1Value::String(chars[su..eu].iter().collect())
                }
                "substring" | "slice" => {
                    let chars: Vec<char> = s.chars().collect();
                    let len = chars.len() as i32;
                    let mut a = args.first().map_or(0, Avm1Value::to_i32);
                    let mut b = args.get(1).map_or(len, Avm1Value::to_i32);
                    if method_name == "slice" {
                        if a < 0 {
                            a = (len + a).max(0);
                        }
                        if b < 0 {
                            b = (len + b).max(0);
                        }
                    } else {
                        a = a.clamp(0, len);
                        b = b.clamp(0, len);
                        if a > b {
                            std::mem::swap(&mut a, &mut b);
                        }
                    }
                    let su = a.clamp(0, len) as usize;
                    let eu = b.clamp(su as i32, len) as usize;
                    Avm1Value::String(chars[su..eu].iter().collect())
                }
                "split" => {
                    let sep = args.first().map_or_else(String::new, |v| {
                        v.to_trace_string(self.swf_version, &self.avm1_objects)
                    });
                    let elems: Vec<Avm1Value> = if sep.is_empty() {
                        s.chars()
                            .map(|c| Avm1Value::String(c.to_string()))
                            .collect()
                    } else {
                        s.split(&sep)
                            .map(|p| Avm1Value::String(p.to_owned()))
                            .collect()
                    };
                    let out_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object {
                        array_elements: Some(elems),
                        ..Default::default()
                    });
                    Avm1Value::Object(out_id)
                }
                _ => Avm1Value::Undefined,
            },
            _ => self.call_avm1_function(method_name, args, caller_locals, steps, depth),
        }
    }

    fn call_avm1_identity_method(
        &mut self,
        target: &str,
        method_name: &str,
        args: Vec<Avm1Value>,
        caller_locals: &mut HashMap<String, Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if target == "_root" {
            if let Some(func) = self.avm1_functions.get(method_name).cloned() {
                return self.invoke_avm1_func_with_this(
                    &func,
                    Avm1Value::MovieClip("_root".to_owned()),
                    None,
                    args,
                    steps,
                    depth,
                );
            }
            if let Some(Avm1Value::Object(fn_id)) = self.avm1_globals.get(method_name).cloned()
                && let Some(func) = self
                    .avm1_objects
                    .get(fn_id)
                    .and_then(|o| o.function.clone())
            {
                return self.invoke_avm1_func_with_this(
                    &func,
                    Avm1Value::MovieClip("_root".to_owned()),
                    None,
                    args,
                    steps,
                    depth,
                );
            }
        } else if let Some(Avm1Value::Object(function_id)) = self
            .target_slot(target)
            .and_then(|slot| slot.props.get(method_name).cloned())
            && let Some(function) = self
                .avm1_objects
                .get(function_id)
                .and_then(|object| object.function.clone())
        {
            return self.invoke_avm1_func_with_this(
                &function,
                Avm1Value::MovieClipIdentity(target.to_owned()),
                None,
                args,
                steps,
                depth,
            );
        }

        match method_name {
            "stop" => {
                if target == "_root" {
                    self.playing = false;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.playing = false;
                }
                return Avm1Value::Undefined;
            }
            "play" => {
                if target == "_root" {
                    self.playing = true;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.playing = true;
                }
                return Avm1Value::Undefined;
            }
            "nextFrame" => {
                if target == "_root" {
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    self.current_frame = (self.current_frame + 1).min(total.saturating_sub(1));
                    self.playing = false;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.current_frame =
                        (slot.current_frame + 1).min(slot.total_frames.saturating_sub(1));
                    slot.playing = false;
                }
                return Avm1Value::Undefined;
            }
            "prevFrame" => {
                if target == "_root" {
                    self.current_frame = self.current_frame.saturating_sub(1);
                    self.playing = false;
                } else if let Some(slot) = self.target_slot_mut(target) {
                    slot.current_frame = slot.current_frame.saturating_sub(1);
                    slot.playing = false;
                }
                return Avm1Value::Undefined;
            }
            "gotoAndStop" | "gotoAndPlay" => {
                if let Some(first) = args.first() {
                    self.set_target_goto_value_identity(
                        target,
                        first,
                        method_name == "gotoAndPlay",
                    );
                }
                return Avm1Value::Undefined;
            }
            "setMask" => {
                self.apply_dynamic_mask(target, args.first());
                return Avm1Value::Undefined;
            }
            "createEmptyMovieClip" => {
                let name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(1).map_or(0, Avm1Value::to_i32);
                return self.create_empty_movie_clip_in_target(target, &name, d);
            }
            "attachMovie" => {
                let id_name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let new_name = args.get(1).map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(2).map_or(0, Avm1Value::to_i32);
                return self.attach_movie_in_target(target, &id_name, &new_name, d, args.get(3));
            }
            "duplicateMovieClip" => {
                let new_name = args.first().map_or_else(String::new, |v| {
                    v.to_trace_string(self.swf_version, &self.avm1_objects)
                });
                let d = args.get(1).map_or(0, Avm1Value::to_i32);
                let swf_depth = script_depth_to_swf(d);
                let res = self.duplicate_movie_clip_at_swf_depth(target, &new_name, swf_depth);
                if let (Avm1Value::MovieClipIdentity(new_id), Some(Avm1Value::Object(init_id))) =
                    (&res, args.get(2))
                    && let Some(props) = self.avm1_objects.get(*init_id).map(|o| o.props.clone())
                {
                    let mc_ref = Avm1Value::MovieClipIdentity(new_id.clone());
                    for (k, v) in props {
                        self.set_avm1_member(&mc_ref, &k, v);
                    }
                }
                return res;
            }
            "removeMovieClip" => {
                self.remove_movie_clip_identity(target);
                return Avm1Value::Undefined;
            }
            "getDepth" => {
                if target == "_root" {
                    return Avm1Value::Number(-16384.0);
                }
                let last_seg = target.rsplit_once('/').map_or(target, |(_, s)| s);
                if let Some(d) = last_seg
                    .strip_prefix("depth")
                    .and_then(|s| s.parse::<u16>().ok())
                {
                    return Avm1Value::Number(f64::from(i32::from(d) - 16384));
                }
                return Avm1Value::Undefined;
            }
            "getNextHighestDepth" => {
                let map_opt = if target == "_root" {
                    Some(&self.display_list)
                } else {
                    self.target_slot(target).map(|s| &s.children)
                };
                if let Some(map) = map_opt {
                    let next = map
                        .keys()
                        .copied()
                        .filter(|&d| d >= 16384)
                        .max()
                        .map_or(0, |max_d| i32::from(max_d) - 16384 + 1);
                    return Avm1Value::Number(f64::from(next));
                }
                return Avm1Value::Number(0.0);
            }
            "swapDepths" => {
                if target != "_root"
                    && let Some(first) = args.first()
                {
                    self.swap_clip_depths(target, first);
                }
                return Avm1Value::Undefined;
            }
            "startDrag" => {
                self.drag_target = Some(target.to_owned());
                return Avm1Value::Undefined;
            }
            "stopDrag" => {
                self.drag_target = None;
                return Avm1Value::Undefined;
            }
            _ => {}
        }

        if target == "_root" {
            let saved_target = std::mem::replace(&mut self.avm1_target, "_root".to_owned());
            let result = self.call_avm1_function(method_name, args, caller_locals, steps, depth);
            self.avm1_target = saved_target;
            return result;
        }
        Avm1Value::Undefined
    }

    fn swap_clip_depths(&mut self, target: &str, arg: &Avm1Value) {
        let (parent_target, last_seg) = target
            .rsplit_once('/')
            .map_or(("_root", target), |(p, s)| (p, s));
        let Some(src_depth) = last_seg
            .strip_prefix("depth")
            .and_then(|s| s.parse::<u16>().ok())
        else {
            return;
        };
        let dst_depth = match arg {
            Avm1Value::Number(_) => script_depth_to_swf(arg.to_i32()),
            _ => {
                let other = self.resolve_avm1_target_value(arg);
                let (other_parent, other_seg) = other
                    .rsplit_once('/')
                    .map_or(("_root", other.as_str()), |(p, s)| (p, s));
                if other_parent != parent_target {
                    return;
                }
                let Some(d) = other_seg
                    .strip_prefix("depth")
                    .and_then(|s| s.parse::<u16>().ok())
                else {
                    return;
                };
                d
            }
        };
        if src_depth == dst_depth {
            return;
        }
        let map = if parent_target == "_root" {
            &mut self.display_list
        } else if let Some(slot) = self.target_slot_mut(parent_target) {
            &mut slot.children
        } else {
            return;
        };
        let Some(src_slot) = map.remove(&src_depth) else {
            return;
        };
        if let Some(dst_slot) = map.remove(&dst_depth) {
            map.insert(src_depth, dst_slot);
        }
        map.insert(dst_depth, src_slot);
    }

    fn invoke_avm1_func_with_this(
        &mut self,
        func: &Avm1Function,
        this_val: Avm1Value,
        super_ctor: Option<usize>,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let saved_pool =
            std::mem::replace(&mut self.avm1_constant_pool, func.constant_pool.clone());
        let saved_regs = self.avm1_registers.clone();
        let saved_target = std::mem::replace(&mut self.avm1_target, func.base_target.clone());
        let saved_with = std::mem::take(&mut self.avm1_with_scopes);

        for r in 0..usize::from(func.register_count) {
            self.avm1_registers[r] = Avm1Value::Undefined;
        }

        let mut fn_locals = HashMap::new();
        let super_val = Avm1Value::Super {
            this_val: Box::new(this_val.clone()),
            super_ctor,
        };

        if func.is_v2 {
            let flags = func.flags;
            let mut next_reg = 1usize;

            // 1. preload_this (0x0001) / suppress_this (0x0002)
            if (flags & 0x0001) != 0 {
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = this_val.clone();
                }
                next_reg += 1;
            } else if (flags & 0x0002) == 0 {
                fn_locals.insert("this".to_owned(), this_val.clone());
            }

            // 2. preload_arguments (0x0004) / suppress_arguments (0x0008)
            if (flags & 0x0004) != 0 || (flags & 0x0008) == 0 {
                let mut arg_props = HashMap::new();
                if let Some(fn_id) = func.self_obj_id {
                    arg_props.insert("callee".to_owned(), Avm1Value::Object(fn_id));
                }
                let args_obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    props: arg_props,
                    array_elements: Some(args.clone()),
                    ..Default::default()
                });
                if (flags & 0x0004) != 0 {
                    if next_reg < 256 {
                        self.avm1_registers[next_reg] = Avm1Value::Object(args_obj_id);
                    }
                    next_reg += 1;
                } else {
                    fn_locals.insert("arguments".to_owned(), Avm1Value::Object(args_obj_id));
                }
            }

            // 3. preload_super (0x0010) / suppress_super (0x0020)
            if (flags & 0x0010) != 0 {
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = super_val;
                }
                next_reg += 1;
            } else if (flags & 0x0020) == 0 {
                fn_locals.insert("super".to_owned(), super_val);
            }

            // 4. preload_root (0x0040)
            if (flags & 0x0040) != 0 {
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = Avm1Value::MovieClip("_root".to_owned());
                }
                next_reg += 1;
            }

            // 5. preload_parent (0x0080)
            if (flags & 0x0080) != 0 {
                let parent_val = match &this_val {
                    Avm1Value::MovieClipIdentity(t) if t != "_root" && !t.is_empty() => {
                        if let Some((p, _)) = t.rsplit_once('/') {
                            Avm1Value::MovieClipIdentity(p.to_owned())
                        } else {
                            Avm1Value::MovieClip("_root".to_owned())
                        }
                    }
                    _ => {
                        let base = self.canonical_movie_target(&func.base_target);
                        if base == "_root" || base.is_empty() {
                            Avm1Value::Undefined
                        } else if let Some((p, _)) = base.rsplit_once('/') {
                            Avm1Value::MovieClipIdentity(p.to_owned())
                        } else {
                            Avm1Value::MovieClip("_root".to_owned())
                        }
                    }
                };
                if next_reg < 256 {
                    self.avm1_registers[next_reg] = parent_val;
                }
                next_reg += 1;
            }

            // 6. preload_global (0x0100)
            if (flags & 0x0100) != 0 && next_reg < 256 {
                self.avm1_registers[next_reg] = Avm1Value::MovieClip("_global".to_owned());
            }

            for (i, (reg, name)) in func.params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(Avm1Value::Undefined);
                if *reg != 0 {
                    self.avm1_registers[usize::from(*reg)] = val;
                } else if !name.is_empty() {
                    fn_locals.insert(name.clone(), val);
                }
            }
        } else {
            fn_locals.insert("this".to_owned(), this_val);
            fn_locals.insert("super".to_owned(), super_val);
            let mut arg_props = HashMap::new();
            if let Some(fn_id) = func.self_obj_id {
                arg_props.insert("callee".to_owned(), Avm1Value::Object(fn_id));
            }
            let args_obj_id = self.avm1_objects.len();
            self.avm1_objects.push(Avm1Object {
                props: arg_props,
                array_elements: Some(args.clone()),
                ..Default::default()
            });
            fn_locals.insert("arguments".to_owned(), Avm1Value::Object(args_obj_id));
            for (i, (_, name)) in func.params.iter().enumerate() {
                let val = args.get(i).cloned().unwrap_or(Avm1Value::Undefined);
                if !name.is_empty() {
                    fn_locals.insert(name.clone(), val);
                }
            }
        }

        let mut fn_stack = Vec::new();
        let ret = self
            .run_avm1_code(&func.body, &mut fn_stack, &mut fn_locals, steps, depth + 1)
            .unwrap_or(Avm1Value::Undefined);
        self.avm1_constant_pool = saved_pool;
        self.avm1_registers = saved_regs;
        self.avm1_target = saved_target;
        self.avm1_with_scopes = saved_with;
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
        self.run_avm2_method_with_this(
            abc_idx,
            method_idx,
            Avm1Value::MovieClip("_root".to_owned()),
            args,
            steps,
            depth,
        )
    }

    fn run_avm2_method_with_this(
        &mut self,
        abc_idx: usize,
        method_idx: usize,
        this_val: Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if depth > MAX_CALL_DEPTH {
            self.record_unsupported_diagnostic(format!(
                "AVM2 call depth exceeded bounded limit ({MAX_CALL_DEPTH})"
            ));
            return Avm1Value::Undefined;
        }
        let Some((code, declared_locals, param_count, opt_defaults)) =
            self.abc_tags.get(abc_idx).and_then(|l| {
                let body = l.abc.bodies_by_method.get(&method_idx)?;
                let (pcount, opts) = l.abc.methods.get(method_idx).map_or_else(
                    || (0, Vec::new()),
                    |m| {
                        let defs: Vec<Avm1Value> = m
                            .optional_values
                            .iter()
                            .map(|&(vidx, vkind)| abc_const_to_avm_value(&l.abc, vidx, vkind))
                            .collect();
                        (m.param_types.len(), defs)
                    },
                );
                Some((body.code.clone(), body.local_count, pcount, opts))
            })
        else {
            return Avm1Value::Undefined;
        };

        let local_slots = declared_locals
            .max(param_count + 1)
            .max(args.len() + 1)
            .clamp(64, 1024);
        let mut locals: Vec<Avm1Value> = vec![Avm1Value::Undefined; local_slots];
        locals[0] = this_val.clone();
        let given_len = args.len();
        for (i, arg) in args.into_iter().enumerate() {
            if i + 1 < locals.len() {
                locals[i + 1] = arg;
            }
        }
        if given_len < param_count && !opt_defaults.is_empty() {
            let first_opt = param_count.saturating_sub(opt_defaults.len());
            for i in given_len.max(first_opt)..param_count {
                if i + 1 < locals.len()
                    && let Some(def_val) = opt_defaults.get(i - first_opt)
                {
                    locals[i + 1] = def_val.clone();
                }
            }
        }

        let mut stack: Vec<Avm1Value> = Vec::new();
        let mut scope_stack: Vec<Avm1Value> = Vec::new();
        let mut r = AbcReader::new(&code);

        while r.pos < code.len() && *steps < MAX_VM_STEPS_PER_FRAME {
            *steps += 1;
            let insn_start = r.pos;
            let Some(op) = r.u8() else { break };
            match op {
                0x02 => {} // nop
                0x09 => {} // label
                0x0C..=0x0F => {
                    // ifnlt (0x0C), ifnle (0x0D), ifngt (0x0E), ifnge (0x0F)
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let cond = match op {
                        0x0C => avm2_less_than(&a, &b) != Some(true),
                        0x0D => avm2_less_than(&b, &a) != Some(false),
                        0x0E => avm2_less_than(&b, &a) != Some(true),
                        _ => avm2_less_than(&a, &b) != Some(false),
                    };
                    if cond {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
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
                    // ifeq (0x13), ifne (0x14), iflt (0x15), ifle (0x16), ifgt (0x17), ifge (0x18), ifstricteq (0x19), ifstrictne (0x1A)
                    let Some(rel) = r.s24() else { break };
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let cond = match op {
                        0x13 => self.avm2_equals(&a, &b),
                        0x14 => !self.avm2_equals(&a, &b),
                        0x15 => avm2_less_than(&a, &b) == Some(true),
                        0x16 => avm2_less_than(&b, &a) == Some(false),
                        0x17 => avm2_less_than(&b, &a) == Some(true),
                        0x18 => avm2_less_than(&a, &b) == Some(false),
                        0x19 => self.avm2_strict_equals(&a, &b),
                        _ => !self.avm2_strict_equals(&a, &b),
                    };
                    if cond {
                        let target = (r.pos as isize) + (rel as isize);
                        if target >= 0 && (target as usize) <= code.len() {
                            r.pos = target as usize;
                        }
                    }
                }
                0x1B => {
                    // lookupswitch (offsets relative to `insn_start`)
                    let Some(default_off) = r.s24() else { break };
                    let Some(max_idx) = r.u30() else { break };
                    let mut case_offs = Vec::with_capacity(max_idx.saturating_add(1).min(4096));
                    let mut ok = true;
                    for _ in 0..=max_idx.min(4096) {
                        if let Some(off) = r.s24() {
                            case_offs.push(off);
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if !ok {
                        break;
                    }
                    let idx = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let rel = if idx >= 0 && (idx as usize) < case_offs.len() {
                        case_offs[idx as usize]
                    } else {
                        default_off
                    };
                    let target = (insn_start as isize) + (rel as isize);
                    if target >= 0 && (target as usize) <= code.len() {
                        r.pos = target as usize;
                    } else {
                        break;
                    }
                }
                0x1C | 0x30 => {
                    // pushwith / pushscope
                    let sc = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if scope_stack.len() < 64 {
                        scope_stack.push(sc);
                    }
                }
                0x1D => {
                    // popscope
                    let _ = scope_stack.pop();
                }
                0x1E => {
                    // nextname
                    let idx_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let keys = self.avm2_enumerable_keys(&obj);
                    let k_idx = ecma_f64_to_i32(avm2_to_number(&idx_val))
                        .saturating_sub(1)
                        .max(0) as usize;
                    let val = keys
                        .get(k_idx)
                        .cloned()
                        .map_or(Avm1Value::Undefined, Avm1Value::String);
                    self.push_avm_stack(&mut stack, val);
                }
                0x1F => {
                    // hasnext
                    let idx_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let keys = self.avm2_enumerable_keys(&obj);
                    let cur_idx = ecma_f64_to_i32(avm2_to_number(&idx_val)).max(0) as usize;
                    let next = if cur_idx < keys.len() {
                        (cur_idx + 1) as f64
                    } else {
                        0.0
                    };
                    self.push_avm_stack(&mut stack, Avm1Value::Number(next));
                }
                0x20 => self.push_avm_stack(&mut stack, Avm1Value::Null),
                0x21 => self.push_avm_stack(&mut stack, Avm1Value::Undefined),
                0x23 => {
                    // nextvalue
                    let idx_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let keys = self.avm2_enumerable_keys(&obj);
                    let k_idx = ecma_f64_to_i32(avm2_to_number(&idx_val))
                        .saturating_sub(1)
                        .max(0) as usize;
                    let val = if let Some(k) = keys.get(k_idx) {
                        self.get_avm2_property_eval(&obj, k, steps, depth)
                    } else {
                        Avm1Value::Undefined
                    };
                    self.push_avm_stack(&mut stack, val);
                }
                0x24 => {
                    // pushbyte
                    let Some(b) = r.i8() else { break };
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(b)));
                }
                0x25 => {
                    // pushshort (signed s30 / i16)
                    let Some(v) = r.s32() else { break };
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(v as i16)));
                }
                0x26 => self.push_avm_stack(&mut stack, Avm1Value::Bool(true)),
                0x27 => self.push_avm_stack(&mut stack, Avm1Value::Bool(false)),
                0x28 => self.push_avm_stack(&mut stack, Avm1Value::Number(f64::NAN)),
                0x29 => {
                    let _ = stack.pop();
                }
                0x2A => {
                    let top = stack.last().cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, top);
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
                    self.push_avm_stack(&mut stack, Avm1Value::String(s));
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
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(v)));
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
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(v)));
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
                    self.push_avm_stack(&mut stack, Avm1Value::Number(v));
                }
                0x32 => {
                    // hasnext2
                    let Some(obj_reg) = r.u30() else { break };
                    let Some(idx_reg) = r.u30() else { break };
                    let obj = locals.get(obj_reg).cloned().unwrap_or(Avm1Value::Undefined);
                    let cur_idx = locals
                        .get(idx_reg)
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)).max(0) as usize);
                    let keys = self.avm2_enumerable_keys(&obj);
                    if cur_idx < keys.len() {
                        if let Some(slot) = locals.get_mut(idx_reg) {
                            *slot = Avm1Value::Number((cur_idx + 1) as f64);
                        }
                        self.push_avm_stack(&mut stack, Avm1Value::Bool(true));
                    } else {
                        if let Some(slot) = locals.get_mut(obj_reg) {
                            *slot = Avm1Value::Null;
                        }
                        if let Some(slot) = locals.get_mut(idx_reg) {
                            *slot = Avm1Value::Number(0.0);
                        }
                        self.push_avm_stack(&mut stack, Avm1Value::Bool(false));
                    }
                }
                0x40 => {
                    // newfunction
                    let Some(midx) = r.u30() else { break };
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::String(format!("__avm2_fn:{abc_idx}:{midx}")),
                    );
                }
                0x41 => {
                    // call
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let fn_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret =
                        self.invoke_avm2_callable_with_this(&fn_val, receiver, cargs, steps, depth);
                    self.push_avm_stack(&mut stack, ret);
                }
                0x42 => {
                    // construct
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let ctor = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let constructed =
                        self.construct_avm2_value(abc_idx, &ctor, cargs, steps, depth);
                    self.push_avm_stack(&mut stack, constructed);
                }
                0x45 | 0x4E => {
                    // callsuper (0x45), callsupervoid (0x4E)
                    let Some(mn_idx) = r.u30() else { break };
                    let Some(argc) = r.u30() else { break };
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret =
                        self.call_avm2_super_method(&receiver, &prop_name, cargs, steps, depth);
                    if op == 0x45 {
                        self.push_avm_stack(&mut stack, ret);
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
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ret = self
                        .call_avm2_property(abc_idx, &receiver, &prop_name, cargs, steps, depth);
                    if op != 0x4F {
                        self.push_avm_stack(&mut stack, ret);
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
                    let mut cargs = Vec::with_capacity(argc.min(64));
                    for _ in 0..argc {
                        cargs.push(stack.pop().unwrap_or(Avm1Value::Undefined));
                    }
                    cargs.reverse();
                    let receiver = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if let Avm1Value::Object(oid) = receiver
                        && let Some((c_abc, c_idx)) =
                            self.avm1_objects.get(oid).and_then(|o| o.avm2_class)
                        && let Some((sup_abc, sup_cls)) = self.avm2_superclass(c_abc, c_idx)
                    {
                        let sup_iinit = self.abc_tags[sup_abc].abc.instances[sup_cls].iinit;
                        let _ = self.run_avm2_method_with_this(
                            sup_abc,
                            sup_iinit,
                            Avm1Value::Object(oid),
                            cargs,
                            steps,
                            depth + 1,
                        );
                    }
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
                    let (pkg, cls_name) = self.pop_avm2_qname(abc_idx, mn_idx, &mut stack);
                    let _receiver = stack.pop();
                    let constructed =
                        self.construct_avm2_named(abc_idx, &pkg, &cls_name, cargs, steps, depth);
                    self.push_avm_stack(&mut stack, constructed);
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
                        ..Default::default()
                    });
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
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
                        array_elements: Some(elems),
                        ..Default::default()
                    });
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
                }
                0x57 => {
                    // newactivation
                    let obj_id = self.avm1_objects.len();
                    self.avm1_objects.push(Avm1Object::default());
                    self.push_avm_stack(&mut stack, Avm1Value::Object(obj_id));
                }
                0x58 => {
                    // newclass
                    let Some(class_idx) = r.u30() else { break };
                    let _base = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::MovieClip(format!("__avm2_class:{abc_idx}:{class_idx}")),
                    );
                }
                0x5B | 0x5C => {
                    // findpropglobalstrict / findpropglobal
                    let Some(mn_idx) = r.u30() else { break };
                    let name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    self.push_avm_stack(&mut stack, Avm1Value::MovieClip(name));
                }
                0x5D | 0x5E => {
                    // findpropstrict / findproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let mut resolved = None;
                    for sc in scope_stack.iter().rev() {
                        if let Avm1Value::Object(oid) = sc
                            && self.avm2_object_has_prop(*oid, &name)
                        {
                            resolved = Some(Avm1Value::Object(*oid));
                            break;
                        }
                    }
                    if resolved.is_none()
                        && let Avm1Value::Object(oid) = &this_val
                        && self.avm2_object_has_prop(*oid, &name)
                    {
                        resolved = Some(Avm1Value::Object(*oid));
                    }
                    self.push_avm_stack(&mut stack, resolved.unwrap_or(Avm1Value::MovieClip(name)));
                }
                0x60 => {
                    // getlex
                    let Some(mn_idx) = r.u30() else { break };
                    let (pkg, name) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let mut val_opt = None;
                    for sc in scope_stack.iter().rev() {
                        if let Avm1Value::Object(oid) = sc
                            && self.avm2_object_has_prop(*oid, &name)
                        {
                            val_opt = Some(self.get_avm2_property_eval(
                                &Avm1Value::Object(*oid),
                                &name,
                                steps,
                                depth,
                            ));
                            break;
                        }
                    }
                    if val_opt.is_none()
                        && let Avm1Value::Object(oid) = &this_val
                        && self.avm2_object_has_prop(*oid, &name)
                    {
                        val_opt = Some(self.get_avm2_property_eval(
                            &Avm1Value::Object(*oid),
                            &name,
                            steps,
                            depth,
                        ));
                    }
                    let val = if let Some(v) = val_opt {
                        v
                    } else if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, &pkg, &name)
                    {
                        Avm1Value::MovieClip(format!("__avm2_class:{c_abc}:{c_idx}"))
                    } else if matches!(
                        name.as_str(),
                        "Object"
                            | "Array"
                            | "String"
                            | "Number"
                            | "int"
                            | "uint"
                            | "Boolean"
                            | "Function"
                            | "MovieClip"
                            | "Sprite"
                            | "DisplayObject"
                            | "DisplayObjectContainer"
                            | "InteractiveObject"
                    ) {
                        Avm1Value::MovieClip(format!("__avm2_type:{name}"))
                    } else if name == "NaN" {
                        Avm1Value::Number(f64::NAN)
                    } else if name == "Infinity" {
                        Avm1Value::Number(f64::INFINITY)
                    } else {
                        self.get_avm2_property_eval(
                            &Avm1Value::MovieClip("_root".into()),
                            &name,
                            steps,
                            depth,
                        )
                    };
                    self.push_avm_stack(&mut stack, val);
                }
                0x61 | 0x68 => {
                    // setproperty / initproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.set_avm2_property_eval(&obj, &prop_name, val, steps, depth);
                }
                0x62 => {
                    // getlocal
                    let Some(reg) = r.u30() else { break };
                    let val = locals.get(reg).cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, val);
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
                    let gs = scope_stack
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".into()));
                    self.push_avm_stack(&mut stack, gs);
                }
                0x65 => {
                    // getscopeobject
                    let Some(idx) = r.u8() else { break };
                    let sc = scope_stack
                        .get(usize::from(idx))
                        .cloned()
                        .unwrap_or_else(|| Avm1Value::MovieClip("_root".into()));
                    self.push_avm_stack(&mut stack, sc);
                }
                0x66 => {
                    // getproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let val = self.get_avm2_property_eval(&obj, &prop_name, steps, depth);
                    self.push_avm_stack(&mut stack, val);
                }
                0x67 => {
                    // getouterscope
                    let Some(_idx) = r.u30() else { break };
                    self.push_avm_stack(&mut stack, Avm1Value::MovieClip("_root".into()));
                }
                0x6A => {
                    // deleteproperty
                    let Some(mn_idx) = r.u30() else { break };
                    let prop_name = self.pop_avm2_multiname(abc_idx, mn_idx, &mut stack);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let deleted = self.delete_avm2_property(&obj, &prop_name);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(deleted));
                }
                0x6C => {
                    // getslot
                    let Some(slot_idx) = r.u30() else { break };
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__slot_{slot_idx}");
                    let val = self.get_avm2_property_eval(&obj, &key, steps, depth);
                    self.push_avm_stack(&mut stack, val);
                }
                0x6D => {
                    // setslot
                    let Some(slot_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__slot_{slot_idx}");
                    self.set_avm2_property_eval(&obj, &key, val, steps, depth);
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
                    self.push_avm_stack(&mut stack, val);
                }
                0x6F => {
                    // setglobalslot
                    let Some(slot_idx) = r.u30() else { break };
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = format!("__gslot_{slot_idx}");
                    self.avm2_globals.insert(key, val);
                }
                0x70 => {
                    // convert_s: null -> "null", undefined -> "undefined"
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let s = avm2_convert_string(&v, &self.avm1_objects);
                    self.push_avm_stack(&mut stack, Avm1Value::String(s));
                }
                0x73 | 0x83 => {
                    // convert_i / coerce_i
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let i = ecma_f64_to_i32(avm2_to_number(&v));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(i)));
                }
                0x74 | 0x88 => {
                    // convert_u / coerce_u
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let u = ecma_f64_to_u32(avm2_to_number(&v));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(u)));
                }
                0x75 | 0x84 => {
                    // convert_d / coerce_d
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, Avm1Value::Number(avm2_to_number(&v)));
                }
                0x76 | 0x81 => {
                    // convert_b / coerce_b
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(b));
                }
                0x77 => {
                    // convert_o
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if matches!(v, Avm1Value::Undefined | Avm1Value::Null) {
                        self.record_unsupported_diagnostic(format!(
                            "AVM2 convert_o on null/undefined in abc#{abc_idx} method#{method_idx} offset=0x{insn_start:x}"
                        ));
                        self.push_avm_stack(&mut stack, Avm1Value::Null);
                    } else {
                        self.push_avm_stack(&mut stack, v);
                    }
                }
                0x80 => {
                    // coerce(multiname)
                    let Some(mn_idx) = r.u30() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = self.avm2_coerce_multiname(abc_idx, mn_idx, v);
                    self.push_avm_stack(&mut stack, out);
                }
                0x82 => {
                    // coerce_a (any `*`: preserve exact value including undefined/null)
                }
                0x85 => {
                    // coerce_s: null/undefined -> null; otherwise convert to String
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = match v {
                        Avm1Value::Undefined | Avm1Value::Null => Avm1Value::Null,
                        other => Avm1Value::String(avm2_convert_string(&other, &self.avm1_objects)),
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x86 => {
                    // astype(multiname)
                    let Some(mn_idx) = r.u30() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let out = if self.avm2_is_type_name(abc_idx, &v, &pkg, &tname) {
                        v
                    } else {
                        Avm1Value::Null
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x87 => {
                    // astypelate
                    let type_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = if self.avm2_is_type_value(abc_idx, &v, &type_val) {
                        v
                    } else {
                        Avm1Value::Null
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x89 => {
                    // coerce_o: undefined -> null, else preserve
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let out = if matches!(v, Avm1Value::Undefined) {
                        Avm1Value::Null
                    } else {
                        v
                    };
                    self.push_avm_stack(&mut stack, out);
                }
                0x90 => {
                    // negate
                    let n = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(-n));
                }
                0xC4 => {
                    // negate_i
                    let n = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(n.wrapping_neg())));
                }
                0x91 => {
                    // increment
                    let n = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(n + 1.0));
                }
                0xC0 => {
                    // increment_i
                    let n = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(n.wrapping_add(1))),
                    );
                }
                0x92 => {
                    // inclocal
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        *slot = Avm1Value::Number(avm2_to_number(slot) + 1.0);
                    }
                }
                0xC2 => {
                    // inclocal_i
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        let next = ecma_f64_to_i32(avm2_to_number(slot)).wrapping_add(1);
                        *slot = Avm1Value::Number(f64::from(next));
                    }
                }
                0x93 => {
                    // decrement
                    let n = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(n - 1.0));
                }
                0xC1 => {
                    // decrement_i
                    let n = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(n.wrapping_sub(1))),
                    );
                }
                0x94 => {
                    // declocal
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        *slot = Avm1Value::Number(avm2_to_number(slot) - 1.0);
                    }
                }
                0xC3 => {
                    // declocal_i
                    let Some(reg) = r.u30() else { break };
                    if let Some(slot) = locals.get_mut(reg) {
                        let next = ecma_f64_to_i32(avm2_to_number(slot)).wrapping_sub(1);
                        *slot = Avm1Value::Number(f64::from(next));
                    }
                }
                0x95 => {
                    // typeof
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let ty = self.avm2_typeof(&v);
                    self.push_avm_stack(&mut stack, Avm1Value::String(ty.to_owned()));
                }
                0x96 => {
                    // not
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined).to_bool(9);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(!b));
                }
                0x97 => {
                    // bitnot
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let i = ecma_f64_to_i32(avm2_to_number(&v));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(!i)));
                }
                0xA0 => {
                    // add
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a_str = matches!(&a, Avm1Value::String(s) if !is_avm2_fn_token(s));
                    let b_str = matches!(&b, Avm1Value::String(s) if !is_avm2_fn_token(s));
                    if a_str || b_str {
                        let sa = avm2_convert_string(&a, &self.avm1_objects);
                        let sb = avm2_convert_string(&b, &self.avm1_objects);
                        self.push_avm_stack(&mut stack, Avm1Value::String(format!("{sa}{sb}")));
                    } else {
                        self.push_avm_stack(
                            &mut stack,
                            Avm1Value::Number(avm2_to_number(&a) + avm2_to_number(&b)),
                        );
                    }
                }
                0xA1 => {
                    // subtract
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a - b));
                }
                0xA2 => {
                    // multiply
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a * b));
                }
                0xA3 => {
                    // divide
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a / b));
                }
                0xA4 => {
                    // modulo
                    let b = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    let a = avm2_to_number(&stack.pop().unwrap_or(Avm1Value::Undefined));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(a % b));
                }
                0xA5 => {
                    let b = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_shl(b))),
                    );
                }
                0xA6 => {
                    let b = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_shr(b))),
                    );
                }
                0xA7 => {
                    let b = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_u32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_shr(b))),
                    );
                }
                0xA8 => {
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(a & b)));
                }
                0xA9 => {
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(a | b)));
                }
                0xAA => {
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(&mut stack, Avm1Value::Number(f64::from(a ^ b)));
                }
                0xAB => {
                    // equals (ECMA-357 / AS3 abstract equality)
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = self.avm2_equals(&a, &b);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(eq));
                }
                0xAC => {
                    // strictequals (AS3 strict equality)
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let eq = self.avm2_strict_equals(&a, &b);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(eq));
                }
                0xAD => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&a, &b) == Some(true)),
                    );
                }
                0xAE => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&b, &a) == Some(false)),
                    );
                }
                0xAF => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&b, &a) == Some(true)),
                    );
                }
                0xB0 => {
                    let b = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let a = stack.pop().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Bool(avm2_less_than(&a, &b) == Some(false)),
                    );
                }
                0xB1 => {
                    // instanceof
                    let type_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let res = self.avm2_instanceof_value(abc_idx, &v, &type_val);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(res));
                }
                0xB2 => {
                    // istype(multiname)
                    let Some(mn_idx) = r.u30() else { break };
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
                    let is_t = self.avm2_is_type_name(abc_idx, &v, &pkg, &tname);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(is_t));
                }
                0xB3 => {
                    // istypelate
                    let type_val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let v = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let is_t = self.avm2_is_type_value(abc_idx, &v, &type_val);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(is_t));
                }
                0xB4 => {
                    // in
                    let obj = stack.pop().unwrap_or(Avm1Value::Undefined);
                    let key = stack
                        .pop()
                        .unwrap_or(Avm1Value::Undefined)
                        .to_trace_string(9, &self.avm1_objects);
                    let has = self.avm2_has_property(&obj, &key);
                    self.push_avm_stack(&mut stack, Avm1Value::Bool(has));
                }
                0xC5 => {
                    // add_i
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_add(b))),
                    );
                }
                0xC6 => {
                    // subtract_i
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_sub(b))),
                    );
                }
                0xC7 => {
                    // multiply_i
                    let b = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    let a = ecma_f64_to_i32(avm2_to_number(
                        &stack.pop().unwrap_or(Avm1Value::Undefined),
                    ));
                    self.push_avm_stack(
                        &mut stack,
                        Avm1Value::Number(f64::from(a.wrapping_mul(b))),
                    );
                }
                0xD0..=0xD3 => {
                    let idx = usize::from(op - 0xD0);
                    let val = locals.get(idx).cloned().unwrap_or(Avm1Value::Undefined);
                    self.push_avm_stack(&mut stack, val);
                }
                0xD4..=0xD7 => {
                    let idx = usize::from(op - 0xD4);
                    let val = stack.pop().unwrap_or(Avm1Value::Undefined);
                    if idx < locals.len() {
                        locals[idx] = val;
                    }
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
                _ => {
                    self.record_unsupported_diagnostic(format!(
                        "Unsupported AVM2 ABC opcode 0x{op:02X} in abc#{abc_idx} method#{method_idx} offset=0x{insn_start:x}"
                    ));
                    break;
                }
            }
        }
        if *steps >= MAX_VM_STEPS_PER_FRAME {
            self.record_unsupported_diagnostic(format!(
                "AVM2 execution reached bounded per-frame step limit ({MAX_VM_STEPS_PER_FRAME})"
            ));
        }
        Avm1Value::Undefined
    }

    fn pop_avm2_multiname(
        &self,
        abc_idx: usize,
        mn_idx: usize,
        stack: &mut Vec<Avm1Value>,
    ) -> String {
        let (has_rt_ns, has_rt_name) = self.abc_tags[abc_idx].abc.runtime_multiname_parts(mn_idx);
        let prop_name = if has_rt_name {
            avm2_convert_string(
                &stack.pop().unwrap_or(Avm1Value::Undefined),
                &self.avm1_objects,
            )
        } else {
            self.abc_tags[abc_idx].abc.split_qname(mn_idx).1
        };
        if has_rt_ns {
            let _ = stack.pop();
        }
        prop_name
    }

    fn pop_avm2_qname(
        &self,
        abc_idx: usize,
        mn_idx: usize,
        stack: &mut Vec<Avm1Value>,
    ) -> (String, String) {
        let (has_rt_ns, has_rt_name) = self.abc_tags[abc_idx].abc.runtime_multiname_parts(mn_idx);
        let (pkg, short) = if has_rt_name {
            let n = avm2_convert_string(
                &stack.pop().unwrap_or(Avm1Value::Undefined),
                &self.avm1_objects,
            );
            (String::new(), n)
        } else {
            self.abc_tags[abc_idx].abc.split_qname(mn_idx)
        };
        if has_rt_ns {
            let _ = stack.pop();
        }
        (pkg, short)
    }

    fn find_avm2_class(
        &self,
        hint_abc: usize,
        pkg: &str,
        cls_name: &str,
    ) -> Option<(usize, usize)> {
        if cls_name.is_empty() || cls_name == "*" {
            return None;
        }
        if let Some(loaded) = self.abc_tags.get(hint_abc) {
            for (c_idx, inst) in loaded.abc.instances.iter().enumerate() {
                let (ipkg, iname) = loaded.abc.split_qname(inst.name);
                if iname == cls_name && (pkg.is_empty() || ipkg == pkg) {
                    return Some((hint_abc, c_idx));
                }
            }
        }
        for (a_idx, loaded) in self.abc_tags.iter().enumerate() {
            for (c_idx, inst) in loaded.abc.instances.iter().enumerate() {
                let (ipkg, iname) = loaded.abc.split_qname(inst.name);
                if iname == cls_name && (pkg.is_empty() || ipkg == pkg) {
                    return Some((a_idx, c_idx));
                }
            }
        }
        None
    }

    fn avm2_superclass(&self, abc_idx: usize, cls_idx: usize) -> Option<(usize, usize)> {
        let inst = self.abc_tags.get(abc_idx)?.abc.instances.get(cls_idx)?;
        if inst.super_name == 0 {
            return None;
        }
        let (spkg, sname) = self.abc_tags[abc_idx].abc.split_qname(inst.super_name);
        if sname == "Object" || sname == "*" || sname.is_empty() {
            return None;
        }
        self.find_avm2_class(abc_idx, &spkg, &sname)
    }

    fn avm2_class_matches_or_extends(
        &self,
        mut cur_abc: usize,
        mut cur_cls: usize,
        target_abc: usize,
        target_cls: usize,
    ) -> bool {
        for _ in 0..32 {
            if cur_abc == target_abc && cur_cls == target_cls {
                return true;
            }
            let inst = match self
                .abc_tags
                .get(cur_abc)
                .and_then(|l| l.abc.instances.get(cur_cls))
            {
                Some(i) => i,
                None => break,
            };
            for &iface_mn in &inst.interfaces {
                let (ipkg, iname) = self.abc_tags[cur_abc].abc.split_qname(iface_mn);
                if let Some((ia, ic)) = self.find_avm2_class(cur_abc, &ipkg, &iname)
                    && ia == target_abc
                    && ic == target_cls
                {
                    return true;
                }
            }
            if let Some((sa, sc)) = self.avm2_superclass(cur_abc, cur_cls) {
                cur_abc = sa;
                cur_cls = sc;
            } else {
                break;
            }
        }
        false
    }

    fn avm2_is_type_name(&self, abc_idx: usize, val: &Avm1Value, pkg: &str, tname: &str) -> bool {
        if tname == "*" {
            return true;
        }
        if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
            return false;
        }
        match tname {
            "Object" => true,
            "Boolean" => matches!(val, Avm1Value::Bool(_)),
            "Number" => matches!(val, Avm1Value::Number(_)),
            "int" => matches!(
                val,
                Avm1Value::Number(n)
                    if n.is_finite()
                        && n.fract() == 0.0
                        && *n >= f64::from(i32::MIN)
                        && *n <= f64::from(i32::MAX)
            ),
            "uint" => matches!(
                val,
                Avm1Value::Number(n)
                    if n.is_finite()
                        && n.fract() == 0.0
                        && *n >= 0.0
                        && *n <= f64::from(u32::MAX)
            ),
            "String" => matches!(val, Avm1Value::String(s) if !is_avm2_fn_token(s)),
            "Function" => match val {
                Avm1Value::String(s) => is_avm2_fn_token(s),
                Avm1Value::Object(id) => self
                    .avm1_objects
                    .get(*id)
                    .is_some_and(|o| o.function.is_some()),
                _ => false,
            },
            "Array" => matches!(
                val,
                Avm1Value::Object(id)
                    if self
                        .avm1_objects
                        .get(*id)
                        .is_some_and(|o| o.array_elements.is_some())
            ),
            "MovieClip"
            | "Sprite"
            | "DisplayObject"
            | "DisplayObjectContainer"
            | "InteractiveObject"
            | "EventDispatcher" => match val {
                Avm1Value::MovieClipIdentity(_) => true,
                Avm1Value::MovieClip(mc) => {
                    mc == "_root"
                        || (!mc.starts_with("__avm2_")
                            && mc != "Math"
                            && self.target_slot(&self.canonical_movie_target(mc)).is_some())
                }
                Avm1Value::Object(id) => {
                    if let Some((c_abc, c_idx)) =
                        self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                        && let Some((t_abc, t_idx)) = self.find_avm2_class(abc_idx, pkg, tname)
                    {
                        self.avm2_class_matches_or_extends(c_abc, c_idx, t_abc, t_idx)
                    } else {
                        false
                    }
                }
                _ => false,
            },
            _ => {
                if let Avm1Value::Object(id) = val
                    && let Some((c_abc, c_idx)) =
                        self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                    && let Some((t_abc, t_idx)) = self.find_avm2_class(abc_idx, pkg, tname)
                {
                    return self.avm2_class_matches_or_extends(c_abc, c_idx, t_abc, t_idx);
                }
                false
            }
        }
    }

    fn avm2_is_type_value(&self, abc_idx: usize, val: &Avm1Value, type_val: &Avm1Value) -> bool {
        match type_val {
            Avm1Value::MovieClip(s) | Avm1Value::String(s) => {
                if let Some((t_abc, t_cls)) = parse_avm2_class_token(s) {
                    if let Avm1Value::Object(id) = val
                        && let Some((c_abc, c_cls)) =
                            self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                    {
                        return self.avm2_class_matches_or_extends(c_abc, c_cls, t_abc, t_cls);
                    }
                    return false;
                }
                let tname = s.strip_prefix("__avm2_type:").unwrap_or(s.as_str());
                self.avm2_is_type_name(abc_idx, val, "", tname)
            }
            _ => false,
        }
    }

    fn avm2_instanceof_value(&self, abc_idx: usize, val: &Avm1Value, type_val: &Avm1Value) -> bool {
        if matches!(
            val,
            Avm1Value::Undefined | Avm1Value::Null | Avm1Value::Bool(_) | Avm1Value::Number(_)
        ) {
            return false;
        }
        if let Avm1Value::String(s) = val
            && !is_avm2_fn_token(s)
        {
            return false;
        }
        self.avm2_is_type_value(abc_idx, val, type_val)
    }

    fn avm2_coerce_multiname(
        &mut self,
        abc_idx: usize,
        mn_idx: usize,
        val: Avm1Value,
    ) -> Avm1Value {
        let (pkg, tname) = self.abc_tags[abc_idx].abc.split_qname(mn_idx);
        match tname.as_str() {
            "*" => val,
            "String" => match val {
                Avm1Value::Undefined | Avm1Value::Null => Avm1Value::Null,
                other => Avm1Value::String(avm2_convert_string(&other, &self.avm1_objects)),
            },
            "int" => Avm1Value::Number(f64::from(ecma_f64_to_i32(avm2_to_number(&val)))),
            "uint" => Avm1Value::Number(f64::from(ecma_f64_to_u32(avm2_to_number(&val)))),
            "Number" => Avm1Value::Number(avm2_to_number(&val)),
            "Boolean" => Avm1Value::Bool(val.to_bool(9)),
            "Object" => {
                if matches!(val, Avm1Value::Undefined) {
                    Avm1Value::Null
                } else {
                    val
                }
            }
            _ => {
                if matches!(val, Avm1Value::Undefined | Avm1Value::Null) {
                    return Avm1Value::Null;
                }
                if self.avm2_is_type_name(abc_idx, &val, &pkg, &tname) {
                    return val;
                }
                if self.find_avm2_class(abc_idx, &pkg, &tname).is_some()
                    || matches!(tname.as_str(), "Array" | "Function")
                {
                    self.record_unsupported_diagnostic(format!(
                        "AVM2 type coercion mismatch to {tname}"
                    ));
                    Avm1Value::Null
                } else {
                    val
                }
            }
        }
    }

    fn avm2_typeof(&self, val: &Avm1Value) -> &'static str {
        match val {
            Avm1Value::Undefined => "undefined",
            Avm1Value::Null => "object",
            Avm1Value::Bool(_) => "boolean",
            Avm1Value::Number(_) => "number",
            Avm1Value::String(s) => {
                if is_avm2_fn_token(s) {
                    "function"
                } else {
                    "string"
                }
            }
            Avm1Value::Object(id) => {
                if self
                    .avm1_objects
                    .get(*id)
                    .is_some_and(|o| o.function.is_some())
                {
                    "function"
                } else {
                    "object"
                }
            }
            Avm1Value::MovieClip(_) | Avm1Value::MovieClipIdentity(_) | Avm1Value::Super { .. } => {
                "object"
            }
        }
    }

    fn avm2_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
        match (a, b) {
            (Avm1Value::Undefined | Avm1Value::Null, Avm1Value::Undefined | Avm1Value::Null) => {
                true
            }
            (Avm1Value::Undefined | Avm1Value::Null, _)
            | (_, Avm1Value::Undefined | Avm1Value::Null) => false,
            (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
            (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
            (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
            (Avm1Value::Bool(x), other) => {
                self.avm2_equals(&Avm1Value::Number(if *x { 1.0 } else { 0.0 }), other)
            }
            (other, Avm1Value::Bool(y)) => {
                self.avm2_equals(other, &Avm1Value::Number(if *y { 1.0 } else { 0.0 }))
            }
            (Avm1Value::Number(x), Avm1Value::String(y))
            | (Avm1Value::String(y), Avm1Value::Number(x)) => {
                if is_avm2_fn_token(y) {
                    false
                } else {
                    *x == avm2_string_to_number(y)
                }
            }
            (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
            (
                Avm1Value::MovieClip(x) | Avm1Value::MovieClipIdentity(x),
                Avm1Value::MovieClip(y) | Avm1Value::MovieClipIdentity(y),
            ) => self.avm2_mc_eq(x, y),
            _ => false,
        }
    }

    fn avm2_strict_equals(&self, a: &Avm1Value, b: &Avm1Value) -> bool {
        match (a, b) {
            (Avm1Value::Undefined, Avm1Value::Undefined) => true,
            (Avm1Value::Null, Avm1Value::Null) => true,
            (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
            (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
            (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
            (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
            (
                Avm1Value::MovieClip(x) | Avm1Value::MovieClipIdentity(x),
                Avm1Value::MovieClip(y) | Avm1Value::MovieClipIdentity(y),
            ) => self.avm2_mc_eq(x, y),
            _ => false,
        }
    }

    fn avm2_mc_eq(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if a.starts_with("__avm2_") || b.starts_with("__avm2_") {
            return false;
        }
        let ca = self.canonical_movie_target(a);
        let cb = self.canonical_movie_target(b);
        !ca.is_empty() && ca == cb && (ca == "_root" || self.target_slot(&ca).is_some())
    }

    fn avm2_enumerable_keys(&self, obj: &Avm1Value) -> Vec<String> {
        match obj {
            Avm1Value::Object(id) => {
                let Some(o) = self.avm1_objects.get(*id) else {
                    return Vec::new();
                };
                let mut keys = Vec::new();
                if let Some(elems) = &o.array_elements {
                    for idx in 0..elems.len() {
                        keys.push(idx.to_string());
                    }
                }
                let mut prop_keys: Vec<String> = o
                    .props
                    .keys()
                    .filter(|k| !k.starts_with("__"))
                    .cloned()
                    .collect();
                prop_keys.sort();
                keys.extend(prop_keys);
                keys
            }
            _ => Vec::new(),
        }
    }

    fn avm2_object_has_prop(&self, obj_id: usize, prop: &str) -> bool {
        let Some(obj) = self.avm1_objects.get(obj_id) else {
            return false;
        };
        if obj.props.contains_key(prop) {
            return true;
        }
        if let Some(elems) = &obj.array_elements
            && (prop == "length" || prop.parse::<usize>().is_ok_and(|idx| idx < elems.len()))
        {
            return true;
        }
        if let Some((c_abc, c_idx)) = obj.avm2_class {
            let mut cur = Some((c_abc, c_idx));
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                if let Some(inst) = self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c)) {
                    for tr in &inst.traits {
                        if self.abc_tags[a].abc.split_qname(tr.name).1 == prop {
                            return true;
                        }
                    }
                }
                cur = self.avm2_superclass(a, c);
            }
        }
        false
    }

    fn avm2_has_property(&self, obj: &Avm1Value, prop: &str) -> bool {
        match obj {
            Avm1Value::Object(id) => self.avm2_object_has_prop(*id, prop),
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                let target = self.canonical_movie_target(mc);
                if target == "_root" {
                    self.avm2_globals.contains_key(prop)
                        || self
                            .display_list
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, prop))
                } else if let Some(slot) = self.target_slot(&target) {
                    matches!(
                        prop,
                        "x" | "y"
                            | "scaleX"
                            | "scaleY"
                            | "alpha"
                            | "visible"
                            | "rotation"
                            | "name"
                            | "mask"
                            | "currentFrame"
                            | "totalFrames"
                    ) || slot.props.contains_key(prop)
                        || slot
                            .children
                            .iter()
                            .any(|(d, s)| slot_matches_segment(*d, s, prop))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn delete_avm2_property(&mut self, obj: &Avm1Value, prop: &str) -> bool {
        match obj {
            Avm1Value::Object(id) => {
                if let Some((c_abc, c_idx)) = self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
                {
                    // Sealed/fixed traits cannot be deleted in AVM2
                    let mut cur = Some((c_abc, c_idx));
                    for _ in 0..32 {
                        let Some((a, c)) = cur else { break };
                        if let Some(inst) =
                            self.abc_tags.get(a).and_then(|l| l.abc.instances.get(c))
                        {
                            for tr in &inst.traits {
                                if self.abc_tags[a].abc.split_qname(tr.name).1 == prop {
                                    return false;
                                }
                            }
                        }
                        cur = self.avm2_superclass(a, c);
                    }
                }
                if let Some(o) = self.avm1_objects.get_mut(*id) {
                    if let Some(elems) = &mut o.array_elements
                        && let Ok(idx) = prop.parse::<usize>()
                    {
                        if idx < elems.len() {
                            elems[idx] = Avm1Value::Undefined;
                        }
                        return true;
                    }
                    let _ = o.props.remove(prop);
                    return true;
                }
                false
            }
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                let target = self.canonical_movie_target(mc);
                if target != "_root"
                    && let Some(slot) = self.target_slot_mut(&target)
                {
                    let _ = slot.props.remove(prop);
                    true
                } else {
                    let _ = self.avm2_globals.remove(prop);
                    true
                }
            }
            _ => false,
        }
    }

    fn construct_avm2_value(
        &mut self,
        abc_idx: usize,
        ctor: &Avm1Value,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::MovieClip(s) | Avm1Value::String(s) = ctor {
            if let Some((c_abc, c_cls)) = parse_avm2_class_token(s) {
                return self.instantiate_avm2_user_class(c_abc, c_cls, cargs, steps, depth);
            }
            let tname = s.strip_prefix("__avm2_type:").unwrap_or(s.as_str());
            return self.construct_avm2_named(abc_idx, "", tname, cargs, steps, depth);
        }
        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object::default());
        Avm1Value::Object(obj_id)
    }

    fn construct_avm2_named(
        &mut self,
        abc_idx: usize,
        pkg: &str,
        cls_name: &str,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        match cls_name {
            "Array" => {
                let elems = if cargs.len() == 1
                    && let Avm1Value::Number(n) = cargs[0]
                    && n.is_finite()
                    && n.fract() == 0.0
                    && (0.0..=4096.0).contains(&n)
                {
                    vec![Avm1Value::Undefined; n as usize]
                } else {
                    cargs
                };
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object {
                    array_elements: Some(elems),
                    ..Default::default()
                });
                return Avm1Value::Object(obj_id);
            }
            "String" => {
                let s = cargs
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                return Avm1Value::String(s);
            }
            "Number" => {
                let n = cargs.first().map_or(0.0, avm2_to_number);
                return Avm1Value::Number(n);
            }
            "int" => {
                let i = cargs
                    .first()
                    .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                return Avm1Value::Number(f64::from(i));
            }
            "uint" => {
                let u = cargs
                    .first()
                    .map_or(0, |v| ecma_f64_to_u32(avm2_to_number(v)));
                return Avm1Value::Number(f64::from(u));
            }
            "Boolean" => {
                let b = cargs.first().is_some_and(|v| v.to_bool(9));
                return Avm1Value::Bool(b);
            }
            "Object" => {
                let obj_id = self.avm1_objects.len();
                self.avm1_objects.push(Avm1Object::default());
                return Avm1Value::Object(obj_id);
            }
            _ => {}
        }
        if let Some((c_abc, c_idx)) = self.find_avm2_class(abc_idx, pkg, cls_name) {
            return self.instantiate_avm2_user_class(c_abc, c_idx, cargs, steps, depth);
        }
        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object::default());
        Avm1Value::Object(obj_id)
    }

    fn instantiate_avm2_user_class(
        &mut self,
        c_abc: usize,
        c_idx: usize,
        cargs: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let mut chain = Vec::new();
        let mut cur = Some((c_abc, c_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            chain.push((a, c));
            cur = self.avm2_superclass(a, c);
        }
        chain.reverse();

        let mut props = HashMap::new();
        for &(a, c) in &chain {
            if let Some(loaded) = self.abc_tags.get(a)
                && let Some(inst) = loaded.abc.instances.get(c)
            {
                for tr in &inst.traits {
                    if let AbcTraitData::Slot {
                        type_name,
                        vindex,
                        vkind,
                        ..
                    } = tr.data
                    {
                        let (_, t_prop) = loaded.abc.split_qname(tr.name);
                        let init_val = if vindex != 0 || vkind != 0 {
                            abc_const_to_avm_value(&loaded.abc, vindex, vkind)
                        } else {
                            let (_, ty) = loaded.abc.split_qname(type_name);
                            match ty.as_str() {
                                "int" | "uint" => Avm1Value::Number(0.0),
                                "Number" => Avm1Value::Number(f64::NAN),
                                "Boolean" => Avm1Value::Bool(false),
                                "*" => Avm1Value::Undefined,
                                _ => Avm1Value::Null,
                            }
                        };
                        props.insert(t_prop, init_val);
                    }
                }
            }
        }

        let obj_id = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props,
            avm2_class: Some((c_abc, c_idx)),
            ..Default::default()
        });
        if let Some(iinit) = self
            .abc_tags
            .get(c_abc)
            .and_then(|l| l.abc.instances.get(c_idx))
            .map(|inst| inst.iinit)
        {
            let _ = self.run_avm2_method_with_this(
                c_abc,
                iinit,
                Avm1Value::Object(obj_id),
                cargs,
                steps,
                depth + 1,
            );
        }
        Avm1Value::Object(obj_id)
    }

    fn resolve_avm2_slot_name(&self, c_abc: usize, c_idx: usize, slot_key: &str) -> Option<String> {
        let slot_num: usize = slot_key.strip_prefix("__slot_")?.parse().ok()?;
        let mut cur = Some((c_abc, c_idx));
        for _ in 0..32 {
            let Some((a, c)) = cur else { break };
            if let Some(loaded) = self.abc_tags.get(a)
                && let Some(inst) = loaded.abc.instances.get(c)
            {
                for tr in &inst.traits {
                    if let AbcTraitData::Slot { slot_id, .. } = tr.data
                        && slot_id == slot_num
                    {
                        return Some(loaded.abc.split_qname(tr.name).1);
                    }
                }
            }
            cur = self.avm2_superclass(a, c);
        }
        None
    }

    fn get_avm2_property_eval(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(id) = obj {
            if let Some((c_abc, c_idx)) = self.avm1_objects.get(*id).and_then(|o| o.avm2_class) {
                let effective_prop = self
                    .resolve_avm2_slot_name(c_abc, c_idx, prop)
                    .unwrap_or_else(|| prop.to_owned());
                let mut cur = Some((c_abc, c_idx));
                for _ in 0..32 {
                    let Some((a, c)) = cur else { break };
                    let found_trait = self
                        .abc_tags
                        .get(a)
                        .and_then(|l| l.abc.instances.get(c))
                        .and_then(|inst| {
                            inst.traits
                                .iter()
                                .find(|tr| {
                                    !matches!(tr.data, AbcTraitData::Setter { .. })
                                        && self.abc_tags[a].abc.split_qname(tr.name).1
                                            == effective_prop
                                })
                                .map(|tr| tr.data.clone())
                        });
                    if let Some(td) = found_trait {
                        match td {
                            AbcTraitData::Getter { method } => {
                                return self.run_avm2_method_with_this(
                                    a,
                                    method,
                                    Avm1Value::Object(*id),
                                    Vec::new(),
                                    steps,
                                    depth + 1,
                                );
                            }
                            AbcTraitData::Method { method } | AbcTraitData::Function { method } => {
                                return Avm1Value::String(format!(
                                    "__avm2_bound:{id}:{a}:{method}"
                                ));
                            }
                            _ => break,
                        }
                    }
                    cur = self.avm2_superclass(a, c);
                }
                return self.get_avm1_member(obj, &effective_prop);
            }
            return self.get_avm1_member(obj, prop);
        }
        if let Avm1Value::String(s) = obj
            && !is_avm2_fn_token(s)
            && prop == "length"
        {
            return Avm1Value::Number(s.chars().count() as f64);
        }
        self.get_avm2_property(obj, prop)
    }

    fn get_avm2_property(&self, obj: &Avm1Value, prop: &str) -> Avm1Value {
        if let Avm1Value::Object(_id) = obj {
            return self.get_avm1_member(obj, prop);
        }
        if let Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) = obj {
            if let Some((c_abc, c_idx)) = parse_avm2_class_token(mc) {
                if let Some(loaded) = self.abc_tags.get(c_abc)
                    && let Some(cls) = loaded.abc.classes.get(c_idx)
                {
                    for tr in &cls.traits {
                        let (_, tname) = loaded.abc.split_qname(tr.name);
                        if tname == prop {
                            match tr.data {
                                AbcTraitData::Slot { vindex, vkind, .. }
                                    if vindex != 0 || vkind != 0 =>
                                {
                                    return abc_const_to_avm_value(&loaded.abc, vindex, vkind);
                                }
                                AbcTraitData::Method { method }
                                | AbcTraitData::Function { method } => {
                                    return Avm1Value::String(format!(
                                        "__avm2_fn:{c_abc}:{method}"
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if let Some(v) = self.avm2_globals.get(&format!("{mc}.{prop}")) {
                    return v.clone();
                }
                return Avm1Value::Undefined;
            }
            if mc == "Math" {
                return match prop {
                    "PI" => Avm1Value::Number(std::f64::consts::PI),
                    "E" => Avm1Value::Number(std::f64::consts::E),
                    "SQRT2" => Avm1Value::Number(std::f64::consts::SQRT_2),
                    "LN2" => Avm1Value::Number(std::f64::consts::LN_2),
                    "LN10" => Avm1Value::Number(std::f64::consts::LN_10),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:Number" {
                return match prop {
                    "NaN" => Avm1Value::Number(f64::NAN),
                    "POSITIVE_INFINITY" => Avm1Value::Number(f64::INFINITY),
                    "NEGATIVE_INFINITY" => Avm1Value::Number(f64::NEG_INFINITY),
                    "MAX_VALUE" => Avm1Value::Number(f64::MAX),
                    "MIN_VALUE" => Avm1Value::Number(f64::MIN_POSITIVE),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:int" {
                return match prop {
                    "MAX_VALUE" => Avm1Value::Number(f64::from(i32::MAX)),
                    "MIN_VALUE" => Avm1Value::Number(f64::from(i32::MIN)),
                    _ => Avm1Value::Undefined,
                };
            }
            if mc == "__avm2_type:uint" {
                return match prop {
                    "MAX_VALUE" => Avm1Value::Number(f64::from(u32::MAX)),
                    "MIN_VALUE" => Avm1Value::Number(0.0),
                    _ => Avm1Value::Undefined,
                };
            }
            let target = self.canonical_movie_target(mc);
            if target != "_root" && !target.is_empty() && target != "Math" && mc != prop {
                if self.target_slot(&target).is_some() {
                    match prop {
                        "x" => return self.get_mc_property_identity(&target, 0),
                        "y" => return self.get_mc_property_identity(&target, 1),
                        "scaleX" => {
                            return Avm1Value::Number(
                                self.get_mc_property_identity(&target, 2).to_number() / 100.0,
                            );
                        }
                        "scaleY" => {
                            return Avm1Value::Number(
                                self.get_mc_property_identity(&target, 3).to_number() / 100.0,
                            );
                        }
                        "currentFrame" => return self.get_mc_property_identity(&target, 4),
                        "totalFrames" => return self.get_mc_property_identity(&target, 5),
                        "alpha" => {
                            return Avm1Value::Number(
                                self.get_mc_property_identity(&target, 6).to_number() / 100.0,
                            );
                        }
                        "visible" => return self.get_mc_property_identity(&target, 7),
                        "rotation" => return self.get_mc_property_identity(&target, 10),
                        "name" => return self.get_mc_property_identity(&target, 13),
                        "mask" => {
                            return self
                                .target_slot(&target)
                                .and_then(|s| s.masked_by.clone())
                                .map_or(Avm1Value::Null, Avm1Value::MovieClipIdentity);
                        }
                        _ => {
                            if let Some(slot) = self.target_slot(&target) {
                                if let Some(v) = slot.props.get(prop) {
                                    return v.clone();
                                }
                                if let Some((depth, child)) =
                                    slot.children.iter().find_map(|(depth, child)| {
                                        slot_matches_segment(*depth, child, prop)
                                            .then_some((*depth, child))
                                    })
                                {
                                    return Avm1Value::MovieClipIdentity(join_target_path(
                                        &target,
                                        &live_slot_segment(depth, child),
                                    ));
                                }
                            }
                            return Avm1Value::MovieClip(format!("{target}.{prop}"));
                        }
                    }
                }
                let dotted = format!("{mc}.{prop}");
                if let Some(v) = self.avm2_globals.get(&dotted) {
                    return v.clone();
                }
                return Avm1Value::MovieClip(dotted);
            }
        }
        if let Some(v) = self.avm2_globals.get(prop) {
            return v.clone();
        }
        // Check if `prop` is a method or constant on the document/timeline class
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
                if let Some((depth, slot)) = self.display_list.iter().find_map(|(depth, slot)| {
                    slot_matches_segment(*depth, slot, prop).then_some((*depth, slot))
                }) {
                    Avm1Value::MovieClipIdentity(live_slot_segment(depth, slot))
                } else {
                    Avm1Value::MovieClip(prop.to_owned())
                }
            }
        }
    }

    fn set_avm2_property_eval(
        &mut self,
        obj: &Avm1Value,
        prop: &str,
        val: Avm1Value,
        steps: &mut usize,
        depth: usize,
    ) {
        if let Avm1Value::Object(id) = obj
            && let Some((c_abc, c_idx)) = self.avm1_objects.get(*id).and_then(|o| o.avm2_class)
        {
            let effective_prop = self
                .resolve_avm2_slot_name(c_abc, c_idx, prop)
                .unwrap_or_else(|| prop.to_owned());
            let mut cur = Some((c_abc, c_idx));
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                let setter_or_slot = self
                    .abc_tags
                    .get(a)
                    .and_then(|l| l.abc.instances.get(c))
                    .and_then(|inst| {
                        inst.traits
                            .iter()
                            .find(|tr| {
                                !matches!(tr.data, AbcTraitData::Getter { .. })
                                    && self.abc_tags[a].abc.split_qname(tr.name).1 == effective_prop
                            })
                            .map(|tr| tr.data.clone())
                    });
                if let Some(td) = setter_or_slot {
                    match td {
                        AbcTraitData::Setter { method } => {
                            let _ = self.run_avm2_method_with_this(
                                a,
                                method,
                                Avm1Value::Object(*id),
                                vec![val],
                                steps,
                                depth + 1,
                            );
                            return;
                        }
                        AbcTraitData::Slot { type_name, .. } => {
                            let coerced = self.avm2_coerce_multiname(a, type_name, val);
                            self.set_avm1_member(obj, &effective_prop, coerced);
                            return;
                        }
                        _ => break,
                    }
                }
                cur = self.avm2_superclass(a, c);
            }
            self.set_avm1_member(obj, &effective_prop, val);
            return;
        }
        self.set_avm2_property(obj, prop, val);
    }

    fn set_avm2_property(&mut self, obj: &Avm1Value, prop: &str, val: Avm1Value) {
        match obj {
            Avm1Value::Object(_) => self.set_avm1_member(obj, prop, val),
            Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) => {
                if mc.starts_with("__avm2_class:") {
                    self.avm2_globals.insert(format!("{mc}.{prop}"), val);
                    return;
                }
                let target = self.canonical_movie_target(mc);
                if target != "_root" && !target.is_empty() && target != "Math" && mc != prop {
                    if self.target_slot(&target).is_some() {
                        match prop {
                            "x" => self.set_mc_property_identity(
                                &target,
                                0,
                                Avm1Value::Number(avm2_to_number(&val)),
                            ),
                            "y" => self.set_mc_property_identity(
                                &target,
                                1,
                                Avm1Value::Number(avm2_to_number(&val)),
                            ),
                            "scaleX" => self.set_mc_property_identity(
                                &target,
                                2,
                                Avm1Value::Number(avm2_to_number(&val) * 100.0),
                            ),
                            "scaleY" => self.set_mc_property_identity(
                                &target,
                                3,
                                Avm1Value::Number(avm2_to_number(&val) * 100.0),
                            ),
                            "alpha" => self.set_mc_property_identity(
                                &target,
                                6,
                                Avm1Value::Number(avm2_to_number(&val) * 100.0),
                            ),
                            "visible" => self.set_mc_property_identity(&target, 7, val),
                            "rotation" => self.set_mc_property_identity(
                                &target,
                                10,
                                Avm1Value::Number(avm2_to_number(&val)),
                            ),
                            "name" => self.set_mc_property_identity(&target, 13, val),
                            "mask" => self.apply_dynamic_mask(&target, Some(&val)),
                            _ => {
                                if let Some(slot) = self.target_slot_mut(&target) {
                                    slot.props.insert(prop.to_owned(), val);
                                }
                            }
                        }
                    } else {
                        self.avm2_globals.insert(format!("{mc}.{prop}"), val);
                    }
                } else {
                    self.avm2_globals.insert(prop.to_owned(), val);
                }
            }
            _ => {
                self.avm2_globals.insert(prop.to_owned(), val);
            }
        }
    }

    fn call_avm2_super_method(
        &mut self,
        receiver: &Avm1Value,
        prop: &str,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::Object(oid) = receiver
            && let Some((c_abc, c_idx)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class)
        {
            let mut cur = self.avm2_superclass(c_abc, c_idx);
            for _ in 0..32 {
                let Some((a, c)) = cur else { break };
                let method_opt = self
                    .abc_tags
                    .get(a)
                    .and_then(|l| l.abc.instances.get(c))
                    .and_then(|inst| {
                        inst.traits.iter().find_map(|tr| {
                            (self.abc_tags[a].abc.split_qname(tr.name).1 == prop)
                                .then_some(&tr.data)
                                .and_then(|td| match td {
                                    AbcTraitData::Method { method }
                                    | AbcTraitData::Function { method } => Some(*method),
                                    _ => None,
                                })
                        })
                    });
                if let Some(m_idx) = method_opt {
                    return self.run_avm2_method_with_this(
                        a,
                        m_idx,
                        Avm1Value::Object(*oid),
                        args,
                        steps,
                        depth + 1,
                    );
                }
                cur = self.avm2_superclass(a, c);
            }
        }
        Avm1Value::Undefined
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
        if let Avm1Value::Object(oid) = receiver {
            if let Some((c_abc, c_idx)) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_class) {
                let mut cur = Some((c_abc, c_idx));
                for _ in 0..32 {
                    let Some((a, c)) = cur else { break };
                    let method_opt = self
                        .abc_tags
                        .get(a)
                        .and_then(|l| l.abc.instances.get(c))
                        .and_then(|inst| {
                            inst.traits.iter().find_map(|tr| {
                                (self.abc_tags[a].abc.split_qname(tr.name).1 == prop)
                                    .then_some(&tr.data)
                                    .and_then(|td| match td {
                                        AbcTraitData::Method { method }
                                        | AbcTraitData::Function { method } => Some(*method),
                                        _ => None,
                                    })
                            })
                        });
                    if let Some(m_idx) = method_opt {
                        return self.run_avm2_method_with_this(
                            a,
                            m_idx,
                            Avm1Value::Object(*oid),
                            args,
                            steps,
                            depth + 1,
                        );
                    }
                    cur = self.avm2_superclass(a, c);
                }
            }
            if let Some(Avm1Value::String(tok)) = self
                .avm1_objects
                .get(*oid)
                .and_then(|o| o.props.get(prop))
                .cloned()
                && is_avm2_fn_token(&tok)
            {
                return self.invoke_avm2_callable_with_this(
                    &Avm1Value::String(tok),
                    Avm1Value::Object(*oid),
                    args,
                    steps,
                    depth,
                );
            }
        }

        if let Avm1Value::MovieClip(mc) = receiver
            && let Some((c_abc, c_idx)) = parse_avm2_class_token(mc)
            && let Some(m_idx) = self
                .abc_tags
                .get(c_abc)
                .and_then(|l| l.abc.classes.get(c_idx))
                .and_then(|cls| {
                    cls.traits.iter().find_map(|tr| {
                        (self.abc_tags[c_abc].abc.split_qname(tr.name).1 == prop)
                            .then_some(&tr.data)
                            .and_then(|td| match td {
                                AbcTraitData::Method { method }
                                | AbcTraitData::Function { method } => Some(*method),
                                _ => None,
                            })
                    })
                })
        {
            return self.run_avm2_method_bounded(c_abc, m_idx, args, steps, depth + 1);
        }

        if let Avm1Value::MovieClip(mc) | Avm1Value::MovieClipIdentity(mc) = receiver {
            let target = self.canonical_movie_target(mc);
            if target != "_root"
                && !target.is_empty()
                && target != "Math"
                && !mc.starts_with("__avm2_")
                && mc != prop
                && let Some(cid) = self.target_slot(&target).map(|s| s.character_id)
            {
                match prop {
                    "stop" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.playing = false;
                        }
                        return Avm1Value::Undefined;
                    }
                    "play" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.playing = true;
                        }
                        return Avm1Value::Undefined;
                    }
                    "nextFrame" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.current_frame =
                                (slot.current_frame + 1).min(slot.total_frames.saturating_sub(1));
                            slot.playing = false;
                        }
                        return Avm1Value::Undefined;
                    }
                    "prevFrame" => {
                        if let Some(slot) = self.target_slot_mut(&target) {
                            slot.current_frame = slot.current_frame.saturating_sub(1);
                            slot.playing = false;
                        }
                        return Avm1Value::Undefined;
                    }
                    "gotoAndStop" | "gotoAndPlay" => {
                        if let Some(first) = args.first() {
                            let resolved_label = match first {
                                Avm1Value::String(lbl) => self
                                    .sprites
                                    .get(&cid)
                                    .and_then(|def| def.resolve_label(lbl)),
                                _ => None,
                            };
                            if let Some(slot) = self.target_slot_mut(&target) {
                                let total = slot.total_frames.max(1);
                                let resolved = match first {
                                    Avm1Value::Number(n) if *n >= 1.0 => {
                                        let f = (*n as i64).saturating_sub(1).max(0) as u32;
                                        Some(f.min(total.saturating_sub(1)))
                                    }
                                    _ => resolved_label.map(|f| f.min(total.saturating_sub(1))),
                                };
                                if let Some(f) = resolved {
                                    slot.current_frame = f;
                                    slot.playing = prop == "gotoAndPlay";
                                }
                            }
                        }
                        return Avm1Value::Undefined;
                    }
                    _ => {}
                }
            }
        }

        let is_global_receiver = matches!(
            receiver,
            Avm1Value::MovieClip(mc) if mc == "_root" || mc == prop || mc.starts_with("__avm2_type:")
        );
        if is_global_receiver {
            match prop {
                "int" => {
                    let i = args
                        .first()
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                    return Avm1Value::Number(f64::from(i));
                }
                "uint" => {
                    let u = args
                        .first()
                        .map_or(0, |v| ecma_f64_to_u32(avm2_to_number(v)));
                    return Avm1Value::Number(f64::from(u));
                }
                "Number" => {
                    let n = args.first().map_or(0.0, avm2_to_number);
                    return Avm1Value::Number(n);
                }
                "String" => {
                    let s = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    return Avm1Value::String(s);
                }
                "Boolean" => {
                    let b = args.first().is_some_and(|v| v.to_bool(9));
                    return Avm1Value::Bool(b);
                }
                "isNaN" => {
                    let n = args.first().map_or(f64::NAN, avm2_to_number);
                    return Avm1Value::Bool(n.is_nan());
                }
                "isFinite" => {
                    let n = args.first().map_or(f64::NAN, avm2_to_number);
                    return Avm1Value::Bool(n.is_finite());
                }
                "parseInt" => {
                    let s = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    let radix = args
                        .get(1)
                        .map_or(0, |v| ecma_f64_to_i32(avm2_to_number(v)));
                    return Avm1Value::Number(parse_avm_int_radix(&s, radix));
                }
                "parseFloat" => {
                    let s = args
                        .first()
                        .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                    return Avm1Value::Number(avm2_string_to_number(&s));
                }
                _ => {}
            }
        }

        match prop {
            "trace" => {
                let line = args
                    .iter()
                    .map(|a| avm2_convert_string(a, &self.avm1_objects))
                    .collect::<Vec<_>>()
                    .join(" ");
                self.record_trace(line);
                return Avm1Value::Undefined;
            }
            "addFrameScript" => {
                for pair in args.as_chunks::<2>().0 {
                    let f_idx = ecma_f64_to_i32(avm2_to_number(&pair[0])).max(0) as u32;
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
                    let scene_offset = args
                        .get(1)
                        .and_then(|s| match s {
                            Avm1Value::String(name) => self
                                .scenes
                                .iter()
                                .find(|(_, sn)| sn == name)
                                .map(|(off, _)| *off),
                            _ => None,
                        })
                        .unwrap_or(0);
                    let total = self.root_timeline.frames.len().max(1) as u32;
                    let resolved = match first {
                        Avm1Value::Number(n) if *n >= 1.0 => {
                            let f = scene_offset + ((*n as i64).saturating_sub(1).max(0) as u32);
                            Some(f.min(total.saturating_sub(1)))
                        }
                        Avm1Value::String(lbl) => {
                            self.root_timeline.resolve_label(lbl).or_else(|| {
                                self.scenes
                                    .iter()
                                    .find(|(_, sn)| sn == lbl)
                                    .map(|(off, _)| *off)
                            })
                        }
                        _ => None,
                    };
                    if let Some(f) = resolved {
                        self.current_frame = f;
                        self.playing = prop == "gotoAndPlay";
                    }
                }
                return Avm1Value::Undefined;
            }
            "stopAll" => {
                self.stream_sound_active = false;
                self.stop_all_sounds_pending = true;
                return Avm1Value::Undefined;
            }
            _ => {}
        }

        if let Some(Avm1Value::String(tok)) = self.avm2_globals.get(prop).cloned()
            && is_avm2_fn_token(&tok)
        {
            return self.invoke_avm2_callable_with_this(
                &Avm1Value::String(tok),
                Avm1Value::MovieClip("_root".into()),
                args,
                steps,
                depth,
            );
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

    fn invoke_avm2_callable_with_this(
        &mut self,
        fn_val: &Avm1Value,
        receiver: Avm1Value,
        args: Vec<Avm1Value>,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        if let Avm1Value::String(s) = fn_val {
            if let Some((oid, a_idx, m_idx)) = parse_avm2_bound_token(s) {
                return self.run_avm2_method_with_this(
                    a_idx,
                    m_idx,
                    Avm1Value::Object(oid),
                    args,
                    steps,
                    depth + 1,
                );
            }
            if let Some((a_idx, m_idx)) = parse_avm2_fn_token(s) {
                let this_arg = if matches!(receiver, Avm1Value::Undefined | Avm1Value::Null) {
                    Avm1Value::MovieClip("_root".to_owned())
                } else {
                    receiver
                };
                return self.run_avm2_method_with_this(
                    a_idx,
                    m_idx,
                    this_arg,
                    args,
                    steps,
                    depth + 1,
                );
            }
        }
        Avm1Value::Undefined
    }
}

fn is_avm2_fn_token(s: &str) -> bool {
    s.starts_with("__avm2_fn:") || s.starts_with("__avm2_bound:")
}

fn parse_avm2_fn_token(s: &str) -> Option<(usize, usize)> {
    if let Some((_, a, m)) = parse_avm2_bound_token(s) {
        return Some((a, m));
    }
    let rest = s.strip_prefix("__avm2_fn:")?;
    let (a, m) = rest.split_once(':')?;
    Some((a.parse().ok()?, m.parse().ok()?))
}

fn parse_avm2_bound_token(s: &str) -> Option<(usize, usize, usize)> {
    let rest = s.strip_prefix("__avm2_bound:")?;
    let (oid, rem) = rest.split_once(':')?;
    let (a, m) = rem.split_once(':')?;
    Some((oid.parse().ok()?, a.parse().ok()?, m.parse().ok()?))
}

fn parse_avm2_class_token(s: &str) -> Option<(usize, usize)> {
    let rest = s.strip_prefix("__avm2_class:")?;
    let (a, c) = rest.split_once(':')?;
    Some((a.parse().ok()?, c.parse().ok()?))
}

fn abc_const_to_avm_value(abc: &AbcFile, vindex: usize, vkind: u8) -> Avm1Value {
    match vkind {
        0x03 => Avm1Value::Number(f64::from(abc.ints.get(vindex).copied().unwrap_or(0))),
        0x04 => Avm1Value::Number(f64::from(abc.uints.get(vindex).copied().unwrap_or(0))),
        0x06 => Avm1Value::Number(abc.doubles.get(vindex).copied().unwrap_or(f64::NAN)),
        0x01 => Avm1Value::String(abc.string(vindex).to_owned()),
        0x0B => Avm1Value::Bool(true),
        0x0A => Avm1Value::Bool(false),
        0x0C => Avm1Value::Null,
        0x00 => Avm1Value::Undefined,
        _ => Avm1Value::Null,
    }
}

fn avm2_string_to_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    if let Some(hex) = t.strip_prefix("-0x").or_else(|| t.strip_prefix("-0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| -(v as f64));
    }
    if let Some(hex) = t.strip_prefix("+0x").or_else(|| t.strip_prefix("+0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

fn avm2_to_number(val: &Avm1Value) -> f64 {
    match val {
        Avm1Value::Undefined => f64::NAN,
        Avm1Value::Null => 0.0,
        Avm1Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Avm1Value::Number(n) => *n,
        Avm1Value::String(s) => {
            if is_avm2_fn_token(s) {
                f64::NAN
            } else {
                avm2_string_to_number(s)
            }
        }
        Avm1Value::Object(_)
        | Avm1Value::MovieClip(_)
        | Avm1Value::MovieClipIdentity(_)
        | Avm1Value::Super { .. } => f64::NAN,
    }
}

fn avm2_convert_string(val: &Avm1Value, objects: &[Avm1Object]) -> String {
    match val {
        Avm1Value::Undefined => "undefined".to_owned(),
        Avm1Value::Null => "null".to_owned(),
        Avm1Value::String(s) if is_avm2_fn_token(s) => "function Function() {}".to_owned(),
        other => other.to_trace_string(9, objects),
    }
}

fn avm2_less_than(a: &Avm1Value, b: &Avm1Value) -> Option<bool> {
    if let (Avm1Value::String(sa), Avm1Value::String(sb)) = (a, b)
        && !is_avm2_fn_token(sa)
        && !is_avm2_fn_token(sb)
    {
        return Some(sa < sb);
    }
    let na = avm2_to_number(a);
    let nb = avm2_to_number(b);
    if na.is_nan() || nb.is_nan() {
        None
    } else {
        Some(na < nb)
    }
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
        let has_key = (event_flags & CLIP_EVENT_KEY_PRESS) != 0;
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

fn place_tag_depth(tag_code: u16, payload: &[u8]) -> Option<u16> {
    let offset = match tag_code {
        4 => 2,
        26 => 1,
        70 => 2,
        _ => return None,
    };
    payload
        .get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn remove_tag_depth(tag_code: u16, payload: &[u8]) -> Option<u16> {
    let offset = match tag_code {
        5 => 2,
        28 => 0,
        _ => return None,
    };
    payload
        .get(offset..offset + 2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn place_tag_replaces_slot(tag_code: u16, payload: &[u8]) -> bool {
    if tag_code == 4 {
        return true;
    }
    payload.first().is_some_and(|flags| (flags & 0x02) != 0)
}

fn push_ordered_clip_lifecycle_scripts(
    handlers: &[ClipEventHandler],
    load_scripts: &mut Vec<Vec<u8>>,
) {
    for flag in [CLIP_EVENT_INITIALIZE, CLIP_EVENT_CONSTRUCT, CLIP_EVENT_LOAD] {
        for handler in handlers {
            if (handler.event_flags & flag) != 0 {
                load_scripts.push(handler.actions.clone());
            }
        }
    }
}

fn apply_place_tag_to_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    sprites: &HashMap<u16, TimelineDef>,
    swf_version: u8,
    tag_code: u16,
    payload: &[u8],
) -> (Vec<Vec<u8>>, bool) {
    let mut r = SwfSlice::new(payload);
    let mut load_scripts = Vec::new();
    let mut unmodeled_filter = false;

    if tag_code == 4 {
        let Some(character_id) = r.u16() else {
            return (load_scripts, false);
        };
        let Some(depth) = r.u16() else {
            return (load_scripts, false);
        };
        let Some(matrix) = r.matrix_px() else {
            return (load_scripts, false);
        };
        let (color_transform, alpha) = if r.remaining() > 0 {
            r.cxform(false)
                .unwrap_or_else(|| (ColorTransform::default(), 1.0))
        } else {
            (ColorTransform::default(), 1.0)
        };
        let tf = Transform::from_matrix(matrix).unwrap_or_default();
        let sprite_total = sprites
            .get(&character_id)
            .map_or(1, |s| s.total_frames.max(1));
        display_list.insert(
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
                skew: tf.skew,
                alpha: alpha * 100.0,
                color_transform,
                visible: true,
                blend_mode: BlendMode::Normal,
                filters: Vec::new(),
                clip_depth: None,
                masked_by: None,
                is_dynamic: false,
                ratio: None,
                cache_as_bitmap: false,
                current_frame: 0,
                total_frames: sprite_total,
                playing: true,
                last_applied_frame: None,
                children: BTreeMap::new(),
                props: HashMap::new(),
                clip_handlers: Vec::new(),
            },
        );
        return (load_scripts, false);
    }

    let Some(flags1) = r.u8() else {
        return (load_scripts, false);
    };
    let flags2 = if tag_code == 70 {
        r.u8().unwrap_or(0)
    } else {
        0
    };
    let Some(depth) = r.u16() else {
        return (load_scripts, false);
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
        let _ = r.c_string(swf_version);
    }
    let new_char = if has_character { r.u16() } else { None };
    let new_matrix = if has_matrix { r.matrix_px() } else { None };
    let new_cxform = if has_cxform { r.cxform(true) } else { None };
    let new_ratio = if has_ratio { r.u16() } else { None };
    let new_name = if has_name {
        r.c_string(swf_version).map(|(s, _)| s)
    } else {
        None
    };
    let new_clip_depth = if has_clip_depth { r.u16() } else { None };
    let mut new_visible = None;
    let mut new_blend = None;
    let mut new_filters = None;
    let mut new_cache_as_bitmap = None;
    if tag_code == 70 {
        if (flags2 & 0x01) != 0 {
            let (parsed, unmodeled) = parse_swf_filter_list(&mut r);
            new_filters = Some(parsed);
            unmodeled_filter |= unmodeled;
        }
        if (flags2 & 0x02) != 0 {
            new_blend = r.u8();
        }
        if (flags2 & 0x04) != 0 {
            new_cache_as_bitmap = r.u8().map(|v| v != 0);
        }
        if (flags2 & 0x20) != 0 {
            new_visible = r.u8().map(|v| v != 0);
        }
        if (flags2 & 0x40) != 0 {
            let _ = r.rgba();
        }
    }
    let new_handlers = if has_clip_actions {
        parse_raw_clip_handlers(&mut r, swf_version)
    } else {
        Vec::new()
    };

    if is_move && let Some(slot) = display_list.get_mut(&depth) {
        if let Some(character_id) = new_char {
            push_ordered_clip_lifecycle_scripts(&new_handlers, &mut load_scripts);
            let matrix = new_matrix.unwrap_or(slot.matrix);
            let transform = Transform::from_matrix(matrix).unwrap_or_default();
            let (color_transform, alpha) = new_cxform
                .map(|(color, alpha)| (color, alpha * 100.0))
                .unwrap_or((slot.color_transform, slot.alpha));
            *slot = LiveSlot {
                character_id,
                name: new_name.unwrap_or_default(),
                matrix,
                x: transform.x,
                y: transform.y,
                scale_x: transform.scale_x * 100.0,
                scale_y: transform.scale_y * 100.0,
                rotation: transform.rotation,
                skew: transform.skew,
                alpha,
                color_transform,
                visible: new_visible.unwrap_or(slot.visible),
                blend_mode: new_blend.and_then(map_blend).unwrap_or(slot.blend_mode),
                filters: new_filters.unwrap_or_else(|| slot.filters.clone()),
                clip_depth: new_clip_depth
                    .map(|value| (value > 0).then_some(value))
                    .unwrap_or(slot.clip_depth),
                masked_by: slot.masked_by.clone(),
                is_dynamic: false,
                ratio: new_ratio.or(slot.ratio),
                cache_as_bitmap: new_cache_as_bitmap.unwrap_or(slot.cache_as_bitmap),
                current_frame: 0,
                total_frames: sprites
                    .get(&character_id)
                    .map_or(1, |sprite| sprite.total_frames.max(1)),
                playing: true,
                last_applied_frame: None,
                children: BTreeMap::new(),
                props: HashMap::new(),
                clip_handlers: new_handlers,
            };
            return (load_scripts, unmodeled_filter);
        }
        if let Some(m) = new_matrix {
            slot.matrix = m;
            if let Some(tf) = Transform::from_matrix(m) {
                slot.x = tf.x;
                slot.y = tf.y;
                slot.scale_x = tf.scale_x * 100.0;
                slot.scale_y = tf.scale_y * 100.0;
                slot.rotation = tf.rotation;
                slot.skew = tf.skew;
            }
        }
        if let Some((ct, a)) = new_cxform {
            slot.color_transform = ct;
            slot.alpha = a * 100.0;
        }
        if let Some(rat) = new_ratio {
            slot.ratio = Some(rat);
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
        if let Some(cab) = new_cache_as_bitmap {
            slot.cache_as_bitmap = cab;
        }
        if let Some(b) = new_blend
            && let Some(bm) = map_blend(b)
        {
            slot.blend_mode = bm;
        }
        if let Some(fl) = new_filters {
            slot.filters = fl;
        }
        if has_clip_actions {
            slot.clip_handlers = new_handlers;
        }
        return (load_scripts, unmodeled_filter);
    }

    let Some(character_id) = new_char else {
        return (load_scripts, unmodeled_filter);
    };
    let matrix = new_matrix.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let tf = Transform::from_matrix(matrix).unwrap_or_default();
    let (color_transform, alpha) = new_cxform.map_or_else(
        || (ColorTransform::default(), 100.0),
        |(ct, a)| (ct, a * 100.0),
    );
    let sprite_total = sprites
        .get(&character_id)
        .map_or(1, |s| s.total_frames.max(1));

    push_ordered_clip_lifecycle_scripts(&new_handlers, &mut load_scripts);

    display_list.insert(
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
            skew: tf.skew,
            alpha,
            color_transform,
            visible: new_visible.unwrap_or(true),
            blend_mode: new_blend.and_then(map_blend).unwrap_or(BlendMode::Normal),
            filters: new_filters.unwrap_or_default(),
            clip_depth: new_clip_depth.filter(|&cd| cd > 0),
            masked_by: None,
            is_dynamic: false,
            ratio: new_ratio,
            cache_as_bitmap: new_cache_as_bitmap.unwrap_or(false),
            current_frame: 0,
            total_frames: sprite_total,
            playing: true,
            last_applied_frame: None,
            children: BTreeMap::new(),
            props: HashMap::new(),
            clip_handlers: new_handlers,
        },
    );
    (load_scripts, unmodeled_filter)
}

fn apply_remove_tag_to_map(
    display_list: &mut BTreeMap<u16, LiveSlot>,
    tag_code: u16,
    payload: &[u8],
) -> Vec<Vec<u8>> {
    let mut r = SwfSlice::new(payload);
    let depth = if tag_code == 5 {
        let _cid = r.u16();
        r.u16()
    } else {
        r.u16()
    };
    let mut unload_scripts = Vec::new();
    if let Some(d) = depth
        && let Some(removed) = display_list.remove(&d)
    {
        for h in removed.clip_handlers {
            if (h.event_flags & CLIP_EVENT_UNLOAD) != 0 {
                unload_scripts.push(h.actions);
            }
        }
    }
    unload_scripts
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

fn avm1_string_to_number(s: &str, swf_version: u8) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return if swf_version <= 4 { 0.0 } else { f64::NAN };
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    if let Some(hex) = t.strip_prefix("-0x").or_else(|| t.strip_prefix("-0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| -(v as f64));
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

fn avm1_equals(a: &Avm1Value, b: &Avm1Value, swf_version: u8, _objects: &[Avm1Object]) -> bool {
    match (a, b) {
        (Avm1Value::Undefined | Avm1Value::Null, Avm1Value::Undefined | Avm1Value::Null) => true,
        (Avm1Value::Undefined | Avm1Value::Null, _)
        | (_, Avm1Value::Undefined | Avm1Value::Null) => false,
        (Avm1Value::Number(x), Avm1Value::Number(y)) => x == y,
        (Avm1Value::Bool(x), Avm1Value::Bool(y)) => x == y,
        (Avm1Value::String(x), Avm1Value::String(y)) => x == y,
        (Avm1Value::Bool(x), other) => avm1_equals(
            &Avm1Value::Number(if *x { 1.0 } else { 0.0 }),
            other,
            swf_version,
            _objects,
        ),
        (other, Avm1Value::Bool(y)) => avm1_equals(
            other,
            &Avm1Value::Number(if *y { 1.0 } else { 0.0 }),
            swf_version,
            _objects,
        ),
        (Avm1Value::Number(x), Avm1Value::String(y))
        | (Avm1Value::String(y), Avm1Value::Number(x)) => {
            *x == avm1_string_to_number(y, swf_version)
        }
        (Avm1Value::Object(x), Avm1Value::Object(y)) => x == y,
        (Avm1Value::MovieClip(x), Avm1Value::MovieClip(y))
        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClipIdentity(y))
        | (Avm1Value::MovieClip(x), Avm1Value::MovieClipIdentity(y))
        | (Avm1Value::MovieClipIdentity(x), Avm1Value::MovieClip(y)) => x == y,
        _ => false,
    }
}

fn clear_mask_source_in_map(display_list: &mut BTreeMap<u16, LiveSlot>, mask_target: &str) {
    for slot in display_list.values_mut() {
        if slot.masked_by.as_deref() == Some(mask_target) {
            slot.masked_by = None;
        }
        clear_mask_source_in_map(&mut slot.children, mask_target);
    }
}

fn script_depth_to_swf(script_depth: i32) -> u16 {
    (16384_i64 + i64::from(script_depth)).clamp(0, 65535) as u16
}

fn parse_avm_int_radix(s: &str, radix: i32) -> f64 {
    let mut t = s.trim();
    if t.is_empty() {
        return f64::NAN;
    }
    let mut neg = false;
    if let Some(rest) = t.strip_prefix('-') {
        neg = true;
        t = rest;
    } else if let Some(rest) = t.strip_prefix('+') {
        t = rest;
    }
    let base: u32 = if radix == 0 {
        if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            t = rest;
            16
        } else {
            10
        }
    } else if (2..=36).contains(&radix) {
        let b = radix as u32;
        if b == 16
            && let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))
        {
            t = rest;
        }
        b
    } else {
        return f64::NAN;
    };
    let mut val = 0.0_f64;
    let mut any = false;
    for ch in t.chars() {
        let Some(d) = ch.to_digit(base) else {
            break;
        };
        any = true;
        val = val * f64::from(base) + f64::from(d);
    }
    if !any {
        f64::NAN
    } else if neg {
        -val
    } else {
        val
    }
}

fn runtime_element_supports_mask_mode(
    project: &Project,
    overrides: &HashMap<String, Symbol>,
    kind: &ElementKind,
    mode: MaskMode,
    visiting: &mut std::collections::HashSet<String>,
) -> bool {
    match kind {
        ElementKind::Path { .. } | ElementKind::PaintedPath { .. } => true,
        ElementKind::Rectangle { width, height, .. }
        | ElementKind::Ellipse { width, height, .. } => *width > 0.0 && *height > 0.0,
        ElementKind::Bitmap { resource_id, .. } if mode.uses_paint_coverage() => project
            .resources
            .iter()
            .find(|r| &r.id == resource_id)
            .is_some_and(|r| r.decode_image().is_ok()),
        ElementKind::Instance { symbol_id, .. } => {
            let Some(sym) = overrides
                .get(symbol_id)
                .or_else(|| project.symbol(symbol_id))
            else {
                return false;
            };
            if !visiting.insert(sym.id.clone()) {
                return false;
            }
            let valid = sym
                .timeline
                .layers
                .iter()
                .filter(|l| l.properties.role != LayerRole::Guide)
                .all(|layer| {
                    let effective_mode = if layer.properties.role == LayerRole::Mask {
                        layer.properties.mask_mode
                    } else {
                        mode
                    };
                    layer.keyframes.iter().flat_map(|k| &k.elements).all(|el| {
                        runtime_element_supports_mask_mode(
                            project,
                            overrides,
                            &el.kind,
                            effective_mode,
                            visiting,
                        )
                    })
                });
            visiting.remove(&sym.id);
            valid
        }
        _ => false,
    }
}

fn runtime_symbol_mask_mode(
    project: &Project,
    overrides: &HashMap<String, Symbol>,
    sym_id: &str,
    visiting: &mut std::collections::HashSet<String>,
) -> Option<MaskMode> {
    let kind = ElementKind::Instance {
        symbol_id: sym_id.to_owned(),
        playback: InstancePlayback::Loop,
    };
    visiting.clear();
    if runtime_element_supports_mask_mode(project, overrides, &kind, MaskMode::Geometry, visiting) {
        return Some(MaskMode::Geometry);
    }
    visiting.clear();
    if runtime_element_supports_mask_mode(project, overrides, &kind, MaskMode::Alpha, visiting) {
        return Some(MaskMode::Alpha);
    }
    None
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
