//! Publication settings and a bounded background size estimate for the R-032 Publish dialog.
//! No UI or document state lives here. The worker uses the exporter's actual package assembly.
use crate::{app::Editor, workspace::PublishPrefs};
use eframe::egui;
use neoflash_core::Project;
use neoflash_export::{PublishOptions, SizeEstimate};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::SystemTime,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum EstimateState {
    #[default]
    Idle,
    Computing,
    Ready(SizeEstimate),
    Failed(String),
}

/// The two files read by the exporter. Replacing the installed runtime invalidates the estimate,
/// including a missing runtime becoming available. Compiled-in host code cannot change in-process.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeStamp {
    directory: PathBuf,
    files: [Option<RuntimeFileStamp>; 2],
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeFileStamp {
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl RuntimeStamp {
    fn read(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            files: ["neoflash_web.js", "neoflash_web_bg.wasm"].map(|name| {
                let metadata = std::fs::metadata(directory.join(name)).ok()?;
                #[cfg(unix)]
                use std::os::unix::fs::MetadataExt;
                Some(RuntimeFileStamp {
                    bytes: metadata.len(),
                    modified: metadata.modified().ok()?,
                    #[cfg(unix)]
                    identity: (
                        metadata.dev(),
                        metadata.ino(),
                        metadata.ctime(),
                        metadata.ctime_nsec(),
                    ),
                })
            }),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Request {
    project: Arc<Project>,
    options: PublishOptions,
    html_config: neoflash_export::HtmlPublishConfig,
    warnings_as_errors: bool,
    runtime: RuntimeStamp,
}

#[derive(Clone)]
struct Demand {
    runtime: PathBuf,
    preferences: PublishPrefs,
}

type Outcome = Result<Option<SizeEstimate>, String>;
struct Running {
    request: Arc<Request>,
    cancel: Arc<AtomicBool>,
    result: Receiver<Outcome>,
}

/// At most one worker and one replacement snapshot. Rapid edits replace the pending request;
/// cancellation does not join the worker on the UI thread or start unbounded concurrent work.
#[derive(Default)]
pub(crate) struct PublishController {
    current: Option<Arc<Request>>,
    pending: Option<Arc<Request>>,
    running: Option<Running>,
    state: EstimateState,
    /// None until the approved UI requests an estimate; ordinary editor frames do no size work.
    demand: Option<Demand>,
}

impl Drop for PublishController {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl PublishController {
    /// Refresh before reading `state`: compares the full authored snapshot, including pending
    /// edits that have not yet acquired a History revision, Undo, and document replacement.
    pub(crate) fn refresh(
        &mut self,
        project: &Project,
        prefs: &PublishPrefs,
        runtime: &Path,
        ctx: &egui::Context,
    ) -> &EstimateState {
        let resolved = project.with_resolved_script_language();
        let project = resolved.as_ref();
        self.demand = Some(Demand {
            runtime: runtime.to_owned(),
            preferences: prefs.clone(),
        });
        let stamp = RuntimeStamp::read(runtime);
        let html_config = prefs.publish_html_config();
        let matches = self.current.as_ref().is_some_and(|request| {
            request.project.as_ref() == project
                && request.options == prefs.options
                && request.html_config == html_config
                && request.warnings_as_errors == prefs.warnings_as_errors
                && request.runtime == stamp
        });
        if !matches {
            if let Some(running) = &self.running {
                running.cancel.store(true, Ordering::Relaxed);
            }
            let request = Arc::new(Request {
                project: Arc::new(project.clone()),
                options: prefs.options.clone(),
                html_config,
                warnings_as_errors: prefs.warnings_as_errors,
                runtime: stamp,
            });
            self.current = Some(request.clone());
            // Validate synchronously so an invalid URL never displays a previous valid total.
            match prefs.export_options(project) {
                Ok(_) => {
                    self.pending = Some(request);
                    self.state = EstimateState::Computing;
                }
                Err(error) => {
                    self.pending = None;
                    self.state = EstimateState::Failed(error);
                }
            }
        }
        self.poll(ctx);
        &self.state
    }

    /// Cancels a closed dialog/document's demand immediately. A running non-interruptible phase
    /// finishes in the background, then observes the flag; its result can never become visible.
    pub(crate) fn cancel(&mut self) {
        if let Some(running) = &self.running {
            running.cancel.store(true, Ordering::Relaxed);
        }
        self.current = None;
        self.pending = None;
        self.demand = None;
        self.state = EstimateState::Idle;
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(running) = &self.running {
            let result = match running.result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(
                    "Publish size worker stopped; reopen Publish settings to retry.".into(),
                )),
            };
            if let Some(result) = result {
                let running = self.running.take().unwrap();
                if !running.cancel.load(Ordering::Relaxed)
                    && self.current.as_ref() == Some(&running.request)
                {
                    self.state = match result {
                        Ok(Some(estimate)) => EstimateState::Ready(estimate),
                        Ok(None) => EstimateState::Idle,
                        Err(error) => EstimateState::Failed(error),
                    };
                }
            }
        }
        if self.running.is_none()
            && let Some(request) = self.pending.take()
        {
            let cancel = Arc::new(AtomicBool::new(false));
            let (sender, result) = mpsc::channel();
            let worker_request = request.clone();
            let worker_cancel = cancel.clone();
            let repaint = ctx.clone();
            match std::thread::Builder::new()
                .name("publish-size".into())
                .spawn(move || {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        estimate(&worker_request, &worker_cancel)
                    }))
                    .unwrap_or_else(|_| {
                        Err("Publish size calculation failed unexpectedly.".into())
                    });
                    let _ = sender.send(outcome);
                    repaint.request_repaint();
                }) {
                Ok(_) => {
                    self.running = Some(Running {
                        request,
                        cancel,
                        result,
                    })
                }
                Err(error) => {
                    self.state =
                        EstimateState::Failed(format!("Cannot calculate publish size: {error}"))
                }
            }
        }
    }
}

fn estimate(request: &Request, cancel: &AtomicBool) -> Outcome {
    let options = request.options.validated()?;
    let result = neoflash_export::estimate_publish_options_cancellable_with_config(
        &request.project,
        &request.runtime.directory,
        &options,
        &request.html_config,
        || cancel.load(Ordering::Relaxed),
    )
    .map_err(|error| error.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if RuntimeStamp::read(&request.runtime.directory) != request.runtime {
        return Err(
            "Runtime files changed during the estimate; calculate again with the current build."
                .into(),
        );
    }
    Ok(result)
}

impl PublishPrefs {
    /// One parameter/validation path for explicit Publish, publish-on-save and size estimates.
    pub(crate) fn export_options(&self, project: &Project) -> Result<PublishOptions, String> {
        let options = self.options.validated()?;
        if self.warnings_as_errors {
            let warnings = neoflash_export::export_warnings(project);
            if !warnings.is_empty() {
                return Err(format!(
                    "{} warning(s) treated as errors ({})",
                    warnings.len(),
                    warnings.join("; ")
                ));
            }
        }
        Ok(options)
    }
}

impl Editor {
    /// Refresh the authored snapshot and selected draft before the dialog reads the estimate.
    pub(crate) fn request_publish_estimate(
        &mut self,
        ctx: &egui::Context,
        draft: &PublishPrefs,
    ) -> &EstimateState {
        self.publish_controller
            .refresh(&self.project, draft, &Self::runtime_dir(), ctx)
    }

    pub(crate) fn update_publish_estimate(&mut self, ctx: &egui::Context) {
        if self
            .publish_controller
            .current
            .as_ref()
            .is_some_and(|request| request.project.root != self.project.root)
        {
            self.publish_controller.cancel();
        }
        if let Some(demand) = self.publish_controller.demand.clone() {
            // The dialog owns a draft: saved workspace defaults must not replace its selection.
            self.publish_controller.refresh(
                &self.project,
                &demand.preferences,
                &demand.runtime,
                ctx,
            );
        } else {
            // Drain a cancelled worker without resurrecting an estimate after the dialog closes.
            self.publish_controller.poll(ctx);
        }
    }

    /// Record structured, jumpable diagnostics in both `Compiler errors` and `Output` whenever
    /// HTML5 or Native Banner publishing fails without touching the destination folder.
    pub(crate) fn record_publish_export_failure(
        &mut self,
        resolved_project: &Project,
        output: &Path,
        error: &neoflash_export::ExportError,
        is_native_banner: bool,
    ) {
        use crate::output_pipeline::{OutputEvent, OutputLocation, OutputSeverity, OutputSource};
        use crate::panels::compiler_errors::{
            CompilerDiagnosticItem, collect_compiler_diagnostics,
        };
        use crate::workspace::PanelId;

        self.refresh_compiler_diagnostics();
        let mut open_compiler_errors = false;

        match error {
            neoflash_export::ExportError::Scripts(_) => {
                open_compiler_errors = true;
                if !self.compiler_errors.items.iter().any(|item| item.is_error) {
                    self.compiler_errors.items = collect_compiler_diagnostics(resolved_project);
                }
                let items = self.compiler_errors.items.clone();
                for item in items {
                    if !item.is_error {
                        continue;
                    }
                    let loc = item.script.as_ref().map(|script_loc| {
                        script_location_to_output_location(
                            resolved_project,
                            script_loc,
                            item.line,
                            item.column,
                        )
                    });
                    let mut event = OutputEvent::new(
                        OutputSeverity::Error,
                        OutputSource::Compiler,
                        format!("{}: {}", item.location_label, item.message),
                    );
                    if let Some(loc) = loc {
                        event = event.with_location(loc);
                    }
                    let _ = self.push_output_unique(event);
                }
            }
            neoflash_export::ExportError::UnsupportedScript(_) if is_native_banner => {
                let banner_diags = neoflash_export::preflight_native_banner(resolved_project);
                for diag in banner_diags.into_iter().filter(|d| d.blocking) {
                    open_compiler_errors = true;
                    let (script_loc, out_loc) =
                        banner_diagnostic_to_locations(resolved_project, &diag);
                    let formatted = format!("[{}] {}", diag.code, diag.message);
                    if !self.compiler_errors.items.iter().any(|existing| {
                        existing.location_label == diag.location && existing.message == formatted
                    }) {
                        self.compiler_errors.items.push(CompilerDiagnosticItem {
                            is_error: true,
                            message: formatted.clone(),
                            location_label: diag.location.clone(),
                            script: script_loc,
                            line: 1,
                            column: 1,
                            quick_fixes: Vec::new(),
                        });
                    }
                    let mut event = OutputEvent::new(
                        OutputSeverity::Error,
                        OutputSource::Publish,
                        format!("[{}] {}: {}", diag.code, diag.location, diag.message),
                    );
                    if let Some(loc) = out_loc {
                        event = event.with_location(loc);
                    }
                    let _ = self.push_output_unique(event);
                }
            }
            _ => {}
        }

        let prefix = if is_native_banner {
            "Banner publish failed"
        } else {
            "Export failed"
        };
        let _ = self.message_event(
            OutputSeverity::Error,
            OutputSource::Publish,
            format!("{prefix}: {error}"),
            Some(OutputLocation::for_file(output.to_path_buf())),
        );
        if open_compiler_errors {
            self.show_panel(PanelId::CompilerErrors);
        }
        self.show_panel(PanelId::Output);
    }

    /// Atomic HTML5 / Native Banner publish entry point that validates/compiles completely before
    /// modifying `output` and surfaces structured jumpable diagnostics on failure.
    pub(crate) fn publish_html5_atomic(
        &mut self,
        output: &Path,
        runtime_dir: &Path,
        settings: &PublishPrefs,
    ) -> bool {
        use crate::output_pipeline::{OutputEvent, OutputLocation, OutputSeverity, OutputSource};
        use crate::workspace::{PanelId, PublishProfilePreset};

        self.finish_edits();
        let mut resolved_project = self.project.with_resolved_script_language().into_owned();
        resolved_project.set_html_render_mode(settings.render_mode);

        if settings.profile == PublishProfilePreset::WebBannerNative {
            let out_html = output.join("index.html");
            let banner_opts = neoflash_export::BannerOptions {
                max_gzip_bytes: Some(settings.banner_max_gzip_kb as usize * 1024),
                max_raw_bytes: Some(settings.banner_max_gzip_kb as usize * 4096),
                loop_timeline: settings.loop_timeline,
                autoplay: settings.autoplay,
                jpeg_quality: settings.jpeg_quality,
                ..neoflash_export::BannerOptions::default()
            };
            return match neoflash_export::export_native_banner_html(
                &resolved_project,
                &out_html,
                &banner_opts,
            ) {
                Ok(report) => {
                    let html_loc = OutputLocation::for_file(report.html_path.clone());
                    let _ = self.message_event(
                        OutputSeverity::Info,
                        OutputSource::Publish,
                        format!(
                            "Native HTML5 Banner exported: {} ({} B, {} B gzip)",
                            report.html_path.display(),
                            report.raw_bytes,
                            report.gzip_bytes
                        ),
                        Some(html_loc),
                    );
                    for diag in report.diagnostics {
                        let (_, out_loc) = banner_diagnostic_to_locations(&resolved_project, &diag);
                        let msg = format!("[{}] {}: {}", diag.code, diag.location, diag.message);
                        self.status = msg.clone();
                        let mut ev =
                            OutputEvent::new(OutputSeverity::Warning, OutputSource::Publish, msg);
                        if let Some(loc) = out_loc {
                            ev = ev.with_location(loc);
                        }
                        let _ = self.push_output_unique(ev);
                    }
                    true
                }
                Err(error) => {
                    self.record_publish_export_failure(&resolved_project, &out_html, &error, true);
                    false
                }
            };
        }

        let warnings = neoflash_export::export_warnings(&resolved_project);
        let options = match settings.export_options(&resolved_project) {
            Ok(options) => options,
            Err(error) => {
                self.refresh_compiler_diagnostics();
                let _ = self.message_event(
                    OutputSeverity::Error,
                    OutputSource::Publish,
                    format!("Publish failed: {error}"),
                    Some(OutputLocation::for_file(output.to_path_buf())),
                );
                if settings.warnings_as_errors {
                    self.show_panel(PanelId::CompilerErrors);
                }
                self.show_panel(PanelId::Output);
                return false;
            }
        };

        match neoflash_export::export_project_with_config(
            &resolved_project,
            output,
            runtime_dir,
            &options,
            &settings.publish_html_config(),
        ) {
            Ok(index) => {
                let _ = self.message_event(
                    OutputSeverity::Info,
                    OutputSource::Publish,
                    format!("HTML5 exported: {}", index.display()),
                    Some(OutputLocation::for_file(index)),
                );
                if let Ok(summary) = neoflash_export::compute_load_summary(&resolved_project) {
                    self.push_output_coalesced(OutputEvent::new(
                        OutputSeverity::Info,
                        OutputSource::Publish,
                        summary.summary_line(),
                    ));
                }
                if let Ok(report) = neoflash_export::format_size_report(&resolved_project) {
                    for line in report.lines() {
                        if !line.trim().is_empty() {
                            self.push_output_coalesced(OutputEvent::new(
                                OutputSeverity::Info,
                                OutputSource::Publish,
                                line.to_string(),
                            ));
                        }
                    }
                }
                let mut seen = std::collections::HashSet::new();
                for warning in warnings {
                    self.status = warning.clone();
                    if seen.insert(warning.clone()) {
                        let _ = self.push_output_unique(OutputEvent::new(
                            OutputSeverity::Warning,
                            OutputSource::Publish,
                            warning,
                        ));
                    }
                }
                true
            }
            Err(error) => {
                self.record_publish_export_failure(&resolved_project, output, &error, false);
                false
            }
        }
    }
}

fn script_location_to_output_location(
    project: &Project,
    location: &crate::panels::actions_sidebar::ScriptLocation,
    line: usize,
    column: usize,
) -> crate::output_pipeline::OutputLocation {
    use crate::output_pipeline::OutputLocation;
    use crate::panels::actions_sidebar::ScriptLocation;

    match location {
        ScriptLocation::Source(id) => {
            let path = project
                .sources
                .iter()
                .find(|u| &u.id == id)
                .map_or_else(|| PathBuf::from(id), |u| PathBuf::from(&u.path));
            OutputLocation::for_source(id.clone(), path).with_line_col(line, column)
        }
        ScriptLocation::Frame { symbol, frame } => {
            let sym_name = project.symbol(symbol).map(|s| s.name.clone());
            OutputLocation::for_frame(symbol.clone(), sym_name, None, *frame)
                .with_line_col(line, column)
        }
        ScriptLocation::Object {
            symbol,
            frame,
            element,
            event,
        } => {
            let sym = project.symbol(symbol);
            let sym_name = sym.map(|s| s.name.clone());
            let layer_idx = sym.and_then(|s| {
                s.timeline.layers.iter().position(|layer| {
                    layer
                        .key_at(*frame)
                        .is_some_and(|kf| kf.elements.iter().any(|el| &el.id == element))
                })
            });
            OutputLocation::for_object(
                symbol.clone(),
                sym_name,
                layer_idx,
                *frame,
                element.clone(),
                *event,
            )
            .with_line_col(line, column)
        }
    }
}

fn banner_diagnostic_to_locations(
    project: &Project,
    diag: &neoflash_export::BannerDiagnostic,
) -> (
    Option<crate::panels::actions_sidebar::ScriptLocation>,
    Option<crate::output_pipeline::OutputLocation>,
) {
    use crate::output_pipeline::OutputLocation;
    use crate::panels::actions_sidebar::ScriptLocation;

    let loc = diag.location.trim();
    if let Some(rest) = loc.strip_prefix("Source '")
        && let Some(source_path) = rest.strip_suffix('\'')
        && let Some(unit) = project.sources.iter().find(|u| u.path == source_path)
    {
        return (
            Some(ScriptLocation::Source(unit.id.clone())),
            Some(OutputLocation::for_source(unit.id.clone(), &unit.path).with_line_col(1, 1)),
        );
    }

    if let Some(rest) = loc.strip_prefix("Symbol '") {
        // Match against known project symbols longest-first in case a symbol name contains quotes/commas.
        let mut matched_sym = None;
        for sym in &project.symbols {
            let prefix = format!("{}', ", sym.name);
            if let Some(after_sym) = rest.strip_prefix(&prefix) {
                matched_sym = Some((sym, after_sym));
                break;
            }
        }
        if let Some((sym, after_sym)) = matched_sym {
            if let Some(frame_str) = after_sym.strip_prefix("frame ")
                && let Ok(frame_1) = frame_str.trim().parse::<u32>()
            {
                let frame = frame_1.saturating_sub(1);
                let script_loc = (diag.code == "banner-unsupported-frame-script").then(|| {
                    ScriptLocation::Frame {
                        symbol: sym.id.clone(),
                        frame,
                    }
                });
                let out_loc =
                    OutputLocation::for_frame(sym.id.clone(), Some(sym.name.clone()), None, frame)
                        .with_line_col(1, 1);
                return (script_loc, Some(out_loc));
            }
            if let Some(after_layer_kw) = after_sym.strip_prefix("layer '") {
                for (layer_idx, layer) in sym.timeline.layers.iter().enumerate() {
                    let lprefix = format!("{}', frame ", layer.name);
                    if let Some(after_layer) = after_layer_kw.strip_prefix(&lprefix) {
                        let (frame_part, element_part) =
                            if let Some((fpart, epart)) = after_layer.split_once(", element '") {
                                (fpart, epart.strip_suffix('\''))
                            } else {
                                (after_layer, None)
                            };
                        if let Ok(frame_1) = frame_part.trim().parse::<u32>() {
                            let frame = frame_1.saturating_sub(1);
                            if let Some(el_label) = element_part
                                && let Some(kf) = layer.key_at(frame)
                                && let Some(el) = kf.elements.iter().find(|el| {
                                    let shown = if el.name.is_empty() { &el.id } else { &el.name };
                                    shown == el_label
                                })
                                && let Some(obj_script) = el.scripts.first()
                            {
                                let script_loc = (diag.code == "banner-unsupported-object-script")
                                    .then(|| ScriptLocation::Object {
                                        symbol: sym.id.clone(),
                                        frame,
                                        element: el.id.clone(),
                                        event: obj_script.event,
                                    });
                                let out_loc = OutputLocation::for_object(
                                    sym.id.clone(),
                                    Some(sym.name.clone()),
                                    Some(layer_idx),
                                    frame,
                                    el.id.clone(),
                                    obj_script.event,
                                )
                                .with_line_col(1, 1);
                                return (script_loc, Some(out_loc));
                            }
                            let out_loc = OutputLocation::for_frame(
                                sym.id.clone(),
                                Some(sym.name.clone()),
                                Some(layer_idx),
                                frame,
                            );
                            return (None, Some(out_loc));
                        }
                    }
                }
            }
        }
    }

    (None, None)
}

#[cfg(test)]
#[path = "publish_controller_tests.rs"]
mod tests;
