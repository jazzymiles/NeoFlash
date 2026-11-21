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
    AuthoredScene, BlendMode, ColorTransform, Element, ElementKind, FillRule, FilterSpec,
    InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MaskMode, PathCommand, Project,
    SoundCommand, SoundSync, Symbol, SymbolKind, Timeline, Transform,
    evaluate_scene_with_symbol_overrides, transform_matrix,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    io::Read,
    path::Path,
};

mod avm1;
mod avm1_runtime;
mod avm2;
mod avm2_runtime;
mod display;

use self::{avm1::*, avm2_runtime::*, display::*};

const MAX_VM_STEPS_PER_FRAME: usize = 50_000;
const MAX_CALL_DEPTH: usize = 32;
const MAX_AVM_STACK: usize = 4096;
const MAX_RUNTIME_SPRITE_DEPTH: usize = 64;
const MAX_FRAME_JUMPS_PER_STEP: usize = 32;

const CLIP_EVENT_LOAD: u32 = 0x0000_0001;
const CLIP_EVENT_ENTER_FRAME: u32 = 0x0000_0002;
const CLIP_EVENT_UNLOAD: u32 = 0x0000_0004;
const CLIP_EVENT_MOUSE_MOVE: u32 = 0x0000_0008;
const CLIP_EVENT_MOUSE_DOWN: u32 = 0x0000_0010;
const CLIP_EVENT_MOUSE_UP: u32 = 0x0000_0020;
const CLIP_EVENT_KEY_DOWN: u32 = 0x0000_0040;
const CLIP_EVENT_KEY_UP: u32 = 0x0000_0080;
const CLIP_EVENT_DATA: u32 = 0x0000_0100;
const CLIP_EVENT_INITIALIZE: u32 = 0x0000_0200;
const CLIP_EVENT_PRESS: u32 = 0x0000_0400;
const CLIP_EVENT_RELEASE: u32 = 0x0000_0800;
const CLIP_EVENT_RELEASE_OUTSIDE: u32 = 0x0000_1000;
const CLIP_EVENT_ROLL_OVER: u32 = 0x0000_2000;
const CLIP_EVENT_ROLL_OUT: u32 = 0x0000_4000;
const CLIP_EVENT_DRAG_OVER: u32 = 0x0000_8000;
const CLIP_EVENT_DRAG_OUT: u32 = 0x0001_0000;
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
    fn resolve_avm1_label(&self, query: &str) -> Option<u32> {
        self.labels
            .iter()
            .find(|(lbl, _)| lbl.eq_ignore_ascii_case(query))
            .map(|(_, frame)| *frame)
    }

    fn resolve_avm2_label(&self, query: &str) -> Option<u32> {
        self.labels
            .iter()
            .find(|(name, _)| name == query)
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

#[derive(Clone, Debug, Default)]
pub(super) struct RuntimeGraphicsState {
    pub(super) elements: Vec<Element>,
    pub(super) active_commands: Vec<PathCommand>,
    pub(super) fill: [u8; 4],
    pub(super) stroke: [u8; 4],
    pub(super) stroke_width: f64,
    pub(super) pen_x: f64,
    pub(super) pen_y: f64,
}

impl RuntimeGraphicsState {
    pub(super) fn is_empty(&self) -> bool {
        self.elements.is_empty() && !Self::commands_have_segments(&self.active_commands)
    }

    fn commands_have_segments(cmds: &[PathCommand]) -> bool {
        cmds.iter().any(|c| {
            matches!(
                c,
                PathCommand::LineTo { .. }
                    | PathCommand::QuadraticTo { .. }
                    | PathCommand::CubicTo { .. }
            )
        })
    }

    pub(super) fn flush_active_path(&mut self) {
        if Self::commands_have_segments(&self.active_commands)
            && (self.fill[3] > 0 || (self.stroke[3] > 0 && self.stroke_width > 0.0))
        {
            let mut cmds = std::mem::take(&mut self.active_commands);
            if self.fill[3] > 0 && !matches!(cmds.last(), Some(PathCommand::Close)) {
                cmds.push(PathCommand::Close);
            }
            let idx = self.elements.len() + 1;
            self.elements.push(Element {
                id: format!("rt-gfx-{idx}"),
                name: String::new(),
                visible: true,
                kind: ElementKind::Path {
                    commands: cmds,
                    fill_rule: FillRule::NonZero,
                    fill: self.fill,
                    stroke: self.stroke,
                    stroke_width: self.stroke_width,
                },
                ..Element::default()
            });
        } else {
            self.active_commands.clear();
        }
    }

    fn rgb_alpha_to_color(rgb: u32, alpha_norm: f64) -> [u8; 4] {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        let a = (alpha_norm.clamp(0.0, 1.0) * 255.0).round() as u8;
        [r, g, b, a]
    }

    pub(super) fn begin_fill(&mut self, rgb: u32, alpha_norm: f64) {
        self.flush_active_path();
        self.fill = Self::rgb_alpha_to_color(rgb, alpha_norm);
        self.active_commands.push(PathCommand::MoveTo {
            x: self.pen_x,
            y: self.pen_y,
        });
    }

    pub(super) fn end_fill(&mut self) {
        self.flush_active_path();
        self.fill = [0, 0, 0, 0];
    }

    pub(super) fn line_style(&mut self, thickness: Option<f64>, rgb: u32, alpha_norm: f64) {
        self.flush_active_path();
        if let Some(th) = thickness
            && th.is_finite()
            && th >= 0.0
        {
            self.stroke_width = th.max(0.1);
            self.stroke = Self::rgb_alpha_to_color(rgb, alpha_norm);
            self.active_commands.push(PathCommand::MoveTo {
                x: self.pen_x,
                y: self.pen_y,
            });
        } else {
            self.stroke_width = 0.0;
            self.stroke = [0, 0, 0, 0];
        }
    }

    pub(super) fn move_to(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.pen_x = x;
            self.pen_y = y;
            self.active_commands.push(PathCommand::MoveTo { x, y });
        }
    }

    pub(super) fn line_to(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            if self.active_commands.is_empty() {
                self.active_commands.push(PathCommand::MoveTo {
                    x: self.pen_x,
                    y: self.pen_y,
                });
            }
            self.pen_x = x;
            self.pen_y = y;
            self.active_commands.push(PathCommand::LineTo { x, y });
        }
    }

    pub(super) fn curve_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        if cx.is_finite() && cy.is_finite() && x.is_finite() && y.is_finite() {
            if self.active_commands.is_empty() {
                self.active_commands.push(PathCommand::MoveTo {
                    x: self.pen_x,
                    y: self.pen_y,
                });
            }
            self.pen_x = x;
            self.pen_y = y;
            self.active_commands
                .push(PathCommand::QuadraticTo { cx, cy, x, y });
        }
    }

    pub(super) fn draw_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
            return;
        }
        self.flush_active_path();
        let rx = if w < 0.0 { x + w } else { x };
        let ry = if h < 0.0 { y + h } else { y };
        let rw = w.abs();
        let rh = h.abs();
        if rw <= 0.0 || rh <= 0.0 {
            return;
        }
        let idx = self.elements.len() + 1;
        let mut el = Element {
            id: format!("rt-gfx-{idx}"),
            name: String::new(),
            visible: true,
            kind: ElementKind::Rectangle {
                width: rw,
                height: rh,
                fill: self.fill,
                stroke: self.stroke,
                stroke_width: self.stroke_width,
            },
            ..Element::default()
        };
        el.transform.x = rx;
        el.transform.y = ry;
        self.elements.push(el);
    }

    pub(super) fn draw_ellipse(&mut self, x: f64, y: f64, w: f64, h: f64) {
        if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
            return;
        }
        self.flush_active_path();
        let ex = if w < 0.0 { x + w } else { x };
        let ey = if h < 0.0 { y + h } else { y };
        let ew = w.abs();
        let eh = h.abs();
        if ew <= 0.0 || eh <= 0.0 {
            return;
        }
        let idx = self.elements.len() + 1;
        let mut el = Element {
            id: format!("rt-gfx-{idx}"),
            name: String::new(),
            visible: true,
            kind: ElementKind::Ellipse {
                width: ew,
                height: eh,
                fill: self.fill,
                stroke: self.stroke,
                stroke_width: self.stroke_width,
            },
            ..Element::default()
        };
        el.transform.x = ex;
        el.transform.y = ey;
        self.elements.push(el);
    }

    pub(super) fn clear(&mut self) {
        self.elements.clear();
        self.active_commands.clear();
        self.fill = [0, 0, 0, 0];
        self.stroke = [0, 0, 0, 0];
        self.stroke_width = 0.0;
        self.pen_x = 0.0;
        self.pen_y = 0.0;
    }

    pub(super) fn materialized_elements(&self, id_prefix: &str) -> Vec<Element> {
        let mut tmp = self.clone();
        tmp.flush_active_path();
        for (idx, el) in tmp.elements.iter_mut().enumerate() {
            el.id = format!("{id_prefix}-gfx-{}", idx + 1);
        }
        tmp.elements
    }
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
    origin_timeline_depth: Option<u16>,
    ratio: Option<u16>,
    cache_as_bitmap: bool,
    current_frame: u32,
    total_frames: u32,
    playing: bool,
    last_applied_frame: Option<u32>,
    removed_timeline_depths: BTreeSet<u16>,
    children: BTreeMap<u16, LiveSlot>,
    props: HashMap<String, Avm1Value>,
    clip_handlers: Vec<ClipEventHandler>,
    graphics: RuntimeGraphicsState,
    placed_this_step: bool,
    avm2_object_id: Option<usize>,
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
                    if let Some((ev_ty, bubbles, cancelable, _, _, phase)) = &obj.avm2_event {
                        return format!(
                            "[Event type=\"{ev_ty}\" bubbles={bubbles} cancelable={cancelable} eventPhase={phase}]"
                        );
                    }
                    if let Some(meta) = &obj.avm2_display {
                        return format!("[object {}]", meta.short_class_name);
                    }
                    if let Some(err_cls) = &obj.avm2_builtin_error {
                        let name = obj
                            .props
                            .get("name")
                            .and_then(|v| match v {
                                Self::String(s) if !s.is_empty() => Some(s.as_str()),
                                _ => None,
                            })
                            .unwrap_or(err_cls.as_str());
                        let msg = obj
                            .props
                            .get("message")
                            .and_then(|v| match v {
                                Self::String(s) => Some(s.as_str()),
                                _ => None,
                            })
                            .unwrap_or("");
                        return if msg.is_empty() {
                            name.to_owned()
                        } else {
                            format!("{name}: {msg}")
                        };
                    }
                    if let Some(elements) = &obj.array_elements {
                        return (0..elements.len())
                            .map(|idx| {
                                if obj.deleted_indices.contains(&idx) {
                                    if swf_version <= 8 {
                                        "undefined".to_owned()
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    elements[idx].to_trace_string(swf_version, objects)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                    }
                    if obj.function.is_some() {
                        return "[type Function]".to_owned();
                    }
                    if let Some(Self::String(cls_short)) = obj.props.get("__avm2_short_class__") {
                        return format!("[object {cls_short}]");
                    }
                }
                "[object Object]".to_owned()
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Avm2DisplayMeta {
    display_type: String,
    short_class_name: String,
    character_id: u16,
    parent_oid: Option<usize>,
    on_stage: bool,
    constructed: bool,
    super_constructed: bool,
    explicit_timeline_name: Option<String>,
    children_oids: Vec<usize>,
    frame_scripts: HashMap<u32, Avm1Value>,
    last_ran_frame_script: Option<u32>,
    current_frame: u32,
    total_frames: u32,
    playing: bool,
    listeners: HashMap<String, Vec<Avm1Value>>,
}

#[derive(Clone, Debug, Default)]
struct Avm1Object {
    props: HashMap<String, Avm1Value>,
    array_elements: Option<Vec<Avm1Value>>,
    deleted_indices: BTreeSet<usize>,
    function: Option<Avm1Function>,
    interfaces: Vec<usize>,
    avm2_class: Option<(usize, usize)>,
    avm2_activation_method: Option<(usize, usize)>,
    avm2_catch_var: Option<(String, usize, usize)>,
    avm2_builtin_error: Option<String>,
    avm2_display: Option<Avm2DisplayMeta>,
    avm2_event: Option<(String, bool, bool, usize, usize, u8)>,
    graphics: Option<RuntimeGraphicsState>,
    avm2_graphics_owner: Option<usize>,
}

#[derive(Clone, Debug)]
struct Avm2Closure {
    abc_idx: usize,
    method_idx: usize,
    outer_scopes: Vec<Avm1Value>,
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
    captured_scopes: Vec<usize>,
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
    button_records: HashMap<u16, Vec<(u8, u16, u16)>>,
    char_kinds: HashMap<u16, &'static str>,
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
    root_graphics: RuntimeGraphicsState,
    display_list: BTreeMap<u16, LiveSlot>,
    removed_timeline_depths: BTreeSet<u16>,
    trace_output: Vec<String>,
    frame_traces: Vec<String>,
    audio_events: Vec<SwfAudioEvent>,
    frame_audio_events: Vec<SwfAudioEvent>,
    unsupported_diagnostics: Vec<String>,
    frame_unsupported_diagnostics: Vec<String>,
    stream_sound_active: bool,
    stop_all_sounds_pending: bool,

    // AVM1 state
    avm1_init_actions_executed: bool,
    avm1_globals: HashMap<String, Avm1Value>,
    avm1_global_obj: HashMap<String, Avm1Value>,
    avm1_functions: HashMap<String, Avm1Function>,
    avm1_registered_classes: HashMap<String, usize>,
    avm1_objects: Vec<Avm1Object>,
    avm1_registers: [Avm1Value; 256],
    avm1_constant_pool: Vec<String>,
    avm1_target: String,
    avm1_detached_target: Option<(String, LiveSlot)>,
    avm1_scope_chain: Vec<usize>,
    avm1_with_scopes: Vec<Avm1Value>,
    avm1_thrown: Option<Avm1Value>,
    drag_target: Option<String>,
    rng_state: u32,

    // AVM2 state
    avm2_initialized: bool,
    avm2_stage_oid: Option<usize>,
    avm2_root_oid: Option<usize>,
    avm2_next_instance_id: u32,
    avm2_sync_goto_handled: bool,
    avm2_globals: HashMap<String, Avm1Value>,
    avm2_frame_scripts: HashMap<u32, Avm1Value>, // frame_idx -> callable token
    avm2_closures: Vec<Avm2Closure>,
    avm2_thrown: Option<Avm1Value>,
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
        let mut button_records: HashMap<u16, Vec<(u8, u16, u16)>> = HashMap::new();
        let mut char_kinds: HashMap<u16, &'static str> = HashMap::new();
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
                2 | 22 | 32 | 46 | 83 | 84 | 11 | 33 => {
                    if payload.len() >= 2 {
                        let cid = u16::from_le_bytes([payload[0], payload[1]]);
                        char_kinds.insert(cid, "Shape");
                    }
                }
                20 | 21 | 35 | 36 => {
                    if payload.len() >= 2 {
                        let cid = u16::from_le_bytes([payload[0], payload[1]]);
                        char_kinds.insert(cid, "Bitmap");
                    }
                }
                37 => {
                    if payload.len() >= 2 {
                        let cid = u16::from_le_bytes([payload[0], payload[1]]);
                        char_kinds.insert(cid, "TextField");
                    }
                }
                60 => {
                    if payload.len() >= 2 {
                        let cid = u16::from_le_bytes([payload[0], payload[1]]);
                        char_kinds.insert(cid, "Video");
                    }
                }
                39 => {
                    // DefineSprite
                    let mut sr = SwfSlice::new(payload);
                    if let (Some(sprite_id), Some(fc)) = (sr.u16(), sr.u16()) {
                        let def = parse_timeline_tags(sr.rest(), container.swf_version, fc);
                        sprites.insert(sprite_id, def);
                        char_kinds.insert(sprite_id, "MovieClip");
                    }
                }
                7 | 34 => {
                    // DefineButton / DefineButton2
                    if let Some((btn_id, recs, conds)) = parse_button_actions(code, payload) {
                        buttons.insert(btn_id, conds);
                        button_records.insert(btn_id, recs);
                        char_kinds.insert(btn_id, "SimpleButton");
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
            button_records,
            char_kinds,
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
            root_graphics: RuntimeGraphicsState::default(),
            display_list: BTreeMap::new(),
            removed_timeline_depths: BTreeSet::new(),
            trace_output: Vec::new(),
            frame_traces: Vec::new(),
            audio_events: Vec::new(),
            frame_audio_events: Vec::new(),
            unsupported_diagnostics: Vec::new(),
            frame_unsupported_diagnostics: Vec::new(),
            stream_sound_active: false,
            stop_all_sounds_pending: false,
            avm1_init_actions_executed: false,
            avm1_globals: HashMap::new(),
            avm1_global_obj: HashMap::new(),
            avm1_functions: HashMap::new(),
            avm1_registered_classes: HashMap::new(),
            avm1_objects: Vec::new(),
            avm1_registers: std::array::from_fn(|_| Avm1Value::Undefined),
            avm1_constant_pool: Vec::new(),
            avm1_target: String::new(),
            avm1_detached_target: None,
            avm1_scope_chain: Vec::new(),
            avm1_with_scopes: Vec::new(),
            avm1_thrown: None,
            drag_target: None,
            rng_state: 0x1234_5678,
            avm2_initialized: false,
            avm2_stage_oid: None,
            avm2_root_oid: None,
            avm2_next_instance_id: 1,
            avm2_sync_goto_handled: false,
            avm2_globals: HashMap::new(),
            avm2_frame_scripts: HashMap::new(),
            avm2_closures: Vec::new(),
            avm2_thrown: None,
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
        self.root_graphics.clear();
        self.display_list.clear();
        self.removed_timeline_depths.clear();
        self.trace_output.clear();
        self.frame_traces.clear();
        self.audio_events.clear();
        self.frame_audio_events.clear();
        self.unsupported_diagnostics.clear();
        self.frame_unsupported_diagnostics.clear();
        self.stream_sound_active = false;
        self.stop_all_sounds_pending = false;
        self.avm1_init_actions_executed = false;
        self.avm1_globals.clear();
        self.avm1_global_obj.clear();
        self.avm1_functions.clear();
        self.avm1_registered_classes.clear();
        self.avm1_objects.clear();
        self.avm1_registers = std::array::from_fn(|_| Avm1Value::Undefined);
        self.avm1_constant_pool.clear();
        self.avm1_target.clear();
        self.avm1_detached_target = None;
        self.avm1_scope_chain.clear();
        self.avm1_with_scopes.clear();
        self.avm1_thrown = None;
        self.drag_target = None;
        self.rng_state = 0x1234_5678;
        self.avm2_initialized = false;
        self.avm2_stage_oid = None;
        self.avm2_root_oid = None;
        self.avm2_next_instance_id = 1;
        self.avm2_sync_goto_handled = false;
        self.avm2_globals.clear();
        self.avm2_frame_scripts.clear();
        self.avm2_closures.clear();
        self.avm2_thrown = None;
    }

    /// Reset all runtime display-list, tombstone, and VM state and evaluate frame 0 cleanly.
    pub fn restart(&mut self) -> SwfFrameState {
        self.reset();
        self.step_frame()
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

    /// Execute one frame step on the SWF timeline and return the resulting `SwfFrameState`.
    pub fn step_frame(&mut self) -> SwfFrameState {
        self.step_frame_inner(0, true)
    }

    fn step_frame_inner(&mut self, jump_depth: usize, follow_frame_control: bool) -> SwfFrameState {
        if jump_depth == 0 {
            self.avm1_thrown = None;
            self.avm2_thrown = None;
        }
        self.frame_traces.clear();
        self.frame_audio_events.clear();
        self.frame_unsupported_diagnostics.clear();

        if self.root_loop_rewind_pending {
            self.root_loop_rewind_pending = false;
            let source_frame = self.root_timeline.frames.len().saturating_sub(1) as u32;
            let destination_frame = self.current_frame;
            if destination_frame != source_frame {
                self.prepare_root_display_list_jump(source_frame, destination_frame);
            }
        } else if let Some(prev_applied) = self.last_applied_frame
            && (self.current_frame < prev_applied
                || self.current_frame > prev_applied.saturating_add(1))
        {
            let destination_frame = self.current_frame;
            self.prepare_root_display_list_jump(prev_applied, destination_frame);
        }

        if !self.avm1_init_actions_executed {
            self.avm1_init_actions_executed = true;
            // Run DoInitAction blocks on frame 0 before frame tags
            let init_blocks = self.init_actions.clone();
            for (_, code) in init_blocks {
                self.execute_avm1_slice(&code);
            }
        }

        let mut entering_frame = self.current_frame;
        let frame_changed = self.last_applied_frame != Some(entering_frame);
        self.last_applied_frame = Some(entering_frame);
        self.avm2_sync_goto_handled = false;

        let mut frame_label = None;

        if self.vm_kind == SwfVmKind::Avm1 {
            let mut action_queue: Vec<PendingAvm1Work> = Vec::new();
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
                            if let Some((lbl, _)) = lr.c_string(self.swf_version)
                                && !lbl.is_empty()
                            {
                                frame_label.get_or_insert(lbl);
                            }
                        }
                        4 | 26 | 70 => {
                            let work = self.apply_place_tag_avm1(
                                tag.code,
                                &tag.payload,
                                entering_frame == 0,
                            );
                            action_queue.extend(work);
                        }
                        5 | 28 => {
                            action_queue.extend(self.apply_remove_tag(tag.code, &tag.payload));
                        }
                        12 => {
                            action_queue.push(PendingAvm1Work::Action {
                                target: "_root".to_owned(),
                                actions: tag.payload,
                            });
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

            let nested_advance_work = self.advance_preexisting_avm1_sprites();
            let mut enter_frame_work = Vec::new();
            collect_preexisting_enter_frame_work(&self.display_list, "", &mut enter_frame_work);
            clear_placed_this_step_in_map(&mut self.display_list);

            self.execute_avm1_work_list(enter_frame_work);
            self.execute_avm1_work_list(nested_advance_work);
            self.execute_avm1_work_list(action_queue);

            // Sync any nested sprites whose `current_frame` was modified by scripts
            let post_depths: Vec<u16> = self.display_list.keys().copied().collect();
            for d in post_depths {
                self.step_sprite_frame_actions(d, false);
            }
            clear_placed_this_step_in_map(&mut self.display_list);
        } else {
            if self.avm2_initialized {
                self.dispatch_avm2_broadcast_event("enterFrame");
            }

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
                            if let Some((lbl, _)) = lr.c_string(self.swf_version)
                                && !lbl.is_empty()
                            {
                                frame_label.get_or_insert(lbl);
                            }
                        }
                        4 | 26 | 70 => {
                            let _ = self.apply_place_tag(tag.code, &tag.payload);
                        }
                        5 | 28 => {
                            let _ = self.apply_remove_tag(tag.code, &tag.payload);
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

            if !self.avm2_initialized {
                self.avm2_initialized = true;
                self.initialize_avm2();
            } else {
                if frame_changed && let Some(root_oid) = self.avm2_root_oid {
                    if let Some(meta) = self
                        .avm1_objects
                        .get_mut(root_oid)
                        .and_then(|o| o.avm2_display.as_mut())
                    {
                        meta.current_frame = entering_frame;
                        meta.last_ran_frame_script = None;
                    }
                    let mut steps = 0usize;
                    self.sync_avm2_container_with_display_list(root_oid, "_root", &mut steps, 0);
                }
                self.advance_avm2_nested_timelines();
            }

            let mut steps = 0usize;
            self.run_avm2_frame_phase(&mut steps, 0);

            if self.avm2_sync_goto_handled {
                entering_frame = self.current_frame;
                frame_label = self
                    .root_timeline
                    .labels
                    .iter()
                    .find(|(_, f)| *f == entering_frame)
                    .map(|(l, _)| l.clone());
                self.avm2_sync_goto_handled = false;
            }
            clear_placed_this_step_in_map(&mut self.display_list);
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

    /// Trigger button condition / clip mouse press+release events on placed buttons/clips.
    pub fn trigger_pointer_click(&mut self) {
        self.trigger_pointer_event(
            CLIP_EVENT_MOUSE_UP | CLIP_EVENT_MOUSE_DOWN | CLIP_EVENT_RELEASE | CLIP_EVENT_PRESS,
            0x000C,
        );
    }

    /// Trigger SWF 6/7+ RollOver / RollOut / ReleaseOutside / DragOver / DragOut / MouseMove clip & button events.
    pub fn trigger_pointer_roll_over(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_ROLL_OVER, 0x0001);
    }

    pub fn trigger_pointer_roll_out(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_ROLL_OUT, 0x0002);
    }

    pub fn trigger_pointer_release_outside(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_RELEASE_OUTSIDE, 0x0010);
    }

    pub fn trigger_pointer_drag_over(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_DRAG_OVER, 0x0020 | 0x0100);
    }

    pub fn trigger_pointer_drag_out(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_DRAG_OUT, 0x0040 | 0x0080);
    }

    pub fn trigger_pointer_move(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_MOUSE_MOVE, 0);
    }

    pub fn trigger_data_event(&mut self) {
        self.trigger_pointer_event(CLIP_EVENT_DATA, 0);
    }

    pub fn trigger_key_down(&mut self, key_code: Option<u8>) {
        self.trigger_key_event(CLIP_EVENT_KEY_DOWN, key_code);
    }

    pub fn trigger_key_up(&mut self, key_code: Option<u8>) {
        self.trigger_key_event(CLIP_EVENT_KEY_UP, key_code);
    }

    pub fn trigger_key_press(&mut self, key_code: u8) {
        self.trigger_key_event(CLIP_EVENT_KEY_PRESS, Some(key_code));
    }

    /// Trigger specific SWF clip mouse/button condition flags (e.g. RollOver, RollOut, ReleaseOutside, DragOver, DragOut).
    pub fn trigger_pointer_event(&mut self, clip_mask: u32, btn_cond_mask: u16) {
        let buttons = self.buttons.clone();
        let mut actions = Vec::new();
        collect_pointer_actions_filtered(
            &self.display_list,
            "",
            &buttons,
            clip_mask,
            btn_cond_mask,
            &mut actions,
        );
        for (target, code) in actions {
            self.execute_avm1_for_target(&target, &code);
        }
    }

    /// Trigger SWF clip/button key events (`KeyDown`, `KeyUp`, `KeyPress` with SWF key code).
    pub fn trigger_key_event(&mut self, clip_mask: u32, key_code: Option<u8>) {
        let buttons = self.buttons.clone();
        let mut actions = Vec::new();
        collect_key_actions_filtered(
            &self.display_list,
            "",
            &buttons,
            clip_mask,
            key_code,
            &mut actions,
        );
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
            let any_child_playing =
                any_playing_descendant(&self.display_list) || self.has_active_avm2_tick_listeners();
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
