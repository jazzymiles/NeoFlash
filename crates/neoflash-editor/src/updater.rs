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
    /// Optional target installation path override for sandboxed self-update E2E tests.
    pub install_target_override: Option<PathBuf>,
    /// Test-only override to skip `/usr/bin/codesign` when validating synthetic cross-platform fixtures.
    pub skip_codesign_verify: bool,
    /// Test-only override to apply the verified swap synchronously without spawning an external GUI process.
    pub dry_run_relaunch: bool,
    /// Save verified updates directly into the user's Downloads/cache directory for one-click install.
    pub use_auto_download_cache: bool,
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
            install_target_override: None,
            skip_codesign_verify: false,
            dry_run_relaunch: false,
            use_auto_download_cache: true,
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
    staged_self_update: Option<PreparedSelfUpdate>,
    committed_self_update_count: usize,
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
            staged_self_update: None,
            committed_self_update_count: 0,
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
        if self.config.use_auto_download_cache
            && let Some(auto_dest) = default_auto_download_path(asset_name)
        {
            return Some(auto_dest);
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save verified NeoFlash update")
            .set_file_name(asset_name);
        if let Some(dir) = default_downloads_dir() {
            dialog = dialog.set_directory(dir);
        }
        dialog.save_file()
    }

    pub(crate) fn can_self_install(&self, archive_path: &Path) -> bool {
        can_self_install_for_platform(archive_path, self.config.platform)
    }

    pub(crate) fn has_staged_self_update(&self) -> bool {
        self.staged_self_update.is_some()
    }

    #[allow(dead_code)]
    pub(crate) fn committed_self_updates(&self) -> usize {
        self.committed_self_update_count
    }

    pub(crate) fn stage_self_update(&mut self, archive_path: &Path) -> Result<(), String> {
        match prepare_verified_install_with_config(archive_path, None, &self.config) {
            Ok(prepared) => {
                self.action_error = None;
                self.staged_self_update = Some(prepared);
                Ok(())
            }
            Err(err) => {
                self.action_error = Some(err.clone());
                self.pending_output.push(UpdaterOutputItem {
                    severity: crate::output_pipeline::OutputSeverity::Error,
                    message: err.clone(),
                    path: Some(archive_path.to_path_buf()),
                });
                Err(err)
            }
        }
    }

    pub(crate) fn commit_staged_self_update(&mut self) -> Result<bool, String> {
        let Some(mut prepared) = self.staged_self_update.take() else {
            return Ok(false);
        };
        if self.config.dry_run_relaunch {
            apply_staged_bundle_swap(
                &prepared.staged_payload_path,
                &prepared.target_install_path,
                std::process::id(),
            )?;
            self.committed_self_update_count += 1;
            return Ok(true);
        }
        spawn_atomic_replace_and_relaunch(&mut prepared, std::process::id(), true)?;
        self.committed_self_update_count += 1;
        Ok(true)
    }

    pub(crate) fn abort_staged_self_update(&mut self) {
        self.staged_self_update = None;
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
    let selected = if platform == PlatformTarget::LinuxX64 {
        select_linux_runtime_asset(
            assets,
            std::env::var_os("APPIMAGE").as_deref(),
            std::env::current_exe().ok().as_deref(),
        )
    } else {
        select_platform_asset(assets, platform)
    };
    let asset = selected.ok_or_else(|| {
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

pub(crate) fn select_linux_runtime_asset<'a>(
    assets: &'a [GitHubAsset],
    appimage_env: Option<&std::ffi::OsStr>,
    current_exe: Option<&Path>,
) -> Option<&'a GitHubAsset> {
    let is_auxiliary = |lower: &str| {
        lower.contains("sha256sums")
            || lower.contains("corresponding-source")
            || lower.ends_with(".sha256")
            || lower.ends_with(".asc")
            || lower.ends_with(".sig")
            || lower.ends_with(".sigstore")
            || lower.ends_with(".txt")
    };
    let find_matching = |pred: &dyn Fn(&str) -> bool| -> Option<&'a GitHubAsset> {
        assets.iter().find(|a| {
            if !is_safe_asset_filename(&a.name) {
                return false;
            }
            let lower = a.name.to_ascii_lowercase();
            !is_auxiliary(&lower) && pred(&lower)
        })
    };
    let matches_tarball = |lower: &str| {
        lower.contains("linux")
            && (lower.contains("x64") || lower.contains("x86_64") || lower.contains("amd64"))
            && !lower.contains("arm64")
            && !lower.contains("aarch64")
            && (lower.ends_with(".tar.gz") || lower.ends_with(".tgz"))
    };
    let matches_appimage = |lower: &str| {
        lower.ends_with(".appimage")
            && (lower.contains("x86_64") || lower.contains("x64") || lower.contains("amd64"))
            && !lower.contains("arm64")
            && !lower.contains("aarch64")
    };

    if appimage_env.is_some_and(|v| !v.is_empty())
        && let Some(found) = find_matching(&matches_appimage)
    {
        return Some(found);
    }
    if let Some(exe) = current_exe {
        let is_system_install = exe.starts_with("/usr")
            || exe.starts_with("/bin")
            || exe.starts_with("/sbin")
            || exe.starts_with("/opt");
        if !is_system_install && let Some(found) = find_matching(&matches_tarball) {
            return Some(found);
        }
    }
    select_platform_asset(assets, PlatformTarget::LinuxX64)
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
            let matches_appimage = |lower: &str| {
                lower.ends_with(".appimage")
                    && (lower.contains("x86_64")
                        || lower.contains("x64")
                        || lower.contains("amd64"))
                    && !lower.contains("arm64")
                    && !lower.contains("aarch64")
            };
            find_matching(&matches_deb)
                .or_else(|| find_matching(&matches_tarball))
                .or_else(|| find_matching(&matches_appimage))
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

// ---------------------------------------------------------------------------
// One-click self-update staging, pre-install verification, and atomic swap
// ---------------------------------------------------------------------------

pub(crate) const ALLOWED_MACOS_BUNDLE_IDS: &[&str] =
    &["com.neoflash.editor", "org.neoflash.editor"];
const MAX_ARCHIVE_ENTRIES: usize = 16_384;
pub(crate) const MAX_ARCHIVE_ENTRY_BYTES: usize = 512 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_TOTAL_BYTES: u64 = 1536 * 1024 * 1024;
const MAX_PLIST_BYTES: usize = 1024 * 1024;

/// Staged, pre-verified self-update payload ready for atomic swap + relaunch (`T-501`).
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct PreparedSelfUpdate {
    pub platform: PlatformTarget,
    pub staging_root: PathBuf,
    pub staged_payload_path: PathBuf,
    pub target_install_path: PathBuf,
    pub relaunch_executable_rel: PathBuf,
    pub bundle_identifier: Option<String>,
    pub bundle_version: Option<String>,
    pub helper_script_path: Option<PathBuf>,
    handed_off_to_helper: bool,
}

impl Drop for PreparedSelfUpdate {
    fn drop(&mut self) {
        if !self.handed_off_to_helper && self.staging_root.exists() {
            let _ = std::fs::remove_dir_all(&self.staging_root);
        }
    }
}

struct StagingDirGuard {
    path: PathBuf,
    armed: bool,
}

impl StagingDirGuard {
    fn new(prefix: &str) -> Result<Self, String> {
        static STAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("{prefix}-{pid}-{nanos}-{seq}"));
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not create isolated update staging directory: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self {
            path: dir,
            armed: true,
        })
    }

    fn disarm(mut self) -> PathBuf {
        self.armed = false;
        self.path.clone()
    }
}

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        if self.armed && self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(target_os = "macos")]
struct DmgMountGuard {
    mount_point: PathBuf,
    mounted: bool,
}

#[cfg(target_os = "macos")]
impl DmgMountGuard {
    fn detach_now(&mut self) -> Result<(), String> {
        if !self.mounted {
            return Ok(());
        }
        let ok = std::process::Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&self.mount_point)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            || std::process::Command::new("/usr/bin/hdiutil")
                .args(["detach", "-force"])
                .arg(&self.mount_point)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        self.mounted = false;
        let _ = std::fs::remove_dir(&self.mount_point);
        if ok {
            Ok(())
        } else {
            Err(format!(
                "Failed to detach mounted disk image at '{}'.",
                self.mount_point.display()
            ))
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for DmgMountGuard {
    fn drop(&mut self) {
        let _ = self.detach_now();
    }
}

pub(crate) fn default_auto_download_path(asset_name: &str) -> Option<PathBuf> {
    if !is_safe_asset_filename(asset_name) {
        return None;
    }
    if let Some(downloads) = default_downloads_dir()
        && verify_directory_writable(&downloads).is_ok()
    {
        return Some(downloads.join(asset_name));
    }
    let fallback = std::env::temp_dir().join("neoflash-updates");
    std::fs::create_dir_all(&fallback).ok()?;
    verify_directory_writable(&fallback).ok()?;
    Some(fallback.join(asset_name))
}

pub(crate) fn can_self_install_for_platform(archive_path: &Path, platform: PlatformTarget) -> bool {
    let Some(name) = archive_path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    match platform {
        PlatformTarget::MacosArm64 | PlatformTarget::MacosX64 => {
            lower.ends_with(".zip") || lower.ends_with(".dmg")
        }
        PlatformTarget::WindowsX64 => lower.ends_with(".zip"),
        PlatformTarget::LinuxX64 => {
            lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".appimage")
        }
        PlatformTarget::Unsupported => false,
    }
}

fn reveal_action_hint(platform: PlatformTarget) -> &'static str {
    match platform {
        PlatformTarget::MacosArm64 | PlatformTarget::MacosX64 => "Reveal in Finder",
        PlatformTarget::WindowsX64 => "Show in Explorer",
        PlatformTarget::LinuxX64 | PlatformTarget::Unsupported => "Open folder",
    }
}

pub(crate) fn resolve_macos_app_bundle_from_exe(exe_path: &Path) -> Option<PathBuf> {
    let macos_dir = exe_path.parent()?;
    if !macos_dir
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("MacOS"))
    {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if !contents_dir
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("Contents"))
    {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    app_dir
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        .then(|| app_dir.to_path_buf())
}

fn normalize_path_for_guard(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let trimmed = if raw.len() > 1 {
        raw.trim_end_matches('/')
    } else {
        raw.as_str()
    };
    if cfg!(windows) || trimmed.as_bytes().get(1) == Some(&b':') {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_owned()
    }
}

/// Unconditionally block filesystem roots, OS system directories, and shared user folders
/// (`~/Downloads`, `~/Desktop`, `~/Documents`, `~/.local/bin`, `%USERPROFILE%`, `%LOCALAPPDATA%`, etc.)
/// so an atomic swap can never wipe a shared directory.
pub(crate) fn reject_dangerous_install_target(
    target_path: &Path,
    platform: PlatformTarget,
) -> Result<(), String> {
    if target_path.as_os_str().is_empty() {
        return Err("Invalid empty installation target path.".to_owned());
    }
    let has_normal_component = target_path
        .components()
        .any(|c| matches!(c, std::path::Component::Normal(_)));
    let raw_norm = normalize_path_for_guard(target_path);
    let looks_like_drive_root = raw_norm.len() == 2 && raw_norm.as_bytes()[1] == b':';
    if !has_normal_component
        || looks_like_drive_root
        || target_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .is_none()
    {
        return Err(format!(
            "Refusing unsafe root installation target '{}'; use {} to install manually.",
            target_path.display(),
            reveal_action_hint(platform)
        ));
    }

    let mut candidates = vec![raw_norm];
    if let Ok(canon) = std::fs::canonicalize(target_path) {
        let canon_norm = normalize_path_for_guard(&canon);
        if !candidates.contains(&canon_norm) {
            candidates.push(canon_norm);
        }
    }

    const FORBIDDEN_STATIC_ROOTS: &[&str] = &[
        "/",
        "/bin",
        "/sbin",
        "/usr",
        "/usr/bin",
        "/usr/sbin",
        "/usr/lib",
        "/usr/local",
        "/usr/local/bin",
        "/opt",
        "/etc",
        "/var",
        "/tmp",
        "/private/tmp",
        "/applications",
        "/system",
        "/library",
        "c:/windows",
        "c:/windows/system32",
        "c:/program files",
        "c:/program files (x86)",
        "c:/users",
    ];

    let mut forbidden_dynamic: Vec<String> = Vec::new();
    let push_forbidden = |list: &mut Vec<String>, p: PathBuf| {
        if !p.as_os_str().is_empty() {
            list.push(normalize_path_for_guard(&p));
            if let Ok(c) = std::fs::canonicalize(&p) {
                list.push(normalize_path_for_guard(&c));
            }
        }
    };
    push_forbidden(&mut forbidden_dynamic, std::env::temp_dir());
    for var in [
        "HOME",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "TEMP",
        "TMP",
        "TMPDIR",
    ] {
        if let Some(val) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            let base = PathBuf::from(val);
            push_forbidden(&mut forbidden_dynamic, base.clone());
            if var == "HOME" || var == "USERPROFILE" {
                for sub in [
                    "Downloads",
                    "Desktop",
                    "Documents",
                    "Applications",
                    ".local",
                    ".local/bin",
                    ".local/share",
                ] {
                    push_forbidden(&mut forbidden_dynamic, base.join(sub));
                }
            } else if var == "LOCALAPPDATA" {
                push_forbidden(&mut forbidden_dynamic, base.join("Programs"));
            }
        }
    }

    for cand in &candidates {
        let cand_lower = cand.to_ascii_lowercase();
        if FORBIDDEN_STATIC_ROOTS.contains(&cand_lower.as_str())
            || forbidden_dynamic.iter().any(|f| {
                if cfg!(windows) || cfg!(target_os = "macos") {
                    f.eq_ignore_ascii_case(cand)
                } else {
                    f == cand
                }
            })
        {
            return Err(format!(
                "Refusing self-update into shared or system directory '{}'; move NeoFlash into its own folder or use {} to install manually.",
                target_path.display(),
                reveal_action_hint(platform)
            ));
        }
    }
    Ok(())
}

pub(crate) fn verify_portable_install_markers(
    target_dir: &Path,
    platform: PlatformTarget,
) -> Result<(), String> {
    let has_meta = target_dir.join("BUILD_METADATA.json").is_file();
    let has_sums = target_dir.join("PACKAGE-SHA256SUMS.txt").is_file();
    if !has_meta || !has_sums {
        return Err(format!(
            "Automatic self-update refused: '{}' is not an isolated NeoFlash portable installation (missing BUILD_METADATA.json and PACKAGE-SHA256SUMS.txt). Extract the update manually via {}.",
            target_dir.display(),
            reveal_action_hint(platform)
        ));
    }
    Ok(())
}

pub(crate) fn resolve_portable_exe_install_dir(
    exe_path: &Path,
    platform: PlatformTarget,
) -> Result<PathBuf, String> {
    let target_dir = exe_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            format!(
                "Could not determine installation folder from '{}'.",
                exe_path.display()
            )
        })?;
    reject_dangerous_install_target(target_dir, platform)?;
    verify_portable_install_markers(target_dir, platform)?;
    Ok(target_dir.to_path_buf())
}

pub(crate) fn resolve_current_install_target(
    platform: PlatformTarget,
    archive_path: &Path,
    override_target: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(target) = override_target {
        reject_dangerous_install_target(target, platform)?;
        return Ok(target.to_path_buf());
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("Could not resolve running NeoFlash executable path: {e}"))?;
    match platform {
        PlatformTarget::MacosArm64 | PlatformTarget::MacosX64 => {
            let bundle = resolve_macos_app_bundle_from_exe(&exe).ok_or_else(|| {
                format!(
                    "Automatic install requires running from a packaged NeoFlash.app bundle (current executable: {}). Use {} to install manually.",
                    exe.display(),
                    reveal_action_hint(platform)
                )
            })?;
            reject_dangerous_install_target(&bundle, platform)?;
            Ok(bundle)
        }
        PlatformTarget::WindowsX64 => resolve_portable_exe_install_dir(&exe, platform),
        PlatformTarget::LinuxX64 => {
            let is_appimage = archive_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.to_ascii_lowercase().ends_with(".appimage"));
            if is_appimage {
                let target_file = if let Some(appimage_env) =
                    std::env::var_os("APPIMAGE").filter(|v| !v.is_empty())
                {
                    PathBuf::from(appimage_env)
                } else {
                    exe
                };
                reject_dangerous_install_target(&target_file, platform)?;
                return Ok(target_file);
            }
            resolve_portable_exe_install_dir(&exe, platform)
        }
        PlatformTarget::Unsupported => Err(format!(
            "Automatic self-update is not supported on {}; use {} to install manually.",
            platform.label(),
            reveal_action_hint(platform)
        )),
    }
}

fn verify_directory_writable(dir: &Path) -> Result<(), String> {
    static PROBE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    if !dir.is_dir() {
        return Err(format!("Directory '{}' does not exist.", dir.display()));
    }
    let probe = dir.join(format!(
        ".neoflash-write-probe-{}-{}",
        std::process::id(),
        PROBE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&probe)
        .map_err(|e| format!("Directory '{}' is not writable ({e}).", dir.display()))?;
    let _ = std::fs::remove_dir(&probe);
    Ok(())
}

pub(crate) fn verify_install_target_writable(
    target_path: &Path,
    platform: PlatformTarget,
) -> Result<(), String> {
    let parent = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            format!(
                "Invalid installation target '{}': no parent directory.",
                target_path.display()
            )
        })?;
    if let Err(err) = verify_directory_writable(parent) {
        return Err(format!(
            "Cannot install update into '{}': {err} Move NeoFlash to a user-writable folder or click {} to install manually.",
            parent.display(),
            reveal_action_hint(platform)
        ));
    }
    #[cfg(unix)]
    if let Ok(parent_meta) = std::fs::symlink_metadata(parent)
        && let Ok(target_meta) = std::fs::symlink_metadata(target_path)
    {
        use std::os::unix::fs::MetadataExt;
        let sticky = (parent_meta.mode() & 0o1000) != 0;
        let euid = unsafe { libc::geteuid() };
        if sticky && euid != 0 && target_meta.uid() != euid {
            return Err(format!(
                "Cannot replace '{}' inside sticky directory '{}' (owned by another user). Click {} to install manually.",
                target_path.display(),
                parent.display(),
                reveal_action_hint(platform)
            ));
        }
    }
    Ok(())
}

/// Stage and verify a downloaded update package before closing the editor (`T-501`).
#[allow(dead_code)]
pub(crate) fn prepare_verified_install(
    archive_path: &Path,
    target_install_path: Option<&Path>,
) -> Result<PreparedSelfUpdate, String> {
    let mut cfg = UpdaterConfig::default();
    if let Some(target) = target_install_path {
        cfg.install_target_override = Some(target.to_path_buf());
    }
    prepare_verified_install_with_config(archive_path, target_install_path, &cfg)
}

pub(crate) fn prepare_verified_install_with_config(
    archive_path: &Path,
    target_install_path: Option<&Path>,
    config: &UpdaterConfig,
) -> Result<PreparedSelfUpdate, String> {
    let archive_meta = std::fs::symlink_metadata(archive_path).map_err(|e| {
        format!(
            "Update archive '{}' could not be opened: {e}",
            archive_path.display()
        )
    })?;
    if archive_meta.file_type().is_symlink() || !archive_meta.is_file() {
        return Err(format!(
            "Update archive '{}' must be a regular file (symlinks rejected).",
            archive_path.display()
        ));
    }
    if archive_meta.len() == 0 {
        return Err(format!(
            "Update archive '{}' is empty.",
            archive_path.display()
        ));
    }

    let effective_target = resolve_current_install_target(
        config.platform,
        archive_path,
        target_install_path.or(config.install_target_override.as_deref()),
    )?;
    verify_install_target_writable(&effective_target, config.platform)?;

    let staging_guard = StagingDirGuard::new("neoflash-self-update")?;
    let unpacked_dir = staging_guard.path.join("unpacked");
    std::fs::create_dir_all(&unpacked_dir)
        .map_err(|e| format!("Could not create unpacked staging directory: {e}"))?;

    let archive_name_lower = archive_path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| "Update archive has an invalid UTF-8 filename.".to_owned())?;

    match config.platform {
        PlatformTarget::MacosArm64 | PlatformTarget::MacosX64 => {
            if !effective_target
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            {
                return Err(format!(
                    "macOS self-update target '{}' must be an .app bundle.",
                    effective_target.display()
                ));
            }
            let staged_app = if archive_name_lower.ends_with(".zip") {
                stage_macos_zip_bundle(archive_path, &unpacked_dir)?
            } else if archive_name_lower.ends_with(".dmg") {
                stage_macos_dmg_bundle(archive_path, &staging_guard.path, &unpacked_dir)?
            } else {
                return Err(format!(
                    "Unsupported macOS update archive '{}'; expected .zip or .dmg.",
                    archive_path.display()
                ));
            };

            verify_no_symlinks_outside_bundle(&unpacked_dir, &staged_app)?;
            verify_no_escaping_symlinks(&staged_app)?;

            let plist_info = verify_macos_bundle_plist(&staged_app, &effective_target)?;
            let exe_rel = PathBuf::from("Contents/MacOS").join(&plist_info.executable);
            let staged_exe = staged_app.join(&exe_rel);
            verify_macos_executable(&staged_exe)?;

            if !config.skip_codesign_verify {
                verify_macos_codesign(&staged_app)?;
            }

            let helper_path = staging_guard.path.join("swap-and-relaunch.sh");
            write_executable_script(
                &helper_path,
                &build_macos_swap_script(
                    &staged_app,
                    &effective_target,
                    &staging_guard.path,
                    std::process::id(),
                    true,
                ),
            )?;

            let staging_root = staging_guard.disarm();
            Ok(PreparedSelfUpdate {
                platform: config.platform,
                staging_root,
                staged_payload_path: staged_app,
                target_install_path: effective_target,
                relaunch_executable_rel: exe_rel,
                bundle_identifier: Some(plist_info.bundle_id),
                bundle_version: plist_info.short_version,
                helper_script_path: Some(helper_path),
                handed_off_to_helper: false,
            })
        }
        PlatformTarget::WindowsX64 => {
            if !archive_name_lower.ends_with(".zip") {
                return Err(format!(
                    "Automatic Windows update requires a .zip package (got '{}'); use Show in Explorer to run installers manually.",
                    archive_path.display()
                ));
            }
            extract_zip_archive_safely(archive_path, &unpacked_dir, false)?;
            let staged_payload =
                resolve_extracted_payload_dir(&unpacked_dir, "neoflash-editor.exe")?;
            verify_no_escaping_symlinks(&staged_payload)?;
            let staged_exe = staged_payload.join("neoflash-editor.exe");
            verify_windows_pe64_executable(&staged_exe)?;

            let helper_path = staging_guard.path.join("swap-and-relaunch.ps1");
            std::fs::write(
                &helper_path,
                build_windows_swap_script(
                    &staged_payload,
                    &effective_target,
                    &staging_guard.path,
                    std::process::id(),
                    true,
                ),
            )
            .map_err(|e| format!("Could not write Windows update swap script: {e}"))?;

            let staging_root = staging_guard.disarm();
            Ok(PreparedSelfUpdate {
                platform: config.platform,
                staging_root,
                staged_payload_path: staged_payload,
                target_install_path: effective_target,
                relaunch_executable_rel: PathBuf::from("neoflash-editor.exe"),
                bundle_identifier: None,
                bundle_version: None,
                helper_script_path: Some(helper_path),
                handed_off_to_helper: false,
            })
        }
        PlatformTarget::LinuxX64 => {
            if archive_name_lower.ends_with(".deb") {
                return Err(format!(
                    "Debian (.deb) packages require root privileges; run `sudo apt install \"{}\"` after quitting NeoFlash.",
                    archive_path.display()
                ));
            }
            let (staged_payload, relaunch_rel, is_single_file) = if archive_name_lower
                .ends_with(".tar.gz")
                || archive_name_lower.ends_with(".tgz")
            {
                extract_tar_gz_safely(archive_path, &unpacked_dir)?;
                let payload = resolve_extracted_payload_dir(&unpacked_dir, "neoflash-editor")?;
                verify_no_escaping_symlinks(&payload)?;
                verify_linux_elf64_executable(&payload.join("neoflash-editor"))?;
                (payload, PathBuf::from("neoflash-editor"), false)
            } else if archive_name_lower.ends_with(".appimage") {
                let staged_file = unpacked_dir.join("NeoFlash.AppImage");
                std::fs::copy(archive_path, &staged_file)
                    .map_err(|e| format!("Could not stage AppImage update: {e}"))?;
                verify_linux_elf64_executable(&staged_file)?;
                (staged_file, PathBuf::new(), true)
            } else {
                return Err(format!(
                    "Unsupported Linux update package '{}'; expected .tar.gz or .AppImage.",
                    archive_path.display()
                ));
            };

            let relaunch_exe = if is_single_file {
                effective_target.clone()
            } else {
                effective_target.join(&relaunch_rel)
            };
            let helper_path = staging_guard.path.join("swap-and-relaunch.sh");
            write_executable_script(
                &helper_path,
                &build_linux_swap_script(
                    &staged_payload,
                    &effective_target,
                    &relaunch_exe,
                    &staging_guard.path,
                    std::process::id(),
                    true,
                ),
            )?;

            let staging_root = staging_guard.disarm();
            Ok(PreparedSelfUpdate {
                platform: config.platform,
                staging_root,
                staged_payload_path: staged_payload,
                target_install_path: effective_target,
                relaunch_executable_rel: relaunch_rel,
                bundle_identifier: None,
                bundle_version: None,
                helper_script_path: Some(helper_path),
                handed_off_to_helper: false,
            })
        }
        PlatformTarget::Unsupported => Err(format!(
            "Self-update is not supported on {}.",
            config.platform.label()
        )),
    }
}

fn stage_macos_zip_bundle(archive_path: &Path, unpacked_dir: &Path) -> Result<PathBuf, String> {
    let raw_zip = std::fs::read(archive_path).map_err(|e| {
        format!(
            "Could not read update ZIP '{}': {e}",
            archive_path.display()
        )
    })?;
    // Pre-scan Central Directory before invoking ditto so zip-slip or escaping symlinks fail
    // closed before touching the filesystem.
    let _ = inspect_zip_safety(&raw_zip, true)?;

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(archive_path)
            .arg(unpacked_dir)
            .output()
            .map_err(|e| format!("Could not execute /usr/bin/ditto to unpack update ZIP: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Failed unpacking macOS update ZIP via ditto: {}",
                stderr.trim()
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        extract_zip_archive_safely(archive_path, unpacked_dir, true)?;
    }

    locate_staged_macos_app(unpacked_dir)
}

#[cfg(target_os = "macos")]
fn stage_macos_dmg_bundle(
    archive_path: &Path,
    staging_root: &Path,
    unpacked_dir: &Path,
) -> Result<PathBuf, String> {
    let mount_point = staging_root.join("dmg-mnt");
    std::fs::create_dir_all(&mount_point)
        .map_err(|e| format!("Could not create DMG mount directory: {e}"))?;
    let mut guard = DmgMountGuard {
        mount_point: mount_point.clone(),
        mounted: false,
    };
    let attach_out = std::process::Command::new("/usr/bin/hdiutil")
        .args([
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount_point)
        .arg(archive_path)
        .output()
        .map_err(|e| format!("Could not execute /usr/bin/hdiutil to mount update DMG: {e}"))?;
    if !attach_out.status.success() {
        let stderr = String::from_utf8_lossy(&attach_out.stderr);
        return Err(format!(
            "Failed mounting update DMG '{}': {}",
            archive_path.display(),
            stderr.trim()
        ));
    }
    guard.mounted = true;

    let mounted_app = locate_staged_macos_app(&mount_point)?;
    verify_no_escaping_symlinks(&mounted_app)?;

    let dest_app = unpacked_dir.join("NeoFlash.app");
    let copy_out = std::process::Command::new("/usr/bin/ditto")
        .arg(&mounted_app)
        .arg(&dest_app)
        .output()
        .map_err(|e| format!("Could not copy NeoFlash.app from mounted DMG: {e}"))?;
    let detach_res = guard.detach_now();
    if !copy_out.status.success() {
        let stderr = String::from_utf8_lossy(&copy_out.stderr);
        return Err(format!(
            "Failed copying NeoFlash.app from mounted DMG: {}",
            stderr.trim()
        ));
    }
    detach_res?;
    Ok(dest_app)
}

#[cfg(not(target_os = "macos"))]
fn stage_macos_dmg_bundle(
    archive_path: &Path,
    _staging_root: &Path,
    _unpacked_dir: &Path,
) -> Result<PathBuf, String> {
    Err(format!(
        "Mounting macOS .dmg archive '{}' is only supported on macOS.",
        archive_path.display()
    ))
}

fn locate_staged_macos_app(dir: &Path) -> Result<PathBuf, String> {
    let direct = dir.join("NeoFlash.app");
    if let Ok(meta) = std::fs::symlink_metadata(&direct) {
        if meta.file_type().is_symlink() {
            return Err(
                "Staged NeoFlash.app must be a directory bundle, not a symlink.".to_owned(),
            );
        }
        if meta.is_dir() {
            return Ok(direct);
        }
    }
    Err(format!(
        "Update archive did not contain 'NeoFlash.app' at root of '{}'.",
        dir.display()
    ))
}

fn verify_no_symlinks_outside_bundle(
    unpacked_dir: &Path,
    bundle_root: &Path,
) -> Result<(), String> {
    let entries = std::fs::read_dir(unpacked_dir)
        .map_err(|e| format!("Could not inspect unpacked update root: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Could not read unpacked update entry: {e}"))?;
        let path = entry.path();
        if path == bundle_root {
            continue;
        }
        let ft = entry
            .file_type()
            .map_err(|e| format!("Could not inspect file type of '{}': {e}", path.display()))?;
        if ft.is_symlink() {
            return Err(format!(
                "Update archive contains a forbidden top-level symlink '{}'.",
                path.display()
            ));
        }
        if ft.is_dir() {
            verify_no_escaping_symlinks_in_aux_dir(&path, unpacked_dir)?;
        }
    }
    Ok(())
}

fn verify_no_escaping_symlinks_in_aux_dir(dir: &Path, root: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Could not inspect auxiliary archive directory: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Could not read auxiliary archive entry: {e}"))?;
        let ft = entry
            .file_type()
            .map_err(|e| format!("Could not inspect auxiliary entry type: {e}"))?;
        if ft.is_symlink() {
            return Err(format!(
                "Update archive contains a symlink '{}' outside NeoFlash.app (root '{}').",
                entry.path().display(),
                root.display()
            ));
        }
        if ft.is_dir() {
            verify_no_escaping_symlinks_in_aux_dir(&entry.path(), root)?;
        }
    }
    Ok(())
}

/// Reject any symlink inside `root_dir` whose target is absolute or escapes `root_dir`.
pub(crate) fn verify_no_escaping_symlinks(root_dir: &Path) -> Result<(), String> {
    let root_meta = std::fs::symlink_metadata(root_dir).map_err(|e| {
        format!(
            "Could not inspect staged bundle '{}': {e}",
            root_dir.display()
        )
    })?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(format!(
            "Staged payload '{}' must be a real directory, not a symlink.",
            root_dir.display()
        ));
    }
    let canon_root = std::fs::canonicalize(root_dir).map_err(|e| {
        format!(
            "Could not canonicalize staged root '{}': {e}",
            root_dir.display()
        )
    })?;
    walk_dir_checking_symlinks(root_dir, root_dir, &canon_root)
}

fn walk_dir_checking_symlinks(
    current_dir: &Path,
    bundle_root: &Path,
    canon_root: &Path,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current_dir).map_err(|e| {
        format!(
            "Could not read staged directory '{}': {e}",
            current_dir.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Error reading staged entry: {e}"))?;
        let path = entry.path();
        let ft = entry
            .file_type()
            .map_err(|e| format!("Could not inspect '{}': {e}", path.display()))?;
        if ft.is_symlink() {
            let link_target = std::fs::read_link(&path).map_err(|e| {
                format!(
                    "Could not read symlink target for '{}': {e}",
                    path.display()
                )
            })?;
            if link_target.is_absolute() {
                return Err(format!(
                    "Rejected symlink '{}' pointing to absolute path '{}'.",
                    path.display(),
                    link_target.display()
                ));
            }
            let rel_parent = current_dir.strip_prefix(bundle_root).map_err(|_| {
                format!(
                    "Internal error computing relative path inside '{}'.",
                    bundle_root.display()
                )
            })?;
            let mut depth_parts: Vec<std::ffi::OsString> = rel_parent
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_os_string()),
                    _ => None,
                })
                .collect();
            for comp in link_target.components() {
                match comp {
                    std::path::Component::CurDir => {}
                    std::path::Component::Normal(seg) => depth_parts.push(seg.to_os_string()),
                    std::path::Component::ParentDir => {
                        if depth_parts.pop().is_none() {
                            return Err(format!(
                                "Rejected symlink '{}' escaping bundle root via '{}'.",
                                path.display(),
                                link_target.display()
                            ));
                        }
                    }
                    _ => {
                        return Err(format!(
                            "Rejected symlink '{}' with unsafe target '{}'.",
                            path.display(),
                            link_target.display()
                        ));
                    }
                }
            }
            if let Ok(canon_resolved) = std::fs::canonicalize(&path)
                && !canon_resolved.starts_with(canon_root)
            {
                return Err(format!(
                    "Rejected symlink '{}' resolving outside bundle ('{}').",
                    path.display(),
                    canon_resolved.display()
                ));
            }
        } else if ft.is_dir() {
            walk_dir_checking_symlinks(&path, bundle_root, canon_root)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct MacosPlistIdentity {
    pub bundle_id: String,
    pub executable: String,
    pub short_version: Option<String>,
}

pub(crate) fn parse_macos_info_plist(plist_path: &Path) -> Result<MacosPlistIdentity, String> {
    let meta = std::fs::symlink_metadata(plist_path).map_err(|e| {
        format!(
            "Missing bundle Info.plist at '{}': {e}",
            plist_path.display()
        )
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!(
            "Bundle Info.plist '{}' must be a regular file.",
            plist_path.display()
        ));
    }
    if meta.len() == 0 || meta.len() as usize > MAX_PLIST_BYTES {
        return Err(format!(
            "Bundle Info.plist '{}' is empty or exceeds size limit.",
            plist_path.display()
        ));
    }
    let raw = std::fs::read(plist_path)
        .map_err(|e| format!("Could not read '{}': {e}", plist_path.display()))?;
    let xml_text = if raw.starts_with(b"bplist00") {
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("/usr/bin/plutil")
                .args(["-convert", "xml1", "-o", "-"])
                .arg(plist_path)
                .output()
                .map_err(|e| format!("Could not run plutil on binary Info.plist: {e}"))?;
            if !out.status.success() {
                return Err("Binary Info.plist failed plutil XML conversion.".to_owned());
            }
            String::from_utf8(out.stdout)
                .map_err(|e| format!("Converted Info.plist is not valid UTF-8: {e}"))?
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Err("Binary Info.plist parsing requires macOS plutil.".to_owned());
        }
    } else {
        String::from_utf8(raw).map_err(|e| {
            format!(
                "Info.plist '{}' is not valid UTF-8: {e}",
                plist_path.display()
            )
        })?
    };

    let bundle_id = extract_plist_string_value(&xml_text, "CFBundleIdentifier")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "Info.plist '{}' is missing CFBundleIdentifier.",
                plist_path.display()
            )
        })?;
    let executable = extract_plist_string_value(&xml_text, "CFBundleExecutable")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "neoflash-editor".to_owned());
    if !is_safe_asset_filename(&executable) {
        return Err(format!(
            "Info.plist declares unsafe CFBundleExecutable '{executable}'."
        ));
    }
    let short_version = extract_plist_string_value(&xml_text, "CFBundleShortVersionString")
        .filter(|s| !s.is_empty());

    Ok(MacosPlistIdentity {
        bundle_id,
        executable,
        short_version,
    })
}

fn extract_plist_string_value(xml: &str, key_name: &str) -> Option<String> {
    let needle = format!("<key>{key_name}</key>");
    let start = xml.find(&needle)? + needle.len();
    let rest = xml[start..].trim_start();
    let after_open = rest.strip_prefix("<string>")?;
    let end = after_open.find("</string>")?;
    Some(unescape_basic_xml(after_open[..end].trim()))
}

fn unescape_basic_xml(raw: &str) -> String {
    raw.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn verify_macos_bundle_plist(
    staged_app: &Path,
    target_install_path: &Path,
) -> Result<MacosPlistIdentity, String> {
    let staged_plist = staged_app.join("Contents/Info.plist");
    let staged_info = parse_macos_info_plist(&staged_plist)?;
    if !ALLOWED_MACOS_BUNDLE_IDS.contains(&staged_info.bundle_id.as_str()) {
        return Err(format!(
            "Rejected update bundle: CFBundleIdentifier '{}' is not an authorized NeoFlash bundle identifier (expected {}).",
            staged_info.bundle_id,
            ALLOWED_MACOS_BUNDLE_IDS.join(" or ")
        ));
    }
    let existing_plist = target_install_path.join("Contents/Info.plist");
    if existing_plist.is_file()
        && let Ok(existing_info) = parse_macos_info_plist(&existing_plist)
        && staged_info.bundle_id != existing_info.bundle_id
    {
        return Err(format!(
            "Rejected update bundle: CFBundleIdentifier mismatch (staged '{}' vs installed '{}').",
            staged_info.bundle_id, existing_info.bundle_id
        ));
    }
    Ok(staged_info)
}

fn verify_macos_executable(exe_path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(exe_path).map_err(|e| {
        format!(
            "Staged macOS bundle is missing executable '{}': {e}",
            exe_path.display()
        )
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() == 0 {
        return Err(format!(
            "Staged macOS executable '{}' must be a non-empty regular file.",
            exe_path.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if (meta.permissions().mode() & 0o111) == 0 {
            return Err(format!(
                "Staged macOS executable '{}' lacks execute permissions.",
                exe_path.display()
            ));
        }
    }
    Ok(())
}

fn verify_macos_codesign(staged_app: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(staged_app)
            .output()
            .map_err(|e| format!("Could not execute /usr/bin/codesign: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            return Err(format!(
                "Code signature verification failed for staged NeoFlash.app{}",
                if detail.is_empty() {
                    ".".to_owned()
                } else {
                    format!(": {detail}")
                }
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = staged_app;
    }
    Ok(())
}

pub(crate) fn verify_windows_pe64_executable(exe_path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(exe_path).map_err(|e| {
        format!(
            "Missing staged Windows executable '{}': {e}",
            exe_path.display()
        )
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!(
            "Staged Windows executable '{}' must be a regular file.",
            exe_path.display()
        ));
    }
    let mut file = std::fs::File::open(exe_path)
        .map_err(|e| format!("Could not open '{}': {e}", exe_path.display()))?;
    let mut buf = vec![0u8; 4096.min(meta.len() as usize)];
    use std::io::Read as _;
    file.read_exact(&mut buf).map_err(|e| {
        format!(
            "Could not read PE header from '{}': {e}",
            exe_path.display()
        )
    })?;
    if buf.len() < 0x40 || &buf[0..2] != b"MZ" {
        return Err(format!(
            "Staged '{}' is not a valid Windows PE executable (missing MZ header).",
            exe_path.display()
        ));
    }
    let e_lfanew = u32::from_le_bytes([buf[0x3c], buf[0x3d], buf[0x3e], buf[0x3f]]) as usize;
    if e_lfanew < 0x40 || e_lfanew + 26 > buf.len() {
        return Err(format!(
            "Staged '{}' has an invalid PE e_lfanew header offset (0x{e_lfanew:x}).",
            exe_path.display()
        ));
    }
    if &buf[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return Err(format!(
            "Staged '{}' is missing the PE\\0\\0 NT signature.",
            exe_path.display()
        ));
    }
    let machine = u16::from_le_bytes([buf[e_lfanew + 4], buf[e_lfanew + 5]]);
    if machine != 0x8664 {
        return Err(format!(
            "Staged '{}' is not an x64 PE image (machine=0x{machine:04x}, expected 0x8664).",
            exe_path.display()
        ));
    }
    let opt_hdr_size = u16::from_le_bytes([buf[e_lfanew + 20], buf[e_lfanew + 21]]) as usize;
    let opt_magic = u16::from_le_bytes([buf[e_lfanew + 24], buf[e_lfanew + 25]]);
    if opt_hdr_size < 24 || opt_magic != 0x020b {
        return Err(format!(
            "Staged '{}' is not a PE32+ 64-bit executable (magic=0x{opt_magic:04x}).",
            exe_path.display()
        ));
    }
    Ok(())
}

pub(crate) fn verify_linux_elf64_executable(exe_path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(exe_path).map_err(|e| {
        format!(
            "Missing staged Linux executable '{}': {e}",
            exe_path.display()
        )
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!(
            "Staged Linux executable '{}' must be a regular file.",
            exe_path.display()
        ));
    }
    let mut file = std::fs::File::open(exe_path)
        .map_err(|e| format!("Could not open '{}': {e}", exe_path.display()))?;
    let mut hdr = [0u8; 64];
    use std::io::Read as _;
    file.read_exact(&mut hdr).map_err(|e| {
        format!(
            "Staged '{}' is truncated or unreadable as ELF64: {e}",
            exe_path.display()
        )
    })?;
    if &hdr[0..4] != b"\x7fELF" {
        return Err(format!(
            "Staged '{}' lacks the \\x7fELF signature.",
            exe_path.display()
        ));
    }
    if hdr[4] != 2 || hdr[5] != 1 {
        return Err(format!(
            "Staged '{}' is not a 64-bit little-endian ELF image (class={}, data={}).",
            exe_path.display(),
            hdr[4],
            hdr[5]
        ));
    }
    let e_type = u16::from_le_bytes([hdr[16], hdr[17]]);
    let e_machine = u16::from_le_bytes([hdr[18], hdr[19]]);
    if (e_type != 2 && e_type != 3) || e_machine != 0x003e {
        return Err(format!(
            "Staged '{}' is not an x86_64 ELF executable (type={e_type}, machine=0x{e_machine:04x}).",
            exe_path.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(exe_path, std::fs::Permissions::from_mode(0o755)).map_err(
            |e| {
                format!(
                    "Could not mark '{}' executable (0755): {e}",
                    exe_path.display()
                )
            },
        )?;
    }
    Ok(())
}

fn resolve_extracted_payload_dir(unpacked_dir: &Path, exe_name: &str) -> Result<PathBuf, String> {
    if unpacked_dir.join(exe_name).is_file() {
        return Ok(unpacked_dir.to_path_buf());
    }
    let mut child_dirs = Vec::new();
    let entries = std::fs::read_dir(unpacked_dir)
        .map_err(|e| format!("Could not inspect unpacked directory: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Could not read unpacked entry: {e}"))?;
        let ft = entry
            .file_type()
            .map_err(|e| format!("Could not inspect unpacked entry type: {e}"))?;
        if ft.is_symlink() {
            return Err(format!(
                "Rejected symlink '{}' in unpacked root.",
                entry.path().display()
            ));
        }
        if ft.is_dir() {
            child_dirs.push(entry.path());
        }
    }
    if child_dirs.len() == 1 && child_dirs[0].join(exe_name).is_file() {
        return Ok(child_dirs.remove(0));
    }
    Err(format!(
        "Extracted update package does not contain '{exe_name}'."
    ))
}

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

struct Crc32Hasher {
    state: u32,
}

impl Crc32Hasher {
    const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.state;
        for &b in bytes {
            let idx = ((crc as u8) ^ b) as usize;
            crc = (crc >> 8) ^ CRC32_TABLE[idx];
        }
        self.state = crc;
    }

    const fn finalize(self) -> u32 {
        !self.state
    }
}

#[allow(dead_code)]
fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut hasher = Crc32Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

// ---------------------------------------------------------------------------
// Streaming RFC 1951 raw DEFLATE reader (constant 32 KiB window, >256 MiB safe)
// ---------------------------------------------------------------------------

const DEFLATE_LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const DEFLATE_LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DEFLATE_DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEFLATE_DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const DEFLATE_CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

#[derive(Clone)]
struct HuffmanTable {
    fast: [u16; 512],
    counts: [u16; 16],
    symbols: [u16; 288],
}

impl HuffmanTable {
    fn build(lengths: &[u8]) -> std::io::Result<Self> {
        let mut counts = [0u16; 16];
        for &len in lengths {
            if len > 15 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "DEFLATE Huffman code length exceeds 15 bits",
                ));
            }
            if len > 0 {
                counts[len as usize] += 1;
            }
        }
        let mut left = 1i32;
        for &cnt in &counts[1..=15] {
            left = (left << 1) - i32::from(cnt);
            if left < 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Oversubscribed DEFLATE Huffman tree",
                ));
            }
        }
        let mut offsets = [0u16; 16];
        let mut next_code = [0u16; 16];
        let mut sum = 0u16;
        let mut code = 0u16;
        for bits in 1..=15 {
            offsets[bits] = sum;
            sum += counts[bits];
            code = (code + counts[bits - 1]) << 1;
            next_code[bits] = code;
        }
        let mut symbols = [0u16; 288];
        let mut fast = [0u16; 512];
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let len_usize = len as usize;
            let idx = offsets[len_usize] as usize;
            offsets[len_usize] += 1;
            symbols[idx] = sym as u16;

            let canonical = next_code[len_usize];
            next_code[len_usize] += 1;
            if len <= 9 {
                let rev = canonical.reverse_bits() >> (16 - len);
                let entry = ((len as u16) << 10) | (sym as u16);
                let step = 1usize << len;
                let mut slot = rev as usize;
                while slot < 512 {
                    fast[slot] = entry;
                    slot += step;
                }
            }
        }
        Ok(Self {
            fast,
            counts,
            symbols,
        })
    }

    fn fixed_lit_len() -> Self {
        let mut lengths = [0u8; 288];
        lengths[0..=143].fill(8);
        lengths[144..=255].fill(9);
        lengths[256..=279].fill(7);
        lengths[280..=287].fill(8);
        Self::build(&lengths).expect("valid RFC 1951 fixed literal/length tree")
    }

    fn fixed_dist() -> Self {
        let lengths = [5u8; 32];
        Self::build(&lengths).expect("valid RFC 1951 fixed distance tree")
    }
}

enum DeflateBlockState {
    NeedHeader,
    Stored {
        remaining: usize,
    },
    Compressed {
        lit_len: Box<HuffmanTable>,
        dist: Box<HuffmanTable>,
        match_dist: usize,
        match_rem: usize,
    },
    Finished,
}

pub(crate) struct DeflateStreamDecoder<R> {
    inner: R,
    in_buf: Box<[u8; 8192]>,
    in_pos: usize,
    in_len: usize,
    in_eof: bool,
    bit_buf: u64,
    bit_count: u8,
    final_block: bool,
    state: DeflateBlockState,
    window: Box<[u8; 32768]>,
    win_pos: usize,
    total_out: u64,
}

impl<R: std::io::Read> DeflateStreamDecoder<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            in_buf: Box::new([0u8; 8192]),
            in_pos: 0,
            in_len: 0,
            in_eof: false,
            bit_buf: 0,
            bit_count: 0,
            final_block: false,
            state: DeflateBlockState::NeedHeader,
            window: Box::new([0u8; 32768]),
            win_pos: 0,
            total_out: 0,
        }
    }

    fn pull_byte(&mut self) -> std::io::Result<Option<u8>> {
        if self.in_pos >= self.in_len {
            if self.in_eof {
                return Ok(None);
            }
            self.in_pos = 0;
            self.in_len = self.inner.read(&mut self.in_buf[..])?;
            if self.in_len == 0 {
                self.in_eof = true;
                return Ok(None);
            }
        }
        let b = self.in_buf[self.in_pos];
        self.in_pos += 1;
        Ok(Some(b))
    }

    fn fill_bits(&mut self, min_bits: u8) -> std::io::Result<()> {
        while self.bit_count < min_bits {
            let Some(b) = self.pull_byte()? else {
                break;
            };
            self.bit_buf |= u64::from(b) << self.bit_count;
            self.bit_count += 8;
        }
        Ok(())
    }

    fn read_bits(&mut self, n: u8) -> std::io::Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        self.fill_bits(n)?;
        if self.bit_count < n {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Truncated DEFLATE bitstream",
            ));
        }
        let val = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.bit_buf >>= n;
        self.bit_count -= n;
        Ok(val)
    }

    fn align_to_byte(&mut self) {
        let drop_bits = self.bit_count & 7;
        self.bit_buf >>= drop_bits;
        self.bit_count -= drop_bits;
    }

    fn read_aligned_byte(&mut self) -> std::io::Result<u8> {
        if self.bit_count >= 8 {
            let b = (self.bit_buf & 0xFF) as u8;
            self.bit_buf >>= 8;
            self.bit_count -= 8;
            return Ok(b);
        }
        self.pull_byte()?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Truncated DEFLATE stored block",
            )
        })
    }

    fn decode_symbol(&mut self, table: &HuffmanTable) -> std::io::Result<u16> {
        self.fill_bits(15)?;
        if self.bit_count >= 9 {
            let entry = table.fast[(self.bit_buf & 0x1FF) as usize];
            if entry != 0 {
                let len = (entry >> 10) as u8;
                self.bit_buf >>= len;
                self.bit_count -= len;
                return Ok(entry & 0x3FF);
            }
        }
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: usize = 0;
        for len in 1..=15usize {
            if self.bit_count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Unexpected EOF decoding DEFLATE Huffman symbol",
                ));
            }
            code |= (self.bit_buf & 1) as i32;
            self.bit_buf >>= 1;
            self.bit_count -= 1;
            let count = i32::from(table.counts[len]);
            if code - count < first {
                return Ok(table.symbols[index + (code - first) as usize]);
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid DEFLATE Huffman code",
        ))
    }

    fn parse_dynamic_tables(&mut self) -> std::io::Result<(HuffmanTable, HuffmanTable)> {
        let hlit = self.read_bits(5)? as usize + 257;
        let hdist = self.read_bits(5)? as usize + 1;
        let hclen = self.read_bits(4)? as usize + 4;
        if hlit > 286 || hdist > 30 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid DEFLATE dynamic table counts",
            ));
        }
        let mut cl_lengths = [0u8; 19];
        for i in 0..hclen {
            cl_lengths[DEFLATE_CL_ORDER[i]] = self.read_bits(3)? as u8;
        }
        let cl_table = HuffmanTable::build(&cl_lengths)?;

        let total = hlit + hdist;
        let mut lengths = [0u8; 318];
        let mut idx = 0usize;
        while idx < total {
            let sym = self.decode_symbol(&cl_table)?;
            match sym {
                0..=15 => {
                    lengths[idx] = sym as u8;
                    idx += 1;
                }
                16 => {
                    if idx == 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "DEFLATE repeat code 16 at start of code lengths",
                        ));
                    }
                    let prev = lengths[idx - 1];
                    let repeat = self.read_bits(2)? as usize + 3;
                    if idx + repeat > total {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "DEFLATE code length repeat 16 overflows table",
                        ));
                    }
                    lengths[idx..idx + repeat].fill(prev);
                    idx += repeat;
                }
                17 => {
                    let repeat = self.read_bits(3)? as usize + 3;
                    if idx + repeat > total {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "DEFLATE code length repeat 17 overflows table",
                        ));
                    }
                    lengths[idx..idx + repeat].fill(0);
                    idx += repeat;
                }
                18 => {
                    let repeat = self.read_bits(7)? as usize + 11;
                    if idx + repeat > total {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "DEFLATE code length repeat 18 overflows table",
                        ));
                    }
                    lengths[idx..idx + repeat].fill(0);
                    idx += repeat;
                }
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid DEFLATE code-length symbol",
                    ));
                }
            }
        }
        if lengths[256] == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "DEFLATE dynamic block lacks end-of-block symbol 256",
            ));
        }
        let lit_len = HuffmanTable::build(&lengths[..hlit])?;
        let dist = HuffmanTable::build(&lengths[hlit..total])?;
        Ok((lit_len, dist))
    }

    #[inline]
    fn emit_byte(&mut self, b: u8, out: &mut [u8], written: &mut usize) {
        self.window[self.win_pos] = b;
        self.win_pos = (self.win_pos + 1) & 0x7FFF;
        self.total_out += 1;
        out[*written] = b;
        *written += 1;
    }
}

impl<R: std::io::Read> std::io::Read for DeflateStreamDecoder<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut written = 0usize;
        while written < out.len() {
            match &mut self.state {
                DeflateBlockState::Finished => break,
                DeflateBlockState::NeedHeader => {
                    if self.final_block {
                        self.state = DeflateBlockState::Finished;
                        break;
                    }
                    let bfinal = self.read_bits(1)?;
                    let btype = self.read_bits(2)?;
                    self.final_block = bfinal == 1;
                    match btype {
                        0 => {
                            self.align_to_byte();
                            let lo = u16::from(self.read_aligned_byte()?);
                            let hi = u16::from(self.read_aligned_byte()?);
                            let len = lo | (hi << 8);
                            let nlo = u16::from(self.read_aligned_byte()?);
                            let nhi = u16::from(self.read_aligned_byte()?);
                            let nlen = nlo | (nhi << 8);
                            if len != !nlen {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "DEFLATE stored block LEN/NLEN complement mismatch",
                                ));
                            }
                            self.state = DeflateBlockState::Stored {
                                remaining: len as usize,
                            };
                        }
                        1 => {
                            self.state = DeflateBlockState::Compressed {
                                lit_len: Box::new(HuffmanTable::fixed_lit_len()),
                                dist: Box::new(HuffmanTable::fixed_dist()),
                                match_dist: 0,
                                match_rem: 0,
                            };
                        }
                        2 => {
                            let (ll, d) = self.parse_dynamic_tables()?;
                            self.state = DeflateBlockState::Compressed {
                                lit_len: Box::new(ll),
                                dist: Box::new(d),
                                match_dist: 0,
                                match_rem: 0,
                            };
                        }
                        _ => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Reserved DEFLATE block type 3",
                            ));
                        }
                    }
                }
                DeflateBlockState::Stored { remaining } => {
                    if *remaining == 0 {
                        self.state = DeflateBlockState::NeedHeader;
                        continue;
                    }
                    let mut rem = *remaining;
                    while rem > 0 && written < out.len() {
                        let b = self.read_aligned_byte()?;
                        self.emit_byte(b, out, &mut written);
                        rem -= 1;
                    }
                    if rem == 0 {
                        self.state = DeflateBlockState::NeedHeader;
                    } else {
                        self.state = DeflateBlockState::Stored { remaining: rem };
                    }
                }
                DeflateBlockState::Compressed { .. } => {
                    let DeflateBlockState::Compressed {
                        lit_len,
                        dist,
                        mut match_dist,
                        mut match_rem,
                    } = std::mem::replace(&mut self.state, DeflateBlockState::NeedHeader)
                    else {
                        unreachable!()
                    };
                    let mut block_done = false;
                    while written < out.len() {
                        if match_rem > 0 {
                            let src_idx = self.win_pos.wrapping_sub(match_dist) & 0x7FFF;
                            let b = self.window[src_idx];
                            self.emit_byte(b, out, &mut written);
                            match_rem -= 1;
                            continue;
                        }
                        let sym = self.decode_symbol(&lit_len)?;
                        if sym < 256 {
                            self.emit_byte(sym as u8, out, &mut written);
                        } else if sym == 256 {
                            block_done = true;
                            break;
                        } else if sym <= 285 {
                            let l_idx = (sym - 257) as usize;
                            let length = usize::from(DEFLATE_LEN_BASE[l_idx])
                                + self.read_bits(DEFLATE_LEN_EXTRA[l_idx])? as usize;
                            let d_sym = self.decode_symbol(&dist)? as usize;
                            if d_sym >= 30 {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "Illegal DEFLATE distance symbol >= 30",
                                ));
                            }
                            let distance = usize::from(DEFLATE_DIST_BASE[d_sym])
                                + self.read_bits(DEFLATE_DIST_EXTRA[d_sym])? as usize;
                            if distance as u64 > self.total_out {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "DEFLATE distance references before start of stream",
                                ));
                            }
                            match_dist = distance;
                            match_rem = length;
                        } else {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Illegal DEFLATE literal/length symbol >= 286",
                            ));
                        }
                    }
                    if !block_done {
                        self.state = DeflateBlockState::Compressed {
                            lit_len,
                            dist,
                            match_dist,
                            match_rem,
                        };
                    }
                }
            }
        }
        Ok(written)
    }
}

fn validate_archive_entry_rel_path(raw_name: &str) -> Result<(PathBuf, bool), String> {
    if raw_name.is_empty() {
        return Err("Update archive contains an entry with an empty path.".to_owned());
    }
    if raw_name.starts_with('/') || raw_name.starts_with('\\') {
        return Err(format!(
            "Unsafe absolute path '{raw_name}' rejected in update archive."
        ));
    }
    if raw_name.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(format!(
            "Unsafe control characters in archive path '{raw_name}'."
        ));
    }
    let is_dir = raw_name.ends_with('/') || raw_name.ends_with('\\');
    let trimmed = raw_name.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err(format!(
            "Unsafe root path '{raw_name}' rejected in update archive."
        ));
    }
    let mut rel = PathBuf::new();
    for seg in trimmed.split(['/', '\\']) {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains(':') {
            return Err(format!(
                "Unsafe path traversal or drive component in archive entry '{raw_name}'."
            ));
        }
        rel.push(seg);
    }
    Ok((rel, is_dir))
}

#[derive(Debug)]
pub(crate) struct ZipEntryRecord {
    pub rel_path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub local_header_offset: u64,
    #[cfg_attr(not(unix), allow(dead_code))]
    pub unix_mode: u32,
}

fn validate_zip_extra_fields(extra: &[u8], context: &str) -> Result<bool, String> {
    let mut pos = 0usize;
    let mut saw_zip64 = false;
    while pos < extra.len() {
        if pos + 4 > extra.len() {
            return Err(format!("Truncated ZIP extra field header in {context}."));
        }
        let tag = u16::from_le_bytes([extra[pos], extra[pos + 1]]);
        let len = u16::from_le_bytes([extra[pos + 2], extra[pos + 3]]) as usize;
        pos += 4;
        if pos + len > extra.len() {
            return Err(format!(
                "Out-of-bounds ZIP extra field payload in {context}."
            ));
        }
        // Reject Info-ZIP Unicode Path (0x7075) and Info-ZIP Unicode Comment (0x6375) overrides so
        // system extractors such as `/usr/bin/ditto` can never interpret a different entry path
        // than `inspect_zip_safety`.
        if tag == 0x7075 || tag == 0x6375 {
            return Err(format!(
                "Rejected ZIP archive containing Info-ZIP Unicode override extra tag 0x{tag:04x} in {context}."
            ));
        }
        if tag == 0x0001 {
            saw_zip64 = true;
        }
        pos += len;
    }
    Ok(saw_zip64)
}

fn verify_zip_data_descriptor(
    gap: &[u8],
    expected_crc: u32,
    expected_comp: u64,
    expected_uncomp: u64,
) -> Result<(), String> {
    let body = if gap.starts_with(b"PK\x07\x08") {
        &gap[4..]
    } else {
        gap
    };
    match body.len() {
        12 => {
            let crc = u32::from_le_bytes(body[0..4].try_into().unwrap());
            let comp = u32::from_le_bytes(body[4..8].try_into().unwrap()) as u64;
            let uncomp = u32::from_le_bytes(body[8..12].try_into().unwrap()) as u64;
            if crc != expected_crc
                || comp != (expected_comp & 0xFFFF_FFFF)
                || uncomp != (expected_uncomp & 0xFFFF_FFFF)
            {
                return Err("ZIP data descriptor CRC/size mismatch.".to_owned());
            }
            Ok(())
        }
        20 => {
            let crc = u32::from_le_bytes(body[0..4].try_into().unwrap());
            let comp = u64::from_le_bytes(body[4..12].try_into().unwrap());
            let uncomp = u64::from_le_bytes(body[12..20].try_into().unwrap());
            if crc != expected_crc || comp != expected_comp || uncomp != expected_uncomp {
                return Err("ZIP64 data descriptor CRC/size mismatch.".to_owned());
            }
            Ok(())
        }
        _ => Err(format!(
            "Invalid or unexpected {}-byte gap after streaming ZIP entry.",
            gap.len()
        )),
    }
}

pub(crate) fn inspect_zip_safety(
    bytes: &[u8],
    allow_macos_bundle_symlinks: bool,
) -> Result<Vec<ZipEntryRecord>, String> {
    if bytes.len() < 22 {
        return Err("Archive is too small to be a valid ZIP file.".to_owned());
    }
    let search_start = bytes.len().saturating_sub(22 + 65_535);
    let mut eocd_offset = None;
    for pos in (search_start..=bytes.len() - 22).rev() {
        if &bytes[pos..pos + 4] == b"PK\x05\x06" {
            let comment_len = u16::from_le_bytes([bytes[pos + 20], bytes[pos + 21]]) as usize;
            if pos + 22 + comment_len == bytes.len() {
                eocd_offset = Some(pos);
                break;
            }
        }
    }
    let eocd = eocd_offset
        .ok_or_else(|| "Invalid ZIP archive: End of Central Directory not found.".to_owned())?;
    let mut total_entries = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as u64;
    let mut cd_size = u32::from_le_bytes([
        bytes[eocd + 12],
        bytes[eocd + 13],
        bytes[eocd + 14],
        bytes[eocd + 15],
    ]) as u64;
    let mut cd_offset = u32::from_le_bytes([
        bytes[eocd + 16],
        bytes[eocd + 17],
        bytes[eocd + 18],
        bytes[eocd + 19],
    ]) as u64;
    let mut expected_after_cd = eocd;

    if (total_entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF)
        && eocd >= 20
        && &bytes[eocd - 20..eocd - 16] == b"PK\x06\x07"
    {
        let locator = eocd - 20;
        let z64_eocd = u64::from_le_bytes(
            bytes[locator + 8..locator + 16]
                .try_into()
                .map_err(|_| "Corrupt ZIP64 locator.")?,
        ) as usize;
        if z64_eocd + 56 > bytes.len() || &bytes[z64_eocd..z64_eocd + 4] != b"PK\x06\x06" {
            return Err("Corrupt ZIP64 End of Central Directory record.".to_owned());
        }
        total_entries = u64::from_le_bytes(bytes[z64_eocd + 32..z64_eocd + 40].try_into().unwrap());
        cd_size = u64::from_le_bytes(bytes[z64_eocd + 40..z64_eocd + 48].try_into().unwrap());
        cd_offset = u64::from_le_bytes(bytes[z64_eocd + 48..z64_eocd + 56].try_into().unwrap());
        expected_after_cd = z64_eocd;
    }

    if total_entries as usize > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "Update ZIP contains {total_entries} entries, exceeding safety limit ({MAX_ARCHIVE_ENTRIES})."
        ));
    }
    let cd_end = cd_offset
        .checked_add(cd_size)
        .ok_or_else(|| "Invalid ZIP central directory bounds.".to_owned())?;
    if cd_end > bytes.len() as u64 {
        return Err("Truncated ZIP central directory.".to_owned());
    }
    if cd_end as usize != expected_after_cd {
        return Err(
            "Rejected ZIP archive with unreferenced bytes between Central Directory and EOCD."
                .to_owned(),
        );
    }

    let mut records = Vec::with_capacity(total_entries as usize);
    let mut lfh_spans: Vec<(usize, usize, u16, u32, u64, u64)> =
        Vec::with_capacity(total_entries as usize);
    let mut cursor = cd_offset as usize;
    let end_usize = cd_end as usize;
    let mut cumulative_uncompressed: u64 = 0;

    for _ in 0..total_entries {
        if cursor + 46 > end_usize || &bytes[cursor..cursor + 4] != b"PK\x01\x02" {
            return Err("Corrupt ZIP central directory entry header.".to_owned());
        }
        let version_made_by = u16::from_le_bytes([bytes[cursor + 4], bytes[cursor + 5]]);
        let creator_os = version_made_by >> 8;
        let flags = u16::from_le_bytes([bytes[cursor + 8], bytes[cursor + 9]]);
        if (flags & 0x0001) != 0 {
            return Err("Encrypted ZIP entries are not allowed in NeoFlash updates.".to_owned());
        }
        let method = u16::from_le_bytes([bytes[cursor + 10], bytes[cursor + 11]]);
        if method != 0 && method != 8 {
            return Err(format!(
                "Unsupported ZIP compression method {method} (expected Stored=0 or Deflate=8)."
            ));
        }
        let crc32 = u32::from_le_bytes(bytes[cursor + 16..cursor + 20].try_into().unwrap());
        let comp_32 = u32::from_le_bytes(bytes[cursor + 20..cursor + 24].try_into().unwrap());
        let uncomp_32 = u32::from_le_bytes(bytes[cursor + 24..cursor + 28].try_into().unwrap());
        let name_len = u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[cursor + 30], bytes[cursor + 31]]) as usize;
        let comment_len = u16::from_le_bytes([bytes[cursor + 32], bytes[cursor + 33]]) as usize;
        let disk_start = u16::from_le_bytes([bytes[cursor + 34], bytes[cursor + 35]]);
        let external_attr = u32::from_le_bytes(bytes[cursor + 38..cursor + 42].try_into().unwrap());
        let lfh_32 = u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap());

        let name_start = cursor + 46;
        let extra_start = name_start
            .checked_add(name_len)
            .ok_or_else(|| "ZIP entry header overflow.".to_owned())?;
        let comment_start = extra_start
            .checked_add(extra_len)
            .ok_or_else(|| "ZIP entry extra overflow.".to_owned())?;
        let next_cursor = comment_start
            .checked_add(comment_len)
            .ok_or_else(|| "ZIP entry comment overflow.".to_owned())?;
        if next_cursor > end_usize {
            return Err("Truncated ZIP central directory entry.".to_owned());
        }

        let cd_name_bytes = &bytes[name_start..extra_start];
        let raw_name = std::str::from_utf8(cd_name_bytes)
            .map_err(|_| "ZIP entry filename is not valid UTF-8.".to_owned())?;
        let (rel_path, name_is_dir) = validate_archive_entry_rel_path(raw_name)?;
        let cd_extra = &bytes[extra_start..comment_start];
        let _ = validate_zip_extra_fields(cd_extra, "Central Directory")?;

        let mut uncompressed_size = u64::from(uncomp_32);
        let mut compressed_size = u64::from(comp_32);
        let mut local_header_offset = u64::from(lfh_32);

        if uncomp_32 == 0xFFFF_FFFF
            || comp_32 == 0xFFFF_FFFF
            || lfh_32 == 0xFFFF_FFFF
            || disk_start == 0xFFFF
        {
            parse_zip64_extra(
                cd_extra,
                uncomp_32 == 0xFFFF_FFFF,
                comp_32 == 0xFFFF_FFFF,
                lfh_32 == 0xFFFF_FFFF,
                &mut uncompressed_size,
                &mut compressed_size,
                &mut local_header_offset,
            )?;
        }

        // Cross-check the corresponding Local File Header (`PK\x03\x04`) against the Central
        // Directory record so parser differentials between `inspect_zip_safety` and system tools
        // like `/usr/bin/ditto` fail closed.
        let lfh = usize::try_from(local_header_offset)
            .map_err(|_| "ZIP local header offset overflow.".to_owned())?;
        if lfh + 30 > cd_offset as usize || &bytes[lfh..lfh + 4] != b"PK\x03\x04" {
            return Err(format!(
                "Corrupt or out-of-bounds ZIP Local File Header for '{}'.",
                rel_path.display()
            ));
        }
        let lfh_flags = u16::from_le_bytes([bytes[lfh + 6], bytes[lfh + 7]]);
        let lfh_method = u16::from_le_bytes([bytes[lfh + 8], bytes[lfh + 9]]);
        if (lfh_flags & 0x0001) != 0 || lfh_method != method {
            return Err(format!(
                "Rejected ZIP entry '{}': Local File Header flags/method mismatch.",
                rel_path.display()
            ));
        }
        let lfh_name_len = u16::from_le_bytes([bytes[lfh + 26], bytes[lfh + 27]]) as usize;
        let lfh_extra_len = u16::from_le_bytes([bytes[lfh + 28], bytes[lfh + 29]]) as usize;
        let lfh_name_end = lfh
            .checked_add(30)
            .and_then(|v| v.checked_add(lfh_name_len))
            .ok_or_else(|| "ZIP LFH name overflow.".to_owned())?;
        let data_start = lfh_name_end
            .checked_add(lfh_extra_len)
            .ok_or_else(|| "ZIP LFH extra overflow.".to_owned())?;
        let comp_usize = usize::try_from(compressed_size)
            .map_err(|_| "ZIP compressed size overflow.".to_owned())?;
        let data_end = data_start
            .checked_add(comp_usize)
            .ok_or_else(|| "ZIP payload end overflow.".to_owned())?;
        if data_end > cd_offset as usize {
            return Err(format!(
                "ZIP entry '{}' payload overlaps Central Directory.",
                rel_path.display()
            ));
        }
        if &bytes[lfh + 30..lfh_name_end] != cd_name_bytes {
            return Err(format!(
                "Rejected ZIP archive: Local File Header filename does not match Central Directory filename for '{}'.",
                rel_path.display()
            ));
        }
        let _ = validate_zip_extra_fields(&bytes[lfh_name_end..data_start], "Local File Header")?;
        lfh_spans.push((
            lfh,
            data_end,
            lfh_flags,
            crc32,
            compressed_size,
            uncompressed_size,
        ));

        if uncompressed_size > MAX_ARCHIVE_ENTRY_BYTES as u64 {
            return Err(format!(
                "ZIP entry '{}' uncompressed size ({uncompressed_size} bytes) exceeds safety limit.",
                rel_path.display()
            ));
        }
        cumulative_uncompressed = cumulative_uncompressed
            .checked_add(uncompressed_size)
            .ok_or_else(|| "ZIP total uncompressed size overflow.".to_owned())?;
        if cumulative_uncompressed > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(format!(
                "ZIP total uncompressed size ({cumulative_uncompressed} bytes) exceeds safety limit."
            ));
        }

        let unix_mode = if creator_os == 3 {
            external_attr >> 16
        } else {
            0
        };
        let unix_ftype = unix_mode & 0o170000;
        let dos_reparse = (creator_os == 0 || creator_os == 10) && (external_attr & 0x0400) != 0;
        let is_symlink = unix_ftype == 0o120000 || dos_reparse;
        let is_dir = name_is_dir || unix_ftype == 0o040000;

        if unix_ftype != 0
            && unix_ftype != 0o100000
            && unix_ftype != 0o040000
            && unix_ftype != 0o120000
        {
            return Err(format!(
                "ZIP entry '{}' has forbidden special file mode 0o{unix_mode:o}.",
                rel_path.display()
            ));
        }

        if is_symlink {
            if !allow_macos_bundle_symlinks {
                return Err(format!(
                    "ZIP entry '{}' is a symlink, which is forbidden in portable update archives.",
                    rel_path.display()
                ));
            }
            if uncompressed_size > 4096 {
                return Err(format!(
                    "ZIP symlink '{}' target exceeds 4096 bytes.",
                    rel_path.display()
                ));
            }
            let mut target_bytes = Vec::with_capacity(uncompressed_size as usize);
            stream_zip_entry_payload(
                bytes,
                ZipPayloadSlice {
                    method,
                    expected_crc32: crc32,
                    compressed_size,
                    uncompressed_size,
                    local_header_offset,
                    rel_path: &rel_path,
                },
                &mut target_bytes,
            )?;
            let target_str = std::str::from_utf8(&target_bytes).map_err(|_| {
                format!(
                    "Symlink '{}' target is not valid UTF-8.",
                    rel_path.display()
                )
            })?;
            verify_zip_bundle_symlink_target(&rel_path, target_str)?;
        }

        records.push(ZipEntryRecord {
            rel_path,
            is_dir,
            is_symlink,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            unix_mode,
        });
        cursor = next_cursor;
    }

    if cursor != end_usize {
        return Err("Unparsed trailing bytes inside ZIP Central Directory.".to_owned());
    }

    // Verify sorted Local File Headers start at offset 0 and span contiguously to `cd_offset`
    // with zero unreferenced `PK\x03\x04` records.
    if lfh_spans.is_empty() {
        if cd_offset != 0 {
            return Err("Rejected empty ZIP archive with unreferenced leading bytes.".to_owned());
        }
    } else {
        lfh_spans.sort_unstable_by_key(|s| s.0);
        if lfh_spans[0].0 != 0 {
            return Err(
                "Rejected ZIP archive with unreferenced leading data before first Local File Header."
                    .to_owned(),
            );
        }
        for idx in 0..lfh_spans.len() {
            let (_lfh_off, data_end, lfh_flags, crc32, comp_sz, uncomp_sz) = lfh_spans[idx];
            let next_off = if idx + 1 < lfh_spans.len() {
                lfh_spans[idx + 1].0
            } else {
                cd_offset as usize
            };
            if next_off < data_end {
                return Err(
                    "Rejected ZIP archive with overlapping Local File Header entries.".to_owned(),
                );
            }
            let gap = &bytes[data_end..next_off];
            if gap.windows(4).any(|w| w == b"PK\x03\x04") {
                return Err(
                    "Rejected ZIP archive containing an unreferenced Local File Header.".to_owned(),
                );
            }
            if (lfh_flags & 0x0008) != 0 {
                verify_zip_data_descriptor(gap, crc32, comp_sz, uncomp_sz)?;
            } else if !gap.is_empty() {
                return Err(
                    "Rejected ZIP archive with unreferenced gap bytes between Local File Headers."
                        .to_owned(),
                );
            }
        }
    }

    Ok(records)
}

fn parse_zip64_extra(
    extra: &[u8],
    need_uncomp: bool,
    need_comp: bool,
    need_lfh: bool,
    uncompressed_size: &mut u64,
    compressed_size: &mut u64,
    local_header_offset: &mut u64,
) -> Result<(), String> {
    let mut pos = 0usize;
    while pos + 4 <= extra.len() {
        let tag = u16::from_le_bytes([extra[pos], extra[pos + 1]]);
        let len = u16::from_le_bytes([extra[pos + 2], extra[pos + 3]]) as usize;
        pos += 4;
        if pos + len > extra.len() {
            return Err("Truncated extra field in ZIP central directory.".to_owned());
        }
        if tag == 0x0001 {
            let field = &extra[pos..pos + len];
            let mut fpos = 0usize;
            if need_uncomp {
                if fpos + 8 > field.len() {
                    return Err("Truncated ZIP64 uncompressed size.".to_owned());
                }
                *uncompressed_size = u64::from_le_bytes(field[fpos..fpos + 8].try_into().unwrap());
                fpos += 8;
            }
            if need_comp {
                if fpos + 8 > field.len() {
                    return Err("Truncated ZIP64 compressed size.".to_owned());
                }
                *compressed_size = u64::from_le_bytes(field[fpos..fpos + 8].try_into().unwrap());
                fpos += 8;
            }
            if need_lfh {
                if fpos + 8 > field.len() {
                    return Err("Truncated ZIP64 local header offset.".to_owned());
                }
                *local_header_offset =
                    u64::from_le_bytes(field[fpos..fpos + 8].try_into().unwrap());
            }
            return Ok(());
        }
        pos += len;
    }
    Err("Missing required ZIP64 extra field.".to_owned())
}

fn verify_zip_bundle_symlink_target(entry_rel: &Path, target_raw: &str) -> Result<(), String> {
    if target_raw.is_empty() || target_raw.starts_with('/') || target_raw.starts_with('\\') {
        return Err(format!(
            "Rejected symlink '{}' pointing to absolute or empty target '{target_raw}'.",
            entry_rel.display()
        ));
    }
    let mut comps = entry_rel.components();
    let Some(std::path::Component::Normal(first)) = comps.next() else {
        return Err(format!(
            "Unsafe symlink location '{}'.",
            entry_rel.display()
        ));
    };
    if first != "NeoFlash.app" {
        return Err(format!(
            "Rejected symlink '{}' outside NeoFlash.app.",
            entry_rel.display()
        ));
    }
    // Depth inside NeoFlash.app excluding the symlink filename itself.
    let mut parts: Vec<String> = comps
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.pop().is_none() {
        return Err("NeoFlash.app itself cannot be a symlink.".to_owned());
    }
    for seg in target_raw.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if parts.pop().is_none() {
                return Err(format!(
                    "Rejected symlink '{}' escaping NeoFlash.app via '{target_raw}'.",
                    entry_rel.display()
                ));
            }
        } else {
            parts.push(seg.to_owned());
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ZipPayloadSlice<'a> {
    method: u16,
    expected_crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    rel_path: &'a Path,
}

fn stream_zip_entry_payload<W: std::io::Write>(
    archive: &[u8],
    spec: ZipPayloadSlice<'_>,
    writer: &mut W,
) -> Result<(), String> {
    use std::io::Read as _;

    let ZipPayloadSlice {
        method,
        expected_crc32,
        compressed_size,
        uncompressed_size,
        local_header_offset,
        rel_path,
    } = spec;

    if uncompressed_size > MAX_ARCHIVE_ENTRY_BYTES as u64 {
        return Err(format!(
            "ZIP entry '{}' uncompressed size ({uncompressed_size} bytes) exceeds safety limit.",
            rel_path.display()
        ));
    }
    let lfh = usize::try_from(local_header_offset)
        .map_err(|_| "ZIP local header offset exceeds addressable size.".to_owned())?;
    if lfh + 30 > archive.len() || &archive[lfh..lfh + 4] != b"PK\x03\x04" {
        return Err(format!(
            "Corrupt ZIP local file header for '{}'.",
            rel_path.display()
        ));
    }
    let lfh_name_len = u16::from_le_bytes([archive[lfh + 26], archive[lfh + 27]]) as usize;
    let lfh_extra_len = u16::from_le_bytes([archive[lfh + 28], archive[lfh + 29]]) as usize;
    let data_start = lfh
        .checked_add(30)
        .and_then(|v| v.checked_add(lfh_name_len))
        .and_then(|v| v.checked_add(lfh_extra_len))
        .ok_or_else(|| "ZIP local file header overflow.".to_owned())?;
    let comp_len =
        usize::try_from(compressed_size).map_err(|_| "ZIP compressed size overflow.".to_owned())?;
    let data_end = data_start
        .checked_add(comp_len)
        .ok_or_else(|| "ZIP entry payload overflow.".to_owned())?;
    if data_end > archive.len() {
        return Err(format!(
            "Truncated ZIP entry payload for '{}'.",
            rel_path.display()
        ));
    }
    let comp_slice = &archive[data_start..data_end];
    let mut crc = Crc32Hasher::new();
    let mut written_total: u64 = 0;

    match method {
        0 => {
            if comp_slice.len() as u64 != uncompressed_size {
                return Err(format!(
                    "Stored ZIP entry '{}' length mismatch ({} vs {uncompressed_size}).",
                    rel_path.display(),
                    comp_slice.len()
                ));
            }
            crc.update(comp_slice);
            writer
                .write_all(comp_slice)
                .map_err(|e| format!("Could not write ZIP entry '{}': {e}", rel_path.display()))?;
            written_total = comp_slice.len() as u64;
        }
        8 => {
            if uncompressed_size == 0 && (comp_slice.is_empty() || comp_slice == [0x03, 0x00]) {
                written_total = 0;
            } else {
                let cap = (MAX_ARCHIVE_ENTRY_BYTES as u64).saturating_add(1);
                let mut decoder = DeflateStreamDecoder::new(comp_slice).take(cap);
                let mut chunk = [0u8; 64 * 1024];
                loop {
                    let n = decoder.read(&mut chunk).map_err(|e| {
                        format!(
                            "Deflate decompression failed for ZIP entry '{}': {e}",
                            rel_path.display()
                        )
                    })?;
                    if n == 0 {
                        break;
                    }
                    written_total += n as u64;
                    if written_total > MAX_ARCHIVE_ENTRY_BYTES as u64 {
                        return Err(format!(
                            "ZIP entry '{}' decompressed output exceeds {MAX_ARCHIVE_ENTRY_BYTES}-byte limit.",
                            rel_path.display()
                        ));
                    }
                    crc.update(&chunk[..n]);
                    writer.write_all(&chunk[..n]).map_err(|e| {
                        format!("Could not write ZIP entry '{}': {e}", rel_path.display())
                    })?;
                }
            }
        }
        other => {
            return Err(format!("Unsupported ZIP compression method {other}."));
        }
    }

    if written_total != uncompressed_size {
        return Err(format!(
            "ZIP entry '{}' decompressed to {written_total} bytes, expected {uncompressed_size}.",
            rel_path.display()
        ));
    }
    let actual_crc = crc.finalize();
    if actual_crc != expected_crc32 {
        return Err(format!(
            "CRC-32 mismatch for ZIP entry '{}' (expected 0x{expected_crc32:08x}, got 0x{actual_crc:08x}).",
            rel_path.display()
        ));
    }
    Ok(())
}

pub(crate) fn extract_zip_archive_safely(
    archive_path: &Path,
    dest_dir: &Path,
    allow_macos_bundle_symlinks: bool,
) -> Result<(), String> {
    let bytes = std::fs::read(archive_path).map_err(|e| {
        format!(
            "Could not read ZIP archive '{}': {e}",
            archive_path.display()
        )
    })?;
    let records = inspect_zip_safety(&bytes, allow_macos_bundle_symlinks)?;
    for entry in &records {
        let out_path = dest_dir.join(&entry.rel_path);
        if entry.is_dir {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("Could not create directory '{}': {e}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Could not create parent directory '{}': {e}",
                    parent.display()
                )
            })?;
        }
        let payload_spec = ZipPayloadSlice {
            method: entry.method,
            expected_crc32: entry.crc32,
            compressed_size: entry.compressed_size,
            uncompressed_size: entry.uncompressed_size,
            local_header_offset: entry.local_header_offset,
            rel_path: &entry.rel_path,
        };
        if entry.is_symlink {
            #[cfg(unix)]
            {
                let mut target_bytes = Vec::with_capacity(entry.uncompressed_size as usize);
                stream_zip_entry_payload(&bytes, payload_spec, &mut target_bytes)?;
                let target_str = std::str::from_utf8(&target_bytes)
                    .map_err(|_| "Invalid symlink UTF-8.".to_owned())?;
                std::os::unix::fs::symlink(target_str, &out_path).map_err(|e| {
                    format!(
                        "Could not create staged symlink '{}': {e}",
                        out_path.display()
                    )
                })?;
                continue;
            }
            #[cfg(not(unix))]
            {
                return Err(format!(
                    "Symlink entry '{}' is not supported on this platform.",
                    out_path.display()
                ));
            }
        }
        let mut out_file = std::fs::File::create(&out_path).map_err(|e| {
            format!(
                "Could not create extracted file '{}': {e}",
                out_path.display()
            )
        })?;
        stream_zip_entry_payload(&bytes, payload_spec, &mut out_file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let is_exec = (entry.unix_mode & 0o111) != 0
                || entry
                    .rel_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
            let mode = if is_exec { 0o755 } else { 0o644 };
            let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

fn parse_gzip_header_offset(gz: &[u8]) -> Result<(usize, u32, u32), String> {
    if gz.len() < 18 || gz[0] != 0x1f || gz[1] != 0x8b {
        return Err("Archive is not a valid GZIP stream (missing 1f 8b magic).".to_owned());
    }
    if gz[2] != 8 {
        return Err(format!("Unsupported GZIP compression method {}.", gz[2]));
    }
    let flags = gz[3];
    if (flags & 0b1110_0000) != 0 {
        return Err("Unsupported reserved GZIP header flags.".to_owned());
    }
    let mut pos = 10usize;
    if (flags & 0x04) != 0 {
        if pos + 2 > gz.len() {
            return Err("Truncated GZIP FEXTRA header.".to_owned());
        }
        let xlen = u16::from_le_bytes([gz[pos], gz[pos + 1]]) as usize;
        pos = pos
            .checked_add(2 + xlen)
            .ok_or_else(|| "GZIP FEXTRA overflow.".to_owned())?;
    }
    if (flags & 0x08) != 0 {
        let nul = gz
            .get(pos..)
            .and_then(|s| s.iter().position(|&b| b == 0))
            .ok_or_else(|| "Unterminated GZIP FNAME header.".to_owned())?;
        pos += nul + 1;
    }
    if (flags & 0x10) != 0 {
        let nul = gz
            .get(pos..)
            .and_then(|s| s.iter().position(|&b| b == 0))
            .ok_or_else(|| "Unterminated GZIP FCOMMENT header.".to_owned())?;
        pos += nul + 1;
    }
    if (flags & 0x02) != 0 {
        pos = pos
            .checked_add(2)
            .ok_or_else(|| "GZIP FHCRC overflow.".to_owned())?;
    }
    if pos + 8 > gz.len() {
        return Err("Truncated GZIP payload or trailer.".to_owned());
    }
    let trailer_start = gz.len() - 8;
    let expected_crc = u32::from_le_bytes(gz[trailer_start..trailer_start + 4].try_into().unwrap());
    let expected_isize = u32::from_le_bytes(gz[trailer_start + 4..].try_into().unwrap());
    Ok((pos, expected_crc, expected_isize))
}

struct GzipVerifiedReader<'a> {
    decoder: std::io::Take<DeflateStreamDecoder<&'a [u8]>>,
    max_uncompressed: u64,
    crc: Crc32Hasher,
    total_out: u64,
    expected_crc: u32,
    expected_isize: u32,
    verified_eof: bool,
}

impl<'a> GzipVerifiedReader<'a> {
    fn from_gzip_bytes(gz: &'a [u8], max_uncompressed: u64) -> Result<Self, String> {
        use std::io::Read as _;
        let (pos, expected_crc, expected_isize) = parse_gzip_header_offset(gz)?;
        let deflate_stream = &gz[pos..gz.len() - 8];
        let decoder =
            DeflateStreamDecoder::new(deflate_stream).take(max_uncompressed.saturating_add(1));
        Ok(Self {
            decoder,
            max_uncompressed,
            crc: Crc32Hasher::new(),
            total_out: 0,
            expected_crc,
            expected_isize,
            verified_eof: false,
        })
    }

    fn finish_and_verify(&mut self) -> Result<u64, String> {
        use std::io::Read as _;
        let mut drain = [0u8; 16 * 1024];
        while !self.verified_eof {
            let n = self
                .read(&mut drain)
                .map_err(|e| format!("GZIP stream verification failed: {e}"))?;
            if n == 0 {
                break;
            }
        }
        Ok(self.total_out)
    }
}

impl std::io::Read for GzipVerifiedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.verified_eof || buf.is_empty() {
            return Ok(0);
        }
        let n = self.decoder.read(buf)?;
        if n == 0 {
            let actual_crc = self.crc.state ^ 0xFFFF_FFFF;
            if actual_crc != self.expected_crc {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "GZIP CRC-32 mismatch (expected 0x{:08x}, got 0x{actual_crc:08x}).",
                        self.expected_crc
                    ),
                ));
            }
            if (self.total_out as u32) != self.expected_isize {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "GZIP uncompressed size mismatch (ISIZE expected {}, got {}).",
                        self.expected_isize, self.total_out as u32
                    ),
                ));
            }
            self.verified_eof = true;
            return Ok(0);
        }
        self.total_out += n as u64;
        if self.total_out > self.max_uncompressed {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "GZIP uncompressed output exceeds {}-byte safety limit.",
                    self.max_uncompressed
                ),
            ));
        }
        self.crc.update(&buf[..n]);
        Ok(n)
    }
}

/// Stream-decompress a GZIP buffer into `sink`, enforcing `max_uncompressed` (up to >1.5 GiB)
/// and verifying CRC-32 + ISIZE without buffering the full uncompressed payload in memory.
#[allow(dead_code)]
pub(crate) fn decompress_gzip_stream_bounded<W: std::io::Write>(
    gz: &[u8],
    max_uncompressed: u64,
    mut sink: W,
) -> Result<u64, String> {
    use std::io::Read as _;
    let mut reader = GzipVerifiedReader::from_gzip_bytes(gz, max_uncompressed)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("GZIP DEFLATE decompression failed: {e}"))?;
        if n == 0 {
            break;
        }
        sink.write_all(&buf[..n])
            .map_err(|e| format!("Failed writing decompressed GZIP stream: {e}"))?;
    }
    reader.finish_and_verify()
}

pub(crate) fn extract_tar_gz_safely(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let gz_bytes = std::fs::read(archive_path).map_err(|e| {
        format!(
            "Could not read .tar.gz archive '{}': {e}",
            archive_path.display()
        )
    })?;
    let mut reader = GzipVerifiedReader::from_gzip_bytes(&gz_bytes, MAX_ARCHIVE_TOTAL_BYTES)?;
    extract_tar_reader_safely(&mut reader, dest_dir)?;
    reader.finish_and_verify()?;
    Ok(())
}

fn read_exact_or_eof<R: std::io::Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, String> {
    let mut pos = 0usize;
    while pos < buf.len() {
        let n = reader
            .read(&mut buf[pos..])
            .map_err(|e| format!("TAR stream read failed: {e}"))?;
        if n == 0 {
            if pos == 0 {
                return Ok(false);
            }
            return Err("Invalid TAR stream: truncated 512-byte block.".to_owned());
        }
        pos += n;
    }
    Ok(true)
}

fn discard_exact<R: std::io::Read>(reader: &mut R, mut remaining: u64) -> Result<(), String> {
    let mut scratch = [0u8; 8192];
    while remaining > 0 {
        let want = (remaining as usize).min(scratch.len());
        let n = reader
            .read(&mut scratch[..want])
            .map_err(|e| format!("TAR stream read failed: {e}"))?;
        if n == 0 {
            return Err("Truncated TAR entry padding.".to_owned());
        }
        remaining -= n as u64;
    }
    Ok(())
}

fn extract_tar_reader_safely<R: std::io::Read>(
    reader: &mut R,
    dest_dir: &Path,
) -> Result<(), String> {
    let mut entries_seen = 0usize;
    let mut pending_long_path: Option<String> = None;
    let mut cumulative_payload: u64 = 0;
    let mut block = [0u8; 512];

    while read_exact_or_eof(reader, &mut block)? {
        if block.iter().all(|&b| b == 0) {
            break;
        }
        entries_seen += 1;
        if entries_seen > MAX_ARCHIVE_ENTRIES {
            return Err("TAR archive exceeds maximum allowed entry count.".to_owned());
        }

        verify_tar_header_checksum(&block)?;
        #[cfg_attr(not(unix), allow(unused_variables))]
        let mode = parse_tar_octal(&block[100..108])? as u32;
        let size = parse_tar_octal(&block[124..136])?;
        let typeflag = block[156];

        if size > MAX_ARCHIVE_ENTRY_BYTES as u64 {
            return Err(format!(
                "TAR entry size ({size} bytes) exceeds {MAX_ARCHIVE_ENTRY_BYTES}-byte limit."
            ));
        }
        cumulative_payload = cumulative_payload
            .checked_add(size)
            .ok_or_else(|| "TAR cumulative payload overflow.".to_owned())?;
        if cumulative_payload > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(format!(
                "TAR cumulative payload ({cumulative_payload} bytes) exceeds safety limit."
            ));
        }
        let pad_bytes = (512 - (size % 512)) % 512;

        match typeflag {
            b'L' => {
                if size > 16 * 1024 {
                    return Err("GNU LongLink path exceeds 16 KiB limit.".to_owned());
                }
                let mut payload = vec![0u8; size as usize];
                reader
                    .read_exact(&mut payload)
                    .map_err(|e| format!("Truncated GNU LongLink TAR payload: {e}"))?;
                discard_exact(reader, pad_bytes)?;
                let raw = std::str::from_utf8(&payload)
                    .map_err(|_| "GNU LongLink path is not valid UTF-8.".to_owned())?
                    .trim_end_matches('\0')
                    .to_owned();
                pending_long_path = Some(raw);
                continue;
            }
            b'x' => {
                if size > 64 * 1024 {
                    return Err("PAX extended header exceeds 64 KiB limit.".to_owned());
                }
                let mut payload = vec![0u8; size as usize];
                reader
                    .read_exact(&mut payload)
                    .map_err(|e| format!("Truncated PAX TAR header: {e}"))?;
                discard_exact(reader, pad_bytes)?;
                if let Some(pax_path) = parse_pax_path(&payload)? {
                    pending_long_path = Some(pax_path);
                }
                continue;
            }
            b'g' => {
                discard_exact(reader, size + pad_bytes)?;
                continue;
            }
            b'1' | b'2' | b'K' => {
                return Err(
                    "TAR archive contains a forbidden symlink or hardlink entry.".to_owned(),
                );
            }
            b'3' | b'4' | b'6' => {
                return Err("TAR archive contains a forbidden device or FIFO node.".to_owned());
            }
            b'0' | 0 | b'5' => {}
            other => {
                return Err(format!("Unsupported TAR entry typeflag 0x{other:02x}."));
            }
        }

        let raw_path = if let Some(override_path) = pending_long_path.take() {
            override_path
        } else {
            parse_ustar_path(&block)?
        };
        let trimmed_root = raw_path.trim_end_matches('/');
        if trimmed_root == "." || trimmed_root.is_empty() {
            discard_exact(reader, size + pad_bytes)?;
            continue;
        }
        let clean_raw = raw_path.strip_prefix("./").unwrap_or(&raw_path);
        let (rel_path, path_is_dir) = validate_archive_entry_rel_path(clean_raw)?;
        let is_dir = typeflag == b'5' || path_is_dir;
        let out_path = dest_dir.join(&rel_path);

        if is_dir {
            discard_exact(reader, size + pad_bytes)?;
            std::fs::create_dir_all(&out_path).map_err(|e| {
                format!(
                    "Could not create TAR directory '{}': {e}",
                    out_path.display()
                )
            })?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "Could not create parent directory '{}': {e}",
                        parent.display()
                    )
                })?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| format!("Could not create TAR file '{}': {e}", out_path.display()))?;
            let mut rem = size;
            let mut chunk = [0u8; 64 * 1024];
            while rem > 0 {
                let want = (rem as usize).min(chunk.len());
                let n = reader
                    .read(&mut chunk[..want])
                    .map_err(|e| format!("Truncated TAR entry payload: {e}"))?;
                if n == 0 {
                    return Err("Truncated TAR entry payload.".to_owned());
                }
                use std::io::Write as _;
                out_file.write_all(&chunk[..n]).map_err(|e| {
                    format!("Could not write TAR file '{}': {e}", out_path.display())
                })?;
                rem -= n as u64;
            }
            discard_exact(reader, pad_bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let file_mode = if (mode & 0o111) != 0 { 0o755 } else { 0o644 };
                let _ =
                    std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(file_mode));
            }
        }
    }
    Ok(())
}

fn verify_tar_header_checksum(block: &[u8]) -> Result<(), String> {
    let expected = parse_tar_octal(&block[148..156])? as u32;
    let mut unsigned_sum: u32 = 0;
    for (idx, &b) in block.iter().enumerate() {
        unsigned_sum += if (148..156).contains(&idx) {
            32
        } else {
            u32::from(b)
        };
    }
    if unsigned_sum != expected {
        return Err(format!(
            "Corrupt TAR header checksum (expected {expected}, got {unsigned_sum})."
        ));
    }
    Ok(())
}

fn parse_tar_octal(field: &[u8]) -> Result<u64, String> {
    let trimmed_nul = match field.iter().position(|&b| b == 0) {
        Some(pos) => &field[..pos],
        None => field,
    };
    let s = std::str::from_utf8(trimmed_nul)
        .map_err(|_| "Invalid non-ASCII TAR numeric field.".to_owned())?
        .trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| format!("Invalid octal TAR field '{s}'."))
}

fn parse_ustar_path(block: &[u8]) -> Result<String, String> {
    let name_end = block[0..100].iter().position(|&b| b == 0).unwrap_or(100);
    let name = std::str::from_utf8(&block[0..name_end])
        .map_err(|_| "TAR entry name is not valid UTF-8.".to_owned())?;
    let is_ustar = &block[257..262] == b"ustar";
    if is_ustar {
        let prefix_end = block[345..500].iter().position(|&b| b == 0).unwrap_or(155);
        if prefix_end > 0 {
            let prefix = std::str::from_utf8(&block[345..345 + prefix_end])
                .map_err(|_| "TAR USTAR prefix is not valid UTF-8.".to_owned())?;
            return Ok(format!("{prefix}/{name}"));
        }
    }
    Ok(name.to_owned())
}

fn parse_pax_path(payload: &[u8]) -> Result<Option<String>, String> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| "PAX extended header is not valid UTF-8.".to_owned())?;
    let mut rest = text;
    while !rest.is_empty() {
        let Some((len_str, _)) = rest.split_once(' ') else {
            break;
        };
        let rec_len: usize = len_str
            .parse()
            .map_err(|_| "Malformed PAX record length.".to_owned())?;
        if rec_len <= len_str.len() + 1 || rec_len > rest.len() {
            return Err("Out-of-bounds PAX extended header record.".to_owned());
        }
        let body = &rest[len_str.len() + 1..rec_len];
        let body = body.strip_suffix('\n').unwrap_or(body);
        if let Some(val) = body.strip_prefix("path=") {
            return Ok(Some(val.to_owned()));
        }
        rest = &rest[rec_len..];
    }
    Ok(None)
}

pub(crate) fn rollback_path_for_target(
    target_path: &Path,
    pid_tag: u32,
) -> Result<PathBuf, String> {
    let parent = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            format!(
                "Target '{}' has no parent directory.",
                target_path.display()
            )
        })?;
    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("Target '{}' has invalid filename.", target_path.display()))?;
    Ok(parent.join(format!("{file_name}.rollback-{pid_tag}")))
}

fn remove_path_any(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if meta.is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn copy_payload_same_fs(src: &Path, dst: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(src)
        .map_err(|e| format!("Staged payload '{}' missing: {e}", src.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Refusing to copy symlink root '{}'.",
            src.display()
        ));
    }
    if meta.is_file() {
        std::fs::copy(src, dst)
            .map_err(|e| format!("Failed copying staged binary to '{}': {e}", dst.display()))?;
        #[cfg(unix)]
        {
            let _ = std::fs::set_permissions(dst, meta.permissions());
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/ditto")
            .arg(src)
            .arg(dst)
            .output()
            .map_err(|e| format!("Could not execute ditto for cross-volume staging: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ditto failed copying staged bundle: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        copy_dir_recursive_no_escape(src, dst)
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_dir_recursive_no_escape(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Could not create '{}': {e}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("Could not read '{}': {e}", src.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Directory iteration error: {e}"))?;
        let s_path = entry.path();
        let d_path = dst.join(entry.file_name());
        let ft = entry
            .file_type()
            .map_err(|e| format!("File type error on '{}': {e}", s_path.display()))?;
        if ft.is_symlink() {
            return Err(format!(
                "Refusing unexpected symlink '{}' during swap.",
                s_path.display()
            ));
        }
        if ft.is_dir() {
            copy_dir_recursive_no_escape(&s_path, &d_path)?;
        } else {
            std::fs::copy(&s_path, &d_path)
                .map_err(|e| format!("Could not copy '{}': {e}", s_path.display()))?;
            #[cfg(unix)]
            if let Ok(meta) = std::fs::metadata(&s_path) {
                let _ = std::fs::set_permissions(&d_path, meta.permissions());
            }
        }
    }
    Ok(())
}

fn move_staged_into_target(
    staged_payload: &Path,
    target_path: &Path,
    pid_tag: u32,
) -> Result<(), String> {
    if std::fs::symlink_metadata(staged_payload).is_err() {
        return Err(format!(
            "Staged payload '{}' does not exist.",
            staged_payload.display()
        ));
    }
    match std::fs::rename(staged_payload, target_path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let is_cross_device = err.raw_os_error() == Some(18) || err.raw_os_error() == Some(17);
            if !is_cross_device {
                return Err(format!(
                    "Could not move staged update '{}' into '{}': {err}",
                    staged_payload.display(),
                    target_path.display()
                ));
            }
            let parent = target_path.parent().ok_or_else(|| {
                format!(
                    "Target '{}' lacks a parent directory.",
                    target_path.display()
                )
            })?;
            let name = target_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("NeoFlash");
            let incoming = parent.join(format!(".{name}.incoming-{pid_tag}"));
            let _ = remove_path_any(&incoming);
            if let Err(copy_err) = copy_payload_same_fs(staged_payload, &incoming) {
                let _ = remove_path_any(&incoming);
                return Err(copy_err);
            }
            std::fs::rename(&incoming, target_path).map_err(|ren_err| {
                let _ = remove_path_any(&incoming);
                format!(
                    "Could not activate incoming bundle at '{}': {ren_err}",
                    target_path.display()
                )
            })
        }
    }
}

/// Synchronously swap `staged_payload` into `target_path` with `.rollback-<pid>` safety (`T-501`).
///
/// Moves existing `target_path` -> `<target>.rollback-<pid>`, moves `staged_payload` -> `target_path`,
/// removes `.rollback-<pid>` on success, or restores `<target>.rollback-<pid>` -> `target_path` if
/// activating the new bundle fails. Never touches user Application Support / `%APPDATA%` / XDG data.
pub(crate) fn apply_staged_bundle_swap(
    staged_payload: &Path,
    target_path: &Path,
    pid_tag: u32,
) -> Result<(), String> {
    let rollback_path = rollback_path_for_target(target_path, pid_tag)?;
    let had_existing = std::fs::symlink_metadata(target_path).is_ok();
    if had_existing && std::fs::symlink_metadata(&rollback_path).is_ok() {
        remove_path_any(&rollback_path).map_err(|e| {
            format!(
                "Could not clear stale rollback path '{}': {e}",
                rollback_path.display()
            )
        })?;
    }

    if had_existing {
        std::fs::rename(target_path, &rollback_path).map_err(|e| {
            format!(
                "Could not stage rollback backup '{}': {e}",
                rollback_path.display()
            )
        })?;
    }

    if let Err(swap_err) = move_staged_into_target(staged_payload, target_path, pid_tag) {
        let _ = remove_path_any(target_path);
        if had_existing && let Err(restore_err) = std::fs::rename(&rollback_path, target_path) {
            return Err(format!(
                "{swap_err}; CRITICAL: rollback from '{}' also failed: {restore_err}",
                rollback_path.display()
            ));
        }
        return Err(format!(
            "{swap_err} (existing installation restored from rollback backup)."
        ));
    }

    if had_existing {
        let _ = remove_path_any(&rollback_path);
    }
    Ok(())
}

fn sh_single_quote(raw: &Path) -> String {
    let s = raw.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn ps_single_quote(raw: &Path) -> String {
    let s = raw.to_string_lossy();
    format!("'{}'", s.replace('\'', "''"))
}

pub(crate) fn build_macos_swap_script(
    staged_app: &Path,
    target_app: &Path,
    staging_root: &Path,
    wait_pid: u32,
    relaunch: bool,
) -> String {
    let rollback = rollback_path_for_target(target_app, wait_pid)
        .unwrap_or_else(|_| target_app.with_extension(format!("app.rollback-{wait_pid}")));
    let relaunch_flag = if relaunch { "1" } else { "0" };
    format!(
        "#!/bin/sh\n\
         set -eu\n\
         WAIT_PID={wait_pid}\n\
         STAGED={staged}\n\
         TARGET={target}\n\
         ROLLBACK={rollback}\n\
         STAGE_ROOT={stage_root}\n\
         RELAUNCH={relaunch_flag}\n\
         cleanup() {{ rm -rf \"$STAGE_ROOT\" 2>/dev/null || true; }}\n\
         trap cleanup EXIT\n\
         elapsed=0\n\
         while kill -0 \"$WAIT_PID\" 2>/dev/null; do\n\
           sleep 0.1\n\
           elapsed=$((elapsed + 1))\n\
           if [ \"$elapsed\" -ge 300 ]; then\n\
             exit 1\n\
           fi\n\
         done\n\
         had_existing=0\n\
         if [ -e \"$TARGET\" ]; then\n\
           had_existing=1\n\
           rm -rf \"$ROLLBACK\" 2>/dev/null || true\n\
           mv \"$TARGET\" \"$ROLLBACK\"\n\
         fi\n\
         if ! mv \"$STAGED\" \"$TARGET\" 2>/dev/null; then\n\
           if ! /usr/bin/ditto \"$STAGED\" \"$TARGET\" 2>/dev/null; then\n\
             rm -rf \"$TARGET\" 2>/dev/null || true\n\
             if [ \"$had_existing\" -eq 1 ]; then\n\
               mv \"$ROLLBACK\" \"$TARGET\"\n\
             fi\n\
             exit 1\n\
           fi\n\
         fi\n\
         rm -rf \"$ROLLBACK\" 2>/dev/null || true\n\
         if [ \"$RELAUNCH\" = \"1\" ]; then\n\
           /usr/bin/open \"$TARGET\" >/dev/null 2>&1 || true\n\
         fi\n",
        staged = sh_single_quote(staged_app),
        target = sh_single_quote(target_app),
        rollback = sh_single_quote(&rollback),
        stage_root = sh_single_quote(staging_root),
    )
}

pub(crate) fn build_windows_swap_script(
    staged_payload: &Path,
    target_dir: &Path,
    staging_root: &Path,
    wait_pid: u32,
    relaunch: bool,
) -> String {
    let rollback = rollback_path_for_target(target_dir, wait_pid)
        .unwrap_or_else(|_| target_dir.with_extension(format!("rollback-{wait_pid}")));
    let incoming = target_dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .and_then(|parent| {
            target_dir
                .file_name()
                .and_then(|s| s.to_str())
                .map(|name| parent.join(format!("{name}.incoming-{wait_pid}")))
        })
        .unwrap_or_else(|| target_dir.with_extension(format!("incoming-{wait_pid}")));
    let relaunch_bool = if relaunch { "$true" } else { "$false" };
    format!(
        "Set-StrictMode -Version Latest\n\
         $ErrorActionPreference = 'Stop'\n\
         $WaitPid = {wait_pid}\n\
         $Staged = {staged}\n\
         $Target = {target}\n\
         $Incoming = {incoming}\n\
         $Rollback = {rollback}\n\
         $StageRoot = {stage_root}\n\
         $Relaunch = {relaunch_bool}\n\
         try {{\n\
             $proc = Get-Process -Id $WaitPid -ErrorAction SilentlyContinue\n\
             if ($null -ne $proc) {{\n\
                 if (-not $proc.WaitForExit(30000)) {{\n\
                     exit 1\n\
                 }}\n\
             }}\n\
             if (Test-Path -LiteralPath $Incoming) {{\n\
                 Remove-Item -LiteralPath $Incoming -Recurse -Force\n\
             }}\n\
             Copy-Item -LiteralPath $Staged -Destination $Incoming -Recurse -Force\n\
             if (-not (Test-Path -LiteralPath (Join-Path $Incoming 'neoflash-editor.exe') -PathType Leaf)) {{\n\
                 Remove-Item -LiteralPath $Incoming -Recurse -Force -ErrorAction SilentlyContinue\n\
                 throw 'Cross-volume staging failed: missing neoflash-editor.exe'\n\
             }}\n\
             $hadExisting = Test-Path -LiteralPath $Target\n\
             if ($hadExisting) {{\n\
                 if (Test-Path -LiteralPath $Rollback) {{\n\
                     Remove-Item -LiteralPath $Rollback -Recurse -Force\n\
                 }}\n\
                 Move-Item -LiteralPath $Target -Destination $Rollback -Force\n\
             }}\n\
             try {{\n\
                 Move-Item -LiteralPath $Incoming -Destination $Target -Force\n\
             }} catch {{\n\
                 Remove-Item -LiteralPath $Incoming -Recurse -Force -ErrorAction SilentlyContinue\n\
                 if (Test-Path -LiteralPath $Target) {{\n\
                     Remove-Item -LiteralPath $Target -Recurse -Force -ErrorAction SilentlyContinue\n\
                 }}\n\
                 if ($hadExisting -and (Test-Path -LiteralPath $Rollback)) {{\n\
                     Move-Item -LiteralPath $Rollback -Destination $Target -Force\n\
                 }}\n\
                 throw\n\
             }}\n\
             if ($hadExisting -and (Test-Path -LiteralPath $Rollback)) {{\n\
                 Remove-Item -LiteralPath $Rollback -Recurse -Force -ErrorAction SilentlyContinue\n\
             }}\n\
             if ($Relaunch) {{\n\
                 $exe = Join-Path $Target 'neoflash-editor.exe'\n\
                 Start-Process -FilePath $exe -WorkingDirectory $Target | Out-Null\n\
             }}\n\
         }} finally {{\n\
             Remove-Item -LiteralPath $StageRoot -Recurse -Force -ErrorAction SilentlyContinue\n\
         }}\n",
        staged = ps_single_quote(staged_payload),
        target = ps_single_quote(target_dir),
        incoming = ps_single_quote(&incoming),
        rollback = ps_single_quote(&rollback),
        stage_root = ps_single_quote(staging_root),
    )
}

pub(crate) fn build_linux_swap_script(
    staged_payload: &Path,
    target_path: &Path,
    relaunch_exe: &Path,
    staging_root: &Path,
    wait_pid: u32,
    relaunch: bool,
) -> String {
    let rollback = rollback_path_for_target(target_path, wait_pid)
        .unwrap_or_else(|_| target_path.with_extension(format!("rollback-{wait_pid}")));
    let relaunch_flag = if relaunch { "1" } else { "0" };
    format!(
        "#!/bin/sh\n\
         set -eu\n\
         WAIT_PID={wait_pid}\n\
         STAGED={staged}\n\
         TARGET={target}\n\
         ROLLBACK={rollback}\n\
         RELAUNCH_EXE={relaunch_exe}\n\
         STAGE_ROOT={stage_root}\n\
         RELAUNCH={relaunch_flag}\n\
         cleanup() {{ rm -rf \"$STAGE_ROOT\" 2>/dev/null || true; }}\n\
         trap cleanup EXIT\n\
         elapsed=0\n\
         while kill -0 \"$WAIT_PID\" 2>/dev/null; do\n\
           sleep 0.1\n\
           elapsed=$((elapsed + 1))\n\
           if [ \"$elapsed\" -ge 300 ]; then\n\
             exit 1\n\
           fi\n\
         done\n\
         had_existing=0\n\
         if [ -e \"$TARGET\" ]; then\n\
           had_existing=1\n\
           rm -rf \"$ROLLBACK\" 2>/dev/null || true\n\
           mv \"$TARGET\" \"$ROLLBACK\"\n\
         fi\n\
         if ! mv \"$STAGED\" \"$TARGET\" 2>/dev/null; then\n\
           if ! cp -a \"$STAGED\" \"$TARGET\" 2>/dev/null; then\n\
             rm -rf \"$TARGET\" 2>/dev/null || true\n\
             if [ \"$had_existing\" -eq 1 ]; then\n\
               mv \"$ROLLBACK\" \"$TARGET\"\n\
             fi\n\
             exit 1\n\
           fi\n\
         fi\n\
         rm -rf \"$ROLLBACK\" 2>/dev/null || true\n\
         if [ \"$RELAUNCH\" = \"1\" ]; then\n\
           \"$RELAUNCH_EXE\" >/dev/null 2>&1 &\n\
         fi\n",
        staged = sh_single_quote(staged_payload),
        target = sh_single_quote(target_path),
        rollback = sh_single_quote(&rollback),
        relaunch_exe = sh_single_quote(relaunch_exe),
        stage_root = sh_single_quote(staging_root),
    )
}

fn write_executable_script(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|e| format!("Could not write helper script '{}': {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            format!(
                "Could not mark helper script '{}' executable: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Spawn a detached OS swap-after-PID-exit helper and transfer staging cleanup ownership to it.
pub(crate) fn spawn_atomic_replace_and_relaunch(
    prepared: &mut PreparedSelfUpdate,
    wait_pid: u32,
    relaunch: bool,
) -> Result<PathBuf, String> {
    use std::process::{Command, Stdio};

    let script_path = match prepared.platform {
        PlatformTarget::MacosArm64 | PlatformTarget::MacosX64 => {
            let path = prepared.staging_root.join("swap-and-relaunch.sh");
            write_executable_script(
                &path,
                &build_macos_swap_script(
                    &prepared.staged_payload_path,
                    &prepared.target_install_path,
                    &prepared.staging_root,
                    wait_pid,
                    relaunch,
                ),
            )?;
            let mut cmd = Command::new("/bin/sh");
            cmd.arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            cmd.spawn()
                .map_err(|e| format!("Could not launch macOS update helper: {e}"))?;
            path
        }
        PlatformTarget::WindowsX64 => {
            let path = prepared.staging_root.join("swap-and-relaunch.ps1");
            std::fs::write(
                &path,
                build_windows_swap_script(
                    &prepared.staged_payload_path,
                    &prepared.target_install_path,
                    &prepared.staging_root,
                    wait_pid,
                    relaunch,
                ),
            )
            .map_err(|e| format!("Could not write Windows update helper: {e}"))?;
            let mut cmd = Command::new("powershell.exe");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                const DETACHED_PROCESS: u32 = 0x0000_0008;
                cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
            }
            cmd.spawn()
                .map_err(|e| format!("Could not launch Windows update helper: {e}"))?;
            path
        }
        PlatformTarget::LinuxX64 => {
            let relaunch_exe = if prepared.relaunch_executable_rel.as_os_str().is_empty() {
                prepared.target_install_path.clone()
            } else {
                prepared
                    .target_install_path
                    .join(&prepared.relaunch_executable_rel)
            };
            let path = prepared.staging_root.join("swap-and-relaunch.sh");
            write_executable_script(
                &path,
                &build_linux_swap_script(
                    &prepared.staged_payload_path,
                    &prepared.target_install_path,
                    &relaunch_exe,
                    &prepared.staging_root,
                    wait_pid,
                    relaunch,
                ),
            )?;
            let mut cmd = Command::new("/bin/sh");
            cmd.arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            cmd.spawn()
                .map_err(|e| format!("Could not launch Linux update helper: {e}"))?;
            path
        }
        PlatformTarget::Unsupported => {
            return Err("Unsupported platform for self-update.".to_owned());
        }
    };

    prepared.helper_script_path = Some(script_path.clone());
    prepared.handed_off_to_helper = true;
    Ok(script_path)
}
