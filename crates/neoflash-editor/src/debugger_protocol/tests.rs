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
    protocol.receive(
        old_epoch,
        json!({"method":"Debugger.paused","sessionId":"owned-session","params":paused_frame(5)}),
    );
    protocol.receive(
        protocol.epoch(),
        json!({"method":"Debugger.paused","sessionId":"owned-session","params":paused_frame(5)}),
    );
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
fn t368_debugger_formats_structured_multidimensional_arrays_objects_and_strips_trailing_location() {
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

    let w_as2 = compile_watch_expression("hero._x + _root.score", ScriptLanguage::ActionScript2);
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
    let as2_root = compile_watch_expression("hero._x + _root.score", ScriptLanguage::ActionScript2);
    let as2_ops =
        compile_watch_expression("bonus <> 0 and bonus >= 5", ScriptLanguage::ActionScript2);
    let as3_cast =
        compile_watch_expression("int(items.length + 2.9)", ScriptLanguage::ActionScript3);
    let as3_missing = compile_watch_expression("missingWatchSymbol", ScriptLanguage::ActionScript3);

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
            e_name = compile_watch_expression("name", ScriptLanguage::ActionScript3).js_expression,
            e_miss = as3_missing.js_expression,
        ))
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), r#"[45,420,true,5,"clip_mc",true]"#);
    }
}

#[test]
fn t410_nested_object_inspection_and_scalar_property_editing_refresh_locals_and_watches() {
    assert_eq!(parse_debug_scalar_value("42").unwrap(), json!(42));
    assert_eq!(parse_debug_scalar_value("-3.5").unwrap(), json!(-3.5));
    assert_eq!(parse_debug_scalar_value("true").unwrap(), json!(true));
    assert_eq!(parse_debug_scalar_value("false").unwrap(), json!(false));
    assert_eq!(parse_debug_scalar_value("null").unwrap(), Value::Null);
    assert_eq!(
        parse_debug_scalar_value("\"hero_1\"").unwrap(),
        json!("hero_1")
    );
    assert_eq!(
        parse_debug_scalar_value("'clip\\'s'").unwrap(),
        json!("clip's")
    );
    assert!(parse_debug_scalar_value("").is_err());
    assert!(parse_debug_scalar_value("{\"x\":1}").is_err());
    assert!(parse_debug_scalar_value("[1,2]").is_err());
    assert!(parse_debug_scalar_value("undefined").is_err());

    let mut protocol = connect();
    protocol.set_watches(vec![WatchRequest::new("hero", "hero")]);
    // Scope index 0 is `with` (filtered out), scope index 1 is `local` -> scope_number MUST be 1!
    event(
        &mut protocol,
        "Debugger.paused",
        json!({
            "reason": "other",
            "callFrames": [
                {
                    "callFrameId": "cf-0",
                    "functionName": "tick",
                    "location": {"scriptId": "script-1", "lineNumber": 2, "columnNumber": 0},
                    "scopeChain": [
                        {"type": "with", "object": {"type": "object", "objectId": "scope-with"}},
                        {"type": "local", "object": {"type": "object", "objectId": "scope-local"}}
                    ]
                },
                {
                    "callFrameId": "cf-1",
                    "functionName": "outer",
                    "location": {"scriptId": "script-1", "lineNumber": 2, "columnNumber": 0},
                    "scopeChain": [
                        {"type": "local", "object": {"type": "object", "objectId": "scope-outer"}}
                    ]
                }
            ]
        }),
    );
    let init_cmds = protocol.drain_commands();
    assert_eq!(init_cmds.len(), 2);
    reply(
        &mut protocol,
        &init_cmds[0],
        json!({
            "result": [
                {"name": "score", "writable": true, "value": {"type": "number", "value": 10}},
                {"name": "hero", "writable": true, "value": {"type": "object", "objectId": "obj-hero", "className": "Object"}},
                {"name": "trap", "writable": true, "value": {"type": "object", "subtype": "proxy", "objectId": "obj-proxy"}}
            ]
        }),
    );
    reply(
        &mut protocol,
        &init_cmds[1],
        json!({"result": {"type": "object", "objectId": "obj-watch-hero", "className": "Object"}}),
    );

    let insp = protocol.inspection();
    assert_eq!(insp.locals.len(), 3);
    assert_eq!(insp.locals[0].scope_number, 1);
    assert!(insp.locals[0].writable);
    assert_eq!(insp.locals[1].object_id.as_deref(), Some("obj-hero"));
    assert_eq!(
        insp.locals[2].object_id, None,
        "proxies must never be expandable"
    );
    assert_eq!(insp.watches[0].object_id.as_deref(), Some("obj-watch-hero"));

    // Lazy expand `obj-hero`.
    protocol
        .command(DebugCommand::InspectObject {
            object_id: "obj-hero".into(),
            subtype: None,
            ancestor_object_ids: Vec::new(),
        })
        .unwrap();
    let expand_cmds = protocol.drain_commands();
    assert_eq!(expand_cmds.len(), 1);
    assert_eq!(expand_cmds[0]["method"], "Runtime.getProperties");
    assert_eq!(
        expand_cmds[0]["params"],
        json!({
            "objectId": "obj-hero",
            "ownProperties": true,
            "accessorPropertiesOnly": false,
            "generatePreview": true
        })
    );
    reply(
        &mut protocol,
        &expand_cmds[0],
        json!({
            "result": [
                {"name": "x", "writable": true, "value": {"type": "number", "value": 24}},
                {"name": "self", "writable": true, "value": {"type": "object", "objectId": "obj-hero"}}
            ]
        }),
    );
    assert_eq!(
        protocol.inspection().object_properties["obj-hero"][0].value,
        "24"
    );

    // Edit top-level local via Debugger.setVariableValue with exact scopeNumber 1.
    let pause_gen = protocol.pause_generation();
    protocol
        .command(DebugCommand::SetVariableValue {
            pause_generation: pause_gen,
            frame: 0,
            scope_number: 1,
            variable_name: "score".into(),
            new_value: "99".into(),
        })
        .unwrap();
    let var_cmds = protocol.drain_commands();
    assert_eq!(var_cmds.len(), 1);
    assert_eq!(var_cmds[0]["method"], "Debugger.setVariableValue");
    assert_eq!(
        var_cmds[0]["params"],
        json!({
            "scopeNumber": 1,
            "variableName": "score",
            "newValue": {"value": 99},
            "callFrameId": "cf-0"
        })
    );
    reply(&mut protocol, &var_cmds[0], json!({}));
    // Must immediately refresh scope locals, watches, AND expanded `obj-hero`!
    let refresh_cmds = protocol.drain_commands();
    assert_eq!(refresh_cmds.len(), 3);
    assert_eq!(refresh_cmds[0]["method"], "Runtime.getProperties");
    assert_eq!(refresh_cmds[1]["method"], "Debugger.evaluateOnCallFrame");
    assert_eq!(refresh_cmds[2]["method"], "Runtime.getProperties");
    assert_eq!(refresh_cmds[2]["params"]["objectId"], "obj-hero");

    // Edit nested object property via Runtime.callFunctionOn.
    let pause_gen2 = protocol.pause_generation();
    protocol
        .command(DebugCommand::SetPropertyValue {
            pause_generation: pause_gen2,
            frame: 0,
            object_id: "obj-hero".into(),
            property_name: "x".into(),
            new_value: "128.5".into(),
        })
        .unwrap();
    let prop_cmds = protocol.drain_commands();
    assert_eq!(prop_cmds.len(), 1);
    assert_eq!(prop_cmds[0]["method"], "Runtime.callFunctionOn");
    assert_eq!(
        prop_cmds[0]["params"],
        json!({
            "objectId": "obj-hero",
            "functionDeclaration": SET_OWN_DATA_PROPERTY_JS,
            "arguments": [{"value": "x"}, {"value": 128.5}],
            "silent": true,
            "returnByValue": true
        })
    );
    reply(
        &mut protocol,
        &prop_cmds[0],
        json!({"result": {"type": "number", "value": 128.5}}),
    );
    let prop_refresh = protocol.drain_commands();
    assert_eq!(prop_refresh.len(), 3);

    // Switching call frames clears cached expanded object properties cleanly.
    reply(
        &mut protocol,
        &prop_refresh[2],
        json!({"result": [{"name": "x", "writable": true, "value": {"type": "number", "value": 128.5}}]}),
    );
    assert!(
        protocol
            .inspection()
            .object_properties
            .contains_key("obj-hero")
    );
    protocol.command(DebugCommand::SelectFrame(1)).unwrap();
    assert!(protocol.inspection().object_properties.is_empty());
}

#[test]
fn t414_sparse_array_preview_clamps_render_len_without_dense_oom() {
    // Sparse array `a[0] = 10; a[2] = 30; a[2_000_000_000] = 1;` with `overflow: true`
    let preview = json!({
        "type": "object",
        "subtype": "array",
        "description": "Array(2000000001)",
        "overflow": true,
        "properties": [
            {"name": "0", "type": "number", "value": "10"},
            {"name": "2", "type": "number", "value": "30"},
            {"name": "2000000000", "type": "number", "value": "1"}
        ]
    });
    let formatted = format_cdp_object_preview(&preview, 0, false).unwrap();
    assert_eq!(formatted, "[10, <empty>, 30, 2000000000: 1, …]");

    // Pure high-index sparse array `a[2_000_000_000] = 1` (`overflow: true` and `false`)
    let single_sparse_overflow = json!({
        "type": "object",
        "subtype": "array",
        "description": "Array(2000000001)",
        "overflow": true,
        "properties": [
            {"name": "2000000000", "type": "number", "value": "1"}
        ]
    });
    assert_eq!(
        format_cdp_object_preview(&single_sparse_overflow, 0, false).unwrap(),
        "[2000000000: 1, …]"
    );

    let single_sparse_exact = json!({
        "type": "object",
        "subtype": "array",
        "description": "Array(2000000001)",
        "overflow": false,
        "properties": [
            {"name": "2000000000", "type": "number", "value": "1"}
        ]
    });
    assert_eq!(
        format_cdp_object_preview(&single_sparse_exact, 0, false).unwrap(),
        "[2000000000: 1]"
    );
}

#[test]
fn t414_bounded_slice_inspection_for_16mb_uint8array_and_large_array() {
    let mut protocol = connect();
    protocol.set_watches(vec![WatchRequest::new("buf16m", "buf16m")]);
    event(&mut protocol, "Debugger.paused", paused_frame(5));
    let init_cmds = protocol.drain_commands();
    assert_eq!(init_cmds.len(), 2);
    reply(
        &mut protocol,
        &init_cmds[0],
        json!({
            "result": [
                {
                    "name": "bigArr",
                    "writable": true,
                    "value": {
                        "type": "object",
                        "subtype": "array",
                        "objectId": "obj-arr-100k",
                        "description": "Array(100000)"
                    }
                }
            ]
        }),
    );
    reply(
        &mut protocol,
        &init_cmds[1],
        json!({
            "result": {
                "type": "object",
                "subtype": "typedarray",
                "objectId": "obj-u8-16m",
                "description": "Uint8Array(16777216)"
            }
        }),
    );

    let insp = protocol.inspection();
    assert_eq!(insp.locals[0].subtype.as_deref(), Some("array"));
    assert_eq!(insp.watches[0].subtype.as_deref(), Some("typedarray"));
    assert_eq!(insp.pending_scope_requests, 0);

    // Inspecting 16 MB Uint8Array must NEVER call unbounded Runtime.getProperties on obj-u8-16m;
    // it uses Runtime.callFunctionOn(INSPECT_INDEXED_SLICE_JS, 256) then releases the temp slice.
    protocol
        .command(DebugCommand::InspectObject {
            object_id: "obj-u8-16m".into(),
            subtype: Some("typedarray".into()),
            ancestor_object_ids: vec!["obj-u8-16m".into()],
        })
        .unwrap();
    let slice_cmds = protocol.drain_commands();
    assert_eq!(slice_cmds.len(), 1);
    assert_eq!(slice_cmds[0]["method"], "Runtime.callFunctionOn");
    assert_eq!(slice_cmds[0]["params"]["objectId"], "obj-u8-16m");
    assert_eq!(
        slice_cmds[0]["params"]["arguments"],
        json!([{"value": MAX_OBJECT_PROPERTIES}])
    );
    reply(
        &mut protocol,
        &slice_cmds[0],
        json!({"result": {"type": "object", "objectId": "slice-u8-temp"}}),
    );
    let get_slice_cmds = protocol.drain_commands();
    assert_eq!(get_slice_cmds.len(), 1);
    assert_eq!(get_slice_cmds[0]["method"], "Runtime.getProperties");
    assert_eq!(get_slice_cmds[0]["params"]["objectId"], "slice-u8-temp");

    let slice_entries: Vec<Value> = (0..MAX_OBJECT_PROPERTIES)
            .map(|i| json!({"name": i.to_string(), "writable": true, "value": {"type": "number", "value": i % 256}}))
            .collect();
    reply(
        &mut protocol,
        &get_slice_cmds[0],
        json!({"result": slice_entries}),
    );
    let release_cmds = protocol.drain_commands();
    assert_eq!(release_cmds.len(), 1);
    assert_eq!(release_cmds[0]["method"], "Runtime.releaseObject");
    assert_eq!(release_cmds[0]["params"]["objectId"], "slice-u8-temp");
    assert_eq!(
        protocol.inspection().object_properties["obj-u8-16m"].len(),
        MAX_OBJECT_PROPERTIES
    );

    // Inspecting Array(100_000) also routes through the bounded slice helper.
    protocol
        .command(DebugCommand::InspectObject {
            object_id: "obj-arr-100k".into(),
            subtype: Some("array".into()),
            ancestor_object_ids: vec!["obj-arr-100k".into()],
        })
        .unwrap();
    let arr_cmds = protocol.drain_commands();
    assert_eq!(arr_cmds.len(), 1);
    assert_eq!(arr_cmds[0]["method"], "Runtime.callFunctionOn");
    assert_eq!(arr_cmds[0]["params"]["objectId"], "obj-arr-100k");

    // Verify the JS slice helper directly in Node on a real 16 MB Uint8Array and Array(100_000)!
    if let Ok(output) = std::process::Command::new("node")
            .arg("-e")
            .arg(format!(
                r#"
const sliceFn = {slice_js};
const setPropFn = {set_js};
const u8 = new Uint8Array(16 * 1024 * 1024);
u8[0] = 7; u8[255] = 99;
const u8Slice = sliceFn.call(u8, 256);
const arr = new Array(100000).fill(3);
const arrSlice = sliceFn.call(arr, 256);
const target = {{ safe: 1 }};
let blockedProto = false;
try {{ setPropFn.call(target, "__proto__", null); }} catch (_) {{ blockedProto = true; }}
const setterObj = {{ get trap() {{ return 1; }}, set trap(_) {{ throw new Error("setter ran"); }} }};
let blockedSetter = false;
try {{ setPropFn.call(setterObj, "trap", 2); }} catch (_) {{ blockedSetter = true; }}
const longKey = "k".repeat(2000);
target[longKey] = 10;
setPropFn.call(target, longKey, 42);
console.log(JSON.stringify([
  Object.getOwnPropertyNames(u8Slice).length,
  u8Slice[0],
  u8Slice[255],
  u8Slice.byteLength,
  Object.getOwnPropertyNames(arrSlice).length,
  arrSlice[0],
  blockedProto,
  Object.getPrototypeOf(target) === Object.prototype,
  blockedSetter,
  target[longKey]
]));
"#,
                slice_js = INSPECT_INDEXED_SLICE_JS,
                set_js = SET_OWN_DATA_PROPERTY_JS,
            ))
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                stdout.trim(),
                "[259,7,99,16777216,256,3,true,true,true,42]"
            );
        }
}

#[test]
fn t414_stale_edit_rejection_unserializable_scalars_json_strings_and_fresh_oid_cycles() {
    assert_eq!(
        parse_debug_call_argument("NaN").unwrap(),
        json!({"unserializableValue": "NaN"})
    );
    assert_eq!(
        parse_debug_call_argument("Infinity").unwrap(),
        json!({"unserializableValue": "Infinity"})
    );
    assert_eq!(
        parse_debug_call_argument("-Infinity").unwrap(),
        json!({"unserializableValue": "-Infinity"})
    );

    let mut protocol = connect();
    event(
        &mut protocol,
        "Debugger.paused",
        json!({
            "reason": "other",
            "callFrames": [
                {
                    "callFrameId": "cf-0",
                    "functionName": "inner",
                    "location": {"scriptId": "script-1", "lineNumber": 5, "columnNumber": 0},
                    "scopeChain": [{"type": "local", "object": {"type": "object", "objectId": "scope-0"}}]
                },
                {
                    "callFrameId": "cf-1",
                    "functionName": "outer",
                    "location": {"scriptId": "script-1", "lineNumber": 6, "columnNumber": 0},
                    "scopeChain": [{"type": "local", "object": {"type": "object", "objectId": "scope-1"}}]
                }
            ]
        }),
    );
    assert_eq!(protocol.inspection().pending_scope_requests, 1);
    let init_cmd = protocol.drain_commands().pop().unwrap();
    let long_str = "a".repeat(MAX_TEXT + 50);
    reply(
        &mut protocol,
        &init_cmd,
        json!({
            "result": [
                {"name": "escaped", "writable": true, "value": {"type": "string", "value": "a\0b'c\x1b"}},
                {"name": "huge", "writable": true, "value": {"type": "string", "value": long_str}},
                {"name": "node", "writable": true, "value": {"type": "object", "objectId": "oid-root-1"}}
            ]
        }),
    );
    let insp = protocol.inspection();
    assert_eq!(insp.pending_scope_requests, 0);
    // JSON string summary uses \u0000 and unescaped `'`, so parse_debug_scalar_value round-trips cleanly!
    assert_eq!(insp.locals[0].value, "\"a\\u0000b'c\\u001b\"");
    assert!(insp.locals[0].writable);
    assert!(parse_debug_scalar_value(&insp.locals[0].value).is_ok());
    // Truncated > MAX_TEXT strings disable inline editing so lost_focus / Enter cannot truncate live V8 strings!
    assert!(!insp.locals[1].writable);

    let frame0_gen = protocol.pause_generation();
    protocol.command(DebugCommand::SelectFrame(1)).unwrap();
    let _ = protocol.drain_commands();
    // Stale frame-0 edit after switching to frame 1 MUST be rejected!
    assert!(
        protocol
            .command(DebugCommand::SetVariableValue {
                pause_generation: frame0_gen,
                frame: 0,
                scope_number: 0,
                variable_name: "x".into(),
                new_value: "1".into(),
            })
            .is_err()
    );
    assert!(
        protocol
            .command(DebugCommand::SetPropertyValue {
                pause_generation: frame0_gen,
                frame: 0,
                object_id: "oid-root-1".into(),
                property_name: "k".into(),
                new_value: "1".into(),
            })
            .is_err()
    );

    // Untruncated >1024-char property key + NaN unserializableValue on current frame 1:
    let frame1_gen = protocol.pause_generation();
    let long_prop_key = "p".repeat(1_500);
    protocol
        .command(DebugCommand::SetPropertyValue {
            pause_generation: frame1_gen,
            frame: 1,
            object_id: "oid-root-1".into(),
            property_name: long_prop_key.clone(),
            new_value: "NaN".into(),
        })
        .unwrap();
    let prop_cmd = protocol.drain_commands().pop().unwrap();
    assert_eq!(
        prop_cmd["params"]["arguments"][0]["value"].as_str(),
        Some(long_prop_key.as_str())
    );
    assert_eq!(
        prop_cmd["params"]["arguments"][1],
        json!({"unserializableValue": "NaN"})
    );
    reply(
        &mut protocol,
        &prop_cmd,
        json!({"result": {"type": "undefined"}}),
    );
    let _ = protocol.drain_commands();

    // Real Chrome fresh objectId cycle detection (`oid-fresh-2 === oid-root-1`):
    protocol
        .command(DebugCommand::InspectObject {
            object_id: "oid-root-1".into(),
            subtype: None,
            ancestor_object_ids: vec!["oid-root-1".into()],
        })
        .unwrap();
    let get_props = protocol.drain_commands().pop().unwrap();
    reply(
        &mut protocol,
        &get_props,
        json!({
            "result": [
                {"name": "self", "writable": true, "value": {"type": "object", "objectId": "oid-fresh-2"}},
                {"name": "child", "writable": true, "value": {"type": "object", "objectId": "oid-child-3"}}
            ]
        }),
    );
    let cycle_cmd = protocol.drain_commands().pop().unwrap();
    assert_eq!(cycle_cmd["method"], "Runtime.callFunctionOn");
    assert_eq!(
        cycle_cmd["params"]["arguments"],
        json!([
            {"value": 1},
            {"objectId": "oid-root-1"},
            {"objectId": "oid-fresh-2"},
            {"objectId": "oid-child-3"}
        ])
    );
    reply(
        &mut protocol,
        &cycle_cmd,
        json!({"result": {"type": "object", "value": [0]}}),
    );
    assert!(
        protocol
            .inspection()
            .circular_children
            .contains(&("oid-root-1".into(), "oid-fresh-2".into()))
    );
    assert!(
        !protocol
            .inspection()
            .circular_children
            .contains(&("oid-root-1".into(), "oid-child-3".into()))
    );
}
