use super::*;

impl DebugProtocol {
    pub(super) fn script_parsed(&mut self, params: &Value) {
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

    pub(super) fn host_blackbox_pattern(&self) -> String {
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

    pub(super) fn blackbox_script(&mut self, id: &str) {
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

    pub(super) fn install(&mut self, id: &str) {
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

    pub(super) fn paused(&mut self, params: &Value) {
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
                .enumerate()
                .filter_map(|(scope_number, scope)| {
                    let kind = scope.get("type")?.as_str()?;
                    if !matches!(kind, "local" | "closure" | "catch" | "block" | "script") {
                        return None;
                    }
                    let object = scope.get("object")?;
                    if object.get("subtype").and_then(Value::as_str) == Some("proxy") {
                        return None;
                    }
                    Some(Scope {
                        scope_number,
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
        self.vm_pause_generation = self.vm_pause_generation.saturating_add(1);
        self.inspection.vm_pause_generation = self.vm_pause_generation;
        let selected = self
            .snapshot
            .frames
            .iter()
            .position(|frame| frame.location.is_some())
            .or_else(|| (!self.snapshot.frames.is_empty()).then_some(0));
        self.snapshot.selected_frame = selected;
        if let Some(index) = selected {
            self.pause_epoch = self.pause_epoch.saturating_add(1);
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

    pub(super) fn finish_gate(&mut self) {
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

    pub(super) fn map_location(&self, location: &Value) -> Option<AuthoredLocation> {
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

    pub(super) fn request_locals(&mut self, frame: usize) {
        self.pause_generation += 1;
        self.snapshot.locals.clear();
        self.inspection.pause_epoch = self.pause_epoch;
        self.inspection.pause_generation = self.pause_generation;
        self.inspection.locals.clear();
        self.inspection.object_properties.clear();
        self.inspection.circular_children.clear();
        self.inspection.preview_properties.clear();
        let scopes = self.scopes.get(frame).cloned().unwrap_or_default();
        for scope in scopes {
            self.queue(
                "Runtime.getProperties",
                json!({
                    "objectId": scope.object_id,
                    "ownProperties": true,
                    "accessorPropertiesOnly": false,
                    "generatePreview": true
                }),
                Pending::Properties {
                    pause: self.pause_generation,
                    frame,
                    scope_number: scope.scope_number,
                    scope: scope.name,
                },
                true,
            );
        }
    }

    pub(super) fn refresh_paused_inspection(&mut self, frame: usize) {
        let expanded: Vec<(String, InspectedObjectSpec)> =
            std::mem::take(&mut self.inspected_objects)
                .into_iter()
                .collect();
        self.request_locals(frame);
        self.request_watches(frame);
        let pause = self.pause_generation;
        for (object_id, (subtype, ancestors)) in expanded {
            if self.queue_inspect_object(
                pause,
                frame,
                object_id.clone(),
                subtype.clone(),
                ancestors.clone(),
            ) {
                self.inspected_objects
                    .insert(object_id, (subtype, ancestors));
            }
        }
    }

    pub(super) fn request_watches(&mut self, frame: usize) {
        self.watch_generation = self.watch_generation.saturating_add(1);
        if self.watches.is_empty() {
            self.watch_results.clear();
            self.inspection.watches.clear();
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
        self.inspection.watches = self
            .watches
            .iter()
            .enumerate()
            .map(|(index, watch)| DebugInspectableWatch {
                index,
                expression: watch.expression.clone(),
                subtype: None,
                object_id: None,
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

    pub(super) fn replace_watches(&mut self, watches: Vec<WatchRequest>) {
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

    pub(super) fn refresh_watches(&mut self) {
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
        self.inspection.watches = self
            .watches
            .iter()
            .enumerate()
            .map(|(index, watch)| DebugInspectableWatch {
                index,
                expression: watch.expression.clone(),
                subtype: None,
                object_id: None,
            })
            .collect();
    }

    pub(super) fn replace_desired(&mut self, mut points: Vec<AuthoredBreakpoint>) {
        points.retain(|point| {
            point.line > 0 && point.source.len() <= 4_096 && point.column != Some(0)
        });
        points.sort();
        points.dedup();
        points.truncate(MAX_BREAKPOINTS);
        self.desired = points;
        self.refresh_breakpoints();
    }
    pub(super) fn refresh_breakpoints(&mut self) {
        self.snapshot.breakpoints = self
            .desired
            .iter()
            .map(|requested| BreakpointStatus {
                requested: requested.clone(),
                resolved: Vec::new(),
            })
            .collect();
    }
    pub(super) fn clear_pause(&mut self) {
        self.pause_epoch = self.pause_epoch.saturating_add(1);
        self.pause_generation += 1;
        self.gate = None;
        self.snapshot.frames.clear();
        self.snapshot.locals.clear();
        self.scopes.clear();
        self.call_frame_ids.clear();
        self.snapshot.selected_frame = None;
        self.inspected_objects.clear();
        self.inspection.pause_epoch = self.pause_epoch;
        self.inspection.pause_generation = self.pause_generation;
        self.inspection.locals.clear();
        self.inspection.object_properties.clear();
        self.inspection.circular_children.clear();
        self.inspection.preview_properties.clear();
        self.refresh_watches();
    }
    pub(super) fn clear_session(&mut self) {
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
    pub(super) fn fail(&mut self, message: &str) {
        self.clear_session();
        self.snapshot.state = DebugState::Failed;
        self.snapshot.reason = bounded(message, MAX_TEXT);
    }
    pub(super) fn note(&mut self, message: &str) {
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
    pub(super) fn record_output(
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
    pub(super) fn mapped_stack_location(
        &self,
        container: Option<&Value>,
    ) -> Option<AuthoredLocation> {
        let frames = container?
            .get("stackTrace")
            .and_then(|st| st.get("callFrames"))
            .and_then(Value::as_array)?;
        frames.iter().find_map(|frame| self.map_location(frame))
    }
    pub(super) fn parse_neoflash_location_arg(&self, arg: &Value) -> Option<AuthoredLocation> {
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
    pub(super) fn console_api_called(&mut self, params: &Value) {
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
    pub(super) fn exception_thrown(&mut self, params: &Value) {
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
    pub(super) fn request_error(&mut self, pending: Pending, message: &str) {
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
            Pending::SliceObject {
                pause,
                frame,
                object_id,
                ..
            } => {
                if self.snapshot.state == DebugState::Paused
                    && pause == self.pause_generation
                    && self.snapshot.selected_frame == Some(frame)
                {
                    self.inspection
                        .object_properties
                        .insert(object_id, Vec::new());
                }
                self.note(message);
            }
            Pending::ObjectProperties {
                pause,
                frame,
                object_id,
                release_object_id,
                ..
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
                if self.snapshot.state == DebugState::Paused
                    && pause == self.pause_generation
                    && self.snapshot.selected_frame == Some(frame)
                {
                    self.inspection
                        .object_properties
                        .insert(object_id, Vec::new());
                }
                self.note(message);
            }
            Pending::CycleCheck { .. } => {}
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
                    if let Some(insp) = self.inspection.watches.get_mut(index) {
                        insp.subtype = None;
                        insp.object_id = None;
                    }
                }
            }
            _ => self.note(message),
        }
    }
}
