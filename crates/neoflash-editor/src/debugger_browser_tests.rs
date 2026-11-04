//! Opt-in real Chromium acceptance for the production debugger modules.
use crate::debugger_protocol::DebugState;
use crate::debugger_protocol::{AuthoredBreakpoint, DebugCommand, DebugProtocol, DebugSnapshot};
use crate::debugger_sources::read_export_sources;
use crate::debugger_transport::{BrowserPipe, browser_executable, file_url, private_directory};
use neoflash_core::FrameScript;
use neoflash_core::{Project, Script, ScriptLanguage};
use serde_json::json;
use std::time::{Duration, Instant};

fn drive(
    protocol: &mut DebugProtocol,
    pipe: &mut BrowserPipe,
    session: &mut Option<String>,
    until: impl Fn(&DebugSnapshot) -> bool,
) -> DebugSnapshot {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        for command in protocol.drain_commands() {
            pipe.send(&command).unwrap();
        }
        let snapshot = protocol.snapshot();
        assert_ne!(snapshot.state, DebugState::Failed, "{snapshot:?}");
        if until(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "Debugger wait timed out: {snapshot:?}"
        );
        if let Some(message) = pipe.receive().unwrap() {
            if std::env::var_os("NEOFLASH_DEBUG_TEST_LOG").is_some() {
                eprintln!("CDP {message}");
            }
            if let Some(id) = message.get("sessionId").and_then(serde_json::Value::as_str) {
                *session = Some(id.into());
            }
            protocol.receive(protocol.epoch(), message);
        }
    }
}

// Test-only observation of a fixture-owned primitive. The production protocol never evaluates
// authored expressions. This read cannot call a getter or silently mutate the suspended target.
fn read_probe(
    protocol: &mut DebugProtocol,
    pipe: &mut BrowserPipe,
    session: &str,
    request: &mut u64,
) -> Option<u64> {
    *request += 1;
    let id = *request;
    pipe.send(&json!({"id":id,"sessionId":session,"method":"Runtime.evaluate",
        "params":{"expression":"globalThis.__neoflashDebugTicks","returnByValue":true,"throwOnSideEffect":true,"silent":true}})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for command in protocol.drain_commands() {
            pipe.send(&command).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "Test probe timed out: {:?}",
            protocol.snapshot()
        );
        if let Some(message) = pipe.receive().unwrap() {
            if message.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
                assert!(message.get("error").is_none(), "{message}");
                assert!(
                    message["result"].get("exceptionDetails").is_none(),
                    "{message}"
                );
                return message["result"]["result"]["value"].as_u64();
            }
            protocol.receive(protocol.epoch(), message);
        }
    }
}

fn wait_for_probe(
    protocol: &mut DebugProtocol,
    pipe: &mut BrowserPipe,
    session: &str,
    request: &mut u64,
    minimum: u64,
) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = read_probe(protocol, pipe, session, request)
            && value >= minimum
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "Fixture timer did not advance: {:?}",
            protocol.snapshot()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs the production compiler/export maps and private OS pipe against a real browser.
/// Kept opt-in because the ordinary shared suite must not launch installed applications.
#[test]
#[ignore = "launches an isolated headless Chromium; requires built HTML5 runtime"]
fn real_browser_breakpoint_step_locals_restart_and_disconnect() {
    let executable = browser_executable().unwrap();
    let output = private_directory().unwrap();
    let mut project = Project::blank();
    let script = Script { language: ScriptLanguage::ActionScript2, source:
        "// Unicode π\nfunction add(value:Number):Number {\n  var local:Number = value + 2;\n  trace(local);\n  return local;\n}\nvar answer:Number = add(5);\ntrace(answer);".into() };
    project
        .symbol_mut(&project.root.clone())
        .unwrap()
        .timeline
        .scripts
        .push(FrameScript { frame: 0, script });
    let timeline = &mut project.symbol_mut(&project.root.clone()).unwrap().timeline;
    timeline.frame_count = timeline.frame_count.max(2);
    timeline.scripts.push(FrameScript { frame:1, script:Script {
        language:ScriptLanguage::JavaScript,
        source:"if (!globalThis.__neoflashDebugTimer) {\n  globalThis.__neoflashDebugTicks = 0;\n  globalThis.__neoflashDebugTimer = globalThis.setInterval(function debugTick() {\n    globalThis.__neoflashDebugTicks += 1;\n  }, 20);\n}".into()
    }});

    let index = neoflash_export::export_project(
        &project,
        &output,
        &std::env::var_os("NEOFLASH_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/web-runtime")
            }),
    )
    .unwrap();
    let (sources, origins) = read_export_sources(&index).unwrap();
    let source = origins[0].location.clone();
    let point = AuthoredBreakpoint {
        source: source.clone(),
        line: 4,
        column: None,
    };
    let mut protocol = DebugProtocol::new(file_url(&index).unwrap(), sources, vec![point]);
    let headless = std::env::var_os("NEOFLASH_TEST_BROWSER_HEADED").is_none();
    let mut pipe = BrowserPipe::launch(&executable, headless).unwrap();
    let profile = pipe.profile().to_path_buf();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
        0o700
    );
    protocol.start();
    let mut session = None;
    let mut probe_request = 1_000_000;
    let paused = drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Paused
            && snapshot
                .locals
                .iter()
                .any(|local| local.name == "local" && local.value == "7")
    });
    assert_eq!(paused.frames[0].location.as_ref().unwrap().source, source);
    assert_eq!(paused.frames[0].location.as_ref().unwrap().line, 4);
    assert!(
        paused
            .frames
            .iter()
            .filter(|frame| frame.location.is_some())
            .count()
            >= 2,
        "{paused:?}"
    );
    assert!(
        paused
            .locals
            .iter()
            .any(|local| local.name == "value" && local.value == "5")
    );
    protocol.command(DebugCommand::StepOver).unwrap();
    let next = drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Paused
            && snapshot
                .frames
                .first()
                .and_then(|frame| frame.location.as_ref())
                .is_some_and(|location| location.line == 5)
    });
    assert!(!next.command_pending);
    protocol.command(DebugCommand::StepOut).unwrap();
    let returned = drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Paused
            && snapshot
                .frames
                .first()
                .and_then(|frame| frame.location.as_ref())
                .is_some_and(|location| location.line >= 7)
    });
    assert!(returned.frames.len() < paused.frames.len());
    protocol.command(DebugCommand::Resume).unwrap();
    drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Running && !snapshot.command_pending
    });
    let epoch = protocol.epoch();
    protocol.command(DebugCommand::Restart).unwrap();
    assert!(protocol.snapshot().frames.is_empty());
    assert!(protocol.snapshot().locals.is_empty());
    let restarted = drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Paused
            && snapshot
                .locals
                .iter()
                .any(|local| local.name == "local" && local.value == "7")
    });
    assert!(restarted.epoch > epoch);
    assert_eq!(restarted.frames[0].location.as_ref().unwrap().line, 4);
    protocol
        .command(DebugCommand::SetBreakpoints(vec![AuthoredBreakpoint {
            source: source.clone(),
            line: 7,
            column: None,
        }]))
        .unwrap();
    protocol.command(DebugCommand::Restart).unwrap();
    assert!(protocol.snapshot().locals.is_empty());
    drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Paused
            && snapshot
                .frames
                .first()
                .and_then(|frame| frame.location.as_ref())
                .is_some_and(|location| location.source == source && location.line == 7)
    });
    protocol.command(DebugCommand::StepInto).unwrap();
    let entered = drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Paused
            && snapshot
                .frames
                .first()
                .and_then(|frame| frame.location.as_ref())
                .is_some_and(|location| location.source == source && location.line == 3)
            && snapshot
                .locals
                .iter()
                .any(|local| local.name == "value" && local.value == "5")
    });
    assert!(
        entered
            .locals
            .iter()
            .any(|local| local.name == "local" && local.value == "undefined"),
        "{entered:?}"
    );
    protocol
        .command(DebugCommand::SetBreakpoints(Vec::new()))
        .unwrap();
    protocol.command(DebugCommand::Resume).unwrap();
    drive(&mut protocol, &mut pipe, &mut session, |snapshot| {
        snapshot.state == DebugState::Running && !snapshot.command_pending
    });
    let session = session.unwrap();
    wait_for_probe(&mut protocol, &mut pipe, &session, &mut probe_request, 2);
    protocol.command(DebugCommand::Pause).unwrap();
    let mut current_session = Some(session.clone());
    let suspended = drive(&mut protocol, &mut pipe, &mut current_session, |snapshot| {
        snapshot.state == DebugState::Paused
    });
    assert!(
        suspended
            .frames
            .first()
            .is_some_and(|frame| frame.location.is_some()),
        "{suspended:?}"
    );
    let before = read_probe(&mut protocol, &mut pipe, &session, &mut probe_request).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let after = read_probe(&mut protocol, &mut pipe, &session, &mut probe_request).unwrap();
    assert_eq!(
        before, after,
        "Actual VM pause must freeze the authored timer"
    );
    assert_eq!(protocol.snapshot().state, DebugState::Paused);
    protocol.command(DebugCommand::Resume).unwrap();
    drive(&mut protocol, &mut pipe, &mut current_session, |snapshot| {
        snapshot.state == DebugState::Running && !snapshot.command_pending
    });
    assert!(protocol.snapshot().locals.is_empty());
    assert!(
        wait_for_probe(
            &mut protocol,
            &mut pipe,
            &session,
            &mut probe_request,
            after + 2
        ) > after
    );
    assert!(
        protocol.command(DebugCommand::StepInto).is_err(),
        "Stepping requires a VM pause"
    );
    protocol.disconnect("Test complete");
    assert!(protocol.command(DebugCommand::Pause).is_err());
    assert!(
        protocol
            .command(DebugCommand::SetBreakpoints(Vec::new()))
            .is_err()
    );
    assert!(!protocol.has_pending_work());
    assert_eq!(protocol.snapshot().state, DebugState::Disconnected);
    assert!(protocol.snapshot().locals.is_empty());
    drop(pipe);
    assert!(!profile.exists(), "owned profile must be removed");
    std::fs::remove_dir_all(output).unwrap();
}
