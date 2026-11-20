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

#[derive(Clone, Debug)]
struct ParsedScript {
    source: usize,
    offset: Option<(usize, usize)>,
    rejected: bool,
}

#[derive(Clone, Debug)]
struct Scope {
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
        scope: String,
    },
    Watch {
        pause: u64,
        watch_gen: u64,
        frame: usize,
        index: usize,
    },
}

pub(crate) struct DebugProtocol {
    page_url: String,
    sources: Vec<DebugSource>,
    desired: Vec<AuthoredBreakpoint>,
    watches: Vec<WatchRequest>,
    watch_results: Vec<DebugWatchResult>,
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
    pause_generation: u64,
    watch_generation: u64,
    gate: Option<String>,
    pause_on_start: bool,
    first_pause: bool,
    generated_steps: Option<usize>,
    next_event_seq: u64,
}

impl DebugProtocol {
    pub fn new(
        page_url: String,
        sources: Vec<DebugSource>,
        breakpoints: Vec<AuthoredBreakpoint>,
    ) -> Self {
        let mut result = Self {
            page_url,
            sources,
            desired: Vec::new(),
            watches: Vec::new(),
            watch_results: Vec::new(),
            snapshot: DebugSnapshot {
                epoch: 1,
                state: DebugState::Disconnected,
                reason: String::new(),
                frames: Vec::new(),
                selected_frame: None,
                locals: Vec::new(),
                breakpoints: Vec::new(),
                messages: Vec::new(),
                structured_messages: Vec::new(),
                output_events: Vec::new(),
                structured_output: Vec::new(),
                command_pending: false,
            },
            target: None,
            session: None,
            next_id: 1,
            pending: BTreeMap::new(),
            outgoing: VecDeque::new(),
            enable_remaining: 0,
            scripts: BTreeMap::new(),
            scopes: Vec::new(),
            call_frame_ids: Vec::new(),
            breakpoint_ids: BTreeMap::new(),
            installed: BTreeSet::new(),
            breakpoint_generation: 1,
            pause_generation: 0,
            watch_generation: 0,
            gate: None,
            pause_on_start: false,
            first_pause: false,
            generated_steps: None,
            next_event_seq: 1,
        };
        result.replace_desired(breakpoints);
        result
    }

    pub fn set_watches(&mut self, watches: Vec<WatchRequest>) {
        self.replace_watches(watches);
    }

    pub fn watch_results(&self) -> Vec<DebugWatchResult> {
        self.watch_results.clone()
    }

    pub fn epoch(&self) -> u64 {
        self.snapshot.epoch
    }
    pub fn snapshot(&self) -> DebugSnapshot {
        self.snapshot.clone()
    }
    pub fn has_pending_work(&self) -> bool {
        self.gate.is_some()
            || self.snapshot.command_pending
            || self
                .pending
                .values()
                .any(|request| !matches!(request, Pending::BlackboxScript))
    }
    pub fn set_pause_on_start(&mut self, value: bool) {
        self.pause_on_start = value;
    }

    pub fn start(&mut self) {
        if self.target.is_some() || !self.pending.is_empty() {
            return;
        }
        self.snapshot.state = DebugState::Connecting;
        self.snapshot.reason = "Opening Test Movie debugger".into();
        self.queue(
            "Target.createTarget",
            json!({"url":"about:blank"}),
            Pending::Create,
            false,
        );
    }

    pub fn drain_commands(&mut self) -> Vec<Value> {
        self.outgoing.drain(..).collect()
    }

    pub fn disconnect(&mut self, reason: impl Into<String>) {
        self.clear_session();
        self.snapshot.state = DebugState::Disconnected;
        self.snapshot.reason = bounded(&reason.into(), MAX_TEXT);
    }

    pub fn command(&mut self, command: DebugCommand) -> Result<(), String> {
        match command {
            DebugCommand::Restart => {
                let target = self.target.clone();
                self.clear_session();
                self.snapshot.output_events.clear();
                self.snapshot.structured_output.clear();
                self.snapshot.epoch = self.snapshot.epoch.saturating_add(1);
                self.first_pause = false;
                if let Some(target) = target {
                    self.queue(
                        "Target.closeTarget",
                        json!({"targetId":target}),
                        Pending::Remove,
                        false,
                    );
                }
                self.snapshot.state = DebugState::Connecting;
                self.snapshot.reason = "Restarting Test Movie debugger".into();
                self.queue(
                    "Target.createTarget",
                    json!({"url":"about:blank"}),
                    Pending::Create,
                    false,
                );
                return Ok(());
            }
            DebugCommand::SetBreakpoints(points) => {
                if matches!(
                    self.snapshot.state,
                    DebugState::Disconnected | DebugState::Failed
                ) {
                    return Err(
                        "Start Test Movie debugging before changing active breakpoints.".into(),
                    );
                }

                self.replace_desired(points);
                self.breakpoint_generation += 1;
                self.installed.clear();
                let ids: Vec<_> = self.breakpoint_ids.keys().cloned().collect();
                self.breakpoint_ids.clear();
                for id in ids {
                    self.queue(
                        "Debugger.removeBreakpoint",
                        json!({"breakpointId":id}),
                        Pending::Remove,
                        true,
                    );
                }
                let scripts: Vec<_> = self.scripts.keys().cloned().collect();
                for id in scripts {
                    self.install(&id);
                }
                return Ok(());
            }
            DebugCommand::SetWatches(watches) => {
                if matches!(
                    self.snapshot.state,
                    DebugState::Disconnected | DebugState::Failed
                ) {
                    return Err("Start Test Movie debugging before changing active watches.".into());
                }
                self.replace_watches(watches);
                if self.snapshot.state == DebugState::Paused
                    && let Some(index) = self.snapshot.selected_frame
                {
                    self.request_watches(index);
                }
                return Ok(());
            }
            DebugCommand::EvaluateWatches => {
                if matches!(
                    self.snapshot.state,
                    DebugState::Disconnected | DebugState::Failed
                ) {
                    return Err("Start Test Movie debugging before evaluating watches.".into());
                }
                if self.snapshot.state == DebugState::Paused
                    && let Some(index) = self.snapshot.selected_frame
                {
                    self.request_watches(index);
                }
                return Ok(());
            }
            DebugCommand::SelectFrame(index) => {
                if self.snapshot.state != DebugState::Paused || index >= self.scopes.len() {
                    return Err("Select a call frame while the debugger is paused.".into());
                }
                self.snapshot.selected_frame = Some(index);
                self.request_locals(index);
                self.request_watches(index);
                return Ok(());
            }
            _ => {}
        }
        if self.snapshot.command_pending || self.gate.is_some() {
            return Err("Wait for the current debugger operation to finish.".into());
        }
        let stepping = matches!(
            command,
            DebugCommand::StepInto | DebugCommand::StepOver | DebugCommand::StepOut
        );
        let (method, required) = match command {
            DebugCommand::Pause => ("Debugger.pause", DebugState::Running),
            DebugCommand::Resume => ("Debugger.resume", DebugState::Paused),
            DebugCommand::StepInto => ("Debugger.stepInto", DebugState::Paused),
            DebugCommand::StepOver => ("Debugger.stepOver", DebugState::Paused),
            DebugCommand::StepOut => ("Debugger.stepOut", DebugState::Paused),
            _ => unreachable!(),
        };
        if self.snapshot.state != required {
            return Err(if required == DebugState::Paused {
                "Pause Test Movie before stepping or continuing."
            } else {
                "Test Movie is not running."
            }
            .into());
        }
        self.pause_generation += 1;
        self.snapshot.locals.clear();
        if !self.queue(method, json!({}), Pending::Control, true) {
            return Err("Debugger request limit reached.".into());
        }
        self.snapshot.command_pending = true;
        self.generated_steps = stepping.then_some(MAX_GENERATED_STEPS);
        Ok(())
    }

    pub fn receive(&mut self, epoch: u64, message: Value) {
        if epoch != self.epoch() {
            return;
        }
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            let Some(pending) = self.pending.remove(&id) else {
                return;
            };
            if let Some(error) = message.get("error") {
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("CDP request failed");
                self.request_error(pending, text);
            } else {
                self.response(pending, message.get("result").unwrap_or(&Value::Null));
            }
            self.finish_gate();
            return;
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = message.get("params").unwrap_or(&Value::Null);
        if method == "Target.detachedFromTarget"
            && params.get("sessionId").and_then(Value::as_str) == self.session.as_deref()
        {
            self.disconnect("Test Movie debugger disconnected");
            return;
        }
        if matches!(method, "Target.targetDestroyed" | "Target.targetCrashed")
            && params.get("targetId").and_then(Value::as_str) == self.target.as_deref()
        {
            self.disconnect(lifecycle_message(
                if method == "Target.targetCrashed" {
                    "Test Movie target crashed"
                } else {
                    "Test Movie window closed"
                },
                params,
            ));
            return;
        }
        if self.session.is_none()
            || message.get("sessionId").and_then(Value::as_str) != self.session.as_deref()
        {
            return;
        }
        match method {
            "Inspector.detached" | "Inspector.targetCrashed" => self.disconnect(lifecycle_message(
                if method == "Inspector.targetCrashed" {
                    "Test Movie renderer crashed"
                } else {
                    "Test Movie debugger detached"
                },
                params,
            )),
            "Debugger.scriptParsed" => self.script_parsed(params),
            "Debugger.paused" => self.paused(params),
            "Debugger.resumed" => {
                self.clear_pause();
                self.snapshot.state = DebugState::Running;
                self.snapshot.reason = "Running".into();
                self.snapshot.command_pending = false;
            }
            "Runtime.executionContextsCleared" => {
                self.clear_pause();
                self.scripts.clear();
                self.installed.clear();
                self.breakpoint_ids.clear();
                self.breakpoint_generation += 1;
                self.refresh_breakpoints();
            }
            "Runtime.consoleAPICalled" => self.console_api_called(params),
            "Runtime.exceptionThrown" => self.exception_thrown(params),
            _ => {}
        }
    }

    fn queue(&mut self, method: &str, params: Value, pending: Pending, session: bool) -> bool {
        if self.pending.len() >= MAX_PENDING || self.outgoing.len() >= MAX_PENDING {
            self.note("Debugger request limit reached; some inspection results are unavailable.");
            return false;
        }
        if session && self.session.is_none() {
            return false;
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let mut message = json!({"id":id,"method":method,"params":params});
        if session {
            message["sessionId"] = json!(self.session);
        }
        self.pending.insert(id, pending);
        self.outgoing.push_back(message);
        true
    }

    fn response(&mut self, pending: Pending, result: &Value) {
        match pending {
            Pending::Create => {
                let Some(target) = id_string(result.get("targetId")) else {
                    self.fail("Chrome did not return a Test Movie target.");
                    return;
                };
                self.target = Some(target.clone());
                self.queue(
                    "Target.attachToTarget",
                    json!({"targetId":target,"flatten":true}),
                    Pending::Attach,
                    false,
                );
            }
            Pending::Attach => {
                let Some(session) = id_string(result.get("sessionId")) else {
                    self.fail("Chrome did not return a debugger session.");
                    return;
                };
                self.session = Some(session);
                self.enable_remaining = 3;
                for method in ["Runtime.enable", "Debugger.enable", "Page.enable"] {
                    self.queue(method, json!({}), Pending::Enable, true);
                }
            }
            Pending::Enable => {
                self.enable_remaining = self.enable_remaining.saturating_sub(1);
                if self.enable_remaining == 0 {
                    self.queue(
                        "Debugger.setBlackboxPatterns",
                        json!({"patterns":[self.host_blackbox_pattern()],"skipAnonymous":true}),
                        Pending::BlackboxPatterns,
                        true,
                    );
                }
            }
            Pending::BlackboxPatterns => {
                self.queue(
                    "Debugger.setInstrumentationBreakpoint",
                    json!({"instrumentation":"beforeScriptExecution"}),
                    Pending::Instrument,
                    true,
                );
            }
            // Chromium can defer this optional acknowledgement until the VM resumes.
            Pending::BlackboxScript => {}
            Pending::Instrument => {
                self.queue(
                    "Page.navigate",
                    json!({"url":self.page_url}),
                    Pending::Navigate,
                    true,
                );
            }
            Pending::Navigate => {
                if let Some(error) = result.get("errorText").and_then(Value::as_str) {
                    self.fail(error);
                } else if self.snapshot.state != DebugState::Paused {
                    self.snapshot.state = DebugState::Running;
                    self.snapshot.reason = "Running".into();
                }
            }
            Pending::Source(id) => {
                let Some(parsed) = self.scripts.get(&id) else {
                    return;
                };
                let source = &self.sources[parsed.source];
                let offset = result
                    .get("scriptSource")
                    .and_then(Value::as_str)
                    .and_then(|text| wrapper_offset(source, text));
                if let Some(parsed) = self.scripts.get_mut(&id) {
                    parsed.offset = offset;
                    parsed.rejected = offset.is_none();
                }
                if offset.is_none() {
                    self.note("A script claimed a NeoFlash source URL but did not match the exported source; it remains unmapped.");
                } else {
                    self.blackbox_script(&id);
                    self.install(&id);
                }
            }
            Pending::Breakpoint {
                script,
                request,
                generation,
            } => {
                let Some(id) = id_string(result.get("breakpointId")) else {
                    return;
                };
                if generation != self.breakpoint_generation || !self.scripts.contains_key(&script) {
                    self.queue(
                        "Debugger.removeBreakpoint",
                        json!({"breakpointId":id}),
                        Pending::Remove,
                        true,
                    );
                    return;
                }
                let actual =
                    self.map_location(result.get("actualLocation").unwrap_or(&Value::Null));
                let acceptable = actual.as_ref().is_some_and(|location| {
                    request.as_ref().is_none_or(|request| {
                        location.source == request.source
                            && location.line == request.line
                            && request
                                .column
                                .is_none_or(|column| location.column >= column)
                    })
                });
                if !acceptable {
                    self.note(
                        "Chrome could not place a breakpoint on the requested authored statement.",
                    );
                    self.queue(
                        "Debugger.removeBreakpoint",
                        json!({"breakpointId":id}),
                        Pending::Remove,
                        true,
                    );
                    return;
                }
                if let (Some(request), Some(location)) = (&request, actual)
                    && let Some(status) = self
                        .snapshot
                        .breakpoints
                        .iter_mut()
                        .find(|status| status.requested == *request)
                    && !status.resolved.contains(&location)
                {
                    status.resolved.push(location);
                }
                self.breakpoint_ids.insert(id, (script, request));
            }
            // Command acknowledgements do not establish VM state. Only paused/resumed events
            // release execution controls, including when a delayed reply follows another step.
            Pending::Control | Pending::GateResume => {}
            Pending::Properties {
                pause,
                frame,
                scope,
            } => {
                if self.snapshot.state != DebugState::Paused
                    || pause != self.pause_generation
                    || self.snapshot.selected_frame != Some(frame)
                {
                    return;
                }
                if result.get("exceptionDetails").is_some() {
                    self.note("Chrome could not read a paused scope.");
                    return;
                }
                if let Some(properties) = result.get("result").and_then(Value::as_array) {
                    for property in properties.iter().take(MAX_LOCALS * 16) {
                        if self.snapshot.locals.len() >= MAX_LOCALS {
                            break;
                        }
                        let Some(name) = property.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let accessor =
                            property.get("get").is_some() || property.get("set").is_some();
                        let remote = property.get("value").unwrap_or(&Value::Null);
                        self.snapshot.locals.push(DebugLocal {
                            scope: scope.clone(),
                            name: bounded(name, MAX_TEXT),
                            value: if property.get("wasThrown").and_then(Value::as_bool)
                                == Some(true)
                            {
                                "<unavailable>".into()
                            } else if accessor {
                                "<accessor; not invoked>".into()
                            } else {
                                remote_summary(remote)
                            },
                            kind: bounded(
                                remote
                                    .get("type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("accessor"),
                                80,
                            ),
                            accessor,
                        });
                    }
                }
            }
            Pending::Watch {
                pause,
                watch_gen,
                frame,
                index,
            } => {
                if self.snapshot.state != DebugState::Paused
                    || pause != self.pause_generation
                    || watch_gen != self.watch_generation
                    || self.snapshot.selected_frame != Some(frame)
                {
                    return;
                }
                let Some(slot) = self.watch_results.get_mut(index) else {
                    return;
                };
                if let Some(details) = result.get("exceptionDetails") {
                    slot.value = format_watch_exception(details, result.get("result"));
                    slot.is_error = true;
                    return;
                }
                let remote = result.get("result").unwrap_or(&Value::Null);
                if remote.is_null() {
                    slot.value = "<unavailable>".into();
                    slot.is_error = true;
                } else {
                    slot.value = remote_summary(remote);
                    slot.is_error = false;
                }
            }
            Pending::Remove => {}
        }
    }

    fn script_parsed(&mut self, params: &Value) {
        let Some(id) = id_string(params.get("scriptId")) else {
            return;
        };
        let Some(url) = params.get("url").and_then(Value::as_str) else {
            return;
        };
        let Some(source) = self
            .sources
            .iter()
            .position(|source| source.source_url == url && !source.locations.is_empty())
        else {
            return;
        };
        if self.scripts.contains_key(&id) {
            return;
        }
        if self.scripts.len() >= MAX_SCRIPTS {
            self.note("Too many generated scripts; later copies are not mapped.");
            return;
        }
        self.scripts.insert(
            id.clone(),
            ParsedScript {
                source,
                offset: None,
                rejected: false,
            },
        );
        if !self.queue(
            "Debugger.getScriptSource",
            json!({"scriptId":id}),
            Pending::Source(id.clone()),
            true,
        ) && let Some(script) = self.scripts.get_mut(&id)
        {
            script.rejected = true;
        }
    }

    fn host_blackbox_pattern(&self) -> String {
        let allowed = self
            .sources
            .iter()
            .filter(|source| !source.locations.is_empty())
            .take(MAX_SCRIPTS)
            .map(|source| {
                source
                    .source_url
                    .chars()
                    .flat_map(|character| {
                        let escape = ".*+?^${}()|[]\\".contains(character);
                        escape
                            .then_some('\\')
                            .into_iter()
                            .chain(std::iter::once(character))
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        if allowed.is_empty() {
            ".*".into()
        } else {
            format!("^(?!(?:{})$).*", allowed.join("|"))
        }
    }

    fn blackbox_script(&mut self, id: &str) {
        let Some(parsed) = self.scripts.get(id) else {
            return;
        };
        let Some((offset_line, offset_column)) = parsed.offset else {
            return;
        };
        let mut lines = BTreeMap::<usize, usize>::new();
        for location in &self.sources[parsed.source].locations {
            let line = location.generated_line.saturating_sub(1) + offset_line;
            let column = location.generated_column.saturating_sub(1)
                + if location.generated_line == 1 {
                    offset_column
                } else {
                    0
                };
            lines
                .entry(line)
                .and_modify(|first| *first = (*first).min(column))
                .or_insert(column);
        }
        let Some((&first_line, &first_column)) = lines.first_key_value() else {
            return;
        };
        let mut positions = vec![json!({"lineNumber":0,"columnNumber":0})];
        if lines.len() > 4_096 {
            // Bound protocol traffic while preserving every later authored statement.
            positions.push(json!({"lineNumber":first_line,"columnNumber":first_column}));
            self.note("Large script: stepping skips the runtime prelude; later generated helper ranges remain visible.");
        } else {
            let mut ranges: Vec<((usize, usize), (usize, usize))> = Vec::new();
            for (line, column) in lines {
                let start = (line, column);
                let end = (line + 1, 0);
                if let Some((_, previous_end)) = ranges.last_mut()
                    && *previous_end == start
                {
                    *previous_end = end;
                    continue;
                }
                ranges.push((start, end));
            }
            for (start, end) in ranges {
                positions.push(json!({"lineNumber":start.0,"columnNumber":start.1}));
                positions.push(json!({"lineNumber":end.0,"columnNumber":end.1}));
            }
        }
        // A statement at the script origin leaves the first interval unblackboxed.
        if positions.len() > 1 && positions[0] == positions[1] {
            positions.drain(..2);
        }
        self.queue(
            "Debugger.setBlackboxedRanges",
            json!({"scriptId":id,"positions":positions}),
            Pending::BlackboxScript,
            true,
        );
    }

    fn install(&mut self, id: &str) {
        let Some(parsed) = self.scripts.get(id) else {
            return;
        };
        let Some((line_offset, column_offset)) = parsed.offset else {
            return;
        };
        let source = &self.sources[parsed.source];
        let mut targets = Vec::new();
        for requested in &self.desired {
            let mut lines = BTreeMap::new();
            for location in &source.locations {
                if location.source == requested.source
                    && location.line == requested.line
                    && requested
                        .column
                        .is_none_or(|column| location.column >= column)
                {
                    lines
                        .entry(location.generated_line)
                        .or_insert(location.generated_column);
                }
            }
            targets.extend(
                lines
                    .into_iter()
                    .take(16)
                    .map(|(line, column)| (line, column, Some(requested.clone()))),
            );
        }
        if self.pause_on_start
            && !self.first_pause
            && let Some(location) = source
                .locations
                .iter()
                .min_by_key(|location| (location.generated_line, location.generated_column))
        {
            targets.push((location.generated_line, location.generated_column, None));
        }
        for (line, column, requested) in targets.into_iter().take(MAX_BREAKPOINTS) {
            let generated_line = line.saturating_sub(1) + line_offset;
            let generated_column =
                column.saturating_sub(1) + if line == 1 { column_offset } else { 0 };
            if !self.installed.insert((
                id.to_owned(),
                generated_line,
                generated_column,
                self.breakpoint_generation,
            )) {
                continue;
            }
            self.queue("Debugger.setBreakpoint", json!({"location":{"scriptId":id,"lineNumber":generated_line,"columnNumber":generated_column}}),
                Pending::Breakpoint { script: id.into(), request: requested, generation: self.breakpoint_generation }, true);
        }
    }

    fn paused(&mut self, params: &Value) {
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("paused");
        let frames = params
            .get("callFrames")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if reason == "instrumentation" {
            let id = params
                .get("data")
                .and_then(|value| value.get("scriptId"))
                .and_then(Value::as_str)
                .or_else(|| {
                    frames
                        .first()
                        .and_then(|frame| frame.get("location"))
                        .and_then(|location| location.get("scriptId"))
                        .and_then(Value::as_str)
                });
            self.gate = Some(id.unwrap_or("").to_owned());
            self.finish_gate();
            return;
        }
        self.clear_pause();
        self.first_pause = true;
        self.snapshot.state = DebugState::Paused;
        self.snapshot.command_pending = false;
        self.snapshot.reason = bounded(reason, 160);
        for frame in frames.iter().take(MAX_FRAMES) {
            let location = frame.get("location").unwrap_or(&Value::Null);
            let id = location
                .get("scriptId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let url = self
                .scripts
                .get(id)
                .map(|parsed| self.sources[parsed.source].source_url.as_str())
                .unwrap_or_else(|| frame.get("url").and_then(Value::as_str).unwrap_or(""));
            self.snapshot.frames.push(DebugFrame {
                function_name: bounded(
                    frame
                        .get("functionName")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    256,
                ),
                location: self.map_location(location),
                generated_url: bounded(url, MAX_TEXT),
                generated_line: number(location.get("lineNumber")) + 1,
                generated_column: number(location.get("columnNumber")) + 1,
            });
            let scopes = frame
                .get("scopeChain")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|scope| {
                    let kind = scope.get("type")?.as_str()?;
                    if !matches!(kind, "local" | "closure" | "catch" | "block" | "script") {
                        return None;
                    }
                    let object = scope.get("object")?;
                    if object.get("subtype").and_then(Value::as_str) == Some("proxy") {
                        return None;
                    }
                    Some(Scope {
                        name: bounded(kind, 80),
                        object_id: id_string(object.get("objectId"))?,
                    })
                })
                .take(MAX_SCOPES)
                .collect();
            self.call_frame_ids
                .push(id_string(frame.get("callFrameId")).unwrap_or_default());
            self.scopes.push(scopes);
        }
        if reason == "step"
            && self
                .snapshot
                .frames
                .first()
                .is_none_or(|frame| frame.location.is_none())
            && let Some(remaining) = self.generated_steps
        {
            if remaining > 0 {
                self.generated_steps = Some(remaining - 1);
                // V8 blackboxes whole helper functions, but can still stop in the unmapped
                // parameter prologue of an authored function. Continue real VM stepping,
                // bounded, without assigning that generated instruction a false source line.
                if self.queue("Debugger.stepInto", json!({}), Pending::Control, true) {
                    self.snapshot.command_pending = true;
                    self.snapshot.reason = "Stepping through generated code".into();
                    return;
                }
            } else {
                self.note("Stepping reached the generated-code limit; the VM remains paused at its actual generated location.");
            }
        }
        self.generated_steps = None;
        let selected = self
            .snapshot
            .frames
            .iter()
            .position(|frame| frame.location.is_some())
            .or_else(|| (!self.snapshot.frames.is_empty()).then_some(0));
        self.snapshot.selected_frame = selected;
        if let Some(index) = selected {
            self.request_locals(index);
            self.request_watches(index);
        }
        let temporary: Vec<_> = self
            .breakpoint_ids
            .iter()
            .filter(|(_, (_, request))| request.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for id in temporary {
            self.breakpoint_ids.remove(&id);
            self.queue(
                "Debugger.removeBreakpoint",
                json!({"breakpointId":id}),
                Pending::Remove,
                true,
            );
        }
    }

    fn finish_gate(&mut self) {
        let Some(id) = self.gate.as_ref() else {
            return;
        };
        if self
            .scripts
            .get(id)
            .is_some_and(|script| !script.rejected && script.offset.is_none())
        {
            return;
        }
        if self
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::Breakpoint {script,..} if script == id))
        {
            return;
        }
        self.gate = None;
        self.snapshot.command_pending = true;
        self.queue("Debugger.resume", json!({}), Pending::GateResume, true);
    }

    fn map_location(&self, location: &Value) -> Option<AuthoredLocation> {
        let parsed = self.scripts.get(location.get("scriptId")?.as_str()?)?;
        let (offset_line, offset_column) = parsed.offset?;
        let line = number(location.get("lineNumber")).checked_sub(offset_line)? + 1;
        let column = number(location.get("columnNumber")).checked_sub(if line == 1 {
            offset_column
        } else {
            0
        })? + 1;
        self.sources[parsed.source]
            .locations
            .iter()
            .filter(|mapping| mapping.generated_line == line && mapping.generated_column <= column)
            .max_by_key(|mapping| mapping.generated_column)
            .map(|mapping| AuthoredLocation {
                source: mapping.source.clone(),
                line: mapping.line,
                column: mapping.column,
            })
    }

    fn request_locals(&mut self, frame: usize) {
        self.pause_generation += 1;
        self.snapshot.locals.clear();
        let scopes = self.scopes.get(frame).cloned().unwrap_or_default();
        for scope in scopes {
            self.queue("Runtime.getProperties", json!({"objectId":scope.object_id,"ownProperties":true,"accessorPropertiesOnly":false,"generatePreview":true}),
                Pending::Properties { pause:self.pause_generation, frame, scope:scope.name }, true);
        }
    }

    fn request_watches(&mut self, frame: usize) {
        self.watch_generation = self.watch_generation.saturating_add(1);
        if self.watches.is_empty() {
            self.watch_results.clear();
            return;
        }
        let call_frame_id = self.call_frame_ids.get(frame).cloned().unwrap_or_default();
        self.watch_results = self
            .watches
            .iter()
            .map(|watch| DebugWatchResult {
                expression: watch.expression.clone(),
                value: if call_frame_id.is_empty() {
                    "<unavailable>".into()
                } else {
                    "…".into()
                },
                is_error: call_frame_id.is_empty(),
            })
            .collect();
        if call_frame_id.is_empty() {
            return;
        }
        let pause = self.pause_generation;
        let watch_gen = self.watch_generation;
        let requests: Vec<(usize, String)> = self
            .watches
            .iter()
            .enumerate()
            .map(|(index, watch)| (index, watch.js_expression.clone()))
            .collect();
        for (index, expression) in requests {
            if !self.queue(
                "Debugger.evaluateOnCallFrame",
                json!({
                    "callFrameId": call_frame_id,
                    "expression": expression,
                    "silent": true,
                    "returnByValue": false,
                    "generatePreview": true
                }),
                Pending::Watch {
                    pause,
                    watch_gen,
                    frame,
                    index,
                },
                true,
            ) && let Some(slot) = self.watch_results.get_mut(index)
            {
                slot.value = "<unavailable>".into();
                slot.is_error = true;
            }
        }
    }

    fn replace_watches(&mut self, watches: Vec<WatchRequest>) {
        self.watches = watches
            .into_iter()
            .filter_map(|watch| {
                let expression = bounded(watch.expression.trim(), MAX_TEXT);
                let js_expression = watch.js_expression.trim().to_owned();
                (!expression.is_empty()
                    && !js_expression.is_empty()
                    && js_expression.len() <= 16_384)
                    .then_some(WatchRequest {
                        expression,
                        js_expression,
                    })
            })
            .take(MAX_WATCHES)
            .collect();
        self.refresh_watches();
    }

    fn refresh_watches(&mut self) {
        self.watch_generation = self.watch_generation.saturating_add(1);
        self.watch_results = self
            .watches
            .iter()
            .map(|watch| DebugWatchResult {
                expression: watch.expression.clone(),
                value: "<not paused>".into(),
                is_error: false,
            })
            .collect();
    }

    fn replace_desired(&mut self, mut points: Vec<AuthoredBreakpoint>) {
        points.retain(|point| {
            point.line > 0 && point.source.len() <= 4_096 && point.column != Some(0)
        });
        points.sort();
        points.dedup();
        points.truncate(MAX_BREAKPOINTS);
        self.desired = points;
        self.refresh_breakpoints();
    }
    fn refresh_breakpoints(&mut self) {
        self.snapshot.breakpoints = self
            .desired
            .iter()
            .map(|requested| BreakpointStatus {
                requested: requested.clone(),
                resolved: Vec::new(),
            })
            .collect();
    }
    fn clear_pause(&mut self) {
        self.pause_generation += 1;
        self.gate = None;
        self.snapshot.frames.clear();
        self.snapshot.locals.clear();
        self.scopes.clear();
        self.call_frame_ids.clear();
        self.snapshot.selected_frame = None;
        self.refresh_watches();
    }
    fn clear_session(&mut self) {
        self.clear_pause();
        self.pending.clear();
        self.outgoing.clear();
        self.scripts.clear();
        self.breakpoint_ids.clear();
        self.installed.clear();
        self.target = None;
        self.session = None;
        self.snapshot.command_pending = false;
        self.generated_steps = None;
        self.refresh_breakpoints();
    }
    fn fail(&mut self, message: &str) {
        self.clear_session();
        self.snapshot.state = DebugState::Failed;
        self.snapshot.reason = bounded(message, MAX_TEXT);
    }
    fn note(&mut self, message: &str) {
        let text = bounded(message, MAX_TEXT);
        let seq = self.next_event_seq;
        self.next_event_seq = self.next_event_seq.saturating_add(1);
        if self.snapshot.messages.len() == 32 {
            self.snapshot.messages.remove(0);
        }
        if self.snapshot.structured_messages.len() == 32 {
            self.snapshot.structured_messages.remove(0);
        }
        self.snapshot.messages.push(text.clone());
        self.snapshot.structured_messages.push((seq, text));
    }
    fn record_output(
        &mut self,
        kind: DebugOutputKind,
        line: String,
        location: Option<AuthoredLocation>,
    ) {
        if line.is_empty() {
            return;
        }
        let formatted = bounded(&line, MAX_TEXT);
        let seq = self.next_event_seq;
        self.next_event_seq = self.next_event_seq.saturating_add(1);
        if self.snapshot.output_events.len() >= MAX_OUTPUT_EVENTS {
            let overflow = self.snapshot.output_events.len() - MAX_OUTPUT_EVENTS + 1;
            self.snapshot.output_events.drain(..overflow);
        }
        if self.snapshot.structured_output.len() >= MAX_OUTPUT_EVENTS {
            let overflow = self.snapshot.structured_output.len() - MAX_OUTPUT_EVENTS + 1;
            self.snapshot.structured_output.drain(..overflow);
        }
        self.snapshot.output_events.push(formatted.clone());
        self.snapshot.structured_output.push(DebugOutputRecord {
            seq,
            kind,
            formatted,
            location,
        });
    }
    fn mapped_stack_location(&self, container: Option<&Value>) -> Option<AuthoredLocation> {
        let frames = container?
            .get("stackTrace")
            .and_then(|st| st.get("callFrames"))
            .and_then(Value::as_array)?;
        frames.iter().find_map(|frame| self.map_location(frame))
    }
    fn parse_neoflash_location_arg(&self, arg: &Value) -> Option<AuthoredLocation> {
        let raw = arg.get("value").and_then(Value::as_str)?;
        let (before_col, col_str) = raw.rsplit_once(':')?;
        let (src_name, line_str) = before_col.rsplit_once(':')?;
        let line: usize = line_str.parse().ok().filter(|&v| v > 0)?;
        let column: usize = col_str.parse().ok().filter(|&v| v > 0)?;
        let known_source = self
            .sources
            .iter()
            .flat_map(|u| &u.locations)
            .any(|l| l.source == src_name);
        known_source.then(|| AuthoredLocation {
            source: src_name.to_owned(),
            line,
            column,
        })
    }
    fn console_api_called(&mut self, params: &Value) {
        let kind = params.get("type").and_then(Value::as_str).unwrap_or("log");
        let args = params
            .get("args")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut loc = self.mapped_stack_location(Some(params));
        let start = if args.len() > 1
            && args[0].get("value").and_then(Value::as_str) == Some("[NeoFlash]")
        {
            1
        } else {
            0
        };
        let trailing_loc = if start == 1 && args.len() > 2 {
            args.last()
                .and_then(|v| self.parse_neoflash_location_arg(v))
        } else {
            None
        };
        let end = if trailing_loc.is_some() {
            args.len() - 1
        } else {
            args.len()
        };
        if loc.is_none() {
            loc = trailing_loc;
        }
        let parts: Vec<String> = args[start..end]
            .iter()
            .map(console_arg_text)
            .filter(|s| !s.is_empty())
            .collect();
        let text = parts.join(" ");
        if text.is_empty() {
            return;
        }
        let (out_kind, formatted) = match kind {
            "error" | "assert" => (
                DebugOutputKind::Error,
                match &loc {
                    Some(loc) => {
                        format!("Runtime error ({} · line {}): {text}", loc.source, loc.line)
                    }
                    None => format!("Runtime error: {text}"),
                },
            ),
            "warning" | "warn" => (
                DebugOutputKind::Warning,
                match &loc {
                    Some(loc) => format!("Warning ({} · line {}): {text}", loc.source, loc.line),
                    None => format!("Warning: {text}"),
                },
            ),
            _ => (DebugOutputKind::Trace, text),
        };
        self.record_output(out_kind, formatted, loc);
    }
    fn exception_thrown(&mut self, params: &Value) {
        let Some(details) = params.get("exceptionDetails") else {
            return;
        };
        let loc = self
            .mapped_stack_location(Some(details))
            .or_else(|| self.map_location(details));
        let message = details
            .get("exception")
            .and_then(|ex| ex.get("description"))
            .and_then(Value::as_str)
            .and_then(|desc| desc.lines().next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| bounded(s, MAX_TEXT))
            .or_else(|| {
                details
                    .get("exception")
                    .map(console_arg_text)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                details
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|s| bounded(s, MAX_TEXT))
            })
            .unwrap_or_else(|| "Uncaught exception".to_owned());
        let formatted = match &loc {
            Some(loc) => format!(
                "Runtime error ({} · line {}): {message}",
                loc.source, loc.line
            ),
            None => format!("Runtime error: {message}"),
        };
        self.record_output(DebugOutputKind::Error, formatted, loc);
    }
    fn request_error(&mut self, pending: Pending, message: &str) {
        match pending {
            Pending::Create
            | Pending::Attach
            | Pending::Enable
            | Pending::Instrument
            | Pending::Navigate => self.fail(message),
            Pending::BlackboxPatterns => {
                self.note("Chrome could not hide host code while stepping; generated frames may be visible.");
                self.queue(
                    "Debugger.setInstrumentationBreakpoint",
                    json!({"instrumentation":"beforeScriptExecution"}),
                    Pending::Instrument,
                    true,
                );
            }
            Pending::Source(id) => {
                if let Some(script) = self.scripts.get_mut(&id) {
                    script.rejected = true;
                }
                self.note(message);
            }
            Pending::Control | Pending::GateResume => {
                self.snapshot.command_pending = false;
                self.generated_steps = None;
                self.note(message);
            }
            Pending::Watch {
                pause,
                watch_gen,
                frame,
                index,
            } => {
                if self.snapshot.state == DebugState::Paused
                    && pause == self.pause_generation
                    && watch_gen == self.watch_generation
                    && self.snapshot.selected_frame == Some(frame)
                    && let Some(slot) = self.watch_results.get_mut(index)
                {
                    let trimmed = message.trim();
                    slot.value = if trimmed.is_empty() {
                        "<unavailable>".into()
                    } else {
                        format!("<{}>", bounded(trimmed, 240))
                    };
                    slot.is_error = true;
                }
            }
            _ => self.note(message),
        }
    }
}

fn format_watch_exception(details: &Value, remote: Option<&Value>) -> String {
    if let Some(desc) = details
        .get("exception")
        .and_then(|ex| ex.get("description"))
        .and_then(Value::as_str)
        .or_else(|| {
            remote
                .and_then(|r| r.get("description"))
                .and_then(Value::as_str)
        })
        .and_then(|d| d.lines().next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("<{}>", bounded(desc, 240));
    }
    if let Some(ex) = details.get("exception")
        && ex.get("value").is_some()
    {
        return format!("<Uncaught {}>", bounded(&remote_summary(ex), 220));
    }
    if let Some(text) = details
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("<{}>", bounded(text, 240));
    }
    "<unavailable>".into()
}

/// Build a `WatchRequest` carrying both the authored expression and its CDP `callFrameId` JS form.
pub(crate) fn compile_watch_expression(expression: &str, language: ScriptLanguage) -> WatchRequest {
    let authored = expression.trim().to_owned();
    let js_expression =
        match language {
            ScriptLanguage::JavaScript | ScriptLanguage::TypeScript => authored.clone(),
            ScriptLanguage::ActionScript1 => translate_actionscript_watch(&authored, false, true)
                .unwrap_or_else(|| authored.clone()),
            ScriptLanguage::ActionScript2 => translate_actionscript_watch(&authored, false, false)
                .unwrap_or_else(|| authored.clone()),
            ScriptLanguage::ActionScript3 => translate_actionscript_watch(&authored, true, false)
                .unwrap_or_else(|| authored.clone()),
        };
    WatchRequest::new(authored, js_expression)
}

#[derive(Clone, Debug, PartialEq)]
enum WatchToken {
    Ident(String),
    Number(String),
    Str(String),
    Punct(&'static str),
}

enum WatchAst {
    Literal(String),
    This,
    Ident(String),
    Member(Box<WatchAst>, String),
    Index(Box<WatchAst>, Box<WatchAst>),
    Call(Box<WatchAst>, Vec<WatchAst>),
    MethodCall(Box<WatchAst>, String, Vec<WatchAst>),
    Unary(&'static str, Box<WatchAst>),
    TypeofIdent(String),
    Binary(&'static str, Box<WatchAst>, Box<WatchAst>),
    TypeCast(&'static str, Box<WatchAst>),
    AsType(Box<WatchAst>, String),
    IsType(Box<WatchAst>, String),
    Ternary(Box<WatchAst>, Box<WatchAst>, Box<WatchAst>),
    Array(Vec<WatchAst>),
    Object(Vec<(String, WatchAst)>),
    Group(Box<WatchAst>),
}

fn lex_watch_tokens(src: &str, allow_diamond_ne: bool) -> Option<Vec<WatchToken>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < src.len() {
        let rest = &src[offset..];
        if let Some(ch) = rest.chars().next()
            && ch.is_whitespace()
        {
            offset += ch.len_utf8();
            continue;
        }
        let ch = rest.chars().next()?;
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let mut end = offset + ch.len_utf8();
            while let Some(next) = src[end..].chars().next() {
                if next.is_alphanumeric() || next == '_' || next == '$' {
                    end += next.len_utf8();
                } else {
                    break;
                }
            }
            out.push(WatchToken::Ident(src[offset..end].to_owned()));
            offset = end;
        } else if ch.is_ascii_digit()
            || (ch == '.' && rest[1..].chars().next().is_some_and(|d| d.is_ascii_digit()))
        {
            let start = offset;
            offset += 1;
            if ch == '0' && src[offset..].starts_with(['x', 'X']) {
                offset += 1;
                let hex_start = offset;
                while src[offset..]
                    .chars()
                    .next()
                    .is_some_and(|d| d.is_ascii_hexdigit())
                {
                    offset += 1;
                }
                if offset == hex_start {
                    return None;
                }
            } else {
                while src[offset..]
                    .chars()
                    .next()
                    .is_some_and(|d| d.is_ascii_digit())
                {
                    offset += 1;
                }
                if ch != '.' && src[offset..].starts_with('.') {
                    offset += 1;
                    while src[offset..]
                        .chars()
                        .next()
                        .is_some_and(|d| d.is_ascii_digit())
                    {
                        offset += 1;
                    }
                }
                if src[offset..].starts_with(['e', 'E']) {
                    offset += 1;
                    if src[offset..].starts_with(['+', '-']) {
                        offset += 1;
                    }
                    let exp_start = offset;
                    while src[offset..]
                        .chars()
                        .next()
                        .is_some_and(|d| d.is_ascii_digit())
                    {
                        offset += 1;
                    }
                    if offset == exp_start {
                        return None;
                    }
                }
            }
            if src[offset..]
                .chars()
                .next()
                .is_some_and(|d| d.is_alphabetic() || d == '_')
            {
                return None;
            }
            out.push(WatchToken::Number(src[start..offset].to_owned()));
        } else if ch == '\'' || ch == '"' {
            let start = offset;
            offset += 1;
            let mut closed = false;
            while let Some(next) = src[offset..].chars().next() {
                offset += next.len_utf8();
                if next == ch {
                    closed = true;
                    break;
                }
                if matches!(next, '\n' | '\r') {
                    return None;
                }
                if next == '\\' {
                    let esc = src[offset..].chars().next()?;
                    offset += esc.len_utf8();
                }
            }
            if !closed {
                return None;
            }
            out.push(WatchToken::Str(src[start..offset].to_owned()));
        } else {
            // Side-effecting assignment / update tokens fall back to V8 syntax/reference reporting.
            const FORBIDDEN: &[&str] = &[
                ">>>=", "<<=", ">>=", "&&=", "||=", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=",
                "|=", "^=",
            ];
            if FORBIDDEN.iter().any(|op| rest.starts_with(op)) {
                return None;
            }
            const OPS: &[&str] = &[
                "===", "!==", ">>>", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "<>", "(",
                ")", "[", "]", "{", "}", ".", ",", "?", ":", "+", "-", "*", "/", "%", "!", "~",
                "&", "|", "^", "<", ">",
            ];
            let op = *OPS
                .iter()
                .find(|&&op| rest.starts_with(op) && (op != "<>" || allow_diamond_ne))?;
            out.push(WatchToken::Punct(op));
            offset += op.len();
        }
        if out.len() > 512 {
            return None;
        }
    }
    Some(out)
}

struct WatchParser {
    tokens: Vec<WatchToken>,
    pos: usize,
    as3: bool,
}

impl WatchParser {
    fn peek(&self) -> Option<&WatchToken> {
        self.tokens.get(self.pos)
    }
    fn eat_punct(&mut self, wanted: &str) -> bool {
        if matches!(self.peek(), Some(WatchToken::Punct(p)) if *p == wanted) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn eat_word(&mut self, wanted: &str) -> bool {
        if matches!(self.peek(), Some(WatchToken::Ident(w)) if w == wanted) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Option<String> {
        match self.peek() {
            Some(WatchToken::Ident(name)) => {
                let n = name.clone();
                self.pos += 1;
                Some(n)
            }
            _ => None,
        }
    }
    fn qualified_type(&mut self) -> Option<String> {
        let mut out = self.ident()?;
        while self.eat_punct(".") {
            out.push('.');
            out.push_str(&self.ident()?);
        }
        Some(out)
    }
    fn parse_expr(&mut self, depth: usize) -> Option<WatchAst> {
        if depth > 48 {
            return None;
        }
        let cond = self.parse_or(depth + 1)?;
        if self.eat_punct("?") {
            let yes = self.parse_expr(depth + 1)?;
            if !self.eat_punct(":") {
                return None;
            }
            let no = self.parse_expr(depth + 1)?;
            Some(WatchAst::Ternary(
                Box::new(cond),
                Box::new(yes),
                Box::new(no),
            ))
        } else {
            Some(cond)
        }
    }
    fn parse_or(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_and(depth + 1)?;
        while self.eat_punct("||") || (!self.as3 && self.eat_word("or")) {
            let rhs = self.parse_and(depth + 1)?;
            lhs = WatchAst::Binary("||", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_and(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_bitor(depth + 1)?;
        while self.eat_punct("&&") || (!self.as3 && self.eat_word("and")) {
            let rhs = self.parse_bitor(depth + 1)?;
            lhs = WatchAst::Binary("&&", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_bitor(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_bitxor(depth + 1)?;
        while self.eat_punct("|") {
            let rhs = self.parse_bitxor(depth + 1)?;
            lhs = WatchAst::Binary("|", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_bitxor(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_bitand(depth + 1)?;
        while self.eat_punct("^") {
            let rhs = self.parse_bitand(depth + 1)?;
            lhs = WatchAst::Binary("^", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_bitand(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_equality(depth + 1)?;
        while self.eat_punct("&") {
            let rhs = self.parse_equality(depth + 1)?;
            lhs = WatchAst::Binary("&", Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_equality(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_relational(depth + 1)?;
        loop {
            let op = if self.eat_punct("===") {
                "==="
            } else if self.eat_punct("!==") {
                "!=="
            } else if self.eat_punct("==") || (!self.as3 && self.eat_word("eq")) {
                "=="
            } else if self.eat_punct("!=")
                || self.eat_punct("<>")
                || (!self.as3 && self.eat_word("ne"))
            {
                "!="
            } else {
                break;
            };
            let rhs = self.parse_relational(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_relational(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_shift(depth + 1)?;
        loop {
            if self.as3 && self.eat_word("as") {
                let ty = self.qualified_type()?;
                lhs = WatchAst::AsType(Box::new(lhs), ty);
                continue;
            }
            if self.as3 && self.eat_word("is") {
                let ty = self.qualified_type()?;
                lhs = WatchAst::IsType(Box::new(lhs), ty);
                continue;
            }
            let op = if self.eat_punct("<=") || (!self.as3 && self.eat_word("le")) {
                "<="
            } else if self.eat_punct(">=") || (!self.as3 && self.eat_word("ge")) {
                ">="
            } else if self.eat_punct("<") || (!self.as3 && self.eat_word("lt")) {
                "<"
            } else if self.eat_punct(">") || (!self.as3 && self.eat_word("gt")) {
                ">"
            } else if self.eat_word("in") {
                "in"
            } else if self.eat_word("instanceof") {
                "instanceof"
            } else {
                break;
            };
            let rhs = self.parse_shift(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_shift(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_additive(depth + 1)?;
        loop {
            let op = if self.eat_punct(">>>") {
                ">>>"
            } else if self.eat_punct("<<") {
                "<<"
            } else if self.eat_punct(">>") {
                ">>"
            } else {
                break;
            };
            let rhs = self.parse_additive(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_additive(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_multiplicative(depth + 1)?;
        loop {
            let op = if self.eat_punct("+") || (!self.as3 && self.eat_word("add")) {
                "+"
            } else if self.eat_punct("-") {
                "-"
            } else {
                break;
            };
            let rhs = self.parse_multiplicative(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_multiplicative(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_unary(depth + 1)?;
        loop {
            let op = if self.eat_punct("*") {
                "*"
            } else if self.eat_punct("/") {
                "/"
            } else if self.eat_punct("%") {
                "%"
            } else {
                break;
            };
            let rhs = self.parse_unary(depth + 1)?;
            lhs = WatchAst::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn parse_unary(&mut self, depth: usize) -> Option<WatchAst> {
        if depth > 64 {
            return None;
        }
        if self.eat_punct("!") || (!self.as3 && self.eat_word("not")) {
            return Some(WatchAst::Unary("!", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_punct("+") {
            return Some(WatchAst::Unary("+", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_punct("-") {
            return Some(WatchAst::Unary("-", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_punct("~") {
            return Some(WatchAst::Unary("~", Box::new(self.parse_unary(depth + 1)?)));
        }
        if self.eat_word("void") {
            return Some(WatchAst::Unary(
                "void ",
                Box::new(self.parse_unary(depth + 1)?),
            ));
        }
        if self.eat_word("typeof") {
            let inner = self.parse_unary(depth + 1)?;
            return Some(match inner {
                WatchAst::Ident(name) => WatchAst::TypeofIdent(name),
                other => WatchAst::Unary("typeof ", Box::new(other)),
            });
        }
        self.parse_postfix(depth + 1)
    }
    fn parse_args(&mut self, depth: usize) -> Option<Vec<WatchAst>> {
        let mut args = Vec::new();
        if !self.eat_punct(")") {
            loop {
                args.push(self.parse_expr(depth + 1)?);
                if self.eat_punct(")") {
                    break;
                }
                if !self.eat_punct(",") {
                    return None;
                }
            }
        }
        Some(args)
    }
    fn parse_postfix(&mut self, depth: usize) -> Option<WatchAst> {
        let mut lhs = self.parse_primary(depth + 1)?;
        loop {
            if self.eat_punct(".") {
                let prop = self.ident()?;
                if self.eat_punct("(") {
                    let args = self.parse_args(depth + 1)?;
                    lhs = WatchAst::MethodCall(Box::new(lhs), prop, args);
                } else {
                    lhs = WatchAst::Member(Box::new(lhs), prop);
                }
            } else if self.eat_punct("[") {
                let idx = self.parse_expr(depth + 1)?;
                if !self.eat_punct("]") {
                    return None;
                }
                lhs = WatchAst::Index(Box::new(lhs), Box::new(idx));
            } else if self.eat_punct("(") {
                let args = self.parse_args(depth + 1)?;
                if let WatchAst::Ident(ref name) = lhs
                    && (name == "int" || (self.as3 && name == "uint"))
                    && args.len() == 1
                {
                    let op = if name == "int" { "| 0" } else { ">>> 0" };
                    let arg = args.into_iter().next().unwrap();
                    lhs = WatchAst::TypeCast(op, Box::new(arg));
                } else {
                    lhs = WatchAst::Call(Box::new(lhs), args);
                }
            } else {
                break;
            }
        }
        Some(lhs)
    }
    fn parse_primary(&mut self, depth: usize) -> Option<WatchAst> {
        if self.eat_punct("(") {
            let inner = self.parse_expr(depth + 1)?;
            if !self.eat_punct(")") {
                return None;
            }
            return Some(WatchAst::Group(Box::new(inner)));
        }
        if self.eat_punct("[") {
            let mut items = Vec::new();
            if !self.eat_punct("]") {
                loop {
                    items.push(self.parse_expr(depth + 1)?);
                    if self.eat_punct("]") {
                        break;
                    }
                    if !self.eat_punct(",") {
                        return None;
                    }
                }
            }
            return Some(WatchAst::Array(items));
        }
        if self.eat_punct("{") {
            let mut entries = Vec::new();
            if !self.eat_punct("}") {
                loop {
                    let key = match self.peek()? {
                        WatchToken::Ident(k) | WatchToken::Str(k) | WatchToken::Number(k) => {
                            let k = k.clone();
                            self.pos += 1;
                            k
                        }
                        _ => return None,
                    };
                    if !self.eat_punct(":") {
                        return None;
                    }
                    let val = self.parse_expr(depth + 1)?;
                    entries.push((key, val));
                    if self.eat_punct("}") {
                        break;
                    }
                    if !self.eat_punct(",") {
                        return None;
                    }
                }
            }
            return Some(WatchAst::Object(entries));
        }
        match self.peek()?.clone() {
            WatchToken::Number(n) | WatchToken::Str(n) => {
                self.pos += 1;
                Some(WatchAst::Literal(n))
            }
            WatchToken::Ident(w) => {
                self.pos += 1;
                match w.as_str() {
                    "true" | "false" | "null" | "undefined" | "NaN" | "Infinity" => {
                        Some(WatchAst::Literal(w))
                    }
                    "this" => Some(WatchAst::This),
                    "break" | "case" | "catch" | "class" | "const" | "continue" | "debugger"
                    | "default" | "delete" | "do" | "else" | "enum" | "export" | "extends"
                    | "finally" | "for" | "function" | "if" | "import" | "let" | "new"
                    | "return" | "super" | "switch" | "throw" | "try" | "var" | "while"
                    | "with" | "yield" | "await" => None,
                    _ => Some(WatchAst::Ident(w)),
                }
            }
            _ => None,
        }
    }
}

fn emit_watch_ast(ast: &WatchAst) -> String {
    match ast {
        WatchAst::Literal(v) => v.clone(),
        WatchAst::This => "__nfWatchThis".into(),
        WatchAst::Ident(name) => {
            format!("__nfWatchId({name:?}, () => typeof {name} !== \"undefined\", () => {name})")
        }
        WatchAst::Member(base, prop) => {
            format!("__nfWatchProp({}, {prop:?})", emit_watch_ast(base))
        }
        WatchAst::Index(base, idx) => format!(
            "__nfWatchProp({}, {})",
            emit_watch_ast(base),
            emit_watch_ast(idx)
        ),
        WatchAst::MethodCall(base, prop, args) => format!(
            "__nfWatchCall({}, {prop:?}, [{}])",
            emit_watch_ast(base),
            args.iter()
                .map(emit_watch_ast)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Call(callee, args) => format!(
            "({})({})",
            emit_watch_ast(callee),
            args.iter()
                .map(emit_watch_ast)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Unary(op, inner) => format!("({op}{})", emit_watch_ast(inner)),
        WatchAst::TypeofIdent(name) => format!(
            "__nfWatchTypeof({name:?}, () => typeof {name} !== \"undefined\", () => {name})"
        ),
        WatchAst::Binary(op, lhs, rhs) if *op == "in" || *op == "instanceof" => {
            format!("({} {op} {})", emit_watch_ast(lhs), emit_watch_ast(rhs))
        }
        WatchAst::Binary(op, lhs, rhs) => {
            format!("({} {op} {})", emit_watch_ast(lhs), emit_watch_ast(rhs))
        }
        WatchAst::TypeCast(op, inner) => format!("(Number({}) {op})", emit_watch_ast(inner)),
        WatchAst::AsType(inner, ty) => format!("__nfWatchAs({}, {ty:?})", emit_watch_ast(inner)),
        WatchAst::IsType(inner, ty) => format!("__nfWatchIs({}, {ty:?})", emit_watch_ast(inner)),
        WatchAst::Ternary(c, y, n) => format!(
            "({} ? {} : {})",
            emit_watch_ast(c),
            emit_watch_ast(y),
            emit_watch_ast(n)
        ),
        WatchAst::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(emit_watch_ast)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Object(entries) => format!(
            "({{{}}})",
            entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", emit_watch_ast(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WatchAst::Group(inner) => format!("({})", emit_watch_ast(inner)),
    }
}

fn translate_actionscript_watch(expr: &str, as3: bool, legacy_case: bool) -> Option<String> {
    if expr.is_empty() || expr.len() > 2_048 {
        return None;
    }
    let tokens = lex_watch_tokens(expr, !as3)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = WatchParser {
        tokens,
        pos: 0,
        as3,
    };
    let ast = parser.parse_expr(0)?;
    if parser.pos != parser.tokens.len() {
        return None;
    }
    let body = emit_watch_ast(&ast);
    Some(format!(
        "(() => {{ \
const __nfWAs3 = {as3}, __nfWLegacy = {legacy_case}; \
const __nfWatchThis = (typeof __nfFrame !== \"undefined\" && __nfFrame !== null) ? __nfFrame : ((typeof this !== \"undefined\" && (typeof globalThis === \"undefined\" || this !== globalThis)) ? this : null); \
const __nfWatchHas = (o, k) => {{ if (o == null || (typeof o !== \"object\" && typeof o !== \"function\")) return false; if (typeof __nfContainerRef === \"function\") {{ try {{ return Boolean(__nfContainerRef(o, k, __nfWAs3, undefined, __nfWLegacy).has()); }} catch (_) {{}} }} return k in o; }}; \
const __nfWatchProp = (o, k) => {{ if (typeof __nfFamilyPropertyValue === \"function\") return __nfFamilyPropertyValue(o, k, __nfWAs3, undefined, __nfWLegacy); return o[k]; }}; \
const __nfWatchCall = (o, k, a) => {{ if (typeof __nfContainerMethod === \"function\") return __nfContainerMethod(o, k, __nfWAs3, undefined, __nfWLegacy)(...a); return o[k](...a); }}; \
const __nfWatchId = (n, hasLex, getLex) => {{ \
const onGlobal = typeof globalThis !== \"undefined\" && (n in globalThis); \
if (hasLex()) {{ const v = getLex(); if (!onGlobal || v !== globalThis[n] || !__nfWatchHas(__nfWatchThis, n)) return v; }} \
if (__nfWatchThis !== null && __nfWatchHas(__nfWatchThis, n)) return __nfWatchProp(__nfWatchThis, n); \
if (!__nfWAs3 && typeof __nfLegacy !== \"undefined\" && __nfLegacy !== null) {{ \
if (/^_level\\d+$/.test(n) && typeof __nfLegacy.level === \"function\") return __nfLegacy.level(Number(n.slice(6))); \
if (typeof __nfLegacy.globalPropRef === \"function\" && (n === \"_quality\" || n === \"_focusrect\" || n === \"_soundbuftime\" || n === \"_highquality\")) return __nfLegacy.globalPropRef(n).get(); \
if (n in __nfLegacy) return __nfLegacy[n]; \
if (__nfLegacy._global && (n in __nfLegacy._global)) return __nfLegacy._global[n]; \
}} \
if (typeof __nfProgram !== \"undefined\" && __nfProgram !== null && typeof __nfProgram.type === \"function\") {{ try {{ const t = __nfProgram.type(n); if (t !== undefined) return t; }} catch (_) {{}} }} \
return getLex(); \
}}; \
const __nfWatchTypeof = (n, hasLex, getLex) => {{ try {{ return typeof __nfWatchId(n, hasLex, getLex); }} catch (_) {{ return \"undefined\"; }} }}; \
const __nfWatchIs = (v, t) => {{ if (typeof __nfCatchType === \"function\" && (t === \"Number\" || t === \"String\" || t === \"Boolean\" || t === \"int\" || t === \"uint\" || t === \"Object\" || t === \"Array\" || t === \"Function\")) return Boolean(__nfCatchType(v, t)); if (typeof __nfProgram !== \"undefined\" && __nfProgram !== null && typeof __nfProgram.is === \"function\") {{ try {{ return Boolean(__nfProgram.is(v, t)); }} catch (_) {{}} }} return false; }}; \
const __nfWatchAs = (v, t) => (__nfWatchIs(v, t) ? v : null); \
return ({body}); \
}})()"
    ))
}

// Retain the browser's actionable lifecycle explanation without dumping authored data.
fn lifecycle_message(base: &str, params: &Value) -> String {
    let mut message = base.to_owned();
    for field in ["reason", "status", "errorCode"] {
        let value = match params.get(field) {
            Some(Value::String(value)) => bounded(value, MAX_TEXT),
            Some(Value::Number(value)) => value.to_string(),
            _ => continue,
        };
        message.push_str(&format!("; {field}: {value}"));
    }
    if message.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    message
}

fn number(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}
fn id_string(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?;
    (!value.is_empty() && value.len() <= 4_096).then(|| value.to_owned())
}
fn bounded(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}
fn console_arg_text(value: &Value) -> String {
    if let Some(Value::String(text)) = value.get("value") {
        return bounded(text, MAX_TEXT);
    }
    remote_summary_inner(value, true)
}
fn remote_summary(value: &Value) -> String {
    remote_summary_inner(value, false)
}
fn format_cdp_key(name: &str) -> String {
    let valid_ident = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if valid_ident {
        bounded(name, 256)
    } else {
        format!("{:?}", bounded(name, 256))
    }
}
fn format_cdp_property_preview(prop: &Value, depth: usize, multiline: bool) -> (String, bool) {
    if let Some(sub) = prop.get("valuePreview")
        && let Some(nested) = format_cdp_object_preview(sub, depth + 1, multiline)
    {
        return (nested, true);
    }
    let kind = prop.get("type").and_then(Value::as_str).unwrap_or("");
    let subtype = prop.get("subtype").and_then(Value::as_str);
    if kind == "accessor" {
        return ("[Getter]".into(), false);
    }
    if subtype == Some("null") {
        return ("null".into(), false);
    }
    if let Some(val) = prop.get("value").and_then(Value::as_str) {
        if kind == "string" {
            return (format!("{:?}", bounded(val, 256)), false);
        }
        return (bounded(val, 256), false);
    }
    if kind == "undefined" {
        return ("undefined".into(), false);
    }
    let fallback = subtype.unwrap_or(if kind.is_empty() { "object" } else { kind });
    (format!("<{}>", bounded(fallback, 64)), false)
}
fn format_cdp_object_preview(preview: &Value, depth: usize, multiline: bool) -> Option<String> {
    if depth > 6 || !preview.is_object() {
        return None;
    }
    let subtype = preview.get("subtype").and_then(Value::as_str);
    if subtype == Some("proxy") {
        return None;
    }
    let overflow = preview
        .get("overflow")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let desc = preview
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");

    if matches!(subtype, Some("array" | "typedarray")) {
        let props = preview.get("properties").and_then(Value::as_array)?;
        let mut indexed: BTreeMap<usize, (String, bool)> = BTreeMap::new();
        let mut extra: Vec<(String, bool)> = Vec::new();
        for prop in props.iter().take(128) {
            let name = prop.get("name").and_then(Value::as_str).unwrap_or("");
            let (val_s, is_nested) = format_cdp_property_preview(prop, depth, multiline);
            if let Ok(idx) = name.parse::<usize>() {
                indexed.insert(idx, (val_s, is_nested));
            } else if !name.is_empty() {
                extra.push((format!("{}: {}", format_cdp_key(name), val_s), is_nested));
            }
        }
        let declared_len = desc
            .strip_prefix("Array(")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.parse::<usize>().ok());
        let max_idx = indexed.keys().next_back().map(|k| k + 1).unwrap_or(0);
        let len = declared_len.unwrap_or(max_idx).min(max_idx.max(128));
        let mut items: Vec<String> = Vec::new();
        let mut has_nested = false;
        let render_len = if overflow { max_idx } else { len.min(128) };
        for idx in 0..render_len {
            if let Some((val_s, nested)) = indexed.remove(&idx) {
                has_nested |= nested;
                items.push(val_s);
            } else {
                items.push("<empty>".into());
            }
        }
        for (_, (val_s, nested)) in indexed {
            has_nested |= nested;
            items.push(val_s);
        }
        for (pair, nested) in extra {
            has_nested |= nested;
            items.push(pair);
        }
        if overflow || declared_len.is_some_and(|dl| dl > render_len && render_len == 128) {
            items.push("…".into());
        }
        let prefix = if subtype == Some("typedarray") && !desc.is_empty() {
            format!("{} ", bounded(desc, 64))
        } else {
            String::new()
        };
        if items.is_empty() {
            return Some(format!("{prefix}[]"));
        }
        if multiline && (has_nested || items.iter().any(|s| s.contains('\n'))) {
            let child_indent = "  ".repeat(depth + 1);
            let close_indent = "  ".repeat(depth);
            let body = items
                .into_iter()
                .map(|s| format!("{child_indent}{s}"))
                .collect::<Vec<_>>()
                .join(",\n");
            return Some(bounded(
                &format!("{prefix}[\n{body}\n{close_indent}]"),
                MAX_TEXT,
            ));
        }
        return Some(bounded(
            &format!("{prefix}[{}]", items.join(", ")),
            MAX_TEXT,
        ));
    }

    if matches!(subtype, Some("map" | "set")) {
        let entries = preview.get("entries").and_then(Value::as_array)?;
        let mut items = Vec::new();
        for entry in entries.iter().take(64) {
            let val_s = entry
                .get("value")
                .and_then(|v| format_cdp_object_preview(v, depth + 1, false))
                .or_else(|| {
                    entry
                        .get("value")
                        .map(|v| format_cdp_property_preview(v, depth, false).0)
                })
                .unwrap_or_else(|| "undefined".into());
            if subtype == Some("map") {
                let key_s = entry
                    .get("key")
                    .and_then(|k| format_cdp_object_preview(k, depth + 1, false))
                    .or_else(|| {
                        entry
                            .get("key")
                            .map(|k| format_cdp_property_preview(k, depth, false).0)
                    })
                    .unwrap_or_else(|| "undefined".into());
                items.push(format!("{key_s} => {val_s}"));
            } else {
                items.push(val_s);
            }
        }
        if overflow {
            items.push("…".into());
        }
        let label = if !desc.is_empty() {
            bounded(desc, 64)
        } else if subtype == Some("map") {
            "Map".into()
        } else {
            "Set".into()
        };
        return Some(if items.is_empty() {
            format!("{label} {{}}")
        } else {
            bounded(&format!("{label} {{ {} }}", items.join(", ")), MAX_TEXT)
        });
    }

    if matches!(subtype, Some("date" | "regexp" | "error")) && !desc.is_empty() {
        return Some(bounded(desc, MAX_TEXT));
    }

    let props = preview.get("properties").and_then(Value::as_array)?;
    let mut items = Vec::new();
    let mut has_nested = false;
    for prop in props.iter().take(64) {
        let Some(name) = prop.get("name").and_then(Value::as_str) else {
            continue;
        };
        let (val_s, nested) = format_cdp_property_preview(prop, depth, multiline);
        has_nested |= nested;
        items.push(format!("{}: {}", format_cdp_key(name), val_s));
    }
    if overflow {
        items.push("…".into());
    }
    let prefix = if !desc.is_empty() && desc != "Object" && !desc.starts_with('[') {
        format!("{} ", bounded(desc, 64))
    } else {
        String::new()
    };
    if items.is_empty() {
        return Some(format!("{prefix}{{}}"));
    }
    if multiline && (has_nested || items.iter().any(|s| s.contains('\n'))) {
        let child_indent = "  ".repeat(depth + 1);
        let close_indent = "  ".repeat(depth);
        let body = items
            .into_iter()
            .map(|s| format!("{child_indent}{s}"))
            .collect::<Vec<_>>()
            .join(",\n");
        return Some(bounded(
            &format!("{prefix}{{\n{body}\n{close_indent}}}"),
            MAX_TEXT,
        ));
    }
    Some(bounded(
        &format!("{prefix}{{ {} }}", items.join(", ")),
        MAX_TEXT,
    ))
}
fn remote_summary_inner(value: &Value, multiline: bool) -> String {
    if let Some(value) = value.get("value") {
        return match value {
            Value::String(value) => format!("{:?}", bounded(value, MAX_TEXT)),
            Value::Bool(_) | Value::Number(_) | Value::Null => value.to_string(),
            _ => "<object>".into(),
        };
    }
    if let Some(value) = value.get("unserializableValue").and_then(Value::as_str) {
        return bounded(value, MAX_TEXT);
    }
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    if kind == "undefined" {
        return "undefined".into();
    }
    let subtype = value.get("subtype").and_then(Value::as_str);
    if subtype != Some("proxy")
        && let Some(preview) = value.get("preview")
        && let Some(formatted) = format_cdp_object_preview(preview, 0, multiline)
    {
        return formatted;
    }
    format!(
        "<{}>",
        bounded(
            value
                .get("className")
                .or_else(|| value.get("subtype"))
                .and_then(Value::as_str)
                .unwrap_or(kind),
            128
        )
    )
}

/// Verify the full known generated body inside Chrome's Function wrapper. A matching sourceURL
/// alone is insufficient: authored strings and unrelated scripts must not gain forged locations.
fn wrapper_offset(source: &DebugSource, script: &str) -> Option<(usize, usize)> {
    if source.code.is_empty() || script.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let index = script.find(&source.code)?;
    if script[index + source.code.len()..].contains(&source.code) {
        return None;
    }
    let prefix = &script[..index];
    let suffix = &script[index + source.code.len()..];
    let function = prefix
        .trim_start_matches('(')
        .strip_prefix("function anonymous(")?;
    let (parameters, body_prefix) = function.split_once(") {\n")?;
    if !parameters
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ',' || c.is_ascii_whitespace())
    {
        return None;
    }
    let (wanted_prefix, wanted_suffix) = if source.source_url == "neoflash-project.js" {
        ("", format!("\n//# sourceURL={}\n}}", source.source_url))
    } else {
        (
            "\"use strict\";\nreturn (function () {\n",
            format!("\n}}).call(this);\n//# sourceURL={}\n}}", source.source_url),
        )
    };
    if body_prefix != wanted_prefix || suffix.trim_end_matches(')').trim_end() != wanted_suffix {
        return None;
    }
    Some((
        prefix.bytes().filter(|c| *c == b'\n').count(),
        prefix.rsplit('\n').next()?.encode_utf16().count(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> DebugSource {
        DebugSource {
            source_url: "neoflash-script-0.js".into(),
            code: "let count = 1;\ncount++;\ntrace(count);".into(),
            locations: (1..=3)
                .map(|line| RuntimeSourceLocation {
                    generated_line: line,
                    generated_column: 1,
                    source: "Scene / frame 1".into(),
                    line: line + 9,
                    column: 1,
                })
                .collect(),
        }
    }
    fn point(line: usize) -> AuthoredBreakpoint {
        AuthoredBreakpoint {
            source: "Scene / frame 1".into(),
            line,
            column: None,
        }
    }
    fn wrapped(source: &DebugSource) -> String {
        format!(
            "(function anonymous(event,trace\n) {{\n\"use strict\";\nreturn (function () {{\n{}\n}}).call(this);\n//# sourceURL={}\n}})",
            source.code, source.source_url
        )
    }
    fn reply(protocol: &mut DebugProtocol, command: &Value, result: Value) {
        protocol.receive(
            protocol.epoch(),
            json!({"id":command["id"],"result":result}),
        );
    }
    fn event(protocol: &mut DebugProtocol, method: &str, params: Value) {
        protocol.receive(
            protocol.epoch(),
            json!({"method":method,"params":params,"sessionId":"owned-session"}),
        );
    }
    fn connect() -> DebugProtocol {
        let mut protocol = DebugProtocol::new(
            "file:///private/preview/index.html".into(),
            vec![source()],
            vec![point(11)],
        );
        protocol.start();
        let command = protocol.drain_commands().pop().unwrap();
        assert_eq!(command["method"], "Target.createTarget");
        assert_eq!(command["params"]["url"], "about:blank");
        reply(&mut protocol, &command, json!({"targetId":"owned-page"}));
        let command = protocol.drain_commands().pop().unwrap();
        assert_eq!(
            command["params"],
            json!({"targetId":"owned-page","flatten":true})
        );
        reply(
            &mut protocol,
            &command,
            json!({"sessionId":"owned-session"}),
        );
        let commands = protocol.drain_commands();
        assert_eq!(commands.len(), 3);
        for command in commands {
            assert_eq!(command["sessionId"], "owned-session");
            reply(&mut protocol, &command, json!({}));
        }
        let command = protocol.drain_commands().pop().unwrap();
        assert_eq!(command["method"], "Debugger.setBlackboxPatterns");
        assert_eq!(
            command["params"]["patterns"],
            json!(["^(?!(?:neoflash-script-0\\.js)$).*"])
        );
        reply(&mut protocol, &command, json!({}));
        let command = protocol.drain_commands().pop().unwrap();
        assert_eq!(command["method"], "Debugger.setInstrumentationBreakpoint");
        reply(
            &mut protocol,
            &command,
            json!({"breakpointId":"instrument"}),
        );
        let command = protocol.drain_commands().pop().unwrap();
        assert_eq!(command["method"], "Page.navigate");
        reply(&mut protocol, &command, json!({"frameId":"page"}));
        assert_eq!(protocol.snapshot().state, DebugState::Running);
        protocol
    }
    fn parsed(protocol: &mut DebugProtocol) -> Value {
        event(
            protocol,
            "Debugger.scriptParsed",
            json!({"scriptId":"script-1","url":"neoflash-script-0.js","startLine":0,"startColumn":0}),
        );
        protocol.drain_commands().pop().unwrap()
    }
    fn paused_frame(line: usize) -> Value {
        json!({"callFrames":[{"callFrameId":"frame-1","functionName":"authored", "location":{"scriptId":"script-1","lineNumber":line,"columnNumber":0},
            "scopeChain":[{"type":"local","object":{"type":"object","objectId":"scope-1"}},{"type":"global","object":{"objectId":"global-1"}}]}],"reason":"other"})
    }

    #[test]
    fn first_frame_breakpoints_wait_for_verified_wrapper_and_real_vm_pause() {
        let mut protocol = connect();
        let request = parsed(&mut protocol);
        event(
            &mut protocol,
            "Debugger.paused",
            json!({"reason":"instrumentation","data":{"scriptId":"script-1"},"callFrames":[]}),
        );
        assert!(
            protocol.drain_commands().is_empty(),
            "do not run authored code before mapping/breakpoints exist"
        );
        assert_ne!(
            protocol.snapshot().state,
            DebugState::Paused,
            "internal gate is not a user pause"
        );
        reply(
            &mut protocol,
            &request,
            json!({"scriptSource":wrapped(&source())}),
        );
        let commands = protocol.drain_commands();
        assert_eq!(commands[0]["method"], "Debugger.setBlackboxedRanges");
        assert_eq!(
            commands[0]["params"]["positions"],
            json!([
                {"lineNumber":0,"columnNumber":0},
                {"lineNumber":4,"columnNumber":0},
                {"lineNumber":7,"columnNumber":0}
            ])
        );
        // Chromium may defer setBlackboxedRanges until the instrumentation pause resumes.
        // It must never prevent the already verified authored breakpoint from running.
        let breakpoint = commands.last().unwrap().clone();
        assert_eq!(breakpoint["method"], "Debugger.setBreakpoint");
        assert_eq!(
            breakpoint["params"]["location"],
            json!({"scriptId":"script-1","lineNumber":5,"columnNumber":0})
        );
        reply(
            &mut protocol,
            &breakpoint,
            json!({"breakpointId":"authored-bp","actualLocation":breakpoint["params"]["location"]}),
        );
        let resume = protocol.drain_commands().pop().unwrap();
        assert_eq!(resume["method"], "Debugger.resume");
        reply(&mut protocol, &commands[0], json!({}));
        reply(&mut protocol, &resume, json!({}));
        event(&mut protocol, "Debugger.resumed", json!({}));
        event(&mut protocol, "Debugger.paused", paused_frame(5));
        let snapshot = protocol.snapshot();
        assert_eq!(snapshot.state, DebugState::Paused);
        assert_eq!(snapshot.frames[0].location.as_ref().unwrap().line, 11);
        assert_eq!(snapshot.breakpoints[0].resolved[0].line, 11);
        let locals = protocol.drain_commands();
        assert_eq!(
            locals.len(),
            1,
            "never inspect global object or execute expressions"
        );
        assert_eq!(locals[0]["method"], "Runtime.getProperties");
        assert_eq!(
            locals[0]["params"],
            json!({"objectId":"scope-1","ownProperties":true,"accessorPropertiesOnly":false,"generatePreview":true})
        );
        reply(
            &mut protocol,
            &locals[0],
            json!({"result":[
            {"name":"count","value":{"type":"number","value":1}},
            {"name":"dangerous","get":{"type":"function","objectId":"getter"}},
            {"name":"model","value":{"type":"object","subtype":"proxy","objectId":"model","description":"do not stringify me"}},
            {"name":"large","value":{"type":"string","value":"x".repeat(100_000)}}]}),
        );
        let snapshot = protocol.snapshot();
        assert_eq!(snapshot.locals[0].value, "1");
        assert!(snapshot.locals[1].accessor);
        assert_eq!(snapshot.locals[2].value, "<proxy>");
        assert!(snapshot.locals[3].value.len() < MAX_TEXT * 2);
        assert!(
            protocol.drain_commands().is_empty(),
            "no getter/function/evaluation command is emitted"
        );
    }

    #[test]
    fn commands_restart_and_late_properties_obey_session_and_pause_epochs() {
        let mut protocol = connect();
        let source_request = parsed(&mut protocol);
        reply(
            &mut protocol,
            &source_request,
            json!({"scriptSource":wrapped(&source())}),
        );
        let commands = protocol.drain_commands();
        reply(&mut protocol, &commands[0], json!({}));
        let breakpoint = commands.last().unwrap().clone();
        reply(
            &mut protocol,
            &breakpoint,
            json!({"breakpointId":"bp","actualLocation":breakpoint["params"]["location"]}),
        );
        event(&mut protocol, "Debugger.paused", paused_frame(5));
        let locals = protocol.drain_commands().pop().unwrap();
        protocol.command(DebugCommand::StepOver).unwrap();
        assert!(protocol.command(DebugCommand::Resume).is_err());
        let step = protocol.drain_commands().pop().unwrap();
        assert_eq!(step["method"], "Debugger.stepOver");
        reply(&mut protocol, &step, json!({}));
        assert!(
            protocol.command(DebugCommand::Resume).is_err(),
            "acknowledging a step does not resume the VM"
        );
        reply(
            &mut protocol,
            &locals,
            json!({"result":[{"name":"stale","value":{"type":"number","value":2}}]}),
        );
        assert!(protocol.snapshot().locals.is_empty());
        event(&mut protocol, "Debugger.resumed", json!({}));
        assert!(protocol.snapshot().frames.is_empty());
        let mut generated_pause = paused_frame(3);
        generated_pause["reason"] = json!("step");
        for _ in 0..MAX_GENERATED_STEPS {
            event(&mut protocol, "Debugger.paused", generated_pause.clone());
            let snapshot = protocol.snapshot();
            assert!(
                snapshot.frames[0].location.is_none(),
                "generated prologue must never gain an authored line"
            );
            assert!(snapshot.command_pending);
            let continuation = protocol.drain_commands().pop().unwrap();
            assert_eq!(continuation["method"], "Debugger.stepInto");
            reply(&mut protocol, &continuation, json!({}));
            event(&mut protocol, "Debugger.resumed", json!({}));
        }
        event(&mut protocol, "Debugger.paused", generated_pause);
        assert_eq!(protocol.snapshot().state, DebugState::Paused);
        assert!(!protocol.snapshot().command_pending);
        assert!(protocol.snapshot().frames[0].location.is_none());
        assert!(
            protocol
                .snapshot()
                .messages
                .iter()
                .any(|message| message.contains("generated-code limit"))
        );
        let old_epoch = protocol.epoch();
        protocol.command(DebugCommand::Restart).unwrap();
        let commands = protocol.drain_commands();
        assert_eq!(commands[0]["method"], "Target.closeTarget");
        assert_eq!(commands[0]["params"]["targetId"], "owned-page");
        assert_eq!(commands[1]["method"], "Target.createTarget");
        protocol.receive(old_epoch,json!({"method":"Debugger.paused","sessionId":"owned-session","params":paused_frame(5)}));
        protocol.receive(protocol.epoch(),json!({"method":"Debugger.paused","sessionId":"owned-session","params":paused_frame(5)}));
        reply(&mut protocol, &step, json!({}));
        assert_eq!(protocol.snapshot().state, DebugState::Connecting);
        assert!(protocol.snapshot().frames.is_empty());
        assert_eq!(protocol.snapshot().breakpoints[0].requested, point(11));
        protocol.disconnect("closed");
        assert!(
            protocol
                .command(DebugCommand::SetBreakpoints(vec![point(12)]))
                .is_err()
        );
        assert!(protocol.command(DebugCommand::Pause).is_err());
        protocol.command(DebugCommand::Restart).unwrap();
        assert_eq!(
            protocol.drain_commands()[0]["method"],
            "Target.createTarget"
        );
    }

    #[test]
    fn spoofed_sources_and_slid_breakpoints_never_claim_an_authored_pause() {
        let mut protocol = connect();
        let request = parsed(&mut protocol);
        event(
            &mut protocol,
            "Debugger.paused",
            json!({"reason":"instrumentation","data":{"scriptId":"script-1"}}),
        );
        reply(
            &mut protocol,
            &request,
            json!({"scriptSource":format!("const forged = {:?};\n//# sourceURL=neoflash-script-0.js",source().code)}),
        );
        let commands = protocol.drain_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0]["method"], "Debugger.resume");
        assert!(protocol.snapshot().breakpoints[0].resolved.is_empty());
        let mut protocol = connect();
        let request = parsed(&mut protocol);
        reply(
            &mut protocol,
            &request,
            json!({"scriptSource":wrapped(&source())}),
        );
        let commands = protocol.drain_commands();
        reply(&mut protocol, &commands[0], json!({}));
        let request = commands.last().unwrap().clone();
        reply(
            &mut protocol,
            &request,
            json!({"breakpointId":"wrong-line","actualLocation":{"scriptId":"script-1","lineNumber":6,"columnNumber":0}}),
        );
        let removal = protocol.drain_commands().pop().unwrap();
        assert_eq!(removal["method"], "Debugger.removeBreakpoint");
        assert!(protocol.snapshot().breakpoints[0].resolved.is_empty());
        assert!(!protocol.snapshot().messages.is_empty());
        event(
            &mut protocol,
            "Inspector.detached",
            json!({"reason":"replaced_with_devtools"}),
        );
        assert_eq!(protocol.snapshot().state, DebugState::Disconnected);
        assert!(
            protocol
                .snapshot()
                .reason
                .contains("reason: replaced_with_devtools")
        );
        assert!(!protocol.has_pending_work());
        let mut protocol = connect();
        protocol.receive(
            protocol.epoch(),
            json!({"method":"Target.targetCrashed", "params":{
                "targetId":"owned-page", "status":"crashed", "errorCode":139
            }}),
        );
        assert_eq!(protocol.snapshot().state, DebugState::Disconnected);
        assert!(
            protocol
                .snapshot()
                .reason
                .contains("status: crashed; errorCode: 139")
        );
        let bounded_reason = lifecycle_message("Detached", &json!({"reason":"🦀".repeat(2_000)}));
        assert!(bounded_reason.len() <= MAX_TEXT);
        assert!(bounded_reason.is_char_boundary(bounded_reason.len()));
    }

    #[test]
    fn console_trace_warnings_and_exceptions_record_mapped_output() {
        let mut protocol = connect();
        let request = parsed(&mut protocol);
        reply(
            &mut protocol,
            &request,
            json!({"scriptSource":wrapped(&source())}),
        );
        event(
            &mut protocol,
            "Runtime.consoleAPICalled",
            json!({
                "type": "log",
                "args": [
                    {"type": "string", "value": "[NeoFlash]"},
                    {"type": "string", "value": "score:"},
                    {"type": "number", "value": 42}
                ]
            }),
        );
        event(
            &mut protocol,
            "Runtime.consoleAPICalled",
            json!({
                "type": "warning",
                "args": [{"type": "string", "value": "Frame skipped"}],
                "stackTrace": {"callFrames": [{"scriptId": "script-1", "lineNumber": 5, "columnNumber": 0}]}
            }),
        );
        event(
            &mut protocol,
            "Runtime.exceptionThrown",
            json!({
                "exceptionDetails": {
                    "text": "Uncaught",
                    "exception": {"type": "object", "description": "ReferenceError: hero is not defined\n    at eval"},
                    "stackTrace": {"callFrames": [{"scriptId": "script-1", "lineNumber": 6, "columnNumber": 0}]}
                }
            }),
        );
        assert_eq!(
            protocol.snapshot().output_events,
            vec![
                "score: 42",
                "Warning (Scene / frame 1 · line 11): Frame skipped",
                "Runtime error (Scene / frame 1 · line 12): ReferenceError: hero is not defined",
            ]
        );
    }

    #[test]
    fn t368_debugger_formats_structured_multidimensional_arrays_objects_and_strips_trailing_location()
     {
        let mut protocol = connect();
        let request = parsed(&mut protocol);
        reply(
            &mut protocol,
            &request,
            json!({"scriptSource":wrapped(&source())}),
        );
        let commands = protocol.drain_commands();
        reply(&mut protocol, &commands[0], json!({}));
        let breakpoint = commands.last().unwrap().clone();
        reply(
            &mut protocol,
            &breakpoint,
            json!({"breakpointId":"authored-bp","actualLocation":breakpoint["params"]["location"]}),
        );

        let matrix_preview = json!({
            "type": "object",
            "subtype": "array",
            "description": "Array(2)",
            "overflow": false,
            "properties": [
                {
                    "name": "0",
                    "type": "object",
                    "subtype": "array",
                    "value": "Array(3)",
                    "valuePreview": {
                        "type": "object",
                        "subtype": "array",
                        "description": "Array(3)",
                        "overflow": false,
                        "properties": [
                            {"name": "0", "type": "number", "value": "0"},
                            {"name": "1", "type": "number", "value": "1"},
                            {"name": "2", "type": "number", "value": "0"}
                        ]
                    }
                },
                {
                    "name": "1",
                    "type": "object",
                    "subtype": "array",
                    "value": "Array(3)",
                    "valuePreview": {
                        "type": "object",
                        "subtype": "array",
                        "description": "Array(3)",
                        "overflow": false,
                        "properties": [
                            {"name": "0", "type": "number", "value": "1"},
                            {"name": "1", "type": "number", "value": "1"},
                            {"name": "2", "type": "number", "value": "1"}
                        ]
                    }
                }
            ]
        });

        event(
            &mut protocol,
            "Runtime.consoleAPICalled",
            json!({
                "type": "log",
                "args": [
                    {"type": "string", "value": "[NeoFlash]"},
                    {"type": "object", "subtype": "array", "description": "Array(2)", "preview": matrix_preview},
                    {"type": "string", "value": "Scene / frame 1:12:1"}
                ]
            }),
        );
        event(
            &mut protocol,
            "Runtime.consoleAPICalled",
            json!({
                "type": "log",
                "args": [{
                    "type": "object",
                    "className": "Object",
                    "description": "Object",
                    "preview": {
                        "type": "object",
                        "description": "Object",
                        "overflow": false,
                        "properties": [
                            {"name": "level", "type": "number", "value": "2"},
                            {
                                "name": "grid",
                                "type": "object",
                                "subtype": "array",
                                "value": "Array(2)",
                                "valuePreview": matrix_preview
                            }
                        ]
                    }
                }]
            }),
        );

        assert_eq!(
            protocol.snapshot().output_events,
            vec![
                "[\n  [0, 1, 0],\n  [1, 1, 1]\n]",
                "{\n  level: 2,\n  grid: [\n    [0, 1, 0],\n    [1, 1, 1]\n  ]\n}",
            ]
        );

        event(&mut protocol, "Debugger.paused", paused_frame(5));
        let locals_req = protocol.drain_commands().pop().unwrap();
        reply(
            &mut protocol,
            &locals_req,
            json!({"result": [
                {
                    "name": "figure",
                    "value": {"type": "object", "subtype": "array", "description": "Array(2)", "preview": matrix_preview}
                },
                {
                    "name": "stats",
                    "value": {
                        "type": "object",
                        "subtype": "map",
                        "description": "Map(1)",
                        "preview": {
                            "type": "object",
                            "subtype": "map",
                            "description": "Map(1)",
                            "overflow": false,
                            "properties": [],
                            "entries": [{
                                "key": {"type": "string", "value": "hp"},
                                "value": {"type": "number", "value": "100"}
                            }]
                        }
                    }
                },
                {
                    "name": "hostileProxy",
                    "value": {
                        "type": "object",
                        "subtype": "proxy",
                        "description": "do not stringify me",
                        "preview": matrix_preview
                    }
                }
            ]}),
        );
        let snap = protocol.snapshot();
        assert_eq!(snap.locals[0].value, "[[0, 1, 0], [1, 1, 1]]");
        assert_eq!(snap.locals[1].value, "Map(1) { \"hp\" => 100 }");
        assert_eq!(snap.locals[2].value, "<proxy>");
    }

    #[test]
    fn t404_watch_evaluate_on_call_frame_roundtrip_frame_switch_and_error_formatting() {
        let mut protocol = connect();
        let request = parsed(&mut protocol);
        reply(
            &mut protocol,
            &request,
            json!({"scriptSource": wrapped(&source())}),
        );
        let commands = protocol.drain_commands();
        reply(&mut protocol, &commands[0], json!({}));
        let breakpoint = commands.last().unwrap().clone();
        reply(
            &mut protocol,
            &breakpoint,
            json!({"breakpointId":"authored-bp","actualLocation":breakpoint["params"]["location"]}),
        );

        let w_js = compile_watch_expression("score + 1", ScriptLanguage::JavaScript);
        assert_eq!(w_js.expression, "score + 1");
        assert_eq!(w_js.js_expression, "score + 1");

        let w_as2 =
            compile_watch_expression("hero._x + _root.score", ScriptLanguage::ActionScript2);
        assert_eq!(w_as2.expression, "hero._x + _root.score");
        assert!(w_as2.js_expression.contains("__nfWatchProp"));
        assert!(w_as2.js_expression.contains("\"_x\""));
        assert!(w_as2.js_expression.contains("\"_root\""));

        let w_as3 = compile_watch_expression("items.length", ScriptLanguage::ActionScript3);
        assert!(w_as3.js_expression.contains("__nfWAs3 = true"));
        assert!(w_as3.js_expression.contains("\"length\""));

        // Register watches while Running before pause.
        protocol
            .command(DebugCommand::SetWatches(vec![
                w_js.clone(),
                WatchRequest::new("items", "items"),
                WatchRequest::new("missingVar", "missingVar"),
            ]))
            .unwrap();
        assert_eq!(protocol.watch_results().len(), 3);
        assert_eq!(protocol.watch_results()[0].value, "<not paused>");
        assert!(protocol.drain_commands().is_empty());

        // Pause with two call frames (frame-top & frame-caller).
        event(
            &mut protocol,
            "Debugger.paused",
            json!({
                "reason": "other",
                "callFrames": [
                    {
                        "callFrameId": "frame-top",
                        "functionName": "inner",
                        "location": {"scriptId": "script-1", "lineNumber": 5, "columnNumber": 0},
                        "scopeChain": [{"type": "local", "object": {"type": "object", "objectId": "scope-top"}}]
                    },
                    {
                        "callFrameId": "frame-caller",
                        "functionName": "outer",
                        "location": {"scriptId": "script-1", "lineNumber": 6, "columnNumber": 0},
                        "scopeChain": [{"type": "local", "object": {"type": "object", "objectId": "scope-caller"}}]
                    }
                ]
            }),
        );

        let batch = protocol.drain_commands();
        assert_eq!(batch.len(), 4);
        assert_eq!(batch[0]["method"], "Runtime.getProperties");
        for (idx, expected_expr) in ["score + 1", "items", "missingVar"].iter().enumerate() {
            let cmd = &batch[idx + 1];
            assert_eq!(cmd["method"], "Debugger.evaluateOnCallFrame");
            assert_eq!(cmd["params"]["callFrameId"], "frame-top");
            assert_eq!(cmd["params"]["expression"], *expected_expr);
            assert_eq!(cmd["params"]["silent"], true);
            assert_eq!(cmd["params"]["returnByValue"], false);
            assert_eq!(cmd["params"]["generatePreview"], true);
        }

        reply(&mut protocol, &batch[0], json!({"result": []}));
        reply(
            &mut protocol,
            &batch[1],
            json!({"result": {"type": "number", "value": 43}}),
        );
        reply(
            &mut protocol,
            &batch[2],
            json!({
                "result": {
                    "type": "object",
                    "subtype": "array",
                    "description": "Array(2)",
                    "preview": {
                        "type": "object",
                        "subtype": "array",
                        "description": "Array(2)",
                        "overflow": false,
                        "properties": [
                            {"name": "0", "type": "number", "value": "10"},
                            {"name": "1", "type": "number", "value": "20"}
                        ]
                    }
                }
            }),
        );
        reply(
            &mut protocol,
            &batch[3],
            json!({
                "result": {
                    "type": "object",
                    "subtype": "error",
                    "description": "ReferenceError: missingVar is not defined\n    at eval"
                },
                "exceptionDetails": {
                    "text": "Uncaught",
                    "exception": {
                        "type": "object",
                        "subtype": "error",
                        "description": "ReferenceError: missingVar is not defined\n    at eval"
                    }
                }
            }),
        );

        let watches = protocol.watch_results();
        assert_eq!(watches[0].value, "43");
        assert!(!watches[0].is_error);
        assert_eq!(watches[1].value, "[10, 20]");
        assert!(!watches[1].is_error);
        assert_eq!(
            watches[2].value,
            "<ReferenceError: missingVar is not defined>"
        );
        assert!(watches[2].is_error);

        // SelectFrame(1) re-evaluates all watches on frame-caller and ignores stale replies.
        protocol.command(DebugCommand::SelectFrame(1)).unwrap();
        let frame_switch_cmds = protocol.drain_commands();
        assert_eq!(frame_switch_cmds.len(), 4);
        assert_eq!(
            frame_switch_cmds[1]["method"],
            "Debugger.evaluateOnCallFrame"
        );
        assert_eq!(
            frame_switch_cmds[1]["params"]["callFrameId"],
            "frame-caller"
        );
        reply(
            &mut protocol,
            &frame_switch_cmds[1],
            json!({"result": {"type": "number", "value": 101}}),
        );
        // Simulate protocol error on frame_switch_cmds[2]
        protocol.receive(
            protocol.epoch(),
            json!({
                "id": frame_switch_cmds[2]["id"],
                "error": {"message": "Cannot find context with specified id"}
            }),
        );
        let snap_after = protocol.snapshot();
        let watches_after = protocol.watch_results();
        assert_eq!(snap_after.selected_frame, Some(1));
        assert_eq!(watches_after[0].value, "101");
        assert!(!watches_after[0].is_error);
        assert_eq!(
            watches_after[1].value,
            "<Cannot find context with specified id>"
        );
        assert!(watches_after[1].is_error);

        // EvaluateWatches re-runs evaluation on the active selected frame without clearing locals.
        reply(
            &mut protocol,
            &frame_switch_cmds[0],
            json!({"result": [{"name": "callerLocal", "value": {"type": "number", "value": 9}}]}),
        );
        assert_eq!(protocol.snapshot().locals.len(), 1);
        protocol.command(DebugCommand::EvaluateWatches).unwrap();
        assert_eq!(protocol.snapshot().locals.len(), 1);
        let eval_cmds = protocol.drain_commands();
        assert_eq!(eval_cmds.len(), 3);
        assert_eq!(eval_cmds[0]["params"]["callFrameId"], "frame-caller");
    }

    #[test]
    fn t404_actionscript_watch_js_translation_resolves_locals_timeline_and_root() {
        let as2_x = compile_watch_expression("_x + bonus", ScriptLanguage::ActionScript2);
        let as2_root =
            compile_watch_expression("hero._x + _root.score", ScriptLanguage::ActionScript2);
        let as2_ops =
            compile_watch_expression("bonus <> 0 and bonus >= 5", ScriptLanguage::ActionScript2);
        let as3_cast =
            compile_watch_expression("int(items.length + 2.9)", ScriptLanguage::ActionScript3);
        let as3_missing =
            compile_watch_expression("missingWatchSymbol", ScriptLanguage::ActionScript3);

        if let Ok(output) = std::process::Command::new("node")
            .arg("-e")
            .arg(format!(
                r#"
"use strict";
globalThis.name = "";
const _root = {{ score: 300 }};
const __nfFrame = {{ _x: 40, name: "clip_mc", hero: {{ _x: 120 }}, items: [10, 20, 30] }};
const __nfContainerRef = (o, k) => ({{ has: () => o != null && (k in o), get: () => o[k] }});
const __nfFamilyPropertyValue = (o, k) => o[k];
function pausedCallFrame(bonus) {{
  const r1 = {e1};
  const r2 = {e2};
  const r3 = {e3};
  const r4 = {e4};
  const rName = {e_name};
  let threwRef = false;
  try {{ void ({e_miss}); }} catch (err) {{ threwRef = err instanceof ReferenceError; }}
  console.log(JSON.stringify([r1, r2, r3, r4, rName, threwRef]));
}}
pausedCallFrame(5);
"#,
                e1 = as2_x.js_expression,
                e2 = as2_root.js_expression,
                e3 = as2_ops.js_expression,
                e4 = as3_cast.js_expression,
                e_name =
                    compile_watch_expression("name", ScriptLanguage::ActionScript3).js_expression,
                e_miss = as3_missing.js_expression,
            ))
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(stdout.trim(), r#"[45,420,true,5,"clip_mc",true]"#);
        }
    }
}
