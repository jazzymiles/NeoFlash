//! Non-blocking, fail-closed GitHub Releases update checker and SHA-256 verified downloader (`T-298`).

use crate::workspace::{UpdateChannel, UpdatePrefs};
use eframe::egui;
use neoflash_export::release_http::{
    DownloadRequest, ReleaseHttpError, download_verified_file, fetch_text_with_policy,
    validate_release_url,
};
use serde::Deserialize;
use std::{
    cmp::Ordering as CmpOrdering,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

pub(crate) const GITHUB_REPO: &str = "jazzymiles/NeoFlash";
pub(crate) const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";
pub(crate) const DEFAULT_GITHUB_WEB_BASE: &str = "https://github.com";
pub(crate) const DAILY_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(60);

/// Target platform/architecture pair used to match GitHub Release binary and checksum assets.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlatformTarget {
    MacosArm64,
    MacosX64,
    WindowsX64,
    LinuxX64,
    Unsupported,
}

impl PlatformTarget {
    pub(crate) const fn current() -> Self {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Self::MacosArm64
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            Self::MacosX64
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Self::WindowsX64
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            Self::LinuxX64
        }
        #[cfg(not(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
        )))]
        {
            Self::Unsupported
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::MacosArm64 => "macOS (Apple Silicon)",
            Self::MacosX64 => "macOS (Intel x64)",
            Self::WindowsX64 => "Windows x64",
            Self::LinuxX64 => "Linux x64",
            Self::Unsupported => "this platform",
        }
    }
}

/// Pre-release identifier per SemVer 2.0.0 §9 & §11.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreId {
    Num(u64),
    Alpha(String),
}

impl Ord for PreId {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        match (self, other) {
            (Self::Num(a), Self::Num(b)) => a.cmp(b),
            // SemVer 2.0.0 §11.4.3: numeric identifiers always have lower precedence than non-numeric.
            (Self::Num(_), Self::Alpha(_)) => CmpOrdering::Less,
            (Self::Alpha(_), Self::Num(_)) => CmpOrdering::Greater,
            (Self::Alpha(a), Self::Alpha(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for PreId {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for PreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(n) => write!(f, "{n}"),
            Self::Alpha(s) => f.write_str(s),
        }
    }
}

/// Semantic version supporting prerelease comparison (`0.1.0-beta.1` < `0.1.0-beta.2` < `0.1.0`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReleaseVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<PreId>,
}

impl ReleaseVersion {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let without_v = trimmed
            .strip_prefix('v')
            .or_else(|| trimmed.strip_prefix('V'))
            .unwrap_or(trimmed);
        // Strip optional `+build` metadata (ignored for version precedence).
        let main_and_pre = match without_v.split_once('+') {
            Some((left, build)) => {
                if build.is_empty()
                    || build
                        .split('.')
                        .any(|seg| seg.is_empty() || !seg.bytes().all(is_ident_byte))
                {
                    return None;
                }
                left
            }
            None => without_v,
        };

        let (core, pre) = match main_and_pre.split_once('-') {
            Some((core, pre_str)) => {
                if pre_str.is_empty() {
                    return None;
                }
                let mut ids = Vec::new();
                for seg in pre_str.split('.') {
                    if seg.is_empty() || !seg.bytes().all(is_ident_byte) {
                        return None;
                    }
                    if seg.bytes().all(|b| b.is_ascii_digit()) {
                        if seg.len() > 1 && seg.starts_with('0') {
                            return None;
                        }
                        let num = seg.parse::<u64>().ok()?;
                        ids.push(PreId::Num(num));
                    } else {
                        ids.push(PreId::Alpha(seg.to_owned()));
                    }
                }
                (core, ids)
            }
            None => (main_and_pre, Vec::new()),
        };

        let mut parts = core.split('.');
        let major = parse_core_num(parts.next()?)?;
        let minor = parse_core_num(parts.next()?)?;
        let patch = parse_core_num(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    pub(crate) fn is_stable(&self) -> bool {
        self.pre.is_empty()
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

fn parse_core_num(raw: &str) -> Option<u64> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if raw.len() > 1 && raw.starts_with('0') {
        return None;
    }
    raw.parse::<u64>().ok()
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => CmpOrdering::Equal,
                // Normal version always has higher precedence than prerelease.
                (true, false) => CmpOrdering::Greater,
                (false, true) => CmpOrdering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(other.pre.iter()) {
                        let ord = a.cmp(b);
                        if ord != CmpOrdering::Equal {
                            return ord;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            })
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            f.write_str("-")?;
            for (idx, part) in self.pre.iter().enumerate() {
                if idx > 0 {
                    f.write_str(".")?;
                }
                write!(f, "{part}")?;
            }
        }
        Ok(())
    }
}

/// Minimal GitHub Releases REST API item schema.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct GitHubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<GitHubAsset>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerifiedAssetTarget {
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub checksum_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AvailableUpdate {
    pub version: String,
    pub release_notes_url: String,
    pub download: Result<VerifiedAssetTarget, String>,
}

/// Configuration for [`UpdaterController`]; overridden cleanly by isolated tests.
#[derive(Clone, Debug)]
pub(crate) struct UpdaterConfig {
    pub repo: String,
    pub api_base_override: Option<String>,
    pub current_version: String,
    pub platform: PlatformTarget,
    pub max_json_bytes: usize,
    pub max_checksum_bytes: usize,
    pub max_download_bytes: u64,
    pub check_timeout: Duration,
    /// Used by automated UI tests to direct downloads to an explicit temp path instead of a native picker.
    pub download_dest_override: Option<PathBuf>,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            repo: GITHUB_REPO.to_owned(),
            api_base_override: None,
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: PlatformTarget::current(),
            max_json_bytes: 2 * 1024 * 1024,
            max_checksum_bytes: 256 * 1024,
            max_download_bytes: 1024 * 1024 * 1024,
            check_timeout: Duration::from_secs(15),
            download_dest_override: None,
        }
    }
}

impl UpdaterConfig {
    pub(crate) fn effective_api_base(&self) -> String {
        if let Some(base) = &self.api_base_override {
            return base.trim().trim_end_matches('/').to_owned();
        }
        if let Ok(env_base) = std::env::var("NEOFLASH_UPDATE_API_BASE") {
            let trimmed = env_base.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
        DEFAULT_GITHUB_API_BASE.to_owned()
    }

    pub(crate) fn effective_web_base(&self) -> String {
        let api_base = self.effective_api_base();
        if api_base != DEFAULT_GITHUB_API_BASE {
            return api_base;
        }
        DEFAULT_GITHUB_WEB_BASE.to_owned()
    }

    pub(crate) fn loopback_base(&self) -> Option<String> {
        let api_base = self.effective_api_base();
        (api_base != DEFAULT_GITHUB_API_BASE).then_some(api_base)
    }

    pub(crate) fn releases_api_url(&self) -> String {
        format!(
            "{}/repos/{}/releases?per_page=20",
            self.effective_api_base(),
            self.repo
        )
    }

    pub(crate) fn default_release_notes_url(&self) -> String {
        format!("{}/{}/releases", self.effective_web_base(), self.repo)
    }

    fn user_agent(&self) -> String {
        format!("NeoFlash/{}", self.current_version)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateState {
    Idle,
    Checking {
        channel: UpdateChannel,
    },
    UpToDate {
        channel: UpdateChannel,
        latest_version: Option<String>,
        release_notes_url: String,
    },
    UpdateAvailable {
        channel: UpdateChannel,
        update: AvailableUpdate,
    },
    Downloading {
        channel: UpdateChannel,
        update: AvailableUpdate,
        dest_path: PathBuf,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    VerifiedDownloaded {
        channel: UpdateChannel,
        version: String,
        path: PathBuf,
        release_notes_url: String,
        reveal_note: Option<String>,
    },
    RateLimited {
        channel: UpdateChannel,
        retry_after_secs: Option<u64>,
        message: String,
        release_notes_url: String,
    },
    Error {
        channel: UpdateChannel,
        message: String,
        release_notes_url: String,
        retry_update: Option<AvailableUpdate>,
    },
}

enum WorkerEvent {
    CheckFinished {
        channel: UpdateChannel,
        result: Result<CheckOutcome, ReleaseHttpError>,
    },
    DownloadProgress {
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    DownloadFinished {
        channel: UpdateChannel,
        update: AvailableUpdate,
        dest_path: PathBuf,
        result: Result<(), ReleaseHttpError>,
    },
}

#[derive(Clone, Debug)]
enum CheckOutcome {
    UpToDate {
        latest_version: Option<String>,
        release_notes_url: String,
    },
    UpdateAvailable(AvailableUpdate),
}

struct ActiveWorker {
    cancel: Arc<AtomicBool>,
    rx: Receiver<WorkerEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdaterOutputItem {
    pub severity: crate::output_pipeline::OutputSeverity,
    pub message: String,
    pub path: Option<PathBuf>,
}

/// Asynchronous controller for GitHub Releases checking and SHA-256 verified downloading.
pub(crate) struct UpdaterController {
    pub config: UpdaterConfig,
    pub state: UpdateState,
    pub action_error: Option<String>,
    worker: Option<ActiveWorker>,
    auto_checked_this_session: bool,
    pending_output: Vec<UpdaterOutputItem>,
}

impl Default for UpdaterController {
    fn default() -> Self {
        Self::new(UpdaterConfig::default())
    }
}

impl Drop for UpdaterController {
    fn drop(&mut self) {
        self.cancel_active_worker();
    }
}

impl UpdaterController {
    pub(crate) fn new(config: UpdaterConfig) -> Self {
        Self {
            config,
            state: UpdateState::Idle,
            action_error: None,
            worker: None,
            auto_checked_this_session: false,
            pending_output: Vec::new(),
        }
    }

    pub(crate) fn drain_output_events(&mut self) -> Vec<UpdaterOutputItem> {
        std::mem::take(&mut self.pending_output)
    }

    pub(crate) fn is_checking(&self) -> bool {
        matches!(self.state, UpdateState::Checking { .. })
    }

    pub(crate) fn is_downloading(&self) -> bool {
        matches!(self.state, UpdateState::Downloading { .. })
    }

    #[allow(dead_code)]
    pub(crate) fn has_active_worker(&self) -> bool {
        self.worker.is_some()
    }

    pub(crate) fn release_notes_url(&self) -> &str {
        match &self.state {
            UpdateState::UpToDate {
                release_notes_url, ..
            }
            | UpdateState::UpdateAvailable {
                update:
                    AvailableUpdate {
                        release_notes_url, ..
                    },
                ..
            }
            | UpdateState::Downloading {
                update:
                    AvailableUpdate {
                        release_notes_url, ..
                    },
                ..
            }
            | UpdateState::VerifiedDownloaded {
                release_notes_url, ..
            }
            | UpdateState::RateLimited {
                release_notes_url, ..
            }
            | UpdateState::Error {
                release_notes_url, ..
            } => release_notes_url,
            UpdateState::Idle | UpdateState::Checking { .. } => {
                // Static string fallback when no release URL is cached yet.
                ""
            }
        }
    }

    pub(crate) fn verified_release_notes_url(&self) -> Result<String, String> {
        let candidate = self.release_notes_url();
        let raw = if candidate.is_empty() {
            self.config.default_release_notes_url()
        } else {
            candidate.to_owned()
        };
        let loopback = self.config.loopback_base();
        validate_release_url(&raw, loopback.as_deref())
            .map(|u| u.into())
            .map_err(|e| e.to_string())
    }

    pub(crate) fn open_release_notes(&mut self) -> Result<(), String> {
        let url = self.verified_release_notes_url()?;
        match crate::platform_open::open_url(&url) {
            Ok(()) => {
                self.action_error = None;
                Ok(())
            }
            Err(err) => {
                let msg = format!("Could not open release notes: {err}");
                self.action_error = Some(msg.clone());
                self.pending_output.push(UpdaterOutputItem {
                    severity: crate::output_pipeline::OutputSeverity::Error,
                    message: msg.clone(),
                    path: None,
                });
                Err(msg)
            }
        }
    }

    pub(crate) fn reveal_downloaded_file(&mut self, path: &Path) -> Result<(), String> {
        match crate::platform_open::reveal_file(path) {
            Ok(()) => {
                self.action_error = None;
                Ok(())
            }
            Err(err) => {
                let msg = format!("Could not reveal file: {err}");
                self.action_error = Some(msg.clone());
                self.pending_output.push(UpdaterOutputItem {
                    severity: crate::output_pipeline::OutputSeverity::Warning,
                    message: msg.clone(),
                    path: Some(path.to_path_buf()),
                });
                Err(msg)
            }
        }
    }

    /// Clear stale check results if the user switched release channel before checking again.
    pub(crate) fn sync_selected_channel(&mut self, channel: UpdateChannel) {
        let state_channel = match &self.state {
            UpdateState::Idle => return,
            UpdateState::Checking { channel: c }
            | UpdateState::UpToDate { channel: c, .. }
            | UpdateState::UpdateAvailable { channel: c, .. }
            | UpdateState::Downloading { channel: c, .. }
            | UpdateState::VerifiedDownloaded { channel: c, .. }
            | UpdateState::RateLimited { channel: c, .. }
            | UpdateState::Error { channel: c, .. } => *c,
        };
        if state_channel != channel && !self.is_checking() && !self.is_downloading() {
            self.state = UpdateState::Idle;
            self.action_error = None;
        }
    }

    /// Trigger an opt-in background daily check at startup if enabled and >= 24 hours elapsed.
    pub(crate) fn maybe_auto_check(
        &mut self,
        prefs: &UpdatePrefs,
        now_unix: u64,
        ctx: &egui::Context,
    ) {
        if self.auto_checked_this_session || !prefs.check_automatically {
            return;
        }
        if !should_run_daily_check(prefs.last_check_unix, now_unix) {
            return;
        }
        self.auto_checked_this_session = true;
        self.start_check(prefs.channel, ctx);
    }

    /// Spawn a background thread querying GitHub Releases for `channel`.
    pub(crate) fn start_check(&mut self, channel: UpdateChannel, ctx: &egui::Context) {
        if self.is_checking() || self.is_downloading() {
            return;
        }
        self.cancel_active_worker();
        self.action_error = None;
        self.state = UpdateState::Checking { channel };

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let config = self.config.clone();
        let ctx_clone = ctx.clone();

        let spawned = std::thread::Builder::new()
            .name("neoflash-update-check".into())
            .spawn(move || {
                let result = run_check_request(&config, channel);
                let _ = tx.send(WorkerEvent::CheckFinished { channel, result });
                ctx_clone.request_repaint();
            });

        match spawned {
            Ok(_) => {
                self.worker = Some(ActiveWorker { cancel, rx });
                ctx.request_repaint();
            }
            Err(err) => {
                let message = format!("Could not start update check thread: {err}");
                self.pending_output.push(UpdaterOutputItem {
                    severity: crate::output_pipeline::OutputSeverity::Error,
                    message: message.clone(),
                    path: None,
                });
                self.state = UpdateState::Error {
                    channel,
                    message,
                    release_notes_url: self.config.default_release_notes_url(),
                    retry_update: None,
                };
            }
        }
    }

    /// Start downloading and verifying `update` into `dest_path` on a background thread.
    pub(crate) fn start_download_to_path(
        &mut self,
        channel: UpdateChannel,
        update: AvailableUpdate,
        dest_path: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.is_checking() || self.is_downloading() {
            return;
        }
        let target = match &update.download {
            Ok(t) => t.clone(),
            Err(reason) => {
                let message = reason.clone();
                self.pending_output.push(UpdaterOutputItem {
                    severity: crate::output_pipeline::OutputSeverity::Error,
                    message: message.clone(),
                    path: Some(dest_path),
                });
                self.state = UpdateState::Error {
                    channel,
                    message,
                    release_notes_url: update.release_notes_url.clone(),
                    retry_update: Some(update),
                };
                return;
            }
        };

        self.cancel_active_worker();
        self.action_error = None;
        self.state = UpdateState::Downloading {
            channel,
            update: update.clone(),
            dest_path: dest_path.clone(),
            received_bytes: 0,
            total_bytes: (target.asset_size > 0).then_some(target.asset_size),
        };

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        let config = self.config.clone();
        let ctx_clone = ctx.clone();

        let spawned = std::thread::Builder::new()
            .name("neoflash-update-download".into())
            .spawn(move || {
                let result = run_verified_download(
                    &config,
                    &target,
                    &dest_path,
                    &cancel_worker,
                    &tx,
                    &ctx_clone,
                );
                let _ = tx.send(WorkerEvent::DownloadFinished {
                    channel,
                    update,
                    dest_path,
                    result,
                });
                ctx_clone.request_repaint();
            });

        match spawned {
            Ok(_) => {
                self.worker = Some(ActiveWorker { cancel, rx });
                ctx.request_repaint();
            }
            Err(err) => {
                self.state = UpdateState::Error {
                    channel,
                    message: format!("Could not start download thread: {err}"),
                    release_notes_url: self.config.default_release_notes_url(),
                    retry_update: None,
                };
            }
        }
    }

    pub(crate) fn cancel_download(&mut self) {
        if let Some(worker) = &self.worker {
            worker.cancel.store(true, Ordering::Release);
        }
        if let UpdateState::Downloading {
            channel, update, ..
        } = &self.state
        {
            self.state = UpdateState::UpdateAvailable {
                channel: *channel,
                update: update.clone(),
            };
        }
    }

    fn cancel_active_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel.store(true, Ordering::Release);
        }
    }

    /// Drain worker events non-blockingly. Returns `(check_completed, newly_verified_download)`.
    pub(crate) fn poll(
        &mut self,
        ctx: &egui::Context,
        reveal_on_finish: bool,
    ) -> (bool, Option<PathBuf>) {
        let mut completed_check = false;
        let mut verified_path = None;
        let mut finish_worker = false;
        let mut queued_output = Vec::new();

        let Some(worker) = self.worker.as_ref() else {
            return (false, None);
        };

        loop {
            match worker.rx.try_recv() {
                Ok(WorkerEvent::CheckFinished { channel, result }) => {
                    finish_worker = true;
                    match result {
                        Ok(CheckOutcome::UpToDate {
                            latest_version,
                            release_notes_url,
                        }) => {
                            completed_check = true;
                            self.state = UpdateState::UpToDate {
                                channel,
                                latest_version,
                                release_notes_url,
                            };
                        }
                        Ok(CheckOutcome::UpdateAvailable(update)) => {
                            completed_check = true;
                            queued_output.push(UpdaterOutputItem {
                                severity: crate::output_pipeline::OutputSeverity::Info,
                                message: format!(
                                    "Update available ({:?}): NeoFlash {}",
                                    channel, update.version
                                ),
                                path: None,
                            });
                            self.state = UpdateState::UpdateAvailable { channel, update };
                        }
                        Err(ReleaseHttpError::RateLimited {
                            retry_after_secs,
                            detail,
                        }) => {
                            let message = match retry_after_secs {
                                Some(secs) => format!(
                                    "GitHub API rate limit reached; retry in {secs} s or open Release notes."
                                ),
                                None => {
                                    if detail.is_empty() {
                                        "GitHub API rate limit reached; try again later or open Release notes."
                                            .to_owned()
                                    } else {
                                        detail
                                    }
                                }
                            };
                            queued_output.push(UpdaterOutputItem {
                                severity: crate::output_pipeline::OutputSeverity::Warning,
                                message: message.clone(),
                                path: None,
                            });
                            self.state = UpdateState::RateLimited {
                                channel,
                                retry_after_secs,
                                message,
                                release_notes_url: self.config.default_release_notes_url(),
                            };
                        }
                        Err(err) => {
                            let message = err.to_string();
                            queued_output.push(UpdaterOutputItem {
                                severity: crate::output_pipeline::OutputSeverity::Error,
                                message: format!("Update check failed: {message}"),
                                path: None,
                            });
                            self.state = UpdateState::Error {
                                channel,
                                message,
                                release_notes_url: self.config.default_release_notes_url(),
                                retry_update: None,
                            };
                        }
                    }
                    ctx.request_repaint();
                }
                Ok(WorkerEvent::DownloadProgress {
                    received_bytes,
                    total_bytes,
                }) => {
                    if let UpdateState::Downloading {
                        received_bytes: rx,
                        total_bytes: tot,
                        ..
                    } = &mut self.state
                    {
                        *rx = received_bytes;
                        if total_bytes.is_some() {
                            *tot = total_bytes;
                        }
                    }
                    ctx.request_repaint();
                }
                Ok(WorkerEvent::DownloadFinished {
                    channel,
                    update,
                    dest_path,
                    result,
                }) => {
                    finish_worker = true;
                    let cancelled = worker.cancel.load(Ordering::Acquire);
                    match result {
                        Ok(()) => {
                            let reveal_note = if reveal_on_finish {
                                crate::platform_open::reveal_file(&dest_path).err()
                            } else {
                                None
                            };
                            queued_output.push(UpdaterOutputItem {
                                severity: crate::output_pipeline::OutputSeverity::Info,
                                message: format!(
                                    "Update downloaded and verified: NeoFlash {} ({})",
                                    update.version,
                                    dest_path.display()
                                ),
                                path: Some(dest_path.clone()),
                            });
                            verified_path = Some(dest_path.clone());
                            self.state = UpdateState::VerifiedDownloaded {
                                channel,
                                version: update.version,
                                path: dest_path,
                                release_notes_url: update.release_notes_url,
                                reveal_note,
                            };
                        }
                        Err(ReleaseHttpError::Cancelled) => {
                            if matches!(self.state, UpdateState::Downloading { .. }) {
                                self.state = UpdateState::UpdateAvailable { channel, update };
                            }
                        }
                        Err(_) if cancelled => {
                            if matches!(self.state, UpdateState::Downloading { .. }) {
                                self.state = UpdateState::UpdateAvailable { channel, update };
                            }
                        }
                        Err(err) => {
                            let message = err.to_string();
                            queued_output.push(UpdaterOutputItem {
                                severity: crate::output_pipeline::OutputSeverity::Error,
                                message: format!("Update download failed: {message}"),
                                path: Some(dest_path),
                            });
                            self.state = UpdateState::Error {
                                channel,
                                message,
                                release_notes_url: update.release_notes_url.clone(),
                                retry_update: Some(update),
                            };
                        }
                    }
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finish_worker = true;
                    break;
                }
            }
        }
        self.pending_output.extend(queued_output);

        if finish_worker {
            self.worker = None;
        }
        (completed_check, verified_path)
    }

    pub(crate) fn choose_download_destination(&self, asset_name: &str) -> Option<PathBuf> {
        if let Some(override_path) = &self.config.download_dest_override {
            return Some(override_path.clone());
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save verified NeoFlash update")
            .set_file_name(asset_name);
        if let Some(dir) = default_downloads_dir() {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file()
    }
}

pub(crate) fn should_run_daily_check(last_check_unix: Option<u64>, now_unix: u64) -> bool {
    match last_check_unix {
        None | Some(0) => true,
        Some(last) => {
            if now_unix < last {
                // Handle large forward clock corruption (>1 day ahead) gracefully without tight loops.
                last.saturating_sub(now_unix) > DAILY_CHECK_INTERVAL_SECS
            } else {
                now_unix - last >= DAILY_CHECK_INTERVAL_SECS
            }
        }
    }
}

pub(crate) fn default_downloads_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let home = home?;
    let downloads = home.join("Downloads");
    if downloads.is_dir() {
        Some(downloads)
    } else if home.is_dir() {
        Some(home)
    } else {
        None
    }
}

fn run_check_request(
    config: &UpdaterConfig,
    channel: UpdateChannel,
) -> Result<CheckOutcome, ReleaseHttpError> {
    let current = ReleaseVersion::parse(&config.current_version).ok_or_else(|| {
        ReleaseHttpError::InvalidUrl(format!(
            "invalid current version '{}'",
            config.current_version
        ))
    })?;
    let loopback = config.loopback_base();
    let json = fetch_text_with_policy(
        &config.releases_api_url(),
        "application/vnd.github+json",
        &config.user_agent(),
        config.max_json_bytes,
        config.check_timeout,
        loopback.as_deref(),
    )?;
    let releases: Vec<GitHubRelease> = serde_json::from_str(&json).map_err(|e| {
        ReleaseHttpError::OfflineOrNetwork(format!("Malformed GitHub Releases response: {e}"))
    })?;
    Ok(select_release_for_channel(
        &releases,
        &current,
        channel,
        config.platform,
        config,
    ))
}

fn run_verified_download(
    config: &UpdaterConfig,
    target: &VerifiedAssetTarget,
    dest_path: &Path,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<WorkerEvent>,
    ctx: &egui::Context,
) -> Result<(), ReleaseHttpError> {
    if cancel.load(Ordering::Acquire) {
        return Err(ReleaseHttpError::Cancelled);
    }
    if !is_safe_asset_filename(&target.asset_name) {
        return Err(ReleaseHttpError::Io(format!(
            "Unsafe release asset filename '{}' rejected.",
            target.asset_name
        )));
    }
    if target.asset_size > config.max_download_bytes {
        return Err(ReleaseHttpError::SizeExceeded(format!(
            "Release asset '{}' declares {} bytes, exceeding the {}-byte download limit.",
            target.asset_name, target.asset_size, config.max_download_bytes
        )));
    }
    let loopback = config.loopback_base();
    let sums_text = match fetch_text_with_policy(
        &target.checksum_url,
        "text/plain",
        &config.user_agent(),
        config.max_checksum_bytes,
        config.check_timeout,
        loopback.as_deref(),
    ) {
        Ok(text) => text,
        Err(_) if cancel.load(Ordering::Acquire) => return Err(ReleaseHttpError::Cancelled),
        Err(err) => return Err(err),
    };
    if cancel.load(Ordering::Acquire) {
        return Err(ReleaseHttpError::Cancelled);
    }
    let expected_sha =
        lookup_sha256_for_asset(&sums_text, &target.asset_name).map_err(ReleaseHttpError::Io)?;

    let mut last_emit = Instant::now() - PROGRESS_EMIT_INTERVAL;
    let req = DownloadRequest {
        url: &target.asset_url,
        dest_path,
        expected_sha256: &expected_sha,
        user_agent: &config.user_agent(),
        max_bytes: config.max_download_bytes,
        allowed_loopback_base: loopback.as_deref(),
        cancel: cancel.as_ref(),
    };
    match download_verified_file(&req, &mut |received, total| {
        let now = Instant::now();
        if received == 0
            || total.is_some_and(|t| received >= t)
            || now.duration_since(last_emit) >= PROGRESS_EMIT_INTERVAL
        {
            last_emit = now;
            let _ = tx.send(WorkerEvent::DownloadProgress {
                received_bytes: received,
                total_bytes: total,
            });
            ctx.request_repaint();
        }
    }) {
        Ok(()) => Ok(()),
        Err(_) if cancel.load(Ordering::Acquire) => Err(ReleaseHttpError::Cancelled),
        Err(err) => Err(err),
    }
}

fn select_release_for_channel(
    releases: &[GitHubRelease],
    current: &ReleaseVersion,
    channel: UpdateChannel,
    platform: PlatformTarget,
    config: &UpdaterConfig,
) -> CheckOutcome {
    let loopback = config.loopback_base();
    let mut best: Option<(ReleaseVersion, &GitHubRelease)> = None;
    for release in releases {
        if release.draft {
            continue;
        }
        let Some(ver) = ReleaseVersion::parse(&release.tag_name) else {
            continue;
        };
        if channel == UpdateChannel::Stable && (release.prerelease || !ver.is_stable()) {
            continue;
        }
        let replace = match &best {
            None => true,
            Some((best_ver, _)) => ver > *best_ver,
        };
        if replace {
            best = Some((ver, release));
        }
    }

    let Some((best_ver, best_release)) = best else {
        return CheckOutcome::UpToDate {
            latest_version: None,
            release_notes_url: config.default_release_notes_url(),
        };
    };

    let raw_notes = if best_release.html_url.trim().is_empty() {
        format!(
            "{}/{}/releases/tag/{}",
            config.effective_web_base(),
            config.repo,
            best_release.tag_name
        )
    } else {
        best_release.html_url.trim().to_owned()
    };
    let release_notes_url = validate_release_url(&raw_notes, loopback.as_deref())
        .map(String::from)
        .unwrap_or_else(|_| config.default_release_notes_url());

    if best_ver <= *current {
        return CheckOutcome::UpToDate {
            latest_version: Some(best_ver.to_string()),
            release_notes_url,
        };
    }

    let download = resolve_verified_asset(&best_release.assets, platform, loopback.as_deref());

    CheckOutcome::UpdateAvailable(AvailableUpdate {
        version: best_ver.to_string(),
        release_notes_url,
        download,
    })
}

fn resolve_verified_asset(
    assets: &[GitHubAsset],
    platform: PlatformTarget,
    loopback_base: Option<&str>,
) -> Result<VerifiedAssetTarget, String> {
    let asset = select_platform_asset(assets, platform).ok_or_else(|| {
        format!(
            "No release package is available for {}; check Release notes.",
            platform.label()
        )
    })?;
    let checksum = select_checksum_asset(assets, platform).ok_or_else(|| {
        "Release is missing a SHA256SUMS manifest; download blocked for safety.".to_owned()
    })?;
    let asset_url = validate_release_url(&asset.browser_download_url, loopback_base)
        .map(String::from)
        .map_err(|e| format!("Blocked unverified download URL: {e}"))?;
    let checksum_url = validate_release_url(&checksum.browser_download_url, loopback_base)
        .map(String::from)
        .map_err(|e| format!("Blocked unverified checksum URL: {e}"))?;
    Ok(VerifiedAssetTarget {
        asset_name: asset.name.clone(),
        asset_url,
        asset_size: asset.size,
        checksum_url,
    })
}

pub(crate) fn is_safe_asset_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.trim() == name
        && !name
            .bytes()
            .any(|b| b == b'/' || b == b'\\' || b < 0x20 || b == 0x7f)
}

pub(crate) fn select_platform_asset(
    assets: &[GitHubAsset],
    platform: PlatformTarget,
) -> Option<&GitHubAsset> {
    let is_auxiliary = |lower: &str| {
        lower.contains("sha256sums")
            || lower.contains("corresponding-source")
            || lower.ends_with(".sha256")
            || lower.ends_with(".asc")
            || lower.ends_with(".sig")
            || lower.ends_with(".sigstore")
            || lower.ends_with(".txt")
    };
    let find_matching = |pred: &dyn Fn(&str) -> bool| -> Option<&GitHubAsset> {
        assets.iter().find(|a| {
            if !is_safe_asset_filename(&a.name) {
                return false;
            }
            let lower = a.name.to_ascii_lowercase();
            !is_auxiliary(&lower) && pred(&lower)
        })
    };

    match platform {
        PlatformTarget::MacosArm64 => {
            let matches_arch = |lower: &str| {
                (lower.contains("macos") || lower.contains("darwin"))
                    && (lower.contains("arm64") || lower.contains("aarch64"))
                    && !lower.contains("x86_64")
                    && !lower.contains("x86-64")
                    && !lower.contains("x64")
                    && !lower.contains("intel")
            };
            find_matching(&|l| matches_arch(l) && l.ends_with(".dmg"))
                .or_else(|| find_matching(&|l| matches_arch(l) && l.ends_with(".zip")))
        }
        PlatformTarget::MacosX64 => {
            let matches_intel = |lower: &str| {
                (lower.contains("macos") || lower.contains("darwin"))
                    && (lower.contains("x86_64")
                        || lower.contains("x86-64")
                        || lower.contains("x64")
                        || lower.contains("intel"))
                    && !lower.contains("arm64")
                    && !lower.contains("aarch64")
            };
            let matches_universal = |lower: &str| {
                (lower.contains("macos") || lower.contains("darwin")) && lower.contains("universal")
            };
            find_matching(&|l| matches_intel(l) && l.ends_with(".dmg"))
                .or_else(|| find_matching(&|l| matches_intel(l) && l.ends_with(".zip")))
                .or_else(|| find_matching(&|l| matches_universal(l) && l.ends_with(".dmg")))
                .or_else(|| find_matching(&|l| matches_universal(l) && l.ends_with(".zip")))
        }
        PlatformTarget::WindowsX64 => {
            let matches_win_x64 = |lower: &str| {
                (lower.contains("windows") || lower.contains("win64"))
                    && (lower.contains("x64")
                        || lower.contains("x86_64")
                        || lower.contains("amd64"))
                    && !lower.contains("arm64")
                    && !lower.contains("aarch64")
            };
            find_matching(&|l| matches_win_x64(l) && l.ends_with(".zip")).or_else(|| {
                find_matching(&|l| {
                    matches_win_x64(l) && (l.ends_with(".msix") || l.ends_with(".msi"))
                })
            })
        }
        PlatformTarget::LinuxX64 => {
            let matches_deb = |lower: &str| {
                lower.ends_with(".deb")
                    && (lower.contains("amd64")
                        || lower.contains("x86_64")
                        || lower.contains("x64"))
                    && !lower.contains("arm64")
                    && !lower.contains("aarch64")
            };
            let matches_tarball = |lower: &str| {
                lower.contains("linux")
                    && (lower.contains("x64")
                        || lower.contains("x86_64")
                        || lower.contains("amd64"))
                    && !lower.contains("arm64")
                    && !lower.contains("aarch64")
                    && (lower.ends_with(".tar.gz") || lower.ends_with(".tgz"))
            };
            find_matching(&matches_deb).or_else(|| find_matching(&matches_tarball))
        }
        PlatformTarget::Unsupported => None,
    }
}

pub(crate) fn select_checksum_asset(
    assets: &[GitHubAsset],
    platform: PlatformTarget,
) -> Option<&GitHubAsset> {
    if platform == PlatformTarget::Unsupported {
        return None;
    }
    let is_sums = |lower: &str| {
        lower == "sha256sums"
            || lower == "sha256sums.txt"
            || lower.ends_with("-sha256sums.txt")
            || lower.ends_with("-sha256sums")
    };
    let find_sums = |pred: &dyn Fn(&str) -> bool| -> Option<&GitHubAsset> {
        assets.iter().find(|a| {
            if !is_safe_asset_filename(&a.name) {
                return false;
            }
            let lower = a.name.to_ascii_lowercase();
            is_sums(&lower) && pred(&lower)
        })
    };

    let exact_hints: &[&str] = match platform {
        PlatformTarget::MacosArm64 => &[
            "macos-arm64",
            "macos-aarch64",
            "darwin-arm64",
            "darwin-aarch64",
        ],
        PlatformTarget::MacosX64 => &[
            "macos-x86_64",
            "macos-x86-64",
            "macos-x64",
            "macos-intel",
            "darwin-x86_64",
            "darwin-x64",
        ],
        PlatformTarget::WindowsX64 => &["windows-x64", "windows-x86_64", "win64"],
        PlatformTarget::LinuxX64 => &["linux-x64", "linux-x86_64", "linux-amd64"],
        PlatformTarget::Unsupported => &[],
    };
    for hint in exact_hints {
        if let Some(found) = find_sums(&|l| l.contains(hint)) {
            return Some(found);
        }
    }

    let is_foreign_platform = |lower: &str| match platform {
        PlatformTarget::MacosArm64 => {
            lower.contains("windows")
                || lower.contains("win64")
                || lower.contains("linux")
                || lower.contains("x86_64")
                || lower.contains("x86-64")
                || lower.contains("x64")
                || lower.contains("intel")
        }
        PlatformTarget::MacosX64 => {
            lower.contains("windows")
                || lower.contains("win64")
                || lower.contains("linux")
                || lower.contains("arm64")
                || lower.contains("aarch64")
        }
        PlatformTarget::WindowsX64 => {
            lower.contains("macos")
                || lower.contains("darwin")
                || lower.contains("linux")
                || lower.contains("arm64")
                || lower.contains("aarch64")
        }
        PlatformTarget::LinuxX64 => {
            lower.contains("macos")
                || lower.contains("darwin")
                || lower.contains("windows")
                || lower.contains("win64")
                || lower.contains("arm64")
                || lower.contains("aarch64")
        }
        PlatformTarget::Unsupported => true,
    };

    let os_hint = match platform {
        PlatformTarget::MacosArm64 | PlatformTarget::MacosX64 => Some("macos"),
        PlatformTarget::WindowsX64 => Some("windows"),
        PlatformTarget::LinuxX64 => Some("linux"),
        PlatformTarget::Unsupported => None,
    };
    if let Some(hint) = os_hint
        && let Some(found) = find_sums(&|l| l.contains(hint) && !is_foreign_platform(l))
    {
        return Some(found);
    }

    find_sums(&|l| !is_foreign_platform(l))
}

/// Look up the expected 64-char SHA-256 hex digest for `asset_name` in a `SHA256SUMS` manifest.
///
/// Rejects unsafe asset names, malformed lines, duplicate conflicting digests, and missing entries.
pub(crate) fn lookup_sha256_for_asset(manifest: &str, asset_name: &str) -> Result<String, String> {
    if !is_safe_asset_filename(asset_name) {
        return Err(format!("Unsafe release asset filename '{asset_name}'."));
    }
    let target_name = Path::new(asset_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(asset_name);

    let body = manifest.strip_prefix('\u{feff}').unwrap_or(manifest);
    let mut matched: Option<String> = None;
    for (idx, raw_line) in body.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((hash_part, rest)) = split_checksum_line(trimmed) else {
            return Err(format!(
                "Invalid SHA256SUMS line {} (expected '<64-hex>  <filename>').",
                idx + 1
            ));
        };
        let without_star = rest.trim().strip_prefix('*').unwrap_or(rest.trim());
        let clean_name = without_star
            .trim_start_matches("./")
            .trim_start_matches(".\\");
        let entry_basename = clean_name.rsplit(['/', '\\']).next().unwrap_or(clean_name);
        if entry_basename.is_empty() || entry_basename == "." || entry_basename == ".." {
            return Err(format!(
                "Invalid SHA256SUMS line {} (empty or unsafe filename).",
                idx + 1
            ));
        }
        if clean_name == asset_name || entry_basename == target_name {
            let lower = hash_part.to_ascii_lowercase();
            if let Some(prev) = &matched {
                if prev != &lower {
                    return Err(format!(
                        "Conflicting SHA-256 entries for '{asset_name}' in SHA256SUMS."
                    ));
                }
            } else {
                matched = Some(lower);
            }
        }
    }

    matched.ok_or_else(|| {
        format!(
            "SHA256SUMS manifest does not contain a checksum for '{asset_name}'; download rejected."
        )
    })
}

fn split_checksum_line(line: &str) -> Option<(&str, &str)> {
    // Support BSD/OpenSSL `SHA256 (<filename>) = <64-hex>` alongside GNU/shasum `<64-hex> [*]<filename>`.
    if let Some(after_tag) = line
        .strip_prefix("SHA256 (")
        .or_else(|| line.strip_prefix("SHA256("))
        && let Some((filename, eq_hash)) = after_tag.rsplit_once(") = ")
    {
        let hash = eq_hash.trim();
        let name = filename.trim();
        if !name.is_empty() && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some((hash, name));
        }
        return None;
    }

    let mut split_idx = None;
    for (i, b) in line.bytes().enumerate() {
        if b.is_ascii_whitespace() {
            split_idx = Some(i);
            break;
        }
    }
    let idx = split_idx?;
    let (hash, rest) = line.split_at(idx);
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let filename = rest.trim_start();
    if filename.is_empty() {
        return None;
    }
    Some((hash, filename))
}

pub(crate) fn format_byte_progress(received: u64, total: Option<u64>) -> String {
    match total {
        Some(tot) if tot > 0 => format!("{} / {}", human_bytes(received), human_bytes(tot)),
        _ => human_bytes(received),
    }
}

fn human_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    if bytes as f64 >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn format_last_checked(last_unix: Option<u64>, now_unix: u64) -> String {
    let Some(last) = last_unix.filter(|&ts| ts > 0) else {
        return "Never checked".to_owned();
    };
    if now_unix <= last {
        return "Last checked just now".to_owned();
    }
    let delta = now_unix - last;
    if delta < 60 {
        "Last checked just now".to_owned()
    } else if delta < 3600 {
        let mins = delta / 60;
        format!("Last checked {} min ago", mins)
    } else if delta < 86_400 {
        let hours = delta / 3600;
        format!(
            "Last checked {hours} {} ago",
            if hours == 1 { "hour" } else { "hours" }
        )
    } else {
        let days = delta / 86_400;
        format!(
            "Last checked {days} {} ago",
            if days == 1 { "day" } else { "days" }
        )
    }
}
