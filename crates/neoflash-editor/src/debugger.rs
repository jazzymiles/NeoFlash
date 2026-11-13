//! Test Movie debugger lifecycle. Authored documents never store debugger state.
use crate::app::{Editor, ScriptTarget};
use crate::debugger_protocol::{
    AuthoredBreakpoint, DebugCommand, DebugProtocol, DebugSnapshot, DebugSource, DebugState,
};
use crate::debugger_sources::{ScriptOrigin, read_export_sources};
use crate::debugger_transport::{BrowserPipe, browser_executable, file_url, private_directory};
use eframe::egui;
use neoflash_core::{Project, Script};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Debugger {
    session: Option<DebugSession>,
    project: Option<Project>,
    pub(crate) breakpoints: Vec<AuthoredBreakpoint>,
    scripts: Vec<ScriptOrigin>,
    last_status: String,
    last_messages: Vec<String>,
    export_directory: Option<PathBuf>,
    close_panel: bool,
    pub(crate) startup_status: Option<DebuggerStartupStatus>,
}

/// Presentation only: these failures occur before any debug session exists.
#[derive(Clone, Debug)]
pub(crate) enum DebuggerStartupStatus {
    NotAvailable(String),
    Failed(String),
}

impl DebuggerStartupStatus {
    fn reason(&self) -> &str {
        match self {
            Self::NotAvailable(reason) | Self::Failed(reason) => reason,
        }
    }
}

struct DebugSession {
    snapshot: Arc<Mutex<DebugSnapshot>>,
    commands: mpsc::SyncSender<DebugCommand>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl DebugSession {
    fn launch(
        executable: PathBuf,
        index: &Path,
        sources: Vec<DebugSource>,
        breakpoints: Vec<AuthoredBreakpoint>,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let mut protocol = DebugProtocol::new(
            file_url(index).map_err(|e| e.to_string())?,
            sources,
            breakpoints,
        );
        protocol.set_pause_on_start(true);
        // Publish Connecting before the worker can be observed by the UI.
        // The queued handshake is sent only after its private pipe is ready.
        protocol.start();
        let snapshot = Arc::new(Mutex::new(protocol.snapshot()));
        let shared = Arc::clone(&snapshot);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let (commands, receiver) = mpsc::sync_channel(64);
        let errors = Arc::new(Mutex::new(Vec::new()));
        let worker_errors = Arc::clone(&errors);
        let worker = std::thread::Builder::new()
            .name("neoflash-debugger".into())
            .spawn(move || {
                let result = (|| -> Result<(), String> {
                    let mut pipe =
                        BrowserPipe::launch(&executable, false).map_err(|e| e.to_string())?;
                    let mut last_progress = Instant::now();
                    while !stopping.load(Ordering::Relaxed) {
                        for _ in 0..64 {
                            match receiver.try_recv() {
                                Ok(command) => {
                                    last_progress = Instant::now();
                                    if let Err(error) = protocol.command(command)
                                        && let Ok(mut errors) = worker_errors.lock()
                                        && errors.len() < 32
                                    {
                                        errors.push(error);
                                    }
                                }
                                Err(mpsc::TryRecvError::Empty) => break,
                                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                            }
                        }
                        for command in protocol.drain_commands() {
                            pipe.send(&command).map_err(|e| e.to_string())?;
                        }
                        if let Some(message) = pipe.receive().map_err(|e| e.to_string())? {
                            last_progress = Instant::now();
                            protocol.receive(protocol.epoch(), message);
                        }
                        if protocol.has_pending_work()
                            && last_progress.elapsed() > Duration::from_secs(20)
                        {
                            return Err(
                                "Test Movie debugger timed out waiting for the browser.".into()
                            );
                        }
                        let next = protocol.snapshot();
                        let terminal =
                            matches!(next.state, DebugState::Disconnected | DebugState::Failed);
                        if let Ok(mut snapshot) = shared.lock()
                            && *snapshot != next
                        {
                            *snapshot = next;
                            ctx.request_repaint();
                        }
                        if terminal {
                            return Ok(());
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    protocol.disconnect(error);
                } else if !matches!(
                    protocol.snapshot().state,
                    DebugState::Disconnected | DebugState::Failed
                ) {
                    protocol.disconnect("Debug session stopped");
                }
                if let Ok(mut snapshot) = shared.lock() {
                    *snapshot = protocol.snapshot();
                }
                ctx.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            snapshot,
            commands,
            stop,
            worker: Some(worker),
            errors,
        })
    }
}

impl Drop for DebugSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Debugger {
    pub(crate) fn snapshot(&self) -> Option<DebugSnapshot> {
        self.session
            .as_ref()?
            .snapshot
            .lock()
            .ok()
            .map(|s| s.clone())
    }
    pub(crate) fn active(&self) -> bool {
        self.session.is_some()
    }
    pub(crate) fn has_presentation(&self) -> bool {
        self.active() || self.startup_status.is_some()
    }
    pub(crate) fn stop(&mut self) {
        self.close_panel = true;
        self.session = None;
        self.last_status.clear();
        self.last_messages.clear();
        self.startup_status = None;
        if let Some(directory) = self.export_directory.take() {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
    pub(crate) fn reset_document(&mut self) {
        self.stop();
        self.project = None;
        self.breakpoints.clear();
        self.scripts.clear();
    }
    fn send(&self, command: DebugCommand) -> Result<(), String> {
        self.session
            .as_ref()
            .ok_or("No Test Movie debug session")?
            .commands
            .try_send(command)
            .map_err(|_| "Debugger is disconnected or busy; try again.".into())
    }
}

impl Drop for Debugger {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Editor {
    pub(crate) fn open_debug_options(&mut self, ctx: &egui::Context) {
        match self.debugger.snapshot().map(|snapshot| snapshot.state) {
            Some(DebugState::Running) => self.debug_command(DebugCommand::Pause),
            Some(DebugState::Paused | DebugState::Connecting) => {}
            _ => self.start_debug_movie(ctx),
        }
        if self.debugger.has_presentation() {
            crate::panels::planned::set_open(
                ctx,
                crate::panels::planned::PlannedPanel::Debugger,
                true,
            );
        }
    }

    pub(crate) fn start_debug_movie(&mut self, ctx: &egui::Context) {
        self.finish_edits();
        if self
            .debugger
            .project
            .as_ref()
            .is_some_and(|project| project != &self.project)
        {
            self.debugger.reset_document();
        }
        self.debugger.stop();
        let result = (|| -> Result<(), DebuggerStartupStatus> {
            let executable = browser_executable().map_err(DebuggerStartupStatus::NotAvailable)?;
            let output =
                private_directory().map_err(|e| DebuggerStartupStatus::Failed(e.to_string()))?;
            self.debugger.export_directory = Some(output.clone());
            let index =
                neoflash_export::export_project(&self.project, &output, &Self::runtime_dir())
                    .map_err(|e| DebuggerStartupStatus::Failed(e.to_string()))?;
            let (sources, scripts) =
                read_export_sources(&index).map_err(DebuggerStartupStatus::Failed)?;
            self.debugger.scripts = scripts;
            self.debugger.session = Some(
                DebugSession::launch(
                    executable,
                    &index,
                    sources,
                    self.debugger.breakpoints.clone(),
                    ctx.clone(),
                )
                .map_err(DebuggerStartupStatus::Failed)?,
            );
            self.debugger.project = Some(self.project.clone());
            self.debugger.close_panel = false;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.message("Test Movie debugger connecting; execution will pause at an authored statement.");
                crate::panels::planned::set_open(
                    ctx,
                    crate::panels::planned::PlannedPanel::Debugger,
                    true,
                );
            }
            Err(status) => self.record_debug_startup_failure(ctx, status),
        }
        self.show_panel(crate::workspace::PanelId::Output);
    }

    fn record_debug_startup_failure(&mut self, ctx: &egui::Context, status: DebuggerStartupStatus) {
        self.debugger.stop();
        self.message(format!("Cannot debug Test Movie: {}", status.reason()));
        self.debugger.startup_status = Some(status);
        self.debugger.project = Some(self.project.clone());
        self.debugger.close_panel = false;
        crate::panels::planned::set_open(ctx, crate::panels::planned::PlannedPanel::Debugger, true);
    }

    pub(crate) fn debug_command(&mut self, command: DebugCommand) {
        if self
            .debugger
            .project
            .as_ref()
            .is_some_and(|project| project != &self.project)
        {
            self.debugger.reset_document();
            self.message("Debug session stopped because the document changed. Start Debug options again to use the edited source.");
            return;
        }
        if let Err(error) = self.debugger.send(command) {
            self.message(error);
        }
    }

    pub(crate) fn poll_debugger(&mut self, ctx: &egui::Context) {
        if self
            .debugger
            .project
            .as_ref()
            .is_some_and(|project| project != &self.project)
        {
            self.debugger.reset_document();
            self.message("Debug session stopped because the document changed. Start Debug options again to use the edited source.");
        }
        if self.debugger.close_panel {
            crate::panels::planned::set_open(
                ctx,
                crate::panels::planned::PlannedPanel::Debugger,
                false,
            );
            self.debugger.close_panel = false;
        }
        let errors = self
            .debugger
            .session
            .as_ref()
            .and_then(|session| session.errors.lock().ok())
            .map(|mut errors| std::mem::take(&mut *errors))
            .unwrap_or_default();
        for error in errors {
            self.message(error);
        }
        if let Some(snapshot) = self.debugger.snapshot() {
            let status = format!("{:?}", snapshot.state);
            if status != self.debugger.last_status {
                self.debugger.last_status = status.clone();
                self.message(format!("Debugger: {status}"));
                if !snapshot.reason.is_empty() {
                    self.message(snapshot.reason.clone());
                }
                if snapshot.state == DebugState::Paused {
                    self.refresh_debugger_watches();
                    if !snapshot.command_pending
                        && let Some(loc) = snapshot
                            .selected_frame
                            .and_then(|idx| snapshot.frames.get(idx))
                            .or_else(|| snapshot.frames.first())
                            .and_then(|f| f.location.as_ref())
                        && self.has_breakpoint(&loc.source, loc.line)
                        && !self.should_pause_at_breakpoint(&loc.source, loc.line)
                    {
                        self.debug_command(DebugCommand::Resume);
                    }
                }
            }
            // Protocol notes can arrive without changing Paused/Running. The
            // bounded list can roll over, so compare suffix/prefix overlap.
            let previous = &self.debugger.last_messages;
            let overlap = (0..=previous.len().min(snapshot.messages.len()))
                .rev()
                .find(|&count| previous[previous.len() - count..] == snapshot.messages[..count])
                .unwrap_or(0);
            for message in &snapshot.messages[overlap..] {
                self.message(message.clone());
            }
            self.debugger.last_messages = snapshot.messages.clone();
            if !matches!(
                snapshot.state,
                DebugState::Disconnected | DebugState::Failed
            ) {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
            // R-033 gives a paused session first claim on F8, even when its panel is hidden.
            // Consume it while a command is pending too, so it cannot become Next problem or
            // Convert to Symbol during the brief wait for the browser's acknowledgement.
            if !self.has_modal_dialog_in(ctx)
                && snapshot.state == DebugState::Paused
                && ctx.input_mut(|input| {
                    let mut consumed = false;
                    input.events.retain(|event| {
                        let plain_f8 = matches!(event, egui::Event::Key {
                            key: egui::Key::F8,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.is_none());
                        consumed |= plain_f8;
                        !plain_f8
                    });
                    consumed
                })
                && !snapshot.command_pending
            {
                self.debug_command(DebugCommand::Resume);
            }
            if !self.has_modal_dialog_in(ctx)
                && crate::panels::planned::is_open(
                    ctx,
                    crate::panels::planned::PlannedPanel::Debugger,
                )
                && !snapshot.command_pending
            {
                if snapshot.state == crate::debugger_protocol::DebugState::Paused {
                    let command = ctx.input_mut(|input| {
                        if input.consume_key(egui::Modifiers::NONE, egui::Key::F10) {
                            Some(DebugCommand::StepOver)
                        } else if input.consume_key(egui::Modifiers::SHIFT, egui::Key::F11) {
                            Some(DebugCommand::StepOut)
                        } else if input.consume_key(egui::Modifiers::NONE, egui::Key::F11) {
                            Some(DebugCommand::StepInto)
                        } else {
                            None
                        }
                    });
                    if let Some(command) = command {
                        self.debug_command(command);
                    }
                }
                if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::B))
                {
                    self.toggle_debug_breakpoint(ctx);
                }
            }
        }
    }

    pub(crate) fn debug_source_is_actionscript(&self, source: &str) -> bool {
        let language = self
            .debugger
            .scripts
            .iter()
            .find(|script| script.location == source)
            .map(|script| script.language)
            .or_else(|| {
                self.project
                    .sources
                    .iter()
                    .find(|unit| unit.path == source)
                    .map(|unit| unit.language)
            });
        matches!(
            language,
            Some(
                neoflash_core::ScriptLanguage::ActionScript1
                    | neoflash_core::ScriptLanguage::ActionScript2
                    | neoflash_core::ScriptLanguage::ActionScript3
            )
        )
    }

    fn debug_source_at_caret(&self, ctx: &egui::Context) -> Option<AuthoredBreakpoint> {
        let (source, name) = if self.script_target == ScriptTarget::Source {
            let unit = self
                .project
                .sources
                .iter()
                .find(|unit| Some(&unit.id) == self.selected_source.as_ref())?;
            (unit.source.as_str(), unit.path.clone())
        } else {
            let script: &Script = match self.script_target {
                ScriptTarget::Frame => {
                    &self
                        .symbol()
                        .timeline
                        .scripts
                        .iter()
                        .find(|frame| frame.frame == self.frame)?
                        .script
                }
                ScriptTarget::Object => {
                    &self
                        .selected_element()?
                        .scripts
                        .iter()
                        .find(|handler| handler.event == self.script_event)?
                        .script
                }
                ScriptTarget::Source => unreachable!(),
            };
            let origin = self.debugger.scripts.iter().find(|origin| {
                origin.language == script.language && origin.source == script.source
            })?;
            (script.source.as_str(), origin.location.clone())
        };
        let offset = egui::text_edit::TextEditState::load(ctx, self.actions_editor_id())
            .and_then(|s| s.cursor.char_range())
            .map_or(0, |cursor| cursor.primary.index);
        let line = 1 + source.chars().take(offset).filter(|c| *c == '\n').count();
        Some(AuthoredBreakpoint {
            source: name,
            line,
            column: None,
        })
    }

    pub(crate) fn toggle_debug_breakpoint(&mut self, ctx: &egui::Context) {
        let Some(point) = self.debug_source_at_caret(ctx) else {
            self.message("Choose an authored script in Actions before toggling a breakpoint.");
            return;
        };
        self.toggle_breakpoint(&point.source, point.line);
        self.debug_command(DebugCommand::SetBreakpoints(
            self.debugger.breakpoints.clone(),
        ));
    }

    /// Evaluate a live debugger step/run arrival at `(source, line)` with `scopes`, updating
    /// paused scopes, refreshing watches, and returning `true` when breakpoint conditions and
    /// hit-count targets dictate pausing at `(source, line)`.
    #[allow(dead_code)]
    pub(crate) fn evaluate_debugger_step(
        &mut self,
        source: &str,
        line: usize,
        scopes: Vec<crate::sources::DebuggerScope>,
    ) -> bool {
        self.set_paused_debugger_scopes(scopes);
        self.should_pause_at_breakpoint(source, line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use neoflash_core::ScriptLanguage;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[cfg(unix)]
    struct StartupFixture(PathBuf);

    #[cfg(unix)]
    impl Drop for StartupFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn remembered_session(editor: &mut Editor) {
        editor.debugger.project = Some(editor.project.clone());
        editor.debugger.breakpoints = vec![AuthoredBreakpoint {
            source: "Frame 1".into(),
            line: 1,
            column: None,
        }];
        editor.debugger.scripts = vec![ScriptOrigin {
            source: "trace('test');".into(),
            language: ScriptLanguage::ActionScript3,
            location: "Frame 1".into(),
        }];
    }

    #[test]
    fn approved_debugger_states_bind_live_data_and_gate_step_controls() {
        use crate::debugger_protocol::{DebugFrame, DebugLocal};

        fn render(
            editor: &mut Editor,
            ctx: &egui::Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 500.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        assert!(!editor.debugger_contents(ui));
                    });
                },
            )
        }

        for theme in [
            crate::theme::ThemeMode::Dark,
            crate::theme::ThemeMode::Light,
        ] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut editor = Editor::from_project(&ctx, Project::blank());
            theme.apply(&ctx);
            let original = editor.project.clone();
            let mut snapshot =
                DebugProtocol::new("file:///fixture.html".into(), vec![], vec![]).snapshot();
            snapshot.frames.push(DebugFrame {
                function_name: "fixtureFunction".into(),
                location: None,
                generated_url: "fixture.js".into(),
                generated_line: 1,
                generated_column: 1,
            });
            snapshot.selected_frame = Some(0);
            snapshot.locals.push(DebugLocal {
                scope: "Local".into(),
                name: "fixtureLocal".into(),
                value: "7".into(),
                kind: "number".into(),
                accessor: false,
            });
            let shared = Arc::new(Mutex::new(snapshot));
            let (commands, receiver) = mpsc::sync_channel(4);
            editor.debugger.session = Some(DebugSession {
                snapshot: shared.clone(),
                commands,
                stop: Arc::new(AtomicBool::new(false)),
                worker: None,
                errors: Arc::new(Mutex::new(vec![])),
            });
            editor.debugger.project = Some(editor.project.clone());
            for (state, pending) in [
                (DebugState::Connecting, false),
                (DebugState::Running, false),
                (DebugState::Disconnected, false),
                (DebugState::Failed, false),
                (DebugState::Paused, true),
                (DebugState::Paused, false),
            ] {
                {
                    let mut snapshot = shared.lock().unwrap();
                    snapshot.state = state;
                    snapshot.command_pending = pending;
                    snapshot.reason = if state == DebugState::Failed {
                        "Fixture browser refused the connection.".into()
                    } else {
                        String::new()
                    };
                }
                let output = render(&mut editor, &ctx, vec![]);
                let nodes = &output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes;
                for label in ["Continue", "Step over", "Step in", "Step out"] {
                    let node = &nodes
                        .iter()
                        .find(|(_, node)| node.label() == Some(label))
                        .unwrap()
                        .1;
                    assert_eq!(
                        node.is_disabled(),
                        state != DebugState::Paused || pending,
                        "{state:?}: {label}"
                    );
                }
                assert_eq!(
                    nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some("Restart")),
                    state == DebugState::Disconnected,
                );
                assert_eq!(
                    nodes.iter().any(|(_, node)| node.label() == Some("Retry")),
                    state == DebugState::Failed,
                );
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text),
                        _ => None,
                    })
                    .collect();
                let palette = neoflash_ui::palette(&ctx);
                if state == DebugState::Failed {
                    assert!(!texts.iter().any(|text| text.galley.job.text == "Failed"));
                    let error = texts
                        .iter()
                        .find(|text| {
                            text.galley.job.text == "Fixture browser refused the connection."
                        })
                        .unwrap();
                    assert!(
                        error
                            .galley
                            .job
                            .sections
                            .iter()
                            .all(|section| { section.format.color == palette.danger_default })
                    );
                } else {
                    let status = texts
                        .iter()
                        .find(|text| text.galley.job.text == format!("{state:?}"))
                        .unwrap();
                    let expected = match state {
                        DebugState::Connecting | DebugState::Disconnected => palette.text_secondary,
                        DebugState::Running => palette.accent_default,
                        _ => palette.warning_default,
                    };
                    assert!(
                        status
                            .galley
                            .job
                            .sections
                            .iter()
                            .all(|section| section.format.color == expected)
                    );
                }
                assert!(texts.iter().any(|text| {
                    text.galley
                        .job
                        .text
                        .starts_with("Watch expressions are not available yet.")
                }));
                assert!(
                    !texts
                        .iter()
                        .any(|text| text.galley.job.text == "Expression")
                );
                assert_eq!(
                    texts
                        .iter()
                        .any(|text| text.galley.job.text == "fixtureLocal"),
                    state == DebugState::Paused
                );
                assert_eq!(
                    texts
                        .iter()
                        .any(|text| text.galley.job.text == "Pause to inspect variables."),
                    state != DebugState::Paused
                );
                assert_eq!(
                    texts
                        .iter()
                        .any(|text| text.galley.job.text == "Pause to view the call stack."),
                    state != DebugState::Paused
                );
            }
            let output = render(&mut editor, &ctx, vec![]);
            let bounds = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Step over"))
                .unwrap()
                .1
                .bounds()
                .unwrap();
            let pos = egui::pos2(
                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                ((bounds.y0 + bounds.y1) * 0.5) as f32,
            );
            let _ = render(
                &mut editor,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let _ = render(
                &mut editor,
                &ctx,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(matches!(receiver.try_recv(), Ok(DebugCommand::StepOver)));
            // A hidden debugger still owns F8 before the global and Actions shortcut routes.
            crate::panels::planned::set_open(
                &ctx,
                crate::panels::planned::PlannedPanel::Debugger,
                false,
            );
            for pending in [false, true] {
                shared.lock().unwrap().command_pending = pending;
                let _ = ctx.run(
                    egui::RawInput {
                        events: vec![egui::Event::Key {
                            key: egui::Key::F8,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        ..Default::default()
                    },
                    |ctx| {
                        editor.poll_debugger(ctx);
                        assert!(!ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::F8)
                        }));
                    },
                );
                if pending {
                    assert!(receiver.try_recv().is_err());
                } else {
                    assert!(matches!(receiver.try_recv(), Ok(DebugCommand::Resume)));
                }
            }
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::F8,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::SHIFT,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    editor.poll_debugger(ctx);
                    assert!(ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::SHIFT, egui::Key::F8)
                    }));
                },
            );
            assert!(receiver.try_recv().is_err());
            for startup in [
                DebuggerStartupStatus::NotAvailable(
                    "Install Chrome or Chromium to debug Test Movie.".into(),
                ),
                DebuggerStartupStatus::Failed("Fixture export could not be written.".into()),
            ] {
                let unavailable = matches!(startup, DebuggerStartupStatus::NotAvailable(_));
                let reason = startup.reason().to_owned();
                editor.record_debug_startup_failure(&ctx, startup);
                assert!(!editor.debugger.active(), "startup status is not a session");
                assert!(editor.debugger.snapshot().is_none());
                assert!(editor.debugger.has_presentation());
                let output = render(&mut editor, &ctx, vec![]);
                let nodes = &output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes;
                for label in [
                    "Continue",
                    "Step over",
                    "Step in",
                    "Step out",
                    "Toggle breakpoint",
                    "Remove all breakpoints",
                ] {
                    assert!(
                        nodes
                            .iter()
                            .find(|(_, node)| node.label() == Some(label))
                            .unwrap()
                            .1
                            .is_disabled(),
                        "startup {label}"
                    );
                }
                assert_eq!(
                    nodes.iter().any(|(_, node)| node.label() == Some("Retry")),
                    !unavailable
                );
                let texts: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    texts
                        .iter()
                        .any(|text| text.galley.job.text == "Not available"),
                    unavailable
                );
                let reason_text = texts
                    .iter()
                    .find(|text| text.galley.job.text == reason)
                    .unwrap();
                let palette = neoflash_ui::palette(&ctx);
                let color = if unavailable {
                    palette.text_secondary
                } else {
                    palette.danger_default
                };
                assert!(
                    reason_text
                        .galley
                        .job
                        .sections
                        .iter()
                        .all(|section| section.format.color == color)
                );
                assert!(texts.iter().any(|text| {
                    text.galley
                        .job
                        .text
                        .starts_with("Watch expressions are not available yet.")
                }));
                editor.debugger.stop();
                editor.poll_debugger(&ctx);
                assert!(!editor.debugger.has_presentation());
                assert!(!crate::panels::planned::is_open(
                    &ctx,
                    crate::panels::planned::PlannedPanel::Debugger
                ));
            }
            editor.record_debug_startup_failure(
                &ctx,
                DebuggerStartupStatus::Failed("Document-scoped failure.".into()),
            );
            editor.project.name.push_str(" changed");
            editor.poll_debugger(&ctx);
            assert!(!editor.debugger.has_presentation());
            editor.project = original.clone();
            assert_eq!(editor.project, original);
            assert!(!editor.history.can_undo());
        }
    }

    #[test]
    #[cfg(unix)]
    fn stopped_sessions_invalidate_after_edits_and_identical_document_switches() {
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::demo());
        let original = editor.project.clone();
        remembered_session(&mut editor);
        editor.debugger.stop();
        editor.poll_debugger(&ctx);
        assert_eq!(editor.project, original);
        assert!(!editor.history.can_undo());
        assert_eq!(editor.debugger.breakpoints.len(), 1);
        assert_eq!(editor.debugger.scripts.len(), 1);

        editor.transact(|e| e.project.name = "Edited after stopping".into());
        let edited = editor.project.clone();
        editor.poll_debugger(&ctx);
        assert_eq!(editor.project, edited);
        assert!(editor.debugger.project.is_none());
        assert!(editor.debugger.breakpoints.is_empty());
        assert!(editor.debugger.scripts.is_empty());
        editor.undo();
        assert_eq!(editor.project, original);
        editor.redo();
        assert_eq!(editor.project, edited);

        // An edit and a command can occur in the same UI update, before polling.
        remembered_session(&mut editor);
        let (commands, receiver) = mpsc::sync_channel(1);
        editor.debugger.session = Some(DebugSession {
            snapshot: Arc::new(Mutex::new(
                DebugProtocol::new("file:///fixture.html".into(), vec![], vec![]).snapshot(),
            )),
            commands,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            errors: Arc::new(Mutex::new(vec![])),
        });
        editor.transact(|e| e.project.name = "Same update edit".into());
        editor.debug_command(DebugCommand::Resume);
        assert!(!editor.debugger.active());
        assert!(editor.debugger.breakpoints.is_empty());
        assert!(
            receiver.try_recv().is_err(),
            "stale commands never reach the worker"
        );
        editor.undo();
        assert_eq!(editor.project, edited);

        remembered_session(&mut editor);
        editor.debugger.stop();
        editor.install_document(&ctx, edited.clone(), None, true);
        assert_eq!(editor.project, edited);
        assert!(!editor.history.can_undo());
        assert!(editor.debugger.project.is_none());
        assert!(editor.debugger.breakpoints.is_empty());
        assert!(editor.debugger.scripts.is_empty());
        assert!(!editor.debugger.active());
        editor.project.validate().unwrap();

        // Exercise the actual worker boundary, without launching a user's
        // browser. The fake endpoint waits until the UI has seen Connecting.
        let fixture = StartupFixture(private_directory().unwrap());
        let executable = fixture.0.join("browser");
        let index = fixture.0.join("index.html");
        std::fs::write(&index, "<!doctype html>").unwrap();
        std::fs::write(
            &executable,
            "#!/bin/sh\nfixture=${0%/*}\nwhile [ ! -f \"$fixture/reply\" ]; do /bin/sleep 0.01; done\nprintf '{\"id\":1,\"error\":{\"message\":\"fixture connection refused\"}}\\000' >&4\nexec /bin/sleep 60\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        editor.debugger.session =
            Some(DebugSession::launch(executable, &index, vec![], vec![], ctx.clone()).unwrap());
        assert_eq!(
            editor.debugger.snapshot().unwrap().state,
            DebugState::Connecting
        );
        let output_start = editor.output.len();
        editor.poll_debugger(&ctx);
        assert!(
            editor.output[output_start..]
                .iter()
                .any(|line| line == "Debugger: Connecting")
        );
        assert!(
            !editor.output[output_start..]
                .iter()
                .any(|line| line.contains("Disconnected"))
        );
        std::fs::write(fixture.0.join("reply"), "ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !editor
            .debugger
            .session
            .as_ref()
            .unwrap()
            .worker
            .as_ref()
            .unwrap()
            .is_finished()
        {
            assert!(
                Instant::now() < deadline,
                "failed protocol left its browser worker alive"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let snapshot = editor.debugger.snapshot().unwrap();
        assert_eq!(snapshot.state, DebugState::Failed);
        assert!(snapshot.reason.contains("fixture connection refused"));
        editor.poll_debugger(&ctx);
        assert!(
            editor
                .output
                .iter()
                .any(|line| line.contains("fixture connection refused"))
        );

        for messages in [
            vec!["first note", "second note"],
            vec!["second note", "third note"],
        ] {
            editor
                .debugger
                .session
                .as_ref()
                .unwrap()
                .snapshot
                .lock()
                .unwrap()
                .messages = messages.into_iter().map(str::to_owned).collect();
            editor.poll_debugger(&ctx);
            editor.poll_debugger(&ctx);
        }
        for note in ["first note", "second note", "third note"] {
            assert_eq!(
                editor
                    .output
                    .iter()
                    .filter(|line| line.as_str() == note)
                    .count(),
                1
            );
        }
        editor.debugger.stop();
        assert_eq!(editor.project, edited);
        assert!(!editor.history.can_undo());
    }
}
