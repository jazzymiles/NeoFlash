use super::*;

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
            inspection: DebugInspection::default(),
            inspected_objects: BTreeMap::new(),
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
            pause_epoch: 0,
            pause_generation: 0,
            vm_pause_generation: 0,
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

    pub fn inspection(&self) -> DebugInspection {
        let mut insp = self.inspection.clone();
        insp.pending_scope_requests = self.pending_scope_requests_count();
        insp
    }

    #[cfg(test)]
    pub fn pause_generation(&self) -> u64 {
        self.pause_generation
    }

    pub(super) fn pending_scope_requests_count(&self) -> usize {
        if self.snapshot.state != DebugState::Paused {
            return 0;
        }
        let pause = self.pause_generation;
        let selected = self.snapshot.selected_frame;
        self.pending
            .values()
            .filter(|pending| {
                matches!(
                    pending,
                    Pending::Properties {
                        pause: p_gen,
                        frame,
                        ..
                    } if *p_gen == pause && Some(*frame) == selected
                )
            })
            .count()
    }

    pub(super) fn queue_inspect_object(
        &mut self,
        pause: u64,
        frame: usize,
        object_id: String,
        subtype: Option<String>,
        ancestor_object_ids: Vec<String>,
    ) -> bool {
        if matches!(subtype.as_deref(), Some("array" | "typedarray")) {
            self.queue(
                "Runtime.callFunctionOn",
                json!({
                    "objectId": object_id,
                    "functionDeclaration": INSPECT_INDEXED_SLICE_JS,
                    "arguments": [{ "value": MAX_OBJECT_PROPERTIES }],
                    "silent": true,
                    "returnByValue": false,
                    "generatePreview": false
                }),
                Pending::SliceObject {
                    pause,
                    frame,
                    object_id,
                    ancestor_object_ids,
                },
                true,
            )
        } else {
            self.queue(
                "Runtime.getProperties",
                json!({
                    "objectId": object_id,
                    "ownProperties": true,
                    "accessorPropertiesOnly": false,
                    "generatePreview": true
                }),
                Pending::ObjectProperties {
                    pause,
                    frame,
                    object_id,
                    ancestor_object_ids,
                    release_object_id: None,
                },
                true,
            )
        }
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
                self.pause_epoch = self.pause_epoch.saturating_add(1);
                self.inspected_objects.clear();
                self.inspection.object_properties.clear();
                self.inspection.circular_children.clear();
                self.request_locals(index);
                self.request_watches(index);
                return Ok(());
            }
            DebugCommand::InspectObject {
                object_id,
                subtype,
                ancestor_object_ids,
            } => {
                if self.snapshot.state != DebugState::Paused || self.snapshot.command_pending {
                    return Err("Pause Test Movie before expanding object properties.".into());
                }
                let Some(frame) = self.snapshot.selected_frame else {
                    return Err("Select a call frame while the debugger is paused.".into());
                };
                let Some(object_id) = id_string(Some(&Value::String(object_id))) else {
                    return Err("Invalid remote object identifier.".into());
                };
                let clean_ancestors: Vec<String> = ancestor_object_ids
                    .into_iter()
                    .filter_map(|id| id_string(Some(&Value::String(id))))
                    .take(16)
                    .collect();
                if self.inspected_objects.len() >= MAX_INSPECTED_OBJECTS
                    && !self.inspected_objects.contains_key(&object_id)
                {
                    return Err("Expanded object limit reached for this pause.".into());
                }
                if !self.queue_inspect_object(
                    self.pause_generation,
                    frame,
                    object_id.clone(),
                    subtype.clone(),
                    clean_ancestors.clone(),
                ) {
                    return Err("Debugger request limit reached.".into());
                }
                self.inspected_objects
                    .insert(object_id, (subtype, clean_ancestors));
                return Ok(());
            }
            DebugCommand::SetVariableValue {
                pause_generation,
                frame: requested_frame,
                scope_number,
                variable_name,
                new_value,
            } => {
                if self.snapshot.state != DebugState::Paused || self.snapshot.command_pending {
                    return Err("Pause Test Movie before editing variables.".into());
                }
                let Some(frame) = self.snapshot.selected_frame else {
                    return Err("Select a call frame while the debugger is paused.".into());
                };
                if pause_generation != self.pause_generation || requested_frame != frame {
                    return Err(
                        "Active pause or call frame changed before editing finished.".into(),
                    );
                }
                let call_frame_id = self
                    .call_frame_ids
                    .get(frame)
                    .cloned()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "Active call frame cannot be modified.".to_owned())?;
                let variable_name = bounded(variable_name.trim(), MAX_TEXT);
                if variable_name.is_empty() {
                    return Err("Variable name cannot be empty.".into());
                }
                let new_arg = parse_debug_call_argument(&new_value)?;
                if !self.queue(
                    "Debugger.setVariableValue",
                    json!({
                        "scopeNumber": scope_number,
                        "variableName": variable_name,
                        "newValue": new_arg,
                        "callFrameId": call_frame_id
                    }),
                    Pending::EditVariable {
                        pause: self.pause_generation,
                        frame,
                    },
                    true,
                ) {
                    return Err("Debugger request limit reached.".into());
                }
                return Ok(());
            }
            DebugCommand::SetPropertyValue {
                pause_generation,
                frame: requested_frame,
                object_id,
                property_name,
                new_value,
            } => {
                if self.snapshot.state != DebugState::Paused || self.snapshot.command_pending {
                    return Err("Pause Test Movie before editing object properties.".into());
                }
                let Some(frame) = self.snapshot.selected_frame else {
                    return Err("Select a call frame while the debugger is paused.".into());
                };
                if pause_generation != self.pause_generation || requested_frame != frame {
                    return Err(
                        "Active pause or call frame changed before editing finished.".into(),
                    );
                }
                let Some(object_id) = id_string(Some(&Value::String(object_id))) else {
                    return Err("Invalid remote object identifier.".into());
                };
                if property_name.len() > 1_048_576 {
                    return Err("Property name is too long.".into());
                }
                let new_arg = parse_debug_call_argument(&new_value)?;
                if !self.queue(
                    "Runtime.callFunctionOn",
                    json!({
                        "objectId": object_id,
                        "functionDeclaration": SET_OWN_DATA_PROPERTY_JS,
                        "arguments": [{ "value": property_name }, new_arg],
                        "silent": true,
                        "returnByValue": true
                    }),
                    Pending::EditProperty {
                        pause: self.pause_generation,
                        frame,
                    },
                    true,
                ) {
                    return Err("Debugger request limit reached.".into());
                }
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
        self.pause_epoch = self.pause_epoch.saturating_add(1);
        self.pause_generation += 1;
        self.snapshot.locals.clear();
        self.inspected_objects.clear();
        self.inspection.pause_epoch = self.pause_epoch;
        self.inspection.pause_generation = self.pause_generation;
        self.inspection.locals.clear();
        self.inspection.watches.clear();
        self.inspection.object_properties.clear();
        self.inspection.circular_children.clear();
        self.inspection.preview_properties.clear();
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

    pub(super) fn queue(
        &mut self,
        method: &str,
        params: Value,
        pending: Pending,
        session: bool,
    ) -> bool {
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

    pub(super) fn response(&mut self, pending: Pending, result: &Value) {
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
                scope_number,
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
                        let Some(decoded) = decode_cdp_property(property) else {
                            continue;
                        };
                        if let Some(preview_fields) = property
                            .get("value")
                            .and_then(extract_preview_scalar_fields)
                            && !preview_fields.is_empty()
                        {
                            self.inspection
                                .preview_properties
                                .insert(decoded.name.clone(), preview_fields);
                        }
                        self.snapshot.locals.push(DebugLocal {
                            scope: scope.clone(),
                            name: decoded.name.clone(),
                            value: decoded.value.clone(),
                            kind: decoded.kind.clone(),
                            accessor: decoded.accessor,
                        });
                        self.inspection.locals.push(DebugInspectableLocal {
                            scope_number,
                            scope: scope.clone(),
                            name: decoded.name,
                            value: decoded.value,
                            kind: decoded.kind,
                            subtype: decoded.subtype,
                            object_id: decoded.object_id,
                            accessor: decoded.accessor,
                            writable: decoded.writable,
                        });
                    }
                }
            }
            Pending::SliceObject {
                pause,
                frame,
                object_id,
                ancestor_object_ids,
            } => {
                let slice_oid = id_string(result.get("result").and_then(|r| r.get("objectId")));
                if self.snapshot.state != DebugState::Paused
                    || pause != self.pause_generation
                    || self.snapshot.selected_frame != Some(frame)
                {
                    if let Some(sid) = slice_oid
                        && sid != object_id
                    {
                        self.queue(
                            "Runtime.releaseObject",
                            json!({"objectId": sid}),
                            Pending::Remove,
                            true,
                        );
                    }
                    return;
                }
                if result.get("exceptionDetails").is_some() {
                    self.inspection
                        .object_properties
                        .insert(object_id, Vec::new());
                    self.note("Chrome could not read paused collection properties.");
                    return;
                }
                let Some(slice_oid) = slice_oid else {
                    self.inspection
                        .object_properties
                        .insert(object_id, Vec::new());
                    return;
                };
                let release_object_id = (slice_oid != object_id).then(|| slice_oid.clone());
                if !self.queue(
                    "Runtime.getProperties",
                    json!({
                        "objectId": slice_oid,
                        "ownProperties": true,
                        "accessorPropertiesOnly": false,
                        "generatePreview": true
                    }),
                    Pending::ObjectProperties {
                        pause,
                        frame,
                        object_id: object_id.clone(),
                        ancestor_object_ids,
                        release_object_id: release_object_id.clone(),
                    },
                    true,
                ) {
                    if let Some(sid) = release_object_id {
                        self.queue(
                            "Runtime.releaseObject",
                            json!({"objectId": sid}),
                            Pending::Remove,
                            true,
                        );
                    }
                    self.inspection
                        .object_properties
                        .insert(object_id, Vec::new());
                }
            }
            Pending::ObjectProperties {
                pause,
                frame,
                object_id,
                ancestor_object_ids,
                release_object_id,
            } => {
                if let Some(sid) = release_object_id
                    && sid != object_id
                {
                    self.queue(
                        "Runtime.releaseObject",
                        json!({"objectId": sid}),
                        Pending::Remove,
                        true,
                    );
                }
                if self.snapshot.state != DebugState::Paused
                    || pause != self.pause_generation
                    || self.snapshot.selected_frame != Some(frame)
                {
                    return;
                }
                if result.get("exceptionDetails").is_some() {
                    self.inspection
                        .object_properties
                        .insert(object_id, Vec::new());
                    self.note("Chrome could not read paused object properties.");
                    return;
                }
                let mut entries = Vec::new();
                if let Some(properties) = result.get("result").and_then(Value::as_array) {
                    for property in properties.iter().take(MAX_OBJECT_PROPERTIES * 8) {
                        if entries.len() >= MAX_OBJECT_PROPERTIES {
                            break;
                        }
                        if let Some(decoded) = decode_cdp_property(property) {
                            entries.push(decoded);
                        }
                    }
                }
                let mut candidates = Vec::new();
                for prop in &entries {
                    let Some(child_oid) = prop.object_id.as_ref() else {
                        continue;
                    };
                    if child_oid == &object_id
                        || ancestor_object_ids.iter().any(|anc| anc == child_oid)
                    {
                        self.inspection
                            .circular_children
                            .insert((object_id.clone(), child_oid.clone()));
                    } else if !ancestor_object_ids.is_empty() && !candidates.contains(child_oid) {
                        candidates.push(child_oid.clone());
                    }
                }
                if !candidates.is_empty() && !ancestor_object_ids.is_empty() {
                    let mut args =
                        Vec::with_capacity(1 + ancestor_object_ids.len() + candidates.len());
                    args.push(json!({"value": ancestor_object_ids.len()}));
                    for anc in &ancestor_object_ids {
                        args.push(json!({"objectId": anc}));
                    }
                    for cand in &candidates {
                        args.push(json!({"objectId": cand}));
                    }
                    self.queue(
                        "Runtime.callFunctionOn",
                        json!({
                            "objectId": ancestor_object_ids[0],
                            "functionDeclaration": DETECT_CIRCULAR_CHILDREN_JS,
                            "arguments": args,
                            "silent": true,
                            "returnByValue": true
                        }),
                        Pending::CycleCheck {
                            pause,
                            frame,
                            parent_object_id: object_id.clone(),
                            candidate_object_ids: candidates,
                        },
                        true,
                    );
                }
                self.inspection.object_properties.insert(object_id, entries);
            }
            Pending::CycleCheck {
                pause,
                frame,
                parent_object_id,
                candidate_object_ids,
            } => {
                if self.snapshot.state != DebugState::Paused
                    || pause != self.pause_generation
                    || self.snapshot.selected_frame != Some(frame)
                {
                    return;
                }
                if let Some(indices) = result
                    .get("result")
                    .and_then(|r| r.get("value"))
                    .and_then(Value::as_array)
                {
                    for idx_val in indices {
                        if let Some(idx) = idx_val.as_u64().and_then(|v| usize::try_from(v).ok())
                            && let Some(child_oid) = candidate_object_ids.get(idx)
                        {
                            self.inspection
                                .circular_children
                                .insert((parent_object_id.clone(), child_oid.clone()));
                        }
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
                    if let Some(insp) = self.inspection.watches.get_mut(index) {
                        insp.subtype = None;
                        insp.object_id = None;
                    }
                    return;
                }
                let remote = result.get("result").unwrap_or(&Value::Null);
                if remote.is_null() {
                    slot.value = "<unavailable>".into();
                    slot.is_error = true;
                    if let Some(insp) = self.inspection.watches.get_mut(index) {
                        insp.subtype = None;
                        insp.object_id = None;
                    }
                } else {
                    slot.value = remote_summary(remote);
                    slot.is_error = false;
                    if let Some(insp) = self.inspection.watches.get_mut(index) {
                        insp.subtype = remote
                            .get("subtype")
                            .and_then(Value::as_str)
                            .map(|s| bounded(s, 80));
                        insp.object_id = remote_inspectable_object_id(remote, false);
                    }
                }
            }
            Pending::EditVariable { pause, frame } | Pending::EditProperty { pause, frame } => {
                if self.snapshot.state != DebugState::Paused
                    || pause != self.pause_generation
                    || self.snapshot.selected_frame != Some(frame)
                {
                    return;
                }
                if let Some(details) = result.get("exceptionDetails") {
                    let reason = format_watch_exception(details, result.get("result"));
                    self.note(&format!("Cannot assign value: {reason}"));
                    return;
                }
                self.refresh_paused_inspection(frame);
            }
            Pending::Remove => {}
        }
    }
}
