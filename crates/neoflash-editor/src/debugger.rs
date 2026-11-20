//! Test Movie debugger lifecycle. Authored documents never store debugger state.
use crate::app::{Editor, ScriptTarget};
use crate::debugger_protocol::{
    AuthoredBreakpoint, AuthoredLocation, DebugCommand, DebugInspection, DebugOutputKind,
    DebugProtocol, DebugSnapshot, DebugSource, DebugState, DebugWatchResult, WatchRequest,
    compile_watch_expression, parse_debug_scalar_value,
};
use crate::debugger_sources::{ScriptOrigin, read_export_sources};
use crate::debugger_transport::{BrowserPipe, browser_executable, file_url, private_directory};
use crate::output_pipeline::{OutputLocation, OutputSeverity, OutputSource};
use crate::panels::actions_sidebar::ScriptLocation;
use crate::workspace::PanelId;
use eframe::egui;
use neoflash_core::{Project, Script};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DebugEditTarget {
    ScopeVariable {
        scope_number: usize,
        variable_name: String,
    },
    ObjectProperty {
        object_id: String,
        property_name: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct DebugInlineEdit {
    pub row_key: String,
    pub target: DebugEditTarget,
    pub draft: String,
    pub focus: bool,
}

#[derive(Default)]
pub(crate) struct Debugger {
    session: Option<DebugSession>,
    project: Option<Project>,
    pub(crate) breakpoints: Vec<AuthoredBreakpoint>,
    scripts: Vec<ScriptOrigin>,
    last_status: String,
    last_messages: Vec<String>,
    last_message_seq: u64,
    last_output_events: Vec<String>,
    last_output_seq: u64,
    last_navigated_pause: Option<(u64, Option<usize>, Option<AuthoredLocation>)>,
    export_directory: Option<PathBuf>,
    pub(crate) startup_status: Option<DebuggerStartupStatus>,
    expanded_paths: BTreeSet<String>,
    requested_object_ids: BTreeSet<(u64, String)>,
    last_pause_epoch: u64,
    pub(crate) inline_edit: Option<DebugInlineEdit>,
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
    watch_results: Arc<Mutex<Vec<DebugWatchResult>>>,
    inspection: Arc<Mutex<DebugInspection>>,
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
        watches: Vec<WatchRequest>,
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let mut protocol = DebugProtocol::new(
            file_url(index).map_err(|e| e.to_string())?,
            sources,
            breakpoints,
        );
        protocol.set_watches(watches);
        protocol.set_pause_on_start(true);
        // Publish Connecting before the worker can be observed by the UI.
        // The queued handshake is sent only after its private pipe is ready.
        protocol.start();
        let snapshot = Arc::new(Mutex::new(protocol.snapshot()));
        let shared = Arc::clone(&snapshot);
        let watch_results = Arc::new(Mutex::new(protocol.watch_results()));
        let shared_watches = Arc::clone(&watch_results);
        let inspection = Arc::new(Mutex::new(protocol.inspection()));
        let shared_inspection = Arc::clone(&inspection);
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
                        let next_watches = protocol.watch_results();
                        let next_inspection = protocol.inspection();
                        let terminal =
                            matches!(next.state, DebugState::Disconnected | DebugState::Failed);
                        let mut changed = false;
                        if let Ok(mut watches) = shared_watches.lock()
                            && *watches != next_watches
                        {
                            *watches = next_watches;
                            changed = true;
                        }
                        if let Ok(mut insp) = shared_inspection.lock()
                            && *insp != next_inspection
                        {
                            *insp = next_inspection;
                            changed = true;
                        }
                        if let Ok(mut snapshot) = shared.lock()
                            && *snapshot != next
                        {
                            *snapshot = next;
                            changed = true;
                        }
                        if changed {
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
                if let Ok(mut watches) = shared_watches.lock() {
                    *watches = protocol.watch_results();
                }
                if let Ok(mut insp) = shared_inspection.lock() {
                    *insp = protocol.inspection();
                }
                if let Ok(mut snapshot) = shared.lock() {
                    *snapshot = protocol.snapshot();
                }
                ctx.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            snapshot,
            watch_results,
            inspection,
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
    pub(crate) fn watch_results(&self) -> Vec<DebugWatchResult> {
        self.session
            .as_ref()
            .and_then(|session| session.watch_results.lock().ok())
            .map(|w| w.clone())
            .unwrap_or_default()
    }
    pub(crate) fn inspection(&self) -> DebugInspection {
        self.session
            .as_ref()
            .and_then(|session| session.inspection.lock().ok())
            .map(|insp| insp.clone())
            .unwrap_or_default()
    }
    pub(crate) fn is_tree_expanded(&self, row_key: &str) -> bool {
        self.expanded_paths.contains(row_key)
    }
    pub(crate) fn toggle_tree_expanded(&mut self, row_key: &str) -> bool {
        if self.expanded_paths.remove(row_key) {
            if self
                .inline_edit
                .as_ref()
                .is_some_and(|e| e.row_key.starts_with(row_key))
            {
                self.inline_edit = None;
            }
            false
        } else {
            self.expanded_paths.insert(row_key.to_owned());
            true
        }
    }
    pub(crate) fn sync_ui_pause_state(
        &mut self,
        paused: bool,
        pause_epoch: u64,
        pause_generation: u64,
    ) {
        if !paused {
            self.requested_object_ids.clear();
            self.inline_edit = None;
            return;
        }
        if pause_epoch != self.last_pause_epoch {
            self.last_pause_epoch = pause_epoch;
            self.inline_edit = None;
        }
        self.requested_object_ids
            .retain(|(generation, _)| *generation == pause_generation);
    }
    pub(crate) fn mark_object_requested(&mut self, pause_generation: u64, object_id: &str) -> bool {
        self.requested_object_ids
            .insert((pause_generation, object_id.to_owned()))
    }
    #[cfg(test)]
    pub(crate) fn set_test_watch_results(&self, results: Vec<DebugWatchResult>) {
        if let Some(session) = &self.session
            && let Ok(mut watches) = session.watch_results.lock()
        {
            *watches = results;
        }
    }
    #[cfg(test)]
    pub(crate) fn active(&self) -> bool {
        self.session.is_some()
    }
    /// A terminal session remains available for its status, messages and stack presentation, but
    /// its worker has exited and its command channel must no longer be used.
    pub(crate) fn commandable(&self) -> bool {
        self.snapshot().is_some_and(|snapshot| {
            !matches!(
                snapshot.state,
                DebugState::Disconnected | DebugState::Failed
            )
        })
    }
    #[cfg(test)]
    pub(crate) fn has_presentation(&self) -> bool {
        self.active() || self.startup_status.is_some()
    }
    #[cfg(test)]
    pub(crate) fn install_test_session(
        &mut self,
        project: Project,
        snapshot: Arc<Mutex<DebugSnapshot>>,
        commands: mpsc::SyncSender<DebugCommand>,
        scripts: Vec<ScriptOrigin>,
    ) {
        self.stop();
        self.project = Some(project);
        self.scripts = scripts;
        self.session = Some(DebugSession {
            snapshot,
            watch_results: Arc::new(Mutex::new(Vec::new())),
            inspection: Arc::new(Mutex::new(DebugInspection::default())),
            commands,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            errors: Arc::new(Mutex::new(Vec::new())),
        });
    }
    pub(crate) fn stop(&mut self) {
        self.session = None;
        self.last_status.clear();
        self.last_messages.clear();
        self.last_message_seq = 0;
        self.last_output_events.clear();
        self.last_output_seq = 0;
        self.last_navigated_pause = None;
        self.startup_status = None;
        self.expanded_paths.clear();
        self.requested_object_ids.clear();
        self.last_pause_epoch = 0;
        self.inline_edit = None;
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
            Some(DebugState::Running) => {
                self.debug_command(DebugCommand::Pause);
                self.show_panel(PanelId::Debugger);
            }
            Some(DebugState::Paused | DebugState::Connecting) => {
                self.show_panel(PanelId::Debugger);
            }
            _ => self.start_debug_movie(ctx),
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
        let mut script_compile_error = false;
        let result = (|| -> Result<(), DebuggerStartupStatus> {
            let executable = browser_executable().map_err(DebuggerStartupStatus::NotAvailable)?;
            let output =
                private_directory().map_err(|e| DebuggerStartupStatus::Failed(e.to_string()))?;
            self.debugger.export_directory = Some(output.clone());
            let resolved_project = self.project.with_resolved_script_language();
            let index =
                neoflash_export::export_project(&resolved_project, &output, &Self::runtime_dir())
                    .map_err(|e| {
                    script_compile_error = matches!(e, neoflash_export::ExportError::Scripts(_));
                    DebuggerStartupStatus::Failed(e.to_string())
                })?;
            let (sources, scripts) =
                read_export_sources(&index).map_err(DebuggerStartupStatus::Failed)?;
            self.debugger.scripts = scripts;
            let initial_watches = self.build_debug_watch_requests();
            self.debugger.session = Some(
                DebugSession::launch(
                    executable,
                    &index,
                    sources,
                    self.debugger.breakpoints.clone(),
                    initial_watches,
                    ctx.clone(),
                )
                .map_err(DebuggerStartupStatus::Failed)?,
            );
            self.debugger.project = Some(self.project.clone());
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.message_event(
                    OutputSeverity::Info,
                    OutputSource::Debugger,
                    "Test Movie debugger connecting; execution will pause at an authored statement.",
                    None,
                );
                self.show_panel(PanelId::Debugger);
            }
            Err(status) => {
                if script_compile_error {
                    self.refresh_compiler_diagnostics();
                }
                self.record_debug_startup_failure(ctx, status);
                if script_compile_error {
                    self.show_panel(PanelId::CompilerErrors);
                }
            }
        }
    }

    fn record_debug_startup_failure(
        &mut self,
        _ctx: &egui::Context,
        status: DebuggerStartupStatus,
    ) {
        self.debugger.stop();
        self.message_event(
            OutputSeverity::Error,
            OutputSource::Debugger,
            format!("Cannot debug Test Movie: {}", status.reason()),
            None,
        );
        self.debugger.startup_status = Some(status);
        self.debugger.project = Some(self.project.clone());
        self.show_panel(PanelId::Debugger);
    }

    pub(crate) fn debug_command(&mut self, command: DebugCommand) -> bool {
        if self
            .debugger
            .project
            .as_ref()
            .is_some_and(|project| project != &self.project)
        {
            self.debugger.reset_document();
            self.message_event(
                OutputSeverity::Warning,
                OutputSource::Debugger,
                "Debug session stopped because the document changed. Start Debug options again to use the edited source.",
                None,
            );
            return false;
        }
        if !self.debugger.commandable() {
            return false;
        }
        if let Err(error) = self.debugger.send(command) {
            self.message_event(OutputSeverity::Error, OutputSource::Debugger, error, None);
            false
        } else {
            true
        }
    }

    /// Restart an existing live debugger for Control ▸ Test movie. Returning `false` lets the
    /// caller launch the normal Test Movie path when the retained session is terminal or its
    /// channel has already closed.
    pub(crate) fn restart_debug_movie_if_commandable(&mut self) -> bool {
        self.debug_command(DebugCommand::Restart)
    }

    fn resolve_debug_output_location(&self, authored: &AuthoredLocation) -> Option<OutputLocation> {
        let script_loc = self.resolve_debug_script_location(&authored.source)?;
        Some(self.output_location_from_script(
            &script_loc,
            Some(authored.line.max(1)),
            Some(authored.column.max(1)),
        ))
    }

    fn parse_fallback_debug_output_line(
        &self,
        line: &str,
    ) -> (OutputSeverity, Option<OutputLocation>) {
        fn parse_prefixed(line: &str, prefix: &str) -> Option<(String, usize)> {
            let rest = line.strip_prefix(prefix)?;
            let (inside, _) = rest.split_once("): ")?;
            let (src, line_str) = inside.rsplit_once(" · line ")?;
            let line_num = line_str.trim().parse::<usize>().ok()?;
            Some((src.to_owned(), line_num.max(1)))
        }

        if let Some((src, line_num)) = parse_prefixed(line, "Runtime error (") {
            let loc = self
                .resolve_debug_script_location(&src)
                .map(|sl| self.output_location_from_script(&sl, Some(line_num), Some(1)));
            return (OutputSeverity::Error, loc);
        }
        if line.starts_with("Runtime error:") {
            return (OutputSeverity::Error, None);
        }
        if let Some((src, line_num)) = parse_prefixed(line, "Warning (") {
            let loc = self
                .resolve_debug_script_location(&src)
                .map(|sl| self.output_location_from_script(&sl, Some(line_num), Some(1)));
            return (OutputSeverity::Warning, loc);
        }
        if line.starts_with("Warning:") {
            return (OutputSeverity::Warning, None);
        }
        (OutputSeverity::Trace, None)
    }

    pub(crate) fn poll_debugger(&mut self, ctx: &egui::Context) {
        if !cfg!(test)
            && std::env::var_os("NEOFLASH_SCREENSHOT").is_some()
            && std::env::var("NEOFLASH_SCREENSHOT_DEBUGGER").as_deref() == Ok("paused")
            && self.debugger.session.is_none()
        {
            self.install_screenshot_paused_debugger();
        }
        if self
            .debugger
            .project
            .as_ref()
            .is_some_and(|project| project != &self.project)
        {
            self.debugger.reset_document();
            self.message_event(
                OutputSeverity::Warning,
                OutputSource::Debugger,
                "Debug session stopped because the document changed. Start Debug options again to use the edited source.",
                None,
            );
        }
        let errors = self
            .debugger
            .session
            .as_ref()
            .and_then(|session| session.errors.lock().ok())
            .map(|mut errors| std::mem::take(&mut *errors))
            .unwrap_or_default();
        for error in errors {
            self.message_event(OutputSeverity::Error, OutputSource::Debugger, error, None);
        }
        if let Some(snapshot) = self.debugger.snapshot() {
            let status = format!("{:?}", snapshot.state);
            let mut auto_resumed = false;
            if status != self.debugger.last_status {
                self.debugger.last_status = status.clone();
                let state_severity = if snapshot.state == DebugState::Failed {
                    OutputSeverity::Error
                } else {
                    OutputSeverity::Info
                };
                self.message_event(
                    state_severity,
                    OutputSource::Debugger,
                    format!("Debugger: {status}"),
                    None,
                );
                if !snapshot.reason.is_empty() {
                    self.message_event(
                        state_severity,
                        OutputSource::Debugger,
                        snapshot.reason.clone(),
                        None,
                    );
                }
                if snapshot.state == DebugState::Paused {
                    self.refresh_debugger_watches_local();
                    let raw_watches = self.raw_debugger_watch_expressions();
                    let cdp_watches = self.debugger.watch_results();
                    if !raw_watches.is_empty() && cdp_watches.len() != raw_watches.len() {
                        self.sync_debug_watches();
                    }
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
                        auto_resumed = true;
                    }
                }
            }
            if snapshot.state == DebugState::Paused {
                let cdp_watches = self.debugger.watch_results();
                self.sync_watch_values_from_cdp(&cdp_watches);
            }
            // Protocol notes and browser console/exception events can arrive without changing
            // Paused/Running. Drain strictly by monotonic `seq` when populated by `DebugProtocol`
            // (so ring rollover, identical lines, or Output Clear never duplicate or drop items),
            // and fall back to suffix/prefix overlap when unit tests populate string lists directly.
            if !snapshot.structured_messages.is_empty() {
                for (seq, message) in &snapshot.structured_messages {
                    if *seq > self.debugger.last_message_seq {
                        self.debugger.last_message_seq = *seq;
                        self.message_event(
                            OutputSeverity::Info,
                            OutputSource::Debugger,
                            message.clone(),
                            None,
                        );
                    }
                }
                self.debugger.last_messages = snapshot.messages.clone();
            } else {
                let previous = &self.debugger.last_messages;
                let overlap = (0..=previous.len().min(snapshot.messages.len()))
                    .rev()
                    .find(|&count| previous[previous.len() - count..] == snapshot.messages[..count])
                    .unwrap_or(0);
                for message in &snapshot.messages[overlap..] {
                    self.message_event(
                        OutputSeverity::Info,
                        OutputSource::Debugger,
                        message.clone(),
                        None,
                    );
                }
                self.debugger.last_messages = snapshot.messages.clone();
            }

            if !snapshot.structured_output.is_empty() {
                for record in &snapshot.structured_output {
                    if record.seq > self.debugger.last_output_seq {
                        self.debugger.last_output_seq = record.seq;
                        let severity = match record.kind {
                            DebugOutputKind::Trace => OutputSeverity::Trace,
                            DebugOutputKind::Warning => OutputSeverity::Warning,
                            DebugOutputKind::Error => OutputSeverity::Error,
                        };
                        let loc = record
                            .location
                            .as_ref()
                            .and_then(|al| self.resolve_debug_output_location(al));
                        self.message_event(
                            severity,
                            OutputSource::Debugger,
                            record.formatted.clone(),
                            loc,
                        );
                    }
                }
                self.debugger.last_output_events = snapshot.output_events.clone();
            } else {
                let prev_output = &self.debugger.last_output_events;
                let output_overlap = (0..=prev_output.len().min(snapshot.output_events.len()))
                    .rev()
                    .find(|&count| {
                        prev_output[prev_output.len() - count..] == snapshot.output_events[..count]
                    })
                    .unwrap_or(0);
                for line in &snapshot.output_events[output_overlap..] {
                    let (severity, loc) = self.parse_fallback_debug_output_line(line);
                    self.message_event(severity, OutputSource::Debugger, line.clone(), loc);
                }
                self.debugger.last_output_events = snapshot.output_events.clone();
            }

            if snapshot.state != DebugState::Paused {
                self.debugger.last_navigated_pause = None;
            } else if !auto_resumed && !snapshot.command_pending {
                let paused_location = snapshot
                    .selected_frame
                    .and_then(|idx| snapshot.frames.get(idx))
                    .or_else(|| snapshot.frames.first())
                    .and_then(|f| f.location.clone());
                let pause_key = (
                    snapshot.epoch,
                    snapshot.selected_frame,
                    paused_location.clone(),
                );
                if self.debugger.last_navigated_pause.as_ref() != Some(&pause_key) {
                    self.debugger.last_navigated_pause = Some(pause_key);
                    if let Some(location) = paused_location {
                        self.navigate_actions_to_debug_location(ctx, &location, false);
                    }
                }
            }

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
                && !self.panels_hidden
                && self.workspace.is_panel_active(PanelId::Debugger)
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

    pub(crate) fn debug_source_language(
        &self,
        source: &str,
    ) -> Option<neoflash_core::ScriptLanguage> {
        self.debugger
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
            })
    }

    pub(crate) fn debug_source_is_actionscript(&self, source: &str) -> bool {
        matches!(
            self.debug_source_language(source),
            Some(
                neoflash_core::ScriptLanguage::ActionScript1
                    | neoflash_core::ScriptLanguage::ActionScript2
                    | neoflash_core::ScriptLanguage::ActionScript3
            )
        )
    }

    pub(crate) fn active_debug_watch_language(&self) -> neoflash_core::ScriptLanguage {
        if let Some(snapshot) = self.debugger.snapshot()
            && let Some(frame) = snapshot
                .selected_frame
                .and_then(|idx| snapshot.frames.get(idx))
                .or_else(|| snapshot.frames.first())
            && let Some(loc) = frame.location.as_ref()
            && let Some(lang) = self.debug_source_language(&loc.source)
        {
            return lang;
        }
        self.project.effective_script_language()
    }

    pub(crate) fn build_debug_watch_requests(&self) -> Vec<WatchRequest> {
        let language = self.active_debug_watch_language();
        self.raw_debugger_watch_expressions()
            .into_iter()
            .map(|expr| compile_watch_expression(&expr, language))
            .collect()
    }

    pub(crate) fn sync_debug_watches(&mut self) {
        if self.debugger.commandable() {
            let requests = self.build_debug_watch_requests();
            let _ = self.debug_command(DebugCommand::SetWatches(requests));
        }
    }

    pub(crate) fn evaluate_active_debug_watches(&mut self) {
        if self.debugger.commandable() {
            let requests = self.build_debug_watch_requests();
            let cdp_watches = self.debugger.watch_results();
            let matches_current = cdp_watches.len() == requests.len()
                && cdp_watches
                    .iter()
                    .zip(&requests)
                    .all(|(cur, req)| cur.expression == req.expression);
            if matches_current {
                let _ = self.debug_command(DebugCommand::EvaluateWatches);
            } else {
                let _ = self.debug_command(DebugCommand::SetWatches(requests));
            }
        }
    }

    pub(crate) fn ensure_debug_object_inspected(
        &mut self,
        pause_generation: u64,
        object_id: &str,
        already_loaded: bool,
    ) {
        if !already_loaded
            && self
                .debugger
                .mark_object_requested(pause_generation, object_id)
        {
            let _ = self.debug_command(DebugCommand::InspectObject {
                object_id: object_id.to_owned(),
            });
        }
    }

    pub(crate) fn commit_debug_inline_edit(&mut self, edit: DebugInlineEdit) -> bool {
        if let Err(reason) = parse_debug_scalar_value(&edit.draft) {
            self.message_event(OutputSeverity::Error, OutputSource::Debugger, reason, None);
            return false;
        }
        match edit.target {
            DebugEditTarget::ScopeVariable {
                scope_number,
                variable_name,
            } => self.debug_command(DebugCommand::SetVariableValue {
                scope_number,
                variable_name,
                new_value: edit.draft,
            }),
            DebugEditTarget::ObjectProperty {
                object_id,
                property_name,
            } => self.debug_command(DebugCommand::SetPropertyValue {
                object_id,
                property_name,
                new_value: edit.draft,
            }),
        }
    }

    fn install_screenshot_paused_debugger(&mut self) {
        use crate::debugger_protocol::{
            DebugFrame, DebugInspectableLocal, DebugInspectableWatch, DebugLocal,
            DebugObjectProperty,
        };
        let mut snapshot =
            DebugProtocol::new("file:///screenshot.html".into(), vec![], vec![]).snapshot();
        snapshot.state = DebugState::Paused;
        snapshot.reason = "breakpoint".into();
        snapshot.frames = vec![
            DebugFrame {
                function_name: "stepHero".into(),
                location: Some(AuthoredLocation {
                    source: "Scene 1 · Frame 12".into(),
                    line: 18,
                    column: 5,
                }),
                generated_url: "project.js".into(),
                generated_line: 42,
                generated_column: 5,
            },
            DebugFrame {
                function_name: "onEnterFrame".into(),
                location: Some(AuthoredLocation {
                    source: "Scene 1 · Frame 1".into(),
                    line: 6,
                    column: 1,
                }),
                generated_url: "project.js".into(),
                generated_line: 14,
                generated_column: 1,
            },
        ];
        snapshot.selected_frame = Some(0);
        snapshot.locals = vec![
            DebugLocal {
                scope: "local".into(),
                name: "score".into(),
                value: "1250".into(),
                kind: "number".into(),
                accessor: false,
            },
            DebugLocal {
                scope: "local".into(),
                name: "hero".into(),
                value: "{ name: \"astro\", pos: { x: 240, y: 180 }, active: true }".into(),
                kind: "object".into(),
                accessor: false,
            },
        ];
        let mut object_properties = std::collections::BTreeMap::new();
        object_properties.insert(
            "obj-hero".into(),
            vec![
                DebugObjectProperty {
                    name: "name".into(),
                    value: "\"astro\"".into(),
                    kind: "string".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
                DebugObjectProperty {
                    name: "pos".into(),
                    value: "{ x: 240, y: 180 }".into(),
                    kind: "object".into(),
                    subtype: None,
                    object_id: Some("obj-pos".into()),
                    accessor: false,
                    writable: false,
                },
                DebugObjectProperty {
                    name: "active".into(),
                    value: "true".into(),
                    kind: "boolean".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
            ],
        );
        object_properties.insert(
            "obj-pos".into(),
            vec![
                DebugObjectProperty {
                    name: "x".into(),
                    value: "240".into(),
                    kind: "number".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
                DebugObjectProperty {
                    name: "y".into(),
                    value: "180".into(),
                    kind: "number".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
            ],
        );
        object_properties.insert(
            "obj-waypoints".into(),
            vec![
                DebugObjectProperty {
                    name: "0".into(),
                    value: "120".into(),
                    kind: "number".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
                DebugObjectProperty {
                    name: "1".into(),
                    value: "240".into(),
                    kind: "number".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
            ],
        );
        let inspection = DebugInspection {
            pause_epoch: 1,
            pause_generation: 1,
            locals: vec![
                DebugInspectableLocal {
                    scope_number: 0,
                    scope: "local".into(),
                    name: "score".into(),
                    value: "1250".into(),
                    kind: "number".into(),
                    subtype: None,
                    object_id: None,
                    accessor: false,
                    writable: true,
                },
                DebugInspectableLocal {
                    scope_number: 0,
                    scope: "local".into(),
                    name: "hero".into(),
                    value: "{ name: \"astro\", pos: { x: 240, y: 180 }, active: true }".into(),
                    kind: "object".into(),
                    subtype: None,
                    object_id: Some("obj-hero".into()),
                    accessor: false,
                    writable: false,
                },
            ],
            watches: vec![DebugInspectableWatch {
                index: 0,
                expression: "waypoints".into(),
                object_id: Some("obj-waypoints".into()),
            }],
            object_properties,
        };
        if self.debugger_watches().is_empty() {
            self.add_debugger_watch("waypoints");
        }
        let watches = vec![DebugWatchResult {
            expression: "waypoints".into(),
            value: "[120, 240]".into(),
            is_error: false,
        }];
        let (commands, _rx) = mpsc::sync_channel(8);
        self.debugger.breakpoints = vec![AuthoredBreakpoint {
            source: "Scene 1 · Frame 12".into(),
            line: 18,
            column: None,
        }];
        self.debugger
            .expanded_paths
            .insert("frame:0:local:0:hero".into());
        self.debugger
            .expanded_paths
            .insert("frame:0:local:0:hero.pos".into());
        self.debugger
            .expanded_paths
            .insert("watch:0:waypoints".into());
        self.debugger.last_pause_epoch = 1;
        self.debugger.project = Some(self.project.clone());
        self.debugger.session = Some(DebugSession {
            snapshot: Arc::new(Mutex::new(snapshot)),
            watch_results: Arc::new(Mutex::new(watches)),
            inspection: Arc::new(Mutex::new(inspection)),
            commands,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            errors: Arc::new(Mutex::new(Vec::new())),
        });
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
                        .frame_script_for_layer(self.layer, self.frame)?
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
            let matched = self.debugger.scripts.iter().find(|origin| {
                origin.language == script.language && origin.source == script.source
            });
            let location = if let Some(origin) = matched {
                origin.location.clone()
            } else {
                self.authored_script_origin_label(self.script_target)?
            };
            (script.source.as_str(), location)
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

    fn authored_script_origin_label(&self, target: ScriptTarget) -> Option<String> {
        match target {
            ScriptTarget::Source => self
                .project
                .sources
                .iter()
                .find(|unit| Some(&unit.id) == self.selected_source.as_ref())
                .map(|unit| unit.path.clone()),
            ScriptTarget::Frame => self
                .symbol()
                .timeline
                .frame_script_for_layer(self.layer, self.frame)
                .map(|frame| format!("{} / frame {}", self.symbol().name, frame.frame + 1)),
            ScriptTarget::Object => {
                let element_id = self.selected_element()?.id.clone();
                let symbol = self.symbol();
                for layer in &symbol.timeline.layers {
                    for keyframe in &layer.keyframes {
                        for element in &keyframe.elements {
                            if element.id == element_id
                                && element
                                    .scripts
                                    .iter()
                                    .any(|handler| handler.event == self.script_event)
                            {
                                return Some(format!(
                                    "{} / {} / frame {} / {} / {:?}",
                                    symbol.name,
                                    layer.name,
                                    keyframe.frame + 1,
                                    element.name,
                                    self.script_event
                                ));
                            }
                        }
                    }
                }
                None
            }
        }
    }

    pub(crate) fn resolve_debug_script_location(&self, source: &str) -> Option<ScriptLocation> {
        if let Some(unit) = self.project.sources.iter().find(|unit| unit.path == source) {
            return Some(ScriptLocation::Source(unit.id.clone()));
        }
        for symbol in &self.project.symbols {
            for frame in &symbol.timeline.scripts {
                let frame_no = frame.frame + 1;
                if format!("{} / frame {}", symbol.name, frame_no) == source
                    || format!("{} · Frame {}", symbol.name, frame_no) == source
                    || format!("{} · frame {}", symbol.name, frame_no) == source
                {
                    return Some(ScriptLocation::Frame {
                        symbol: symbol.id.clone(),
                        layer: frame.layer.clone(),
                        frame: frame.frame,
                    });
                }
            }
            for layer in &symbol.timeline.layers {
                for keyframe in &layer.keyframes {
                    for element in &keyframe.elements {
                        for handler in &element.scripts {
                            if format!(
                                "{} / {} / frame {} / {} / {:?}",
                                symbol.name,
                                layer.name,
                                keyframe.frame + 1,
                                element.name,
                                handler.event
                            ) == source
                            {
                                return Some(ScriptLocation::Object {
                                    symbol: symbol.id.clone(),
                                    frame: keyframe.frame,
                                    element: element.id.clone(),
                                    event: handler.event,
                                });
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub(crate) fn navigate_actions_to_debug_location(
        &mut self,
        ctx: &egui::Context,
        location: &AuthoredLocation,
        always_focus_actions: bool,
    ) -> bool {
        let Some(script_loc) = self.resolve_debug_script_location(&location.source) else {
            return false;
        };
        let source = self.script_source_for(&script_loc).unwrap_or_default();
        self.open_script_location(script_loc);
        if always_focus_actions
            || !self
                .workspace
                .shares_dock_group(PanelId::Actions, PanelId::Debugger)
        {
            self.open_actions();
        }
        let editor_id = self.actions_editor_id();
        crate::sources::jump_to(
            ctx,
            editor_id,
            &source,
            location.line.max(1),
            location.column.max(1),
        );
        true
    }

    pub(crate) fn toggle_debug_breakpoint(&mut self, ctx: &egui::Context) {
        let Some(point) = self.debug_source_at_caret(ctx) else {
            self.message("Choose an authored script in Actions before toggling a breakpoint.");
            return;
        };
        self.toggle_breakpoint(&point.source, point.line);
        if self.debugger.commandable() {
            self.debug_command(DebugCommand::SetBreakpoints(
                self.debugger.breakpoints.clone(),
            ));
        }
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
    fn terminal_sessions_remain_presented_but_do_not_accept_restart_or_breakpoint_commands() {
        for state in [DebugState::Disconnected, DebugState::Failed] {
            let ctx = egui::Context::default();
            let mut editor = Editor::from_project(&ctx, Project::blank());
            let mut snapshot =
                DebugProtocol::new("file:///fixture.html".into(), vec![], vec![]).snapshot();
            snapshot.state = state;
            snapshot.reason = format!("terminal {state:?}");
            let (commands, receiver) = mpsc::sync_channel(4);
            editor.debugger.session = Some(DebugSession {
                snapshot: Arc::new(Mutex::new(snapshot)),
                watch_results: Arc::new(Mutex::new(vec![])),
                inspection: Arc::new(Mutex::new(DebugInspection::default())),
                commands,
                stop: Arc::new(AtomicBool::new(false)),
                worker: None,
                errors: Arc::new(Mutex::new(vec![])),
            });
            editor.debugger.project = Some(editor.project.clone());
            editor.debugger.breakpoints = vec![AuthoredBreakpoint {
                source: "Scene 1 / frame 1".into(),
                line: 1,
                column: None,
            }];

            assert!(editor.debugger.active());
            assert!(editor.debugger.has_presentation());
            assert!(!editor.debugger.commandable());
            assert!(
                !editor.restart_debug_movie_if_commandable(),
                "terminal {state:?} must fall through to the normal Test Movie launch"
            );
            assert!(receiver.try_recv().is_err());

            assert!(!editor.debug_command(DebugCommand::SetBreakpoints(
                editor.debugger.breakpoints.clone()
            )));
            assert!(
                receiver.try_recv().is_err(),
                "terminal {state:?} must not receive breakpoint synchronization"
            );
            assert_eq!(editor.debugger.snapshot().unwrap().state, state);
        }

        // A live session still takes the fast restart path and receives exactly one command.
        let ctx = egui::Context::default();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        let mut snapshot =
            DebugProtocol::new("file:///fixture.html".into(), vec![], vec![]).snapshot();
        snapshot.state = DebugState::Running;
        let (commands, receiver) = mpsc::sync_channel(4);
        editor.debugger.session = Some(DebugSession {
            snapshot: Arc::new(Mutex::new(snapshot)),
            watch_results: Arc::new(Mutex::new(vec![])),
            inspection: Arc::new(Mutex::new(DebugInspection::default())),
            commands,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            errors: Arc::new(Mutex::new(vec![])),
        });
        editor.debugger.project = Some(editor.project.clone());
        assert!(editor.debugger.commandable());
        assert!(editor.restart_debug_movie_if_commandable());
        assert!(matches!(receiver.try_recv(), Ok(DebugCommand::Restart)));
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
                watch_results: Arc::new(Mutex::new(vec![])),
                inspection: Arc::new(Mutex::new(DebugInspection::default())),
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
                assert!(!texts.iter().any(|text| {
                    text.galley
                        .job
                        .text
                        .starts_with("Watch expressions are not available yet.")
                }));
                assert!(
                    texts
                        .iter()
                        .any(|text| text.galley.job.text == "Expression")
                );
                assert!(
                    nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some("Add watch"))
                );
                assert!(
                    nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some("Evaluate watches"))
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
            editor.workspace.set_panel_visible(PanelId::Debugger, false);
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
                assert!(!texts.iter().any(|text| {
                    text.galley
                        .job
                        .text
                        .starts_with("Watch expressions are not available yet.")
                }));
                assert!(
                    texts
                        .iter()
                        .any(|text| text.galley.job.text == "Expression")
                );
                editor.debugger.stop();
                editor.poll_debugger(&ctx);
                assert!(!editor.debugger.has_presentation());
                assert!(editor.workspace.is_panel_active(PanelId::Debugger));
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
            watch_results: Arc::new(Mutex::new(vec![])),
            inspection: Arc::new(Mutex::new(DebugInspection::default())),
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
        editor.debugger.session = Some(
            DebugSession::launch(executable, &index, vec![], vec![], vec![], ctx.clone()).unwrap(),
        );
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

    #[test]
    fn t291_debugger_idle_docking_trace_routing_and_pause_navigation() {
        use crate::debugger_protocol::DebugFrame;
        use neoflash_core::{FrameScript, ScriptLanguage};

        fn render_debugger(editor: &mut Editor, ctx: &egui::Context) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = editor.debugger_contents(ui);
                    });
                },
            )
        }

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut editor = Editor::from_project(&ctx, Project::blank());
        editor.symbol_mut().timeline.scripts.push(FrameScript {
            layer: None,
            frame: 1,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: "var a = 1;\ntrace(a);\n".into(),
            },
        });
        let authored_source = format!("{} / frame 2", editor.symbol().name);
        assert!(editor.toggle_breakpoint(&authored_source, 2));

        // 1. Idle docked PanelId::Debugger stays visible, shows "Not running", enables Continue &
        // Toggle breakpoint, disables Stop debugging, and renders idle breakpoints.
        editor.show_panel(PanelId::Debugger);
        assert!(editor.workspace.is_panel_active(PanelId::Debugger));
        let idle_output = render_debugger(&mut editor, &ctx);
        let idle_nodes = &idle_output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        assert!(
            !idle_nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Continue"))
                .unwrap()
                .1
                .is_disabled()
        );
        assert!(
            !idle_nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Toggle breakpoint"))
                .unwrap()
                .1
                .is_disabled()
        );
        assert!(
            idle_nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Stop debugging"))
                .unwrap()
                .1
                .is_disabled()
        );
        let idle_texts: Vec<_> = idle_output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        assert!(idle_texts.iter().any(|t| t == "Not running"));
        assert!(
            idle_texts
                .iter()
                .any(|t| t == &format!("{authored_source} · line 2"))
        );
        assert!(editor.workspace.is_panel_active(PanelId::Debugger));

        // 2. Mapped trace/warning/exception output events route into Output once without duplicating,
        // and Paused + selected frame navigates Actions to the authored frame & line while keeping
        // Debugger open.
        let mut snapshot =
            DebugProtocol::new("file:///fixture.html".into(), vec![], vec![]).snapshot();
        snapshot.state = DebugState::Paused;
        snapshot.epoch = 1;
        snapshot.reason = "breakpoint".into();
        snapshot.output_events = vec![
            "1".into(),
            format!("Warning ({authored_source} · line 2): Step check"),
        ];
        snapshot.frames.push(DebugFrame {
            function_name: "frame2".into(),
            location: Some(AuthoredLocation {
                source: authored_source.clone(),
                line: 2,
                column: 1,
            }),
            generated_url: "bundle.js".into(),
            generated_line: 10,
            generated_column: 1,
        });
        snapshot.selected_frame = Some(0);
        let shared = Arc::new(Mutex::new(snapshot));
        let (commands, _receiver) = mpsc::sync_channel(4);
        editor.debugger.session = Some(DebugSession {
            snapshot: shared.clone(),
            watch_results: Arc::new(Mutex::new(vec![])),
            inspection: Arc::new(Mutex::new(DebugInspection::default())),
            commands,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            errors: Arc::new(Mutex::new(vec![])),
        });
        editor.debugger.project = Some(editor.project.clone());
        editor.script_target = ScriptTarget::Object;
        editor.frame = 0;

        editor.poll_debugger(&ctx);
        // Append another trace line and poll twice; previous lines must not duplicate.
        shared
            .lock()
            .unwrap()
            .output_events
            .push(format!("Runtime error ({authored_source} · line 2): Boom"));
        editor.poll_debugger(&ctx);
        editor.poll_debugger(&ctx);

        assert_eq!(
            editor.output.iter().filter(|l| l.as_str() == "1").count(),
            1
        );
        assert_eq!(
            editor
                .output
                .iter()
                .filter(
                    |l| l.as_str() == format!("Warning ({authored_source} · line 2): Step check")
                )
                .count(),
            1
        );
        assert_eq!(
            editor
                .output
                .iter()
                .filter(
                    |l| l.as_str() == format!("Runtime error ({authored_source} · line 2): Boom")
                )
                .count(),
            1
        );
        assert!(editor.script_target == ScriptTarget::Frame);
        assert_eq!(editor.frame, 1);
        if editor
            .workspace
            .shares_dock_group(PanelId::Actions, PanelId::Debugger)
        {
            // When docked together, pause keeps Debugger active while still positioning Actions.
            assert!(editor.workspace.is_panel_active(PanelId::Debugger));
            assert!(editor.navigate_actions_to_debug_location(
                &ctx,
                &AuthoredLocation {
                    source: authored_source.clone(),
                    line: 2,
                    column: 3,
                },
                true,
            ));
            assert!(editor.workspace.is_panel_active(PanelId::Actions));
        } else {
            assert!(editor.workspace.is_panel_active(PanelId::Actions));
            assert!(editor.workspace.is_panel_active(PanelId::Debugger));
        }
        let cursor = egui::text_edit::TextEditState::load(&ctx, editor.actions_editor_id())
            .and_then(|s| s.cursor.char_range())
            .expect("debugger pause places Actions caret on target line");
        assert_eq!(cursor.primary.index, "var a = 1;\n".len() + 2);

        editor.debugger.stop();
        assert!(editor.workspace.is_panel_visible(PanelId::Debugger));
    }

    #[test]
    fn t404_debugger_watch_panel_syncs_with_actions_and_fits_all_densities() {
        use crate::debugger_protocol::{DebugFrame, DebugWatchResult};
        use crate::sources::{DebuggerScope, DebuggerVariable};
        use neoflash_core::ScriptLanguage;

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        project.script_language = Some(ScriptLanguage::ActionScript2);
        let mut editor = Editor::from_project(&ctx, project);

        // 1. AuthoredPlayback fallback when no connected CDP session is active.
        editor.set_paused_debugger_scopes(vec![DebuggerScope {
            name: "Local".into(),
            variables: vec![DebuggerVariable::scalar("score", "20", "Number")],
        }]);
        editor.add_debugger_watch("score + 5");
        assert_eq!(editor.debugger_watches().len(), 1);
        assert_eq!(editor.debugger_watches()[0].last_value, "25");

        // 2. Connect a paused CDP session: adding/removing/evaluating watches dispatches SetWatches
        //    with AS2-translated JS expressions, and live CDP values sync back to both Debugger & Actions.
        let mut snapshot =
            DebugProtocol::new("file:///fixture.html".into(), vec![], vec![]).snapshot();
        snapshot.state = DebugState::Paused;
        snapshot.frames.push(DebugFrame {
            function_name: "onEnterFrame".into(),
            location: Some(AuthoredLocation {
                source: "Scene 1 / frame 1".into(),
                line: 1,
                column: 1,
            }),
            generated_url: "bundle.js".into(),
            generated_line: 1,
            generated_column: 1,
        });
        snapshot.selected_frame = Some(0);
        let shared = Arc::new(Mutex::new(snapshot));
        let (commands, receiver) = mpsc::sync_channel(16);
        editor.debugger.install_test_session(
            editor.project.clone(),
            shared.clone(),
            commands,
            vec![ScriptOrigin {
                language: ScriptLanguage::ActionScript2,
                source: "var score = 100;".into(),
                location: "Scene 1 / frame 1".into(),
            }],
        );
        editor
            .debugger
            .set_test_watch_results(vec![DebugWatchResult {
                expression: "score + 5".into(),
                value: "105".into(),
                is_error: false,
            }]);
        editor.poll_debugger(&ctx);
        assert_eq!(editor.debugger_watches()[0].last_value, "105");

        // Render across all 3 NF3 densities in a narrow 620 pt Debugger pane and verify zero overflow.
        for density in neoflash_ui::metrics::Density::ALL {
            neoflash_ui::metrics::set_density(density);
            let panel_width = 620.0;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(panel_width, 320.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let _ = editor.debugger_contents(ui);
                        });
                },
            );
            let nodes = &out.platform_output.accesskit_update.as_ref().unwrap().nodes;
            for label in ["Add watch", "Evaluate watches", "Remove watch"] {
                let bounds = nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(label))
                    .unwrap_or_else(|| panic!("missing `{label}` in {density:?}"))
                    .1
                    .bounds()
                    .unwrap();
                assert!(
                    bounds.x1 <= f64::from(panel_width) + 0.5,
                    "`{label}` overflowed narrow pane in {density:?}: x1={} > {panel_width}",
                    bounds.x1
                );
            }
        }
        neoflash_ui::metrics::set_density(neoflash_ui::metrics::Density::Comfortable);

        // Add a second watch via draft + clicking "Add watch" in panels/debugger.rs.
        editor.set_debugger_watch_draft("hero._x");
        let render_panel = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 420.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = ed.debugger_contents(ui);
                    });
                },
            )
        };
        let out = render_panel(&mut editor, vec![]);
        let add_bounds = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Add watch"))
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let add_pos = egui::pos2(
            ((add_bounds.x0 + add_bounds.x1) * 0.5) as f32,
            ((add_bounds.y0 + add_bounds.y1) * 0.5) as f32,
        );
        let _ = render_panel(
            &mut editor,
            vec![
                egui::Event::PointerMoved(add_pos),
                egui::Event::PointerButton {
                    pos: add_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = render_panel(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: add_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(editor.debugger_watches().len(), 2);
        assert_eq!(editor.debugger_watches()[1].expression, "hero._x");
        assert!(editor.debugger_watch_draft().is_empty());

        let mut last_watches = None;
        while let Ok(cmd) = receiver.try_recv() {
            if let DebugCommand::SetWatches(reqs) = cmd {
                last_watches = Some(reqs);
            }
        }
        let sent = last_watches.expect("adding watch must send SetWatches to CDP session");
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].expression, "hero._x");
        assert!(sent[1].js_expression.contains("__nfWatchProp"));

        // Clicking "Evaluate watches" sends EvaluateWatches when watch expressions already match.
        editor.debugger.set_test_watch_results(vec![
            DebugWatchResult {
                expression: "score + 5".into(),
                value: "105".into(),
                is_error: false,
            },
            DebugWatchResult {
                expression: "hero._x".into(),
                value: "240".into(),
                is_error: false,
            },
        ]);
        let out = render_panel(&mut editor, vec![]);
        let eval_bounds = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Evaluate watches"))
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let eval_pos = egui::pos2(
            ((eval_bounds.x0 + eval_bounds.x1) * 0.5) as f32,
            ((eval_bounds.y0 + eval_bounds.y1) * 0.5) as f32,
        );
        let _ = render_panel(
            &mut editor,
            vec![
                egui::Event::PointerMoved(eval_pos),
                egui::Event::PointerButton {
                    pos: eval_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let _ = render_panel(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: eval_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(matches!(
            receiver.try_recv(),
            Ok(DebugCommand::EvaluateWatches)
        ));
        assert_eq!(editor.debugger_watches()[1].last_value, "240");

        // Stopping the connected CDP session falls back cleanly to AuthoredPlayback watch values.
        editor.debugger.stop();
        editor.refresh_debugger_watches();
        assert_eq!(editor.debugger_watches()[0].last_value, "25");
    }

    #[test]
    #[cfg(unix)]
    fn t410_debugger_nested_tree_expansion_cycle_safety_and_property_editing() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut project = Project::blank();
        project.script_language = Some(ScriptLanguage::JavaScript);
        let mut editor = Editor::from_project(&ctx, project);
        editor.add_debugger_watch("items");
        editor.add_debugger_watch("hero.x + score");

        let fixture = StartupFixture(private_directory().unwrap());
        let executable = fixture.0.join("mock-cdp-browser");
        let index = fixture.0.join("index.html");
        let log_path = fixture.0.join("cdp_log.jsonl");
        std::fs::write(&index, "<!doctype html>").unwrap();
        let mock_py = r#"#!/usr/bin/env python3
import json, os, re, sys

inp = os.fdopen(3, "rb", buffering=0)
out = os.fdopen(4, "wb", buffering=0)
log_file = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cdp_log.jsonl")

state = {
    "step": 1,
    "score": 10,
    "caller_val": 77,
    "objects": {
        "obj-hero": {"x": 24, "flag": False, "label": "ship", "note": "init", "self": ("ref", "obj-hero"), "d1": ("ref", "obj-d1")},
        "obj-d1": {"d2": ("ref", "obj-d2")},
        "obj-d2": {"d3": ("ref", "obj-d3")},
        "obj-d3": {"d4": ("ref", "obj-d4")},
        "obj-d4": {"d5": ("ref", "obj-d5")},
        "obj-d5": {"d6": ("ref", "obj-d6")},
        "obj-d6": {"d7": ("ref", "obj-d7"), "leaf": 6},
        "obj-d7": {"deep": 7},
        "obj-items": {"0": 5, "1": 8},
        "obj-hero-step2": {"x": 500, "flag": True, "label": "next", "note": None},
    },
}

def send(msg):
    out.write(json.dumps(msg).encode("utf-8") + b"\x00")

def log_cmd(msg):
    with open(log_file, "a", encoding="utf-8") as fh:
        fh.write(json.dumps(msg) + "\n")

def encode_remote(val):
    if isinstance(val, tuple) and val[0] == "ref":
        oid = val[1]
        if oid == "obj-items":
            d = state["objects"][oid]
            return {
                "type": "object",
                "subtype": "array",
                "objectId": oid,
                "description": "Array(2)",
                "preview": {
                    "type": "object",
                    "subtype": "array",
                    "description": "Array(2)",
                    "overflow": False,
                    "properties": [
                        {"name": "0", "type": "number", "value": str(d["0"])},
                        {"name": "1", "type": "number", "value": str(d["1"])},
                    ],
                },
            }
        return {"type": "object", "objectId": oid, "className": "Object"}
    if val is None:
        return {"type": "object", "subtype": "null", "value": None}
    if isinstance(val, bool):
        return {"type": "boolean", "value": val}
    if isinstance(val, (int, float)):
        return {"type": "number", "value": val}
    return {"type": "string", "value": str(val)}

def emit_paused():
    hero_oid = "obj-hero" if state["step"] == 1 else "obj-hero-step2"
    send({
        "method": "Debugger.paused",
        "sessionId": "sess-1",
        "params": {
            "reason": "other",
            "callFrames": [
                {
                    "callFrameId": f"cf-0-{state['step']}",
                    "functionName": "tick",
                    "location": {"scriptId": "s-1", "lineNumber": state["step"], "columnNumber": 0},
                    "scopeChain": [
                        {"type": "with", "object": {"type": "object", "objectId": "scope-with"}},
                        {"type": "local", "object": {"type": "object", "objectId": f"scope-local-{hero_oid}"}},
                    ],
                },
                {
                    "callFrameId": f"cf-1-{state['step']}",
                    "functionName": "main",
                    "location": {"scriptId": "s-1", "lineNumber": 0, "columnNumber": 0},
                    "scopeChain": [
                        {"type": "local", "object": {"type": "object", "objectId": "scope-caller"}},
                    ],
                },
            ],
        },
    })

buf = b""
while True:
    chunk = inp.read(4096)
    if not chunk:
        break
    buf += chunk
    while b"\x00" in buf:
        raw, buf = buf.split(b"\x00", 1)
        if not raw:
            continue
        msg = json.loads(raw.decode("utf-8"))
        mid = msg.get("id")
        method = msg.get("method")
        params = msg.get("params") or {}
        if method == "Target.createTarget":
            send({"id": mid, "result": {"targetId": "t-1"}})
        elif method == "Target.attachToTarget":
            send({"id": mid, "result": {"sessionId": "sess-1"}})
        elif method in ("Runtime.enable", "Debugger.enable", "Page.enable", "Debugger.setBlackboxPatterns", "Debugger.setInstrumentationBreakpoint"):
            send({"id": mid, "result": {}})
        elif method == "Page.navigate":
            send({"id": mid, "result": {"frameId": "f-1"}})
            emit_paused()
        elif method == "Runtime.getProperties":
            log_cmd({"method": method, "params": params})
            oid = params.get("objectId", "")
            if oid.startswith("scope-local-"):
                hero_oid = oid[len("scope-local-"):]
                props = [
                    {"name": "score", "writable": True, "value": encode_remote(state["score"])},
                    {"name": "hero", "writable": True, "value": encode_remote(("ref", hero_oid))},
                ]
            elif oid == "scope-caller":
                props = [
                    {"name": "callerVal", "writable": True, "value": encode_remote(state["caller_val"])},
                ]
            else:
                obj = state["objects"].get(oid, {})
                props = [
                    {"name": k, "writable": True, "value": encode_remote(v)}
                    for k, v in obj.items()
                ]
            send({"id": mid, "result": {"result": props}})
        elif method == "Debugger.evaluateOnCallFrame":
            expr = params.get("expression", "")
            hero_oid = "obj-hero" if state["step"] == 1 else "obj-hero-step2"
            if expr == "items":
                send({"id": mid, "result": {"result": encode_remote(("ref", "obj-items"))}})
            elif expr == "hero.x + score":
                total = state["objects"][hero_oid]["x"] + state["score"]
                send({"id": mid, "result": {"result": encode_remote(total)}})
            else:
                send({"id": mid, "result": {"result": {"type": "undefined"}}})
        elif method == "Debugger.setVariableValue":
            log_cmd({"method": method, "params": params})
            if params.get("scopeNumber") == 1 and params.get("variableName") == "score":
                state["score"] = params["newValue"]["value"]
            send({"id": mid, "result": {}})
        elif method == "Runtime.callFunctionOn":
            log_cmd({"method": method, "params": params})
            oid = params.get("objectId", "")
            decl = params.get("functionDeclaration", "")
            m = re.search(r'this\[("(?:\\.|[^"])*")\]\s*=\s*v;', decl)
            if m and oid in state["objects"]:
                prop_name = json.loads(m.group(1))
                state["objects"][oid][prop_name] = params["arguments"][0]["value"]
            send({"id": mid, "result": {"result": {"type": "undefined"}}})
        elif method == "Debugger.stepOver":
            send({"id": mid, "result": {}})
            send({"method": "Debugger.resumed", "sessionId": "sess-1", "params": {}})
            state["step"] += 1
            emit_paused()
        elif method == "Debugger.resume":
            send({"id": mid, "result": {}})
            send({"method": "Debugger.resumed", "sessionId": "sess-1", "params": {}})
"#;
        std::fs::write(&executable, mock_py).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

        let initial_watches = editor.build_debug_watch_requests();
        editor.debugger.project = Some(editor.project.clone());
        editor.debugger.session = Some(
            DebugSession::launch(
                executable,
                &index,
                vec![],
                vec![],
                initial_watches,
                ctx.clone(),
            )
            .unwrap(),
        );

        let wait_until = |ed: &mut Editor, pred: &dyn Fn(&Editor) -> bool, desc: &str| {
            let deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < deadline {
                ed.poll_debugger(&ctx);
                if pred(ed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("timed out waiting for {desc}");
        };

        let render_panel = |ed: &mut Editor, events: Vec<egui::Event>| -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 720.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    ed.poll_debugger(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = ed.debugger_contents(ui);
                    });
                },
            )
        };

        let text_center = |out: &egui::FullOutput, needle: &str| -> Option<egui::Pos2> {
            out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == needle => {
                    let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                    Some(rect.center())
                }
                _ => None,
            })
        };

        let all_texts = |out: &egui::FullOutput| -> Vec<String> {
            out.shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                    _ => None,
                })
                .collect()
        };

        let click_pos = |ed: &mut Editor, pos: egui::Pos2| {
            let _ = render_panel(
                ed,
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
            let _ = render_panel(
                ed,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };

        // 1. Wait until paused with top-level Locals & Watches populated; no lazy child fetches yet!
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger.inspection().locals.len() == 2
                    && ed.debugger_watches().len() == 2
                    && ed.debugger_watches()[1].last_value == "34"
            },
            "initial paused locals and watches",
        );
        assert!(editor.debugger.inspection().object_properties.is_empty());

        let out = render_panel(&mut editor, vec![]);
        let texts = all_texts(&out);
        assert!(texts.iter().any(|t| t == "score"));
        assert!(texts.iter().any(|t| t == "▸ hero"));
        assert!(texts.iter().any(|t| t == "▸ items"));

        // 2. Click `▸ hero` in Locals AND `▸ items` in Watch -> lazy Runtime.getProperties!
        let hero_pos = text_center(&out, "▸ hero").expect("`▸ hero` row visible");
        click_pos(&mut editor, hero_pos);
        let _ = render_panel(&mut editor, vec![]);
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger
                    .inspection()
                    .object_properties
                    .contains_key("obj-hero")
            },
            "obj-hero Runtime.getProperties",
        );

        let out = render_panel(&mut editor, vec![]);
        let items_pos = text_center(&out, "▸ items").expect("`▸ items` watch row visible");
        click_pos(&mut editor, items_pos);
        let _ = render_panel(&mut editor, vec![]);
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger
                    .inspection()
                    .object_properties
                    .contains_key("obj-items")
            },
            "obj-items Runtime.getProperties",
        );

        let out = render_panel(&mut editor, vec![]);
        let texts = all_texts(&out);
        assert!(texts.iter().any(|t| t == "▾ hero"));
        assert!(texts.iter().any(|t| t == "    x"));
        assert!(
            texts.iter().any(|t| t == "    self"),
            "cycle child visible as leaf"
        );
        assert!(
            texts.iter().any(|t| t.starts_with("<circular>")),
            "cyclic objectId marked <circular> without expandable chevron"
        );
        assert!(texts.iter().any(|t| t == "▾ items"));
        assert!(texts.iter().any(|t| t == "    0"));

        // 3. Expand d1..d5 to verify bounded depth up to 6 levels (`d6` is depth 6, so `d7` at depth 7 is NOT expandable).
        for (depth, name, oid) in [
            (1usize, "d1", "obj-d1"),
            (2, "d2", "obj-d2"),
            (3, "d3", "obj-d3"),
            (4, "d4", "obj-d4"),
            (5, "d5", "obj-d5"),
        ] {
            let out = render_panel(&mut editor, vec![]);
            let label = format!("{}▸ {name}", "  ".repeat(depth));
            let pos = text_center(&out, &label).unwrap_or_else(|| panic!("missing `{label}`"));
            click_pos(&mut editor, pos);
            let _ = render_panel(&mut editor, vec![]);
            wait_until(
                &mut editor,
                &|ed| ed.debugger.inspection().object_properties.contains_key(oid),
                oid,
            );
        }
        let out = render_panel(&mut editor, vec![]);
        let texts = all_texts(&out);
        // Depth 6 (`d6` under `d5`) is rendered as non-expandable (`  d6` instead of `▸ d6`) so depth never exceeds 6!
        assert!(
            texts
                .iter()
                .any(|t| t == &format!("{}  d6", "  ".repeat(6)))
        );
        assert!(
            !texts
                .iter()
                .any(|t| t == &format!("{}▸ d6", "  ".repeat(6)))
        );

        // 4. Inline edit top-level local `score` -> 100 via Debugger.setVariableValue!
        let score_pos = text_center(&out, "score").expect("`score` row visible");
        click_pos(&mut editor, score_pos);
        let _ = render_panel(&mut editor, vec![]); // grants focus
        let _ = render_panel(
            &mut editor,
            vec![
                egui::Event::Text("100".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        wait_until(
            &mut editor,
            &|ed| ed.debugger_watches()[1].last_value == "124",
            "watch refresh after Debugger.setVariableValue",
        );

        // 5. Inline edit expanded nested object property `hero.x` -> 200 via Runtime.callFunctionOn,
        //    plus boolean (`true`), string (`"booster"`), null (`null`), and Watch array element `items[0]` -> 42!
        let out = render_panel(&mut editor, vec![]);
        let x_pos = text_center(&out, "    x").expect("`    x` child row visible");
        click_pos(&mut editor, x_pos);
        let _ = render_panel(&mut editor, vec![]);
        let _ = render_panel(
            &mut editor,
            vec![
                egui::Event::Text("200".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger_watches()[1].last_value == "300"
                    && ed
                        .debugger
                        .inspection()
                        .object_properties
                        .get("obj-hero")
                        .and_then(|p| p.iter().find(|i| i.name == "x"))
                        .is_some_and(|i| i.value == "200")
            },
            "Locals + Watch + expanded subtree refresh after Runtime.callFunctionOn(number)",
        );

        for (prop_label, literal, expected_display) in [
            ("    flag", "true", "true"),
            ("    label", "\"booster\"", "\"booster\""),
            ("    note", "null", "null"),
        ] {
            let out = render_panel(&mut editor, vec![]);
            let pos =
                text_center(&out, prop_label).unwrap_or_else(|| panic!("missing {prop_label}"));
            click_pos(&mut editor, pos);
            let _ = render_panel(&mut editor, vec![]);
            let _ = render_panel(
                &mut editor,
                vec![
                    egui::Event::Text(literal.into()),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let prop_name = prop_label.trim();
            wait_until(
                &mut editor,
                &|ed| {
                    ed.debugger
                        .inspection()
                        .object_properties
                        .get("obj-hero")
                        .and_then(|p| p.iter().find(|i| i.name == prop_name))
                        .is_some_and(|i| i.value == expected_display)
                },
                prop_name,
            );
        }

        // Also edit expanded Watch array child `items[0]` -> 42!
        let out = render_panel(&mut editor, vec![]);
        let item0_pos = text_center(&out, "    0").expect("Watch child `    0` visible");
        click_pos(&mut editor, item0_pos);
        let _ = render_panel(&mut editor, vec![]);
        let _ = render_panel(
            &mut editor,
            vec![
                egui::Event::Text("42".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        wait_until(
            &mut editor,
            &|ed| ed.debugger_watches()[0].last_value == "[42, 8]",
            "Watch array refresh after editing items[0]",
        );

        // Verify exact wire commands logged by the loopback CDP pipe!
        let log_raw = std::fs::read_to_string(&log_path).unwrap();
        assert!(log_raw.contains("\"Runtime.getProperties\""));
        assert!(log_raw.contains("\"ownProperties\": true"));
        assert!(log_raw.contains("\"accessorPropertiesOnly\": false"));
        assert!(log_raw.contains("\"generatePreview\": true"));
        assert!(log_raw.contains("\"Debugger.setVariableValue\""));
        assert!(log_raw.contains("\"Runtime.callFunctionOn\""));
        assert!(log_raw.contains("function(v) { this[\\\"x\\\"] = v; }"));

        // 6. StepOver clears pause-1 `obj-hero` and cleanly re-fetches `obj-hero-step2` (`x = 500`)!
        assert!(editor.debug_command(DebugCommand::StepOver));
        wait_until(
            &mut editor,
            &|ed| {
                !ed.debugger
                    .inspection()
                    .object_properties
                    .contains_key("obj-hero")
                    && ed.debugger_watches()[1].last_value == "600"
            },
            "step clears stale objectId",
        );
        let _ = render_panel(&mut editor, vec![]);
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger
                    .inspection()
                    .object_properties
                    .get("obj-hero-step2")
                    .and_then(|p| p.iter().find(|i| i.name == "x"))
                    .is_some_and(|i| i.value == "500")
            },
            "step re-fetches new pause objectId",
        );

        // 7. Call-frame switch (`SelectFrame(1)`) & `Resume` clear stale object properties cleanly!
        assert!(editor.debug_command(DebugCommand::SelectFrame(1)));
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger
                    .inspection()
                    .locals
                    .iter()
                    .any(|l| l.name == "callerVal")
                    && !ed
                        .debugger
                        .inspection()
                        .object_properties
                        .contains_key("obj-hero-step2")
            },
            "SelectFrame(1) clears frame-0 object properties",
        );
        assert!(editor.debug_command(DebugCommand::Resume));
        wait_until(
            &mut editor,
            &|ed| {
                ed.debugger
                    .snapshot()
                    .is_some_and(|s| s.state == DebugState::Running)
                    && ed.debugger.inspection().object_properties.is_empty()
            },
            "Resume clears paused inspection",
        );
        editor.debugger.stop();
    }
}
