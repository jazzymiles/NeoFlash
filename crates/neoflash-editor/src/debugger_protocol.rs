//! Pure CDP state machine for the editor's privately owned Test Movie target.
//!
//! Protocol references: ChromeDevTools/devtools-protocol json/{js,browser}_protocol.json.
//! No implicit scope getter invocation, socket discovery or unrelated-target attachment.
use neoflash_core::ScriptLanguage;
use neoflash_script::RuntimeSourceLocation;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_PENDING: usize = 256;
const MAX_SCRIPTS: usize = 512;
const MAX_BREAKPOINTS: usize = 128;
const MAX_WATCHES: usize = 64;
const MAX_FRAMES: usize = 64;
const MAX_SCOPES: usize = 8;
const MAX_GENERATED_STEPS: usize = 64;
const MAX_LOCALS: usize = 256;
const MAX_INSPECTED_OBJECTS: usize = 128;
const MAX_OBJECT_PROPERTIES: usize = 256;
const MAX_TEXT: usize = 1_024;
const MAX_OUTPUT_EVENTS: usize = 128;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WatchRequest {
    pub expression: String,
    pub js_expression: String,
}

impl WatchRequest {
    pub fn new(expression: impl Into<String>, js_expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            js_expression: js_expression.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugWatchResult {
    pub expression: String,
    pub value: String,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DebugSource {
    pub source_url: String,
    pub code: String,
    pub locations: Vec<RuntimeSourceLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AuthoredBreakpoint {
    pub source: String,
    pub line: usize,
    pub column: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthoredLocation {
    pub source: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebugState {
    Connecting,
    Running,
    Paused,
    Disconnected,
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) enum DebugCommand {
    Pause,
    Resume,
    StepInto,
    StepOver,
    StepOut,
    Restart,
    SetBreakpoints(Vec<AuthoredBreakpoint>),
    SetWatches(Vec<WatchRequest>),
    EvaluateWatches,
    SelectFrame(usize),
    InspectObject {
        object_id: String,
        subtype: Option<String>,
        ancestor_object_ids: Vec<String>,
    },
    SetVariableValue {
        pause_generation: u64,
        frame: usize,
        scope_number: usize,
        variable_name: String,
        new_value: String,
    },
    SetPropertyValue {
        pause_generation: u64,
        frame: usize,
        object_id: String,
        property_name: String,
        new_value: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugFrame {
    pub function_name: String,
    pub location: Option<AuthoredLocation>,
    pub generated_url: String,
    pub generated_line: usize,
    pub generated_column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugLocal {
    pub scope: String,
    pub name: String,
    pub value: String,
    pub kind: String,
    pub accessor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BreakpointStatus {
    pub requested: AuthoredBreakpoint,
    pub resolved: Vec<AuthoredLocation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebugOutputKind {
    Trace,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugOutputRecord {
    pub seq: u64,
    pub kind: DebugOutputKind,
    pub formatted: String,
    pub location: Option<AuthoredLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugSnapshot {
    pub epoch: u64,
    pub state: DebugState,
    pub reason: String,
    pub frames: Vec<DebugFrame>,
    pub selected_frame: Option<usize>,
    pub locals: Vec<DebugLocal>,
    pub breakpoints: Vec<BreakpointStatus>,
    pub messages: Vec<String>,
    pub structured_messages: Vec<(u64, String)>,
    pub output_events: Vec<String>,
    pub structured_output: Vec<DebugOutputRecord>,
    pub command_pending: bool,
}

pub(crate) const MAX_INLINE_EDIT_STRING_CHARS: usize = MAX_TEXT;

const INSPECT_INDEXED_SLICE_JS: &str = "function(limit) { \
const out = Object.create(null); \
let count = 0; \
if (ArrayBuffer.isView(this) && !(this instanceof DataView) && typeof this.length === \"number\") { \
const n = Math.min(this.length, limit); \
for (let i = 0; i < n; i++) { out[i] = this[i]; count++; } \
out.length = this.length; \
out.byteLength = this.byteLength; \
out.byteOffset = this.byteOffset; \
return out; \
} \
if (Array.isArray(this)) { \
const len = this.length >>> 0; \
const maxProbe = Math.min(len, 4096); \
for (let i = 0; i < maxProbe && count < limit; i++) { \
if (Object.prototype.hasOwnProperty.call(this, i)) { \
const d = Object.getOwnPropertyDescriptor(this, i); \
if (d) { Object.defineProperty(out, i, d); count++; } \
} \
} \
if (count < limit && len > 4096) { \
for (let step = 4096; step < len && step <= 65536 && count < limit; step *= 2) { \
if (Object.prototype.hasOwnProperty.call(this, step)) { \
for (let i = step; i < len && count < limit && i < step + limit; i++) { \
if (Object.prototype.hasOwnProperty.call(this, i)) { \
const d = Object.getOwnPropertyDescriptor(this, i); \
if (d) { Object.defineProperty(out, i, d); count++; } \
} \
} \
} \
} \
} \
if (count < limit && len <= 65536) { \
const names = Object.getOwnPropertyNames(this); \
for (let j = 0; j < names.length && count < limit; j++) { \
const k = names[j]; \
if (k === \"length\" || Object.prototype.hasOwnProperty.call(out, k)) continue; \
const d = Object.getOwnPropertyDescriptor(this, k); \
if (d) { Object.defineProperty(out, k, d); count++; } \
} \
} \
if (count < limit) { \
Object.defineProperty(out, \"length\", { value: len, writable: true, enumerable: false, configurable: false }); \
} \
return out; \
} \
return this; \
}";

const SET_OWN_DATA_PROPERTY_JS: &str = "function(k, v) { const d = Object.getOwnPropertyDescriptor(this, k); if (!d || !('value' in d) || !d.writable) { throw new TypeError('Not a writable own data property'); } Object.defineProperty(this, k, { value: v }); }";

const DETECT_CIRCULAR_CHILDREN_JS: &str = "function(ancCount, ...objs) { const ancs = objs.slice(0, ancCount); const kids = objs.slice(ancCount); const out = []; for (let i = 0; i < kids.length; i++) { const c = kids[i]; for (let j = 0; j < ancs.length; j++) { if (c === ancs[j]) { out.push(i); break; } } } return out; }";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugInspectableLocal {
    pub scope_number: usize,
    pub scope: String,
    pub name: String,
    pub value: String,
    pub kind: String,
    pub subtype: Option<String>,
    pub object_id: Option<String>,
    pub accessor: bool,
    pub writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugInspectableWatch {
    pub index: usize,
    pub expression: String,
    pub subtype: Option<String>,
    pub object_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugObjectProperty {
    pub name: String,
    pub value: String,
    pub kind: String,
    pub subtype: Option<String>,
    pub object_id: Option<String>,
    pub accessor: bool,
    pub writable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DebugInspection {
    pub pause_epoch: u64,
    pub pause_generation: u64,
    pub vm_pause_generation: u64,
    pub pending_scope_requests: usize,
    pub locals: Vec<DebugInspectableLocal>,
    pub watches: Vec<DebugInspectableWatch>,
    pub object_properties: BTreeMap<String, Vec<DebugObjectProperty>>,
    pub circular_children: BTreeSet<(String, String)>,
    pub preview_properties: BTreeMap<String, Vec<(String, String, String)>>,
}

type InspectedObjectSpec = (Option<String>, Vec<String>);

#[derive(Clone, Debug)]
struct ParsedScript {
    source: usize,
    offset: Option<(usize, usize)>,
    rejected: bool,
}

#[derive(Clone, Debug)]
struct Scope {
    scope_number: usize,
    name: String,
    object_id: String,
}

#[derive(Clone, Debug)]
enum Pending {
    Create,
    Attach,
    Enable,
    BlackboxPatterns,
    BlackboxScript,
    Instrument,
    Navigate,
    Source(String),
    Breakpoint {
        script: String,
        request: Option<AuthoredBreakpoint>,
        generation: u64,
    },
    Remove,
    Control,
    GateResume,
    Properties {
        pause: u64,
        frame: usize,
        scope_number: usize,
        scope: String,
    },
    SliceObject {
        pause: u64,
        frame: usize,
        object_id: String,
        ancestor_object_ids: Vec<String>,
    },
    ObjectProperties {
        pause: u64,
        frame: usize,
        object_id: String,
        ancestor_object_ids: Vec<String>,
        release_object_id: Option<String>,
    },
    CycleCheck {
        pause: u64,
        frame: usize,
        parent_object_id: String,
        candidate_object_ids: Vec<String>,
    },
    Watch {
        pause: u64,
        watch_gen: u64,
        frame: usize,
        index: usize,
    },
    EditVariable {
        pause: u64,
        frame: usize,
    },
    EditProperty {
        pause: u64,
        frame: usize,
    },
}

pub(crate) struct DebugProtocol {
    page_url: String,
    sources: Vec<DebugSource>,
    desired: Vec<AuthoredBreakpoint>,
    watches: Vec<WatchRequest>,
    watch_results: Vec<DebugWatchResult>,
    inspection: DebugInspection,
    inspected_objects: BTreeMap<String, InspectedObjectSpec>,
    snapshot: DebugSnapshot,
    target: Option<String>,
    session: Option<String>,
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
    outgoing: VecDeque<Value>,
    enable_remaining: usize,
    scripts: BTreeMap<String, ParsedScript>,
    scopes: Vec<Vec<Scope>>,
    call_frame_ids: Vec<String>,
    breakpoint_ids: BTreeMap<String, (String, Option<AuthoredBreakpoint>)>,
    installed: BTreeSet<(String, usize, usize, u64)>,
    breakpoint_generation: u64,
    pause_epoch: u64,
    pause_generation: u64,
    vm_pause_generation: u64,
    watch_generation: u64,
    gate: Option<String>,
    pause_on_start: bool,
    first_pause: bool,
    generated_steps: Option<usize>,
    next_event_seq: u64,
}

mod events_pause;
mod formatting;
mod session_commands;
#[cfg(test)]
mod tests;
mod watch_compiler;

#[allow(unused_imports)]
use formatting::{
    bounded, console_arg_text, decode_cdp_property, extract_preview_scalar_fields, format_cdp_key,
    format_cdp_object_preview, format_cdp_property_preview, format_watch_exception, id_string,
    lifecycle_message, number, remote_inspectable_object_id, remote_summary, remote_summary_inner,
    wrapper_offset,
};
#[allow(unused_imports)]
pub(crate) use formatting::{parse_debug_call_argument, parse_debug_scalar_value};
#[allow(unused_imports)]
pub(crate) use watch_compiler::compile_watch_expression;
#[allow(unused_imports)]
use watch_compiler::translate_actionscript_watch;
