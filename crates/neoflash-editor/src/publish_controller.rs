//! Publication settings and a bounded background size estimate, ready for NF2 request R-032.
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
        self.demand = Some(Demand {
            runtime: runtime.to_owned(),
            preferences: prefs.clone(),
        });
        let stamp = RuntimeStamp::read(runtime);
        let matches = self.current.as_ref().is_some_and(|request| {
            request.project.as_ref() == project
                && request.options == prefs.options
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
    let result = neoflash_export::estimate_publish_levels_cancellable(
        &request.project,
        &request.runtime.directory,
        options.fonts,
        &[options.size],
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
    Ok(result.and_then(|mut estimates| estimates.pop()))
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
    /// Entry point for the R-032 dialog once its design is available. Opening the existing static
    /// preview does not start expensive work or imply that its sample controls are implemented.
    #[allow(dead_code)]
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
}

#[cfg(test)]
#[path = "publish_controller_tests.rs"]
mod tests;
