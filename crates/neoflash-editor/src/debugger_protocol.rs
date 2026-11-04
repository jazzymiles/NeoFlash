//! Pure CDP state machine for the editor's privately owned Test Movie target.
//!
//! Protocol references: ChromeDevTools/devtools-protocol json/{js,browser}_protocol.json.
//! No expression evaluation, getter invocation, socket discovery or unrelated-target attachment.
use neoflash_script::RuntimeSourceLocation;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_PENDING: usize = 256;
const MAX_SCRIPTS: usize = 512;
const MAX_BREAKPOINTS: usize = 128;
const MAX_FRAMES: usize = 64;
const MAX_SCOPES: usize = 8;
const MAX_GENERATED_STEPS: usize = 64;
const MAX_LOCALS: usize = 256;
const MAX_TEXT: usize = 1_024;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

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
}

pub(crate) struct DebugProtocol {
    page_url: String,
    sources: Vec<DebugSource>,
    desired: Vec<AuthoredBreakpoint>,
    snapshot: DebugSnapshot,
    target: Option<String>,
    session: Option<String>,
    next_id: u64,
    pending: BTreeMap<u64, Pending>,
    outgoing: VecDeque<Value>,
    enable_remaining: usize,
    scripts: BTreeMap<String, ParsedScript>,
    scopes: Vec<Vec<Scope>>,
    breakpoint_ids: BTreeMap<String, (String, Option<AuthoredBreakpoint>)>,
    installed: BTreeSet<(String, usize, usize, u64)>,
    breakpoint_generation: u64,
    pause_generation: u64,
    gate: Option<String>,
    pause_on_start: bool,
    first_pause: bool,
    generated_steps: Option<usize>,
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
            snapshot: DebugSnapshot {
                epoch: 1,
                state: DebugState::Disconnected,
                reason: String::new(),
                frames: Vec::new(),
                selected_frame: None,
                locals: Vec::new(),
                breakpoints: Vec::new(),
                messages: Vec::new(),
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
            breakpoint_ids: BTreeMap::new(),
            installed: BTreeSet::new(),
            breakpoint_generation: 1,
            pause_generation: 0,
            gate: None,
            pause_on_start: false,
            first_pause: false,
            generated_steps: None,
        };
        result.replace_desired(breakpoints);
        result
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
            DebugCommand::SelectFrame(index) => {
                if self.snapshot.state != DebugState::Paused || index >= self.scopes.len() {
                    return Err("Select a call frame while the debugger is paused.".into());
                }
                self.snapshot.selected_frame = Some(index);
                self.request_locals(index);
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
            self.queue("Runtime.getProperties", json!({"objectId":scope.object_id,"ownProperties":true,"accessorPropertiesOnly":false,"generatePreview":false}),
                Pending::Properties { pause:self.pause_generation, frame, scope:scope.name }, true);
        }
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
        self.snapshot.selected_frame = None;
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
        if self.snapshot.messages.len() == 32 {
            self.snapshot.messages.remove(0);
        }
        self.snapshot.messages.push(bounded(message, MAX_TEXT));
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
            _ => self.note(message),
        }
    }
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
fn remote_summary(value: &Value) -> String {
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
            json!({"objectId":"scope-1","ownProperties":true,"accessorPropertiesOnly":false,"generatePreview":false})
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
}
